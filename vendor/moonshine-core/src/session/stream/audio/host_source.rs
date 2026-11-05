//! PCM source for hosts that capture audio outside this crate (e.g. the
//! ALSA loopback capture on the Anbernic RG SP), replacing the deleted
//! PulseAudio server as the `AudioFrame` producer.
//!
//! Unlike video, Moonshine's own Opus encoder (`encoder.rs`) was never
//! deleted: it still takes raw f32 PCM `AudioFrame`s and does the real
//! Opus encode, CBR, FEC and RTP packetization exactly as upstream wrote
//! it. Only the *producer* — PulseAudio, which mixed application audio —
//! had no replacement once it was removed. This file is that replacement:
//! it converts i16 PCM handed in from the host into `AudioFrame`s and
//! drives the same recycle-pool contract the deleted PulseAudio server
//! did (see `git show f23d52e^:.../pulse_server/mod.rs::clock_tick`,
//! preserved in history as the upstream-blessed reference).
//!
//! Kept in its own file, separate from `mod.rs`, so future `git subtree
//! pull`s of upstream Moonshine never have to merge through it: `mod.rs`
//! only gains a `mod host_source;` line and a call site, both additions
//! upstream cannot also touch.


use async_shutdown::ShutdownManager;
use tokio::sync::mpsc;

use crate::session::manager::SessionShutdownReason;

use super::{AudioFrame, HostAudioChunk, CAPTURE_SAMPLE_RATE};

/// Duration of one Opus frame Moonlight negotiates, in milliseconds.
const FRAME_DURATION_MS: u32 = 5;

/// Number of PCM frames (samples per channel) in one Opus frame. Derived
/// from `CAPTURE_SAMPLE_RATE` rather than hardcoded, so a future change to
/// either constant can't silently desync from the other and mistime frames
/// at the client.
///
/// `pub` (re-exported from `audio/mod.rs`) so the host's capture period can
/// be checked against it at compile time: `rgsp_host::audio::PERIOD_FRAMES`
/// must equal this, or every chunk the host sends is dropped by
/// `chunk_len_is_valid` below — silent audio, no error.
pub const FRAME_FRAMES: usize = (CAPTURE_SAMPLE_RATE / 1000 * FRAME_DURATION_MS) as usize;

/// Convert one PCM chunk into an `AudioFrame`, reusing a buffer from the
/// recycle pool if one is ready, falling back to a stashed `spare_frame` (to
/// avoid losing an allocation from a previous full `frame_tx`), and only
/// allocating fresh if both are exhausted. Mirrors the deleted PulseAudio
/// server's `clock_tick`, whose contract this replaces.
///
/// Normalizes by dividing by `i16::MAX` (32767), the conventional approach:
/// symmetric divisor, no branch. This means `i16::MIN` (-32768) maps to
/// slightly more negative than -1.0 (~-1.0000305) rather than exactly -1.0 —
/// negligible in practice, and matches what most PCM-to-float converters do.
fn build_frame(
	pcm: &[i16],
	frame_recycle_rx: &crossbeam_channel::Receiver<AudioFrame>,
	spare_frame: &mut Option<AudioFrame>,
) -> AudioFrame {
	let mut frame = frame_recycle_rx
		.try_recv()
		.ok()
		.or_else(|| spare_frame.take())
		.unwrap_or(AudioFrame {
			buf: Vec::new(),
			capture_ts_ms: 0, source_pts_ns: 0, discontinuity: false, queued_at: std::time::Instant::now(),
		});

	frame.buf.clear();
	frame.buf.extend(pcm.iter().map(|&s| s as f32 / i16::MAX as f32));
	// Not tracked on this path — `capture_ts_ms` is `#[allow(dead_code)]`
	// downstream (the encoder never reads it), and the host doesn't hand in
	// a capture timestamp for the simple `Vec<i16>` channel this bridges from.
	frame.capture_ts_ms = 0;
	frame.queued_at = std::time::Instant::now();
	frame
}

