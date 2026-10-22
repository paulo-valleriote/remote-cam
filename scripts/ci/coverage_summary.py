"""Publish a coverage percentage from a JaCoCo XML report."""

import sys
import xml.etree.ElementTree as ElementTree


def line_coverage_percent(report_path: str) -> float | None:
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
    if len(sys.argv) != 2:
        print("usage: coverage_summary.py <jacoco-report.xml>", file=sys.stderr)
        return 2

    percentage = line_coverage_percent(sys.argv[1])
    if percentage is None:
        print("android protocol line coverage: no report")
        return 0

    print(f"android protocol line coverage: {percentage:.2f}%")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
