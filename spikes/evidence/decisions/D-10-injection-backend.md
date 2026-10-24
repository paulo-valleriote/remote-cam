# D-10 — Injection backend

| Field | Value |
| --- | --- |
| `decision_id` | `D-10` |
| `topic` | Which backend injects the camera picture into consumer applications: the Windows 11 Media Foundation virtual camera API, or a DirectShow capture filter |
| `status` | `draft` |
| `chosen_option` | `mf_virtual_camera` (recommended; not yet `accepted`) |
| `approver` | Technical lead — **not yet signed**; the record cannot be `accepted` before the matrix exists |
| `decided_at` | — |
| `evidence_refs` | `spikes/f0-injection/src/mf_virtual_camera.rs`, `spikes/f0-injection/src/registration.rs`, `spikes/tools/analysis/injection.py`, `spikes/f0-camera-injection/run_matrix.py`, `spikes/evidence/injection/runs/mf_virtual_camera/chrome/run.json` (**partial run**), `spikes/evidence/injection/injection_matrix.json` (**not yet produced**) |
| `downstream_impact` | The media and injection phase: fixes the frame handoff contract, the installer, and the bitness support obligation |

## Status note

This record is **`draft`**, deliberately. The data model makes `accepted`
conditional on measured evidence, a named approver and a decision date, and none
of the three can be produced on the F0 host: it is Linux/WSL2 with no Windows
Media Foundation, no FFmpeg, no OBS and no camera
(`spikes/evidence/environment.json` records `hardware_accelerated: false` and
`within_reference_conditions: false`). Claiming an accepted backend decision
from this host would be fabricating gate evidence.

What *is* settled and recorded here: the option set, the selection rule, the
implementation status of each option, the licensing impact, and the exact
procedure that turns this draft into an accepted record.

**Partial evidence recorded at F0 closure (2026-10-23).** One matrix cell was
collected with Google Chrome 124.0.6367.208 (64-bit, 1280×720@30) under
`spikes/evidence/injection/runs/mf_virtual_camera/chrome/`: 974 observations
over 33.2845 s. It proves the media source was visible to one consumer for a
short window; it does not satisfy the 1800 s per-cell, five-category, 32-bit
criterion, so it does not select a backend. No further matrix evidence was
collected at closure.

## Options considered

### Option A — `mf_virtual_camera` (Media Foundation virtual camera API)

A Media Foundation media source registered through `MFCreateVirtualCamera` with
`MFVirtualCameraAccess_CurrentUser`, hosted by the Windows Frame Server.

* Implemented in `spikes/f0-injection/src/mf_virtual_camera.rs`:
  `IMFMediaSource` + `IMFMediaStream` + `IMFMediaEventGenerator`, NV12
  1280×720@30 and 640×480@30, frame timestamps, stream state transitions, and
  the `IClassFactory`/`DllGetClassObject` registration points.
* Registration in `spikes/f0-injection/src/registration.rs`: the COM
  in-process server under `HKCU\Software\Classes\CLSID\{...}` and the camera
  instance with `MFVirtualCameraLifetime_Session`. `register_current_user`
  refuses to elevate; `register_machine_wide` exists only to *measure* the
  `HKLM` fallback for Q-1.
* Status: **compiles and is clippy-clean for `x86_64-pc-windows-msvc`**;
  **never executed** — the Frame Server exists only on Windows.
* Why it is preferred: one registration call, the camera is a first-class
  Windows device, and Media Foundation consumers (including the DirectShow
  bridge) see it without a custom filter in the graph.

### Option B — `dshow_filter` (DirectShow capture filter)

A project-owned live source filter with one NV12 output pin, registered under
the "Video Capture Sources" category, with separate 32-bit and 64-bit CLSIDs.

* Located in `spikes/f0-dshow-vcam-fallback/`.
* Status: **written but never compiled** — no MSVC toolchain and no Windows SDK
  on the F0 host. `README.md` lists exactly what is unfinished.
