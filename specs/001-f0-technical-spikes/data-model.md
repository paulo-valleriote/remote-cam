# Data Model: F0 — Risk-Reduction Spikes and Technical Decisions

**Feature**: [001-f0-technical-spikes](./spec.md) | **Plan**: [plan.md](./plan.md) | **Research**: [research.md](./research.md)

This document defines the **domain entities** of the F0 phase and the **artefacts** the phase produces. It is deliberately independent of any implementation: no Rust type, Kotlin class, `windows` crate symbol or Media Foundation identifier appears here. The wire and file formats that carry these entities are specified separately in [`contracts/`](./contracts/).

Two vocabularies coexist and must not be confused:

- **Domain entities** (sections 1–3) — the things the phase reasons about.
- **Evidence artefacts** (sections 4–5) — the files on disk that carry the evidence. These are the raw material of the report and of `quickstart.md`.

---

## 1. Governance entities

### 1.1 Spike Gate

A decision checkpoint with a pass criterion, a measurement method, a verdict, an evidence reference, an accountable role, a target date, an execution wave, and the set of gates it depends on.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `gate_id` | identifier | yes | Stable, kebab-case, unique within the phase. Assigned once at plan time; never reused. |
| `title` | text | yes | Human-readable, English. |
| `wave` | integer ∈ {1, 2, 3} | yes | Execution wave. **Wave 1** = camera injection and pairing handshake; **wave 2** = desktop receive pipeline; **wave 3** = codec interoperability and virtual audio. |
| `depends_on` | list of `gate_id` | yes (may be empty) | Every referenced gate must exist. **No cycles.** Every dependency must be in a strictly lower wave. |
| `pass_criterion` | text | yes | Must be measurable — a number, a count, or an enumeration of acceptable outcomes. A criterion containing "adequate", "good" or "reasonable" is invalid. |
| `measurement_method` | text | yes | Must reference a named procedure in `docs/spike/` that a second engineer can execute. |
| `preconditions` | list of precondition identifiers | yes | The plan's preconditions PC-1…PC-6 that guard this gate. A gate with no precondition may still list none. |
| `accountable_role` | Gate Role | yes | Exactly one role. The role is named in the gate record, not just the role type. |
| `target_date` | date | yes | All five gates share **2026-11-06**. |
| `verdict` | Gate Verdict | yes | Exactly one, assigned at the phase review. |
| `evidence_refs` | list of artefact references | yes when verdict ≠ `not_run` | Every reference must resolve to a file that exists and is inside the evidence root. |

