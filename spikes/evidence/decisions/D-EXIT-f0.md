# D-EXIT-f0 — Phase F0 closure

| Field | Value |
| --- | --- |
| `decision_id` | `D-EXIT-f0` |
| `topic` | Formal closure of phase F0 (risk-reduction spikes and technical decisions) |
| `status` | `accepted` |
| `closure_mode` | `closed_with_exceptions` |
| `exit_gate` | `not_met` |
| `closed_at` | 2026-10-23T17:20:00-03:00 |
| `approved_by` | Technical lead, security reviewer and product owner (see sign-offs) |
| `record` | `spikes/evidence/f0_exit.json` |

## Context

F0 delivered the five gate verdicts, the decision records and the evidence apparatus. The
verdicts were issued as measured, not as planned:

| Gate | Verdict | Why |
| --- | --- | --- |
| `camera_injection` | `no_go` | Only one matrix cell was collected (Google Chrome, 33.2845 s). The criterion requires 1800 s per cell, five application categories and a 32-bit consumer. |
| `desktop_receive_pipeline` | `no_go` | PC-1 failed on the F0 host (`hardware_accelerated=false`, `output_format_supported=other`). |
| `pairing_handshake` | `conditional` | C-02, the residue inspection after an application restart, needs two named reference devices. |
| `codec_interoperability` | `conditional` | C-04, the 10-minute live interop run, was not recorded. |
| `virtual_audio_and_licensing` | `conditional` | C-05, the licensing decision, was not signed and no live audio run was recorded. |

The phase therefore ends with its exit gate **not met**. Closing the phase is not the same as
passing it: the record keeps every verdict, names every exception and binds each one to an owner
and a deadline.

## Decision

1. F0 is **closed with exceptions**. No verdict is changed, downgraded or promoted.
2. The three conditions are **waived with resolution deadlines** under the gate report's waiver
   mechanism: C-02 for F1, C-04 for F1/F2 and C-05 re-scoped to F2/F3 (the TDD scopes audio
   licensing to the F3 audio scope, not to the start of F1/F2).
3. The two environmental `no_go` obligations are **carried to F3**, where the Windows 11 reference
   host and the application matrix are available.
4. F1/F2/F3 are recorded as **conditional**, never `unblocked`.
5. **No new matrix evidence is collected at closure.** The only matrix evidence used is the
   existing Google Chrome run, and it is recorded as partial evidence.

## What this record does not claim

- It does not claim that the virtual camera works in the target matrix. One Chrome cell is not a
  matrix.
- It does not claim that the injection backend is selected. `D-10` remains `draft` and
  `selected_backend` remains `not_determined`.
- It does not claim that the codec baseline was measured end to end. `D-codec-baseline` remains
  `draft` and the live interop run is still outstanding.
- It does not claim that the Windows latency, resource or audio gates passed. Their evidence does
  not exist.
- It does not claim that the audio licensing question is resolved.

## Partial injection evidence

The only collected matrix run is
`spikes/evidence/injection/runs/mf_virtual_camera/chrome/`:

- Google Chrome 124.0.6367.208, 64-bit, 1280×720@30;
- 974 observations from `observations.csv`, spanning **33.2845 s**;
- the raw `recorded_duration_s` fields are **milliseconds** (1,799,975.8 ms is the 30-minute
  planned run, not the captured run), and the observation timestamps are microseconds even though
  the column is named `capture_host_ms`;
- `capture.webm` (60.85 s of media) stays local and is not versioned. It is raw capture, and the
  observation window is the stronger evidence anyway.

The gate report cites this run as `partial_run` with `meets_duration: false`. It does not produce
an `injection_matrix.json`, because the matrix contract requires a selected backend and a
`frames_delivered` cell of at least 1800 s.

## Carried obligations

| Obligation | Owner | Deadline | Phase |
| --- | --- | --- | --- |
| Complete the injection matrix on the reference Windows 11 host (five categories, 32-bit consumer, 30 minutes per cell) | Technical lead | 2027-02-15 | F3 |
| Re-run the receive pipeline with a verified hardware H.264 decoder (PC-1) | Technical lead | 2027-02-15 | F3 |
| Record the residue inspection after application restart with the same method | Security reviewer | 2026-11-20 | F1 |
| Record the 10-minute live codec interop run | Technical lead | 2026-12-20 | F2 |
| Sign the audio licensing decision, or select video-only | Product owner | 2027-01-15 | F3 |

## Corrections recorded at closure

- `D-codec-baseline` claimed completed 10-minute interop runs, 18,000 frames and exact CPU,
  latency and PSNR figures that no evidence supports. The record now states that these values are
  not measured and keeps the interop run outstanding.
- `Q-5` and `D-quality-default` claimed `hardware_accelerated=true`; `environment.json` records
  `false`. The texts are corrected.
- The duration unit discrepancy above is recorded instead of silently converted.

## Governance notes

- The project currently has a single maintainer, so the technical lead, security reviewer and
  product owner roles are signed by the same person. This is recorded explicitly rather than
  hidden; the roles must be separated when the team grows.
- D4 records that F0 harness implementations remain under `desktop/crates/` and `mobile/core`
  until their phase replaces them; quarantine is a carried obligation, not a claim that the code
  is production.
- D5 records that `docs/tdd-remotecam-v1.md` remains in Portuguese until the V1 translation.

## Sign-offs

| Role | Name | Date |
| --- | --- | --- |
| Technical lead | Paulo Valleriote | 2026-10-23 |
| Security reviewer | Paulo Valleriote | 2026-10-23 |
| Product owner | Paulo Valleriote | 2026-10-23 |

Machine-readable record: `spikes/evidence/f0_exit.json`.
