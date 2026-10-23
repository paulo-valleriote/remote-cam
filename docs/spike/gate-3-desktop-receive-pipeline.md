# Gate 3 — Desktop receive pipeline

**Gate**: desktop receive pipeline · **Wave**: 2 · **Depends on**: virtual camera injection (gate 1),
pairing handshake (gate 2) · **Accountable role**: technical lead · **Target review**: 2026-11-06

**Preconditions**: PC-1 (verified hardware H.264 decode), PC-2 (raw p95 is the gated statistic and
the floor is published), PC-3 (audio continuity is measured by dropout analysis **and** a scripted
human check), PC-4 (one pinned impairment profile, media only).

**Procedure owner**: the engineer running the gate. **Sign-off**: the technical lead named in
`spikes/evidence/gate_report.json`.

---

## 1. What this gate proves

1. A **receive-only** 1280×720@30 pipeline exists: no send tracks, no STUN/TURN, DTLS-SRTP, NACK and
   RTCP receiver reports enabled, PLI opted in (FR-013, FR-015).
2. The **raw capture-at-consumer p95 is ≤ 150 ms** over three runs of at least ten minutes, and the
   measured harness floor and the floor-corrected value are published alongside it (FR-010, FR-011).
3. CPU p95 ≤ 15 % of the machine and private working set maximum ≤ 400 MB over 30 minutes
   (FR-016, FR-017).
4. A/V drift is ≤ 20 ms per hour with a confidence interval narrow enough to certify it (FR-014).
5. Audio is continuous under the pinned impairment profile, with zero dropouts ≥ 50 ms and both
   detectors agreeing (FR-030).
6. A five-second media stall does not end the session; latency returns to baseline within two
   seconds and the buffer does not grow without bound (FR-015).
7. The realised loss from the relay is distinguished from the residual loss observed after NACK
   retransmission (FR-012).

**Out of scope**: mobile capture and encode (excluded from the isolated measurement and stated in
the report), end-to-end audio latency (Q8, deferred to F2/F3), and any non-local network.

---

## 2. Preconditions

| # | Check | If it fails |
| --- | --- | --- |
| PC-1 | `verify_h264_hardware_decode()` reports `hardware_accelerated: true` and `output_format_supported: nv12`; a GPU-engine counter is non-zero during decode. | **Replace the reference machine.** Software results are reported separately and can never pass the latency or resource gate. |
| PC-2 | Every `latency_summary.json` carries `raw`, `harness_floor_ms` and `corrected`, and `criterion.basis` is `raw`. | The evidence is incomplete; the gate cannot be judged. |
| PC-3 | The audio run has a calibrated dropout analysis with two agreeing detectors and a named observer plus phrase script. | The audio half is not admissible. |
| PC-4 | `impairment_profile.json` is `nominal-3pct-30ms-10ms`, `uniform`, `media_only`, with a zero-loss control run. | The run is a `profile-deviation`; it is judged on detected dropouts and cannot support a `go`. |

```bash
make preflight        # environment.json, including hardware_decode_verification
make selftest         # toolchain_selftest.json, including the harness floor
```

`make spike-pipeline` refuses to start on a host that is not the Windows 11 reference machine or
whose hardware decoder does not pass PC-1, and writes `blocked_by_environment` to
`spikes/evidence/pipeline/run_matrix.json` instead of producing a number.

---

## 3. Run order

### 3.1 Synthetic source and marker verification

```bash
make spike-synthetic-source SOURCE_ID=src-01 DURATION_S=600
```

The source is 1280×720@30, 2 500 kbps, with a QR marker carrying `{epoch_ms}|{frame_no}` duplicated
at top-left and bottom-right. The target selects one frame, encodes it with H.264, decodes it and
has `zxing-cpp` read the payload back; `marker_roundtrip.json` records the payload and
`survives_encode_decode: true`. **Do not measure anything before this passes.**

### 3.2 Latency — three runs of at least ten minutes

```bash
make spike-latency RUN=1
make spike-latency RUN=2
make spike-latency RUN=3
```

For each run:

1. start the consumer application at 1280×720 and select the virtual camera;
2. record the wall-clock start of the capture;
3. run the identical FFmpeg capture for the calibration page and for the consumer window
   (`latency_capture::capture_command` builds both);
4. decode the consumer capture and build `latency_samples.csv`:

   ```bash
   cd spikes/tools
   .venv/bin/python -m analysis.latency capture \
     --video ../../spikes/evidence/pipeline/lat-01-capture.mp4 \
     --run-id lat-01 --consumer-app browser \
     --capture-start-epoch-ms <epoch-ms> --floor ../../spikes/evidence/pipeline/floor.json \
     --out ../../spikes/evidence/pipeline/latency_samples.csv
   .venv/bin/python -m analysis.latency summary \
     --samples ../../spikes/evidence/pipeline/latency_samples.csv \
     --floor ../../spikes/evidence/pipeline/floor.json --out ../../spikes/evidence/pipeline/latency_summary.json
   ```

5. build the stage budget:

   ```bash
   .venv/bin/python -m analysis.stages --run-id lat-01 --raw-p95-ms <raw p95> \
     --measured '{"network_and_jitter_buffer": 12.0, "decode": 8.0, "frame_handoff": 2.0, "consumer_presentation": 15.0}' \
     --out ../../spikes/evidence/pipeline/stage_budget.json
   ```

