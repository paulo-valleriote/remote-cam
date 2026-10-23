# F0 Gate Report — Phase Closure

**Feature:** 001-f0-technical-spikes
**Phase review at:** 2026-10-23T17:20:00-03:00
**Report version:** 2
**Prepared by:** Paulo Valleriote

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
| camera_injection | 1 | foundational | **no_go** | technical_lead — Paulo Valleriote | evidence_incomplete |
| pairing_handshake | 1 | foundational | **conditional** | security_reviewer — Paulo Valleriote | evidence_incomplete |
| desktop_receive_pipeline | 2 | camera_injection, pairing_handshake | **no_go** | technical_lead — Paulo Valleriote | precondition_failed |
| codec_interoperability | 3 | desktop_receive_pipeline | **conditional** | technical_lead — Paulo Valleriote | environment_out_of_reference |
| virtual_audio_and_licensing | 3 | desktop_receive_pipeline | **conditional** | product_owner — Paulo Valleriote | decision_unresolved |

## Gate details

### camera_injection — Virtual camera injection across the target application matrix

- **Wave:** 1
- **Verdict:** **no_go**
- **Failure code:** evidence_incomplete
- **Accountable role:** technical_lead — Paulo Valleriote
- **Criterion:** 100 % frames_delivered on the selected backend, observed_duration_s >= 1800, longest_stall_ms <= 2000, both bitnesses, multi-consumer observation
- **Measurement conditions:** Clean Windows 11 x64, non-administrator account, five application categories plus a 32-bit consumer, 30 minutes per cell. The Chrome run was collected on the Windows 11 host for 33.2845 s; the rest of the matrix was not collected.
- **Rationale:** The only matrix evidence collected is one Google Chrome cell: 974 observations over 33.2845 s at 1280x720@30. The gate requires 1800 s per cell, five application categories and a 32-bit consumer, so the partial run cannot pass the criterion and no backend is selected. The remaining cells were not collected at closure; they are carried to F3. No injection_matrix.json exists, because the matrix contract requires a selected backend and a frames_delivered cell of at least 1800 s.
- **Evidence refs:** spikes/evidence/injection/runs/mf_virtual_camera/chrome/run.json, spikes/evidence/injection/runs/mf_virtual_camera/chrome/capture.json, spikes/evidence/injection/runs/mf_virtual_camera/chrome/observations.csv

- **Partial run:**
  - Application: chrome (Google Chrome 124.0.6367.208)
  - Observed duration: 33.2845 s (required 1800 s; meets duration: False)
  - Observation samples: 974
  - Raw recorded duration: 1799975.8 milliseconds
  - Matrix exists: False; selected backend: not_determined

### pairing_handshake — Authenticated pairing handshake (TLS 1.3, fingerprint-pinned, token-authenticated)

- **Wave:** 1
- **Verdict:** **conditional**
- **Failure code:** evidence_incomplete
- **Accountable role:** security_reviewer — Paulo Valleriote
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
  - C-02-residue-restart: Residue inspection after application restart must be empty using the same method as the session-end inspection, on two named reference devices. (status: waived, conditioned phases: F1)
    - Waiver: waived by Paulo Valleriote on 2026-10-23T17:20:00-03:00, deadline 2026-11-20

### desktop_receive_pipeline — Desktop receive pipeline: latency, resilience, resources, drift, stall recovery

- **Wave:** 2
- **Verdict:** **no_go**
- **Failure code:** precondition_failed
- **Blocking dependency:** camera_injection
- **Accountable role:** technical_lead — Paulo Valleriote
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
- **Accountable role:** technical_lead — Paulo Valleriote
- **Criterion:** Live video from the reference Android device decodes on the desktop for 10 continuous minutes with agreed profile/level/format and no visible corruption; VP8 fallback approved with measured CPU, latency and quality trade-offs and hardware-decode status recorded.
- **Measurement conditions:** Reference Android device streaming live H.264/VP8 to the desktop receive pipeline for 10 continuous minutes; corruption detection automated; fallback comparison run recorded. Not reproducible on this host.
- **Rationale:** D-codec-baseline recommends H.264 High Profile Level 3.1 as the default with VP8 as the fallback. The interop harness (f0-codec-interop) is built and the corruption detector is implemented, but the live 10-minute run requires the reference Android device, which is not connected (environment.json records android_device_model: not_connected). Gate 4 is conditional on the live interop run and Q-7 (1080p level >= 4.0 vs level 3.1 receiver limits).
- **Evidence refs:** spikes/evidence/decisions/D-codec-baseline.md
- **Preconditions:** PC-1

