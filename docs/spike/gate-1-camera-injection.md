# Gate 1 — Virtual camera injection

**Gate id**: `virtual_camera_injection` · **Phase**: C · **Budget**: ≈ 90 minutes
of operator time plus 6 × 30 minutes of unattended capture ·
**User story**: US1 (P1) · **Decision record**: [`D-10`](../../spikes/evidence/decisions/D-10-injection-backend.md)

## 1. What this gate decides

Whether RemoteCam can present its decoded picture to the target consumer
applications as a camera they can select, and which backend does it.

The gate answers three open questions:

| Question | Answered by |
| --- | --- |
| Q-1 — does per-user (`HKCU`) registration work without elevation, and is an MSI therefore required? | `register.ps1` → `registration.json` |
| Q-6 — can two consumers open the camera at once, and what does each observe? | the multi-consumer scenario, §6 |
| Q-8 — can a 32-bit consumer see the camera on 64-bit Windows? | the `chrome32` cell, §5 |

Q-7 (720p vs 1080p) is **not** decided here; see D-10's capability statement.

## 2. Preconditions

| # | Precondition | How it is checked |
| --- | --- | --- |
| PC-1 | Windows 11 x64 reference machine, build recorded in `spikes/evidence/environment.json` | `environment_id` is written into the matrix |
| PC-2 | A **non-administrator** account | `register.ps1` records the elevation state; an elevated run does not answer Q-1 |
| PC-3 | Rust toolchain with the `x86_64-pc-windows-msvc` (and `i686-pc-windows-msvc` for the 32-bit consumer) targets | `rustup target list --installed` |
| PC-4 | Node.js 20+ (browser harness), FFmpeg on `PATH` (window capture and decoding), OBS Studio with obs-websocket 5.7.x | each harness fails loudly when its tool is missing |
| PC-5 | The six applications of §4 installed, each with the exact version recorded | the plan refuses to start without `product_name`/`version` |
| PC-6 | No other process is using the camera | a busy device surfaces as `device_not_visible`, which would be a false failure |

**This host cannot run the gate.** The F0 development host is Linux/WSL2: no
Media Foundation, no Frame Server, no FFmpeg, no OBS and no camera, and
`spikes/evidence/environment.json` records `hardware_accelerated: false` and
`within_reference_conditions: false`. Everything below is the procedure to be
executed on the reference machine; nothing in this document may be filled in
from anywhere else.

## 3. What "pass" means

`spikes/evidence/injection/injection_matrix.json` validates against
[`contracts/injection-matrix.schema.json`](../../specs/001-f0-technical-spikes/contracts/injection-matrix.schema.json)
and, for **every** cell of the selected backend:

| Criterion | Rule |
| --- | --- |
| `outcome` | `frames_delivered` |
| `observed_duration_s` | ≥ 1800 |
| `delivered_size_ok` | `true` (1280×720 delivered) |
| `delivered_rate_ok` | `true` (30 fps ± 10 %) |
| `longest_stall_ms` | ≤ 2000 |

Plus, for the matrix as a whole:

* all five application categories present, and **both bitnesses** represented;
* a 32-bit consumer present **and** seeing the device — a 32-bit consumer that
  cannot see it is a **matrix failure**, not a limitation;
* `selected_backend` and `decision_rationale` recorded, with
  `licensing_impact` and `media_phase_consequences`;
* `multi_consumer_observation` recording what two simultaneous consumers saw.

A stall boundary note: the schema caps `longest_stall_ms` at 2000, so a gap of
exactly 2000 ms passes and anything above it fails. `tasks.md` T028 words the
boundary loosely; the schema wins and the tests assert 2000 passes / 2000.001
fails.

## 4. Minimum matrix

