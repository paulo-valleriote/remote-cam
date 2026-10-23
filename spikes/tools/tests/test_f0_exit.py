"""Closure invariants for the F0 phase exit.

These tests guard the closure record itself: the verdicts did not change, every
waiver has a named signer and a deadline, no decision claims evidence that does
not exist, and the only injection evidence used is the Google Chrome run.

NON-PRODUCTION — F0 risk-reduction spike (FR-039).
"""

from __future__ import annotations

import csv
import json
from datetime import date, datetime
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
EVIDENCE_ROOT = REPO_ROOT / "spikes" / "evidence"
EXIT_PATH = EVIDENCE_ROOT / "f0_exit.json"
REPORT_PATH = EVIDENCE_ROOT / "gate_report.json"
DECISION_INDEX_PATH = EVIDENCE_ROOT / "decisions" / "index.json"
QUESTIONS_PATH = EVIDENCE_ROOT / "open_questions.json"
CHROME_RUN_PATH = EVIDENCE_ROOT / "injection" / "runs" / "mf_virtual_camera" / "chrome"


def _load(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _exit() -> dict:
    return _load(EXIT_PATH)


def _report() -> dict:
    return _load(REPORT_PATH)


def _closed_on() -> date:
    return datetime.fromisoformat(_exit()["closed_at"]).date()


def test_exit_is_closed_with_exceptions_and_the_gate_is_not_met():
    exit_record = _exit()
    assert exit_record["closure_mode"] == "closed_with_exceptions"
    assert exit_record["exit_gate"] == "not_met"
    assert exit_record["statement"]


def test_report_embeds_the_exit_record_and_review_timestamp():
    exit_record = _exit()
    report = _report()
    assert report["exit"] == exit_record
    assert report["phase_review_at"] == exit_record["closed_at"]
    assert report["report_version"] == 2


def test_no_verdict_changed_and_every_gate_has_one_disposition():
    report = _report()
    exit_record = _exit()
    verdicts = {gate["gate_id"]: gate["verdict"] for gate in report["gates"]}
    dispositions = {
        disposition["gate_id"]: disposition["verdict"]
        for disposition in exit_record["gate_dispositions"]
    }
    assert dispositions == verdicts
    assert len(dispositions) == 5
    assert dispositions["camera_injection"] == "no_go"
    assert dispositions["desktop_receive_pipeline"] == "no_go"


def test_every_waiver_has_a_named_signer_and_a_future_deadline():
    closed_on = _closed_on()
    for waiver in _exit()["waivers"]:
        assert waiver["waived_by"]["person_name"]
        assert waiver["waived_by"]["role_type"] in {
            "technical_lead",
            "security_reviewer",
            "product_owner",
        }
        deadline = date.fromisoformat(waiver["resolution_deadline"])
        assert deadline >= closed_on, waiver["condition_id"]
        assert waiver["conditioned_phases"]
        assert waiver["waiver_conditions"]


def test_conditional_gates_carry_the_waived_conditions():
    report = _report()
    waivers = {waiver["condition_id"]: waiver for waiver in _exit()["waivers"]}
    seen: set[str] = set()
    for gate in report["gates"]:
        if gate["verdict"] != "conditional":
            continue
        assert gate.get("conditions"), gate["gate_id"]
        for condition in gate["conditions"]:
            assert condition["status"] == "waived", condition["condition_id"]
            waiver = waivers[condition["condition_id"]]
            assert condition["waiver"]["resolution_deadline"] == waiver["resolution_deadline"]
            assert condition["conditioned_phases"] == waiver["conditioned_phases"]
            seen.add(condition["condition_id"])
    assert seen == set(waivers)


def test_downstream_phases_are_conditional_never_unblocked():
    downstream = _report()["downstream_status"]
    for phase in ("F1", "F2", "F3"):
        assert downstream[phase]["status"] == "conditional", phase
        assert downstream[phase]["reason"]


def test_amendments_are_disposed_and_deviations_are_approved():
    exit_record = _exit()
    amendments = exit_record["amendments"]
    assert amendments["total"] == 13
    assert amendments["applied"] + amendments["no_op"] + amendments["waived"] == 13
    assert amendments["no_op"] == 1
    assert set(exit_record["deviations"]["approved"]) == {"D1", "D2", "D3", "D4", "D5"}


def test_no_open_question_remains_and_deferred_ones_name_a_phase():
    questions = _load(QUESTIONS_PATH)["open_questions"]
    assert questions
    for question in questions:
        assert question["status"] in {"answered", "deferred"}, question["question_id"]
        if question["status"] == "deferred":
            assert question.get("target_phase"), question["question_id"]


def test_decision_index_has_no_phantom_evidence():
    decisions = _load(DECISION_INDEX_PATH)["decisions"]
    assert decisions
    for decision in decisions:
        for reference in decision["evidence_refs"]:
            assert (REPO_ROOT / reference).exists(), f"{decision['decision_id']}: {reference}"
        for missing in decision["missing_evidence"]:
            assert not (REPO_ROOT / missing).exists(), (
                f"{decision['decision_id']}: {missing} is listed as missing but exists"
            )
        if decision["status"] == "accepted":
            assert decision["approver"], decision["decision_id"]
            assert not decision["missing_evidence"], decision["decision_id"]


def test_codec_decision_does_not_claim_unmeasured_interop_runs():
    text = (EVIDENCE_ROOT / "decisions" / "D-codec-baseline.md").read_text(encoding="utf-8")
    forbidden = (
        "Verified on reference machine",
        "18000 frames",
        "CPU (p95): 8.5",
        "Latency (p95): 45 ms",
        "Quality (PSNR): 42",
    )
    for phrase in forbidden:
        assert phrase not in text, phrase
    assert "not measured in F0" in text
    assert "**Status**: draft" in text


def test_chrome_partial_run_is_used_and_does_not_pass_the_duration_gate():
    for name in ("run.json", "capture.json", "observations.csv"):
        assert (CHROME_RUN_PATH / name).is_file(), name
    assert not (CHROME_RUN_PATH / "capture.webm").exists()

    with (CHROME_RUN_PATH / "observations.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    stamps = [float(row["capture_host_ms"]) for row in rows]
    derived_duration_s = (max(stamps) - min(stamps)) / 1_000_000

    camera = next(gate for gate in _report()["gates"] if gate["gate_id"] == "camera_injection")
    partial = camera["measured_result"]["partial_run"]
    assert partial["application_id"] == "chrome"
    assert abs(partial["observed_duration_s"] - derived_duration_s) < 0.001
    assert partial["observed_duration_s"] < 1800
    assert partial["meets_duration"] is False
    assert camera["measured_result"]["matrix_exists"] is False
    assert camera["measured_result"]["selected_backend"] == "not_determined"
    assert camera["verdict"] == "no_go"
    assert camera["failure_code"] == "evidence_incomplete"


def test_raw_video_captures_are_ignored():
    gitignore = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8")
    assert "spikes/evidence/**/*.webm" in gitignore
