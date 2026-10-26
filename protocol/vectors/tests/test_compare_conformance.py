"""Behaviour of the Rust/Kotlin conformance comparator (REC-14)."""

from __future__ import annotations

import copy
import json
import subprocess
import sys
from pathlib import Path

import pytest

VECTORS = Path(__file__).resolve().parents[1]
SCRIPT = VECTORS / "compare_conformance.py"


def scenario_ids(kind: str) -> list[str]:
    document = json.loads((VECTORS / "signaling/scenarios.json").read_text())
    return [item["id"] for item in document["scenarios"] if item["kind"] == kind]


def base_documents() -> tuple[dict, dict]:
    """A consistent pair of implementation outputs covering every vector."""
    qr = [
        {"id": path.stem, "accepted": True, "reason_code": None}
        for path in sorted((VECTORS / "qr").glob("*.json"))
    ]
    signaling = [
        {"id": item, "kind": "message", "accepted": True, "reason_code": None}
        for item in scenario_ids("message")
    ]
    encoding_file = json.loads((VECTORS / "signaling/encoding.json").read_text())
    encoding = [
        {"id": item["id"], "encoded_bytes": 10} for item in encoding_file["vectors"]
    ]
    reasons = {"count": 27, "ok": True}
    scenarios = json.loads((VECTORS / "signaling/scenarios.json").read_text())
    handshake = [
        {
            "id": item["id"],
            "layer": item["layer"],
            "executed": item["layer"] != "transport",
            "accepted": True,
            "reason_code": None,
            "state": "negotiating",
            "transition": "handshaking->negotiating",
            "epoch": 1,
            "protocol_violations": 0,
        }
        for item in scenarios["scenarios"]
        if item["kind"] == "handshake"
    ]
    main = {
        "qr": qr,
        "signaling": signaling,
        "reason_codes": reasons,
        "encoding": encoding,
    }
    return main, {"handshake": handshake}


def run(tmp_path: Path, rust, kotlin, rust_handshake, kotlin_handshake):
    paths = {}
    for name, document in {
        "rust": rust,
        "kotlin": kotlin,
        "rust_handshake": rust_handshake,
        "kotlin_handshake": kotlin_handshake,
    }.items():
        paths[name] = tmp_path / f"{name}.json"
        paths[name].write_text(json.dumps(document))
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            str(paths["rust"]),
            str(paths["kotlin"]),
            "--rust-handshake",
            str(paths["rust_handshake"]),
            "--kotlin-handshake",
            str(paths["kotlin_handshake"]),
            "--vectors",
            str(VECTORS),
        ],
        capture_output=True,
        check=False,
        text=True,
    )


def test_identical_documents_pass(tmp_path):
    main, handshake = base_documents()
    result = run(
        tmp_path, main, copy.deepcopy(main), handshake, copy.deepcopy(handshake)
    )
    assert result.returncode == 0, result.stderr


def test_a_diverging_handshake_state_fails(tmp_path):
    main, handshake = base_documents()
    kotlin_handshake = copy.deepcopy(handshake)
    kotlin_handshake["handshake"][0]["state"] = "ended"
    result = run(tmp_path, main, copy.deepcopy(main), handshake, kotlin_handshake)
    assert result.returncode == 1
    assert "handshake/" in result.stderr


def test_a_diverging_handshake_reason_code_fails(tmp_path):
    main, handshake = base_documents()
    kotlin_handshake = copy.deepcopy(handshake)
    kotlin_handshake["handshake"][1]["reason_code"] = "token_invalid"
    result = run(tmp_path, main, copy.deepcopy(main), handshake, kotlin_handshake)
    assert result.returncode == 1


def test_a_handshake_scenario_missing_from_both_reports_fails(tmp_path):
    main, handshake = base_documents()
    handshake["handshake"].pop()
    result = run(
        tmp_path, main, copy.deepcopy(main), handshake, copy.deepcopy(handshake)
    )
    assert result.returncode == 1
    assert "not executed" in result.stderr


def test_a_handshake_scenario_missing_from_one_report_fails(tmp_path):
    main, handshake = base_documents()
    kotlin_handshake = copy.deepcopy(handshake)
    kotlin_handshake["handshake"].pop(0)
    result = run(tmp_path, main, copy.deepcopy(main), handshake, kotlin_handshake)
    assert result.returncode == 1


def test_a_message_scenario_skipped_by_both_fails(tmp_path):
    main, handshake = base_documents()
    main["signaling"].pop()
    result = run(
        tmp_path, main, copy.deepcopy(main), handshake, copy.deepcopy(handshake)
    )
    assert result.returncode == 1
    assert "not executed" in result.stderr


def test_a_diverging_encoding_vector_fails(tmp_path):
    main, handshake = base_documents()
    kotlin = copy.deepcopy(main)
    kotlin["encoding"][0]["encoded_bytes"] = 11
    result = run(tmp_path, main, kotlin, handshake, copy.deepcopy(handshake))
    assert result.returncode == 1
    assert "encoding/" in result.stderr


def test_a_missing_encoding_section_fails(tmp_path):
    main, handshake = base_documents()
    kotlin = copy.deepcopy(main)
    del kotlin["encoding"]
    result = run(tmp_path, main, kotlin, handshake, copy.deepcopy(handshake))
    assert result.returncode == 1


@pytest.mark.parametrize("section", ["qr", "signaling"])
def test_a_diverging_message_vector_fails(tmp_path, section):
    main, handshake = base_documents()
    kotlin = copy.deepcopy(main)
    kotlin[section][0]["accepted"] = False
    result = run(tmp_path, main, kotlin, handshake, copy.deepcopy(handshake))
    assert result.returncode == 1


def relabel(handshake: dict, scenario_id: str, **changes) -> dict:
    relabelled = copy.deepcopy(handshake)
    for item in relabelled["handshake"]:
        if item["id"] == scenario_id:
            item.update(changes)
    return relabelled


def test_a_core_scenario_reported_as_not_executed_by_both_fails(tmp_path):
    main, handshake = base_documents()
    skipped = relabel(
        handshake, "hs-valid-pairing", layer="transport", executed=False
    )
    result = run(tmp_path, main, copy.deepcopy(main), skipped, copy.deepcopy(skipped))
    assert result.returncode == 1
    assert "hs-valid-pairing" in result.stderr


def test_a_core_scenario_that_claims_not_to_be_executed_fails(tmp_path):
    main, handshake = base_documents()
    skipped = relabel(handshake, "hs-valid-pairing", executed=False)
    result = run(tmp_path, main, copy.deepcopy(main), skipped, copy.deepcopy(skipped))
    assert result.returncode == 1


def test_a_transport_scenario_that_claims_to_be_executed_fails(tmp_path):
    main, handshake = base_documents()
    claimed = relabel(handshake, "hs-wrong-fingerprint", executed=True)
    result = run(tmp_path, main, copy.deepcopy(main), claimed, copy.deepcopy(claimed))
    assert result.returncode == 1