/// Hand a built frame to the encoder, stashing it as `spare_frame` instead of
/// dropping it if the channel is full — same tradeoff the deleted PulseAudio
/// server made: keep the allocation, drop the (now stale) samples instead.
static DROPPED_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Returns `false` once the encoder has disconnected and the bridge should stop.
fn send_frame(
	frame_tx: &crossbeam_channel::Sender<AudioFrame>,
	frame: AudioFrame,
	spare_frame: &mut Option<AudioFrame>,
) -> (bool, bool) {
	match frame_tx.try_send(frame) {
		Ok(()) => (true, false),
		Err(crossbeam_channel::TrySendError::Full(frame)) => {
			// The encoder is behind and this 5 ms of audio is discarded. That
			// is audible: a dropped frame is a hole in the waveform, and a
			// steady trickle of them is the crackle/stutter a listener
			// reports as "choppy". Count them - silently losing audio with no
			// diagnostic is how this went unnoticed.
			let n = DROPPED_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
			if n == 1 || n % 100 == 0 {
				tracing::warn!("audio encoder behind: {n} PCM frame(s) dropped (5 ms each)");
			}
			*spare_frame = Some(frame);
			(true, true)
		},
		Err(crossbeam_channel::TrySendError::Disconnected(_)) => (false, false),
	}
}

/// Whether a PCM chunk is exactly one Opus frame's worth of samples for the
/// given channel count. A short or long chunk would desync Opus's fixed
/// 5 ms framing, so anything else must be rejected rather than encoded.
///
/// KNOWN LIMITATION: `expected_samples` scales with the *negotiated* channel
/// count, but the host's capture is fixed at stereo. A client that negotiates
/// 5.1 therefore has every chunk rejected here — silent audio, with nothing
/// louder than the warning below to say so. Moonlight defaults to stereo, so
/// this has not bitten; fixing it means upmixing host-side or pinning the
/// negotiation to stereo.
fn chunk_len_is_valid(pcm: &[i16], expected_samples: usize) -> bool {
	pcm.len() == expected_samples
}

/// Bridge host-supplied interleaved i16 PCM into the encoder's `AudioFrame`
/// channel. `channels` sizes the expected chunk length; chunks of any other
/// size are dropped with a warning rather than fed to the encoder (see
/// `chunk_len_is_valid`).
// 5 ms Opus packets retain consecutive network timestamps, including bounded
// silence inserted for a known source gap. Larger gaps re-anchor immediately.
const BLOCK_NS: u64 = 5_000_000;
const MAX_CONCEAL_BLOCKS: u64 = 4;
const PTS_TOLERANCE_NS: u64 = 1_000_000;

#[derive(Default)]
pub struct AudioSourceTimeline {
    epoch: Option<([u8; 16], u64)>,
    last_pts: Option<u64>,
}

impl AudioSourceTimeline {
    // Returns missing 5 ms blocks to conceal and whether to re-anchor.
    pub fn accept(&mut self, chunk: &HostAudioChunk) -> Option<(u64, bool)> {
        let epoch = (chunk.session_id, chunk.generation);
        if chunk.generation == 0 || chunk.session_id == [0; 16] || chunk.pts_ns == 0 {
            return None;
        }
        if self.epoch.is_some_and(|old| old != epoch) { return None; }
        let mut missing = 0;
        let mut resync = false;
        if let Some(last) = self.last_pts {
            let delta = chunk.pts_ns.checked_sub(last)?;
            if delta == 0 { return None; }
            let steps = delta.saturating_add(BLOCK_NS / 2) / BLOCK_NS;
            if steps == 0 { return None; }
            let expected = steps.saturating_mul(BLOCK_NS);
            if delta.abs_diff(expected) > PTS_TOLERANCE_NS && !chunk.discontinuity { return None; }
            if steps > 1 && !chunk.discontinuity { return None; }
            if steps > MAX_CONCEAL_BLOCKS + 1 { resync = true; }
            else { missing = steps - 1; }
            if delta.abs_diff(expected) > PTS_TOLERANCE_NS { resync = true; missing = 0; }
        }
        self.epoch = Some(epoch);
        self.last_pts = Some(chunk.pts_ns);
        Some((missing, resync))
    }
}

