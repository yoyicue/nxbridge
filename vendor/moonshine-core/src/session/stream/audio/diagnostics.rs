//! Opt-in wall-clock probe, separate from deterministic protocol tests.
//! Runs real Opus/FEC with the production queue sizes and a paced source.
use super::*;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_batches_decode_and_recover_two_missing_audio_packets() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let stop = ShutdownManager::new();
    let stream = AudioStream {
        udp_socket: socket,
        stop: stop.clone(),
    };
    let (_keys, keys_rx) = tokio::sync::watch::channel(crate::session::SessionKeyData {
        remote_input_key: vec![],
        remote_input_key_id: 0,
    });
    let (tx, rx) = mpsc::channel(20);
    let start = stream
        .start(
            AudioStreamContext {
                packet_duration_ms: 5,
                audio_config: AudioConfig::from_channels(AudioChannels::Stereo, 3, false),
                ..Default::default()
            },
            keys_rx,
            rx,
        )
        .unwrap();
    start.trigger();
    client.send_to(b"PING", address).await.unwrap();
    let producer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let origin = tokio::time::Instant::now();
        for i in 0..40 {
            tokio::time::sleep_until(origin + Duration::from_millis(i * 5)).await;
            if tx
                .send(HostAudioChunk {
                    session_id: [1; 16],
                    generation: 1,
                    pts_ns: 1_000_000_000 + i * 5_000_000,
                    discontinuity: false,
                    pcm: vec![1000; 480],
                })
                .await
                .is_err()
            {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    let mut groups = std::collections::BTreeMap::<u16, Vec<Option<Vec<u8>>>>::new();
    let mut decoder = opus::MSDecoder::new(48000, 1, 1, &[0, 1]).unwrap();
    let mut decoded = 0;
    let mut last_timestamp = None;
    let mut buf = [0u8; 2048];
    while let Ok(Ok((n, _))) =
        tokio::time::timeout(Duration::from_millis(500), client.recv_from(&mut buf)).await
    {
        let packet = &buf[..n];
        if packet[1] == 97 {
            let sequence = u16::from_be_bytes(packet[2..4].try_into().unwrap());
            let timestamp = u32::from_be_bytes(packet[4..8].try_into().unwrap());
            if let Some(previous) = last_timestamp {
                assert_eq!(timestamp.wrapping_sub(previous), 5);
            }
            last_timestamp = Some(timestamp);
            let mut pcm = [0i16; 480];
            assert_eq!(decoder.decode(&packet[12..], &mut pcm, false).unwrap(), 240);
            decoded += 1;
            let group = groups
                .entry(sequence / 4 * 4)
                .or_insert_with(|| vec![None; 6]);
            group[usize::from(sequence % 4)] = Some(packet[12..].to_vec());
        } else if packet[1] == 127 {
            assert_eq!(packet[13], 97);
            let base = u16::from_be_bytes(packet[14..16].try_into().unwrap());
            let group = groups.entry(base).or_insert_with(|| vec![None; 6]);
            group[4 + usize::from(packet[12])] = Some(packet[24..].to_vec());
        }
    }
    let _ = stop.trigger_shutdown(SessionShutdownReason::UserStopped);
    producer.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), stop.wait_shutdown_complete())
        .await
        .unwrap();
    assert!(decoded >= 8, "too few real UDP Opus packets: {decoded}");
    let mut shards = groups
        .into_values()
        .find(|g| g.iter().all(Option::is_some))
        .expect("no complete four-data/two-parity block reached UDP");
    let expected = shards.clone();
    let mut fec = fec_rs::ReedSolomon::new(4, 2).unwrap();
    fec.set_parity_matrix(&[0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c])
        .unwrap();
    assert!(
        fec.verify(
            &shards
                .iter()
                .map(|s| s.as_ref().unwrap())
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
    shards[0] = None;
    shards[2] = None;
    fec.reconstruct(&mut shards).unwrap();
    assert_eq!(shards, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "wall-clock audio pipeline probe; run explicitly with --nocapture"]
async fn audio_pipeline_timing_probe() {
    use std::io::Write;
    // Validate the opt-in artifact path before spending time on the probe.
    // Cargo runs this test in the vendored crate directory; prefer an absolute path.
    let mut trace = std::env::var("NXB_AUDIO_PROBE_TRACE").ok().map(|path| {
        std::io::BufWriter::new(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap_or_else(|error| panic!("cannot create audio trace {path}: {error}")),
        )
    });
    let seconds = std::env::var("NXB_AUDIO_PROBE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5)
        .clamp(2, 60);
    let stall = std::env::var("NXB_AUDIO_PROBE_STALL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .min(500);
    let (tx, rx) = mpsc::channel(20);
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(16);
    let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(16);
    let (packet_tx, mut packet_rx) = mpsc::channel(1);
    let (_keys, key_rx) = tokio::sync::watch::channel(crate::session::SessionKeyData {
        remote_input_key: vec![],
        remote_input_key_id: 0,
    });
    let gate = AudioStartGate::new();
    let stop = ShutdownManager::new();
    encoder::AudioEncoder::spawn(
        48000,
        &OPUS_STEREO,
        frame_rx,
        recycle_tx,
        key_rx,
        false,
        packet_tx,
        stop.clone(),
        gate.clone(),
    )
    .unwrap();
    host_source::spawn_pcm_bridge(rx, 2, frame_tx, recycle_rx, gate.clone(), stop.clone());
    gate.open();
    let origin = Instant::now() + Duration::from_millis(20);
    let producer = std::thread::spawn(move || {
        let mut dropped = 0;
        let mut gap = false;
        for i in 0..seconds * 200 {
            let target = origin + Duration::from_millis(i * 5);
            std::thread::sleep(target.saturating_duration_since(Instant::now()));
            let pcm = (0..240)
                .flat_map(|s| {
                    let sample =
                        ((((i * 240 + s) as f64) * 440.0 * std::f64::consts::TAU / 48000.0).sin()
                            * 10000.0) as i16;
                    [sample, sample]
                })
                .collect();
            let chunk = HostAudioChunk {
                session_id: [1; 16],
                generation: 1,
                pts_ns: 1_000_000_000 + i * 5_000_000,
                discontinuity: gap,
                pcm,
            };
            match tx.try_send(chunk) {
                Ok(()) => gap = false,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    dropped += 1;
                    gap = true;
                }
                Err(_) => break,
            }
        }
        // Leave time for the finite source to drain before closing its channel.
        std::thread::sleep(Duration::from_millis(150));
        dropped
    });
    let mut arrivals = Vec::new();
    let mut parity = 0;
    let mut injected = false;
    let mut timestamps = Vec::new();
    while let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(2), packet_rx.recv()).await
    {
        for packet in &batch.packets {
            if packet[1] == 97 {
                arrivals.push(Instant::now());
                timestamps.push(u32::from_be_bytes(packet[4..8].try_into().unwrap()));
            } else {
                parity += 1;
            }
        }
        let _ = batch.sent.send(());
        if stall > 0 && !injected && arrivals.len() == 100 {
            injected = true;
            tokio::time::sleep(Duration::from_millis(stall)).await;
        }
    }
    let _ = stop.trigger_shutdown(SessionShutdownReason::UserStopped);
    let source_drops = producer.join().unwrap();
    assert!(timestamps.windows(2).all(|w| w[1].wrapping_sub(w[0]) == 5));
    if let Some(output) = trace.as_mut() {
        writeln!(output, "arrival_us,rtp_ms").unwrap();
        for (arrival, timestamp) in arrivals.iter().zip(&timestamps) {
            writeln!(
                output,
                "{},{}",
                arrival.duration_since(origin).as_micros(),
                timestamp
            )
            .unwrap();
        }
    }
    let mut intervals: Vec<_> = arrivals
        .windows(2)
        .map(|w| w[1].duration_since(w[0]).as_micros())
        .collect();
    intervals.sort_unstable();
    let quantile = |q: usize| intervals[(intervals.len() - 1) * q / 100];
    let max_in_5ms = arrivals
        .iter()
        .enumerate()
        .map(|(i, t)| {
            arrivals[i..]
                .iter()
                .take_while(|next| next.duration_since(*t) < Duration::from_millis(5))
                .count()
        })
        .max()
        .unwrap_or(0);
    eprintln!(
        "audio_probe seconds={seconds} stall_ms={stall} data={} parity={parity} source_drops={source_drops} interval_us_p50={} p95={} p99={} max={} max_data_in_5ms={max_in_5ms}",
        arrivals.len(),
        quantile(50),
        quantile(95),
        quantile(99),
        intervals.last().unwrap()
    );
    assert!(arrivals.len() > 100, "probe did not exercise the encoder");
}
