#!/usr/bin/env python3
"""Compares the Rust and Kotlin conformance result documents.

Both implementations must emit the same observed outcome for every vector, for
the message vectors and for the handshake scenarios. A divergence is a contract
break and fails the make target. A vector that neither implementation reports
is also a failure: nothing may be skipped silently.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SECTIONS = ("qr", "signaling", "encoding")


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


def expected_ids(vectors: Path) -> dict[str, set[str]]:
    """Every id the pinned vector files declare, per result section."""
    scenarios = json.loads((vectors / "signaling/scenarios.json").read_text())[
        "scenarios"
    ]
    encoding = json.loads((vectors / "signaling/encoding.json").read_text())["vectors"]
    return {
        "qr": {path.stem for path in (vectors / "qr").glob("*.json")},
        "signaling": {item["id"] for item in scenarios if item["kind"] == "message"},
        "handshake": {item["id"] for item in scenarios if item["kind"] == "handshake"},
        "encoding": {item["id"] for item in encoding},
    }


def check_coverage(name: str, declared: set[str], reported: list[dict]) -> list[str]:
    missing = sorted(declared - {item["id"] for item in reported})
    if missing:
        return [f"{name}: vectors not executed by the implementations: {missing}"]
    return []


def declared_layers(vectors: Path) -> dict[str, str]:
    scenarios = json.loads((vectors / "signaling/scenarios.json").read_text())
    return {
        item["id"]: item["layer"]
        for item in scenarios["scenarios"]
        if item["kind"] == "handshake"
    }


def check_layers(layers: dict[str, str], reported: list[dict]) -> list[str]:
    """Only transport scenarios may go unexecuted, and the layer must be the declared one."""
    failures: list[str] = []
    for item in reported:
        declared = layers.get(item["id"])
        if item.get("layer") != declared:
            failures.append(
                f"handshake/{item['id']}: reported layer {item.get('layer')!r}, "
                f"declared {declared!r}"
            )
        if bool(item.get("executed")) != (declared != "transport"):
            failures.append(
                f"handshake/{item['id']}: executed={item.get('executed')!r} "
                f"is not allowed for layer {declared!r}"
            )
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("rust")
    parser.add_argument("kotlin")
    parser.add_argument("--rust-handshake", required=True)
    parser.add_argument("--kotlin-handshake", required=True)
    parser.add_argument("--vectors", required=True)
    arguments = parser.parse_args()

    rust = load(arguments.rust)
    kotlin = load(arguments.kotlin)
    rust_handshake = load(arguments.rust_handshake).get("handshake", [])
    kotlin_handshake = load(arguments.kotlin_handshake).get("handshake", [])
    declared = expected_ids(Path(arguments.vectors))

    failures: list[str] = []
    for section in SECTIONS:
        failures.extend(
            compare_section(section, rust.get(section, []), kotlin.get(section, []))
        )
        failures.extend(
            check_coverage(section, declared[section], rust.get(section, []))
        )
        failures.extend(
            check_coverage(section, declared[section], kotlin.get(section, []))
        )
    failures.extend(compare_section("handshake", rust_handshake, kotlin_handshake))
    failures.extend(check_coverage("handshake", declared["handshake"], rust_handshake))
    failures.extend(
        check_coverage("handshake", declared["handshake"], kotlin_handshake)
    )
    layers = declared_layers(Path(arguments.vectors))
    failures.extend(check_layers(layers, rust_handshake))
    failures.extend(check_layers(layers, kotlin_handshake))
    if rust.get("reason_codes") != kotlin.get("reason_codes"):
        failures.append(
            f"reason_codes: Rust {rust.get('reason_codes')!r} != "
            f"Kotlin {kotlin.get('reason_codes')!r}"
        )

    if failures:
        for failure in failures:
            print(f"conformance divergence: {failure}", file=sys.stderr)
        return 1
    executed = sum(1 for item in rust_handshake if item.get("executed"))
    print(
        "conformance: both implementations agree on "
        f"{len(rust.get('qr', []))} QR vectors, "
        f"{len(rust.get('signaling', []))} message scenarios, "
        f"{len(rust.get('encoding', []))} encoding vectors and "
        f"{len(rust_handshake)} handshake scenarios "
        f"({executed} replayed by both cores, {len(rust_handshake) - executed} "
        "delegated to the pairing transport tests)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
