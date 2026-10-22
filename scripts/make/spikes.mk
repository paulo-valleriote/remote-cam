PIPELINE_MANIFEST ?= spikes/f0-pipeline/Cargo.toml
CONFORMANCE_SCRIPT ?= protocol/vectors/run_conformance.sh

.PHONY: preflight selftest validate-evidence conformance generate-fixtures \
	spike-handshake spike-pipeline spike-synthetic-source \
	spike-latency spike-loss spike-resources spike-drift spike-stall \
	spike-injection spike-interop spike-audio gate-report

preflight: ## Record the reference environment and verify hardware decode
	$(call need_script,spikes/tools/preflight.py)
	$(PYTHON) spikes/tools/preflight.py

selftest: ## Run the measurement toolchain self-test
	$(call need_script,spikes/tools/selftest.py)
	$(PYTHON) spikes/tools/selftest.py

validate-evidence: ## Validate the recorded evidence against the contracts
	$(call need_script,spikes/tools/validation/validate_evidence.py)
	$(PYTHON) spikes/tools/validation/validate_evidence.py --evidence-root spikes/evidence

conformance: ## Run the protocol vectors against the Rust and Kotlin implementations
	$(call need_script,$(CONFORMANCE_SCRIPT))
	bash $(CONFORMANCE_SCRIPT)

generate-fixtures: ## Regenerate the committed synthetic fixtures
	$(PYTHON) spikes/fixtures/tools/generate_calibration.py

spike-handshake: ## Run the pairing, throttling and residue scenarios
	$(call need_script,spikes/f0-handshake/Cargo.toml)
	$(CARGO) run --manifest-path spikes/f0-handshake/Cargo.toml

spike-pipeline: ## Run the full desktop receive pipeline scenario matrix
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST)

spike-synthetic-source: ## Run the timestamped synthetic media source
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) --bin synthetic-source

spike-latency: ## Run one latency measurement run
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- latency

spike-loss: ## Run the pinned loss-and-jitter measurement
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- loss

spike-resources: ## Run the resource measurement
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- resources

spike-drift: ## Run the drift measurement
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- drift

spike-stall: ## Run the media stall recovery scenario
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- stall

spike-injection: ## Run the virtual camera injection matrix
	$(call need_script,spikes/f0-camera-injection/run_matrix.py)
	$(PYTHON) spikes/f0-camera-injection/run_matrix.py

spike-interop: ## Run the codec interoperability scenario
	$(call need_script,spikes/f0-codec-interop/Cargo.toml)
	$(CARGO) run --manifest-path spikes/f0-codec-interop/Cargo.toml

spike-audio: ## Run the virtual audio scenario
	$(call need_script,spikes/f0-virtual-audio/Cargo.toml)
	$(CARGO) run --manifest-path spikes/f0-virtual-audio/Cargo.toml

gate-report: ## Assemble the F0 gate report from the evidence
	$(call need_script,spikes/tools/report/build_gate_report.py)
	$(PYTHON) spikes/tools/report/build_gate_report.py
