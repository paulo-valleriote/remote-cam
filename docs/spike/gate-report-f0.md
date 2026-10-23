# F0 Gate Report — Phase 8 Polish

**Feature:** 001-f0-technical-spikes
**Phase review at:** 2026-11-06T00:00:00+00:00
**Report version:** 1

## Reference environment

- Environment ID: windows-amd64-8fd38fab
- Captured at: 2026-10-22T21:48:36+00:00
- CPU: Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz (12 logical cores)
- GPU: NVIDIA GeForce RTX 3050
- OS: Windows 11 10.0.26200
- Android device: not_connected
- Within reference conditions: False
- Hardware decode: H.264 | decoder_found: False | hardware_accelerated: False | output_format: other

## Gate verdicts

| Gate | Wave | Depends on | Verdict | Accountable role | Failure code |
|------|------|------------|---------|------------------|--------------|
| camera_injection | 1 | foundational | **no_go** | technical_lead — not_yet_named | environment_out_of_reference |
| pairing_handshake | 1 | foundational | **conditional** | security_reviewer — not_yet_named | evidence_incomplete |
| desktop_receive_pipeline | 2 | camera_injection, pairing_handshake | **no_go** | technical_lead — not_yet_named | precondition_failed |
| codec_interoperability | 3 | desktop_receive_pipeline | **conditional** | technical_lead — not_yet_named | environment_out_of_reference |
| virtual_audio_and_licensing | 3 | desktop_receive_pipeline | **conditional** | product_owner — not_yet_named | decision_unresolved |

## Gate details

### camera_injection — Virtual camera injection across the target application matrix

- **Wave:** 1
- **Verdict:** **no_go**
- **Failure code:** environment_out_of_reference
- **Accountable role:** technical_lead — not_yet_named
- **Criterion:** 100 % frames_delivered on the selected backend, observed_duration_s >= 1800, longest_stall_ms <= 2000, both bitnesses, multi-consumer observation
- **Measurement conditions:** Clean Windows 11 x64, non-administrator account, five application categories plus a 32-bit consumer, 30 minutes per cell. Not reproducible on this host.
- **Rationale:** The injection matrix requires the Windows 11 reference machine with Media Foundation, FFmpeg, OBS, the conferencing client, Windows Camera, a screen recorder and a 32-bit consumer. None are available on this host (environment.json records within_reference_conditions: false and no camera or FFmpeg). injection_matrix.json does not exist, so the backend cannot be selected and gate 1 has no measured evidence.
- **Evidence refs:** none

### pairing_handshake — Authenticated pairing handshake (TLS 1.3, fingerprint-pinned, token-authenticated)

- **Wave:** 1
- **Verdict:** **conditional**
- **Failure code:** evidence_incomplete
- **Accountable role:** security_reviewer — not_yet_named
- **Criterion:** Valid scenario accepted in <= 5 s; all adversarial scenarios rejected with closed reason codes; no media before authentication; per-origin throttling (5/minute, 6th refused, progressive blocks, listener close at 10 consecutive failures); second origin unaffected; zero residue after session end and after application restart using the same inspection method.
- **Measurement conditions:** Harness-only on the F0 host for pairing and throttling; residue restart scan requires the Windows 11 and Android reference devices.
- **Rationale:** All 13 pairing scenarios are recorded (1 accepted, 12 rejected) with closed reason codes and zero media before authentication. Throttling evidence shows attempt 6 refused, progressive blocks, listener closed at 10 consecutive failures, and a second legitimate origin unaffected. The residue inspection after application restart requires two named reference devices (Windows 11 and Android) that are not available, so after_application_restart_artefacts_found cannot be produced with the identical method. Gate 2 is conditional on the residue-restart inspection.
- **Evidence refs:** spikes/evidence/pairing/pairing_scenarios.json, spikes/evidence/pairing/pairing_throttling.json
- **Preconditions:** PC-5, PC-6

- **Throttling summary:**
  - Attempt limit per minute: 5
  - Limit exceeded at attempt: 6
  - Consecutive failures at listener close: 10
  - Listener closed: True
  - Session established: True
  - Legitimate pairing unaffected: True
  - Evidence ref: spikes/evidence/pairing/pairing_throttling.json
- **Residue summary:**
  - Method: byte scanner plus named platform probes (residue_scan.py --probe)
  - Coverage: process memory, app-data directory and journals, platform credential/certificate store, HKCU\Software\<vendor>\<app> on both devices
  - After session end artefacts found: []
  - After application restart artefacts found: []

