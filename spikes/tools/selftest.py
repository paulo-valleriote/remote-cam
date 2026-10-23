#!/usr/bin/env python3
"""Toolchain self-test: prove the apparatus measures what it claims.

Produces ``spikes/evidence/toolchain_selftest.json`` with exactly four checks
(``contracts/measurement-formats.md`` §11):

1. ``silence_fixture`` -- the committed calibration WAV yields exactly two
   dropouts (nominally 60 ms and 120 ms, within a 10 ms window tolerance);
   the 40 ms gap is below threshold.
2. ``qr_roundtrip`` -- a golden-vector payload rendered as a QR decodes back
   byte-identically and the SHA-256 values match.
3. ``sampler_agreement`` -- two independent CPU counter sources agree within one
   percentage point after machine normalisation. On the Windows reference
   machine the second source is ``typeperf``; elsewhere the note records the
   substitute so the reference check is not silently claimed.
4. ``harness_floor`` -- the apparatus's own latency floor is measured and
   recorded with its basis.

If any check fails, ``all_passed`` is false and **no measurement result is
valid** until the toolchain is fixed and the self-test re-run.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import sys
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import numpy as np
import qrcode
import soundfile as sf
import zxingcpp

_TOOLS_DIR = Path(__file__).resolve().parent
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))

from analysis.dropouts import detect_dropouts  # noqa: E402
from analysis.sampler import measure_sampler_agreement  # noqa: E402
from metrics.criteria import percentile_set  # noqa: E402
from redaction import redact  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = REPO_ROOT / "spikes" / "evidence" / "toolchain_selftest.json"
FIXTURE = REPO_ROOT / "spikes" / "fixtures" / "calibration-40-60-120.wav"
QR_VECTOR = REPO_ROOT / "protocol" / "vectors" / "qr" / "minimal-v1.json"


def _check(
    name: str, expected: dict[str, Any], observed: dict[str, Any], passed: bool
) -> dict[str, Any]:
    return {
        "name": name,
        "expected": expected,
        "observed": observed,
        "passed": bool(passed),
    }


def check_silence_fixture() -> dict[str, Any]:
    """The 40/60/120 ms fixture must yield two dropouts within a 10 ms tolerance."""

    try:
        samples, sample_rate = sf.read(FIXTURE, dtype="float64")
        result = detect_dropouts(
            samples,
            sample_rate,
            threshold_dbfs=-50.0,
            min_dropout_ms=50.0,
            window_ms=5.0,
            hold_ms=10.0,
        )
    except (OSError, RuntimeError, ValueError) as error:
        return _check(
            "silence_fixture",
            {"dropout_count": 2, "dropout_durations_ms": [60, 120]},
            {"error": str(error)},
            False,
        )

    durations = [round(interval.duration_ms) for interval in result.dropouts]
    short = [round(interval.duration_ms) for interval in result.below_threshold]
    passed = (
        result.dropout_count == 2
        and len(durations) == 2
        and abs(durations[0] - 60) <= 10
        and abs(durations[1] - 120) <= 10
        and 40 in short
    )
    return _check(
        "silence_fixture",
        {
            "dropout_count": 2,
            "dropout_durations_ms": [60, 120],
            "tolerance_ms": 10,
            "below_threshold_durations_ms": [40],
        },
        {
            "dropout_count": result.dropout_count,
            "dropout_durations_ms": durations,
            "below_threshold_durations_ms": short,
            "window_ms": result.window_ms,
            "threshold_dbfs": result.threshold_dbfs,
        },
        passed,
    )


def _golden_payload() -> bytes:
    vector = json.loads(QR_VECTOR.read_text(encoding="utf-8"))
    return bytes.fromhex(vector["expected"]["payload_hex"])


def _render_qr(payload: bytes) -> tuple[np.ndarray, int]:
    symbol = qrcode.QRCode(
        error_correction=qrcode.constants.ERROR_CORRECT_M,
        box_size=8,
        border=4,
    )
    symbol.add_data(payload, optimize=0)
    symbol.make(fit=True)
    image = symbol.make_image(fill_color="black", back_color="white").convert("RGB")
    return np.asarray(image), symbol.version


def check_qr_roundtrip() -> dict[str, Any]:
    """Render a golden payload and require a byte-identical raw-byte decode."""

    try:
        payload = _golden_payload()
        image, version = _render_qr(payload)
        decoded = zxingcpp.read_barcode(image)
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        return _check(
            "qr_roundtrip",
            {"decoded_payload_sha256": "<unavailable>"},
            {"error": str(error)},
            False,
        )

    expected_sha = hashlib.sha256(payload).hexdigest()
    observed_bytes = decoded.bytes if decoded is not None else b""
    observed_sha = hashlib.sha256(observed_bytes).hexdigest()
    passed = decoded is not None and observed_bytes == payload
    return _check(
        "qr_roundtrip",
        {"decoded_payload_sha256": expected_sha, "payload_bytes": len(payload)},
        {
            "decoded_payload_sha256": observed_sha,
            "payload_bytes": len(observed_bytes),
            "qr_version": version,
            "error_correction": "M",
            "decoded_as": "raw bytes" if decoded is not None else None,
        },
        passed,
    )


def check_sampler_agreement() -> dict[str, Any]:
    """Cross-check the machine-normalised CPU formula against a second source."""

    agreement = measure_sampler_agreement()
    return _check(
        "sampler_agreement",
        {"delta_p95_cpu_pct": "<= 1.0"},
        agreement.as_observed(),
        agreement.passed,
    )


def _measure_software_floor(iterations: int = 21) -> tuple[float, dict[str, Any]]:
    payload = _golden_payload()
    timings: list[float] = []
    for _ in range(iterations):
        started = time.perf_counter()
        image, _version = _render_qr(payload)
        decoded = zxingcpp.read_barcode(image)
        if decoded is None or decoded.bytes != payload:
            raise RuntimeError("the floor measurement's QR did not decode")
        timings.append((time.perf_counter() - started) * 1000.0)
    return percentile_set(timings, suffix="_ms")["p95_ms"], {"samples": iterations}


def check_harness_floor() -> dict[str, Any]:
    """Measure the apparatus floor and record what was actually measured.

    On the reference machine the floor comes from rendering the identical QR in a
    local page and capturing it with the identical consumer-capture command. On a
    host without that apparatus the check measures the local render/decode path
    and marks ``reference_measurement_required`` so the value cannot be mistaken
    for the gate floor. An operator may supply the real measurement through
    ``REMOTECAM_HARNESS_FLOOR_MS``.
    """

    override = os.environ.get("REMOTECAM_HARNESS_FLOOR_MS")
    if override:
        try:
            floor_ms = float(override)
        except ValueError:
            return _check(
                "harness_floor",
                {"floor_ms": "measured"},
                {"error": "REMOTECAM_HARNESS_FLOOR_MS is not a number"},
                False,
            )
        observed = {
            "floor_ms": round(floor_ms, 2),
            "method": "operator-recorded capture floor",
            "basis": "operator-recorded",
            "within_expected_range": 17.0 <= floor_ms <= 50.0,
            "reference_measurement_required": False,
        }
        return _check("harness_floor", {"floor_ms": "measured"}, observed, floor_ms > 0)

    try:
        floor_ms, detail = _measure_software_floor()
    except (OSError, RuntimeError, ValueError) as error:
        return _check(
            "harness_floor",
            {"floor_ms": "measured"},
            {"error": str(error)},
            False,
        )

    within = 17.0 <= floor_ms <= 50.0
    observed = {
        "floor_ms": round(floor_ms, 2),
        "method": "software QR render and decode",
        "basis": "local-software-only",
        "within_expected_range": within,
        "reference_measurement_required": platform.system() != "Windows",
        "note": (
            "This host has no display-capture apparatus. The value measures the QR render/decode "
            "path only and MUST be replaced by the documented capture floor on the reference "
            "Windows machine before any latency-gate result is cited."
            if platform.system() != "Windows"
            else (
                "Rendered and decoded locally; confirm the capture floor with the consumer command."
            )
        ),
        **detail,
    }
    return _check("harness_floor", {"floor_ms": "measured"}, observed, floor_ms > 0)


def run_selftest() -> dict[str, Any]:
    checks = [
        check_silence_fixture(),
        check_qr_roundtrip(),
        check_sampler_agreement(),
        check_harness_floor(),
    ]
    document = {
        "run_at": datetime.now(UTC).isoformat(timespec="seconds"),
        "checks": checks,
        "all_passed": all(check["passed"] for check in checks),
    }
    return redact(document)


def _write(document: dict[str, Any], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    temporary.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run the F0 measurement toolchain self-test.")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--json", action="store_true", help="print the self-test document")
    args = parser.parse_args(argv)

    document = run_selftest()
    _write(document, args.output)

    if args.json:
        print(json.dumps(document, indent=2, sort_keys=True))
    else:
        for check in document["checks"]:
            status = "PASS" if check["passed"] else "FAIL"
            print(f"{status:4} {check['name']}")
            if not check["passed"]:
                print(f"     expected: {check['expected']}")
                print(f"     observed: {check['observed']}")
        print(f"\nall_passed: {document['all_passed']} -> {args.output}")

    return 0 if document["all_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
