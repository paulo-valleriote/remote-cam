# Quickstart: Validating F0 — Risk-Reduction Spikes and Technical Decisions

**Feature**: [001-f0-technical-spikes](./spec.md) | **Plan**: [plan.md](./plan.md) | **Research**: [research.md](./research.md) | **Data model**: [data-model.md](./data-model.md) | **Contracts**: [`contracts/`](./contracts/)

This guide is the **runnable validation** of F0. It is written so that a second engineer, who has never seen the spike, can reproduce every gate result **within one working day** using only the recorded procedure and the environment description (FR-038, SC-009).

It is a **validation and run guide**, not an implementation guide. The measurement file formats are specified in [`contracts/measurement-formats.md`](./contracts/measurement-formats.md); the entity semantics are in [`data-model.md`](./data-model.md); the interface contracts are in [`contracts/`](./contracts/). The task breakdown and the implementation belong to `tasks.md` and the implementation phase.

---

## 0. What you are proving

Five gates, three waves, five verdicts at one phase review by **2026-11-06**.

| # | Gate | Wave | Depends on | Accountable role | Pass criterion (abbreviated) |
| --- | --- | --- | --- | --- | --- |
| 1 | Virtual camera injection | 1 | — | Technical lead | 100 % of the target matrix sees stable frames for 30 min; both 32-bit and 64-bit consumers; no stall > 2 s. |
| 2 | Pairing handshake | 1 | — | Security reviewer | 100 % of adversarial scenarios rejected, the valid one succeeds, no media before authentication, no credential residue, **throttling proved, no residue after a session end or a restart**. |
| 3 | Desktop receive pipeline | 2 | 1, 2 | Technical lead | **raw p95 capture-at-consumer ≤ 150 ms** (floor and floor-corrected value published alongside); CPU ≤ 15 %; memory ≤ 400 MB; drift ≤ 20 ms/h; audio continuous under the nominal impairment profile. |
| 4 | Codec interoperability | 3 | 3 | Technical lead | Preferred codec interoperable for 10 min without corruption, or the fallback approved with measured trade-offs. |
| 5 | Virtual audio and licensing | 3 | 3 | Product owner | Audio intelligible and continuous for 10 min under the nominal impairment profile, **and** a licensing decision or an approved plan B. |

**Wave discipline**: a `no_go` in wave 1 does **not** stop wave 2 — it is isolated and reported. A gate whose dependency failed **still receives a recorded verdict** that names the blocking dependency (FR-046, FR-047).

**Preconditions**. Six plan preconditions guard the gates. A precondition that fails stops the gate it guards and does **not** stop the gates that do not depend on it.

| # | Precondition | Guards |
| --- | --- | --- |
| PC-1 | Hardware-accelerated H.264 decode verified on the reference machine **before** the pipeline gate | 3 |
| PC-2 | The latency figure is judged on the **raw** capture-at-consumer p95; the harness floor and the floor-corrected value are published alongside it | 3 |
| PC-3 | Audio continuity and intelligibility are judged by **automated dropout detection** on the recorded capture **plus** a **scripted human check** with a recorded observer | 5 |
| PC-4 | The loss-and-jitter test uses exactly **one** impairment profile: 3 % uniform loss, 30 ms added latency, ±10 ms jitter, media only | 3, 5 |
| PC-5 | The pairing gate proves the **per-origin attempt limit**, **progressive blocking** and **listener shutdown** | 2 |
| PC-6 | The pairing gate proves **zero pairing artefacts** after a session end **and** after an application restart, with the **same inspection method** | 2 |

---

## 1. Prerequisites

### 1.1 Hardware

| Item | Requirement | Note |
| --- | --- | --- |
| Reference PC | Windows 11 x64, build 22000 or later (22621 recommended) | Must be able to run the Media Foundation hardware decode path. |
| Reference PC CPU/GPU | Record the exact model | A "modern CPU" is not a record. |
| Reference Android device | Mid-range, 2021 or later | The interop reference device. |
| Network | 5 GHz Wi-Fi 5 or better, within 5 m of the access point, internet **disabled** at the router | 2.4 GHz or a congested channel ⇒ the run is labelled outside the reference environment and cannot pass a gate. |
| Optional | A second Android device or laptop speaker | For the scripted intelligibility playback. |

