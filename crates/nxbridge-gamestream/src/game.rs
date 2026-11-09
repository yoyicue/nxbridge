//! Experimental bridge from the nxnative Mach worker into a GameStream session.
//! The worker inherits private socket pairs; no media is published on a LAN port.
use moonshine_core::session::{
    manager::SessionManager,
    stream::{audio::HostAudioChunk, video::EncodedFrame},
};
use nxbridge_media::Epoch;
use std::{
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::process::CommandExt,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{UnixDatagram, UnixStream},
    sync::mpsc,
};

// The group is created by this session and inherited by its worker and game.
// On cancellation, signal that exact group so an orphan cannot keep the save open.
struct SessionGroup(Option<i32>);
impl Drop for SessionGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
    }
}

fn child_fd(fd: i32) -> io::Result<OwnedFd> {
    let n = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(n) })
    }
}

// A Darwin AF_UNIX datagram pair defaults to only about two audio packets.
// Reserve a bounded burst buffer on the producing and receiving ends; the
// downstream queue still sheds stale audio instead of accumulating latency.
// Video uses the same byte bound: a large keyframe streams through it without
// being truncated, and backpressure reaches the pre-encode latest-frame slot.
fn configure_media_socket_buffers(
    producer: &impl AsRawFd,
    consumer: &impl AsRawFd,
) -> io::Result<()> {
    const BUFFER_BYTES: libc::c_int = 32 * 1024;
    for (fd, option) in [
        (producer.as_raw_fd(), libc::SO_SNDBUF),
        (consumer.as_raw_fd(), libc::SO_RCVBUF),
    ] {
        let result = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                option,
                (&BUFFER_BYTES as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

// The experimental bridge emits fixed 5 ms F32 stereo blocks. Validate source
// time and flags before converting to the downstream session-scoped chunk.
fn decode_audio_packet(packet: &[u8], last_pts: u64) -> Option<(u64, u32, Vec<i16>)> {
    if packet.len() < 16 {
        return None;
    }
    let pts = u64::from_le_bytes(packet[..8].try_into().ok()?);
    let frames = u32::from_le_bytes(packet[8..12].try_into().ok()?) as usize;
    let flags = u32::from_le_bytes(packet[12..16].try_into().ok()?);
    if pts <= last_pts
        || frames != 240
        || packet.len() != 16 + frames * 8
        || flags & !nxbridge_media::AUDIO_DISCONTINUITY != 0
    {
        return None;
    }
    let pcm = packet[16..]
        .chunks_exact(4)
        .map(|b| {
            let f = f32::from_le_bytes(b.try_into().unwrap());
            if f.is_finite() {
                (f.clamp(-1.0, 1.0) * 32767.0) as i16
            } else {
                0
            }
        })
        .collect();
    Some((pts, flags, pcm))
}

// The source must keep draining its bounded socket even when the encoder is
// delayed. Remember any dropped block until a later block is accepted.
fn enqueue_audio(
    audio: &mpsc::Sender<HostAudioChunk>,
    mut chunk: HostAudioChunk,
    pending_discontinuity: &mut bool,
) -> Result<bool, ()> {
    chunk.discontinuity |= *pending_discontinuity;
    match audio.try_send(chunk) {
        Ok(()) => {
            *pending_discontinuity = false;
            Ok(false)
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            *pending_discontinuity = true;
            Ok(true)
        }
        Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
    }
}

pub async fn run(
    manager: SessionManager,
    epoch: Epoch,
    video: mpsc::Sender<EncodedFrame>,
    audio: mpsc::Sender<HostAudioChunk>,
    input: moonshine_core::session::stream::control::host_input::InputReceiver,
    selected: Option<&crate::catalog::Game>,
) -> io::Result<()> {
    let _save_lock = selected.map(crate::catalog::lock_save).transpose()?;
    let legacy =
        |key| std::env::var_os(key).ok_or_else(|| io::Error::other(format!("{key} missing")));
    let worker = if let Some(g) = selected {
        g.worker.clone().into_os_string()
    } else {
        legacy("NXB_GAME_WORKER")?
    };
    let host = if let Some(g) = selected {
        g.host.clone().into_os_string()
    } else {
        legacy("NXB_GAME_HOST")?
    };
    let game = if let Some(g) = selected {
        g.game_dir.clone().into_os_string()
    } else {
        legacy("NXB_GAME_DIR")?
    };
    let sandbox = if let Some(g) = selected {
        g.sandbox.clone().into_os_string()
    } else {
        legacy("NXB_GAME_SANDBOX")?
    };
    let port = std::env::var("NXB_GAME_INPUT_PORT")
        .unwrap_or_else(|_| "47999".into())
        .parse::<u16>()
        .map_err(|_| io::Error::other("invalid input port"))?;
    if port < 1024 {
        return Err(io::Error::other("invalid input port"));
    }
    let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
    let mut token = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut token)?;
    let token_hex: String = token.iter().map(|b| format!("{b:02x}")).collect();
    let (video_parent, video_child) = std::os::unix::net::UnixStream::pair()?;
    let (audio_parent, audio_child) = std::os::unix::net::UnixDatagram::pair()?;
    configure_media_socket_buffers(&audio_child, &audio_parent)?;
    configure_media_socket_buffers(&video_child, &video_parent)?;
    video_parent.set_nonblocking(true)?;
    audio_parent.set_nonblocking(true)?;
    let vf = child_fd(video_child.as_raw_fd())?;
    let af = child_fd(audio_child.as_raw_fd())?;
    let raw_v = vf.as_raw_fd();
    let raw_a = af.as_raw_fd();
    let session: String = epoch
        .session_id
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut cmd = tokio::process::Command::new(worker);
    if let Some(g) = selected {
        // Do not inherit another game's save, cheats, shader or asset settings.
        crate::catalog::apply_environment(&mut cmd, g, std::env::vars_os().map(|(key, _)| key));
    }
    cmd.arg("/usr/bin/sandbox-exec")
        .arg("-f")
        .arg(sandbox)
        .arg(host)
        .arg(game)
        .env("NXB_BRIDGE_VIDEO_FD", "3")
        .env("NXB_BRIDGE_AUDIO_FD", "4")
        .env("NXB_SESSION_ID", session)
        .env("NXB_VIDEO_GENERATION", epoch.generation.to_string())
        .env("NXBRIDGE_INPUT_PORT", port.to_string())
        .env("NXBRIDGE_INPUT_TOKEN", token_hex)
        .env("DM_AUDIO", "0")
        .env_remove("DM_WINDOW")
        .env_remove("NXB_WORKER_TEST_SECONDS")
        .env_remove("NXB_WORKER_OUTPUT")
        .env_remove("NXB_WORKER_AUDIO")
        .kill_on_drop(true);
    unsafe {
        cmd.as_std_mut().pre_exec(move || {
            if libc::setpgid(0, 0) < 0 || libc::dup2(raw_v, 3) < 0 || libc::dup2(raw_a, 4) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    drop(reservation);
    let mut child = cmd.spawn()?;
    let mut group = SessionGroup(child.id().map(|pid| pid as i32));
    drop(vf);
    drop(af);
    drop(video_child);
    drop(audio_child);
    let mut vr = UnixStream::from_std(video_parent)?;
    let ar = UnixDatagram::from_std(audio_parent)?;
    let input_task = tokio::spawn(async move {
        if let Err(e) = crate::game_input::forward(input, port, token).await {
            eprintln!("game input bridge: {e}");
        }
    });
    let v = tokio::spawn(async move {
        let mut number = 1u32;
        let mut last = 0u64;
        loop {
            let mut h = [0u8; 12];
            if vr.read_exact(&mut h).await.is_err() {
                break;
            }
            let pts = u64::from_le_bytes(h[0..8].try_into().unwrap());
            let len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
            if pts <= last || len == 0 || len > 16 * 1024 * 1024 {
                break;
            }
            let mut data = vec![0; len];
            if vr.read_exact(&mut data).await.is_err() {
                break;
            }
            let key = data
                .windows(5)
                .any(|x| x[..4] == [0, 0, 0, 1] && x[4] & 31 == 5);
            let timestamp = ((u128::from(pts) * 90_000 / 1_000_000_000) & 0xffff_ffff) as u32;
            if video
                .send(EncodedFrame {
                    queued_at: std::time::Instant::now(),
                    data,
                    is_key_frame: key,
                    frame_number: number,
                    rtp_timestamp: timestamp,
                })
                .await
                .is_err()
            {
                break;
            }
            last = pts;
            number = number.wrapping_add(1);
        }
    });
    let a = tokio::spawn(async move {
        let mut packet = [0u8; 16 + 480 * 8];
        let mut last = 0u64;
        let mut discontinuities = 0u64;
        let mut dropped = 0u64;
        let mut received = 0u64;
        let mut accepted = 0u64;
        let mut first_pts = None;
        let started = tokio::time::Instant::now();
        let mut last_report = started;
        let mut pending_discontinuity = false;
        let mut last_receive = None;
        let mut receive_gap_max_us = 0u128;
        let mut source_age_max_us = 0u64;
        let mut source_late_blocks = 0u64;
        loop {
            let n = match ar.recv(&mut packet).await {
                Ok(n) => n,
                Err(_) => break,
            };
            let Some((pts, flags, pcm)) = decode_audio_packet(&packet[..n], last) else {
                continue;
            };
            let received_at = tokio::time::Instant::now();
            if let Some(previous) = last_receive.replace(received_at) {
                receive_gap_max_us =
                    receive_gap_max_us.max(received_at.duration_since(previous).as_micros());
            }
            // Both the native source and this process use CLOCK_MONOTONIC.
            // This measures source-to-Rust age, not a cross-device latency.
            let source_age_us = crate::clock_ns().saturating_sub(pts) / 1_000;
            source_age_max_us = source_age_max_us.max(source_age_us);
            source_late_blocks += u64::from(source_age_us > 20_000);
            if flags & nxbridge_media::AUDIO_DISCONTINUITY != 0 {
                discontinuities += 1;
                if discontinuities <= 3 || discontinuities.is_power_of_two() {
                    eprintln!("game audio source discontinuity #{discontinuities}");
                }
            }
            received += 1;
            let initial_pts = *first_pts.get_or_insert(pts);
            let chunk = HostAudioChunk {
                session_id: epoch.session_id,
                generation: epoch.generation,
                pts_ns: pts,
                discontinuity: flags & nxbridge_media::AUDIO_DISCONTINUITY != 0,
                pcm,
            };
            match enqueue_audio(&audio, chunk, &mut pending_discontinuity) {
                Ok(true) => {
                    dropped += 1;
                    if dropped <= 3 || dropped.is_power_of_two() {
                        eprintln!("game audio downstream queue full: {dropped} block(s) dropped");
                    }
                }
                Ok(false) => accepted += 1,
                Err(()) => break,
            }
            let now = tokio::time::Instant::now();
            if now.duration_since(last_report) >= Duration::from_secs(10) {
                eprintln!(
                    "game audio stats wall_ms={} source_ms={} received={} accepted={} dropped={} receive_gap_max_us={} source_age_max_us={} source_late_blocks={}",
                    now.duration_since(started).as_millis(),
                    pts.saturating_sub(initial_pts) / 1_000_000,
                    received,
                    accepted,
                    dropped,
                    receive_gap_max_us,
                    source_age_max_us,
                    source_late_blocks
                );
                last_report = now;
                receive_gap_max_us = 0;
                source_age_max_us = 0;
            }
            last = pts;
        }
    });
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if v.is_finished()
            || a.is_finished()
            || input_task.is_finished()
            || manager.video_frame_sender().await.is_none()
            || child.try_wait()?.is_some()
        {
            break;
        }
    }
    v.abort();
    a.abort();
    input_task.abort();
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    if tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .is_err()
    {
        if let Some(pid) = group.0 {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    if let Some(pid) = group.0 {
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
    }
    group.0 = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_bridge_checks_flags_length_and_preserves_discontinuity() {
        let mut packet = vec![0u8; 16 + 240 * 8];
        packet[..8].copy_from_slice(&1_000_000u64.to_le_bytes());
        packet[8..12].copy_from_slice(&240u32.to_le_bytes());
        packet[12..16].copy_from_slice(&nxbridge_media::AUDIO_DISCONTINUITY.to_le_bytes());
        packet[16..20].copy_from_slice(&1.0f32.to_le_bytes());
        let (pts, flags, samples) = decode_audio_packet(&packet, 0).unwrap();
        assert_eq!(
            (pts, flags, samples.len(), samples[0]),
            (1_000_000, 1, 480, 32767)
        );
        assert!(decode_audio_packet(&packet, pts).is_none());
        assert!(decode_audio_packet(&packet[..packet.len() - 1], 0).is_none());
        packet[12..16].copy_from_slice(&2u32.to_le_bytes());
        assert!(decode_audio_packet(&packet, 0).is_none());
    }
    #[test]
    fn source_timeline_enforces_epoch_gap_and_recovery_policy() {
        use moonshine_core::session::stream::audio::AudioSourceTimeline;
        let make = |pts_ns, discontinuity, generation| HostAudioChunk {
            session_id: [1; 16],
            generation,
            pts_ns,
            discontinuity,
            pcm: vec![0; 480],
        };
        let mut timeline = AudioSourceTimeline::default();
        assert_eq!(
            timeline.accept(&make(1_000_000_000, false, 1)),
            Some((0, false))
        );
        assert_eq!(timeline.accept(&make(1_000_000_000, true, 1)), None);
        assert_eq!(timeline.accept(&make(995_000_000, true, 1)), None);
        assert_eq!(timeline.accept(&make(1_005_000_000, false, 2)), None);
        assert_eq!(timeline.accept(&make(1_015_000_000, false, 1)), None);
        assert_eq!(
            timeline.accept(&make(1_015_000_000, true, 1)),
            Some((2, false))
        );
        assert_eq!(
            timeline.accept(&make(2_000_000_000, true, 1)),
            Some((0, true))
        );
        assert_eq!(
            timeline.accept(&make(2_007_000_000, true, 1)),
            Some((0, true))
        );
    }
    #[test]
    fn overloaded_source_marks_next_accepted_chunk_discontinuous() {
        let (tx, mut rx) = mpsc::channel(1);
        let make = |pts_ns| HostAudioChunk {
            session_id: [1; 16],
            generation: 1,
            pts_ns,
            discontinuity: false,
            pcm: vec![0; 480],
        };
        let mut pending = false;
        assert_eq!(
            enqueue_audio(&tx, make(1_000_000_000), &mut pending),
            Ok(false)
        );
        assert_eq!(
            enqueue_audio(&tx, make(1_005_000_000), &mut pending),
            Ok(true)
        );
        assert!(pending);
        assert!(!rx.try_recv().unwrap().discontinuity);
        assert_eq!(
            enqueue_audio(&tx, make(1_010_000_000), &mut pending),
            Ok(false)
        );
        let recovered = rx.try_recv().unwrap();
        assert_eq!(recovered.pts_ns, 1_010_000_000);
        assert!(recovered.discontinuity);
        assert!(!pending);
        drop(rx);
        assert_eq!(
            enqueue_audio(&tx, make(1_015_000_000), &mut pending),
            Err(())
        );
    }
    #[test]
    fn audio_socket_buffer_absorbs_short_bursts_without_blocking() {
        let (consumer, producer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        configure_media_socket_buffers(&producer, &consumer).unwrap();
        producer.set_nonblocking(true).unwrap();
        consumer.set_nonblocking(true).unwrap();
        let packet = [7u8; 16 + 240 * 2 * 4];
        let mut sent = 0;
        while producer.send(&packet).is_ok() {
            sent += 1;
            assert!(sent < 100, "audio transport buffer must remain bounded");
        }
        assert!(
            sent >= 12,
            "buffer should absorb at least 60 ms of packets, got {sent}"
        );
        let mut received = 0;
        let mut buf = [0u8; 16 + 240 * 2 * 4];
        while let Ok(n) = consumer.recv(&mut buf) {
            assert_eq!(n, packet.len());
            assert_eq!(buf, packet);
            received += 1;
        }
        assert_eq!(received, sent);
    }
}
