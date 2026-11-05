use nxbridge_stream::annexb::{AnnexBError, AnnexBSplitter};

#[test]
fn splits_arbitrary_pipe_boundaries_and_identifies_keyframes() {
    let bytes = [
        0, 0, 0, 1, 9, 0xf0, 0, 0, 0, 1, 7, 1, 0, 0, 1, 5, 2, 0, 0, 0, 1, 9, 0xf0, 0, 0, 1, 1, 3,
    ];
    for chunk_size in 1..=bytes.len() {
        let mut splitter = AnnexBSplitter::new();
        let mut units = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            units.extend(splitter.push(chunk).unwrap());
        }
        units.extend(splitter.finish().unwrap());
        assert_eq!(units.len(), 2);
        assert!(units[0].is_key_frame);
        assert!(!units[1].is_key_frame);
        assert_eq!(
            units.into_iter().flat_map(|u| u.data).collect::<Vec<_>>(),
            bytes
        );
    }
}

#[test]
fn rejects_missing_aud_and_oversized_reads() {
    let mut s = AnnexBSplitter::new();
    assert_eq!(
        s.push(&[0, 0, 1, 5, 42]),
        Err(AnnexBError::MissingDelimiter)
    );
    let mut s = AnnexBSplitter::new();
    assert_eq!(
        s.push(&vec![0; 64 * 1024 + 1]),
        Err(AnnexBError::AccessUnitTooLarge)
    );
    assert_eq!(s.finish(), Ok(None));
}
