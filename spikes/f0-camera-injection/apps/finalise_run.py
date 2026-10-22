#!/usr/bin/env python3
"""Finalise one or more harness runs into gate evidence.

**NON-PRODUCTION** — F0 risk-reduction spike.

A harness records a capture and `capture.json`; this tool turns that into
`observations.csv` and `run.json`, which is what `run_matrix.py --captures-root`
consumes. It exists so a run can be decoded again after the fact — for example
on a machine that has FFmpeg but not the camera — without re-running anything.

Usage::

    python spikes/f0-camera-injection/apps/finalise_run.py \\
        --backend mf_virtual_camera spikes/evidence/injection/runs/mf_virtual_camera/chrome

    python spikes/f0-camera-injection/apps/finalise_run.py --backend dshow_filter \\
        spikes/evidence/injection/runs/dshow_filter/*

Exits non-zero when a directory cannot be finalised, with the reason on stderr.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from harness_common import BACKENDS, HarnessError, finalise_run  # noqa: E402


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--backend", required=True, choices=BACKENDS)
    parser.add_argument("directories", nargs="+", type=Path)
    args = parser.parse_args(argv)

    failures = 0
    for directory in args.directories:
        try:
            run_path = finalise_run(directory, backend=args.backend)
        except (HarnessError, ValueError) as error:
            print(f"{directory}: {error}", file=sys.stderr)
            failures += 1
            continue
        print(f"{directory}: wrote {run_path.name}")

    if failures:
        print(
            f"{failures} of {len(args.directories)} directories could not be finalised",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