/// Validate the source timeline and forward bounded PCM to the encoder, which
/// owns the only output pacer. The upstream channel is bounded and its sender must drop
/// with a discontinuity flag rather than blocking a real-time source.
pub(crate) fn spawn_pcm_bridge(
    mut pcm_rx: mpsc::Receiver<HostAudioChunk>,
    channels: u8,
    frame_tx: crossbeam_channel::Sender<AudioFrame>,
    frame_recycle_rx: crossbeam_channel::Receiver<AudioFrame>,
    start: super::AudioStartGate,
    stop: ShutdownManager<SessionShutdownReason>,
) {
    tokio::spawn(async move {
        if stop.wrap_cancel(start.wait()).await.is_err() { return; }
        let _stop_token = stop.trigger_shutdown_token(SessionShutdownReason::AudioSourceStopped);
        let _delay_stop = stop.delay_shutdown_token();
        let expected_samples = FRAME_FRAMES * channels as usize;
        let mut spare_frame = None;
        let mut timeline = AudioSourceTimeline::default();
        let mut encoder_gap = false;
        let mut stale_drops = 0u64;
        while !stop.is_shutdown_triggered() {
            let mut chunk = match stop.wrap_cancel(pcm_rx.recv()).await {
                Ok(Some(chunk)) => chunk,
                _ => break,
            };
            // Do not replay stale audio after a stall. Retain at most four
            // queued blocks (20 ms); the channel itself is bounded to 20.
            let mut skipped = false;
            while pcm_rx.len() > 3 {
                match pcm_rx.try_recv() {
                    Ok(next) => { chunk = next; skipped = true; stale_drops += 1; }
                    Err(_) => break,
                }
            }
            if skipped {
                chunk.discontinuity = true;
                if stale_drops <= 3 || stale_drops.is_power_of_two() {
                    tracing::warn!("discarded {stale_drops} stale audio block(s)");
                }
            }
            if !chunk_len_is_valid(&chunk.pcm, expected_samples) { continue; }
            let Some((mut missing, mut resync)) = timeline.accept(&chunk) else {
                tracing::warn!("Dropping invalid, out-of-order or foreign-session audio block");
                continue;
            };
            // Concealing a past gap while new audio is already queued would
            // lengthen the backlog and can create a drop/conceal feedback loop.
            if skipped || (missing > 0 && !pcm_rx.is_empty()) {
                missing = 0;
                resync = true;
            }
            for index in 0..=missing {
                let is_silence = index < missing;
                let silence;
                let pcm = if is_silence {
                    silence = vec![0i16; expected_samples];
                    silence.as_slice()
                } else { chunk.pcm.as_slice() };
                let mut frame = build_frame(pcm, &frame_recycle_rx, &mut spare_frame);
                frame.source_pts_ns = chunk.pts_ns.saturating_sub((missing - index) * BLOCK_NS);
                frame.capture_ts_ms = frame.source_pts_ns / 1_000_000;
                frame.discontinuity = encoder_gap || (index == 0 && (chunk.discontinuity || resync));
                let (connected, dropped) = send_frame(&frame_tx, frame, &mut spare_frame);
                encoder_gap = dropped;
                if !connected { return; }
            }
        }
    });
}

#[cfg(test)]
mod tests {
	use super::*;

	fn frame_of(buf: Vec<f32>) -> AudioFrame {
		AudioFrame { buf, capture_ts_ms: 0, source_pts_ns: 0, discontinuity: false, queued_at: std::time::Instant::now() }
	}

    fn chunk(pts_ns: u64, discontinuity: bool) -> HostAudioChunk {
        HostAudioChunk { session_id: [1; 16], generation: 1, pts_ns, discontinuity, pcm: vec![100; 480] }
    }

    #[test]
    fn timeline_rejects_duplicates_reordering_foreign_epoch_and_unmarked_gaps() {
        let mut t = AudioSourceTimeline::default();
        assert_eq!(t.accept(&chunk(1_000_000_000, false)), Some((0, false)));
        assert_eq!(t.accept(&chunk(1_000_000_000, true)), None);
        assert_eq!(t.accept(&chunk(999_000_000, true)), None);
        let mut other = chunk(1_005_000_000, false);
        other.generation = 2;
        assert_eq!(t.accept(&other), None);
        assert_eq!(t.accept(&chunk(1_010_000_000, false)), None);
        assert_eq!(t.accept(&chunk(1_005_000_000, false)), Some((0, false)));
    }

    #[test]
    fn timeline_conceals_small_gaps_and_reanchors_large_or_irregular_gaps() {
        let mut t = AudioSourceTimeline::default();
        assert_eq!(t.accept(&chunk(1_000_000_000, false)), Some((0, false)));
        assert_eq!(t.accept(&chunk(1_015_000_000, true)), Some((2, false)));
        assert_eq!(t.accept(&chunk(2_000_000_000, true)), Some((0, true)));
        assert_eq!(t.accept(&chunk(2_007_000_000, true)), Some((0, true)));
        let mut new_epoch = AudioSourceTimeline::default();
        let mut reset = chunk(10_000_000, false); reset.generation = 2;
        assert_eq!(new_epoch.accept(&reset), Some((0, false)));
    }

