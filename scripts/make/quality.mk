.PHONY: test coverage lint

test: ## Run the desktop, harness, Android and spike-tooling tests
	$(CARGO) test --manifest-path desktop/Cargo.toml --workspace
	cd harness && $(CARGO) test --workspace
	$(CARGO) test --manifest-path spikes/f0-signaling/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-media-core/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-injection/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-audio-bridge/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-handshake/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-pipeline/Cargo.toml
	$(CARGO) test --manifest-path spikes/f0-virtual-audio/Cargo.toml
	$(GRADLE) -p mobile :protocol:testDebugUnitTest
	cd spikes/tools && $(PYTHON_BIN) -m pytest tests ../f0-camera-injection ../f0-handshake ../f0-virtual-audio
	GRADLE="$(GRADLE)" bash protocol/vectors/run_conformance.sh

coverage: ## Report coverage for the delivered product packages (≥90% required; spikes/ exempt per T117)
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "error: cargo-llvm-cov is required; install it with 'cargo install cargo-llvm-cov'." >&2; exit 1; }
	mkdir -p .coverage
	$(CARGO) llvm-cov --manifest-path desktop/Cargo.toml --package remotecam-protocol --summary-only > .coverage/rust_protocol_summary.txt
	$(CARGO) llvm-cov --manifest-path desktop/Cargo.toml --package remotecam-session-core --summary-only > .coverage/rust_session_core_summary.txt
	$(CARGO) llvm-cov --manifest-path desktop/Cargo.toml --package remotecam-pairing --summary-only > .coverage/rust_pairing_summary.txt
	cd harness && $(CARGO) llvm-cov --package remotecam-harness --summary-only > ../.coverage/rust_harness_summary.txt
	$(GRADLE) -p mobile :protocol:jacocoTestReport
	python3 scripts/ci/coverage_gate.py \
		--rust-summary .coverage/rust_protocol_summary.txt \
		--rust-session-core-summary .coverage/rust_session_core_summary.txt \
		--rust-pairing-summary .coverage/rust_pairing_summary.txt \
		--rust-harness-summary .coverage/rust_harness_summary.txt \
		--jacoco-report mobile/protocol/build/reports/jacoco/jacocoTestReport/jacocoTestReport.xml

lint: ## Check formatting and run the linters
	$(CARGO) fmt --manifest-path desktop/Cargo.toml --all -- --check
	$(CARGO) clippy --manifest-path desktop/Cargo.toml --workspace --all-targets -- -D warnings
	cd harness && $(CARGO) fmt --all -- --check
	cd harness && $(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-signaling/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-signaling/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-media-core/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-media-core/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-injection/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-injection/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-audio-bridge/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-audio-bridge/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-handshake/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-handshake/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-pipeline/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-pipeline/Cargo.toml --all-targets -- -D warnings
	$(CARGO) fmt --manifest-path spikes/f0-virtual-audio/Cargo.toml -- --check
	$(CARGO) clippy --manifest-path spikes/f0-virtual-audio/Cargo.toml --all-targets -- -D warnings
	cd spikes/tools && $(PYTHON_BIN) -m ruff check .
	cd spikes/tools && $(PYTHON_BIN) -m ruff format --check .
	cd spikes/f0-camera-injection && $(PYTHON_BIN) -m ruff check .
	cd spikes/f0-camera-injection && $(PYTHON_BIN) -m ruff format --check .
	cd spikes/f0-handshake && $(PYTHON_BIN) -m ruff check .
	cd spikes/f0-handshake && $(PYTHON_BIN) -m ruff format --check .
	cd spikes/f0-virtual-audio && $(PYTHON_BIN) -m ruff check .
	cd spikes/f0-virtual-audio && $(PYTHON_BIN) -m ruff format --check .
