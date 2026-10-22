# F0 — virtual camera injection (gate 1)

**NON-PRODUCTION** — F0 risk-reduction spike. Nothing here merges into
`desktop/` or `mobile/` (FR-039).

Gate 1 asks whether every target consumer application can open RemoteCam as a
camera. The procedure is [`docs/spike/gate-1-camera-injection.md`](../../docs/spike/gate-1-camera-injection.md);
the decision record is
[`spikes/evidence/decisions/D-10-injection-backend.md`](../evidence/decisions/D-10-injection-backend.md).

## Layout

| Path | What it is |
| --- | --- |
| `applications.json` | The run plan: the six applications, their categories, bitnesses and harnesses |
| `run_matrix.py` | `make spike-injection`: runs the matrix, or assembles it offline from existing captures |
| `register.ps1` | Q-1/Q-6/Q-8 registration check; never elevates |
| `apps/harness_common.py` | The harness contract: identity, capture record, finalisation, decoding |
| `apps/browser/`, `apps/browser32/` | Chromium consumers (64-bit and **32-bit**); the page times frames with `requestVideoFrameCallback` |
| `apps/obs/` | OBS Studio through obs-websocket 5.7.x, with a stdlib WebSocket client |
| `apps/window_capture.py` | FFmpeg `gdigrab` window capture, or the application's own recording |
| `apps/finalise_run.py` | Turn a harness run into `observations.csv` + `run.json` |
| `tests/` | Matrix-contract, runner and OBS-client tests |

## The harness contract

A harness **records**, it never judges. Each one writes:

* the capture file itself (`capture.webm`, `capture.mp4`, `capture.mkv`, …);
* `capture.json` — the exact product and version, the requested format, the
  recorded duration and what the harness observed;
* optionally `observations.csv` — one row per frame the harness itself timed,
  used when the harness can measure presentation times directly;
* or `failure.json` — the consumer could not open the camera, which becomes a
  `device_not_visible` cell rather than silence.

`run_matrix.py` then calls `harness_common.finalise_run`, which decodes the
capture with FFmpeg (or accepts the harness's own CSV), writes
`observations.csv` and `run.json`, and refuses to continue when a capture is
missing, unreadable or empty. All gate judgement lives in
`spikes/tools/analysis/injection.py`.

Every harness is invoked with the same arguments:

```text
--backend <mf_virtual_camera|dshow_filter> --application-id <id> \
--duration-s <seconds> --output-dir <dir> \
--product-name <exact product> --version <exact version>
```

`--product-name`/`--version` come from the plan; a live run refuses to start
while either is empty, because the exact build is gate evidence and cannot be
recovered afterwards.

## Running it

```bash
make spike-injection                       # live: needs the Windows 11 machine
python spikes/f0-camera-injection/run_matrix.py --captures-root spikes/evidence/injection/runs
```

The live mode refuses to start off Windows. The offline mode assembles a matrix
from captures that already exist and validates it against
`contracts/injection-matrix.schema.json` before writing anything.

## Tests

```bash
cd spikes/tools && .venv/bin/python -m pytest tests ../f0-camera-injection
```

The runner's refusal rules are the ones that matter: no cell is concluded from
an absent capture, no product identity the plan did not authorise, and no matrix
that the contract rejects.
