"""Coverage gate for the F0 protocol packages.

Enforces ≥ 90 % line coverage for ``desktop/crates/protocol`` (via
``cargo-llvm-cov``) and ``mobile/protocol`` (via JaCoCo). ``spikes/`` is
exempt per the recorded deviation (T117).

Usage::

    python scripts/ci/coverage_gate.py \
        --rust-summary <cargo-llvm-cov --summary-only output> \
        --jacoco-report <jacocoTestReport.xml>

Exit status is 1 when either package is below 90 %.
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
        description="Enforce ≥ 90 % coverage for F0 protocol packages."
    )
    parser.add_argument(
        "--rust-summary",
        help="Text output of `cargo llvm-cov --summary-only` for remotecam-protocol",
    )
    parser.add_argument(
        "--jacoco-report",
        help="Path to JaCoCo XML report for mobile/protocol",
    )
    args = parser.parse_args()

    failures = []

    if args.rust_summary:
        with open(args.rust_summary, "r", encoding="utf-8") as fh:
            rust_pct = rust_coverage_from_summary(fh.read())
        if rust_pct is None:
            print("error: could not parse Rust protocol coverage from summary")
            return 1
        print(f"Rust protocol coverage: {rust_pct:.2f}%")
        if rust_pct < THRESHOLD:
            failures.append(
                f"Rust protocol coverage {rust_pct:.2f}% below {THRESHOLD}% threshold"
            )

    if args.jacoco_report:
        jacoco_pct = jacoco_coverage(args.jacoco_report)
        if jacoco_pct is None:
            print("error: could not parse JaCoCo protocol coverage report")
            return 1
        print(f"Android protocol coverage: {jacoco_pct:.2f}%")
        if jacoco_pct < THRESHOLD:
            failures.append(
                f"Android protocol coverage {jacoco_pct:.2f}% below {THRESHOLD}% threshold"
            )

    if failures:
        for f in failures:
            print(f, file=sys.stderr)
        return 1

    print("Coverage gate PASSED: both protocol packages at or above 90%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
