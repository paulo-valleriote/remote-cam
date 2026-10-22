# Contract: Measurement Evidence Formats

**Status**: proposed · **Consumers**: the measurement tooling under `spikes/tools/`, the gate report, the second engineer reproducing the results

Every evidence file the F0 phase produces is specified here. The formats exist so that a result is **diffable**: two engineers running the same procedure produce files that can be compared byte for byte where they should match, and differ only where the measurement genuinely differed.

General rules:

- **UTF-8**, LF line endings, no byte-order mark.
- **Timestamps** are ISO 8601 with an explicit UTC offset.
- **Paths** inside artefacts are repository-relative.
- **Units** are stated in the field name (`_ms`, `_s`, `_pct`, `_bytes`, `_kbps`) and are never implied.
- **No secrets, no personal data, no real user media.** See the redaction rules in `../data-model.md` §4.1.
- Raw media captures (`.wav`, `.mp4`) are **large binaries**. They are produced locally, referenced by path, and are **not** committed. What is committed is the derived metric and the script that produced it.

---

## 1. `environment.json` — the reference environment

One JSON object per environment record.

```json
{
  "environment_id": "ref-win11-01",
  "captured_at": "2026-11-03T09:14:22+00:00",
  "cpu_model": "AMD Ryzen 7 5800X",
  "cpu_logical_cores": 16,
  "gpu_model": "NVIDIA GeForce RTX 3060",
  "os_name": "Windows 11",
  "os_build": "22631.4169",
  "android_device_model": "Samsung SM-G991B",
  "android_os_version": "Android 14, API 34",
  "app_versions": {
    "chrome": "131.0.6778.86",
    "obs": "31.0.1",
    "windows_camera": "11.2409.1002.0"
  },
  "tool_versions": {
    "ffmpeg": "8.1.3",
    "python": "3.12.7",
    "zxing_cpp": "3.1.1"
  },
  "network": {
    "band": "5_ghz",
    "channel_width_mhz": 80,
    "access_point_model": "ACME AX6000 / fw 1.4.2",
    "distance_m": 2.5,
    "client_adapter": "MediaTek MT7921",
    "wifi_power_save": "disabled",
    "baseline_throughput_mbps": 612.4,
    "baseline_rtt_ms": 3.1,
    "baseline_loss_pct": 0.0,
    "internet_enabled": false,
    "within_reference_conditions": true
  },
  "hardware_decode_verification": {
    "codec": "H.264",
    "verified_at": "2026-11-03T09:20:01+00:00",
    "discovery_method": "mft_enum_syncmft",
    "decoder_found": true,
    "hardware_accelerated": true,
    "output_format_supported": "nv12",
    "evidence": "MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, SYNCMFT|SORTANDFILTER) -> MSH264DecoderMFT; \\GPU Engine(pid_8124_*)\\Utilization Percentage non-zero during decode"
  }
}
```

**Validation**

- `hardware_decode_verification.hardware_accelerated` MUST be `true` for any run whose results are cited for the latency gate.
- `network.within_reference_conditions` MUST be `false` for a run that used 2.4 GHz, a congested channel, or an enabled Wi-Fi power save.
- `network.internet_enabled` MUST be `false` in a reference environment.

---

## 2. `latency_samples.csv` — one row per latency observation

| Column | Type | Notes |
| --- | --- | --- |
| `sample_id` | integer | Sequential from 1. |
| `run_id` | string | |
| `consumer_app` | string | The `application_id` of the consumer. |
| `source_frame_no` | integer | The frame counter burned into the source frame. **Primary key of the observation.** |
| `source_epoch_ms` | integer | Unix epoch milliseconds, UTC, as encoded in the frame. |
| `capture_host_ms` | number | Host timestamp at which the consumer's capture device received the frame. |
| `clock_offset_ms` | number | 0 for a PC-hosted source; otherwise the value from the sync-frame handshake. |
| `latency_ms` | number | `capture_host_ms - source_epoch_ms - clock_offset_ms`. |
| `harness_floor_ms` | number | The apparatus's own floor, subtracted by the analyser when computing the corrected figure. |

**Header row is mandatory.** Column order is fixed.

### 2.1 `latency_summary.json`