### 1.2 Software

| Tool | Version | Purpose |
| --- | --- | --- |
| Rust | 1.92 or later, edition 2024 | Desktop harness and measurement sampler. |
| JDK | 17 | Android build. |
| Android SDK | compileSdk 36, build-tools 36.0.0 | Android build. |
| Python | 3.12 or later | Analysis tooling. |
| FFmpeg | **8.1 or later** | `silencedetect`, `qrencode`, `gdigrab`, `astats`. Below 5.0, `silencedetect` is per-frame rather than sample-accurate. |
| `iperf3` | current | Network baseline. |
| MSVC build tools | current | Only if the DirectShow fallback backend is compiled. |

Python packages: `numpy`, `scipy`, `zxing-cpp`, `jiwer`, `soundfile`, `pystoi`, `pyloudnorm`.

### 1.3 Windows configuration

- The phone's **Wi-Fi power save must be disabled** and the fact recorded. DTIM power save adds 100+ ms of latency and makes every latency number meaningless.
- Secure Boot **enabled**, so that the virtual audio driver's signature status is tested in the state a consumer would see.
- No other application holding the camera during a run.

### 1.4 Virtual audio device

Install the **pinned** device for the technical measurement: **VB-CABLE**, base package, driver pack 45. Record the exact product and version in the procedure — the evidence stays valid only for the device actually measured. Do **not** redistribute the installer.

---

## 2. Repository layout and build

```bash
git clone <repository> remotecam
cd remotecam

# The root Makefile is the constitution's required entry point for every
# documented command. Run `make help` for the full list.
make help
```

### 2.1 Targets expected to exist

| Target | Purpose |
| --- | --- |
| `make preflight` | Records the reference environment and verifies hardware decode. Produces `spikes/evidence/environment.json`. |
| `make selftest` | Runs the toolchain self-test. Produces `spikes/evidence/toolchain_selftest.json`. **Must pass before any measurement is trusted.** |
| `make build-desktop` / `make build-mobile` | Builds the harnesses. |
| `make test` | Unit and contract tests across both stacks. |
| `make coverage` | Coverage report; enforces the 90 % gate on the delivered packages. |
| `make lint` | Linters for both stacks. |
| `make spike-handshake` | Runs the pairing gate scenarios, the throttling scenarios and the residue inspections. |
| `make spike-pipeline` | Runs the latency, resource, drift and stall scenarios. |
| `make spike-injection` | Runs the per-application injection matrix. |
| `make spike-interop` | Runs the codec interoperability scenario. |
| `make spike-audio` | Runs the virtual audio scenario. |
| `make gate-report` | Assembles the gate report from the evidence. |
| `make clean` | Removes generated artefacts. Never removes committed evidence. |

Every command documented in a README **must** exist as a Makefile target. If a command is missing, the merge is blocked by the constitution.

---

## 3. Phase A — Preflight (≈ 45 min)

**Purpose**: produce the evidence that makes every later measurement admissible and reproducible.

```bash
make preflight
```

Expected outcome:

- `spikes/evidence/environment.json` exists and contains the exact CPU model, logical core count, GPU model, OS build, Android device model, application versions, tool versions, and the network baseline.
- `hardware_decode_verification.hardware_accelerated` is `true` **and** `output_format_supported` is `nv12`.
- `network.wifi_power_save` is `disabled`.
- `network.internet_enabled` is `false`.
- `network.within_reference_conditions` is `true`.

**Stop condition — hardware decode (PC-1).** If `hardware_accelerated` is `false`, the reference machine **MUST be replaced** (FR-044). Software-decode results are reported separately and **cannot** be used to pass the latency gate. Do not continue with the substituted machine's numbers. The verification result is recorded as **gate evidence** and is a precondition of the pipeline gate.

**Stop condition — network.** If `within_reference_conditions` is `false`, every run from this point is labelled `outside-reference-environment` and cannot be used for a `go` verdict.

---

## 4. Phase B — Toolchain self-test (≈ 45 min)

**Purpose**: prove the measurement apparatus measures what it claims, before trusting any of its output.

```bash
make selftest
```

Expected outcome — all four checks pass:

