PYTHON ?= python3

.PHONY: check check-rust check-native-media check-audio-bridge check-video-bridge check-release check-release-content check-synthetic check-clean-source preflight help

check:
	$(PYTHON) scripts/check_project.py

check-rust:
	cargo check --workspace --all-targets --offline

check-native-media:
	sh scripts/check-native-media.sh

check-audio-bridge:
	cargo test --manifest-path vendor/moonshine-core/Cargo.toml --locked session::stream::audio
	cargo test --manifest-path vendor/moonshine-core/Cargo.toml --locked blocking_worker_preserves_packet_bytes_and_sequence_state

check-video-bridge:
	cargo test --manifest-path vendor/fec-rs/Cargo.toml --locked --features parallel
	cargo test --manifest-path vendor/moonshine-core/Cargo.toml --locked session::stream::video

check-synthetic:
	$(PYTHON) scripts/synthetic-selfcheck.py

check-clean-source:
	$(PYTHON) scripts/check-clean-source.py

check-release:
	$(PYTHON) scripts/check_release.py

check-release-content:
	$(PYTHON) scripts/check_release.py --content-only

preflight:
	$(PYTHON) scripts/preflight.py

help:
	@echo "make check      Validate documentation and project metadata"
	@echo "make check-rust Offline Rust workspace compile check"
	@echo "make check-native-media Run device-free macOS media regression tests"
	@echo "make check-audio-bridge Test timestamped audio, Opus and FEC"
	@echo "make check-video-bridge Test FEC compatibility and bounded video handoff"
	@echo "make check-synthetic Run local synthetic media encode/decode"
	@echo "make check-clean-source Build and self-check an isolated source snapshot"
	@echo "make check-release Check candidate files and require a root license"
	@echo "make check-release-content Scan candidate files without requiring a root license"
	@echo "make preflight  Read-only host inventory; outputs JSON"
