use async_shutdown::ShutdownManager;
use moonshine_core::{
    clients::ClientManager,
    config::Config,
    rtsp::RtspServer,
    session::{
        application::ApplicationConfig,
        manager::SessionManager,
        stream::{audio::HostAudioChunk, video::EncodedFrame},
    },
    webserver::Webserver,
};
use nxbridge_media::{AudioLeases, Epoch, VideoLeases, RELEASE_ENCODED};
use nxbridge_stream::{annexb::AnnexBSplitter, synthetic::SyntheticSource};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod catalog;
mod game;
mod game_input;
mod synthetic_input;

fn application_config(
    game_mode: bool,
    title: Option<String>,
    boxart: Option<std::path::PathBuf>,
) -> ApplicationConfig {
    if game_mode {
        ApplicationConfig {
            title: title
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "NXBridge game".into()),
            boxart,
            ..Default::default()
        }
    } else {
        ApplicationConfig {
            title: "NXBridge synthetic".into(),
            ..Default::default()
        }
    }
}

fn clock_ns() -> u64 {
    let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) },
        0
    );
    let ts = unsafe { ts.assume_init() };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut args = std::env::args_os().skip(1);
    let address = args
        .next()
        .expect("usage: nxbridge-gamestream BIND_IPV4 STATE_DIR [HTTP_PORT]");
    let path = std::path::PathBuf::from(args.next().expect("state directory required"));
    let http_port: u16 = args
        .next()
        .map(|s| s.to_string_lossy().parse().expect("invalid port"))
        .unwrap_or(47989);
    assert!(args.next().is_none(), "unexpected arguments");
    let address = address.into_string().expect("bind address must be UTF-8");
    let ip: std::net::Ipv4Addr = address.parse().expect("bind address must be IPv4");
    assert!(!ip.is_unspecified(), "select a specific local interface");
    assert!(
        http_port >= 6 && http_port <= 65514,
        "port group out of range"
    );
    std::fs::create_dir_all(&path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("protect state directory");
    let mut c = Config::default();
    let library_covers = std::env::var_os("NXB_LIBRARY_COVER_DIR").map(std::path::PathBuf::from);
    let catalog = std::env::var_os("NXB_GAME_CATALOG").map(|p| {
        catalog::load(std::path::Path::new(&p), library_covers.as_deref())
            .expect("invalid NXB_GAME_CATALOG")
    });
    let game_mode = catalog.is_some() || std::env::var_os("NXB_GAME_WORKER").is_some();
    c.name = std::env::var("NXB_HOST_NAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            if game_mode {
                "NXBridge Games"
            } else {
                "NXBridge synthetic 032"
            }
            .into()
        });
    c.address = address;
    c.webserver.port = http_port;
    c.webserver.port_https = http_port - 5;
    c.stream.port = http_port + 21;
    c.stream.video.port = http_port + 9;
    c.stream.audio.port = http_port + 11;
    c.stream.control.port = http_port + 10;
    c.stream.timeout = 5;
    c.webserver.enable_pairing = true;
    c.webserver.certificate = path.join("cert.pem");
    c.webserver.private_key = path.join("key.pem");
    let mut boxart = (game_mode && catalog.is_none())
        .then(|| std::env::var_os("NXB_APP_BOXART"))
        .flatten()
        .map(std::path::PathBuf::from);
    if let (None, Some(dir), Ok(id)) = (
        &catalog,
        &library_covers,
        std::env::var("NXB_LIBRARY_GAME_ID"),
    ) {
        boxart = Some(catalog::library_cover(dir, &id).expect("invalid library cover mapping"));
    } else if let Some(path) = &boxart {
        assert!(
            path.is_absolute() && path.is_file(),
            "NXB_APP_BOXART must be an existing absolute file"
        );
    }
    c.applications = match &catalog {
        Some(games) => games.iter().map(|game| game.application()).collect(),
        None => vec![application_config(
            game_mode,
            std::env::var("NXB_APP_TITLE").ok(),
            boxart,
        )],
    };
    if game_mode && catalog.is_none() {
        if let Ok(id) = std::env::var("NXB_LIBRARY_GAME_ID") {
            catalog::library_cover(std::path::Path::new("/"), &id)
                .expect("invalid library game ID");
            c.applications[0].library_game_id = Some(id);
        }
    }
    let shutdown = ShutdownManager::new();
    let (cert, key) = moonshine_core::tls::load_or_create_certificate(&c).expect("certificate");
    let manager = SessionManager::new(
        c.stream.video.clone(),
        c.stream.audio.clone(),
        c.stream.control.clone(),
        c.address.clone(),
        c.stream.timeout,
        shutdown.clone(),
    )
    .expect("session manager");
    let clients = ClientManager::new_at(cert.clone(), key, &path).expect("client manager");
    let id = clients.persistent_state().get_uuid().expect("uuid");
    let _rtsp = RtspServer::new(
        c.address.clone(),
        c.stream.port,
        c.stream.video.clone(),
        c.stream.audio.clone(),
        c.stream.control.clone(),
        manager.clone(),
        shutdown.clone(),
    );
    let _web = Webserver::new(
        c.name.clone(),
        c.address.clone(),
        c.stream.port,
        c.webserver.clone(),
        c.applications,
        1,
        false,
        id,
        cert,
        clients.clone(),
        manager.clone(),
        shutdown.clone(),
    )
    .expect("web");
    let _discovery =
        moonshine_core::discovery::MdnsDiscovery::spawn(&c.address, c.webserver.port, &c.name);
    println!(
        "NXBridge {} host ready; input PIN via STATE_DIR/pair-pin",
        if game_mode { "game" } else { "synthetic" }
    );
    let pin_manager = clients.clone();
    let pin_path = path.join("pair-pin");
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let pending = pin_manager.pending_clients();
            if pending.len() == 1 {
                if let Ok(pin) = std::fs::read_to_string(&pin_path) {
                    let pin = pin.trim();
                    if pin.len() == 4 && pin.bytes().all(|c| c.is_ascii_digit()) {
                        for p in pending {
                            let _ = pin_manager.register_pin(&p.id, pin);
                        }
                        let _ = std::fs::remove_file(&pin_path);
                        println!("PIN submitted for pending client");
                    }
                }
            }
        }
    });
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut pump = tokio::spawn(session_pump(manager.clone(), catalog, stop_rx));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("register SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
    let _ = stop_tx.send(true);
    let _ = manager.stop_session().await;
    // A free protocol session can precede worker teardown. Wait for our
    // session pump so its per-session process group is actually reaped.
    if tokio::time::timeout(Duration::from_secs(8), &mut pump)
        .await
        .is_err()
    {
        pump.abort();
        let _ = pump.await;
    }
}