```json
{
  "run_id": "lat-01",
  "sample_count": 17812,
  "harness_floor_ms": 33.4,
  "raw":        { "p50_ms": 71.2, "p95_ms": 118.6, "p99_ms": 141.0, "max_ms": 187.3 },
  "corrected":  { "p50_ms": 37.8, "p95_ms": 85.2,  "p99_ms": 107.6, "max_ms": 153.9 },
  "criterion": { "metric": "raw.p95_ms", "threshold": 150, "comparison": "<=", "basis": "raw" },
  "verdict": "pass",
  "included_stages": ["network_and_jitter_buffer", "decode", "frame_handoff", "consumer_presentation"],
  "excluded_stages": ["capture", "encode"],
  "attribution": "The harness floor of 33.4 ms accounts for 28 % of the raw p95. The failure attribution below is stated because a harness-limited result is not a product regression."
}
```

**Validation**: `criterion.basis` MUST be `raw`. The gate is judged on the **raw** capture-at-consumer p95 against the 150 ms threshold; the `corrected` figure is reported and is **never** the judged value. `harness_floor_ms`, `raw` and `corrected` MUST all be present — a summary carrying only one of the two figures is incomplete evidence. `corrected.p95_ms` MUST equal `raw.p95_ms − harness_floor_ms` within rounding. When the outcome is attributed to the capture procedure rather than to the product under test, the `attribution` field MUST say so.

### 2.2 `stage_budget.json`

One object per latency stage, with the measured time compared against the budget.

```json
{
  "run_id": "lat-01",
  "total_budget_ms": 100,
  "stages": [
    { "stage": "capture",             "budget_ms": 33, "measured_ms": null, "included_in_gate": false, "measurement_method": "excluded: mobile capture is out of scope for the isolated pipeline measurement" },
    { "stage": "encode",              "budget_ms": 15, "measured_ms": null, "included_in_gate": false, "measurement_method": "excluded: mobile encode is out of scope for the isolated pipeline measurement" },
    { "stage": "network_and_jitter_buffer", "budget_ms": 20, "measured_ms": 21.4, "included_in_gate": true,  "measurement_method": "jitter buffer playout timestamp minus packet arrival timestamp, p95" },
    { "stage": "decode",              "budget_ms": 12, "measured_ms": 9.8,  "included_in_gate": true,  "measurement_method": "media foundation transform output timestamp minus input timestamp, p95" },
    { "stage": "frame_handoff",       "budget_ms": 3,  "measured_ms": 1.6,  "included_in_gate": true,  "measurement_method": "ring buffer publish timestamp minus decode output timestamp, p95" },
    { "stage": "consumer_presentation","budget_ms": 17,"measured_ms": 41.9,  "included_in_gate": true,  "measurement_method": "consumer capture timestamp minus ring buffer publish timestamp, p95" }
  ]
}
```

**Validation**: the sum of `measured_ms` over the included stages MUST be ≤ the reported **raw** capture-at-consumer figure for the same run, or the difference MUST be explained in `measurement_method`.

---

## 3. `resource_samples.csv` — one row per resource sample

| Column | Type | Notes |
| --- | --- | --- |
| `sample_id` | integer | Sequential from 1. |
| `run_id` | string | |
| `elapsed_ms` | number | Milliseconds since the start of the measurement window. Strictly increasing. |
| `cpu_pct_machine` | number | **Gated metric.** `100 * delta_process_time / delta_system_time`. Range 0–100. |
| `cpu_pct_single_core` | number | `100 * delta_process_time / delta_wall_clock`. May exceed 100. |
| `private_working_set_bytes` | integer | **Gated memory metric.** What Task Manager shows as "Memory". |
| `total_working_set_bytes` | integer | Includes shared pages. |
| `peak_working_set_bytes` | integer | |
| `thread_count` | integer | |
| `handle_count` | integer | |

**Cross-check rule**: `cpu_pct_machine ≈ cpu_pct_single_core / cpu_logical_cores`. A mismatch beyond rounding is an instrumentation error and invalidates the run.

### 3.1 `resource_summary.json`