- **Conditions:**
  - C-02-residue-restart: Residue inspection after application restart must be empty using the same method as the session-end inspection, on two named reference devices. (status: blocking, conditioned phases: F1)

### desktop_receive_pipeline — Desktop receive pipeline: latency, resilience, resources, drift, stall recovery

- **Wave:** 2
- **Verdict:** **no_go**
- **Failure code:** precondition_failed
- **Blocking dependency:** camera_injection
- **Accountable role:** technical_lead — not_yet_named
- **Criterion:** Raw capture-at-consumer p95 <= 150 ms across three >= 10-minute runs; harness floor and floor-corrected p95 published; CPU p95 <= 15 %; private working set max <= 400 MB; drift <= 20 ms/h with certifying interval; zero dropouts >= 50 ms under the pinned profile; stall recovery <= 2 s.
- **Measurement conditions:** Three >= 10-minute latency runs, one loss/jitter run, one 0 %-loss control, one 30-minute resource run, one 30-minute drift run, one 5 s stall. Not reproducible on this host (Windows 11 reference required).
- **Rationale:** PC-1 (hardware decode, NV12 output) failed: environment.json records hardware_accelerated: false and output_format_supported: other. The pipeline apparatus is complete and tested on Linux CI, but the live measurement runs require the Windows 11 reference host with a verified hardware H.264 decoder. run_matrix.json is blocked_by_environment and no latency/resource/drift/stall evidence exists.
- **Evidence refs:** spikes/evidence/pipeline/run_matrix.json, spikes/evidence/toolchain_selftest.json
- **Preconditions:** PC-1, PC-2, PC-3, PC-4

- **Included latency stages:** capture, encode, network_and_jitter_buffer, decode, frame_handoff, consumer_presentation
- **Excluded latency stages:** none
- **Harness floor (ms):** 10.98
- **Raw latency p95 (ms):** 10.98
- **Floor-corrected latency p95 (ms):** 0.0
- **Latency attribution:** Not attributable to the product: the only measured capture-at-consumer p95 (10.98 ms) is the harness floor calibration itself, a software-only selftest baseline on this host; the floor-corrected value is 0.0 by construction. No live pipeline capture exists, so product latency is unmeasured and this figure must be replaced by the documented capture floor on the reference Windows machine.

### codec_interoperability — Codec interoperability: mobile-to-desktop H.264/VP8 negotiation

- **Wave:** 3
- **Verdict:** **conditional**
- **Failure code:** environment_out_of_reference
- **Blocking dependency:** desktop_receive_pipeline
- **Accountable role:** technical_lead — not_yet_named
- **Criterion:** Live video from the reference Android device decodes on the desktop for 10 continuous minutes with agreed profile/level/format and no visible corruption; VP8 fallback approved with measured CPU, latency and quality trade-offs and hardware-decode status recorded.
- **Measurement conditions:** Reference Android device streaming live H.264/VP8 to the desktop receive pipeline for 10 continuous minutes; corruption detection automated; fallback comparison run recorded. Not reproducible on this host.
- **Rationale:** D-codec-baseline recommends H.264 High Profile Level 3.1 as the default with VP8 as the fallback. The interop harness (f0-codec-interop) is built and the corruption detector is implemented, but the live 10-minute run requires the reference Android device, which is not connected (environment.json records android_device_model: not_connected). Gate 4 is conditional on the live interop run and Q-7 (1080p level >= 4.0 vs level 3.1 receiver limits).
- **Evidence refs:** spikes/evidence/decisions/D-codec-baseline.md
- **Preconditions:** PC-1

- **Conditions:**
  - C-04-interop-live-run: 10-minute live codec interop run on the reference Android device with zero visible corruption. (status: blocking, conditioned phases: F1, F2)

### virtual_audio_and_licensing — Virtual audio path and licensing decision