| `application_id` | Category | Bitness | Harness | Capture method |
| --- | --- | --- | --- | --- |
| `chrome` | `chromium_browser` | 64-bit | `apps/browser/capture.mjs` | `getUserMedia` + `MediaRecorder` |
| `chrome32` | `chromium_browser` | **32-bit** | `apps/browser32/capture.mjs` | `getUserMedia` + `MediaRecorder` |
| `conferencing` | `video_conferencing_client` | 64-bit | `apps/window_capture.py` | `gdigrab` window capture |
| `obs` | `obs` | 64-bit | `apps/obs/capture.py` | obs-websocket recording |
| `windows_camera` | `windows_camera_app` | 64-bit | `apps/window_capture.py` | `gdigrab` window capture |
| `screen_recorder` | `screen_recorder` | 64-bit | `apps/window_capture.py` | the application's own recording |

The plan in `spikes/f0-camera-injection/applications.json` is authoritative; the
table is its readable form.

## 5. Procedure

### 5.1 Fill in the plan (once per run)

In `spikes/f0-camera-injection/applications.json`:

* `product_name` and `version` for **all six** applications — `run_matrix.py`
  refuses to start while any is empty, because the exact build is gate evidence
  and cannot be recovered afterwards;
* `harness_args` for the three window-capture applications: at minimum
  `--window-title "<window title>"`, plus `--exe <path>` so the file version is
  read from the binary;
* for `screen_recorder`, `--capture-file <path to the recording the application
  produced>`.

The angle-bracket placeholders are deliberate: a run that still contains them
fails loudly instead of recording a cell against the wrong product.

### 5.2 Register the camera and answer Q-1 (≈ 5 min)

```powershell
pwsh spikes/f0-camera-injection/register.ps1
```

The script registers the in-process COM server per-user, starts the camera,
holds it while enumerating what a consumer sees, unregisters, and verifies both
registry views are clean. It **never elevates**: a refusal is the Q-1 answer.

* Q-1 is `per_user` when `registration.json` has `per_user_accepted: true`;
* the `HKLM` fallback is measured only with `-AllowMachineWide` from an elevated
  shell, and is recorded separately because it answers a different question;
* Q-6 (`installer_required`) follows: `none` when per-user works, otherwise the
  installer must be able to write `HKLM`.

Keep `spikes/evidence/injection/registration.json`.

### 5.3 Run the matrix (≈ 3 h unattended)

```powershell
make spike-injection          # python spikes/f0-camera-injection/run_matrix.py
```

