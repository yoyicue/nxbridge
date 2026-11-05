//! Inspect an AUD-delimited Annex-B encoder stream without decoding it.
use nxbridge_stream::annexb::AnnexBSplitter;
use std::{env, fs::File, io::Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os()
        .nth(1)
        .ok_or("usage: annexb-inspect FILE.h264")?;
    let mut input = File::open(path)?;
    let mut splitter = AnnexBSplitter::new();
    let mut buf = [0_u8; 4096];
    let mut frames = 0;
    let mut keyframes = 0;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for unit in splitter.push(&buf[..n]).map_err(|e| format!("{e:?}"))? {
            frames += 1;
            keyframes += usize::from(unit.is_key_frame);
        }
    }
    if let Some(unit) = splitter.finish().map_err(|e| format!("{e:?}"))? {
        frames += 1;
        keyframes += usize::from(unit.is_key_frame);
    }
    println!("access units: {frames}, IDR access units: {keyframes}");
    Ok(())
}