* Why it remains the fallback: a 32-bit consumer on 64-bit Windows reads the
  `WOW6432Node` view and can only load a 32-bit DLL, and some conferencing
  clients enumerate DirectShow devices only. A DirectShow filter addresses both
  by construction; the Media Foundation path addresses the first only if a
  32-bit media source is also built and registered.

### Option C — a kernel-mode camera driver

Rejected before implementation. A signed kernel driver is an install-time
administrator action, contradicts the no-admin promise the gate is testing, and
cannot be delivered by a per-user installer. Recorded here so the option is
visibly considered rather than silently dropped.

## Selection rule (already encoded, already tested)

`spikes/tools/analysis/injection.py::derive_selected_backend` implements the
rule the contract states:

1. `mf_virtual_camera` is selected when **every** one of its cells is
   `frames_delivered`;
2. otherwise `dshow_filter` is selected when every one of *its* cells is
   `frames_delivered`;
3. otherwise `build_matrix` raises: the schema has no `null` `selected_backend`,
   so "no backend passed" cannot be written down as a decision.

`run_matrix.py` evaluates the fallback only when a primary cell fails, and then
for **every** application, so the two backends are compared like with like.

## Evidence that exists today

| Evidence | Where | What it proves |
| --- | --- | --- |
| 12 ring-buffer tests | `spikes/f0-injection/tests/ring_buffer.rs` | drop-oldest under backpressure, multi-reader cursors, monotonic publish, discard counting, shutdown release |
| 3 marker tests | `spikes/f0-injection/src/test_pattern.rs` | the per-frame identifier survives at both announced sizes |
| 16 continuity tests | `spikes/tools/tests/test_injection_continuity.py` | the 2 000 ms stall boundary, duplicates, size/rate mismatch, `observed_duration_s ≥ 1800` |
| 9 matrix tests | `spikes/f0-camera-injection/tests/test_matrix_schema.py` | full coverage per evaluated backend, a reason on every `not_tested` cell, a derivable decision |
| 15 marker-decoder tests | `spikes/tools/tests/test_marker.py` | the Python decoder agrees with the Rust producer |
| Windows typecheck | `cargo clippy -p remotecam-injection --target x86_64-pc-windows-msvc -D warnings` | the media source, the registration and the driver example are free of compile and lint errors for the target |
| Google Chrome partial run | `spikes/evidence/injection/runs/mf_virtual_camera/chrome/` | the media source was visible to one 64-bit consumer (Chrome 124) for 33.2845 s at 1280×720@30; not a gate cell |

None of that is gate evidence. It is the apparatus the gate will use, and it is
tested so that the 30-minute run measures the camera rather than the harness.

## Evidence that is missing

| Missing | Why | Consequence |
| --- | --- | --- |
| `spikes/evidence/injection/injection_matrix.json` | requires the remaining five application cells and 30-minute runs on the reference machine; the Chrome cell is partial evidence only | the backend cannot be *selected*, only recommended |
| Per-application cells for both bitnesses | requires Chrome, a 32-bit Chromium build, the conferencing client, OBS, Windows Camera, the screen recorder | the coverage rule cannot be satisfied |
| The multi-consumer observation | requires two consumers opening the camera at once | `multi_consumer_observation.outcome` stays `not_tested` |
| Q-1 answer | requires a real `HKCU` write to be accepted or refused | the installer decision stays open |
| Q-6 answer | follows from Q-1 | installer format (`none` / `msi` / `msix`) stays open |
| Q-8 answer | requires a 32-bit consumer | 32-bit support stays unproven |

## Licensing impact