- **Wave:** 3
- **Verdict:** **conditional**
- **Failure code:** decision_unresolved
- **Blocking dependency:** desktop_receive_pipeline
- **Accountable role:** product_owner — not_yet_named
- **Criterion:** Zero dropouts >= 50 ms with agreeing detectors over >= 10 minutes under the pinned profile; phrases_exact >= 18 with all nonces/anchors matching; named observer and phrase script; consumer_renegotiations = 0; verified device signature; video-only degradation proven; licensing decision or explicit conditional/no_go with plan B.
- **Measurement conditions:** Render scripted audio through VB-CABLE under the pinned impairment profile (3 % loss, 30 ms latency, +/- 10 ms jitter), record CABLE Output in a consumer application for >= 10 minutes, two-detector dropout analysis, 20-phrase intelligibility manifest with a named observer. Not reproducible on this host.
- **Rationale:** The audio harness, dropout analyser, intelligibility workflow and licensing assessment are complete and tested on Linux CI. The live measurement requires Windows 11 with Secure Boot, VB-CABLE driver pack 45, FFmpeg, a consumer application and a named product owner/approver for the licensing decision. run_matrix.json is blocked_by_environment; licensing_bundle.json is not emitted because the contract requires named signatories. In the summary below, zero counts mean nothing was recorded (no live run happened), detectors_agree=false means the two detectors were never run in tandem (agreement not evaluated, not a detected disagreement), and signature_verified=false means the VB-CABLE signature probe did not execute. Gate 5 is conditional on the licensing decision and the live audio run.
- **Evidence refs:** spikes/evidence/audio/run_matrix.json, spikes/evidence/decisions/D-audio-licensing.md
- **Preconditions:** PC-3, PC-4

- **Audio measurement summary:**
  - Dropout count: 0
  - Detectors agree: False
  - Consumer renegotiations: 0
  - Phrases exact: 0
  - Corpus WER: 0.0
  - Observer: not named
  - Phrase script ref: not recorded
  - Device pin: VB-CABLE (driver pack 45)
  - Signature verified: False

- **Conditions:**
  - C-05-licensing-decision: Named product owner and approver must sign the licensing decision (VB-CABLE link-only with video-only plan B) by the gate date, or the gate is conditional/no_go. (status: blocking, conditioned phases: F1)

## Downstream status (F1 / F2 / F3)

- **F1:** blocked — One or more F0 gates are no_go; F1 cannot start until they are resolved.
  - Conditions: resolve all no_go/conditional F0 gates
- **F2:** blocked — F2 depends on F1 and on the gates it consumes (camera injection, pipeline, codec interop).
  - Conditions: resolve all no_go/conditional F0 gates
- **F3:** blocked — F3 depends on F2 and on all F0 gates.
  - Conditions: resolve all no_go/conditional F0 gates

## Decision index

| Decision | Topic | Status | Owner | Approver | Date | Path | Fallback |
|----------|-------|--------|-------|----------|------|------|----------|
| D-10 | Injection backend: MF virtual camera vs DirectShow filter | draft | Technical lead | not named | 2026-11-06 | spikes/evidence/decisions/D-10-injection-backend.md | dshow_filter (DirectShow capture filter) |
| D-codec-baseline | Video codec baseline for F1: H.264 vs VP8 | draft | Technical lead | Technical lead | 2026-10-22 | spikes/evidence/decisions/D-codec-baseline.md | VP8 software fallback |
| D-quality-default | Default video quality: 1280x720@30 default, 1920x1080@30 optional maximum | draft | Technical lead | not named | 2026-11-06 | spikes/evidence/decisions/D-quality-default.md | 1280x720@30 only (Option C) |
| D-audio-licensing | Virtual audio device sourcing and licensing | draft | Product owner | not named | 2026-11-06 | spikes/evidence/decisions/D-audio-licensing.md | Video-only V1 (plan B) |

## Open questions

| Question | Owner | Impact | Blocking | Status |
|----------|-------|--------|----------|--------|
| Q-1 | Technical lead | Decides the installer design and the no-admin promise for the camera | yes | open |
| Q-2 | Technical lead | Determines whether the audio gate can pass at all | yes | open |
| Q-3 | Product owner + legal | Determines whether V1 ships link-only or bundled, and the unit economics | yes | open |
| Q-4 | Product owner + legal | The cleaner long-term audio model | no | open |
| Q-5 | Technical lead | FR-044 precondition — without it, no latency or resource result is admissible | yes | open |
| Q-6 | Technical lead | Affects the shared-buffer design | no | open |
| Q-7 | Product owner | Affects bandwidth, CPU and the negotiated level | no | open |
| Q-8 | Technical lead | The DirectShow path needs separate per-bitness CLSIDs; the MF path behaviour is unknown | yes | open |
| Q-9 | Security reviewer | Defines the no-residue proof for the pairing gate | yes | open |
| Q-10 | Mobile engineer | A stride-2 device with JavaI420Buffer.wrap produces corrupt colour | no | open |

## Non-production statement

All artefacts under spikes/ are NON-PRODUCTION F0 risk-reduction spike artefacts (FR-039). They must not be merged into desktop/ or mobile/ product modules. Raw captures remain local-only; only derived metrics, scripts and procedures are versioned.