| Check | Expected |
| --- | --- |
| `silence_fixture` | The committed fixture (a 40 ms gap, a 60 ms gap, a 120 ms gap, continuous speech between them) yields **exactly 2** dropouts — the 60 ms and the 120 ms gaps — and the 40 ms gap is **not** counted. |
| `qr_roundtrip` | The generated marker decodes back to a byte-identical payload; the decoded SHA-256 matches the generated one. |
| `sampler_agreement` | The Rust sampler's p95 CPU and the `typeperf` cross-check agree **within 1 percentage point** after normalisation. |
| `harness_floor` | The apparatus's own latency floor is measured and recorded. Expect roughly **17–50 ms** — the same order as the entire latency budget. This number is the uncertainty on the gate figure and MUST appear in the report. |

**Stop condition**: if any check fails, **no measurement result is valid**. Fix the toolchain and re-run. The failure of the `sampler_agreement` check in particular means the CPU normalisation is wrong, which would silently invalidate the resource gate.

---

## 5. Phase C — Gate 1: virtual camera injection (≈ 90 min)

**Procedure**: `docs/spike/gate-1-camera-injection.md`.

```bash
make spike-injection
```

### 5.1 Target matrix

Every category must be represented, with **both bitnesses** where the category has them:

| Category | Product to select | Capture method |
| --- | --- | --- |
| Chromium-based browser | Chrome or Edge | `getUserMedia` + `MediaRecorder` in a local page |
| Video conferencing client | The team's conferencing client | Screen capture of the window |
| OBS | OBS Studio | `obs-websocket` start/stop recording |
| Windows Camera | The inbox app | Screen capture of the window |
| Screen recorder | The team's recorder | Native recording |

Record the exact product and version for each.

### 5.2 Backends

Evaluate **both** backends and record per-application results for each:

- `mf_virtual_camera` — the Windows 11 virtual camera API.
- `dshow_filter` — the DirectShow fallback. **Only required if any cell of the primary backend fails**, but the matrix must contain results for whichever backends were evaluated.

### 5.3 Expected outcome

`spikes/evidence/injection/injection_matrix.json` conforms to [`contracts/injection-matrix.schema.json`](./contracts/injection-matrix.schema.json) and satisfies:

- Every cell for the selected backend has `outcome = frames_delivered`.
- `observed_duration_s ≥ 1800` in every such cell.
- `delivered_size_ok` and `delivered_rate_ok` are `true`.
- `longest_stall_ms ≤ 2000`.
- A 32-bit consumer is present in the matrix and sees the device. A 32-bit consumer that does not see it is a **matrix failure**, not an acceptable limitation.
- `selected_backend` and `decision_rationale` are recorded, along with `licensing_impact` and `media_phase_consequences`.
- `multi_consumer_observation` records what two simultaneous consumers observed.

### 5.4 Registration questions that must be answered here

These are the open questions from [`research.md`](./research.md) §3 that belong to this gate:

1. Does per-user registration work without elevation?
2. Does it require an MSI installer rather than MSIX? Record what the installer must be.
3. Does a 32-bit consumer see the device on 64-bit Windows, for **each** backend?

---

## 6. Phase D — Gate 2: pairing handshake (≈ 60 min)

**Procedure**: `docs/spike/gate-2-pairing-handshake.md`.

```bash
make spike-handshake
```

### 6.1 Scenarios that must be executed

| Scenario | Expected outcome | Reason code |
| --- | --- | --- |
| Valid pairing | Accepted | — |
| Wrong fingerprint (one byte differs) | Rejected | `certificate_pin_mismatch` |
| Chain-only trust attempt | Rejected | `certificate_chain_rejected` |
| TLS downgrade attempt | Rejected | `tls_version_unsupported` |
| Expired pairing token | Rejected | `token_expired` |
| Reused pairing token (replay) | Rejected | `token_already_used` |
| Wrong token | Rejected | `token_invalid` |
| Connection from a non-private address | Rejected | `source_address_not_private` |
| Interception proxy | Rejected | `certificate_pin_mismatch` or `tls_handshake_failed` |
| QR expired before scanning | Rejected | `qr_expired` |
| Malformed QR payload | Rejected | `invalid_qr_payload` |
| QR with a public endpoint | Rejected | `no_private_endpoint` |
| **6th attempt from one origin within a minute** | **Refused** | `rate_limited` |
| **10 consecutive failures from one origin** | **Listener closed** | `listener_closed` |
| **Flood from one origin while a second origin pairs legitimately** | **The legitimate pairing succeeds** | — |