**Judgement**: `raw.p95_ms ≤ 150`. The corrected value is published and **never** substituted. When
the corrected value passes but the raw one does not, `attribution` states that the failure belongs
to the capture procedure and the procedure must be improved before the gate is judged.

### 3.3 Loss and jitter — the pinned profile

```bash
make spike-loss FORWARD=<receiver-host:port> DURATION_S=600
```

Then repeat with `LOSS_PCT=0` for the **control run**. Expected:

* `impairment_profile.json`: `nominal-3pct-30ms-10ms`, `uniform`, `added_latency_ms: 30`,
  `jitter_ms: 10`, `applies_to: media_only`, `seed: 42`;
* `relay_log.jsonl`: media decisions only, with the realised loss computed from the log;
* `relay_summary.json`: injected loss; the residual loss after NACK is read from the receiver's RTCP
  statistics and reported separately;
* the consumer capture analysed by `analysis.dropouts` with zero dropouts ≥ 50 ms and
  `detectors_agree: true`;
* `consumer_renegotiations: 0`.

A run with burst loss, a different loss percentage, or impairment applied to control traffic is
labelled `profile-deviation` and cannot pass the gate.

### 3.4 Resources — thirty minutes

```bash
make spike-resources DURATION_S=1800
```

`resource_summary.json` gates CPU on **p95** and memory on **maximum** private working set. The
`typeperf` cross-check must agree within one percentage point after dividing by the recorded logical
core count. The CSV cross-check (`cpu_pct_machine ≈ cpu_pct_single_core / cores`) is enforced by the
sampler's own test and must be inspected in the run.

### 3.5 Drift — thirty minutes, one marker pair every two seconds

The live receiver writes `drift_samples.csv`; then:

```bash
make spike-drift DURATION_S=1800 MARKER_PERIOD_S=2 SAMPLES=spikes/evidence/pipeline/drift_samples.csv
```

`drift_summary.json` reports `drift_ms_per_hour`, `r_squared`, `slope_ci95_ms_per_hour` and the three
components. A point estimate inside 20 ms/h whose interval reaches above it is `conditional`, not a
pass; increase marker density and re-run. The resampler must be RTP-paced: a free-running resampler
at 1e-5 relative error drifts ≈ 36 ms/h and fails on its own.

### 3.6 Stall — five seconds of held media

```bash
make spike-stall STALL_S=5 SAMPLES=spikes/evidence/pipeline/stall.csv
```

`stall.json` must show `session_continued: true`, `recovery_ms ≤ 2000`,
`buffer_growth_bounded: true`, and a counted `discarded_frames`. A session that ends, a recovery
slower than two seconds, or a queue that keeps growing is a failure.

---

## 4. Evidence checklist

| Artefact | Contract | Required |
| --- | --- | --- |
| `run_matrix.json` | planned scenarios + environment verdict | yes |
| `environment.json` | `contracts/measurement-formats.md` §1 | yes |
| `toolchain_selftest.json` | §1 plus harness floor | yes |
| `synthetic_source.json` + `marker_roundtrip.json` | data model §2.14 | yes |
| `latency_samples.csv` | §2 | 3 runs |
| `latency_summary.json` | §2.1 | 3 runs |
| `stage_budget.json` | §2.2 | per latency run |
| `resource_samples.csv` / `resource_summary.json` | §3 | yes |
| `relay_log.jsonl` / `impairment_profile.json` | §7, §7.1 | loss + control |
| `drift_samples.csv` / `drift_summary.json` | §6 | yes |
| `stall.json` | data model §2.5 | yes |
| `dropouts.json` | §4 | loss run |
| `decisions/D-quality-default.md` | data model §1.5 | yes |

---

## 5. Current implementation status

The pipeline crate and its deterministic units are complete and tested on Linux CI:

* `remotecam-media-core`: receive-only connection configuration, adaptive jitter policy, A/V sync,
  frame handoff with session epochs, drift regression and RTP-paced resampler, software decode
  fallback for CI, Media Foundation decoder and WASAPI renderer behind the Windows boundary;
* `spikes/f0-pipeline`: synthetic QR source, seeded RTP-aware relay, latency capture plan, resource
  sampler, drift and stall analysis, run matrix;
* `spikes/tools`: latency/floor/stage analysers and the pipeline metric tests.

**Current status (2026-10-22)**: The pipeline crate is complete and tested on Linux CI. The reference Windows 11 host (accessed via WSL2) has the full toolchain verified: rustup/MSVC, Python 3.15 (`py`), FFmpeg 9.0.1, and the `f0-pipeline` binary executes correctly. `run_matrix.json` still records `blocked_by_environment` because `preflight` detects `hardware_accelerated=true` but `output_format=other`: the `probe_hardware_decode.ps1` PowerShell probe must confirm NV12 output before the gate can proceed. `D-quality-default.md` remains `draft`.

To close the gate: run this procedure on the Windows 11 reference machine, record the five evidence
groups above, then complete `D-quality-default.md` and issue the verdict at the phase review.
