//! Translate authenticated GameStream controller 0 into the existing local
//! nxnative input receiver protocol. No global keyboard or mouse injection.
use moonshine_core::session::stream::control::host_input::InputReceiver;
use std::{io, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct State {
    buttons: u16,
    axes: [i16; 4],
}
fn parse(p: &[u8]) -> Option<State> {
    if p.len() < 24 || !matches!(u32::from_le_bytes(p[..4].try_into().ok()?), 0x0c | 0x0d) {
        return None;
    }
    if u16::from_le_bytes(p[6..8].try_into().ok()?) != 0 {
        return None;
    }
    if u16::from_le_bytes(p[8..10].try_into().ok()?) & 1 == 0 {
        return Some(State::default());
    }
    let flags = u16::from_le_bytes(p[12..14].try_into().ok()?);
    let sources = [
        0x1000, 0x2000, 0x4000, 0x8000, 0x0040, 0x0080, 0x0100, 0x0200, 0x0010, 0x0020, 0x0004,
        0x0001, 0x0008, 0x0002,
    ];
    let dest = [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15];
    let mut state = State::default();
    for (source, bit) in sources.into_iter().zip(dest) {
        if flags & source != 0 {
            state.buttons |= 1 << bit;
        }
    }
    if p[14] >= 128 {
        state.buttons |= 1 << 8;
    }
    if p[15] >= 128 {
        state.buttons |= 1 << 9;
    }
    for (i, axis) in state.axes.iter_mut().enumerate() {
        *axis = i16::from_le_bytes(p[16 + i * 2..18 + i * 2].try_into().ok()?);
    }
    Some(state)
}
fn packet(seq: u64, state: State) -> [u8; 22] {
    let mut p = [0; 22];
    p[..4].copy_from_slice(b"NXI1");
    p[4..12].copy_from_slice(&seq.to_be_bytes());
    p[12..14].copy_from_slice(&state.buttons.to_be_bytes());
    for (i, x) in state.axes.iter().enumerate() {
        p[14 + i * 2..16 + i * 2].copy_from_slice(&x.to_be_bytes());
    }
    p
}
fn reset_packet(seq: u64) -> [u8; 22] {
    let mut p = packet(seq, State::default());
    p[..4].copy_from_slice(b"NXIR");
    p
}
/// Forward one session's input to its local receiver. A broken local socket is
/// retried for a bounded time; queued states are discarded and a fresh neutral
/// is required before any new held input can reach the game.
pub async fn forward(input: InputReceiver, port: u16, token: [u8; 32]) -> io::Result<()> {
    forward_inner(input, port, token, true).await
}

// The synthetic observer currently consumes a single connection. It retains the
// same packet / overflow handling but does not opt into game-side reconnection.
pub async fn forward_single(input: InputReceiver, port: u16, token: [u8; 32]) -> io::Result<()> {
    forward_inner(input, port, token, false).await
}

async fn forward_inner(
    input: InputReceiver,
    port: u16,
    token: [u8; 32],
    reconnect: bool,
) -> io::Result<()> {
    let mut rx = input.events;
    let mut overflow = input.overflow;
    let mut overflow_open = true;
    let mut ever_connected = false;
    let mut recovery_deadline = None;
    loop {
        let deadline = recovery_deadline
            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(20));
        let mut stream = loop {
            if rx.is_closed() {
                return Ok(());
            }
            match TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
                Ok(s) => break s,
                Err(e) if tokio::time::Instant::now() >= deadline => return Err(e),
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        };
        // Events buffered before this connection was established are stale,
        // including neutral and press transitions that occurred while offline.
        while rx.try_recv().is_ok() {}
        let mut epoch = *overflow.borrow_and_update();
        let mut quarantine = ever_connected || epoch != 0;
        let mut force_reset = quarantine;
        let mut current = State::default();
        let mut seq = 0u64;
        let mut heartbeat = tokio::time::interval(Duration::from_millis(200));
        let auth = tokio::time::timeout(Duration::from_millis(500), stream.write_all(&token)).await;
        if !matches!(auth, Ok(Ok(()))) {
            if !reconnect {
                return Err(io::Error::other("local input authentication write failed"));
            }
            ever_connected = true;
            let deadline = *recovery_deadline
                .get_or_insert_with(|| tokio::time::Instant::now() + Duration::from_secs(5));
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::other("local input recovery timed out"));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        ever_connected = true;
        let connected_at = tokio::time::Instant::now();
        loop {
            tokio::select! {
                biased;
                notice = overflow.changed(), if overflow_open => match notice {
                    Ok(()) => { epoch = *overflow.borrow_and_update(); quarantine = true; current = State::default(); force_reset = true; },
                    Err(_) => { overflow_open = false; continue; },
                },
                event = rx.recv() => match event {
                    Some(p) => {
                        let latest = *overflow.borrow_and_update();
                        let changed = latest > epoch || p.epoch > epoch;
                        if changed { epoch = epoch.max(latest).max(p.epoch); quarantine = true; current = State::default(); force_reset = true; }
                        if p.epoch != epoch {
                            if !changed { continue; }
                        } else if let Some(s) = parse(&p.payload) {
                            if quarantine { if s == State::default() { quarantine = false; } }
                            else { current = s; }
                        }
                    },
                    None => {
                        seq += 1;
                        let _ = tokio::time::timeout(Duration::from_millis(500), stream.write_all(&reset_packet(seq))).await;
                        return Ok(());
                    },
                },
                _ = heartbeat.tick() => {},
            }
            seq += 1;
            let outgoing = if force_reset {
                force_reset = false;
                reset_packet(seq)
            } else {
                packet(seq, current)
            };
            match tokio::time::timeout(Duration::from_millis(500), stream.write_all(&outgoing))
                .await
            {
                Ok(Ok(())) => {
                    // Do not reset the retry budget for connections that accept
                    // and immediately close; a stable heartbeat can reset it.
                    if connected_at.elapsed() >= Duration::from_secs(1) {
                        recovery_deadline = None;
                    }
                }
                outcome => {
                    let error = match outcome {
                        Ok(Err(e)) => e,
                        Err(_) => {
                            io::Error::new(io::ErrorKind::TimedOut, "local input write timed out")
                        }
                        Ok(Ok(())) => unreachable!(),
                    };
                    if !reconnect {
                        return Err(error);
                    }
                    let deadline = *recovery_deadline.get_or_insert_with(|| {
                        tokio::time::Instant::now() + Duration::from_secs(5)
                    });
                    if tokio::time::Instant::now() >= deadline {
                        return Err(error);
                    }
                    eprintln!("local input connection lost; attempting a fresh, neutral-gated connection: {error}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overflow_neutralizes_and_requires_a_new_neutral_before_input() {
        use moonshine_core::session::stream::control::host_input::{InputPacket, InputReceiver};
        use tokio::{
            io::AsyncReadExt,
            net::TcpListener,
            sync::{mpsc, watch},
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, events) = mpsc::channel(8);
        let (reset, overflow) = watch::channel(0u64);
        let task = tokio::spawn(forward(InputReceiver { events, overflow }, port, [7u8; 32]));
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut token = [0; 32];
        socket.read_exact(&mut token).await.unwrap();
        assert_eq!(token, [7u8; 32]);
        async fn frame(s: &mut TcpStream) -> ([u8; 4], u16) {
            let mut p = [0; 22];
            tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut p))
                .await
                .unwrap()
                .unwrap();
            (
                p[..4].try_into().unwrap(),
                u16::from_be_bytes([p[12], p[13]]),
            )
        }
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 0));
        let mut press = vec![0; 24];
        press[..4].copy_from_slice(&13u32.to_le_bytes());
        press[8] = 1;
        press[12..14].copy_from_slice(&0x1000u16.to_le_bytes());
        let mut neutral = press.clone();
        neutral[12..14].copy_from_slice(&0u16.to_le_bytes());
        tx.send(InputPacket {
            epoch: 0,
            payload: press.clone(),
        })
        .await
        .unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 1));
        reset.send(1).unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXIR", 0));
        tx.send(InputPacket {
            epoch: 0,
            payload: press.clone(),
        })
        .await
        .unwrap();
        tx.send(InputPacket {
            epoch: 1,
            payload: press.clone(),
        })
        .await
        .unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 0));
        tx.send(InputPacket {
            epoch: 1,
            payload: neutral,
        })
        .await
        .unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 0));
        tx.send(InputPacket {
            epoch: 1,
            payload: press,
        })
        .await
        .unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 1));
        drop(tx);
        drop(reset);
        assert_eq!(frame(&mut socket).await, (*b"NXIR", 0));
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn reconnect_discards_offline_events_and_requires_fresh_neutral() {
        use moonshine_core::session::stream::control::host_input::{InputPacket, InputReceiver};
        use tokio::{
            io::AsyncReadExt,
            net::TcpListener,
            sync::{mpsc, watch},
        };
        async fn frame(s: &mut TcpStream) -> ([u8; 4], u16) {
            let mut p = [0; 22];
            tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut p))
                .await
                .unwrap()
                .unwrap();
            (
                p[..4].try_into().unwrap(),
                u16::from_be_bytes([p[12], p[13]]),
            )
        }
        async fn accept(listener: &TcpListener) -> TcpStream {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut token = [0u8; 32];
            socket.read_exact(&mut token).await.unwrap();
            assert_eq!(token, [8u8; 32]);
            socket
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, events) = mpsc::channel(16);
        let (epoch_tx, overflow) = watch::channel(0u64);
        let task = tokio::spawn(forward(InputReceiver { events, overflow }, port, [8u8; 32]));
        let mut socket = accept(&listener).await;
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 0));
        let mut press = vec![0; 24];
        press[..4].copy_from_slice(&13u32.to_le_bytes());
        press[8] = 1;
        press[12..14].copy_from_slice(&0x1000u16.to_le_bytes());
        let mut neutral = press.clone();
        neutral[12..14].copy_from_slice(&0u16.to_le_bytes());
        tx.send(InputPacket {
            epoch: 0,
            payload: press.clone(),
        })
        .await
        .unwrap();
        assert_eq!(frame(&mut socket).await, (*b"NXI1", 1));
        drop(socket);
        drop(listener);
        // Allow the heartbeat to detect the closed socket while no listener is available.
        tokio::time::sleep(Duration::from_millis(800)).await;
        for payload in [press.clone(), neutral.clone(), press.clone()] {
            tx.send(InputPacket { epoch: 0, payload }).await.unwrap();
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut socket = accept(&listener).await;
        assert_eq!(frame(&mut socket).await, (*b"NXIR", 0));
        // A held state after reconnect is still suppressed until an explicit neutral.
        tx.send(InputPacket {
            epoch: 0,
            payload: press.clone(),
        })
        .await
        .unwrap();
        let until = tokio::time::Instant::now() + Duration::from_millis(350);
        while tokio::time::Instant::now() < until {
            let (_, buttons) = frame(&mut socket).await;
            assert_eq!(buttons, 0, "stale or held input revived during quarantine");
        }
        tx.send(InputPacket {
            epoch: 0,
            payload: neutral,
        })
        .await
        .unwrap();
        tx.send(InputPacket {
            epoch: 0,
            payload: press,
        })
        .await
        .unwrap();
        loop {
            let (_, buttons) = frame(&mut socket).await;
            if buttons == 1 {
                break;
            }
        }
        drop(tx);
        drop(epoch_tx);
        assert_eq!(frame(&mut socket).await, (*b"NXIR", 0));
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn repeatedly_closed_connections_exhaust_recovery_budget() {
        use moonshine_core::session::stream::control::host_input::InputReceiver;
        use tokio::{
            io::AsyncReadExt,
            net::TcpListener,
            sync::{mpsc, watch},
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, events) = mpsc::channel(4);
        let (epoch_tx, overflow) = watch::channel(0u64);
        let task = tokio::spawn(forward(InputReceiver { events, overflow }, port, [9u8; 32]));
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut token = [0u8; 32];
                if socket.read_exact(&mut token).await.is_ok() {
                    assert_eq!(token, [9u8; 32]);
                }
                drop(socket);
            }
        });
        let result = tokio::time::timeout(Duration::from_secs(9), task)
            .await
            .expect("retry loop did not finish")
            .unwrap();
        assert!(
            result.is_err(),
            "repeated failures must eventually stop the session"
        );
        drop(tx);
        drop(epoch_tx);
        server.abort();
    }

    #[tokio::test]
    async fn closed_session_does_not_wait_for_reconnect_timeout() {
        use moonshine_core::session::stream::control::host_input::InputReceiver;
        use tokio::{
            io::AsyncReadExt,
            net::TcpListener,
            sync::{mpsc, watch},
        };
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, events) = mpsc::channel(4);
        let (epoch_tx, overflow) = watch::channel(0u64);
        let task = tokio::spawn(forward(InputReceiver { events, overflow }, port, [6u8; 32]));
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut auth = [0u8; 32];
        socket.read_exact(&mut auth).await.unwrap();
        let mut first = [0u8; 22];
        socket.read_exact(&mut first).await.unwrap();
        drop(socket);
        drop(listener);
        tokio::time::sleep(Duration::from_millis(800)).await;
        drop(tx);
        drop(epoch_tx);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("session stayed alive during reconnect")
            .unwrap()
            .unwrap();
    }

    #[test]
    fn controller_mapping_and_neutral() {
        let mut p = [0u8; 24];
        p[..4].copy_from_slice(&13u32.to_le_bytes());
        p[8] = 1;
        p[12..14].copy_from_slice(&(0x1000u16 | 0x0010).to_le_bytes());
        p[14] = 255;
        p[16..18].copy_from_slice(&(-1234i16).to_le_bytes());
        let s = parse(&p).unwrap();
        assert_eq!(s.buttons, (1 << 0) | (1 << 8) | (1 << 10));
        assert_eq!(s.axes[0], -1234);
        assert_eq!(&packet(7, s)[4..12], &7u64.to_be_bytes());
        p[8] = 0;
        assert_eq!(parse(&p), Some(State::default()));
    }
}