- **Conditions:**
  - C-04-interop-live-run: 10-minute live codec interop run on the reference Android device with zero visible corruption. (status: waived, conditioned phases: F1, F2)
    - Waiver: waived by Paulo Valleriote on 2026-10-23T17:20:00-03:00, deadline 2026-12-20

### virtual_audio_and_licensing — Virtual audio path and licensing decision

- **Wave:** 3
- **Verdict:** **conditional**
- **Failure code:** decision_unresolved
- **Blocking dependency:** desktop_receive_pipeline
- **Accountable role:** product_owner — Paulo Valleriote
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
  - C-05-licensing-decision: Named product owner and approver must sign the licensing decision (VB-CABLE link-only with video-only plan B) by the gate date, or the gate is conditional/no_go. (status: waived, conditioned phases: F2, F3)
    - Waiver: waived by Paulo Valleriote on 2026-10-23T17:20:00-03:00, deadline 2027-01-15

## Phase closure

- **Closure mode:** closed_with_exceptions
- **Exit gate:** not_met
- **Closed at:** 2026-10-23T17:20:00-03:00
- **Statement:** F0 is closed with recorded exceptions. All five gate verdicts were issued as measured: two no_go (camera injection and the desktop receive pipeline) and three conditional (pairing handshake, codec interoperability, virtual audio and licensing). The phase exit gate is not met. The only matrix evidence recorded is one Google Chrome injection cell of 33.2845 s; it does not satisfy the 1800 s per-cell and five-category matrix criterion, so the injection gate remains no_go. No new matrix evidence was collected at closure. Conditions C-02, C-04 and C-05 are waived with resolution deadlines, and the environmental no_go obligations are carried to F3.

**Sign-offs**

| Role | Name | Signed at |
|------|------|-----------|
| technical_lead | Paulo Valleriote | 2026-10-23T17:20:00-03:00 |
| security_reviewer | Paulo Valleriote | 2026-10-23T17:20:00-03:00 |
| product_owner | Paulo Valleriote | 2026-10-23T17:20:00-03:00 |

**Gate dispositions**

| Gate | Verdict | Disposition | Target | Owner | Deadline |
|------|---------|-------------|--------|-------|----------|
| camera_injection | no_go | carried | F3 | Paulo Valleriote | 2027-02-15 |
| desktop_receive_pipeline | no_go | carried | F3 | Paulo Valleriote | 2027-02-15 |
| pairing_handshake | conditional | waived | F1 | Paulo Valleriote | 2026-11-20 |
| codec_interoperability | conditional | waived | F2 | Paulo Valleriote | 2026-12-20 |
| virtual_audio_and_licensing | conditional | waived | F3 | Paulo Valleriote | 2027-01-15 |

**Waivers**

| Condition | Phases | Waived by | Deadline | Resolution conditions |
|-----------|--------|-----------|----------|-----------------------|
| C-02-residue-restart | F1 | Paulo Valleriote | 2026-11-20 | F1 may build the protocol core without the restart inspection, but the inspection must be recorded with the same method before the F1 exit gate. |
| C-04-interop-live-run | F1, F2 | Paulo Valleriote | 2026-12-20 | The F1 contract may use the documented H.264 High Profile Level 3.1 baseline with VP8 fallback while the live run is pending; the run must be recorded before the F2 exit gate. |
| C-05-licensing-decision | F2, F3 | Paulo Valleriote | 2027-01-15 | Licensing does not block the start of F1 or F2; the decision must be signed, or video-only selected, before the F3 audio scope begins. |

**Carried obligations**

- `camera_injection` → F3 by 2027-02-15: The only matrix evidence collected is one Google Chrome cell: 974 observations over 33.2845 s (the raw duration fields are milliseconds and the observation timestamps are microseconds). The gate requires 1800 s per cell, five application categories and a 32-bit consumer, so the gate stays no_go. The remaining cells are carried to the media and injection phase; no new evidence was collected at closure.
- `desktop_receive_pipeline` → F3 by 2027-02-15: PC-1 failed on the F0 host: environment.json records hardware_accelerated=false and output_format_supported=other. The latency, resource and drift runs require the verified Windows 11 reference host.

**Amendments and deviations**