### 6.2 Expected outcome

`spikes/evidence/pairing/pairing_scenarios.json`:

- Every scenario is present and its `reason_code` is a member of the closed set in [`contracts/reason-codes.md`](./contracts/reason-codes.md).
- `media_flowed_before_authentication` is `false` in **every** scenario.
- `residual_credentials` is empty in **every** scenario.
- No rejected scenario was followed by an unauthenticated fallback. The absence of a fallback path is itself an assertion.
- `established_channel_ms` and `pinning_added_delay_ms` are recorded for the valid scenario.

### 6.3 Throttling evidence (PC-5)

`spikes/evidence/pairing/pairing_throttling.json` proves the attempt limit, the progressive blocking and the listener shutdown. A configuration value is not evidence.

Expected outcome:

- `attempt_limit_per_minute` is **5** and `limit_exceeded_at_attempt` is **6** — the sixth attempt within the minute is refused.
- `blocked` is `true` and `block_duration_s` **increases** across successive failures.
- `consecutive_failures_at_listener_close` is **10** and `listener_closed` is `true`.
- `session_established` is **false** for the throttled origin.
- `legitimate_pairing_unaffected` is **true**: the second origin paired successfully while the first was being throttled.
- Every entry in `decisions` carries a reason code from the closed set **and** an observed outcome.
- The listener closure is recorded as a **security event**, not as a product failure.

### 6.4 Residue inspection (PC-6)

`spikes/evidence/pairing/pairing_residue.json` proves that no pairing artefact survives — in **both** cases, using the **same** inspection method.

Expected outcome:

- **Both** inspections are present: `after_session_end` and `after_application_restart`.
- Both name the **same** `method`, and the `coverage` states what was searched.
- `artefact_kinds_checked` lists all four kinds — session key, pairing token, fingerprint, pairing record — in both inspections.
- `artefacts_found` is **empty** in both. A residue is a **hard failure** of this gate, independent of the verdict.
- **Both** devices are inspected in each case.
- A pairing token from the previous session is not reusable without a new one-time token.

> **Note**: the residue inspection happens **after** the application restart scenario, so the restart scenario must be executed before the residue inspection.

---

## 7. Phase E — Gate 3: desktop receive pipeline (≈ 3 h)

**Procedure**: `docs/spike/gate-3-desktop-receive-pipeline.md`.

This gate has the most scenarios. Run them in this order; each writes to its own evidence directory.

### 7.1 Synthetic source

```bash
make spike-synthetic-source SOURCE_ID=src-01 DURATION_S=600
```

The source is 1280×720 at 30 frames per second, 2 500 kbps, with a per-frame QR marker carrying `{epoch_ms}|{frame_no}`. Confirm the marker survives the encode **before** measuring anything: decode one frame and check the payload.

### 7.2 Latency — three runs of at least 10 minutes

```bash
make spike-latency RUN=1
make spike-latency RUN=2
make spike-latency RUN=3
```

Expected outcome per run:

- `latency_samples.csv` holds one row per observation, with the raw samples retained (FR-011).
- `latency_summary.json` reports **three** figures together: the **raw** percentiles, the **measured harness floor**, and the **floor-corrected** percentiles.
- The gate is judged on the **raw p95**: `raw.p95_ms ≤ 150 ms`. The floor-corrected figure is published alongside and is **never** the judged value.
- `stage_budget.json` shows the actual versus the budgeted time for all six stages, and states which stages are included and which are excluded. Mobile capture and encode are **excluded** in the isolated pipeline measurement, and the report says so.
- The sum of the included stages is consistent with the reported **raw** capture-at-consumer figure.
- When the outcome is not a clear pass, the report states the **attribution**: whether the latency came from the capture procedure or from the product under test. A harness-limited failure and a product regression are distinguished, never merged.

### 7.3 Loss and jitter — the nominal impairment profile, at least 10 minutes

```bash
make spike-loss LOSS_PCT=3 ADDED_LATENCY_MS=30 JITTER_MS=10 SEED=42 DURATION_S=600
```

