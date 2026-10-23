"""Harness-floor calibration (T077).

The screen-capture path adds its own latency (compositor + gdigrab + encode),
typically 17-50 ms -- the same order as the whole latency budget. The floor is
measured by rendering the **same** QR marker in a local HTML page and capturing
it with the **identical** FFmpeg command used for the measurement run.

The published floor is the p95 of the calibration latencies: a conservative
single number that cannot be selected after the fact. It is recorded in
``toolchain_selftest.json`` and in every ``latency_summary.json``.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Final

from analysis.latency import LatencySample, read_latency_csv
from metrics.criteria import percentile_set

FLOOR_METHOD: Final = "local-html-page captured with the identical ffmpeg gdigrab command"
FLOOR_EXPECTED_RANGE_MS: Final = (17.0, 50.0)


def measure_floor(samples: Sequence[LatencySample]) -> dict:
    """Build the floor document from a calibration capture's samples."""

    if len(samples) < 10:
        raise ValueError(f"the floor needs at least 10 samples, got {len(samples)}")
    latencies = [sample.latency_ms for sample in samples]
    percentiles = percentile_set(latencies, suffix="_ms")
    outside_range = not (
        FLOOR_EXPECTED_RANGE_MS[0] <= percentiles["p95_ms"] <= FLOOR_EXPECTED_RANGE_MS[1]
    )
    return {
        "harness_floor_ms": percentiles["p95_ms"],
        "sample_count": len(samples),
        "method": FLOOR_METHOD,
        "percentiles": percentiles,
        "expected_range_ms": list(FLOOR_EXPECTED_RANGE_MS),
        "outside_expected_range": outside_range,
        "note": (
            "outside the expected 17-50 ms range: verify the capture region, the display "
            "refresh rate and that the browser is not throttled before using this floor"
            if outside_range
            else "within the expected range for the reference machine"
        ),
    }


def write_floor(path: Path | str, document: dict) -> None:
    Path(path).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


def read_floor(path: Path | str) -> float:
    payload = json.loads(Path(path).read_text(encoding="utf-8"))
    return float(payload["harness_floor_ms"])


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", required=True, help="calibration latency_samples.csv")
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)

    samples = read_latency_csv(args.samples)
    document = measure_floor(samples)
    write_floor(args.out, document)
    print(json.dumps(document, indent=2))
    return 0 if not document["outside_expected_range"] else 1


__all__ = [
    "FLOOR_EXPECTED_RANGE_MS",
    "FLOOR_METHOD",
    "main",
    "measure_floor",
    "read_floor",
    "write_floor",
]


if __name__ == "__main__":
    sys.exit(main())
