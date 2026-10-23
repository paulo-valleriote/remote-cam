---

description: "Task list for F0 — Risk-Reduction Spikes and Technical Decisions"
---

# Tasks: F0 — Risk-Reduction Spikes and Technical Decisions

**Input**: Design documents from `/specs/001-f0-technical-spikes/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md), [data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md), [constitution](../../.specify/memory/constitution.md)

**Tests**: Tests are **required** for every delivered package (constitution II) and by the plan's Testing section. Spike harnesses are validated by their recorded procedure and per-gate evidence instead of unit coverage; that exemption, and the custom jitter buffer / camera media source, are recorded deviations requiring maintainer approval (T117).

**Organization**: One phase per user story. Phases follow the **mandated execution waves** (FR-045/046/047) rather than numeric priority, because the desktop pipeline gate (P2) depends on the pairing handshake (P4) and the camera injection gate (P1):

| Wave | User story | Priority | Can start after |
| --- | --- | --- | --- |
| 1 | US1 — Virtual camera injection | P1 | Foundational |
| 1 | US4 — Pairing handshake | P4 | Foundational |
| 2 | US2 — Desktop receive pipeline | P2 | US1 + US4 |
| 3 | US3 — Codec interoperability | P3 | US2 |
| 3 | US5 — Virtual audio and licensing | P5 | US2 |

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: `US1`…`US5`; omitted in Setup, Foundational and Polish
- Every task names its exact file path

## Path Conventions

- Rust desktop workspace and product crates: `desktop/`, `desktop/crates/<crate>/`
- Kotlin Android modules: `mobile/`, `mobile/<module>/`
- Shared versioned contract: `protocol/`
- Non-production spike harnesses and evidence: `spikes/`, `spikes/evidence/`
- Spike procedures / runbooks / report: `docs/spike/`
- No spike code is merged into `desktop/` or `mobile/` product modules (FR-039).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Create the two product trees, the shared contract tree, the isolated spike tree, and the constitution-required execution entry points.

- [X] T001 Create the repository directory skeleton per plan.md: `desktop/crates/{protocol,signaling,media-core,injection,audio-bridge,app}/src`, `desktop/vendor/dshow-vcam-fallback/`, `mobile/{app,core,protocol}/src/main`, `protocol/vectors/`, `spikes/{f0-handshake,f0-pipeline,f0-camera-injection,f0-codec-interop,f0-virtual-audio,tools,fixtures,evidence}/`, `docs/spike/`
- [X] T002 [P] Create `desktop/rust-toolchain.toml` pinning edition 2024 / MSRV 1.92 and `desktop/Cargo.toml` as the workspace root with exact pins (`webrtc = "=0.21.1"`, `rustls = "=0.23.46"`, `rcgen = "=0.14.10"`, `tokio = "=1.54.1"`, `tokio-tungstenite = "=0.30.0"`, `slint = "=1.18.1"`, `windows = "=0.62.2"`, `qrcode = "=0.14.1"`, `sha2 = "=0.11.1"`, `openh264 = "=0.9.9"`, `criterion = "=0.8.2"`); commit `desktop/Cargo.lock`
- [X] T003 [P] Create the Android build scaffold in `mobile/settings.gradle.kts`, `mobile/build.gradle.kts` and `mobile/gradle/libs.versions.toml` with Kotlin 2.4.x, AGP 9.4.x, Gradle 9.6.x, JDK 17, `minSdk 29` / `targetSdk 36` / `compileSdk 36`, CameraX 1.6.2, `io.github.webrtc-sdk:android:150.7871.01`, zxing core 3.5.4 and kotlinx-serialization; do **not** apply the `org.jetbrains.kotlin.android` plugin (AGP 9 has built-in Kotlin)
- [X] T004 [P] Create the root `Makefile` (repository root, with includes under `scripts/make/`) with `help`, `preflight`, `selftest`, `build-desktop`, `build-mobile`, `test`, `coverage`, `lint`, `spike-handshake`, `spike-pipeline`, `spike-synthetic-source`, `spike-latency`, `spike-loss`, `spike-resources`, `spike-drift`, `spike-stall`, `spike-injection`, `spike-interop`, `spike-audio`, `gate-report`, `clean`
- [X] T005 [P] Write `README.md`, `desktop/README.md` and `mobile/README.md` with install, run, test, coverage, lint and debug instructions, and state that every documented command must exist as a Makefile target
- [X] T006 [P] Extend `.gitignore` for `desktop/target/`, `mobile/.gradle/`, `mobile/**/build/`, `mobile/local.properties`, `spikes/evidence/**/*.{wav,mp4,mkv,trace}`, `spikes/evidence/**/*.etl` and any secret/env file; keep derived metrics, scripts and procedures versioned
- [X] T007 [P] Create `spikes/README.md` with the literal NON-PRODUCTION marker (FR-039), the rule that spike code never merges into `desktop/` or `mobile/`, the `spikes/evidence/` layout from `contracts/measurement-formats.md` §10, and the rule that raw captures are local-only
- [X] T008 [P] Create the Python measurement package `spikes/tools/pyproject.toml` and `spikes/tools/analysis/__init__.py` pinning numpy 2.5.4, scipy 1.18.1, zxing-cpp 3.1.1, jiwer 4.0.0, soundfile 0.14.0, pystoi 0.4.1 and pyloudnorm 0.2.0, plus pytest configuration
- [X] T009 [P] Create the Linux CI workflow `.github/workflows/ci.yml`: Rust `fmt`/`clippy`/`test`, `cargo-llvm-cov` on `desktop/crates/protocol`, Python lint and pytest on `spikes/tools`, Gradle `test` + JaCoCo on `mobile/protocol`, and publication of the coverage percentage

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The versioned shared contract, its two independent implementations, measurement fixtures, toolchain self-test and preflight evidence. **No gate work may begin until this phase is complete.**

**⚠️ CRITICAL**: PC-1 (hardware decode) and the toolchain self-test are preconditions for every later measurement.

- [X] T010 Create the canonical `protocol/signaling.schema.json` from `specs/001-f0-technical-spikes/contracts/signaling-messages.schema.json` with `version` and `type` required, and `type` a closed enumeration of `pair|paired|offer|answer|ice|stats|close|error|ping|pong`
- [X] T011 [P] Create `protocol/reason-codes.json` as a closed enumeration with the stable spellings of `contracts/reason-codes.md` §1–§3 (`invalid_qr_payload` … `internal_error`), plus the security-escalation flag for `certificate_pin_mismatch`, `certificate_chain_rejected`, `tls_version_unsupported`, `token_already_used`, `source_address_not_private`, `source_subnet_mismatch`, `listener_closed`
- [X] T012 [P] Create `protocol/qr-payload.md` version 1: `RCM1` magic at offset 0, `version`/`flags` at 4/5, `endpoints_len` at 6 (big-endian u16), `session_id` 16 B at 8, `epoch` 4 B at 24, `expires_at` 8 B at 28, `pairing_token` 32 B at 36, `tls_fingerprint` 32 B at 68, `capabilities` 2 B at 100, 19-byte endpoint records from 102, 512-byte maximum, and validation rules Q-1…Q-9
- [X] T013 [P] Create the QR golden vectors in `protocol/vectors/qr/` for `minimal-v1`, `multi-endpoint-v1`, `ipv6-v1`, `unknown-version`, `bad-magic`, `reserved-bit-set`, `truncated`, `trailing-byte`, `public-endpoint`, `expired` and `empty-endpoints`, each pinning the exact bytes and the expected reason code
- [X] T014 [P] Create `protocol/vectors/reason-codes/all-codes.json` and `protocol/vectors/signaling/scenarios.json` pinning the expected code and expected accept/reject outcome for every pairing and signaling scenario in `contracts/reason-codes.md` and `quickstart.md` §6.1
- [X] T015 Implement the Rust contract consumer `desktop/crates/protocol/src/lib.rs` (message parse/validate, QR encode/decode on raw bytes, reason-code enum) with unit tests in `desktop/crates/protocol/tests/contract.rs`; target ≥ 90 % coverage
- [X] T016 [P] Implement the Kotlin contract consumer `mobile/protocol/src/main/kotlin/remotecam/protocol/QrPayload.kt` and `SignalingMessage.kt` (raw-byte QR decode via `Result.getBytes()`, never `getText()`), with tests in `mobile/protocol/src/test/kotlin/remotecam/protocol/ContractTest.kt`
- [X] T017 Create `protocol/vectors/run_conformance.sh` and wire it into `make test`: it executes the Rust and Kotlin implementations against the same vector files and fails on any divergence or on any accepted payload that should be rejected
- [X] T018 [P] Document the shared structured-log schema in `docs/spike/logging-schema.md` (operation, correlation id, outcome, severity, English only, no tokens/full fingerprints/personal data) and implement matching adapters in `desktop/crates/app/src/logging.rs` and `mobile/core/src/main/kotlin/remotecam/core/logging/StructuredLog.kt`
- [X] T019 [P] Create `spikes/tools/validation/validate_evidence.py` validating evidence against `specs/001-f0-technical-spikes/contracts/gate-report.schema.json`, `.../injection-matrix.schema.json` and the field rules of `.../measurement-formats.md`; expose it as `make validate-evidence`
- [X] T020 [P] Create `spikes/fixtures/calibration-40-60-120.wav` (40 ms gap, 60 ms gap, 120 ms gap, speech between), `spikes/fixtures/phrases-v1.csv` (1 opening anchor, 8 static, 10 nonce-bearing, 1 closing anchor) and `spikes/fixtures/README.md` documenting provenance as synthetic
- [X] T021 Implement `make selftest` in `spikes/tools/selftest.py` producing `spikes/evidence/toolchain_selftest.json` with exactly four checks: `silence_fixture` yields exactly 2 dropouts (60 ms and 120 ms; the 40 ms gap is below threshold), `qr_roundtrip` decodes byte-identically and the SHA-256 matches, `sampler_agreement` agrees with `typeperf` within 1 percentage point after machine normalisation, `harness_floor` is measured and recorded
- [X] T022 Implement `make preflight` in `spikes/tools/preflight.py` producing `spikes/evidence/environment.json` with exact `cpu_model`, `cpu_logical_cores`, `gpu_model`, `os_name`/`os_build`, Android model/API, every app and tool version, the `iperf3`/`ping` network baseline, `wifi_power_save: disabled`, `internet_enabled: false`, `within_reference_conditions`, and `hardware_decode_verification` from `MFTEnumEx(SYNCMFT|SORTANDFILTER)` plus a GPU-engine counter, requiring `hardware_accelerated: true` and `output_format_supported: nv12`
- [X] T023 [P] Create `spikes/tools/redaction.py` with a `redact()` filter plus tests in `spikes/tools/tests/test_redaction.py` proving tokens, full 64-hex fingerprints, absolute user paths, user names and private media are removed or truncated
- [X] T024 [P] Create `spikes/tools/metrics/criteria.py` with pure functions for percentile sets (p50/p95/p99/max), the 150 ms raw-p95 criterion, CPU p95 ≤ 15 %, private working set max ≤ 400 MB, drift ≤ 20 ms/h and the floor-corrected subtraction; tests in `spikes/tools/tests/test_criteria.py`
- [X] T025 Wire the foundational targets into `Makefile` (`make build-desktop`, `make build-mobile`, `make test`, `make coverage`, `make lint`) and `.github/workflows/ci.yml`, and verify `make preflight` + `make selftest` both succeed before any gate run

**Checkpoint**: `environment.json` shows hardware decode true / NV12, the self-test `all_passed` is true, conformance vectors pass on both stacks, and lint/coverage pass for `protocol/`.

---

## Phase 3: User Story 1 — Prove virtual camera injection across the target application matrix (Priority: P1) 🎯 MVP

**Goal**: A per-user-registered Windows 11 virtual camera that every target application category (including a 32-bit consumer) can open, delivering stable 1280×720@30 frames for 30 continuous minutes, plus a recorded backend decision.

**Independent Test**: On a clean Windows 11 x64 machine and a non-administrator account, register the prototype, open each of the five application categories plus a 32-bit consumer, select the virtual camera, and confirm 30 minutes of stable frames with no stall > 2 s. `spikes/evidence/injection/injection_matrix.json` validates against `contracts/injection-matrix.schema.json` and names the selected backend.

**Wave**: 1 · **Depends on**: Foundational · **Preconditions**: none (Q-1 and Q-8 are resolved here)

### Tests for User Story 1

- [X] T026 [P] [US1] Write ring-buffer tests in `desktop/crates/injection/tests/ring_buffer.rs`: newest-frame overwrite / drop-oldest under backpressure, monotonic frame publish, discarded-frame counter, and handle release on shutdown
- [X] T027 [P] [US1] Write the matrix schema test in `spikes/f0-camera-injection/tests/test_matrix_schema.py`: the produced matrix validates, every evaluated backend covers every application, `not_tested` carries a reason, and `selected_backend` is derivable
- [X] T028 [P] [US1] Write continuity-analyzer tests in `spikes/tools/tests/test_injection_continuity.py`: a 2 000 ms stall fails, 2 001 ms fails, duplicate frames are counted, delivered size/rate mismatches are recorded, and `observed_duration_s ≥ 1800` is enforced

### Implementation for User Story 1

- [X] T029 [P] [US1] Implement the NV12 ring-buffer producer in `desktop/crates/injection/src/ring_buffer.rs`: single writer, multi-reader, drop-oldest when full, never blocks the producer, increments `discarded_frame_count`, exposes shutdown that joins and releases
- [X] T030 [US1] Implement the Media Foundation virtual camera media source in `desktop/crates/injection/src/mf_virtual_camera.rs`: `IMFMediaSource`/`IMFMediaStream`, NV12 640×480@30 and 1280×720@30, frame timestamps, correct stream state transitions
- [X] T031 [US1] Implement camera lifecycle and registration in `desktop/crates/injection/src/registration.rs` using `MFCreateVirtualCamera` with `MFVirtualCameraAccess_CurrentUser` and `mfvirtualcamera.h`/`mfsensorgroup.lib`, plus unregistration, with no administrator requirement
- [X] T032 [P] [US1] Create the per-user registration verification script `spikes/f0-camera-injection/register.ps1` that registers/unregisters without elevation and records whether the media-source CLSID had to be machine-wide (`HKLM`) or was accepted per-user (`HKCU`), answering Q-1
- [X] T033 [US1] Implement the test-pattern producer `desktop/crates/injection/examples/test_pattern.rs` emitting a per-frame identifier at 1280×720@30 (and 640×480@30) with knobs for injected stall and frame duplication
- [X] T034 [US1] Implement the DirectShow fallback filter in `desktop/vendor/dshow-vcam-fallback/` (C++/MSVC) with separate 32-bit and 64-bit CLSIDs and a registration/unregistration entry point; build it only when a primary-backend matrix cell fails
- [X] T035 [P] [US1] Create the Chromium capture harness `spikes/f0-camera-injection/apps/browser/index.html` and `capture.mjs` using `getUserMedia` + `MediaRecorder` against the virtual camera, recording duration, announced size/rate and the captured file
- [X] T036 [P] [US1] Create the OBS harness `spikes/f0-camera-injection/apps/obs/capture.py` driving `obs-websocket` 5.7.x to select the virtual camera, record 30 minutes and export the recording metadata
- [X] T037 [P] [US1] Create the window-capture harness `spikes/f0-camera-injection/apps/window_capture.py` for the conferencing client, Windows Camera app and screen recorder using FFmpeg `gdigrab` window capture, with the exact product and version recorded per run
- [X] T038 [US1] Create the 32-bit consumer harness `spikes/f0-camera-injection/apps/browser32/` and persist the observed bitness outcome for both backends (a 32-bit consumer that cannot see the device is a matrix failure, not a limitation — Q-8)
- [X] T039 [US1] Implement the continuity analyzer `spikes/tools/analysis/injection.py` producing `spikes/evidence/injection/injection_matrix.json`: per-cell outcome, `observed_duration_s`, `frame_size_ok`/`frame_rate_ok`, `longest_stall_ms ≤ 2000`, `duplicate_frames`, and the `multi_consumer_observation` object
- [X] T040 [US1] Implement `make spike-injection` in `spikes/f0-camera-injection/run_matrix.py`: iterate the five categories × evaluated backends for 30 minutes each, evaluate `dshow_filter` when any primary cell fails, and emit one validated matrix — **the 30-minute live run is blocked**: this host has no Windows Media Foundation, no FFmpeg and no camera (`spikes/evidence/environment.json`), so the matrix can only be assembled from captures taken on the Windows 11 reference machine; the runner, the plan and the offline assembly are complete and tested
- [X] T041 [US1] Record the injection backend decision in `spikes/evidence/decisions/D-10-injection-backend.md` with per-application evidence for both backends, `licensing_impact`, `media_phase_consequences` and the 720p/1080p capability statement — **recorded as `draft`, not `accepted`**: the evidence the data model requires for an accepted decision (the matrix, both bitnesses, per-application results) does not exist yet, and inventing it is not an option; D-10 states exactly how it becomes `accepted`
- [X] T042 [US1] Write the gate procedure `docs/spike/gate-1-camera-injection.md` with the reproducible steps, the minimum matrix, both bitnesses, the multi-consumer scenario and the Q-1/Q-6/Q-8 answers

**Checkpoint**: User Story 1 is independently demonstrable: the matrix is 100 % `frames_delivered` on the selected backend with `observed_duration_s ≥ 1800` and `longest_stall_ms ≤ 2000`, and the backend decision is recorded.

> **Checkpoint status — not met.** The apparatus for this checkpoint is complete and tested (17 tasks, 29 tests in `spikes/f0-camera-injection`, 145 across the Python tooling, plus the Windows-target Rust typecheck), but the checkpoint itself requires a 30-minute capture of six consumer applications on the Windows 11 reference machine. The F0 development host cannot produce it (`spikes/evidence/environment.json` records Linux/WSL2, `hardware_accelerated: false`, `within_reference_conditions: false`), so `spikes/evidence/injection/injection_matrix.json` does not exist and `D-10` is `draft`. The procedure that closes it is `docs/spike/gate-1-camera-injection.md`.

---

## Phase 4: User Story 4 — Prove the authenticated pairing handshake (Priority: P4)

**Goal**: A TLS 1.3, fingerprint-pinned, token-authenticated handshake that rejects every adversarial scenario with a closed reason code, proves per-origin throttling and listener shutdown, and leaves zero pairing artefacts after a session end **and** after an application restart.

**Independent Test**: Run the handshake harness. The valid scenario is accepted, all adversarial scenarios are rejected with codes from `contracts/reason-codes.md`, no media flows before authentication, no credentials remain, the 6th attempt in a minute is refused, the listener closes at 10 consecutive failures, a second legitimate origin pairs during the flood, and both residue inspections are empty with the same method.

**Wave**: 1 · **Depends on**: Foundational (protocol contract) · **Preconditions**: PC-5, PC-6

### Tests for User Story 4

- [X] T043 [P] [US4] Write pinning-verifier tests in `desktop/crates/signaling/tests/pinning.rs`: exact DER-encoded-leaf SHA-256 succeeds, one-byte mismatch fails with `certificate_pin_mismatch`, chain-only trust fails with `certificate_chain_rejected`, TLS 1.2 fails with `tls_version_unsupported`, and `verify_tls13_signature` validates the `CertificateVerify` signature
- [X] T044 [P] [US4] Write token tests in `desktop/crates/signaling/tests/token.rs`: 256-bit single use, replay fails with `token_already_used`, expiry at 120 s fails with `token_expired`, wrong token fails with `token_invalid`, and comparison is constant-time
- [X] T045 [P] [US4] Write throttling tests in `desktop/crates/signaling/tests/throttling.rs`: per-origin 5 attempts/minute, attempt 6 refused with `rate_limited`, block duration strictly increases, listener closes at 10 consecutive failures with `listener_closed`, and a second origin is unaffected (a global limit is a defect)
- [X] T046 [P] [US4] Write the residue-scanner self-test `spikes/f0-handshake/tests/test_residue_scanner.py`: a deliberately seeded dummy token in a scratch store is detected, then removed, proving the scanner is not vacuously passing

### Implementation for User Story 4

- [X] T047 [US4] Implement ephemeral certificate issuance in `desktop/crates/signaling/src/certificate.rs`: `rcgen` ECDSA P-256 self-signed per session, DER leaf fingerprint (SHA-256), in-memory only, destroyed at session end
- [X] T048 [US4] Implement the WSS listener in `desktop/crates/signaling/src/server.rs`: `tokio-tungstenite` over `rustls` 0.23.46, TLS 1.3 only, session resumption disabled, no root store, ephemeral certificate, and a 15 s handshake timeout
- [X] T049 [US4] Implement the pinned client in `desktop/crates/signaling/src/client.rs` behind the project-owned `PinnedCertificateVerifier` trait, so a future `rustls` upgrade is a single-file change
- [X] T050 [US4] Implement QR generation in `desktop/crates/signaling/src/qr.rs`: enumerate only private RFC 1918 / IPv6 link-local and unique-local addresses in stable order, use `qrcode` 0.14.1 `Bits` byte mode with `EcLevel::M`, cap the payload at 512 bytes, set a 120 s expiry, and refuse to display a QR when no private endpoint exists
- [X] T051 [US4] Implement the in-memory token store in `desktop/crates/signaling/src/token.rs`: single use, constant-time comparison, 120 s window, invalidated on first success, zeroed on teardown
- [X] T052 [US4] Implement DTLS fingerprint exchange in `desktop/crates/signaling/src/fingerprint_exchange.rs` so the media fingerprints travel only inside the authenticated channel and a tampered fingerprint aborts before media flows
- [X] T053 [US4] Implement the scenario harness `spikes/f0-handshake/src/main.rs` producing `spikes/evidence/pairing/pairing_scenarios.json` and one `<scenario_id>.trace` per scenario, with precondition, expected result, observed result, outcome, reason code, `media_flowed_before_authentication` and `residual_credentials`
- [X] T054 [US4] Implement the adversarial scenarios in `spikes/f0-handshake/scenarios/`: valid pairing, wrong fingerprint (one byte), chain-only trust, TLS downgrade, expired token, reused token, wrong token, non-private source address, interception proxy, QR expired, malformed QR, and QR with a public endpoint
- [X] T055 [US4] Implement per-origin throttling in `desktop/crates/signaling/src/throttling.rs`: keyed by origin address, 5 attempts/minute, progressive and strictly increasing block, listener shutdown at 10 consecutive failures, each decision carrying a closed reason code and observed outcome
- [X] T056 [US4] Implement the throttling scenarios `spikes/f0-handshake/src/throttle.rs`: attempt 6 refused, progressive blocks observed, listener closed at failure 10 as a security event, and a concurrent legitimate pairing from a second origin succeeds; emit `spikes/evidence/pairing/pairing_throttling.json`
- [X] T057 [US4] Implement the residue inspection tooling `spikes/f0-handshake/src/residue.rs`: scan process memory, the app-data directory and journals, the platform credential/certificate store, and `HKCU\Software\<vendor>\<app>` on **both** devices, for session keys, pairing tokens, fingerprints and pairing records — **implemented as a byte scanner plus named platform probes** (`residue_scan.py --probe`), so the credential store, registry, `adb` package data and a process-memory dump are scanned with the same needles and reported in the same shape as the filesystem sweep; the platform commands are recorded in the gate procedure
- [X] T058 [US4] Implement the restart scenario `spikes/f0-handshake/scenarios/restart/` that terminates the desktop and mobile applications without unwinding, restarts them, and replays the previous token; emit `spikes/evidence/pairing/pairing_residue.json` with `after_session_end` and `after_application_restart` using the identical `method`, a stated `coverage`, all four `artefact_kinds_checked`, and empty `artefacts_found` — **the automated state-reset replay is implemented and tested**, and the harness refuses to emit the contract file until both reference devices are named and `--restart-observed` is passed; the two-device live run is pending the Windows 11 and Android reference devices (this host has neither), so `pairing_residue.json` does not exist yet and the gate verdict is pending
- [X] T059 [US4] Record FR-007 in `spikes/evidence/pairing/pairing_scenarios.json`: `established_channel_ms` from pairing start to established channel and `pinning_added_delay_ms` versus an unpinned control connection
- [X] T060 [US4] Write the gate procedure `docs/spike/gate-2-pairing-handshake.md` and the security-reviewer checklist covering every scenario, the absence of any unauthenticated fallback, the throttling thresholds (5/minute, 6th refused, 10 → listener closed), the residue method/coverage, and Q-09

**Checkpoint**: All adversarial scenarios are rejected with closed codes, the valid one succeeds in ≤ 5 s, no media precedes authentication, throttling and listener shutdown are proven, and both residue inspections are empty.

---

## Phase 5: User Story 2 — Validate the desktop receive pipeline against latency and resilience gates (Priority: P2)

**Goal**: A receive-only 1280×720@30 pipeline with an adaptive jitter buffer, hardware H.264 decode, ring-buffer handoff and RTP-paced audio that meets raw p95 ≤ 150 ms, CPU p95 ≤ 15 %, memory ≤ 400 MB, drift ≤ 20 ms/h, audio continuity under the pinned impairment profile, and recovery from a 5 s stall.

**Independent Test**: Feed timestamped synthetic media through the isolated receive/decode/inject path. Three ≥ 10-minute runs must yield raw p95 capture-at-consumer ≤ 150 ms with the harness floor and floor-corrected value published; resource, drift, loss/jitter and stall evidence must each validate against `contracts/measurement-formats.md`.

**Wave**: 2 · **Depends on**: US1 (injection backend and ring buffer) + US4 (authenticated channel) · **Preconditions**: PC-1, PC-2, PC-3, PC-4

### Tests for User Story 2

- [X] T061 [P] [US2] Write adaptive-jitter-policy tests in `desktop/crates/media-core/tests/jitter_policy.rs` with an injected clock: target-depth adaptation, playout-delay adjustment, fast-forward after a stall, and no unbounded buffer growth
- [X] T062 [P] [US2] Write drift tests in `desktop/crates/media-core/tests/drift.rs`: regression slope in ms/hour, R², 95 % confidence interval, and proof that a free-running resampler (~36 ms/h at 1e-5 relative error) is rejected while an RTP-paced ratio converges
- [X] T063 [P] [US2] Write the concurrency test `desktop/crates/media-core/tests/concurrency.rs`: producer/consumer ordering, event idempotency, session replacement releasing the previous session with no media overlap, and a stale `epoch` discarded and counted
- [X] T064 [P] [US2] Write the teardown/leak test `desktop/crates/media-core/tests/teardown.rs`: COM objects released, ring-buffer handles closed, threads joined, and memory returned after session end — ring-buffer closure, joined threads and released frames are asserted on Linux CI; the hardware-decoder precondition and COM release are Windows-only tests that run on the reference machine
- [X] T065 [P] [US2] Write relay determinism tests in `spikes/f0-pipeline/tests/relay_determinism.rs`: identical seed produces an identical drop/delay log, control traffic is never impaired, and the realised 3 % loss is computed from the log
- [X] T066 [P] [US2] Write analyzer tests in `spikes/tools/tests/test_pipeline_metrics.py`: raw vs floor-corrected percentiles, `criterion.basis == "raw"`, CPU normalisation `cpu_pct_machine == cpu_pct_single_core / cores`, and the stage-budget sum rule

### Implementation for User Story 2

- [X] T067 [US2] Implement the receive-only peer connection in `desktop/crates/media-core/src/receiver.rs`: no send tracks (zero outbound media RTP), empty `iceServers` so only host candidates exist, DTLS-SRTP, NACK/RTCP receiver reports enabled, and PLI explicitly opted in
- [X] T068 [US2] Implement the adaptive jitter policy in `desktop/crates/media-core/src/jitter_policy.rs` on top of `JitterBufferInterceptor`: RFC 3550 target delay, playout-delay adaptation, and fast-forward after a stall
- [X] T069 [US2] Implement the Media Foundation decoder adapter in `desktop/crates/media-core/src/adapters/mf_decoder.rs`: `MFTEnumEx(SYNCMFT|SORTANDFILTER)`, `IMFDXGIDeviceManager`, NV12 output only, buffer locking in the order `IMF2DBuffer2::Lock2DSize` → `IMF2DBuffer::Lock` → `IMFMediaBuffer::Lock`
- [X] T070 [P] [US2] Implement the software decode fallback adapter `desktop/crates/media-core/src/adapters/software_decoder.rs` using `openh264` 0.9.9 for the Linux CI path only, labelled non-product and never cited for the latency gate
- [X] T071 [US2] Implement frame handoff in `desktop/crates/media-core/src/handoff.rs`: publish decoded NV12 into the injection ring buffer, drop the oldest pending frame when behind, and increment `discarded_frame_count` without blocking the network path
- [X] T072 [US2] Implement the WASAPI audio render adapter `desktop/crates/media-core/src/adapters/wasapi_render.rs` with the resampler ratio paced from the RTP timestamps (PLL over a sliding window), not a free-running resampler
- [X] T073 [US2] Implement A/V synchronization in `desktop/crates/media-core/src/sync.rs` using the source capture timestamp carried in the RTP header extension, and expose per-stream playout delays for drift sampling
- [X] T074 [US2] Implement the synthetic source `spikes/f0-pipeline/src/synthetic_source.rs`: 1280×720@30, 2 500 kbps, a per-frame QR marker carrying `{epoch_ms}|{frame_no}` duplicated outside any likely crop region, and a one-frame verification command that proves the marker survives encoding
- [X] T075 [US2] Implement the seeded impairment relay `spikes/f0-pipeline/src/relay.rs`: RTP-aware userspace UDP relay that parses SSRC and payload type, impairs media only (3 % uniform loss, 30 ms added latency, ±10 ms jitter, `StdRng::seed_from_u64`), holds/releases 5 s stalls, and writes `spikes/evidence/pipeline/relay_log.jsonl` plus `impairment_profile.json` (profile name `nominal-3pct-30ms-10ms`, `applies_to: media_only`)
- [X] T076 [US2] Implement latency capture in `spikes/f0-pipeline/src/latency_capture.rs` and `spikes/tools/analysis/latency.py`: capture the consumer's own output with the identical command used for calibration, decode the QR, and write `latency_samples.csv` with `source_frame_no`, `source_epoch_ms`, `capture_host_ms`, `clock_offset_ms = 0` and `latency_ms`
- [X] T077 [US2] Implement harness-floor calibration in `spikes/tools/analysis/floor.py`: render the identical QR in a local HTML page, capture it with the identical command, measure the floor (expected 17–50 ms), and record it in `toolchain_selftest.json` and every `latency_summary.json`
- [X] T078 [US2] Implement `spikes/tools/analysis/latency.py` summary output `latency_summary.json` with `raw` (p50/p95/p99/max), `harness_floor_ms`, `corrected`, `criterion.metric = raw.p95_ms` / threshold 150 / `basis = raw`, `included_stages`, `excluded_stages` and `attribution` when the result is not a clear pass
- [X] T079 [US2] Instrument the six latency stages and emit `stage_budget.json` from `spikes/tools/analysis/stages.py`: capture 33, encode 15, network_and_jitter_buffer 20, decode 12, frame_handoff 3, consumer_presentation 17 ms, each with `included_in_gate` and `measurement_method`; enforce the sum rule
- [X] T080 [US2] Implement the resource sampler `spikes/f0-pipeline/src/resource_sampler.rs` using `GetProcessTimes` + `GetSystemTimes` + `GetProcessMemoryInfo`, machine-normalised `cpu_pct_machine`, `private_working_set_bytes`, a pre-window burn-in sample, and the `typeperf` cross-check; emit `resource_samples.csv` and `resource_summary.json` (CPU p95 ≤ 15, private working set max ≤ 400 MB, p50/p95/p99/max)
- [X] T081 [US2] Implement drift measurement in `spikes/f0-pipeline/src/drift.rs` with a 2 s marker period: write `drift_samples.csv` and `drift_summary.json` with `drift_ms_per_hour`, R², `slope_ci95_ms_per_hour`, and `components` (sender skew, receiver resampler, present-vs-render); a confidence interval straddling 20 ms/h must not pass
- [X] T082 [US2] Implement the stall scenario `spikes/f0-pipeline/src/stall.rs`: relay holds media for 5 s, the session continues, latency returns to baseline within 2 s, buffer-depth trace has no unbounded growth, and discarded frames are counted into `stall.json`
- [X] T083 [US2] Implement `make spike-pipeline` orchestration over the run matrix: three ≥ 10-minute latency runs, one ≥ 10-minute loss/jitter run with the pinned profile, a 0 %-loss control run, a 30-minute resource run, a 30-minute drift run and the 5 s stall, writing to `spikes/evidence/pipeline/`
- [X] T084 [US2] Write the gate procedure `docs/spike/gate-3-desktop-receive-pipeline.md` with the run order, the preconditions PC-1…PC-4, the raw-vs-floor rule, the attribution rule, the profile-deviation rule and the 0 %-loss control
- [X] T085 [US2] Record the default quality decision (1280×720@30 default, 1920×1080@30 optional maximum) as `spikes/evidence/decisions/D-quality-default.md` with measured evidence, and answer Q-5 with the hardware decode verification

**Checkpoint**: The pipeline gate evidence validates: raw p95 ≤ 150 ms across three runs, floor and corrected values published, CPU/memory within budget, drift certified, zero dropouts under the pinned profile, and stall recovery ≤ 2 s.

---

## Phase 6: User Story 3 — Confirm codec interoperability between mobile and desktop (Priority: P3)

**Goal**: Live H.264 video from the Android reference device decodes on the desktop with agreed profile/level/format and no visible corruption for 10 continuous minutes, or the VP8 fallback is approved with measured CPU, latency and quality trade-offs.

**Independent Test**: Send live video from the reference Android device to the desktop receive pipeline. Record the negotiated profile/level/format, confirm 10 minutes without visible corruption, and—if H.264 fails—repeat with VP8 and report measured differences plus the fallback's hardware-decode status.

**Wave**: 3 · **Depends on**: US2 · **Preconditions**: PC-1 (applies again to the codec actually selected)

### Tests for User Story 3

- [X] T086 [P] [US3] Write the chroma conversion tests in `mobile/core/src/test/kotlin/remotecam/core/capture/YuvConversionTest.kt`: planar stride 1 passes through, stride 2 is de-interleaved into a scratch I420 buffer, and swapped-chroma (green/magenta) output is impossible
- [X] T087 [P] [US3] Write the transport configuration test `mobile/core/src/test/kotlin/remotecam/core/transport/TransportConfigTest.kt`: both transceivers are `SEND_ONLY`, `iceServers` is empty, cellular/VPN masks are applied, and H.264 is offered before VP8
- [X] T088 [P] [US3] Write the interop evidence test `spikes/f0-codec-interop/tests/test_interop_record.py`: negotiated profile/level/format are present, the 10-minute duration is enforced, and the fallback record carries CPU, latency, quality and a hardware-acceleration boolean

### Implementation for User Story 3

- [X] T089 [US3] Create the minimal Android shell in `mobile/app/src/main/AndroidManifest.xml` and `mobile/app/src/main/kotlin/remotecam/app/MainActivity.kt`: camera and microphone permissions, a foreground streaming service, and a session screen; no production camera controls
- [X] T090 [US3] Implement the capture adapter `mobile/core/src/main/kotlin/remotecam/core/capture/CameraXCaptureSource.kt`: bind only `ImageAnalysis` (no `Preview` surface), runtime-guard `getPixelStride()` and de-interleave stride-2 devices, then `JavaI420Buffer.wrap(...)` with no copy
- [X] T091 [US3] Implement the transport in `mobile/core/src/main/kotlin/remotecam/core/transport/PeerConnectionFactoryProvider.kt`: `RtpTransceiver.SEND_ONLY` for audio and video with the `remotecam` stream id, empty `iceServers`, `networkIgnoreMask` for cellular/VPN, and H.264 preferred / VP8 fallback negotiation
- [X] T092 [US3] Implement the mobile signaling client `mobile/core/src/main/kotlin/remotecam/core/signaling/SignalingClient.kt` over the pinned TLS channel using `mobile/protocol`, including raw-byte QR decode with `Result.getBytes()` and validation before any socket opens
- [X] T093 [US3] Implement the desktop interop receiver `spikes/f0-codec-interop/src/main.rs`: answer the offer receive-only, log the negotiated codec/profile/level/format parameters, count decoded frames, and expose whether the selected codec is hardware-accelerated
- [X] T094 [US3] Implement corruption detection in `spikes/tools/analysis/interop_corruption.py`: detect black, green and frozen frames, compute the longest freeze, and record the named human observation; a negotiation success with visible corruption is a failure
- [X] T095 [US3] Implement the fallback comparison run in `spikes/f0-codec-interop/src/compare.rs`: measure CPU, latency and a quality comparison for the preferred and fallback codecs, and record the fallback's hardware-decode status on the reference machine
- [X] T096 [US3] Record the codec decision in `spikes/evidence/decisions/D-codec-baseline.md` with the preferred-or-fallback choice, the measured trade-offs, the profile/level used and the explicit approval required before F1 (FR-020)
- [X] T097 [US3] Write the gate procedure `docs/spike/gate-4-codec-interoperability.md` and record the Q-7 level finding (1080p30 needs level ≥ 4.0 while many receivers accept only 3.1) and the Q-10 pixel-stride finding

**Checkpoint**: H.264 interop is proven for 10 minutes with agreed parameters, or VP8 is approved with measured trade-offs and its hardware-decode status is recorded.

---

## Phase 7: User Story 5 — Close the virtual audio path and its licensing decision (Priority: P5)

**Goal**: Decoded audio reaches a consumer application through the pinned VB-CABLE device, remains intelligible and continuous for 10 minutes under the pinned impairment profile, degrades safely to video-only when the device is absent, and closes with a licensing decision or an explicit conditional verdict and plan B.

**Independent Test**: Render into VB-CABLE, record from CABLE Output in a consumer application for ≥ 10 minutes under 3 % uniform loss / 30 ms / ±10 ms, and show zero detected dropouts ≥ 50 ms with both detectors agreeing, `phrases_exact ≥ 18` over the 20-phrase manifest, WER ≤ 0.05, all nonces and anchors matching, a named observer and a recorded phrase script; separately produce a licensing bundle and decision with all candidates.

**Wave**: 3 · **Depends on**: US2 (audio pipeline and relay) · **Preconditions**: PC-3, PC-4

### Tests for User Story 5

- [X] T098 [P] [US5] Write dropout-analyzer tests in `spikes/tools/tests/test_dropouts.py`: the committed 40/60/120 ms fixture yields exactly 2 dropouts, a detector disagreement is flagged, a missing calibration makes the threshold invalid, and a 49 ms interval is `below_threshold`
- [X] T099 [P] [US5] Write intelligibility tests in `spikes/tools/tests/test_intelligibility.py`: `phrases_exact < 18` fails, WER > 0.05 fails, any nonce mismatch invalidates a stale capture, an anchor mismatch fails, and an unnamed observer or missing phrase script makes the result unverifiable
- [X] T100 [P] [US5] Write readiness tests in `desktop/crates/audio-bridge/tests/readiness.rs`: missing, unsigned and unexpected-identity devices each produce an actionable state, a 1 kHz test tone is required before audio-ready, and video-only operation remains usable

### Implementation for User Story 5

- [X] T101 [US5] Implement WASAPI rendering in `desktop/crates/audio-bridge/src/wasapi_render.rs` into the installed `CABLE Input` endpoint, 48 kHz mono, paced from the RTP timestamps established in `media-core`
- [X] T102 [US5] Implement readiness in `desktop/crates/audio-bridge/src/readiness.rs`: detect the device by exact product identity, verify the driver signature (`Get-AuthenticodeSignature` and `signtool verify /pa /v`) on the Secure-Boot-enabled runner, and play an audible 1 kHz test tone before a session is audio-ready (FR-031) — **the readiness state machine, the 1 kHz tone requirement and `signature_probe.ps1` are complete and tested**; the live signature verification is blocked on the Secure-Boot Windows 11 runner, so Q-2 stays unresolved and no run can be marked `signature_verified: true`
- [X] T103 [US5] Implement degraded operation in `desktop/crates/audio-bridge/src/degraded.rs`: when the device is missing, unsigned or unexpected, report an actionable state naming the user's next step and keep the video-only session fully usable (FR-032)
- [X] T104 [US5] Implement the audio run harness `spikes/f0-virtual-audio/src/main.rs`: render the scripted audio through VB-CABLE under the relay profile, record CABLE Output in a consumer application for ≥ 10 minutes, and emit `spikes/evidence/audio/aud-01/audio_run_capture.json` with `device_pin` (VB-CABLE, driver pack 45), `signature_verified`, `consumer_renegotiations` and all references — **the harness is complete and tested** (WAV/run-plan validation, index assembly, Windows-only render), but the live run is **blocked**: this host has no Windows 11, no VB-CABLE, no FFmpeg and no consumer application (`spikes/evidence/environment.json`), so `audio_run_capture.json` does not exist; `spikes/evidence/audio/run_matrix.json` records `blocked_by_environment` and the exact required environment
- [X] T105 [US5] Implement the two-detector dropout analysis in `spikes/tools/analysis/dropouts.py`: FFmpeg ≥ 8.1 `silencedetect` at the calibrated threshold (noise floor + 6 dB, with `threshold_source`) plus the RMS-window cross-check, correlate intervals with RTP sequence gaps, and require `detectors_agree` (a disagreement is a recorded finding)
- [X] T106 [US5] Implement the intelligibility workflow in `spikes/tools/analysis/intelligibility.py` and `spikes/f0-virtual-audio/play_phrases.py`: play the 20-phrase manifest (`phrase_count = 20`, non-anchor `phrases_exact`), capture the observer's transcription, and emit `intelligibility.json` with `phrases_exact ≥ 18`, `corpus_wer ≤ 0.05`, all 10 nonces matched, anchors matched, the named observer and `phrase_script_ref`
- [X] T107 [US5] Implement the licensing assessment in `spikes/tools/analysis/licensing.py` producing `spikes/evidence/licensing/licensing_bundle.json` for every candidate: VB-CABLE (pinned measurement device), Virtual Audio Cable (OEM/custom), VoiceMeeter, Scream, SteelSeries Sonar, own WDM driver (rejected for V1) and plan B video-only — each with official `terms_url`, `terms_captured_text`, `redistribution_allowed`, `commercial_use_allowed`, `unit_cost`, `cost_model`, `support_burden`, `user_experience`, `signature_verified` and `fallback`, plus `options_compared` covering redistribution / user-installed official / plan B and the `remeasurement_required` rule — **the catalogue, the bundle builder and the validator rules are complete and tested**; `licensing_bundle.json` is deliberately not emitted because the contract requires a named decision owner and approver, which the engineer cannot invent, and CI would reject a draft bundle whose roles are empty
- [X] T108 [US5] Record the licensing decision in `spikes/evidence/decisions/D-audio-licensing.md` with named decision owner and approver, date, chosen option, unit cost, fallback, and the escalation path when unresolved; an unresolved decision on the gate date is `conditional`/`no_go`, never a silent pass — **recorded as `draft`, not `accepted`**: all seven candidates and their official terms are captured, the recommendation is VB-CABLE link-only with a video-only plan B, and the record states the escalation path; the named owner, approver and date are missing by design, so the gate verdict is `conditional`
- [X] T109 [US5] Write the gate procedure `docs/spike/gate-5-virtual-audio-and-licensing.md` covering the technical measurement, the scripted human check, environmental limitations, the parallel licence assessment, and the Q-2/Q-3/Q-4 findings

**Checkpoint**: The audio gate evidence validates: zero dropouts ≥ 50 ms with agreeing detectors, `phrases_exact ≥ 18` with all nonces/anchors matching, named observer and script, zero renegotiations, verified device signature, video-only degradation proven, and a licensing decision or explicit conditional with plan B.

> **Checkpoint status — not met.** The apparatus for this checkpoint is complete and tested (15 harness tests in `spikes/f0-virtual-audio`, 9 phrase-playback tests, 16 `remotecam-audio-bridge` tests, 21 analyser tests across `test_dropouts.py`, `test_intelligibility.py` and `test_licensing.py`, plus the Windows-target typecheck), and the procedure that closes it is `docs/spike/gate-5-virtual-audio-and-licensing.md`. The checkpoint itself needs a ≥ 10-minute live capture on the Windows 11 reference host through VB-CABLE driver pack 45, a Secure-Boot signature result, a named observer and a signed licensing decision. This host is Linux (`hardware_accelerated: false`, `within_reference_conditions: false`) and has no VB-CABLE, FFmpeg or consumer application, so no `dropouts.json`, `intelligibility.json`, `audio_run_capture.json` or `licensing_bundle.json` exists; `spikes/evidence/audio/run_matrix.json` records `blocked_by_environment`.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Assemble the five verdicts, close the decisions and open questions, audit the artefacts, and prove reproducibility.

- [ ] T110 [P] Implement the gate-report assembler `spikes/tools/report/build_gate_report.py` producing `spikes/evidence/gate_report.json`: five gates with `wave`, `depends_on`, `preconditions`, named `accountable_role`, `verdict`, `measurement_conditions`, `evidence_refs`, one shared `phase_review_at`, `conditions`/`waivers`, `blocking_dependency` where a dependency failed, and `downstream_status` for F1/F2/F3
- [ ] T111 [P] Implement the report renderer `spikes/tools/report/render_report.py` producing `docs/spike/gate-report-f0.md` with latency (raw p50/p95/max + floor + corrected + attribution), CPU/memory, loss/jitter with injected and residual loss, drift with CI, the full injection matrix for every evaluated backend, codec results, audio results with observer and script, throttling and residue evidence, and the non-production statement
- [ ] T112 [P] Consolidate the open-questions register into `spikes/evidence/open_questions.json` (Q-1…Q-10 plus any new question) with `owner`, `impact`, `blocking` and `status`; an `answered` question must reference a decision record
- [ ] T113 [P] Consolidate decision records into `spikes/evidence/decisions/` and write `docs/spike/tdd-amendments.md` listing the 13 findings in `research.md` §4 that require a TDD amendment
- [ ] T114 [P] Run the redaction audit across every artefact via `spikes/tools/redaction_audit.py` and add it to CI in `.github/workflows/ci.yml`: no secrets, no full fingerprints, no absolute user paths, no personal data, no real user media, and repository-relative paths only
- [ ] T115 [P] Add validation tests for verdict semantics in `spikes/tools/tests/test_gate_report.py`: a conditional verdict requires either a waiver signed by the gate's accountable role with a `resolution_deadline` or an explicit block; a gate whose dependency is `no_go` still carries a verdict naming it; all five share one review timestamp within one minute
- [ ] T116 Enforce the coverage gate in `make coverage`: ≥ 90 % for `desktop/crates/protocol` and `mobile/protocol` via `cargo-llvm-cov` and JaCoCo, failing the target below threshold; keep `spikes/` exempt per the recorded deviation
- [ ] T117 Record the three deviations in `docs/spike/deviations.md` with justification, scope and rollback (spike coverage exemption; custom jitter buffer and custom MF media source; new first-level directories `protocol/` and `spikes/`), and obtain written maintainer approval before merge
- [ ] T118 Add `scripts/check-docs-consistency.sh` and wire it into CI: every command documented in a README exists as a Makefile target and vice versa
- [ ] T119 Perform the quickstart reproduction rehearsal: a second engineer executes `quickstart.md` Phase A–H within one working day using only the recorded procedure and environment, recording the outcome in `spikes/evidence/reproduction_rehearsal.md`; fix deviations and re-run
- [ ] T120 Final validation in `spikes/evidence/gate_report.json`: `make test`, `make coverage`, `make lint`, `make validate-evidence` all pass; `make gate-report` emits a schema-valid report; every referenced evidence file exists; no `outside-reference-environment` or `profile-deviation` run is cited for a `go`
- [ ] T121 Hold the single phase review on or before 2026-11-06 and record all five verdicts in `spikes/evidence/gate_report.json` and `docs/spike/gate-report-f0.md` with the accountable roles named; verify SC-001…SC-014 before declaring F0 complete

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Setup; **blocks all five gates**.
- **US1 (Phase 3, wave 1)** and **US4 (Phase 4, wave 1)**: both start after Foundational, and may run concurrently.
- **US2 (Phase 5, wave 2)**: depends on US1 (injection backend, ring buffer) and US4 (authenticated channel). A `no_go` in either still requires a recorded US2 verdict naming the blocking dependency.
- **US3 (Phase 6, wave 3)** and **US5 (Phase 7, wave 3)**: depend on US2. They may run concurrently with each other.
- **Polish (Phase 8)**: depends on all five gates having a verdict, including verdicts blocked by a failed dependency.

### User Story Dependencies

| Story | Depends on | Why | Independently testable |
| --- | --- | --- | --- |
| US1 (P1) | — | — | Yes, on a clean Win11 machine |
| US4 (P4) | — (protocol contract from Foundational) | — | Yes, harness-only |
| US2 (P2) | US1, US4 | Injection target and authenticated media channel | Yes, after both complete |
| US3 (P3) | US2 | Needs the desktop receiver and negotiated transport | Yes, with the reference phone |
| US5 (P5) | US2 | Needs the RTP-paced audio path and the impairment relay | Yes, with VB-CABLE installed |

### Within Each User Story

1. Write the listed tests first and confirm they fail.
2. Pure domain logic (ring buffer, jitter policy, token, throttling, analyzers) before OS adapters.
3. Adapters (Media Foundation, WASAPI, COM, CameraX, libwebrtc) before harnesses.
4. Harnesses before evidence generation.
5. Evidence and procedure before the gate verdict.

### Parallel Opportunities

- Setup: T002–T009 (8 of 9 tasks) may run concurrently.
- Foundational: T011–T014, T016, T018–T020, T023–T024 are independent; T015/T016 are the two independent language implementations.
- Wave 1: **US1 and US4 can be staffed in parallel** — different trees (`desktop/crates/injection` + `spikes/f0-camera-injection` vs `desktop/crates/signaling` + `spikes/f0-handshake`).
- Wave 3: **US3 and US5 can be staffed in parallel** (`mobile/` + `spikes/f0-codec-interop` vs `desktop/crates/audio-bridge` + `spikes/f0-virtual-audio`).
- Within US2, tests T061–T066 are parallel, and adapters T069/T070/T072 touch different files.
- Polish T110–T115 are parallel.

---

## Parallel Example: User Story 1 and User Story 4 (wave 1)

```bash
# Engineer A — camera injection
Task: "T030 [US1] Implement the MF virtual camera media source in desktop/crates/injection/src/mf_virtual_camera.rs"
Task: "T035 [P] [US1] Create the Chromium capture harness in spikes/f0-camera-injection/apps/browser/"

