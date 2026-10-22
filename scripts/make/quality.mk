.PHONY: test coverage lint

test: ## Run the desktop, mobile and measurement-tooling tests
	$(CARGO) test --manifest-path desktop/Cargo.toml --workspace
	$(GRADLE) -p mobile :protocol:testDebugUnitTest :core:testDebugUnitTest
	cd spikes/tools && $(PYTHON_BIN) -m pytest
	bash protocol/vectors/run_conformance.sh

coverage: ## Report coverage for the delivered contract packages
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "error: cargo-llvm-cov is required; install it with 'cargo install cargo-llvm-cov'." >&2; exit 1; }
	$(CARGO) llvm-cov --manifest-path desktop/Cargo.toml --package remotecam-protocol --summary-only
	$(GRADLE) -p mobile :protocol:jacocoTestReport

lint: ## Check formatting and run the linters
	$(CARGO) fmt --manifest-path desktop/Cargo.toml --all -- --check
	$(CARGO) clippy --manifest-path desktop/Cargo.toml --workspace --all-targets -- -D warnings
	cd spikes/tools && $(PYTHON_BIN) -m ruff check .
	cd spikes/tools && $(PYTHON_BIN) -m ruff format --check .