```json
{
  "run_id": "res-01",
  "duration_s": 1800,
  "sample_count": 54000,
  "cpu_pct_machine": { "p50": 6.1, "p95": 11.4, "p99": 14.2, "max": 19.8 },
  "private_working_set_mb": { "p50": 218.4, "p95": 287.1, "max": 331.6 },
  "criterion": {
    "cpu": { "metric": "cpu_pct_machine.p95", "threshold": 15, "comparison": "<=" },
    "memory": { "metric": "private_working_set_mb.max", "threshold": 400, "comparison": "<=" }
  },
  "verdict": "pass",
  "sampler": "GetProcessTimes + GetSystemTimes + GetProcessMemoryInfo",
  "cross_check": {
    "tool": "typeperf",
    "cpu_pct_machine_p95": 11.1,
    "agreement_within_one_point": true
  }
}
```

**Validation**: the gated CPU statistic is the **p95**, never the mean. The gated memory statistic is the **maximum** private working set.

---

## 4. `dropouts.json` — the silence analysis

```json
{
  "run_id": "aud-01",
  "capture_file": "spikes/evidence/aud-01/consumer_capture.wav",
  "sample_rate_hz": 48000,
  "channels": 1,
  "duration_s": 610.4,
  "criterion": { "min_dropout_ms": 50, "threshold_dbfs": -50, "threshold_source": "calibrated: noise floor -56 dBFS + 6 dB" },
  "detectors": {
    "silencedetect": { "dropout_count": 0, "intervals": [] },
    "rms_window":   { "dropout_count": 0, "intervals": [], "window_ms": 5, "hold_ms": 10 }
  },
  "detectors_agree": true,
  "below_threshold_intervals": 3,
  "correlated_rtp_gaps": 0,
  "consumer_renegotiations": 0,
  "verdict": "pass",
  "analyser_version": "1.0.0"
}
```

An interval object, when present:

```json
{ "start_s": 61.42, "end_s": 61.472, "duration_ms": 52.0, "detector": "silencedetect", "correlated_rtp_gap": true }
```

**Rules**

- A dropout is any contiguous detected silent interval of **at least 50 ms**. Shorter intervals are recorded as `below_threshold_intervals` and MUST NOT count as dropouts.
- `detectors_agree` MUST be `true` for a clean run. A disagreement is a **finding**, recorded separately, and never silently resolved by picking one detector.
- `threshold_dbfs` MUST be the calibrated value with `threshold_source` naming the calibration. A hard-coded threshold without a calibration record is invalid.
- The dropped intervals MUST be cross-referenced against the receiver's RTP sequence-number gap log; `correlated_rtp_gap` distinguishes a transport loss from a render or consumer-side defect.

---

## 5. `intelligibility.json` — the scripted phrase check

```json
{
  "run_id": "aud-01",
  "manifest": "spikes/fixtures/phrases-v1.csv",
  "phrase_count": 20,
  "non_anchor_count": 19,
  "phrases_exact": 19,
  "corpus_wer": 0.0,
  "nonces_matched": 10,
  "nonces_total": 10,
  "anchors_match": true,
  "observer": "<named observer>",
  "criterion": {
    "phrases_exact_min": 18,
    "corpus_wer_max": 0.05,
    "nonces_must_all_match": true,
    "anchors_must_match": true
  },
  "verdict": "pass",
  "supplementary": {
    "asr_wer": 0.037,
    "asr_model": "faster-whisper / base.en",
    "estoi_median": 0.81,
    "note": "supplementary trend evidence only; never gated"
  },
  "per_phrase": [
    { "id": "P01", "kind": "static", "reference": "the quick brown fox jumps over the lazy dog", "transcribed": "the quick brown fox jumps over the lazy dog", "exact": true, "wer": 0.0 }
  ]
}
```

**Rules**

- `phrases_exact` counts **non-anchor** phrases only.
- Every nonce-bearing phrase MUST have its nonce match. A mismatch invalidates the capture as stale or spliced, and the run MUST be repeated.
- The opening and closing anchors MUST agree.
- A phrase the observer heard nothing for is a **failure**, not a missing row.

---

## 6. `drift_samples.csv` and `drift_summary.json`

`drift_samples.csv` columns:

| Column | Type | Notes |
| --- | --- | --- |
| `sample_id` | integer | |
| `run_id` | string | |
| `elapsed_s` | number | Seconds since the start of the drift run. |
| `source_skew_ms` | number | Source-time difference between the matched audio and video items. |
| `audio_playout_delay_ms` | number | |
| `video_playout_delay_ms` | number | |
| `playout_skew_ms` | number | `source_skew_ms + audio_playout_delay_ms - video_playout_delay_ms`. |

