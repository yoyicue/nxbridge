use nxbridge_media::*;
use std::mem::{offset_of, size_of};

fn epoch() -> Epoch {
    Epoch {
        session_id: [7; 16],
        generation: 1,
    }
}
fn frame(seq: u64, buffer_id: u64) -> VideoFrame {
    VideoFrame {
        struct_size: size_of::<VideoFrame>() as u32,
        abi_version: ABI_V1,
        epoch: epoch(),
        sequence: seq,
        pts_ns: seq * 33_333_333,
        duration_ns: 33_333_333,
        buffer_id,
        pixel_format: VIDEO_BGRA8,
        width: 1280,
        height: 720,
        color_primaries: COLOR_BT709,
        transfer: TRANSFER_BT709,
        matrix: MATRIX_RGB,
        color_range: RANGE_FULL,
        plane_count: 1,
        ready_kind: READY_CPU_COMPLETE,
        reserved: [0; 3],
    }
}
fn audio(seq: u64, sample: u64, id: u64) -> AudioChunk {
    AudioChunk {
        struct_size: size_of::<AudioChunk>() as u32,
        abi_version: ABI_V1,
        epoch: epoch(),
        sequence: seq,
        pts_ns: sample * 1_000_000_000 / 48_000,
        first_sample: sample,
        buffer_id: id,
        sample_format: AUDIO_F32LE_INTERLEAVED,
        sample_rate: AUDIO_RATE,
        channels: AUDIO_CHANNELS,
        frames: 480,
        flags: 0,
        reserved: 0,
    }
}
#[test]
fn rust_abi_matches_c_sizes_and_offsets() {
    assert_eq!(
        (
            size_of::<Epoch>(),
            size_of::<Plane>(),
            size_of::<VideoBuffer>(),
            size_of::<VideoFrame>(),
            size_of::<AudioBuffer>(),
            size_of::<AudioChunk>(),
            size_of::<VideoRelease>()
        ),
        (24, 24, 120, 112, 56, 88, 56)
    );
    assert_eq!(
        (
            offset_of!(VideoBuffer, planes),
            offset_of!(VideoFrame, sequence),
            offset_of!(AudioChunk, first_sample),
            offset_of!(VideoRelease, disposition)
        ),
        (72, 32, 48, 48)
    );
}
#[test]
fn video_slot_lifetime_and_epoch_isolation() {
    let mut q = VideoLeases::new(epoch()).unwrap();
    for i in 1..=3 {
        assert_eq!(q.publish(frame(i, i)), Ok(None));
    }
    let dropped = q.publish(frame(4, 4)).unwrap().unwrap();
    assert_eq!(
        (dropped.sequence, dropped.buffer_id, dropped.disposition),
        (1, 1, RELEASE_DROPPED)
    );
    assert_eq!(q.publish(frame(5, 2)), Err(Error::BufferBusy));
    let a = q.begin_encode().unwrap();
    let b = q.begin_encode().unwrap();
    assert!(q.begin_encode().is_none());
    assert_eq!(
        q.complete(
            Epoch {
                generation: 2,
                ..epoch()
            },
            a.buffer_id,
            a.sequence,
            RELEASE_ENCODED
        ),
        Err(Error::WrongEpoch)
    );
    assert_eq!(
        q.complete(epoch(), a.buffer_id, a.sequence + 1, RELEASE_ENCODED),
        Err(Error::UnknownLease)
    );
    let pending = q.close();
    assert_eq!(pending.len(), 1);
    assert!(!q.drained());
    assert_eq!(q.publish(frame(6, 6)), Err(Error::Closing));
    q.complete(epoch(), a.buffer_id, a.sequence, RELEASE_ENCODED)
        .unwrap();
    assert!(!q.drained());
    q.complete(epoch(), b.buffer_id, b.sequence, RELEASE_FAILED)
        .unwrap();
    assert!(q.drained());
}
#[test]
fn buffers_validate_layout_and_gpu_completion_source() {
    let f = frame(1, 1);
    let mut b = VideoBuffer {
        struct_size: size_of::<VideoBuffer>() as u32,
        abi_version: ABI_V1,
        epoch: epoch(),
        buffer_id: 1,
        allocation_bytes: 1280 * 720 * 4,
        storage_kind: STORAGE_CPU_SHARED,
        pixel_format: VIDEO_BGRA8,
        width: 1280,
        height: 720,
        plane_count: 1,
        reserved: 0,
        planes: [
            Plane {
                offset_bytes: 0,
                length_bytes: 1280 * 720 * 4,
                stride_bytes: 1280 * 4,
                reserved: 0,
            },
            Plane::default(),
        ],
    };
    assert_eq!(b.validate_for(&f), Ok(()));
    let mut gpu = f;
    gpu.ready_kind = READY_GPU_COMPLETE;
    assert_eq!(b.validate_for(&gpu), Err(Error::Invalid));
    b.storage_kind = STORAGE_IOSURFACE;
    assert_eq!(b.validate_for(&gpu), Ok(()));
    b.planes[0].length_bytes -= 1;
    assert_eq!(b.validate(), Err(Error::Invalid));
}
#[test]
fn audio_capacity_sample_clock_and_release() {
    let mut q = AudioLeases::new(epoch()).unwrap();
    for i in 0..20 {
        q.publish(audio(i + 1, i * 480, i + 1)).unwrap();
    }
    assert_eq!(q.publish(audio(21, 20 * 480, 21)), Err(Error::Capacity));
    let a = q.begin_consume().unwrap();
    assert!(q.begin_consume().is_none());
    assert_eq!(
        q.complete(epoch(), a.buffer_id, a.sequence + 1, RELEASE_ENCODED),
        Err(Error::UnknownLease)
    );
    q.complete(epoch(), a.buffer_id, a.sequence, RELEASE_ENCODED)
        .unwrap();
    let mut c = audio(21, 20 * 480, 21);
    c.pts_ns += 1;
    assert_eq!(q.publish(c), Err(Error::OutOfOrder));
    c.pts_ns -= 1;
    q.publish(c).unwrap();
    assert_eq!(q.close().len(), 20);
    assert!(q.drained());
}
#[test]
fn audio_gap_requires_discontinuity() {
    let mut q = AudioLeases::new(epoch()).unwrap();
    q.publish(audio(1, 0, 1)).unwrap();
    let mut c = audio(2, 960, 2);
    assert_eq!(q.publish(c), Err(Error::OutOfOrder));
    c.flags = AUDIO_DISCONTINUITY;
    assert_eq!(q.publish(c), Ok(()));
    q.publish(audio(3, 1440, 3)).unwrap();
}

#[test]
fn audio_sequence_gap_requires_discontinuity_even_if_samples_are_contiguous() {
    let mut q = AudioLeases::new(epoch()).unwrap();
    q.publish(audio(1, 0, 1)).unwrap();
    let mut next = audio(3, 480, 2);
    assert_eq!(q.publish(next), Err(Error::OutOfOrder));

    // A rejected block cannot advance the timeline or take ownership.
    q.publish(audio(2, 480, 2)).unwrap();
    next = audio(4, 960, 3);
    next.flags = AUDIO_DISCONTINUITY;
    q.publish(next).unwrap();
    q.publish(audio(5, 1440, 4)).unwrap();
}