**One profile exists.** 3 % **uniform** loss, 30 ms added latency, ±10 ms jitter, applied to the **media stream only**. The impairment is applied by the seeded userspace relay, which impairs **only media** — ICE, DTLS and RTCP pass through untouched. The relay's JSONL log records every decision, and `impairment_profile.json` records the pinned profile with the run.

Expected outcome:

- `impairment_profile.json` records `profile_name: nominal-3pct-30ms-10ms`, `loss_distribution: uniform`, `applies_to: media_only` and the seed.
- The **injected** loss and the **residual** loss at the application after retransmission are both reported.
- The consumer capture is analysed for dropouts. `dropouts.json` shows **zero** dropouts of at least 50 ms, and the two detectors agree.
- The threshold used is the calibrated one, with `threshold_source` naming the calibration. A hard-coded threshold is invalid.
- `consumer_renegotiations` is **zero**.
- A control run at 0 % loss is also recorded, so the 3 % figure is correctly labelled a stress scenario.

**Deviation rule.** A run that deviates from the profile — **including burst loss** — is labelled `profile-deviation`, is judged on **detected dropouts** rather than on average loss percentage, and is **never** used to pass a gate.

### 7.4 Resources — 30 minutes

```bash
make spike-resources DURATION_S=1800
```

Expected outcome:

- `resource_summary.json`: `cpu_pct_machine.p95 ≤ 15` and `private_working_set_mb.max ≤ 400`.
- The `typeperf` cross-check agrees within one percentage point after normalisation, and the report states the logical core count.
- The p50, p95, p99 and maximum are all reported — never the mean alone.

### 7.5 Drift — at least 30 minutes

```bash
make spike-drift DURATION_S=1800 MARKER_PERIOD_S=2
```

Expected outcome:

- `drift_summary.json` reports the rate in ms/hour, the R², and the **95 % confidence interval on the slope**.
- The rate is **≤ 20 ms/hour** and the confidence interval is narrow enough to certify it. An interval that straddles the threshold does not pass — increase the marker density and re-run.
- `components` breaks the drift into the sender-side skew, the receiver resampler, and the present-versus-render clock difference.

**Note on the resampler**: the receiver audio renderer must be paced from the RTP timestamps. A free-running fixed-point resampler has a relative frequency error of about 1e-5, which is about **36 ms per hour** and would fail this criterion on its own. If the drift is high, measure the resampler separately before assuming the cause is elsewhere.

### 7.6 Stall — a 5-second media stall

```bash
make spike-stall STALL_S=5
```

Expected outcome:

- The session **continues** — it does not terminate.
- Buffered latency returns to the pre-stall baseline **within 2 seconds**.
- Buffer depth shows **no unbounded growth**; the buffer-depth trace is included in the evidence.
- The number of discarded frames is counted and reported.

---

## 8. Phase F — Gate 4: codec interoperability (≈ 45 min)

**Procedure**: `docs/spike/gate-4-codec-interoperability.md`.

```bash
make spike-interop
```

Expected outcome:

- The Android reference device and the desktop receiver agree on the **profile, level and format parameters**; the negotiated values are recorded.
- Decoded video is free of visible corruption for **10 continuous minutes**.
- If the preferred codec fails, the fallback codec is exercised and the CPU, latency and quality differences are **measured and reported** — not merely asserted.
- The record states whether the fallback codec is **hardware-accelerated on the reference environment**, so that the decode precondition and the latency criterion can be applied to the codec actually selected.

**A note on the level**: 1080p30 requires H.264 level 4.0 or above, while many receivers accept only level 3.1. Ship 720p30 as the default and treat 1080p30 as a negotiated upgrade. This is recorded as open question Q-7.

---

## 9. Phase G — Gate 5: virtual audio and licensing (≈ 2 h)

**Procedure**: `docs/spike/gate-5-virtual-audio-and-licensing.md`.

```bash
make spike-audio
```

### 9.1 Technical measurement

The prototype renders decoded audio into the pinned virtual audio device; a consumer application records from the paired capture endpoint.

Expected outcome:

