"""Coverage gate for delivered packages and the product harness.

Enforces ≥ 90 % line coverage for ``desktop/crates/protocol``,
``desktop/crates/session-core``, ``desktop/crates/pairing`` and
``desktop/crates/media-core`` (via ``cargo-llvm-cov``), the product harness (via ``cargo-llvm-cov``), and
``mobile/protocol`` and ``mobile/session-core`` (via JaCoCo). ``spikes/`` is exempt as a recorded deviation
(T117); the harness has its own REC-25 gate despite not being shipped.

Usage::

    python scripts/ci/coverage_gate.py \
        --rust-summary <cargo-llvm-cov --summary-only output for remotecam-protocol> \
        --rust-session-core-summary <cargo-llvm-cov --summary-only output for remotecam-session-core> \
        --rust-pairing-summary <cargo-llvm-cov --summary-only output for remotecam-pairing> \
        --rust-media-core-summary <cargo-llvm-cov --summary-only output for remotecam-media-core> \
        --rust-harness-summary <cargo-llvm-cov --summary-only output for remotecam-harness> \
        --jacoco-report <jacocoTestReport.xml for mobile/protocol> \
        --jacoco-session-core-report <jacocoTestReport.xml for mobile/session-core>

Exit status is 1 when any supplied package or the harness is below 90 %.
"""

from __future__ import annotations

import argparse
import re
import sys
import xml.etree.ElementTree as ElementTree

THRESHOLD = 90.0


def rust_coverage_from_summary(text: str) -> float | None:
    """Parse ``cargo llvm-cov --summary-only`` output for the TOTAL line.

    The summary row carries three percentages (regions, functions, lines);
    the gate is judged on **line** coverage, so the third value is returned.
    """
    for line in text.splitlines():
        if not line.lstrip().startswith("TOTAL"):
            continue
        percentages = re.findall(r"([0-9.]+)%", line)
        if len(percentages) >= 3:
            return float(percentages[2])
    return None


def jacoco_coverage(report_path: str) -> float | None:
    """Parse JaCoCo XML report for LINE coverage."""
    try:
        root = ElementTree.parse(report_path).getroot()
    except (OSError, ElementTree.ParseError):
        return None
    for counter in root.findall("counter"):
        if counter.get("type") != "LINE":
            continue
        covered = int(counter.get("covered", "0"))
        missed = int(counter.get("missed", "0"))
        total = covered + missed
        return 100.0 * covered / total if total else 0.0
    return 0.0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Enforce ≥ 90 % coverage for the delivered product packages."
    )
    parser.add_argument(
        "--rust-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-protocol",
    )
    parser.add_argument(
        "--rust-session-core-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-session-core",
    )
    parser.add_argument(
        "--rust-pairing-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-pairing",
    )
    parser.add_argument(
        "--rust-media-core-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-media-core",
    )
    parser.add_argument(
        "--rust-harness-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-harness",
    )
    parser.add_argument(
        "--jacoco-report",
        help="Path to JaCoCo XML report for mobile/protocol",
    )
    parser.add_argument(
        "--jacoco-session-core-report",
        help="Path to JaCoCo XML report for mobile/session-core",
    )
    args = parser.parse_args()

    failures = []

    rust_summaries = [
        ("Rust protocol", args.rust_summary),
        ("Rust session core", args.rust_session_core_summary),
        ("Rust pairing", args.rust_pairing_summary),
        ("Rust media core", args.rust_media_core_summary),
        ("Product harness", args.rust_harness_summary),
    ]
    for label, summary_path in rust_summaries:
        if not summary_path:
            continue
        with open(summary_path, "r", encoding="utf-8") as fh:
            rust_pct = rust_coverage_from_summary(fh.read())
        if rust_pct is None:
            print(f"error: could not parse {label} coverage from summary")
            return 1
        print(f"{label} coverage: {rust_pct:.2f}%")
        if rust_pct < THRESHOLD:
            failures.append(
                f"{label} coverage {rust_pct:.2f}% below {THRESHOLD}% threshold"
            )

    jacoco_reports = [
        ("Kotlin protocol", args.jacoco_report),
        ("Kotlin session core", args.jacoco_session_core_report),
    ]
    for label, report_path in jacoco_reports:
        if not report_path:
            print(f"error: missing {label} JaCoCo report")
            return 1
        jacoco_pct = jacoco_coverage(report_path)
        if jacoco_pct is None:
            print(f"error: could not parse {label} coverage report")
            return 1
        print(f"{label} coverage: {jacoco_pct:.2f}%")
        if jacoco_pct < THRESHOLD:
            failures.append(
                f"{label} coverage {jacoco_pct:.2f}% below {THRESHOLD}% threshold"
            )

    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    print("Coverage gate PASSED: all supplied packages at or above 90%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
