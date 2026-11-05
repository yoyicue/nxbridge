use nxbridge_media::{AudioLeases, Epoch, VideoLeases, RELEASE_ENCODED};
use nxbridge_stream::synthetic::SyntheticSource;

#[test]
fn moving_counted_frames_and_audio_share_epoch_and_clock() {
    let epoch = Epoch {
        session_id: [42; 16],
        generation: 1,
    };
    let source = SyntheticSource::new(epoch, 7_000_000_000, 320, 180).unwrap();
    let f0 = source.video(0, 1).unwrap();
    let f1 = source.video(1, 2).unwrap();
    let f30 = source.video(30, 3).unwrap();
    assert_eq!(f0.buffer.validate_for(&f0.descriptor), Ok(()));
    assert_eq!(f1.buffer.validate_for(&f1.descriptor), Ok(()));
    assert_ne!(f0.pixels, f1.pixels);
    assert_eq!(f30.descriptor.pts_ns - f0.descriptor.pts_ns, 1_000_000_000);
    assert_eq!(f0.descriptor.sequence, 1);
    assert_eq!(f30.descriptor.sequence, 31);
    let pulse = source.audio(0, 1).unwrap();
    let silence = source.audio(10, 2).unwrap();
    let pulse2 = source.audio(100, 3).unwrap();
    assert_eq!(pulse.buffer.validate_for(&pulse.descriptor), Ok(()));
    assert!(pulse.pcm_le.iter().any(|x| *x != 0));
    assert!(silence.pcm_le.iter().all(|x| *x == 0));
    assert_eq!(
        pulse2.descriptor.pts_ns - f0.descriptor.pts_ns,
        1_000_000_000
    );
    let mut v = VideoLeases::new(epoch).unwrap();
    v.publish(f0.descriptor).unwrap();
    let in_flight = v.begin_encode().unwrap();
    v.complete(
        epoch,
        in_flight.buffer_id,
        in_flight.sequence,
        RELEASE_ENCODED,
    )
    .unwrap();
    let mut a = AudioLeases::new(epoch).unwrap();
    a.publish(pulse.descriptor).unwrap();
    let c = a.begin_consume().unwrap();
    a.complete(epoch, c.buffer_id, c.sequence, RELEASE_ENCODED)
        .unwrap();
}
#[test]
fn dimensions_indices_and_buffer_ids_are_checked() {
    let epoch = Epoch {
        session_id: [42; 16],
        generation: 1,
    };
    assert!(SyntheticSource::new(epoch, 0, 175, 90).is_err());
    let s = SyntheticSource::new(epoch, 0, 176, 90).unwrap();
    assert!(s.video(0, 0).is_err());
    assert!(s.video(u64::MAX, 1).is_err());
    assert!(s.audio(u64::MAX, 1).is_err());
    assert_eq!(s.video(0, 1).unwrap().pixels.len(), 176 * 90 * 4);
}