- The capture is intelligible and continuous for **at least 10 minutes** under the nominal impairment profile (3 % uniform loss, 30 ms added latency, ±10 ms jitter).
- `dropouts.json` shows **zero** dropouts of at least 50 ms, with the two detectors agreeing. This is the **automated** half of the criterion.
- `intelligibility.json` shows **at least 18 of 19 non-anchor phrases** transcribed with zero errors, a corpus WER **≤ 0.05**, all nonces matching, and the anchors matching. This is the **scripted human** half of the criterion.
- The **observer is named** and the **phrase script reference** is recorded. An unnamed observer or an unrecorded script makes the result unverifiable.
- `consumer_renegotiations` is **zero**.
- The device used for the measurement is pinned by product and version, and `signature_verified` is `true` on a Secure-Boot-enabled machine.
- **No audio latency is measured.** End-to-end audio latency requires the real mobile capture path and is deferred to F2/F3. A latency figure produced in F0 would omit the mobile stages and would not be comparable with the F2/F3 figure.

### 9.2 Readiness and degraded operation

Verify that when the device is missing, unsigned, or reports an unexpected identity:

- The application reports an **actionable** state, naming what the user should do.
- **Video-only operation remains usable.** Confirm a session still starts and delivers video.

### 9.3 Licensing decision

This half of the gate is a decision, not a measurement. It cannot be closed by engineering alone.

The assessment MUST compare at least three options and document the cost, legal constraints, support burden and user experience of each:

| Option | Description |
| --- | --- |
| Redistribution licensing | Ship the driver inside the installer. |
| User-installed official driver | The user downloads and installs it themselves. |
| Plan B alternative | Video-only V1. |

And the licence terms of **every candidate device considered** MUST be documented — including candidates that were **not** used for the technical measurement. Each entry needs the official terms URL, the quoted text that supports the claim, the unit cost, the redistribution position and the fallback.

The assessment runs **in parallel** with the technical measurement, on the same test steps, so a change of the selected device does not invalidate the technical evidence.

Expected outcome:

- A decision record exists with the chosen option, the named decision owner, the approver, the date, the unit cost and the fallback.
- `licensing_bundle.json` records the **measurement device** separately from the **candidate options**, so the evidence stays anchored to the device that was actually measured.
- If the selected device is **not** the measured device, `remeasurement_required` is `true`: the newly selected device MUST be re-measured against the same recorded steps before it becomes the shipping option.
- **If the decision is not closed on the gate date, the gate is recorded as failed or conditional** with the blocker, the escalation owner and the interim audio scope stated. It MUST NOT pass silently.

**Legal note**: this is engineering research, not legal advice. The licensing decision requires sign-off from qualified counsel before any distribution decision is made.

---

## 10. Phase H — Assemble the report (≈ 90 min)

```bash
make gate-report
```

Produces `spikes/evidence/gate_report.json` (conforming to [`contracts/gate-report.schema.json`](./contracts/gate-report.schema.json)) and the human-readable `docs/spike/gate-report-f0.md`.

The report MUST include:

- **Latency**: the **raw** p50, p95, maximum, the **measured harness floor** and the **floor-corrected** value, with the gate judged on the raw p95 and the attribution stated.
- **CPU and memory**, with the sampler identified and the logical core count stated.
- **Loss and jitter behaviour**: the pinned impairment profile, injected loss, residual loss, dropout list, renegotiation count.
- **Drift**, with the regression and its confidence interval.
- **The per-application injection matrix**, for every evaluated backend.
- **Codec results**, with the negotiated profile and level.
- **Audio results**, with the dropout list, the intelligibility score, the **named observer** and the **phrase script reference**.
- **The pairing throttling evidence**: the per-origin attempt limit, the progressive blocking, the listener shutdown, and the fact that a legitimate pairing was unaffected.
- **The pairing residue evidence**: the inspection method and coverage, and the empty artefact lists for **both** the session end and the application restart.
- **The five gate verdicts**, each with its wave, its dependencies, its preconditions, its accountable role, its evidence, its measurement conditions, and its conditions and waivers where applicable.
- **The downstream status**: whether F1, F2 and F3 are unblocked, and any conditions attached to a conditional verdict.
- **The open-questions register**, with owner, impact and blocking status.
- **A statement that the spike artefacts are non-production** and must not be merged into product modules.

### 10.1 Validation checklist

