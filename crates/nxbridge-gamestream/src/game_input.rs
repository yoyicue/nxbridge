//! Translate authenticated GameStream controller 0 into the existing local
//! nxnative input receiver protocol. No global keyboard or mouse injection.
use std::{io, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::mpsc};

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
pub async fn forward(
    mut rx: mpsc::Receiver<Vec<u8>>,
    port: u16,
    token: [u8; 32],
) -> io::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut stream = loop {
        match TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
            Ok(s) => break s,
            Err(e) if tokio::time::Instant::now() >= deadline => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    };
    stream.write_all(&token).await?;
    // Events received before the isolated game listener was ready are stale.
    while rx.try_recv().is_ok() {}
    let mut current = State::default();
    let mut seq = 0u64;
    let mut heartbeat = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            event = rx.recv() => match event { Some(p) => { if let Some(s) = parse(&p) { current = s; } }, None => break },
            _ = heartbeat.tick() => {},
        }
        seq += 1;
        tokio::time::timeout(
            Duration::from_millis(500),
            stream.write_all(&packet(seq, current)),
        )
        .await??;
    }
    seq += 1;
    let _ = tokio::time::timeout(
        Duration::from_millis(500),
        stream.write_all(&packet(seq, State::default())),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
