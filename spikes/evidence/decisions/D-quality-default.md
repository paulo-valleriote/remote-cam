# D-quality-default — Default video quality

| Field | Value |
| --- | --- |
| `decision_id` | `D-quality-default` |
| `topic` | The default and maximum video quality the product offers on a local-network session |
| `status` | `draft` |
| `chosen_option` | 1280×720@30 default, 1920×1080@30 optional maximum (recommended; not yet `accepted`) |
| `approver` | Technical lead — **not yet signed**; the record cannot be `accepted` before the measured evidence exists |
| `decided_at` | — |
| `evidence_refs` | `spikes/f0-media-core/src/receiver.rs`, `spikes/f0-media-core/src/adapters/mf_decoder.rs`, `spikes/f0-media-core/src/jitter_policy.rs`, `spikes/f0-pipeline/src/synthetic_source.rs`, `spikes/evidence/pipeline/latency_summary.json` (**not yet produced**) |
| `downstream_impact` | F1/F2 encode settings, the mobile camera profile, the bandwidth budget and the installer's default quality |

## Status note

This record is **`draft`**, deliberately. The reference Windows 11 host is accessible via WSL2; Rust (rustup), MSVC (`link.exe`), Python 3.15 (`py`), FFmpeg 9.0.1, and the pipeline binary (`f0-pipeline.exe`) are installed and verified. The pipeline executes and writes evidence, but `run_matrix.json` remains `blocked_by_environment` because the hardware-decode verification (`probe_hardware_decode.ps1`) has not confirmed NV12 output (`output_format=other`, `hardware_accelerated=false`). A named approver and measured `latency_summary.json`/`resource_summary.json` are still required for `accepted`.

What is settled here: the option set, the criterion that selects between them, the exact procedure,
and the evidence that has been produced (pipeline executes, MSVC/link active, preflight detects decoder,
but `run_matrix.json` remains `blocked_by_environment` due to `output_format=other`).

## Options considered

### Option A — 1280×720@30 default, 1920×1080@30 optional maximum

* 720p30 is the resolution the latency and resource gates are budgeted for (≈ 100 ms budget,
  2 500 kbps reference source).
* 1080p30 is offered only when the measured p95 still fits the 150 ms gate and the CPU p95 stays
  under 15 % at the higher rate. It is an explicit maximum, not the default.
* Hardware decode must still expose NV12 at both sizes.

### Option B — 1920×1080@30 default

* Higher detail, but the reference budget was built for 720p30; without a measured 1080p run the
  latency margin is unknown.
* The source TDD's default is 720p. Choosing otherwise would require evidence this phase does not
  have.

### Option C — 1280×720@30 only

* The safest shape if the 1080p run misses the criterion.
* Loses the option of a sharper picture on capable machines and the product explicitly allows
  1920×1080 as an optional maximum.

### Option D — adaptive resolution

Rejected for V1: it adds a congestion-control loop and a second encoder configuration to a phase
whose risk is already the receive pipeline. Revisit after F2/F3 have real mobile capture data.

## Selection rule

Option A becomes `accepted` only when **all** of the following are recorded:

1. `latency_summary.json` for three ≥ 10-minute runs at 720p30: `raw.p95_ms ≤ 150`, with floor and
   corrected values;
2. `resource_summary.json` at 720p30: `cpu_pct_machine.p95 ≤ 15`, `private_working_set_mb.max ≤ 400`;
3. a 1080p30 variant of the same run shows `raw.p95_ms ≤ 150` and CPU p95 ≤ 15 % — otherwise Option C
   is selected and 1080p is removed from the claim;
4. the hardware-decode verification in `environment.json` states `hardware_accelerated: true` and
   `output_format_supported: nv12`;
5. the technical lead signs the record with a date.

If (3) fails, the record is rewritten as Option C; it is not silently edge-cased in the UI.

## Q-5 — hardware decode on the reference machine

Q-5 asks whether the reference machine exposes an H.264 decoder through the `SYNCMFT` discovery path
and whether it is actually hardware-accelerated.

The answer is produced by `remotecam_media_core::adapters::mf_decoder::verify_h264_hardware_decode`,
which enumerates `MFT_CATEGORY_VIDEO_DECODER` with `SYNCMFT|SORTANDFILTER`, enumerates the
`HARDWARE` flag set, activates the first decoder and checks for an NV12 output type. The command is
run by `make preflight` and by the pipeline harness before any measurement; the result is recorded in
`environment.json.hardware_decode_verification` and `run_matrix.json`.

**Current answer: not verified.** The reference Windows 11 host (WSL2 access) has Rust/MSVC/FFmpeg installed and the pipeline binary executes, but `environment.json` records `hardware_accelerated=false`, `decoder_found=false` and `output_format=other`; the `probe_hardware_decode.ps1` probe failed with an `MFTEnumEx` cast error, so NV12 output was never demonstrated. The Linux CI cannot verify PC-1.

## Evidence that exists today

| Evidence | Where | What it proves |
| --- | --- | --- |
| Receive-only configuration tests | `spikes/f0-media-core/src/receiver.rs` | No ICE server, no send track, NACK/RTCP/PLI are required |
| Jitter-policy tests | `spikes/f0-media-core/tests/jitter_policy.rs` | Target depth adapts, playout delay moves in bounded steps, stall fast-forwards, depth is bounded |
| Drift tests | `spikes/f0-media-core/tests/drift.rs` | 5 ms/h recovered; a free-running resampler at 36 ms/h fails; RTP pacing converges |
| Concurrency and teardown tests | `spikes/f0-media-core/tests/{concurrency,teardown}.rs` | Ordering, idempotent session events, epoch invalidation, handle release |
| Media Foundation verification and decoder | `spikes/f0-media-core/src/adapters/mf_decoder.rs` | NV12-only output, DXGI device manager, documented buffer-lock order |
| Latency analyser tests | `spikes/tools/tests/test_pipeline_metrics.py` | Raw p95 is the judged value; floor and corrected value are published; stage sum rule |

The gate procedure is `docs/spike/gate-3-desktop-receive-pipeline.md`.