For each application the runner invokes the harness with the plan's exact
product identity, then finalises the run: it decodes the capture (or accepts the
harness's own frame observation), writes `observations.csv` and `run.json`, and
judges the cell with `spikes/tools/analysis/injection.py`.

Operator actions during the run:

* **`chrome`** — the page opens in the default browser; select *RemoteCam
  Virtual Camera (Spike)* in the permission prompt and press **Start the run**;
* **`chrome32`** — the runner prints a URL. Open it **in the 32-bit Chromium
  build** (confirm the bitness on `chrome://version` first). The harness
  deliberately does not open a browser, because the default one is 64-bit and
  the cell would silently test the wrong bitness;
* **`conferencing` / `windows_camera`** — bring the application to the front and
  select the virtual camera in its preview before the capture window starts;
* **`obs`** — OBS must be running with obs-websocket enabled; set
  `OBS_WEBSOCKET_PASSWORD` if authentication is on. The harness verifies OBS
  lists the device *and* that the picture changes before recording;
* **`screen_recorder`** — record with the application itself, then let the
  runner pick up the file named in the plan.

### 5.4 The DirectShow fallback

Evaluated **only if at least one primary cell fails**, and then for every
application. Build and register it first:

```powershell
cmake -S spikes/f0-dshow-vcam-fallback -B build/dshow-vcam-x64 -A x64 -DREMOTECAM_BASE_CLASSES=<sdk samples>\multimedia\directshow\baseclasses
cmake --build build/dshow-vcam-x64 --config Release
regsvr32 /n /i:user build\dshow-vcam-x64\Release\dshow_vcam_fallback.dll
```

Then re-run §5.3 with the fallback backend. The matrix must contain a cell for
every application under `dshow_filter`, even where the primary backend passed,
so the two are compared like with like.

### 5.5 Assemble (≈ 5 min)

```powershell
python spikes/f0-camera-injection/run_matrix.py --captures-root spikes/evidence/injection/runs
```

The runner validates the matrix against the contract before writing it, and
refuses to write one that does not satisfy it. A run that produced no capture is
an error, never an empty cell.

## 6. Multi-consumer scenario (Q-6)

With the camera registered, open **two** consumers simultaneously — the
`chrome` page and the Windows Camera app is the cheapest pair — and record:

```json
{
  "tested": true,
  "outcome": "both_worked",
  "notes": "Chrome 142 and Windows Camera 2026.0 both showed the moving marker for 5 minutes; neither froze when the other opened."
}
```

Save it as `spikes/evidence/injection/runs/multi_consumer.json`; §5.5 picks it
up. `outcome` is one of `both_worked`, `one_failed`, `undefined`. If the
scenario was not run, it must say `not_tested` — the schema accepts that, but the
gate report must then state that Q-6 is unanswered.

## 7. Recording the answers

| Answer | Where it goes | What it must say |
| --- | --- | --- |
| Q-1 | `registration.json` → `q1_answer` | whether `HKCU` was accepted without elevation |
| Q-6 | `registration.json` → `q6_answer`, and `multi_consumer_observation` | installer requirement, and what two consumers observed |
| Q-8 | `registration.json` → `q8_answer`, plus the `chrome32` cell | whether a 32-bit consumer saw the device, for the selected backend |

Then update [`D-10`](../../spikes/evidence/decisions/D-10-injection-backend.md)
to `accepted` with the approver, the date and the matrix as evidence — or rewrite
it as the fallback decision if the primary backend failed a cell.

## 8. Artefacts produced

| Artefact | Content |
| --- | --- |
| `spikes/evidence/injection/injection_matrix.json` | the validated matrix: every cell, the decision, licensing and media-phase consequences |
| `spikes/evidence/injection/registration.json` | the Q-1/Q-6/Q-8 answers and the registry outcome per bitness |
| `spikes/evidence/injection/runs/<backend>/<application_id>/` | `capture.json`, the capture file, `observations.csv`, `run.json` |
| `spikes/evidence/injection/runs/multi_consumer.json` | the Q-6 observation |

Captures are large; keep them locally and commit the JSON/CSV evidence, as
`contracts/measurement-formats.md` §10 prescribes.

## 9. Honest limitations of this spike

These are properties of the apparatus, not of the product, and they belong in
the gate report:

1. **The media source self-feeds the committed test pattern.** The Frame Server
   hosts the source out of process, so it cannot read the application's ring
   buffer. Gate 1 therefore proves *delivery to consumers*, not cross-process
   frame transport; that belongs to the media phase.
2. **A screen recorder cannot expose per-frame delivery.** For OBS the honest
   evidence is the recording plus OBS's own `outputSkippedFrames` counter, which
   is recorded in `run.json` as `observed`.
3. **A consumer that rescales the picture cannot be marker-decoded.** The marker
   is drawn at 1280×720; a capture at another size yields unreadable markers and
   is reported as a size mismatch rather than being silently accepted.
4. **`longest_stall_ms` is measured on the consumer's timeline.** For consumers
   whose recording repeats the last frame with advancing timestamps, a stall
   appears as duplicates rather than as a gap; the injected-stall behaviour is
   therefore also covered by the ring-buffer tests and the `test_pattern`
   example, which can inject a stall on demand.
5. **The DirectShow fallback has never been compiled** on any host available to
   this spike.

## 10. Reproducing a second time

A second engineer reproduces the gate with: the same Windows 11 build, the same
six application versions recorded in the plan, the same FFmpeg and Node
versions, and the two commands in §5.2 and §5.3. The matrix is written with the
`environment_id` of the machine that produced it, so a matrix from another
machine is visibly a different matrix rather than an update.
