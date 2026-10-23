PIPELINE_MANIFEST ?= spikes/f0-pipeline/Cargo.toml
CONFORMANCE_SCRIPT ?= protocol/vectors/run_conformance.sh

RUN ?= 1
DURATION_S ?= 600
SOURCE_ID ?= src-01
SEED ?= 42
LOSS_PCT ?= 3
ADDED_LATENCY_MS ?= 30
JITTER_MS ?= 10
STALL_S ?= 5
MARKER_PERIOD_S ?= 2
LISTEN ?= 0.0.0.0:41000
FORWARD ?=
SAMPLES ?=
EVIDENCE_ROOT ?= spikes/evidence/pipeline

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

spike-pipeline: ## Report the pipeline run matrix and the reference-environment gate
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- all \
		--run lat-$(RUN) --duration-s $(DURATION_S) --seed $(SEED) \
		--evidence-root $(EVIDENCE_ROOT)

spike-synthetic-source: ## Render the timestamped synthetic source and verify the marker
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) --bin synthetic-source -- \
		--source-id $(SOURCE_ID) --duration-s $(DURATION_S) \
		--evidence-root $(EVIDENCE_ROOT) --verify

spike-latency: ## Run one latency measurement run (RUN=1..3)
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- latency \
		--run lat-$(RUN) --duration-s $(DURATION_S) --evidence-root $(EVIDENCE_ROOT)

spike-loss: ## Run the pinned loss-and-jitter measurement through the seeded relay
	$(call need_script,$(PIPELINE_MANIFEST))
	@test -n "$(FORWARD)" || { echo "error: set FORWARD=<host:port> to the media receiver." >&2; exit 2; }
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- loss \
		--duration-s $(DURATION_S) --seed $(SEED) --loss-pct $(LOSS_PCT) \
		--added-latency-ms $(ADDED_LATENCY_MS) --jitter-ms $(JITTER_MS) \
		--listen $(LISTEN) --forward $(FORWARD) --evidence-root $(EVIDENCE_ROOT)

spike-resources: ## Run the resource measurement
	$(call need_script,$(PIPELINE_MANIFEST))
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- resources \
		--duration-s $(DURATION_S) --evidence-root $(EVIDENCE_ROOT)

spike-drift: ## Summarise a drift CSV into drift_summary.json
	$(call need_script,$(PIPELINE_MANIFEST))
	@test -n "$(SAMPLES)" || { echo "error: set SAMPLES=<drift.csv> recorded by the live run." >&2; exit 2; }
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- drift \
		--run drift-01 --samples $(SAMPLES) --evidence-root $(EVIDENCE_ROOT)

spike-stall: ## Analyse a recorded stall trace
	$(call need_script,$(PIPELINE_MANIFEST))
	@test -n "$(SAMPLES)" || { echo "error: set SAMPLES=<stall.csv> recorded by the live run." >&2; exit 2; }
	$(CARGO) run --manifest-path $(PIPELINE_MANIFEST) -- stall \
		--stall-s $(STALL_S) --samples $(SAMPLES) --evidence-root $(EVIDENCE_ROOT)

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
	$(call need_script,spikes/tools/report/render_report.py)
	$(PYTHON) spikes/tools/report/build_gate_report.py --out spikes/evidence/gate_report.json
	$(PYTHON) spikes/tools/report/render_report.py
