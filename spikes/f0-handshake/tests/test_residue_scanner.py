"""Self-test for the residue scanner (T046).

The scanner is only evidence if it can find a deliberately seeded artefact and
then report a clean tree after the artefact is removed. A scanner that always
returns an empty list would pass a naive residue check; these tests make that
impossible.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from residue_scan import (  # noqa: E402
    Finding,
    Needle,
    main,
    parse_needle,
    scan_bytes,
    scan_probe,
    scan_tree,
)

TOKEN = bytes.fromhex("ab" * 32)
FINGERPRINT_HEX = "cd" * 32


def test_a_seeded_dummy_token_is_detected_then_removed(tmp_path: Path) -> None:
    store = tmp_path / "app-data"
    journals = store / "journals"
    journals.mkdir(parents=True)
    (store / "clean.bin").write_bytes(b"nothing secret here")
    needles = [Needle("pairing_token", TOKEN)]

    assert scan_tree(store, needles) == [], "the clean tree must be clean"

    seeded = journals / "crash-recovery.bin"
    seeded.write_bytes(b"header" + TOKEN + b"tail")
    findings = scan_tree(store, needles)
    assert findings == [Finding("journals/crash-recovery.bin", "pairing_token", 6)]

    seeded.unlink()
    assert scan_tree(store, needles) == [], "the scanner must not pass vacuously"


def test_a_hex_fingerprint_is_detected_across_a_journal(tmp_path: Path) -> None:
    (tmp_path / "settings.json").write_text(
        json.dumps({"latest": FINGERPRINT_HEX}), encoding="utf-8"
    )
    needles = [Needle("fingerprint_hex", FINGERPRINT_HEX.encode("ascii"))]
    findings = scan_tree(tmp_path, needles)
    assert len(findings) == 1
    assert findings[0].label == "fingerprint_hex"


def test_symlinks_are_not_followed(tmp_path: Path) -> None:
    store = tmp_path / "store"
    outside = tmp_path / "outside.bin"
    store.mkdir()
    outside.write_bytes(TOKEN)
    (store / "link").symlink_to(outside)

    assert scan_tree(store, [Needle("pairing_token", TOKEN)]) == []


def test_needle_parsing_rejects_malformed_values() -> None:
    assert parse_needle("pairing_token=abcd") == Needle("pairing_token", b"\xab\xcd")
    for raw in (
        "pairing_token",
        "=abcd",
        "pairing_token=",
        "pairing_token=abc",
        "pairing_token=zz",
    ):
        with pytest.raises(ValueError):
            parse_needle(raw)


def test_scan_bytes_reports_every_occurrence() -> None:
    needles = [Needle("pairing_token", b"ab")]
    assert scan_bytes(b"ab..ab", needles) == [("pairing_token", 0), ("pairing_token", 4)]


def test_a_platform_probe_output_is_scanned() -> None:
    needles = [Needle("pairing_token_hex", TOKEN.hex().encode("ascii"))]
    assert scan_probe("clean", "echo nothing-secret", needles) == []
    findings = scan_probe("seeded", f"echo {TOKEN.hex()}", needles)
    assert len(findings) == 1
    assert findings[0].location == "probe:seeded"


def test_cli_detects_a_seeded_token_and_returns_failure(tmp_path: Path) -> None:
    (tmp_path / "store.bin").write_bytes(TOKEN)
    output = tmp_path / "findings.json"
    exit_code = main(
        [
            "--root",
            str(tmp_path),
            "--needle",
            f"pairing_token={TOKEN.hex()}",
            "--output",
            str(output),
        ]
    )
    assert exit_code == 1
    document = json.loads(output.read_text(encoding="utf-8"))
    assert document["findings"][0]["label"] == "pairing_token"
