use crate::{galois, ReedSolomon};

fn cauchy(data: usize, parity: usize) -> Vec<u8> {
    (0..parity).flat_map(|row| (0..data).map(move |col|
        galois::div(1, ((parity + col) ^ row) as u8))).collect()
}

fn check(data: usize, parity: usize, rows: &[u8]) {
    let mut old = ReedSolomon::new(data, parity).unwrap();
    old.set_parity_matrix(rows).unwrap();
    let direct = ReedSolomon::new_with_parity_matrix(data, parity, rows).unwrap();
    assert!(old == direct, "systematic matrix differs for {data}+{parity}");
    let shards: Vec<Vec<u8>> = (0..data + parity).map(|i|
        (0..1037).map(|j| ((i * 31 + j * 7) % 256) as u8).collect()).collect();
    let mut reference = shards.clone();
    let mut actual = shards;
    old.encode(&mut reference).unwrap();
    direct.encode(&mut actual).unwrap();
    assert_eq!(actual, reference);
    let mut missing: Vec<_> = actual.into_iter().map(Some).collect();
    missing[0] = None;
    if data > 1 && parity > 1 { missing[data - 1] = None; }
    // The unmodified decoder must recover new encoder output.
    old.reconstruct(&mut missing).unwrap();
    assert_eq!(missing.into_iter().map(Option::unwrap).collect::<Vec<_>>(), reference);
}

#[test]
fn direct_constructor_matches_old_bytes_and_recovers_losses() {
    for (data, parity) in [(1, 1), (4, 2), (31, 7), (128, 26), (213, 42), (255, 1), (1, 255)] {
        check(data, parity, &cauchy(data, parity));
    }
    check(4, 2, &[0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c]);
}

#[test]
fn direct_constructor_rejects_invalid_shape_without_overflow() {
    assert!(ReedSolomon::new_with_parity_matrix(0, 1, &[]).is_err());
    assert!(ReedSolomon::new_with_parity_matrix(1, 0, &[]).is_err());
    assert!(ReedSolomon::new_with_parity_matrix(usize::MAX, 1, &[]).is_err());
    assert!(ReedSolomon::new_with_parity_matrix(200, 57, &[]).is_err());
    assert!(ReedSolomon::new_with_parity_matrix(4, 2, &[1; 7]).is_err());
}

#[test]
#[ignore = "wall-clock constructor comparison; run with --release --nocapture"]
fn constructor_timing_probe() {
    use std::{hint::black_box, time::Instant};
    for (data, parity) in [(32, 7), (64, 13), (128, 26), (200, 40), (213, 42)] {
        let mut previous = Vec::new(); let mut direct = Vec::new();
        for sample in 0..21 {
            let rows = cauchy(data, parity);
            let started = Instant::now();
            let mut old = ReedSolomon::new(data, parity).unwrap();
            old.set_parity_matrix(&rows).unwrap();
            previous.push(started.elapsed().as_nanos()); black_box(old);
            let started = Instant::now();
            let fast = ReedSolomon::new_with_parity_matrix(data, parity, &rows).unwrap();
            direct.push(started.elapsed().as_nanos()); black_box(fast);
            eprintln!("fec_raw data={data} parity={parity} sample={sample} old_ns={} direct_ns={}", previous.last().unwrap(), direct.last().unwrap());
        }
        previous.sort_unstable(); direct.sort_unstable();
        eprintln!("fec_constructor data={data} parity={parity} old_median_ns={} old_max_ns={} direct_median_ns={} direct_max_ns={}", previous[10], previous[20], direct[10], direct[20]);
    }
}