#[cfg(test)]
mod application_tests {
    use super::application_config;
    use std::path::PathBuf;

    #[test]
    fn game_metadata_is_optional_and_synthetic_is_unchanged() {
        let game = application_config(
            true,
            Some("旷野之息".into()),
            Some(PathBuf::from("/art.png")),
        );
        assert_eq!(game.title, "旷野之息");
        assert_eq!(game.boxart, Some(PathBuf::from("/art.png")));
        assert_eq!(
            application_config(true, Some(" ".into()), None).title,
            "NXBridge game"
        );
        let synthetic = application_config(
            false,
            Some("ignored".into()),
            Some(PathBuf::from("/art.png")),
        );
        assert_eq!(synthetic.title, "NXBridge synthetic");
        assert_eq!(synthetic.boxart, None);
    }
}

async fn session_pump(
    manager: SessionManager,
    catalog: Option<Vec<catalog::Game>>,
    stopping: tokio::sync::watch::Receiver<bool>,
) {
    let mut generation = 0u64;
    loop {
        if *stopping.borrow() {
            return;
        }
        let tx = loop {
            if *stopping.borrow() {
                return;
            }
            if let Some(tx) = manager.video_frame_sender().await {
                break tx;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let Some(ctx) = manager.active_video_context().await else {
            continue;
        };
        let Some(audio_tx) = manager.audio_frame_sender().await else {
            let _ = manager.stop_session().await;
            continue;
        };
        let mut control = manager.encoder_control_receiver().await;
        let mut input = manager.input_receiver().await;
        generation += 1;
        println!(
            "session {generation}: {}x{} {} fps, codec {:?}, bitrate {}",
            ctx.width, ctx.height, ctx.fps, ctx.video_format, ctx.bitrate
        );
        if ctx.fps != 30
            || !matches!(
                ctx.video_format,
                moonshine_core::session::stream::video::VideoFormat::H264
            )
        {
            eprintln!("only 30 fps H.264 supported for this experiment");
            let _ = manager.stop_session().await;
            continue;
        }
        let mut sid = [0u8; 16];
        sid[..8].copy_from_slice(&clock_ns().to_le_bytes());
        sid[8..].copy_from_slice(&generation.to_le_bytes());
        let epoch = Epoch {
            session_id: sid,
            generation,
        };
        if catalog.is_some() || std::env::var_os("NXB_GAME_WORKER").is_some() {
            let selected = if let Some(games) = &catalog {
                let id = manager
                    .get_session_context()
                    .await
                    .ok()
                    .flatten()
                    .map(|c| c.application_id);
                match id.and_then(|id| catalog::select(games, id)) {
                    Some(game) => Some(game),
                    None => {
                        eprintln!("no game configuration for active application");
                        let _ = manager.stop_session().await;
                        continue;
                    }
                }
            } else {
                None
            };
            if ctx.width != 1280 || ctx.height != 720 {
                eprintln!("game prototype requires 1280x720 H.264 at 30 fps");
            } else if let Some(input) = input.take() {
                let controls = tokio::spawn(async move {
                    if let Some(ref mut r) = control {
                        while r.recv().await.is_some() {}
                    }
                });
                if let Err(e) =
                    game::run(manager.clone(), epoch, tx, audio_tx, input, selected).await
                {
                    eprintln!("game bridge failed: {e}");
                }
                controls.abort();
            } else {
                eprintln!("game input receiver unavailable");
            }
            let _ = manager.stop_session().await;
            continue;
        }
        let origin = clock_ns() + 500_000_000;
        let source = match SyntheticSource::new(epoch, origin, ctx.width, ctx.height) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("source error {e:?}");
                let _ = manager.stop_session().await;
                continue;
            }
        };
        let mut child = match tokio::process::Command::new("ffmpeg")
            .args([
                "-v",
                "warning",
                "-f",
                "rawvideo",
                "-pixel_format",
                "bgra",
                "-video_size",
                &format!("{}x{}", ctx.width, ctx.height),
                "-framerate",
                "30",
                "-i",
                "pipe:0",
                "-an",
                "-c:v",
                "h264_videotoolbox",
                "-realtime",
                "1",
                "-g",
                "30",
                "-bf",
                "0",
                "-b:v",
                &ctx.bitrate.to_string(),
                "-bsf:v",
                "h264_mp4toannexb,h264_metadata=aud=insert",
                "-flush_packets",
                "1",
                "-f",
                "h264",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ffmpeg spawn {e}");
                let _ = manager.stop_session().await;
                continue;
            }
        };
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let vs = source.clone();
        let vtx = tx.clone();
        let video = tokio::spawn(async move {
            let mut leases = VideoLeases::new(epoch).unwrap();
            let mut i = 0u64;
            loop {
                if vtx.is_closed() {
                    break;
                }
                let target = origin.saturating_add(i * 1_000_000_000 / 30);
                let now = clock_ns();
                if target > now {
                    tokio::time::sleep(Duration::from_nanos(target - now)).await;
                }
                let sample = match vs.video(i, 1) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("video sample {e:?}");
                        break;
                    }
                };
                if sample.buffer.validate_for(&sample.descriptor).is_err()
                    || leases.publish(sample.descriptor).is_err()
                {
                    break;
                }
                let f = leases.begin_encode().unwrap();
                if stdin.write_all(&sample.pixels).await.is_err() {
                    break;
                }
                if leases
                    .complete(epoch, f.buffer_id, f.sequence, RELEASE_ENCODED)
                    .is_err()
                {
                    break;
                }
                i += 1;
            }
            let _ = stdin.shutdown().await;
        });
        let atx = audio_tx.clone();
        let asrc = source.clone();
        let audio = tokio::spawn(async move {
            let mut leases = AudioLeases::new(epoch).unwrap();
            let mut i = 0u64;
            loop {
                if atx.is_closed() {
                    break;
                }
                // Provisional fixed compensation for the measured 720p VideoToolbox pipeline;
                // ABI timestamps remain anchored to origin.
                let target = origin
                    .saturating_add(180_000_000)
                    .saturating_add(i * 10_000_000);
                let now = clock_ns();
                if target > now {
                    tokio::time::sleep(Duration::from_nanos(target - now)).await;
                }
                let a = match asrc.audio(i, 1) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                if a.buffer.validate_for(&a.descriptor).is_err()
                    || leases.publish(a.descriptor).is_err()
                {
                    break;
                }
                let c = leases.begin_consume().unwrap();
                for (part, bytes) in a.pcm_le.chunks_exact(240 * 2 * 4).enumerate() {
                    let target = target + part as u64 * 5_000_000;
                    let now = clock_ns();
                    if target > now {
                        tokio::time::sleep(Duration::from_nanos(target - now)).await;
                    }
                    let pcm: Vec<i16> = bytes
                        .chunks_exact(4)
                        .map(|s| {
                            (f32::from_le_bytes(s.try_into().unwrap()).clamp(-1.0, 1.0) * 32767.0)
                                as i16
                        })
                        .collect();
                    let chunk = HostAudioChunk {
                        session_id: epoch.session_id,
                        generation: epoch.generation,
                        pts_ns: c.pts_ns + part as u64 * 5_000_000,
                        discontinuity: part == 0
                            && c.flags & nxbridge_media::AUDIO_DISCONTINUITY != 0,
                        pcm,
                    };
                    if atx.send(chunk).await.is_err() {
                        return;
                    }
                }
                if leases
                    .complete(epoch, c.buffer_id, c.sequence, RELEASE_ENCODED)
                    .is_err()
                {
                    break;
                }
                i += 1;
            }
        });
        let frames = tokio::spawn(async move {
            let mut splitter = AnnexBSplitter::new();
            let mut buf = [0u8; 16384];
            let mut number = 1u32;
            loop {
                let n = match stdout.read(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                let units = match splitter.push(&buf[..n]) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("annexb {e:?}");
                        break;
                    }
                };
                for unit in units {
                    if number % 300 == 1 {
                        println!("frame {number}, IDR={}", unit.is_key_frame);
                    }
                    let frame = EncodedFrame {
                        queued_at: std::time::Instant::now(),
                        data: unit.data,
                        is_key_frame: unit.is_key_frame,
                        frame_number: number,
                        rtp_timestamp: number.wrapping_mul(3000),
                    };
                    if tx.send(frame).await.is_err() {
                        return;
                    }
                    number = number.wrapping_add(1);
                }
            }
        });
        let controls = tokio::spawn(async move {
            if let Some(ref mut r) = control {
                while let Some(c) = r.recv().await {
                    println!("encoder control {c:?}; periodic IDR interval <= 1s");
                }
            }
        });
        let inputs = tokio::spawn(async move {
            if let Some(r) = input.take() {
                if let Err(error) = synthetic_input::observe(r).await {
                    eprintln!("synthetic input observer failed: {error}");
                }
            }
        });
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if *stopping.borrow()
                || frames.is_finished()
                || video.is_finished()
                || audio.is_finished()
                || manager.video_frame_sender().await.is_none()
            {
                break;
            }
        }
        println!("session {generation} ending; dropping input state");
        let _ = manager.stop_session().await;
        video.abort();
        audio.abort();
        frames.abort();
        controls.abort();
        inputs.abort();
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}