`drift_summary.json`:

```json
{
  "run_id": "drift-01",
  "duration_s": 1830,
  "sample_count": 900,
  "marker_period_s": 2,
  "regression": {
    "slope_ms_per_s": 0.00112,
    "drift_ms_per_hour": 4.03,
    "intercept_ms": -0.8,
    "r_squared": 0.91,
    "slope_ci95_ms_per_hour": [1.4, 6.7]
  },
  "criterion": { "metric": "drift_ms_per_hour", "threshold": 20, "comparison": "<=" },
  "verdict": "pass",
  "components": {
    "sender_side_skew_drift_ms_per_hour": 0.0,
    "receiver_resampler_drift_ms_per_hour": 3.1,
    "receiver_present_vs_render_drift_ms_per_hour": 0.9
  },
  "note": "The receiver audio renderer is paced from the RTP timestamps; a free-running fixed-point resampler was measured separately and is reported because its relative frequency error of approximately 1e-5 corresponds to approximately 36 ms per hour, which exceeds the criterion on its own."
}
```

**Rules**

- The `slope_ci95_ms_per_hour` interval MUST be narrow enough to certify the criterion. A regression whose interval straddles the threshold does **not** pass; the verdict is `conditional` and the marker density MUST be increased.
- `components` MUST break the drift down, because the three causes have different remediations.

---

## 7. `relay_log.jsonl` — the impairment log

One JSON object per line, one line per impairment decision.

```json
{"t_ms": 12043.2, "ssrc": 2245318912, "payload_type": 96, "seq": 18422, "action": "drop", "delay_ms": 0.0}
{"t_ms": 12067.8, "ssrc": 2245318912, "payload_type": 96, "seq": 18423, "action": "delay", "delay_ms": 14.2}
{"t_ms": 12100.1, "ssrc": 2245318912, "payload_type": 96, "seq": 18424, "action": "stall_release", "delay_ms": 5000.0}
```

**Rules**

- The file MUST record the `seed` in a header line or a sibling `relay_config.json`. The seed is what makes the run reproducible.
- **Control traffic is never impaired.** Only media SSRCs appear in this log. A dropped DTLS or ICE packet would break the session and present as a product defect rather than as media loss.
- The realised loss percentage MUST be computed from this log and reported alongside the loss the application actually observed after retransmission.

---

## 7.1 `impairment_profile.json` — the pinned impairment profile

The **single** nominal profile used by every loss-and-jitter run. One profile exists; a run that deviates from it is labelled and cannot pass a gate.

```json
{
  "profile_name": "nominal-3pct-30ms-10ms",
  "loss_pct": 3.0,
  "loss_distribution": "uniform",
  "added_latency_ms": 30,
  "jitter_ms": 10,
  "applies_to": "media_only",
  "seed": 42,
  "relay_log_ref": "spikes/evidence/pipeline/relay_log.jsonl"
}
```

**Rules**

- `profile_name` MUST be `nominal-3pct-30ms-10ms`. There is exactly one profile.
- `loss_distribution` MUST be `uniform`. **Burst loss is not a member of this profile.**
- `jitter_ms` is the jitter **amplitude**; the realised deviation is within ±10 ms.
- `applies_to` MUST be `media_only`.
- A run whose realised conditions deviate from this profile MUST carry the label `profile-deviation`, MUST be judged on **detected dropouts** rather than on average loss percentage, and MUST NOT be cited as the evidence for a `go` verdict.
- The realised loss and the residual loss at the application MUST both be reported.

---

## 8. `pairing_scenarios.json` — the handshake evidence