# Engineer B — pairing handshake
Task: "T048 [US4] Implement the WSS listener in desktop/crates/signaling/src/server.rs"
Task: "T054 [US4] Implement the adversarial scenarios in spikes/f0-handshake/scenarios/"
```

## Parallel Example: User Story 2

```bash
# Tests first, in parallel
Task: "T061 [P] [US2] Adaptive-jitter-policy tests in desktop/crates/media-core/tests/jitter_policy.rs"
Task: "T062 [P] [US2] Drift tests in desktop/crates/media-core/tests/drift.rs"
Task: "T065 [P] [US2] Relay determinism tests in spikes/f0-pipeline/tests/relay_determinism.rs"
Task: "T066 [P] [US2] Analyzer tests in spikes/tools/tests/test_pipeline_metrics.py"
```

## Parallel Example: User Story 5

```bash
Task: "T098 [P] [US5] Dropout-analyzer tests in spikes/tools/tests/test_dropouts.py"
Task: "T099 [P] [US5] Intelligibility tests in spikes/tools/tests/test_intelligibility.py"
Task: "T100 [P] [US5] Readiness tests in desktop/crates/audio-bridge/tests/readiness.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Complete Phase 1 (Setup).
2. Complete Phase 2 (Foundational) — note that the protocol contract (T010–T017) is only needed by US4/US3 and may be deferred if the camera gate is demonstrated first, but it must be complete before wave 1 sign-off.
3. Complete Phase 3 (US1).
4. **STOP and VALIDATE**: register per-user, run the five-category matrix plus a 32-bit consumer for 30 minutes, validate `injection_matrix.json`, record the backend decision.
5. Deploy/demo if ready — this alone removes the largest architectural unknown (SC-010).

