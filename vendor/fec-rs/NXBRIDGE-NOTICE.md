# fec-rs provenance and local change

This directory contains the crates.io `fec-rs` 0.1.0 package from
[hgaiser/fec-rs](https://github.com/hgaiser/fec-rs), retaining its BSD-2-Clause
license in `LICENSE`. The registry package SHA-256 is
`7a86d2bb0db9647ee5dece2ea54a808c14428db009ad751f95b091598835cd0b`.
This identifies the source package, not a mutable repository branch.
The package's `.cargo_vcs_info.json` identifies upstream revision
`402f41f970bb8c3ef7c88639821032e3b6460f63`.

NXBridge adds `ReedSolomon::new_with_parity_matrix` to construct the systematic
identity/data rows and caller-supplied parity rows directly. The original
constructor, SIMD encoding, parallel threshold, and recovery algorithms remain
unchanged. Tests compare the new constructor with the original constructor plus
`set_parity_matrix`, including missing-shard recovery and audio's fixed matrix.
Both the workspace and standalone protocol tests patch the same local package.