```json
{
  "harness_version": "1.0.0",
  "scenarios": [
    {
      "scenario_id": "valid-01",
      "category": "valid",
      "preconditions": "Fresh desktop session; QR displayed; mobile scans.",
      "expected_result": "Authenticated channel established; media flows.",
      "observed_result": "Channel established in 2140 ms; media received.",
      "outcome": "accepted",
      "reason_code": null,
      "media_flowed_before_authentication": false,
      "residual_credentials": [],
      "established_channel_ms": 2140,
      "pinning_added_delay_ms": 11,
      "trace_ref": "spikes/evidence/pairing/valid-01.trace"
    },
    {
      "scenario_id": "adversarial-wrong-fingerprint",
      "category": "adversarial",
      "preconditions": "Mobile configured with a fingerprint that differs in one byte.",
      "expected_result": "Connection aborted before any media.",
      "observed_result": "TLS alert; no media sent or received.",
      "outcome": "rejected",
      "reason_code": "certificate_pin_mismatch",
      "media_flowed_before_authentication": false,
      "residual_credentials": [],
      "trace_ref": "spikes/evidence/pairing/wrong-fingerprint.trace"
    }
  ],
  "summary": {
    "total": 12,
    "accepted": 1,
    "rejected": 11,
    "inconclusive": 0,
    "scenarios_with_media_before_authentication": 0,
    "scenarios_with_residual_credentials": 0
  }
}
```

**Rules**

- Every scenario MUST appear, including the valid one.
- `media_flowed_before_authentication` MUST be `false` in every scenario.
- `residual_credentials` MUST be empty in every scenario.
- `reason_code` MUST be a member of the closed set in [`reason-codes.md`](./reason-codes.md).

---

## 8.1 `pairing_throttling.json` — the throttling evidence

Proves the per-origin attempt limit, the progressive blocking and the listener shutdown. A configuration value is not evidence.

```json
{
  "harness_version": "1.0.0",
  "scenarios": [
    {
      "throttle_id": "throttle-rate-limit",
      "origin_id": "192.168.1.77",
      "attempts_observed": 12,
      "attempt_limit_per_minute": 5,
      "limit_exceeded_at_attempt": 6,
      "reason_code": "rate_limited",
      "blocked": true,
      "block_duration_s": 60,
      "legitimate_pairing_unaffected": true,
      "session_established": false,
      "decisions": [
        { "sequence": 1, "at_ms": 0,     "origin_id": "192.168.1.77", "action": "allowed",      "reason_code": null,              "outcome": "challenge issued" },
        { "sequence": 6, "at_ms": 42000, "origin_id": "192.168.1.77", "action": "rate_limited", "reason_code": "rate_limited",   "outcome": "attempt refused" },
        { "sequence": 7, "at_ms": 43500, "origin_id": "192.168.1.77", "action": "blocked",      "reason_code": "rate_limited",   "outcome": "origin blocked for 60 s" }
      ],
      "trace_ref": "spikes/evidence/pairing/throttle-rate-limit.trace",
      "security_event": false
    },
    {
      "throttle_id": "throttle-listener-close",
      "origin_id": "192.168.1.77",
      "attempts_observed": 10,
      "attempt_limit_per_minute": 5,
      "limit_exceeded_at_attempt": 6,
      "reason_code": "rate_limited",
      "blocked": true,
      "block_duration_s": 120,
      "consecutive_failures_at_listener_close": 10,
      "listener_closed": true,
      "legitimate_pairing_unaffected": true,
      "session_established": false,
      "decisions": [
        { "sequence": 10, "at_ms": 91000, "origin_id": "192.168.1.77", "action": "listener_closed", "reason_code": "listener_closed", "outcome": "pairing listener closed after 10 consecutive failures" }
      ],
      "trace_ref": "spikes/evidence/pairing/throttle-listener-close.trace",
      "security_event": true
    }
  ]
}
```

**Rules**

- `attempt_limit_per_minute` MUST be **5** and `limit_exceeded_at_attempt` MUST be **6** — the sixth attempt within a minute is refused.
- `consecutive_failures_at_listener_close` MUST be **10**.
- The limit is scoped **per origin**. A global limit is a defect: it turns one hostile origin into a denial of service against the legitimate user.
- `session_established` MUST be `false` for every throttled origin.
- Every entry in `decisions` MUST carry a reason code from the closed set and an observed outcome. Free-text reasons are forbidden; the `outcome` field is an observation, not a reason.
- `legitimate_pairing_unaffected` MUST be `true` when a legitimate second origin pairs during the flood.
- The listener closure MUST be recorded as a **security event**, not as a product failure.

---

## 8.2 `pairing_residue.json` — the residue inspections

Proves that no pairing artefact survives a normal session end **and** an application restart, using the **same** inspection method in both cases.