**State transitions** (a gate's *execution* state, distinct from its verdict):

```text
not_run ──► executed ──► reviewed
   │            │            │
   │            └──► blocked_by_dependency
   └──► not_applicable
```

- `not_run → executed`: the gate's procedure was run and evidence was produced.
- `executed → blocked_by_dependency`: a dependency received a `no-go`. The gate **still receives a recorded verdict** (FR-047) stating the blocking dependency; it is not simply skipped.
- `not_run → not_applicable`: only with a written, approved reason recorded on the gate.

**Invariants**

- **G-1**: A `conditional` verdict MUST name the phases it conditions and either a written waiver (with conditions and a resolution deadline) or an explicit block. A `conditional` verdict without one of the two is invalid.
- **G-2**: A gate whose dependency received `no_go` MUST still carry a verdict whose text states the blocking dependency.
- **G-3**: `wave` and `depends_on` MUST be consistent: every dependency has a strictly lower wave number.
- **G-4**: All five verdicts MUST be issued at a single phase review on or before `2026-11-06`.
- **G-5**: A gate MUST carry the preconditions that guard it. A precondition that failed makes the gate's verdict `no_go` with the failure code `precondition_failed` — it never makes the gate silently `not_run`.

### 1.2 Gate Role

The accountable role for a gate verdict — the technical lead, the security reviewer, or the product owner — recorded **by name** in the gate record.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `role_type` | enumeration | yes | `technical_lead` \| `security_reviewer` \| `product_owner`. |
| `person_name` | text | yes | The individual. A role title without a name is invalid — FR-035 requires the role to be *named*. |
| `email` | text | no | Contact address. Never used as a secret, never a credential. |

**Assignment rule** (fixed by FR-035 and not re-assignable by the implementer):

| Gate | `role_type` |
| --- | --- |
| Virtual camera injection | `technical_lead` |
| Desktop receive pipeline | `technical_lead` |
| Codec interoperability | `technical_lead` |
| Pairing handshake | `security_reviewer` |
| Virtual audio and licensing | `product_owner` |

### 1.3 Gate Verdict

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `verdict` | enumeration | yes | `go` \| `no_go` \| `conditional`. `not_run` is not a verdict — it is an execution state. |
| `issued_at` | timestamp | yes | ISO 8601 with offset. All five verdicts MUST share the same phase-review timestamp (within one minute). |
| `evidence_refs` | list | yes | Non-empty for `go` and `conditional`. For `no_go` it MAY be empty, but a `no_go` without evidence is a process failure. |
| `measurement_conditions` | text | yes | The reference environment, network conditions, and the included/excluded latency stages, as required by FR-010 and FR-037. |
| `rationale` | text | yes | Must reference the criterion that was met or missed. |
| `conditions` | list of Condition | only when `conditional` | Non-empty. |
| `blocking_dependency` | `gate_id` | only when the dependency failed | Required for a verdict recorded because a dependency failed. |

### 1.4 Condition and Waiver

A `conditional` verdict blocks the phases it conditions. It is lifted only by a written waiver from the accountable role of that gate.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `condition_id` | identifier | yes | Unique within the gate. |
| `description` | text | yes | The unmet condition, stated so that a reader can tell whether it has since been met. |
| `conditioned_phases` | list of phase identifiers | yes | Which downstream phases are blocked — e.g. `F1`, `F2`, `F3`. |
| `waiver` | Waiver | optional | Absent means the block stands. |
| `status` | enumeration | yes | `blocking` \| `waived` \| `resolved`. |

**Waiver**

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `waived_by` | Gate Role reference | yes | MUST be the accountable role of the gate that issued the condition. |
| `waived_at` | timestamp | yes | ISO 8601 with offset. |
| `waiver_conditions` | text | yes | The limits of the waiver. |
| `resolution_deadline` | date | yes | A concrete date. A waiver without a deadline is invalid. |

### 1.5 Decision Record

A durable decision with options considered, evidence, rationale, approver, date, and downstream impact.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `decision_id` | identifier | yes | Unique, stable, referenced from the gate report. |
| `topic` | text | yes | English. |
| `status` | enumeration | yes | `draft` \| `accepted` \| `superseded`. An `accepted` record MUST NOT be edited in place — a change creates a new record that supersedes it. |
| `options_considered` | list of Option | yes | Non-empty. Each option names what it is, not only what it is not. |
| `chosen_option` | Option | yes when `accepted` | MUST be a member of `options_considered`. |
| `evidence_refs` | list | yes | Non-empty for `accepted`. |
| `rationale` | text | yes | Must address the trade-off, not only the benefit. |
| `approver` | Gate Role reference | yes when `accepted` | Named individual. |
| `decided_at` | date | yes when `accepted`. |
| `downstream_impact` | text | yes | Which phases are affected and how. |
| `reusable_findings` | list of text | no | Findings that carry forward to the protocol, mobile and media phases (FR-039). |

### 1.6 Open Question

A question encountered during the spike, recorded with owner, impact and blocking status. Open questions from the source technical design remain tracked and are **not** silently resolved by the spike (see the spec's Assumptions).

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `question_id` | identifier | yes | Unique. |
| `question` | text | yes | English. |
| `context` | text | yes | Where it came from. |
| `owner` | text | yes | A named individual or role. |
| `impact` | text | yes | What it blocks or affects. |
| `blocking` | boolean | yes | `true` means it blocks a phase from starting. |
| `status` | enumeration | yes | `open` \| `answered` \| `deferred`. An `answered` question MUST reference a Decision Record or be promoted into one. |

---

## 2. Measurement entities

### 2.1 Reference Environment

The documented hardware, operating system, network and devices against which gates are judged, including the recorded hardware-decode capability verification result for the codec under test.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `environment_id` | identifier | yes | One per environment record. |
| `captured_at` | timestamp | yes | ISO 8601 with offset. |
| `cpu_model` | text | yes | Exact model string, e.g. `"AMD Ryzen 7 5800X"`. "Modern CPU" is invalid. |
| `cpu_logical_cores` | integer ≥ 1 | yes | Required to normalise the CPU metric (see §4.3). |
| `gpu_model` | text | yes | Exact model string. |
| `os_name` | text | yes | `"Windows 11"`. |
| `os_build` | text | yes | Exact build number, e.g. `"22631.4169"`. |
| `android_device_model` | text | yes | Exact model. |
| `android_os_version` | text | yes | Exact version and API level. |
| `network` | Network Condition | yes | See §2.2. |
| `hardware_decode_verification` | Hardware Decode Verification | yes | See §2.4. **Mandatory.** |
| `app_versions` | map of name → version | yes | Every application in the target matrix, with its exact version. |
| `tool_versions` | map of name → version | yes | Every measurement tool, pinned. |
| `internet_enabled` | boolean | yes | MUST be `false` for a reference environment. |

### 2.2 Network Condition

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `band` | enumeration | yes | `2_4_ghz` \| `5_ghz` \| `wired` \| `emulated`. |
| `channel_width_mhz` | integer | when wireless | 20/40/80/160. |
| `access_point_model` | text | when wireless | Exact model and firmware. |
| `distance_m` | number | when wireless | ≤ 5 m for a reference environment. |
| `client_adapter` | text | when wireless | Exact adapter or phone Wi-Fi chipset. |
| `wifi_power_save` | enumeration | when wireless | `disabled` \| `enabled`. **A reference environment requires `disabled`** — DTIM power save adds 100+ ms of latency. |
| `baseline_throughput_mbps` | number | yes | From `iperf3` immediately before the run. |
| `baseline_rtt_ms` | number | yes | From `ping`. |
| `baseline_loss_pct` | number | yes | From `ping`. |
| `within_reference_conditions` | boolean | yes | `false` ⇒ the run MUST be labelled as outside the reference environment and MUST NOT be used to pass a gate. |

### 2.3 Impairment Profile

The **single** named network condition applied to a loss-and-jitter measurement run. It is applied to the **media stream only**, so that connection establishment and control traffic remain unaffected. It is the sole condition under which a loss-and-jitter gate scenario may pass, and it MUST be recorded with the run evidence.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `profile_name` | text | yes | The profile's name as used in the procedure. One profile exists: `nominal-3pct-30ms-10ms`. |
| `loss_pct` | number | yes | **3.0**. |
| `loss_distribution` | enumeration | yes | `uniform`. A burst distribution is **not** a member of this profile. |
| `added_latency_ms` | number | yes | **30**. |
| `jitter_ms` | number | yes | **10** — the jitter **amplitude**; the actual deviation is within ±10 ms. |
| `applies_to` | enumeration | yes | `media_only`. Control traffic MUST NOT be impaired. |
| `seed` | integer | yes | The relay's PRNG seed. It is what makes the realised loss pattern reproducible. |
| `relay_log_ref` | artefact reference | yes | The relay's JSONL log. The realised loss is computed from it, not from the configuration. |

**Validation rules**

- **P-1**: A run MUST record the profile it used. A run with no profile is not admissible for a loss-and-jitter gate.
- **P-2**: A run that deviates from the profile — **including a burst loss pattern** — MUST carry the label `profile-deviation`, MUST be judged on **detected dropouts** rather than on average loss percentage, and MUST NOT be used to pass a gate.
- **P-3**: The realised loss computed from `relay_log_ref` and the residual loss observed at the application MUST both be reported. They differ whenever NACK retransmission recovers a packet.
- **P-4**: A run whose `applies_to` is not `media_only` is invalid — impairing ICE or DTLS breaks the connection and presents as a product defect rather than as media loss.

### 2.4 Hardware Decode Verification

The recorded check that the reference machine provides hardware-accelerated decoding for the codec under test. This is a **precondition** (FR-044): latency or resource results measured on a machine without it are not admissible.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `codec` | text | yes | `"H.264"`. |
| `verified_at` | timestamp | yes | ISO 8601 with offset. |
| `discovery_method` | enumeration | yes | `mft_enum_syncmft` \| `mft_enum_hardware` \| `gpu_engine_counter` \| `other`. |
| `decoder_found` | boolean | yes | |
| `hardware_accelerated` | boolean | yes | MUST be `true` for the result to be admissible. |
| `evidence` | text | yes | The exact command output or counter that proves it. |
| `output_format_supported` | enumeration | yes | `nv12` \| `i420` \| `other`. `nv12` is the only 8-bit format available under hardware decode. |

**Validation**: if `hardware_accelerated` is `false`, every measurement run bound to this environment MUST be flagged `admissible_for_latency_gate = false`.

### 2.5 Measurement Run

One execution of a gate scenario, including environment, duration, conditions, raw samples and summary metrics.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `run_id` | identifier | yes | Unique. Embeds the gate id and a sequence. |
| `gate_id` | `gate_id` | yes | Must exist. |
| `environment_id` | `environment_id` | yes | Must exist. |
| `started_at` / `ended_at` | timestamp | yes | `ended_at > started_at`. |
| `duration_s` | number > 0 | yes | Derived; MUST satisfy the gate's minimum duration (10 min for latency and loss runs, 30 min for the resource and drift runs). |
| `scenario` | text | yes | A named scenario from the gate's procedure. |
| `conditions` | map | yes | Loss percentage, jitter, delay, stall duration, seed, bitrate, resolution, frame rate. |
| `impairment_profile` | Impairment Profile | when the run is loss-and-jitter | See §2.3. **Mandatory** for any run that can pass a loss-and-jitter gate. |
| `seed` | integer | when randomised | Required whenever the run involves randomised impairment; it is what makes the run reproducible. |
| `sample_refs` | list | yes | References to the raw sample files. Every referenced file MUST exist. |
| `summary_metrics` | map | yes | The computed statistics; each MUST name the criterion it is compared against. |
| `admissible_for_latency_gate` | boolean | yes | `false` if the hardware-decode precondition failed, or if the run was outside the reference environment. |
| `labels` | list of text | no | e.g. `stress-scenario`, `outside-reference-environment`, `profile-deviation`. |

### 2.6 Latency Sample

One observation of capture-at-consumer latency.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `sample_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `source_frame_no` | integer ≥ 0 | yes | The frame counter burned into the source frame. Primary key of the observation. |
| `source_epoch_ms` | integer | yes | The wall-clock instant encoded in the frame, in milliseconds since the Unix epoch, UTC. |
| `capture_host_ms` | number | yes | The host timestamp at which the consumer's capture device received the frame. Same clock domain as `source_epoch_ms`, or the clock offset is recorded with the run. |
| `latency_ms` | number ≥ 0 | yes | `capture_host_ms − source_epoch_ms − clock_offset_ms`. This is the **raw** capture-at-consumer latency. |
| `clock_offset_ms` | number | yes | 0 when the source runs on the PC; otherwise the value from the sync-frame handshake at that time. |
| `consumer_app` | `target_application_id` | yes | Which consumer produced the capture. |

**Derived statistics** (computed, not stored per sample): p50, p95, p99, maximum, and count. The **raw p95** is the gated statistic.

### 2.7 Latency Summary

The three figures that every latency artefact MUST publish together, for one run.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `run_id` | `run_id` | yes | |
| `sample_count` | integer ≥ 1 | yes | |
| `harness_floor_ms` | number ≥ 0 | yes | The measurement apparatus's own floor, measured by the calibration procedure (the local-HTML-page method). |
| `raw` | percentile set | yes | p50, p95, p99, maximum of `latency_ms`. |
| `floor_corrected` | percentile set | yes | The same percentiles with the harness floor removed. **Reported, never gated.** |
| `criterion` | object | yes | `metric`, `threshold` (150), `comparison` (`<=`), `basis` (always `raw`). |
| `included_stages` / `excluded_stages` | lists | yes | The latency stages inside and outside the measured figure. |
| `verdict` | `pass` \| `fail` | yes | Judged on `raw.p95_ms` against the threshold. |

**Validation rules**

- **L-1**: The gated value MUST be `raw.p95_ms`. A verdict judged on `floor_corrected` is invalid.
- **L-2**: `harness_floor_ms`, `raw` and `floor_corrected` MUST all be present. A summary with only one of the two figures is incomplete evidence.
- **L-3**: `floor_corrected.p95_ms` MUST equal `raw.p95_ms − harness_floor_ms` within rounding.
- **L-4**: When the failure is attributable to the capture procedure rather than to the product under test, the attribution MUST be stated. A harness-limited failure and a product regression are distinguished, never merged.

### 2.8 Stage Budget Measurement

The actual versus budgeted time for each stage of the latency budget (capture, encode, network and jitter buffer, decode, frame handoff, consumer presentation).

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `stage` | enumeration | yes | `capture` \| `encode` \| `network_and_jitter_buffer` \| `decode` \| `frame_handoff` \| `consumer_presentation`. |
| `budget_ms` | number | yes | From the source design: 33, 15, 20, 12, 3, 17 — total ≈ 100 ms. |
| `measured_ms` | number | yes | MUST be ≥ 0. |
| `included_in_gate` | boolean | yes | Whether this stage is inside the measured capture-at-consumer figure. The included/excluded set MUST be stated explicitly (FR-010, FR-011). |
| `measurement_method` | text | yes | How the stage time was obtained. |

**Sum rule**: the sum of `measured_ms` over included stages MUST be ≤ the reported raw capture-at-consumer figure for the same run, or the report MUST explain the difference.

### 2.9 Resource Sample

One observation of the desktop process's CPU and memory usage.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `sample_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `elapsed_ms` | number | yes | Milliseconds since the start of the measurement window. Strictly increasing. |
| `cpu_pct_machine` | number ∈ [0, 100] | yes | **Normalised to the whole machine**: `100 × Δprocess_time / Δsystem_time`. This is the gated statistic. |
| `cpu_pct_single_core` | number | yes | `100 × Δprocess_time / Δwall_clock`; may exceed 100. Recorded for cross-checking against PDH counters. |
| `private_working_set_bytes` | integer | yes | The gated memory metric (what Task Manager shows as "Memory"). |
| `total_working_set_bytes` | integer | yes | Includes shared pages. |
| `peak_working_set_bytes` | integer | yes | |
| `thread_count` / `handle_count` | integer | yes | Leak indicators. |

**Validation**: `cpu_pct_machine` MUST equal `cpu_pct_single_core / cpu_logical_cores` within rounding. A mismatch is an instrumentation error and invalidates the run.

### 2.10 Dropout Interval

A contiguous silent interval in the recorded consumer capture.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `dropout_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `start_s` | number ≥ 0 | yes | Seconds into the capture. |
| `end_s` | number | yes | `end_s ≥ start_s`. |
| `duration_ms` | number | yes | **A dropout is any contiguous detected silent interval of at least 50 ms.** Intervals below the threshold are recorded as `below_threshold` and MUST NOT count as dropouts. |
| `threshold_dbfs` | number | yes | The calibration-derived detection threshold used by the primary detector. |
| `detector` | enumeration | yes | `silencedetect` \| `rms_window`. The two detectors MUST agree; a disagreement is a finding in its own right. |
| `correlated_rtp_gap` | boolean | yes | Whether an RTP sequence-number gap coincides with the interval. Distinguishes a transport loss from a render or consumer-side defect. |

### 2.11 Intelligibility Result

The outcome of the scripted spoken-phrase check. Together with the Dropout Intervals, this is the **"continuous and intelligible audio"** criterion (Q3): the automated half and the human half.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `result_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `phrase_script_ref` | artefact reference | yes | The committed phrase manifest that was played. The script MUST be recorded (FR-029). |
| `phrase_count` | integer | yes | 20 in the standard manifest (1 opening anchor, 8 static, 10 nonce-bearing, 1 closing anchor). |
| `phrases_exact` | integer | yes | Number of non-anchor phrases transcribed with zero errors. The gated statistic: **≥ 18 of 20**. |
| `corpus_wer` | number ∈ [0, ∞) | yes | Gated: ≤ 0.05. |
| `nonces_matched` | integer | yes | MUST equal the number of nonce-bearing phrases. A mismatch invalidates the capture as stale or spliced. |
| `anchors_match` | boolean | yes | The opening and closing session markers MUST agree. |
| `observer` | text | yes | The named observer. A role title without a name is invalid (FR-029). |
| `observed_at` | timestamp | yes | When the check was performed. |
| `asr_wer` | number | no | Supplementary trend evidence only. Never gated. |
| `estoi_median` | number | no | Supplementary trend evidence only. Never gated. |

### 2.12 Drift Sample

One observation of the audio/video time-base offset.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `sample_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `elapsed_s` | number | yes | Seconds since the start of the drift run. |
| `source_skew_ms` | number | yes | Source-time difference between the matched audio and video items, computed from the in-band source timestamp. |
| `audio_playout_delay_ms` | number | yes | Receiver-side delay from packet arrival to playout. |
| `video_playout_delay_ms` | number | yes | |
| `playout_skew_ms` | number | yes | `source_skew_ms + audio_playout_delay_ms − video_playout_delay_ms`. |

**Derived**: the drift rate in ms/hour, obtained by linear regression of `playout_skew_ms` against `elapsed_s`. The report MUST state the regression's R² and the 95 % confidence interval on the slope. **Gated: ≤ 20 ms/hour**, with a confidence interval narrow enough to certify it — a regression whose interval spans the criterion does not pass.

### 2.13 Audio Run Capture

The recorded consumer-application capture for one measurement run, together with the dropout analysis output and the scripted intelligibility check result.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `capture_id` | identifier | yes | |
| `run_id` | `run_id` | yes | |
| `capture_file_ref` | artefact reference | yes | The recorded audio. |
| `sample_rate_hz` | integer | yes | 48000. |
| `channels` | integer | yes | 1 (mono) for the standard configuration. |
| `duration_s` | number | yes | ≥ 600 s for a gated audio run. |
| `consumer_renegotiations` | integer | yes | MUST be 0. Counts stream renegotiations observed by the consumer application. |
| `dropout_analysis_ref` | artefact reference | yes | The dropout list. |
| `intelligibility_ref` | artefact reference | yes | The intelligibility result. |
| `analyser_version` | text | yes | The analysis tool version, for reproducibility. |

### 2.14 Synthetic Media Source

The timestamped synthetic media fed through the receive and injection path in isolation.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `source_id` | identifier | yes | |
| `width` / `height` | integer | yes | 1280 × 720 by default. |
| `frame_rate` | integer | yes | 30. |
| `bitrate_kbps` | integer | yes | 2500 by default. |
| `duration_s` | number | yes | ≥ 600 s. |
| `marker_encoding` | text | yes | The per-frame marker payload format, e.g. `"{epoch_ms}|{frame_no}"`. |
| `marker_region` | text | yes | Where in the frame the marker is drawn. A marker in a corner MUST be duplicated if the consumer preview may crop it. |
| `generation_command` | text | yes | The exact, reproducible command. |
| `includes_mobile_capture` | boolean | yes | MUST be `false` for the isolated pipeline measurement, and the report MUST state it. |
| `includes_mobile_encode` | boolean | yes | Same. |

---

## 3. Compatibility, licensing and artefact entities

### 3.1 Target Application

A consumer application category with product, version, bitness and observed injection outcome.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `application_id` | identifier | yes | Unique. |
| `category` | enumeration | yes | `chromium_browser` \| `video_conferencing_client` \| `obs` \| `windows_camera_app` \| `screen_recorder`. These five are the minimum matrix. |
| `product_name` | text | yes | Exact product. |
| `version` | text | yes | Exact version. |
| `bitness` | enumeration | yes | `32_bit` \| `64_bit`. Both MUST be represented in the matrix. |
| `capture_method` | text | yes | How the outcome was observed, e.g. `getusermedia_mediarecorder`, `obs_websocket`, `gdigrab_window_capture`. |
| `injection_outcome` | enumeration | yes | `frames_delivered` \| `device_not_visible` \| `frames_unstable` \| `not_tested`. |
| `observed_duration_s` | number | when `frames_delivered` | ≥ 1800 s (30 continuous minutes). |
| `frame_size_ok` / `frame_rate_ok` | boolean | when `frames_delivered` | The delivered size and rate match what the application selected. |
| `longest_stall_ms` | number | when `frames_delivered` | MUST be ≤ 2000. |
| `duplicate_frames` | integer | when `frames_delivered` | Counted, not zero-bounded. |
| `evidence_ref` | artefact reference | yes | |

### 3.2 Injection Matrix Cell

The result of one target application against one injection backend.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `cell_id` | identifier | yes | |
| `backend` | enumeration | yes | `mf_virtual_camera` \| `dshow_filter`. |
| `application_id` | `application_id` | yes | |
| `outcome` | enumeration | yes | `frames_delivered` \| `device_not_visible` \| `frames_unstable` \| `not_tested`. |
| `notes` | text | no | |

**Matrix rule**: the matrix MUST cover every `target_application` against every **evaluated** backend. A `not_tested` cell MUST carry a reason. The backend decision MUST be derivable from the matrix.

### 3.3 Licensing Option

A virtual audio device sourcing option with licence terms, cost, redistribution rights, support burden and user experience.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `option_id` | identifier | yes | |
| `device_name` | text | yes | Exact product. |
| `vendor` | text | yes | |
| `license_model` | enumeration | yes | `donationware` \| `proprietary_paid` \| `permissive_open_source` \| `own_driver` \| `plan_b_video_only`. |
| `terms_url` | URL | yes | The **official** terms page, not a blog post. |
| `terms_captured_text` | text | yes | The quoted text that supports the claim. A claim without captured text is unverifiable. |
| `redistribution_allowed` | enumeration | yes | `yes_with_conditions` \| `no` \| `unverified`. `unverified` MUST be recorded explicitly. |
| `commercial_use_allowed` | boolean | yes | |
| `unit_cost` | number | yes | Per unit, at the recorded volume. |
| `cost_model` | text | yes | How the cost scales. |
| `support_burden` | enumeration | yes | `low` \| `medium` \| `high`, with a justification. |
| `user_experience` | text | yes | Install friction, admin requirement, reboot, default-device side effects. |
| `signature_verified` | boolean | yes | Whether the driver passed the Windows 11 Secure-Boot signature check. |
| `fallback` | text | yes | What happens if this option is not obtained. |

**Licensing rule**: FR-033 requires the licence terms of **every candidate device considered**, including candidates that were **not** used for the technical measurement. The licensing assessment MUST compare at least: redistribution licensing, user-installed official driver, and a plan B alternative.

### 3.4 Prototype Artefact

Non-production code, harness or configuration used to produce evidence, with a clear non-production marker.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `artifact_id` | identifier | yes | |
| `path` | path | yes | MUST be inside the spike tree. |
| `kind` | enumeration | yes | `code` \| `script` \| `configuration` \| `fixture` \| `capture` \| `report`. |
| `non_production_marker` | text | yes | The literal marker text present in the artefact. |
| `produces_evidence` | list | yes | Which evidence files this artefact generates. |
| `reusable_findings` | list of text | no | Findings that carry forward. |

**Rule**: no prototype artefact MUST be merged into a product module (FR-039). The spike tree is separate from `desktop/` and `mobile/` by construction.

### 3.5 Pairing Scenario Result

The outcome of one handshake scenario, valid or adversarial.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `scenario_id` | identifier | yes | |
| `category` | enumeration | yes | `valid` \| `adversarial`. |
| `preconditions` | text | yes | What was set up. |
| `expected_result` | text | yes | |
| `observed_result` | text | yes | |
| `outcome` | enumeration | yes | `accepted` \| `rejected` \| `aborted` \| `inconclusive`. |
| `reason_code` | Reason Code | when rejected or aborted | MUST be a member of the closed set. |
| `media_flowed_before_authentication` | boolean | yes | MUST be `false` in every scenario. |
| `residual_credentials` | list | yes | MUST be empty. A non-empty list is a hard failure. |
| `trace_ref` | artefact reference | yes | The log or packet trace that evidences the scenario (FR-006). |
| `established_channel_ms` | number | when accepted | Time from pairing start to an established authenticated channel. |
| `pinning_added_delay_ms` | number | when accepted | Whether fingerprint pinning added perceptible delay (FR-007). |
| `throttle_evidence_ref` | artefact reference | when the scenario is a throttling scenario | The Pairing Throttling Evidence for the scenario (Q9). |
| `residue_inspection_ref` | artefact reference | when the scenario is a residue scenario | The Pairing Residue Inspection for the scenario (Q10). |

### 3.6 Reason Code

The closed set of rejection and close codes. A rejection MUST carry exactly one code from this set; free-text reasons are forbidden.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `code` | identifier | yes | Stable, from the closed set in [`contracts/reason-codes.md`](./contracts/reason-codes.md). |
| `category` | enumeration | yes | `pairing_rejection` \| `session_close` \| `error`. |
| `description` | text | yes | English, generic. MUST NOT contain host data, paths, user names or public IP addresses. |
| `user_actionable` | boolean | yes | Whether the message tells the user what to do. |

### 3.7 Pairing Throttling Evidence

The record of the per-origin attempt limit, the progressive blocking and the listener shutdown (Q9, FR-049). It proves the property; a configuration value does not.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `throttle_id` | identifier | yes | Unique. |
| `origin_id` | text | yes | The origin the attempts came from. The limit is **per origin**. |
| `attempts_observed` | integer ≥ 0 | yes | The number of pairing attempts made by this origin within the observation window. |
| `attempt_limit_per_minute` | integer | yes | **5**. The source threshold. |
| `limit_exceeded_at_attempt` | integer | yes | The attempt number at which the limit was enforced. MUST be 6 for a correct implementation. |
| `reason_code` | Reason Code | yes | `rate_limited`. |
| `blocked` | boolean | yes | Whether the origin was progressively blocked after exceeding the limit. |
| `block_duration_s` | number ≥ 0 | when `blocked` | The progressive block duration. A longer block MUST follow each further failure. |
| `consecutive_failures_at_listener_close` | integer | when the listener closed | **10**. The source threshold. |
| `listener_closed` | boolean | yes | Whether the pairing listener was closed after the consecutive-failure threshold. |
| `legitimate_pairing_unaffected` | boolean | when tested concurrently | Whether a legitimate pairing from a **second** origin completed while the first was being throttled. A flood MUST NOT deny service to the legitimate user. |
| `session_established` | boolean | yes | MUST be `false` for a throttled origin. |
| `decisions` | list of Throttling Decision | yes | One entry per throttling decision. |
| `trace_ref` | artefact reference | yes | The log or packet trace (FR-006). |
| `security_event` | boolean | yes | Whether the listener closure was recorded as a security event rather than a product failure. |

**Throttling Decision**

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `sequence` | integer ≥ 1 | yes | The decision's order within the scenario. |
| `at_ms` | number | yes | Milliseconds since the start of the scenario. |
| `origin_id` | text | yes | The origin the decision applied to. |
| `action` | enumeration | yes | `allowed` \| `rate_limited` \| `blocked` \| `listener_closed`. |
| `reason_code` | Reason Code | yes | From the closed set. |
| `outcome` | text | yes | What actually happened, observed. |

**Validation rules**

- **T-1**: Every throttling decision MUST carry a reason code from the closed set and an observed outcome (FR-049).
- **T-2**: `limit_exceeded_at_attempt` MUST be 6 — the sixth attempt in a minute is refused.
- **T-3**: `consecutive_failures_at_listener_close` MUST be 10.
- **T-4**: The limit is scoped **per origin**. A global limit is a defect.
- **T-5**: A scenario in which a legitimate second origin is denied service because of another origin's flood is a **failure**, not an expected outcome.

### 3.8 Pairing Residue Inspection

The record of the artefact inspection that proves no pairing state survived (Q10, FR-005). The **same** inspection method is used after a normal session end and after an application restart, so the two results are comparable.

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `inspection_id` | identifier | yes | Unique. |
| `method` | text | yes | The inspection method — the commands, the artefact stores searched and the procedure. The **same** method MUST be used in both cases. |
| `coverage` | list of text | yes | What the inspection covered: every location searched for a session key, pairing token, fingerprint or pairing record. MUST be stated (FR-005). |
| `case` | enumeration | yes | `after_session_end` \| `after_application_restart`. Both MUST be present. |
| `inspected_at` | timestamp | yes | ISO 8601 with offset. |
| `devices_inspected` | list | yes | The devices inspected. Both the desktop and the mobile MUST be inspected. |
| `artefacts_found` | list | yes | **MUST be empty.** A non-empty list is a hard failure of the pairing gate. |
| `artefact_kinds_checked` | list | yes | `session_key` \| `pairing_token` \| `fingerprint` \| `pairing_record`. All four MUST be listed. |
| `evidence_ref` | artefact reference | yes | The before/after inspection output. |

**Validation rules**

- **R-1**: Both `after_session_end` and `after_application_restart` inspections MUST exist, and both MUST name the **same** `method`.
- **R-2**: `artefacts_found` MUST be empty in both inspections. A residue is a hard failure, independent of the verdict.
- **R-3**: `coverage` MUST be non-empty and MUST name what was searched. An inspection with no stated coverage is not evidence.
- **R-4**: All four `artefact_kinds_checked` MUST be present in both inspections.

### 3.9 Licensing Decision Bundle

The set of licence records that close the virtual-audio commercial question (Q5, FR-033, FR-034).

| Field | Type | Required | Validation |
| --- | --- | --- | --- |
| `bundle_id` | identifier | yes | Unique. |
| `measurement_device` | Licensing Option | yes | The option whose product and version were **pinned for the technical measurement**. |
| `candidate_options` | list of Licensing Option | yes | Every candidate device considered, **including the ones that were not used for the technical measurement**. |
| `options_compared` | list | yes | MUST cover at least: redistribution licensing, user-installed official driver, and a plan B alternative. |
| `decision_owner` | Gate Role | yes | Named individual. |
| `approver` | Gate Role | yes | Named individual. |
| `decided_at` | date | yes | |
| `fallback` | text | yes | The approved plan B if the licence is not granted. |
| `remeasurement_required` | boolean | yes | `true` when the selected device is **not** the measured device. The newly selected device MUST be re-measured against the same recorded steps before it becomes the shipping option. |

---

## 4. Artefact and file structure

The evidence files referenced by the entities above. Formats are specified in [`contracts/measurement-formats.md`](./contracts/measurement-formats.md).

| Artefact | Format | Contents |
| --- | --- | --- |
| `environment.json` | JSON | One Reference Environment. |
| `latency_samples.csv` | CSV | Latency Samples, one row per observation. |
| `latency_summary.json` | JSON | The Latency Summary: raw percentiles, harness floor and floor-corrected percentiles. |
| `stage_budget.json` | JSON | Stage Budget Measurements, actual versus budgeted. |
| `resource_samples.csv` | CSV | Resource Samples, one row per sample. |
| `resource_summary.json` | JSON | Derived CPU and memory statistics. |
| `dropouts.json` | JSON | Dropout Intervals from both detectors, plus the RTP-gap correlation. |
| `intelligibility.json` | JSON | The Intelligibility Result and the per-phrase detail. |
| `drift_samples.csv` | CSV | Drift Samples. |
| `drift_summary.json` | JSON | The regression, the rate in ms/hour, R² and the confidence interval. |
| `relay_log.jsonl` | JSON Lines | One line per impairment decision: the realised loss and the seed. |
| `impairment_profile.json` | JSON | The pinned Impairment Profile used by the loss-and-jitter run. |
| `injection_matrix.json` | JSON | The Injection Matrix Cells. |
| `pairing_scenarios.json` | JSON | The Pairing Scenario Results. |
| `pairing_throttling.json` | JSON | The Pairing Throttling Evidence: per-origin limit, progressive blocking, listener shutdown. |
| `pairing_residue.json` | JSON | The Pairing Residue Inspections, after a session end and after an application restart. |
| `licensing_bundle.json` | JSON | The Licensing Decision Bundle: every candidate option, the decision, and the fallback. |
| `audio_run_capture.json` | JSON | The Audio Run Capture index (the media itself is a separate file). |
| `decisions/*.md` | Markdown | Decision Records, one file per decision. |
| `gate_report.json` | JSON | The five Gate Verdicts with their conditions and waivers. |
| `gate_report.md` | Markdown | The human-readable report (FR-036, FR-037). |
| `open_questions.json` | JSON | The Open Question register. |

### 4.1 Redaction rules (apply to every artefact)

1. **No secrets.** Tokens, private keys, session keys and full identity fingerprints MUST NOT appear. A fingerprint MAY appear truncated to its first 8 hex characters and only where the full value is not needed to reproduce the result.
2. **No personal data.** No user names, home paths, e-mail addresses, phone numbers or device identifiers belonging to the test operator.
3. **No real user media.** All captured media is synthetic or from a scripted voice.
4. **No absolute user paths.** Artefact paths are repository-relative.
5. **No network addresses** except RFC 1918 / link-local addresses already covered by the "private address" statement in the protocol contract.

---

## 5. Relationships

```text
ReferenceEnvironment 1 ──── * MeasurementRun
ReferenceEnvironment 1 ──── 1 HardwareDecodeVerification

MeasurementRun * ──── 1 SpikeGate
MeasurementRun 1 ──── * LatencySample
MeasurementRun 1 ──── * LatencySummary
MeasurementRun 1 ──── * ResourceSample
MeasurementRun 1 ──── * DriftSample
MeasurementRun 1 ──── * AudioRunCapture
MeasurementRun 1 ──── * StageBudgetMeasurement
MeasurementRun 0..1 ──── 1 ImpairmentProfile

AudioRunCapture 1 ──── * DropoutInterval
AudioRunCapture 1 ──── 1 IntelligibilityResult

SpikeGate * ──── * SpikeGate        (depends_on, acyclic, strictly decreasing wave)
SpikeGate 1 ──── 1 GateRole
SpikeGate 1 ──── 1 GateVerdict
GateVerdict 1 ──── * Condition
Condition 0..1 ──── 1 Waiver

DecisionRecord * ──── * Option
DecisionRecord 1 ──── * EvidenceArtefact

TargetApplication * ──── * InjectionBackend   (via InjectionMatrixCell)
LicensingOption 1 ──── * DecisionRecord
PairingScenarioResult * ──── 1 ReasonCode
PairingThrottlingEvidence 1 ──── * ThrottlingDecision
PairingResidueInspection 1 ──── * ReasonCode     (via artefact_kinds_checked)
LicensingDecisionBundle 1 ──── * LicensingOption
OpenQuestion 1 ──── 0..1 DecisionRecord
```

### 5.1 Cross-entity validation rules

| # | Rule |
| --- | --- |
| R-1 | Every `MeasurementRun.environment_id` MUST resolve to a Reference Environment whose `hardware_decode_verification.hardware_accelerated` is `true`, for the run to be `admissible_for_latency_gate = true`. |
| R-2 | Every `LatencySample` MUST reference a `consumer_app` that exists in the Target Application set. |
| R-3 | The `DropoutInterval` count from the `silencedetect` detector and from the `rms_window` detector MUST be equal for a run to be considered clean; a disagreement is recorded as a finding. |
| R-4 | Every `InjectionMatrixCell` MUST reference an existing `TargetApplication` and an evaluated backend. |
| R-5 | Every `DecisionRecord` with status `accepted` MUST have a non-empty `chosen_option` present in `options_considered` and a named approver. |
| R-6 | Every `Condition` with status `waived` MUST have a `Waiver` whose `waived_by` is the accountable role of the owning gate. |
| R-7 | Every `PairingScenarioResult` MUST have `media_flowed_before_authentication = false` and an empty `residual_credentials`. A violation is a hard failure of the pairing gate regardless of the verdict. |
| R-8 | A run labelled `outside-reference-environment` MUST NOT be cited as the evidence for a `go` verdict. |
| R-9 | Every `LicensingOption` MUST carry a `terms_captured_text`; an option with `redistribution_allowed = unverified` MUST be flagged in the gate report. |
| R-10 | Every `LatencySummary` MUST publish `raw`, `harness_floor_ms` and `floor_corrected` together, and MUST be judged on `raw.p95_ms`. |
| R-11 | Every loss-and-jitter run MUST reference an `ImpairmentProfile`; a run labelled `profile-deviation` MUST NOT be cited as the evidence for a `go` verdict. |
| R-12 | Every `PairingThrottlingEvidence` MUST show `limit_exceeded_at_attempt = 6`, `consecutive_failures_at_listener_close = 10`, and a non-empty `decisions` list. |
| R-13 | Both `PairingResidueInspection` cases MUST exist, MUST name the same `method`, and MUST have an empty `artefacts_found`. |
| R-14 | Every `PairingThrottlingEvidence` MUST have `session_established = false` for the throttled origin. |
| R-15 | A `LicensingDecisionBundle` whose selected device differs from the measured device MUST have `remeasurement_required = true`. |

---

## 6. State transitions — the session under test

The handshake state machine that the pairing gate exercises. This is the **validated** design; F0 proves it, it does not design it.

```text
IDLE ──user starts session──► PAIRING_READY
  │                               │
  │                               ├──QR expires (120 s) / user cancels──► IDLE
  │                               │
  │                          mobile connects (TLS 1.3, pinned)
  │                               ▼
  │                          HANDSHAKING
  │                               │
  │                    token invalid / expired / replayed
  │                               ├──► IDLE (with a reason code)
  │                               │
  │                        token valid, DTLS fingerprints exchanged
  │                               ▼
  │                          NEGOTIATING
  │                               │
  │                   ICE failure / media failure
  │                               ├──► IDLE (with a reason code)
  │                               │
  │              ICE host-only + DTLS-SRTP + first frames
  │                               ▼
  │                          CONNECTED
  │                               │
  │            ┌──────────────────┴──────────────────┐
  │            ▼                                     ▼
  │          ENDED                                SUPERSEDED
  │    (either side ends)              (a new pairing invalidates this one)
  │            │                                     │
  └────────────┴─────────────────────────────────────┴──► IDLE
                                    (resources released)
```

**Timeouts**: QR valid for 120 s; handshake within 15 s; ICE within 10 s; first frames within 3 s. Any timeout leads to `IDLE` with a reason code shown to the user. There is **no automatic reconnection** — a new session requires a new QR and a new scan.

**Transitions that the pairing gate MUST exercise**:

| Transition | Trigger | Expected |
| --- | --- | --- |
| `PAIRING_READY → IDLE` | QR expiry, cancellation | No listener remains open; no residue. |
| `HANDSHAKING → IDLE` | Wrong fingerprint, expired token, reused token, replay, downgrade attempt, interception proxy | Rejected with a reason code from the closed set; **never** falls back to an unauthenticated channel. |
| `NEGOTIATING → IDLE` | ICE failure, media failure | Session terminated; reason code recorded. |
| `CONNECTED → SUPERSEDED` | A new valid pairing with a higher epoch | The previous session's resources are released; **media overlap is prohibited**. |
| `CONNECTED → ENDED` | Either side ends the session | Resources released; no residue. |

**Session replacement rule**: a message carrying an `epoch` lower than the current one is discarded silently and counted as a protocol violation.

---

## 7. Metrics and their consumers

Constitution Principle III requires every metric to declare a purpose and a consumer before it exists.

| Metric | Purpose | Consumer | Gated? |
| --- | --- | --- | --- |
| `raw.latency_p95_ms` | Prove the pipeline meets the latency promise | Desktop receive pipeline gate | **Yes — p95 ≤ 150 ms, judged on the raw value** |
| `harness_floor_ms` | State the measurement apparatus's own delay, so a harness-limited result is distinguishable from a product regression | Report §"latency"; the gate rationale | No — reported |
| `floor_corrected.latency_p95_ms` | Show the latency with the apparatus's own delay removed | Report §"latency" | No — reported, never substituted for the raw value |
| `stage_ms` per budget stage | Show where the budget is spent | Report §"actual versus budgeted" | No |
| `cpu_pct_machine` | Prove the desktop stays within budget | Desktop receive pipeline gate | Yes — p95 ≤ 15 % |
| `private_working_set_bytes` | Prove the desktop stays within the memory budget | Desktop receive pipeline gate | Yes — ≤ 400 MB |
| `dropout_count` | Prove audio continuity under loss | Virtual audio gate | Yes — 0 |
| `phrases_exact` | Prove audio intelligibility | Virtual audio gate | Yes — ≥ 18/20 |
| `consumer_renegotiations` | Prove the stream is not interrupted | Virtual audio gate | Yes — 0 |
| `drift_ms_per_hour` | Prove audio/video stay aligned | Desktop receive pipeline gate | Yes — ≤ 20 ms/h |
| `injected_loss_pct` / `residual_loss_pct` | Prove the loss scenario was actually applied and survived | Report §"loss and jitter behaviour" | No |
| `impairment_profile` | Prove the run used the single pinned nominal profile | Desktop receive pipeline gate; Virtual audio gate | Yes — the pinned profile is a precondition |
| `injection_outcome` per application | Prove the camera is usable in real applications | Virtual camera injection gate | Yes — 100 % of the matrix |
| `longest_stall_ms` | Prove the injected stream does not freeze | Virtual camera injection gate | Yes — ≤ 2000 ms |
| `discarded_frame_count` | Prove the pipeline drops rather than blocks | Report; later the media phase | No |
| `hardware_accelerated` | The precondition for accepting latency results | Desktop receive pipeline gate | Yes — precondition |
| `residual_credentials` | Prove no credential survives the session | Pairing handshake gate | Yes — must be empty |
| `artefacts_found` (residue inspection) | Prove no pairing artefact survives a session end **or** an application restart | Pairing handshake gate | Yes — must be empty in both cases |
| `limit_exceeded_at_attempt` | Prove the per-origin attempt limit is enforced | Pairing handshake gate | Yes — 6 |
| `consecutive_failures_at_listener_close` | Prove the listener shuts down after the failure threshold | Pairing handshake gate | Yes — 10 |
| `legitimate_pairing_unaffected` | Prove throttling one origin does not deny service to another | Pairing handshake gate | Yes — must be true |
| `media_flowed_before_authentication` | Prove no media precedes authentication | Pairing handshake gate | Yes — must be false |
| `established_channel_ms` | Report pairing latency | Report; later the protocol phase | No (reported) |
| `negotiated_profile_level_id` | Record the codec parameters actually agreed | Codec interoperability gate | No (recorded) |
| `licensing_option` | Close the commercial blocker | Virtual audio and licensing gate | Yes — a decision or an approved plan B |
| `audio_latency_ms` | **Not measured in F0** — end-to-end audio latency requires the real mobile capture path | F2/F3, once the mobile capture path exists | **No — out of F0 scope** |
