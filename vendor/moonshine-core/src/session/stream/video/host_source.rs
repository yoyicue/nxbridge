//! Frame source for hosts that encode outside this crate (e.g. the Cedar
//! video engine on the Anbernic RG SP), replacing the deleted Vulkan
//! `VideoPipeline`.
//!
//! Kept in its own file, separate from `mod.rs`, so future `git subtree
//! pull`s of upstream Moonshine never have to merge through it: `mod.rs`
//! only gains a `mod host_source;` line and a call site, both additions
//! upstream cannot also touch.

use std::sync::Arc;

use async_shutdown::ShutdownManager;
use tokio::sync::{Notify, broadcast, mpsc, oneshot};

use crate::session::SessionKeysReceiver;
use crate::session::manager::SessionShutdownReason;

use super::packetizer::Packetizer;
use super::shard_batch::ShardBatch;
use super::{VideoPacketBatch, VideoStreamConfig, VideoStreamContext};

/// One encoded video frame, produced outside the crate by the host's
/// hardware encoder and fed into `VideoStream::start`'s `frame_rx` for
/// packetizing.
///
/// Replaces the deleted Vulkan `VideoPipeline`'s `ExportedFrame` as the
/// frame source: the host now supplies pre-encoded bytes directly instead
/// of exporting DMA-BUFs for GPU encode.
pub struct EncodedFrame {
	/// Local handoff time, used only for queue diagnostics (never a media clock).
	pub queued_at: std::time::Instant,
	pub data: Vec<u8>,
	pub is_key_frame: bool,
	pub frame_number: u32,
	pub rtp_timestamp: u32,
}

/// A client recovery request, mapped from Moonlight's three independent
/// signals (IDR, reference-frame invalidation, stream reset) into one
/// channel for the host's encoder to consume.
///
/// Collapsed to one enum instead of exposing the three raw broadcast
/// channels `VideoStreamHandle` uses internally, because the Cedar video
/// engine (the only encoder this project drives) can't act on them
/// differently anyway — see the variant docs below.
#[derive(Debug, Clone, Copy)]
pub enum EncoderControl {
	/// Client reported it can't decode; encode the next frame as a
	/// keyframe. Maps directly to `Capture::request_idr()`.
	Idr,
	/// Client reported the inclusive `[first, last]` frame-index range it
	/// couldn't decode. Upstream's Vulkan encoder could drop just the
	/// affected reference frames and predict from a surviving one; the C
	/// capture layer has no equivalent API, so this degrades to a full IDR
	/// — more expensive than upstream's cheap path, but correct.
	Invalidate { first: u32, last: u32 },
	/// A client is resuming an already-running session. The host's frame
	/// counter (owned by `rgsp_host::video::VideoStream`, not this crate)
	/// must restart from zero, and the resumed client needs a decodable
	/// starting frame — so this also implies an IDR.
	Reset,
}

