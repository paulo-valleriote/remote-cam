#!/usr/bin/env python3
"""Compares the Rust and Kotlin conformance result documents.

Both implementations must emit the same observed outcome for every vector. A
divergence is a contract break and fails the make target.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load(path: str) -> dict:
    return json.loads(Path(path).read_text())


def index(items: list[dict]) -> dict[str, dict]:
    return {item["id"]: item for item in items}


def compare_section(name: str, left: list[dict], right: list[dict]) -> list[str]:
    failures: list[str] = []
    left_index = index(left)
    right_index = index(right)
    if set(left_index) != set(right_index):
        missing = sorted(set(left_index) - set(right_index))
        extra = sorted(set(right_index) - set(left_index))
        if missing:
            failures.append(f"{name}: Kotlin is missing {missing}")
        if extra:
            failures.append(f"{name}: Kotlin has unexpected {extra}")
    for vector_id in sorted(set(left_index) & set(right_index)):
        if left_index[vector_id] != right_index[vector_id]:
            failures.append(
                f"{name}/{vector_id}: Rust {left_index[vector_id]!r} != "
                f"Kotlin {right_index[vector_id]!r}"
            )
    return failures


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: compare_conformance.py <rust.json> <kotlin.json>", file=sys.stderr)
        return 2
    rust = load(sys.argv[1])
    kotlin = load(sys.argv[2])

    failures: list[str] = []
    failures.extend(compare_section("qr", rust.get("qr", []), kotlin.get("qr", [])))
    failures.extend(
        compare_section("signaling", rust.get("signaling", []), kotlin.get("signaling", []))
    )
    if rust.get("reason_codes") != kotlin.get("reason_codes"):
        failures.append(
            f"reason_codes: Rust {rust.get('reason_codes')!r} != "
            f"Kotlin {kotlin.get('reason_codes')!r}"
        )

    if failures:
        for failure in failures:
            print(f"conformance divergence: {failure}", file=sys.stderr)
        return 1
    print(
        "conformance: both implementations agree on "
        f"{len(rust.get('qr', []))} QR vectors and "
        f"{len(rust.get('signaling', []))} message scenarios"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