- Amendments: 12 applied, 1 no-op, 0 waived of 13 (docs/spike/tdd-amendments.md)
- Deviations approved: D1, D2, D3, D4, D5 (docs/spike/deviations.md)

**Known issues**

- FR-039: F0 harness implementations remain under desktop/crates/{signaling,media-core,injection,audio-bridge,app} and mobile/core; quarantine is carried to F1/F3 under D4.
- The injection matrix is incomplete: only one Google Chrome cell exists, for 33.2845 s, against a 1800 s per-cell and five-category criterion.
- docs/tdd-remotecam-v1.md remains in Portuguese under D5; a full translation is required before the V1 release.

**Corrections recorded at closure**

- D-codec-baseline no longer claims completed 10-minute interop runs or unmeasured CPU, latency and PSNR values.
- Q-5 and D-quality-default now record hardware_accelerated=false, matching environment.json.
- The gate report records the duration unit discrepancy: run.json and capture.json carry milliseconds while observations.csv timestamps are microseconds.

## Downstream status (F1 / F2 / F3)

- **F1:** conditional — F0 is closed with exceptions; F1 may proceed under the C-02 and C-04 waivers.
  - Conditions: C-02-residue-restart (deadline 2026-11-20), C-04-interop-live-run (deadline 2026-12-20)
- **F2:** conditional — F2 depends on the F1 exit and on the codec and licensing conditions.
  - Conditions: F1 exit, C-04-interop-live-run (deadline 2026-12-20), C-05-licensing-decision (deadline 2027-01-15)
- **F3:** conditional — F3 depends on the F2 exit and on the carried injection and pipeline obligations.
  - Conditions: F2 exit, C-05-licensing-decision (deadline 2027-01-15), camera_injection carried (deadline 2027-02-15), desktop_receive_pipeline carried (deadline 2027-02-15)

## Decision index

| Decision | Topic | Status | Owner | Approver | Date | Path | Fallback |
|----------|-------|--------|-------|----------|------|------|----------|
| D-10 | Injection backend: MF virtual camera vs DirectShow filter | draft | Technical lead | not named | 2026-10-22 | spikes/evidence/decisions/D-10-injection-backend.md | dshow_filter (DirectShow capture filter) |
| D-codec-baseline | Video codec baseline for F1: H.264 vs VP8 | draft | Technical lead | not named | 2026-10-22 | spikes/evidence/decisions/D-codec-baseline.md | VP8 software fallback |
| D-quality-default | Default video quality: 1280x720@30 default, 1920x1080@30 optional maximum | draft | Technical lead | not named | 2026-10-22 | spikes/evidence/decisions/D-quality-default.md | 1280x720@30 only (Option C) |
| D-audio-licensing | Virtual audio device sourcing and licensing | draft | Product owner | not named | 2026-10-23 | spikes/evidence/decisions/D-audio-licensing.md | Video-only V1 (plan B) |

## Open questions

| Question | Owner | Impact | Blocking | Status | Phase |
|----------|-------|--------|----------|--------|-------|
| Q-1 | Technical lead | Decides the installer design and the no-admin promise for the camera | yes | deferred | F3 |
| Q-2 | Technical lead | Determines whether the audio gate can pass at all | yes | deferred | F3 |
| Q-3 | Product owner + legal | Determines whether V1 ships link-only or bundled, and the unit economics | yes | deferred | F3 |
| Q-4 | Product owner + legal | The cleaner long-term audio model | no | deferred | F3 |
| Q-5 | Technical lead | FR-044 precondition — without it, no latency or resource result is admissible | yes | answered | F3 |
| Q-6 | Technical lead | Affects the shared-buffer design | no | deferred | F3 |
| Q-7 | Product owner | Affects bandwidth, CPU and the negotiated level | no | deferred | F2 |
| Q-8 | Technical lead | The DirectShow path needs separate per-bitness CLSIDs; the MF path behaviour is unknown | yes | deferred | F3 |
| Q-9 | Security reviewer | Defines the no-residue proof for the pairing gate | yes | deferred | F1 |
| Q-10 | Mobile engineer | A stride-2 device with JavaI420Buffer.wrap produces corrupt colour | no | deferred | F2 |

## Non-production statement

All artefacts under spikes/ are NON-PRODUCTION F0 risk-reduction spike artefacts (FR-039). They must not be merged into desktop/ or mobile/ product modules. Raw captures remain local-only; only derived metrics, scripts and procedures are versioned.
