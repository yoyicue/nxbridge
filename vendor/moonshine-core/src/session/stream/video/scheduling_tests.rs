use super::*;
use std::time::{Duration, Instant};

fn packetizer() -> Packetizer {
	let (_, keys) = tokio::sync::watch::channel(crate::session::SessionKeyData {
		remote_input_key: vec![],
		remote_input_key_id: 0,
	});
	Packetizer::new(false, keys)
}

fn frame() -> EncodedFrame {
	EncodedFrame {
		queued_at: std::time::Instant::now(),
		data: vec![0x51; 200_000],
		is_key_frame: true,
		frame_number: 1,
		rtp_timestamp: 3000,
	}
}

#[tokio::test]
async fn blocking_worker_preserves_packet_bytes_and_sequence_state() {
	let mut original = packetizer();
	let mut sequence = 0;
	let mut worker = packetizer();
	let mut worker_sequence = 0;
	// Exercise cold and cached matrices without a wall-clock pass criterion.
	for number in 1..=2 {
		let source = EncodedFrame {
			queued_at: std::time::Instant::now(),
			data: vec![0x51; 4096],
			is_key_frame: number == 1,
			frame_number: number,
			rtp_timestamp: number * 3000,
		};
		let expected = original
			.packetize(
				&source.data,
				source.is_key_frame,
				1024,
				2,
				20,
				number,
				&mut sequence,
				number * 3000,
				0,
			)
			.unwrap();
		let (returned, actual, next) = packetize_on_worker(worker, source, 1024, 2, 20, worker_sequence)
			.await
			.unwrap();
		worker = returned;
		worker_sequence = next;
		let actual = actual.unwrap();
		assert_eq!(actual.as_bytes(), expected.as_bytes());
		assert_eq!(actual.shard_size(), expected.shard_size());
		assert_eq!(worker_sequence, sequence);
	}
}

#[tokio::test]
async fn video_handoff_waits_for_completion_and_preserves_reference_order() {
	let (tx, mut rx) = mpsc::channel(1);
	let stop = ShutdownManager::new();
	let task = tokio::spawn(async move {
		let mut p = packetizer();
		let mut seq = 0;
		for number in 1..=3 {
			let batch = p
				.packetize(
					&[number as u8; 100],
					number == 1,
					1024,
					2,
					20,
					number,
					&mut seq,
					number * 3000,
					0,
				)
				.unwrap();
			send_video_batch(&tx, batch, &stop).await.unwrap();
		}
	});
	let mut reference = packetizer();
	let mut seq = 0;
	for number in 1..=3 {
		let batch = tokio::time::timeout(Duration::from_secs(2), rx.recv())
			.await
			.unwrap()
			.unwrap();
		tokio::task::yield_now().await;
		assert!(!task.is_finished());
		assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
		let expected = reference
			.packetize(
				&[number as u8; 100],
				number == 1,
				1024,
				2,
				20,
				number,
				&mut seq,
				number * 3000,
				0,
			)
			.unwrap();
		assert_eq!(batch.batch.as_bytes(), expected.as_bytes());
		batch.sent.send(()).unwrap();
	}
	task.await.unwrap();
}

#[tokio::test]
async fn video_handoff_cancels_while_waiting_for_socket_acknowledgement() {
	let (tx, mut rx) = mpsc::channel(1);
	let stop = ShutdownManager::new();
	let worker_stop = stop.clone();
	let task = tokio::spawn(async move { send_video_batch(&tx, ShardBatch::empty(), &worker_stop).await });
	let held = rx.recv().await.unwrap();
	let _ = stop.trigger_shutdown(SessionShutdownReason::UserStopped);
	assert!(
		tokio::time::timeout(Duration::from_secs(2), task)
			.await
			.unwrap()
			.unwrap()
			.is_err()
	);
	assert!(held.sent.send(()).is_err());
}

#[tokio::test]
async fn packet_handler_acknowledges_empty_batch_without_peer() {
	let socket = super::super::gso_socket::UdpGsoSocket::new("127.0.0.1", 0)
		.await
		.unwrap();
	let (tx, rx) = mpsc::channel(1);
	let start = Arc::new(Notify::new());
	let stop = ShutdownManager::new();
	super::super::spawn_handle_video_packets(rx, socket, start.clone(), stop.clone());
	start.notify_one();
	tokio::time::timeout(
		Duration::from_secs(2),
		send_video_batch(&tx, ShardBatch::empty(), &stop),
	)
	.await
	.unwrap()
	.unwrap();
	let _ = stop.trigger_shutdown(SessionShutdownReason::UserStopped);
}

/// Reproduce an I/O-worker stall using the previous synchronous path as a
/// control. This deliberately uses one async worker; it does not claim that
/// every multi-thread production run experiences the same stall or drop rate.
#[tokio::test]
#[ignore = "wall-clock local datagram/FEC scheduling comparison"]
async fn audio_datagrams_during_cold_video_fec_probe() {
	for inline in [true, false] {
		let (reader, writer) = std::os::unix::net::UnixDatagram::pair().unwrap();
		use std::os::fd::AsRawFd;
		let bytes: libc::c_int = 32 * 1024;
		for (socket, option) in [(&writer, libc::SO_SNDBUF), (&reader, libc::SO_RCVBUF)] {
			assert_eq!(
				unsafe {
					libc::setsockopt(
						socket.as_raw_fd(),
						libc::SOL_SOCKET,
						option,
						(&bytes as *const libc::c_int).cast(),
						std::mem::size_of_val(&bytes) as libc::socklen_t,
					)
				},
				0
			);
		}
		reader.set_nonblocking(true).unwrap();
		writer.set_nonblocking(true).unwrap();
		let reader = tokio::net::UnixDatagram::from_std(reader).unwrap();
		let origin = Instant::now();
		let source = std::thread::spawn(move || {
			let mut full = 0;
			for i in 0..300u64 {
				std::thread::sleep((origin + Duration::from_millis(i * 5)).saturating_duration_since(Instant::now()));
				if let Err(error) = writer.send(&[0u8; 1936]) {
					assert!(
						error.kind() == std::io::ErrorKind::WouldBlock || error.raw_os_error() == Some(libc::ENOBUFS)
					);
					full += 1;
				}
			}
			full
		});
		let video = tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(50)).await;
			let started = Instant::now();
			if inline {
				let mut p = packetizer();
				p.packetize(&frame().data, true, 1024, 2, 20, 1, &mut 0, 3000, 0)
					.unwrap();
			} else {
				packetize_on_worker(packetizer(), frame(), 1024, 2, 20, 0)
					.await
					.unwrap()
					.1
					.unwrap();
			}
			started.elapsed().as_micros()
		});
		let deadline = tokio::time::Instant::now() + Duration::from_millis(1750);
		let mut buf = [0u8; 2048];
		let mut received = 0;
		let mut last = Instant::now();
		let mut gaps = Vec::new();
		while let Ok(Ok(_)) = tokio::time::timeout_at(deadline, reader.recv(&mut buf)).await {
			let now = Instant::now();
			if received > 0 {
				gaps.push(now.duration_since(last).as_micros());
			}
			last = now;
			received += 1;
		}
		let video_us = video.await.unwrap();
		let full = source.join().unwrap();
		gaps.sort_unstable();
		eprintln!(
			"fec_ipc_probe inline={inline} produced=300 received={received} socket_full={full} cold_video_us={video_us} receive_gap_p99_us={} receive_gap_max_us={}",
			gaps[(gaps.len() - 1) * 99 / 100],
			gaps.last().unwrap()
		);
		assert_eq!(received + full, 300);
	}
}
