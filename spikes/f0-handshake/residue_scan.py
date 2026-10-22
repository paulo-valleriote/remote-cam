#!/usr/bin/env python3
"""Residue scanner for the F0 pairing gate.

The same scanner, with the same needles and the same roots, is used after a
normal session end and after an application restart. A clean result is only
meaningful because the scanner is seeded and verified first (see
``tests/test_residue_scanner.py``).

This is non-production spike tooling (FR-039).
"""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

MAX_FILE_BYTES = 64 * 1024 * 1024


@dataclass(frozen=True)
class Finding:
    """One located occurrence of a secret pattern."""

    location: str
    label: str
    offset: int

    def as_dict(self) -> dict[str, object]:
        return {"location": self.location, "label": self.label, "offset": self.offset}


@dataclass(frozen=True)
class Needle:
    """A labelled byte pattern."""

    label: str
    pattern: bytes


def parse_needle(raw: str) -> Needle:
    """Parses ``label=hex`` into a needle."""

    label, separator, hex_value = raw.partition("=")
    if not separator or not label or not hex_value:
        raise ValueError(f"needle {raw!r} must be label=hex")
    if len(hex_value) % 2:
        raise ValueError(f"needle {raw!r} has an odd-length hex value")
    try:
        pattern = bytes.fromhex(hex_value)
    except ValueError as error:
        raise ValueError(f"needle {raw!r} is not hexadecimal") from error
    if not pattern:
        raise ValueError(f"needle {raw!r} is empty")
    return Needle(label=label, pattern=pattern)


def scan_bytes(data: bytes, needles: list[Needle]) -> list[tuple[str, int]]:
    """Finds every needle occurrence in ``data``, in needle order."""

    found: list[tuple[str, int]] = []
    for needle in needles:
        start = 0
        while start <= len(data) - len(needle.pattern):
            offset = data.find(needle.pattern, start)
            if offset < 0:
                break
            found.append((needle.label, offset))
            start = offset + 1
    return found


def scan_file(path: Path, needles: list[Needle], display: str | None = None) -> list[Finding]:
    """Scans one regular file."""

    if path.is_symlink() or not path.is_file():
        return []
    if path.stat().st_size > MAX_FILE_BYTES:
        return []
    data = path.read_bytes()
    location = display if display is not None else path.as_posix()
    return [
        Finding(location=location, label=label, offset=offset)
        for label, offset in scan_bytes(data, needles)
    ]


def scan_probe(label: str, command: str, needles: list[Needle]) -> list[Finding]:
    """Runs one platform probe and scans its combined output.

    The probes are the platform-specific parts of the method: the certificate and
    credential stores on Windows, the registry, an Android package's data
    through ``adb``, or a process-memory dump produced by the procedure. Keeping
    them as commands lets the same recorded method be executed on both devices.
    """

    result = subprocess.run(
        shlex.split(command),
        capture_output=True,
        check=False,
        timeout=120,
    )
    output = result.stdout + result.stderr
    return [
        Finding(location=f"probe:{label}", label=needle_label, offset=offset)
        for needle_label, offset in scan_bytes(output, needles)
    ]


def scan_tree(root: Path, needles: list[Needle]) -> list[Finding]:
    """Recursively scans a directory tree without following symlinks."""

    if not root.exists():
        raise FileNotFoundError(f"inspection root does not exist: {root}")
    findings: list[Finding] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        findings.extend(scan_file(path, needles, display=path.relative_to(root).as_posix()))
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Scan an app-data tree for pairing artefacts")
    parser.add_argument("--root", type=Path, required=True, help="directory to scan")
    parser.add_argument(
        "--needle",
        action="append",
        default=[],
        metavar="LABEL=HEX",
        help="secret pattern to search for; repeatable",
    )
    parser.add_argument(
        "--probe",
        action="append",
        default=[],
        metavar="LABEL=COMMAND",
        help="run a platform probe and scan its output; repeatable",
    )
    parser.add_argument("--output", type=Path, help="write the findings as JSON")
    arguments = parser.parse_args(argv)

    try:
        needles = [parse_needle(raw) for raw in arguments.needle]
    except ValueError as error:
        parser.error(str(error))
    if not needles:
        parser.error("at least one --needle is required")

    try:
        findings = scan_tree(arguments.root, needles)
    except FileNotFoundError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    for raw in arguments.probe:
        label, separator, command = raw.partition("=")
        if not separator or not label or not command:
            parser.error(f"probe {raw!r} must be label=command")
        findings.extend(scan_probe(label, command, needles))

    document = {
        "root": arguments.root.as_posix(),
        "needles": [needle.label for needle in needles],
        "probes": [raw.split("=", 1)[0] for raw in arguments.probe],
        "findings": [finding.as_dict() for finding in findings],
    }
    text = json.dumps(document, indent=2) + "\n"
    if arguments.output:
        arguments.output.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0 if not findings else 1


if __name__ == "__main__":
    raise SystemExit(main())