| Backend | Licence | Distribution consequence |
| --- | --- | --- |
| `mf_virtual_camera` | MIT (`applications.json` records `license_file_present: true`) | the spike borrows the API shape from the Microsoft Windows-Camera virtual camera sample; the product ships its own implementation and no third-party binary |
| `dshow_filter` | `proprietary` — project-owned | nothing is redistributed, but the DirectShow **base classes** (`streams.h`, `strmbase.lib`) come from the Windows SDK samples and are **not** redistributable as part of the product. `CMakeLists.txt` links them from a local samples directory rather than vendoring them, and the fallback must either keep doing that or be rewritten against the raw COM interfaces before it can ship. This is a real cost of choosing Option B and is recorded here rather than discovered later |

Choosing Option A therefore has **no new licensing obligation**; choosing
Option B adds a build-time dependency on SDK samples that a commercial product
must not redistribute.

## Media-phase consequences

The selected backend fixes the frame handoff contract:

* the injection crate consumes decoded **NV12** frames and never negotiates a
  codec, so the codec decision (D-codec-baseline) is independent of this one;
* the ring buffer keeps **1280×720@30** as the default and **1920×1080@30** as
  the optional maximum, with drop-oldest under backpressure and the discard
  count exported for the resource measurements;
* the media source announces 1280×720@30 and 640×480@30 — a consumer that
  requires a type outside those two falls back, and any such consumer is a
  matrix finding, not a silent downgrade;
* the ring buffer lives in the producing process while the media source is
  hosted out of process, so the spike feeds the source from the committed test
  pattern. **Cross-process frame transport is the media phase's problem** and is
  not claimed here.

### 720p / 1080p capability statement

| Format | Claimed status | Basis |
| --- | --- | --- |
| NV12 1280×720@30 | **implemented, not measured** | the media source announces it and the ring buffer is sized for it; no consumer has been observed receiving it |
| NV12 640×480@30 | **implemented, not measured** | announced as the second type for consumers that prefer it |
| NV12 1920×1080@30 | **not implemented in this spike** | the 1080p maximum is a media-phase target; it needs an H.264 level ≥ 4.0 decision (Q-7) and is out of scope for gate 1 |

No frame rate above 30 fps is claimed at any size.

## How this record becomes `accepted`

1. On the Windows 11 reference machine, non-administrator account, run
   `pwsh spikes/f0-camera-injection/register.ps1` and keep
   `spikes/evidence/injection/registration.json` (answers Q-1, Q-6, Q-8).
2. Fill `product_name`, `version` and `harness_args` in
   `spikes/f0-camera-injection/applications.json` for all six applications.
3. Run `make spike-injection` (30 minutes per cell, six applications, both
   bitnesses represented) — it evaluates the fallback only if a primary cell
   fails.
4. Record the multi-consumer observation as
   `spikes/evidence/injection/runs/multi_consumer.json`.
5. Assemble and validate:
   `python spikes/f0-camera-injection/run_matrix.py --captures-root spikes/evidence/injection/runs`.
6. Set `status: accepted`, fill `approver` and `decided_at`, and reference the
   matrix. If a cell of the selected backend is not `frames_delivered`, this
   record must be rewritten as the fallback decision — not patched.

Until step 6, the honest statement is: **Media Foundation is the recommended
primary backend; the DirectShow filter is the recommended fallback. Only Google
Chrome has been observed receiving frames, for 33.3 s; no backend has completed
the matrix.**

## Reusable findings

* Per-user COM registration and the camera instance are two separate questions;
  a harness that answers only one of them produces a misleading Q-1 answer.
* A 32-bit consumer needs a 32-bit media source *and* a `WOW6432Node`
  registration; registering only the 64-bit view is a matrix failure, not a
  limitation.
* A marker duplicated in two corners lets a cropped or scaled capture be
  rejected instead of decoded as frame 0.
* Continuity must be judged on the consumer's own timeline. Re-deriving
  timestamps from a sampled frame index hides exactly the stall the gate is
  looking for, which is why the browser harness times frames with
  `requestVideoFrameCallback` and the file decoder reads real presentation times.
* A screen recorder cannot expose per-frame delivery; OBS's own
  `outputSkippedFrames` counter is the honest substitute and must be recorded as
  such.