```json
{
  "harness_version": "1.0.0",
  "method": "Process memory dump scan (procdump), filesystem sweep of the app data directory and the platform credential store, and registry enumeration of HKCU\\Software for the app vendor key; each location searched for a session key, pairing token, fingerprint or pairing record.",
  "coverage": [
    "app data directory (files, registry-backed stores and crash-recovery journals)",
    "platform credential and certificate store",
    "registry HKCU\\Software\\<vendor>\\<app>",
    "process memory after the session closed"
  ],
  "inspected_at": "2026-11-05T10:22:41+00:00",
  "devices_inspected": ["windows-11-reference", "android-reference"],
  "inspections": [
    {
      "inspection_id": "residue-after-session-end",
      "case": "after_session_end",
      "inspected_at": "2026-11-05T10:18:03+00:00",
      "devices_inspected": ["windows-11-reference", "android-reference"],
      "artefact_kinds_checked": ["session_key", "pairing_token", "fingerprint", "pairing_record"],
      "artefacts_found": [],
      "evidence_ref": "spikes/evidence/pairing/residue-after-session-end.txt"
    },
    {
      "inspection_id": "residue-after-application-restart",
      "case": "after_application_restart",
      "inspected_at": "2026-11-05T10:22:41+00:00",
      "devices_inspected": ["windows-11-reference", "android-reference"],
      "artefact_kinds_checked": ["session_key", "pairing_token", "fingerprint", "pairing_record"],
      "artefacts_found": [],
      "evidence_ref": "spikes/evidence/pairing/residue-after-application-restart.txt"
    }
  ]
}
```

**Rules**

- **Both** cases MUST be present: `after_session_end` and `after_application_restart`.
- Both inspections MUST name the **same** `method`. A different method for the restart case makes the two results incomparable and is invalid.
- `coverage` MUST be non-empty and MUST state what was searched. An inspection with no stated coverage is not evidence.
- `artefact_kinds_checked` MUST list all four kinds in both inspections.
- `artefacts_found` MUST be empty in both. A residue is a **hard failure** of the pairing gate, independent of the verdict.
- Both the desktop and the mobile MUST be inspected in each case.
- A pairing token from a previous session MUST NOT be reusable without a new one-time token.

---

## 9. `audio_run_capture.json` — the audio run index

```json
{
  "capture_id": "aud-01",
  "run_id": "aud-01",
  "capture_file": "spikes/evidence/aud-01/consumer_capture.wav",
  "capture_format": "wav",
  "sample_rate_hz": 48000,
  "channels": 1,
  "duration_s": 610.4,
  "device_name": "CABLE Output (VB-Audio Virtual Cable)",
  "device_pin": "VB-CABLE",
  "device_version": "driver pack 45",
  "signature_verified": true,
  "consumer_app": "obs",
  "consumer_renegotiations": 0,
  "dropout_analysis_ref": "spikes/evidence/aud-01/dropouts.json",
  "intelligibility_ref": "spikes/evidence/aud-01/intelligibility.json",
  "intelligibility_observer": "<named observer>",
  "phrase_script_ref": "spikes/fixtures/phrases-v1.csv",
  "analyser_version": "1.0.0"
}
```

`device_pin` is the product and version **used for the technical measurement**, pinned so that a later change of the selected device does not invalidate the evidence. `intelligibility_observer` and `phrase_script_ref` are **mandatory**: the scripted human check is half of the "continuous and intelligible audio" criterion, and an unnamed observer or an unrecorded script makes the result unverifiable (FR-029).

The licence terms of every candidate device considered — including candidates not used for the measurement — are recorded separately in `licensing_bundle.json`. If the licensing decision selects a device other than the measured one, the bundle MUST set `remeasurement_required = true`: the newly selected device MUST be re-measured against the same recorded steps before it becomes the shipping option.

---

## 9.1 `licensing_bundle.json` — the licensing decision bundle

The licence terms of **every** candidate device considered, assessed in parallel with the technical measurement, plus the decision itself.

