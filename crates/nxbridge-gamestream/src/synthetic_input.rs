//! Observe the same session-scoped input bridge used by the game runtime.
//! The loopback sink is local to this synthetic session and receives no secrets
//! or input from any other session.
use moonshine_core::session::stream::control::host_input::InputReceiver;
use std::io;
use tokio::{io::AsyncReadExt, net::TcpListener};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    buttons: u16,
    axes: [i16; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observation {
    Ready,
    State(State),
    Reset,
}

// Preserve the previous synthetic button log's GameStream flag convention.
fn gamestream_buttons(native: u16) -> u16 {
    let sources = [
        0x1000, 0x2000, 0x4000, 0x8000, 0x0040, 0x0080, 0x0100, 0x0200, 0x0010, 0x0020, 0x0004,
        0x0001, 0x0008, 0x0002,
    ];
    let dest = [0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15];
    sources
        .into_iter()
        .zip(dest)
        .fold(0, |flags, (source, bit)| {
            if native & (1 << bit) != 0 {
                flags | source
            } else {
                flags
            }
        })
}

pub async fn observe(input: InputReceiver) -> io::Result<()> {
    let mut previous_flags = 0;
    observe_with(input, move |event| match event {
        Observation::Ready => println!("session input bridge ready"),
        Observation::State(state) => {
            println!(
                "session input buttons=0x{:04x} axes={:?}",
                state.buttons, state.axes
            );
            let flags = gamestream_buttons(state.buttons);
            if flags != previous_flags {
                previous_flags = flags;
                println!("session controller buttons=0x{flags:04x}");
            }
        }
        Observation::Reset => {
            println!("session input reset buttons=0x0000 axes=[0, 0, 0, 0]");
            if previous_flags != 0 {
                previous_flags = 0;
                println!("session controller buttons=0x0000");
            }
        }
    })
    .await
}

async fn observe_with<F>(input: InputReceiver, mut callback: F) -> io::Result<()>
where
    F: FnMut(Observation) + Send,
{
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    // Generate a per-instance marker; never reuse a user's authentication token.
    // The random-port listener is bound only to loopback.
    let mut marker = [0u8; 32];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut marker)?;
    let consume = async move {
        let (mut socket, _) = listener.accept().await?;
        let mut got = [0u8; 32];
        socket.read_exact(&mut got).await?;
        if got != marker {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic input marker mismatch",
            ));
        }
        callback(Observation::Ready);
        let mut last = State::default();
        loop {
            let mut packet = [0u8; 22];
            match socket.read_exact(&mut packet).await {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            }
            if &packet[..4] == b"NXIR" {
                last = State::default();
                callback(Observation::Reset);
            } else if &packet[..4] == b"NXI1" {
                let mut state = State {
                    buttons: u16::from_be_bytes([packet[12], packet[13]]),
                    axes: [0; 4],
                };
                for (i, axis) in state.axes.iter_mut().enumerate() {
                    *axis = i16::from_be_bytes([packet[14 + i * 2], packet[15 + i * 2]]);
                }
                if state != last {
                    last = state;
                    callback(Observation::State(state));
                }
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid synthetic input frame",
                ));
            }
        }
    };
    tokio::try_join!(
        crate::game_input::forward_single(input, port, marker),
        consume
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use moonshine_core::session::stream::control::host_input::InputPacket;
    use std::time::Duration;
    use tokio::sync::{mpsc, watch};

    #[tokio::test]
    async fn observes_real_bridge_reset_and_rejects_stale_or_held_state() {
        let (tx, events) = mpsc::channel(8);
        let (epoch, overflow) = watch::channel(0u64);
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(observe_with(InputReceiver { events, overflow }, move |e| {
            let _ = seen_tx.send(e);
        }));
        async fn next(rx: &mut mpsc::UnboundedReceiver<Observation>) -> Observation {
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap()
        }
        assert_eq!(next(&mut seen_rx).await, Observation::Ready);
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
        assert_eq!(
            next(&mut seen_rx).await,
            Observation::State(State {
                buttons: 1,
                axes: [0; 4]
            })
        );
        epoch.send(1).unwrap();
        assert_eq!(next(&mut seen_rx).await, Observation::Reset);
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
        // No output may reappear from either an old packet or a held input
        // in the new epoch before an explicit neutral arrives.
        assert!(
            tokio::time::timeout(Duration::from_millis(250), seen_rx.recv())
                .await
                .is_err()
        );
        tx.send(InputPacket {
            epoch: 1,
            payload: neutral,
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(250), seen_rx.recv())
                .await
                .is_err()
        );
        tx.send(InputPacket {
            epoch: 1,
            payload: press,
        })
        .await
        .unwrap();
        assert_eq!(
            next(&mut seen_rx).await,
            Observation::State(State {
                buttons: 1,
                axes: [0; 4]
            })
        );
        drop(tx);
        drop(epoch);
        assert_eq!(next(&mut seen_rx).await, Observation::Reset);
        task.await.unwrap().unwrap();
    }

    #[test]
    fn legacy_button_log_keeps_gamestream_flag_mapping() {
        assert_eq!(gamestream_buttons(1), 0x1000);
        assert_eq!(gamestream_buttons(1 << 2), 0x4000);
        assert_eq!(gamestream_buttons(1 << 8), 0); // triggers weren't in the old flag log
    }
}