- [ ] Every evidence file referenced by the report exists.
- [ ] Every measured value is compared against its criterion, and the comparison is stated.
- [ ] The included and excluded latency stages are stated.
- [ ] The **raw** p95, the **harness floor** and the **floor-corrected** value are all published, and the gate was judged on the raw value.
- [ ] The latency attribution is stated whenever the outcome is not a clear pass.
- [ ] The pinned impairment profile is recorded with the loss-and-jitter run, and no `profile-deviation` run is cited for a `go` verdict.
- [ ] The **named observer** and the **phrase script** are recorded for the intelligibility check.
- [ ] The measured virtual audio device is pinned by product and version, and `remeasurement_required` is set when the selected device differs from it.
- [ ] The throttling evidence shows the limit enforced at attempt 6, the listener closed at 10 consecutive failures, and a legitimate pairing unaffected.
- [ ] The residue evidence covers **both** the session end and the application restart, with the same inspection method and empty artefact lists.
- [ ] All five verdicts share the same review timestamp.
- [ ] A gate whose dependency failed still carries a verdict naming the blocking dependency.
- [ ] Every conditional verdict has a waiver with a deadline, or an explicit block.
- [ ] Every waiver is signed by the accountable role of that gate.
- [ ] No run labelled outside the reference environment is cited for a `go` verdict.
- [ ] No secrets, personal data, production credentials or real user media appear anywhere.
- [ ] The report is in English.
- [ ] `make test`, `make coverage` and `make lint` all pass, and the READMEs and the Makefile are consistent with what was actually run.

---

## 11. One-working-day schedule

| Block | Duration | Task |
| --- | --- | --- |
| A | 45 min | Preflight: environment, hardware decode, network baseline |
| B | 45 min | Toolchain self-test |
| C | 90 min | Gate 1 — camera injection matrix |
| D | 60 min | Gate 2a — pairing scenarios |
| D | 20 min | Gate 2b — throttling scenarios and listener shutdown |
| D | 15 min | Gate 2c — residue inspection after session end and after application restart |
| E | 90 min | Gate 3a — three latency runs |
| E | 60 min | Gate 3b — loss and jitter run with the dropout analysis |
| E | 40 min | Gate 3c — 30-minute resource run |
| E | 40 min | Gate 3d — 30-minute drift run |
| E | 30 min | Gate 3e — 5-second stall |
| F | 45 min | Gate 4 — codec interoperability |
| G | 60 min | Gate 5a — virtual audio technical measurement |
| G | 60 min | Gate 5b — licensing assessment |
| H | 90 min | Report assembly and validation |
| | **≈ 9.5 h** | |

Blocks A, B and E's resource and drift runs are scriptable and may be left unattended overnight. The licence assessment (G-b) is bounded by **counsel's availability**, not by engineering effort, so it starts first and runs in parallel.

---

## 12. Where to look when something fails

| Symptom | First place to look |
| --- | --- |
| Latency p95 too high | `stage_budget.json` — the stage breakdown shows where the time went. Then `harness_floor_ms` — the apparatus may be the cause. Check `latency_attribution` in the report. |
| CPU above 15 % | `resource_samples.csv` — is it a sustained load or a spike? Is hardware decode actually active? Check `environment.json`. |
| Dropouts reported | `dropouts.json` — do the two detectors agree? Is there a correlated RTP gap? A gap means transport; no gap means render or consumer. |
| Drift too high | `drift_summary.json` `components` — is it the resampler? Check whether the renderer is paced from the RTP timestamps. |
| Audio unintelligible | `intelligibility.json` — are the nonces matching? A mismatch means a stale capture, not a bad stream. Check that the observer is named and the script is recorded. |
| Camera not visible | `injection_matrix.json` — which backend, which application, which bitness? Compare against the other backend's cells. |
| Pairing rejected unexpectedly | `pairing_scenarios.json` — the `reason_code` and the trace. Check the clock offsets and the token lifetime first. |
| Pairing flood blocked the legitimate user | `pairing_throttling.json` — is the limit per origin? A global limit is the defect. Check `legitimate_pairing_unaffected`. |
| Residue found after a restart | `pairing_residue.json` — the `coverage` shows what was searched. A restart-only residue means the clean teardown path is not the only cleanup path. |
| Nothing measures correctly | `toolchain_selftest.json` — the toolchain is not trustworthy. Fix it before re-running any scenario. |

Failure diagnosis must be possible from the documented logs, metrics and tests alone, without ad hoc instrumentation (constitution Principle III).