    #[tokio::test]
    async fn stale_backlog_is_bounded_and_reanchored_without_inserting_silence() {
        let (tx, rx) = mpsc::channel(20);
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(16);
        let (_recycle_tx, recycle_rx) = crossbeam_channel::bounded(16);
        let gate = super::super::AudioStartGate::new();
        let stop = async_shutdown::ShutdownManager::new();
        spawn_pcm_bridge(rx, 2, frame_tx, recycle_rx, gate.clone(), stop.clone());
        for i in 0..10 { tx.send(chunk(1_000_000_000 + i * BLOCK_NS, false)).await.unwrap(); }
        gate.open();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let frames: Vec<_> = frame_rx.try_iter().collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames.iter().map(|f| f.source_pts_ns).collect::<Vec<_>>(),
                   vec![1_030_000_000, 1_035_000_000, 1_040_000_000, 1_045_000_000]);
        assert!(frames[0].discontinuity);
        assert!(frames.iter().all(|f| f.buf.iter().any(|x| *x != 0.0)));
        drop(tx);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_opus_and_fec_keep_rtp_timestamps_regular_after_gap_and_stall() {
        let (tx, rx) = mpsc::channel(4);
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(16);
        let (recycle_tx, recycle_rx) = crossbeam_channel::bounded(16);
        let (packet_tx, mut packet_rx) = mpsc::channel(16);
        let (_key_tx, key_rx) = tokio::sync::watch::channel(crate::session::SessionKeyData {
            remote_input_key: vec![], remote_input_key_id: 0,
        });
        let gate = super::super::AudioStartGate::new();
        let stop = async_shutdown::ShutdownManager::new();
        super::super::encoder::AudioEncoder::spawn(
            48000, &super::super::OPUS_STEREO, frame_rx, recycle_tx, key_rx,
            false, packet_tx, stop.clone(), gate.clone(),
        ).unwrap();
        spawn_pcm_bridge(rx, 2, frame_tx, recycle_rx, gate.clone(), stop.clone());
        gate.open();
        // Keep a live source after the injected gap. The encoder deliberately
        // expires PCM after 20 ms: on a busy runner, four startup blocks need
        // not all survive. Exact concealment is tested below without an encoder;
        // this test checks RTP/FEC continuity with the real expiration policy.
        let producer_stop = stop.clone();
        let producer = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            for index in 0u64.. {
                if producer_stop.wrap_cancel(interval.tick()).await.is_err() { break; }
                let steps = if index == 0 { 0 } else { index + 2 };
                let next = chunk(1_000_000_000 + steps * BLOCK_NS, index == 1);
                match producer_stop.wrap_cancel(tx.send(next)).await {
                    Ok(Ok(())) => {},
                    _ => break,
                }
            }
        });
        let mut data_timestamps = Vec::new();
        let mut parity = 0;
        for batch_index in 0..4 {
            let batch = tokio::time::timeout(std::time::Duration::from_secs(5), packet_rx.recv())
                .await.unwrap().unwrap();
            for pkt in &batch.packets {
                assert!(pkt.len() > 12);
                match pkt[1] {
                    97 => data_timestamps.push(u32::from_be_bytes(pkt[4..8].try_into().unwrap())),
                    127 => parity += 1,
                    other => panic!("unexpected RTP type {other}"),
                }
            }
            // Force PCM expiration behind one held send completion as well as
            // the source timestamp gap, without changing production deadlines.
            if batch_index == 0 { tokio::time::sleep(std::time::Duration::from_millis(40)).await; }
            batch.sent.send(()).unwrap();
        }
        assert_eq!(data_timestamps, vec![0, 5, 10, 15]);
        assert_eq!(parity, 2);
        let _ = stop.trigger_shutdown(SessionShutdownReason::UserStopped);
        producer.await.unwrap();
    }

    #[tokio::test]
    async fn bridge_conceals_two_blocks_and_stops_when_sender_disconnects() {
        let (tx, rx) = mpsc::channel(4);
        let (frame_tx, frame_rx) = crossbeam_channel::bounded(16);
        let (_recycle_tx, recycle_rx) = crossbeam_channel::bounded(16);
        let gate = super::super::AudioStartGate::new();
        let stop = async_shutdown::ShutdownManager::new();
        spawn_pcm_bridge(rx, 2, frame_tx, recycle_rx, gate.clone(), stop.clone());
        gate.open();
        tx.send(chunk(1_000_000_000, false)).await.unwrap();
        tx.send(chunk(1_015_000_000, true)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        let frames: Vec<_> = frame_rx.try_iter().collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames.iter().map(|f| f.source_pts_ns).collect::<Vec<_>>(),
                   vec![1_000_000_000, 1_005_000_000, 1_010_000_000, 1_015_000_000]);
        assert!(frames[1].buf.iter().all(|x| *x == 0.0));
        assert!(frames[2].buf.iter().all(|x| *x == 0.0));
        assert!(frames[1].discontinuity);
        assert!(!frames[3].buf.iter().all(|x| *x == 0.0));
        drop(tx);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(stop.is_shutdown_triggered());
    }

	#[test]
	fn frame_frames_is_five_ms_derived_from_capture_rate() {
		assert_eq!(
			FRAME_FRAMES,
			(CAPTURE_SAMPLE_RATE / 1000 * FRAME_DURATION_MS) as usize,
			"must be derived from the vendored constants, not a bare literal"
		);
		assert_eq!(FRAME_FRAMES, 240, "Moonlight negotiates 5 ms Opus frames at 48 kHz");
	}

	#[test]
	fn chunk_len_is_valid_accepts_exact_length() {
		assert!(chunk_len_is_valid(&[0i16; 480], 480));
	}

	#[test]
	fn chunk_len_is_valid_rejects_short_chunk() {
		assert!(!chunk_len_is_valid(&[0i16; 479], 480));
	}

	#[test]
	fn chunk_len_is_valid_rejects_long_chunk() {
		assert!(!chunk_len_is_valid(&[0i16; 481], 480));
	}

	#[test]
	fn chunk_len_is_valid_rejects_empty_slice() {
		assert!(!chunk_len_is_valid(&[], 480));
	}

	#[test]
	fn converts_i16_to_normalized_f32() {
		let (_recycle_tx, recycle_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		let mut spare = None;
		let pcm = [i16::MAX, i16::MIN, 0];
		let frame = build_frame(&pcm, &recycle_rx, &mut spare);

		assert_eq!(frame.buf.len(), pcm.len());
		assert!((frame.buf[0] - 1.0).abs() < 1e-6, "max i16 should normalize to ~1.0");
		assert!((frame.buf[1] - (-1.0)).abs() < 1e-4, "min i16 should normalize to ~-1.0 (see build_frame's doc comment)");
		assert_eq!(frame.buf[2], 0.0);
	}

	#[test]
	fn reuses_a_recycled_buffer_before_allocating() {
		let (recycle_tx, recycle_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		recycle_tx.send(frame_of(vec![9.0; 8])).unwrap();
		let mut spare = None;
		let pcm = [1i16, 2, 3, 4];

		let frame = build_frame(&pcm, &recycle_rx, &mut spare);

		assert_eq!(frame.buf.len(), 4, "stale recycled contents must not leak into the new frame");
		assert!(recycle_rx.try_recv().is_err(), "the recycled buffer should have been drained");
	}

	#[test]
	fn falls_back_to_spare_frame_when_recycle_pool_is_empty() {
		let (_recycle_tx, recycle_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		let mut spare = Some(frame_of(vec![0.0; 100]));
		let pcm = [5i16, 6];

		let frame = build_frame(&pcm, &recycle_rx, &mut spare);

		assert_eq!(frame.buf.len(), 2);
		assert!(spare.is_none(), "the spare frame should be consumed, not left stashed");
	}

	#[test]
	fn allocates_fresh_when_both_recycle_pool_and_spare_are_empty() {
		let (_recycle_tx, recycle_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		let mut spare = None;
		let pcm = [1i16, 2, 3];

		let frame = build_frame(&pcm, &recycle_rx, &mut spare);

		assert_eq!(frame.buf.len(), 3);
	}

	#[test]
	fn stashes_frame_as_spare_when_encoder_channel_is_full() {
		let (frame_tx, frame_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		frame_tx.try_send(frame_of(vec![0.0; 1])).unwrap();
		let mut spare = None;

		let ok = send_frame(&frame_tx, frame_of(vec![1.0, 2.0]), &mut spare);

		assert_eq!(ok, (true, true), "a full encoder channel drops the frame");
		assert!(spare.is_some(), "the frame's allocation should be stashed rather than dropped");
		assert_eq!(spare.unwrap().buf, vec![1.0, 2.0]);
		drop(frame_rx);
	}

	#[test]
	fn stops_when_encoder_channel_is_disconnected() {
		let (frame_tx, frame_rx) = crossbeam_channel::bounded::<AudioFrame>(1);
		drop(frame_rx);
		let mut spare = None;

		let ok = send_frame(&frame_tx, frame_of(vec![1.0]), &mut spare);

		assert_eq!(ok, (false, false), "a disconnected encoder channel must stop the bridge");
	}
}