```json
{
  "bundle_id": "lic-01",
  "measurement_device": {
    "option_id": "vb-cable-base",
    "device_name": "VB-CABLE",
    "vendor": "VB-Audio",
    "version": "driver pack 45",
    "license_model": "donationware",
    "terms_url": "https://vb-audio.com/Cable/WebDownloads/",
    "terms_captured_text": "…the quoted text that supports the redistribution position…",
    "redistribution_allowed": "yes_with_conditions",
    "commercial_use_allowed": true,
    "unit_cost": 3.61,
    "cost_model": "per unit at 100 units",
    "support_burden": "low",
    "user_experience": "user installs from the official site; no admin rights beyond the driver install; no reboot on the current driver pack",
    "signature_verified": true,
    "fallback": "video-only V1"
  },
  "candidate_options": [
    { "option_id": "vac-oem", "device_name": "Virtual Audio Cable", "vendor": "MuTools", "license_model": "proprietary_paid", "terms_url": "https://www.museaudio.com/products/virtual-audio-cable/", "terms_captured_text": "…", "redistribution_allowed": "no", "commercial_use_allowed": true, "unit_cost": 50, "cost_model": "one per running instance", "support_burden": "medium", "user_experience": "…", "signature_verified": false, "fallback": "VB-CABLE link-only" }
  ],
  "options_compared": ["redistribution_licensing", "user_installed_official_driver", "plan_b_video_only"],
  "decision_owner": { "role_type": "product_owner", "person_name": "<named individual>" },
  "approver": { "role_type": "product_owner", "person_name": "<named individual>" },
  "decided_at": "2026-11-05",
  "fallback": "video-only V1",
  "remeasurement_required": false
}
```

**Rules**

- `candidate_options` MUST include **every** device considered, including the ones that were **not** used for the technical measurement (FR-033).
- `options_compared` MUST cover at least redistribution licensing, a user-installed official driver, and a plan B alternative.
- Every option MUST carry `terms_captured_text` — the quoted text that supports the claim. A claim without captured text is unverifiable.
- `decision_owner` and `approver` MUST be **named** individuals, not role titles (FR-034).
- `remeasurement_required` MUST be `true` when the selected device is **not** `measurement_device`. The newly selected device MUST be re-measured against the same recorded steps before it becomes the shipping option.
- An unresolved licensing decision on the gate date MUST NOT be reported as passed. The bundle is recorded as failed or conditional, with the blocker, the escalation owner and the interim audio scope stated.

---

## 10. File and directory layout of the evidence root

```text
spikes/evidence/
├── environment.json
├── pairing/
│   ├── pairing_scenarios.json
│   ├── pairing_throttling.json
│   ├── pairing_residue.json
│   └── <scenario_id>.trace
├── pipeline/
│   ├── latency_run1_samples.csv
│   ├── latency_run1_summary.json
│   ├── latency_run2_samples.csv
│   ├── latency_run2_summary.json
│   ├── latency_run3_samples.csv
│   ├── latency_run3_summary.json
│   ├── stage_budget.json
│   ├── resource_samples.csv
│   ├── resource_summary.json
│   ├── drift_samples.csv
│   ├── drift_summary.json
│   ├── impairment_profile.json
│   ├── stall.json
│   └── relay_log.jsonl
├── injection/
│   └── injection_matrix.json
├── audio/
│   ├── aud-01/
│   │   ├── consumer_capture.wav     # local only, not committed
│   │   ├── dropouts.json
│   │   └── intelligibility.json
│   └── relay_log.jsonl
├── licensing/
│   └── licensing_bundle.json
└── decisions/
    └── <decision_id>.md
```

---

## 11. Toolchain self-test artefacts

Before any measurement is trusted, the toolchain MUST pass its own self-test. The result is committed as `spikes/evidence/toolchain_selftest.json`:

```json
{
  "run_at": "2026-11-03T09:02:00+00:00",
  "checks": [
    { "name": "silence_fixture", "expected": { "dropout_count": 2 }, "observed": { "dropout_count": 2 }, "passed": true },
    { "name": "qr_roundtrip", "expected": { "decoded_payload_sha256": "…" }, "observed": { "decoded_payload_sha256": "…" }, "passed": true },
    { "name": "sampler_agreement", "expected": { "delta_p95_cpu_pct": "<= 1.0" }, "observed": { "delta_p95_cpu_pct": 0.3 }, "passed": true },
    { "name": "harness_floor", "expected": { "floor_ms": "measured" }, "observed": { "floor_ms": 33.4 }, "passed": true }
  ],
  "all_passed": true
}
```

**Rule**: if `all_passed` is `false`, **no measurement result is valid**. The self-test is the mechanism that turns "a second engineer can reproduce this within one working day" from an intention into a check.
