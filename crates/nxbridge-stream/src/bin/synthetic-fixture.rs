//! Write a finite raw fixture for local encoder testing; this is not a server.
use nxbridge_media::{AudioLeases, Epoch, VideoLeases, RELEASE_ENCODED};
use nxbridge_stream::synthetic::SyntheticSource;
use std::{
    env,
    fs::{create_dir_all, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("usage: synthetic-fixture OUTPUT_DIRECTORY")?,
    );
    create_dir_all(&dir)?;
    let epoch = Epoch {
        session_id: [1; 16],
        generation: 1,
    };
    let source =
        SyntheticSource::new(epoch, 1_000_000_000, 320, 180).map_err(|e| format!("{e:?}"))?;
    let mut video = BufWriter::new(File::create(dir.join("video.bgra"))?);
    let mut audio = BufWriter::new(File::create(dir.join("audio.f32le"))?);
    let mut vl = VideoLeases::new(epoch).map_err(|e| format!("{e:?}"))?;
    let mut al = AudioLeases::new(epoch).map_err(|e| format!("{e:?}"))?;
    for i in 0..60 {
        let v = source.video(i, 1).map_err(|e| format!("{e:?}"))?;
        v.buffer
            .validate_for(&v.descriptor)
            .map_err(|e| format!("{e:?}"))?;
        vl.publish(v.descriptor).map_err(|e| format!("{e:?}"))?;
        let f = vl.begin_encode().ok_or("video lease unavailable")?;
        video.write_all(&v.pixels)?;
        vl.complete(epoch, f.buffer_id, f.sequence, RELEASE_ENCODED)
            .map_err(|e| format!("{e:?}"))?;
    }
    for i in 0..200 {
        let a = source.audio(i, 1).map_err(|e| format!("{e:?}"))?;
        a.buffer
            .validate_for(&a.descriptor)
            .map_err(|e| format!("{e:?}"))?;
        al.publish(a.descriptor).map_err(|e| format!("{e:?}"))?;
        let c = al.begin_consume().ok_or("audio lease unavailable")?;
        audio.write_all(&a.pcm_le)?;
        al.complete(epoch, c.buffer_id, c.sequence, RELEASE_ENCODED)
            .map_err(|e| format!("{e:?}"))?;
    }
    video.flush()?;
    audio.flush()?;
    println!("Wrote 60 numbered 320x180 BGRA frames and 2 seconds of 48 kHz stereo PCM.");
    Ok(())
}