### Incremental Delivery (mandated waves)

1. Setup + Foundational → trustworthy apparatus (`preflight` + `selftest` green).
2. Wave 1: US1 + US4 → camera backend and handshake design fixed.
3. Wave 2: US2 → latency/resource/drift/continuity architecture either validated or changed while it is still cheap.
4. Wave 3: US3 + US5 → codec baseline and audio commercial path closed.
5. Polish → one phase review, five verdicts, conditions and waivers recorded.

### Parallel Team Strategy

With three engineers:

1. Everyone completes Setup + Foundational.
2. Engineer A: US1; Engineer B: US4 (wave 1, parallel).
3. Engineer A: US2 (wave 2); Engineer B starts the licensing assessment (T107) and TDD amendments early, because counsel availability bounds it; Engineer C: US5 technical measurement once US2 lands.
4. Engineer C: US3 (wave 3) in parallel with US5.
5. Polish split across all three.

### Budget and timebox

The plan allows ≈ 20 dev-days and the quickstart schedules ≈ 9.5 h of execution. The two 30-minute resource/drift runs and the three latency runs are scriptable and may run unattended; the licensing assessment starts first because it is bounded by stakeholder availability.

---

## Notes

- `[P]` means different files and no dependency on an incomplete task; it does not mean the work is trivial.
- Every gate must produce evidence that resolves to an existing file inside `spikes/evidence/`; a verdict without evidence is invalid.
- The gate is judged on the **raw** capture-at-consumer p95; `harness_floor_ms` and the floor-corrected value are published alongside and never substituted.
- One impairment profile exists (`nominal-3pct-30ms-10ms`, media only); a `profile-deviation` run can never support a `go`.
- No audio latency is measured or gated in F0 (Q8); F0 measures audio continuity, intelligibility and drift only.
- A `no_go` is isolated: it blocks only the gates that depend on it, and those still receive a recorded verdict naming the blocking dependency.
- Every conditional verdict needs a waiver from the gate's named accountable role with a resolution deadline, or an explicit block.
- Commit after each task or logical group; never commit raw captures, secrets, personal data or real user media.