/// Packetize each `EncodedFrame` from the host's encoder into `ShardBatch`es
/// and forward them to the packet handler; also the sole consumer of the
/// IDR / reference-invalidation / reset broadcast channels, mapping each to
/// an `EncoderControl` and forwarding it to the host via `control_tx`.
///
/// This is a minimal replacement for the deleted Vulkan `VideoPipeline`'s
/// encode+packetize loop, covering only packetizing and control-request
/// relay: the host now supplies pre-encoded frames, so there is no GPU
/// encode, HDR metadata, or per-frame latency stats to produce here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_packetize_frames(
	mut frame_rx: mpsc::Receiver<EncodedFrame>,
	config: VideoStreamConfig,
	context: VideoStreamContext,
	keys_rx: SessionKeysReceiver,
	packet_tx: mpsc::Sender<VideoPacketBatch>,
	mut idr_rx: broadcast::Receiver<()>,
	mut invalidate_rx: broadcast::Receiver<(u32, u32)>,
	mut reset_rx: broadcast::Receiver<()>,
	control_tx: mpsc::Sender<EncoderControl>,
	start: Arc<Notify>,
	stop_session_manager: ShutdownManager<SessionShutdownReason>,
) {
	tokio::spawn(async move {
		if stop_session_manager.wrap_cancel(start.notified()).await.is_err() {
			return;
		}

		// FEC cache construction and packetization are CPU work. In particular,
		// a cold large-keyframe matrix must not block an async I/O worker that
		// also drains 5 ms audio datagrams. Only one blocking job is outstanding
		// per video stream; the existing bounded frame channel keeps order.
		// Direct construction is cheap and the bounded cache is shared. Avoid
		// pre-filling shapes that this stream may never use.
		let mut packetizer = Packetizer::new(context.encrypt_video, keys_rx);
		let mut sequence_number: u32 = 0;
		let mut reported_at = std::time::Instant::now();
		let mut frames = 0u64;
		let (mut queue_max, mut packetize_max, mut handoff_max) = (
			std::time::Duration::ZERO,
			std::time::Duration::ZERO,
			std::time::Duration::ZERO,
		);

		// Trigger session shutdown if we exit unexpectedly.
		let _stop_token = stop_session_manager.trigger_shutdown_token(SessionShutdownReason::VideoEncoderStopped);
		let _delay_stop = stop_session_manager.delay_shutdown_token();

		while !stop_session_manager.is_shutdown_triggered() {
			tokio::select! {
				frame = stop_session_manager.wrap_cancel(frame_rx.recv()) => {
					let frame = match frame {
						Ok(Some(frame)) => frame,
						Ok(None) => {
							tracing::debug!("Encoded frame channel closed.");
							break;
						},
						Err(_) => break,
					};

					queue_max = queue_max.max(frame.queued_at.elapsed());
					let packetize_started = std::time::Instant::now();
					let result = stop_session_manager.wrap_cancel(packetize_on_worker(
						packetizer, frame, context.packet_size,
						context.minimum_fec_packets, config.fec_percentage, sequence_number,
					)).await;
					let (returned, batch, next_sequence) = match result {
						Ok(Ok(value)) => value,
						Ok(Err(error)) => {
							tracing::warn!("Video packetization worker failed: {error}");
							break;
						},
						Err(_) => break,
					};
					packetizer = returned;
					packetize_max = packetize_max.max(packetize_started.elapsed());
					sequence_number = next_sequence;
					let batch = match batch {
						Ok(batch) => batch,
						Err(()) => continue,
					};

					let handoff_started = std::time::Instant::now();
					if send_video_batch(&packet_tx, batch, &stop_session_manager).await.is_err() {
						tracing::debug!("Packet handler stopped, exiting packetize loop.");
						break;
					}
					handoff_max = handoff_max.max(handoff_started.elapsed());
					frames += 1;
					if reported_at.elapsed() >= std::time::Duration::from_secs(10) {
						tracing::info!(frames, queue_max_ms = queue_max.as_secs_f64() * 1000.0,
							packetize_max_ms = packetize_max.as_secs_f64() * 1000.0,
							handoff_max_ms = handoff_max.as_secs_f64() * 1000.0,
							"Video pipeline window (local socket handoff, not client display)");
						reported_at = std::time::Instant::now();
						frames = 0;
						queue_max = std::time::Duration::ZERO;
						packetize_max = std::time::Duration::ZERO;
						handoff_max = std::time::Duration::ZERO;
					}
				},

				idr = idr_rx.recv() => {
					match idr {
						Ok(()) => forward_control(&control_tx, EncoderControl::Idr),
						Err(broadcast::error::RecvError::Lagged(n)) => {
							tracing::warn!("Missed {n} IDR request(s) while lagging; forwarding one now.");
							forward_control(&control_tx, EncoderControl::Idr);
						},
						Err(broadcast::error::RecvError::Closed) => {},
					}
				},

				invalidate = invalidate_rx.recv() => {
					match invalidate {
						Ok((first, last)) => forward_control(&control_tx, EncoderControl::Invalidate { first, last }),
						Err(broadcast::error::RecvError::Lagged(n)) => {
							tracing::warn!(
								"Missed {n} reference-invalidation request(s) while lagging; forcing an IDR."
							);
							forward_control(&control_tx, EncoderControl::Idr);
						},
						Err(broadcast::error::RecvError::Closed) => {},
					}
				},

				reset = reset_rx.recv() => {
					match reset {
						Ok(()) => forward_control(&control_tx, EncoderControl::Reset),
						Err(broadcast::error::RecvError::Lagged(n)) => {
							tracing::warn!("Missed {n} reset request(s) while lagging; forwarding one now.");
							forward_control(&control_tx, EncoderControl::Reset);
						},
						Err(broadcast::error::RecvError::Closed) => {},
					}
				},
			}
		}

		tracing::debug!("Video packetize loop stopped.");
	});
}

/// Keep one batch outstanding across enqueue and socket transmission. Cancellation
/// drops the acknowledgement, without replaying frames into a different session.
async fn send_video_batch(
	packet_tx: &mpsc::Sender<VideoPacketBatch>,
	batch: ShardBatch,
	stop: &ShutdownManager<SessionShutdownReason>,
) -> Result<(), ()> {
	let (sent, received) = oneshot::channel();
	stop.wrap_cancel(packet_tx.send(VideoPacketBatch { batch, sent }))
		.await
		.map_err(|_| ())?
		.map_err(|_| ())?;
	stop.wrap_cancel(received).await.map_err(|_| ())?.map_err(|_| ())
}

/// Best-effort forward: a full or not-yet-consumed control channel (no host
/// consumer wired up until Task 10) must not block or kill the packetize
/// loop — packetizing real frames takes priority over control-request
/// delivery.
fn forward_control(control_tx: &mpsc::Sender<EncoderControl>, control: EncoderControl) {
	if let Err(e) = control_tx.try_send(control) {
		tracing::debug!("Dropping encoder control signal {control:?}: {e}");
	}
}

/// Own the packetizer across an awaited CPU job, retaining its FEC/encryption
/// state and packet order without blocking the runtime's network workers.
async fn packetize_on_worker(
	mut packetizer: Packetizer,
	frame: EncodedFrame,
	packet_size: usize,
	minimum_fec_packets: u32,
	fec_percentage: u8,
	mut sequence: u32,
) -> Result<(Packetizer, Result<ShardBatch, ()>, u32), tokio::task::JoinError> {
	tokio::task::spawn_blocking(move || {
		let batch = packetizer.packetize(
			&frame.data,
			frame.is_key_frame,
			packet_size,
			minimum_fec_packets,
			fec_percentage,
			frame.frame_number,
			&mut sequence,
			frame.rtp_timestamp,
			0,
		);
		(packetizer, batch, sequence)
	})
	.await
}

#[cfg(test)]
#[path = "scheduling_tests.rs"]
mod scheduling_tests;
