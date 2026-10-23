"""Validation tests for verdict semantics in the F0 gate report.

Covers T115: a conditional verdict requires either a waiver signed by the gate's
accountable role with a `resolution_deadline` or an explicit block; a gate whose
dependency is `no_go` still carries a verdict naming it; all five share one review
timestamp within one minute.

NON-PRODUCTION — F0 risk-reduction spike (FR-039).
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
REPORT_PATH = REPO_ROOT / "spikes" / "evidence" / "gate_report.json"


def _load_report() -> dict:
    with REPORT_PATH.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def test_report_has_required_top_level_fields():
    report = _load_report()
    for field in (
        "report_version",
        "feature",
        "phase_review_at",
        "reference_environment",
        "gates",
        "downstream_status",
        "open_questions",
        "exit",
    ):
        assert field in report, f"missing top-level field: {field}"
    assert report["report_version"] == 2
    assert report["feature"] == "001-f0-technical-spikes"


def test_five_gates_present():
    report = _load_report()
    gates = report["gates"]
    assert len(gates) == 5, f"expected 5 gates, got {len(gates)}"
    expected_ids = {
        "camera_injection",
        "desktop_receive_pipeline",
        "codec_interoperability",
        "pairing_handshake",
        "virtual_audio_and_licensing",
    }
    actual_ids = {g["gate_id"] for g in gates}
    assert actual_ids == expected_ids, f"gate ids mismatch: {actual_ids}"


def test_all_gates_share_one_review_timestamp_within_one_minute():
    report = _load_report()
    phase_review_at = report["phase_review_at"]
    base = datetime.fromisoformat(phase_review_at.replace("Z", "+00:00"))
    for g in report["gates"]:
        issued = datetime.fromisoformat(g["issued_at"].replace("Z", "+00:00"))
        delta = abs((issued - base).total_seconds())
        assert delta <= 60, (
            f"gate {g['gate_id']} issued_at {g['issued_at']} is {delta}s from "
            f"phase_review_at {phase_review_at} (> 60s)"
        )


def test_verdict_values_are_valid():
    report = _load_report()
    valid_verdicts = {"go", "no_go", "conditional"}
    for g in report["gates"]:
        assert g["verdict"] in valid_verdicts, (
            f"gate {g['gate_id']} has invalid verdict {g['verdict']}"
        )


def test_no_go_or_conditional_has_failure_code():
    report = _load_report()
    for g in report["gates"]:
        if g["verdict"] in ("no_go", "conditional"):
            assert g.get("failure_code"), (
                f"gate {g['gate_id']} is {g['verdict']} but has no failure_code"
            )


def test_conditional_has_at_least_one_condition():
    report = _load_report()
    for g in report["gates"]:
        if g["verdict"] == "conditional":
            conditions = g.get("conditions", [])
            assert len(conditions) >= 1, f"gate {g['gate_id']} is conditional but has no conditions"
            for c in conditions:
                assert "condition_id" in c
                assert "description" in c
                assert "conditioned_phases" in c
                assert "status" in c
                assert c["status"] in ("blocking", "waived", "resolved")


def test_conditional_waiver_has_resolution_deadline():
    report = _load_report()
    for g in report["gates"]:
        if g["verdict"] == "conditional":
            for c in g.get("conditions", []):
                if c["status"] == "waived":
                    waiver = c.get("waiver")
                    assert waiver is not None, (
                        f"gate {g['gate_id']} condition {c['condition_id']} is waived "
                        "but has no waiver"
                    )
                    assert waiver.get("resolution_deadline"), (
                        f"gate {g['gate_id']} condition {c['condition_id']} waiver has "
                        "no resolution_deadline"
                    )
                    assert waiver.get("waived_by"), (
                        f"gate {g['gate_id']} condition {c['condition_id']} waiver has no waived_by"
                    )


def test_gate_whose_dependency_is_no_go_still_carries_a_verdict_naming_it():
    report = _load_report()
    no_go_ids = {g["gate_id"] for g in report["gates"] if g["verdict"] == "no_go"}
    for g in report["gates"]:
        for dep in g.get("depends_on", []):
            if dep in no_go_ids:
                assert g.get("blocking_dependency") == dep, (
                    f"gate {g['gate_id']} depends on no_go gate {dep} but blocking_dependency "
                    f"is {g.get('blocking_dependency')}"
                )


def test_go_gate_has_at_least_one_evidence_ref():
    report = _load_report()
    for g in report["gates"]:
        if g["verdict"] == "go":
            assert len(g.get("evidence_refs", [])) >= 1, (
                f"gate {g['gate_id']} is go but has no evidence_refs"
            )


def test_latency_gate_has_raw_and_floor_fields():
    report = _load_report()
    for g in report["gates"]:
        if g["gate_id"] == "desktop_receive_pipeline":
            for field in (
                "harness_floor_ms",
                "raw_latency_p95_ms",
                "floor_corrected_latency_p95_ms",
                "included_latency_stages",
                "excluded_latency_stages",
            ):
                assert field in g, f"latency gate missing field: {field}"
            assert len(g["included_latency_stages"]) >= 1


def test_pairing_gate_has_throttling_and_residue():
    report = _load_report()
    for g in report["gates"]:
        if g["gate_id"] == "pairing_handshake":
            assert "throttling_summary" in g
            assert "residue_summary" in g
            ts = g["throttling_summary"]
            assert ts["attempt_limit_per_minute"] == 5
            assert ts["limit_exceeded_at_attempt"] == 6
            assert ts["consecutive_failures_at_listener_close"] == 10
            rs = g["residue_summary"]
            assert rs["method"]
            assert len(rs["coverage"]) >= 1


def test_audio_gate_has_audio_measurement_summary():
    report = _load_report()
    for g in report["gates"]:
        if g["gate_id"] == "virtual_audio_and_licensing":
            assert "audio_measurement_summary" in g
            ams = g["audio_measurement_summary"]
            for field in (
                "dropout_count",
                "detectors_agree",
                "consumer_renegotiations",
                "phrases_exact",
                "corpus_wer",
                "observer",
                "phrase_script_ref",
            ):
                assert field in ams, f"audio gate missing audio_measurement_summary field: {field}"


def test_downstream_status_has_f1_f2_f3():
    report = _load_report()
    ds = report["downstream_status"]
    for phase in ("F1", "F2", "F3"):
        assert phase in ds, f"downstream_status missing {phase}"
        st = ds[phase]
        assert "status" in st and "reason" in st
        assert st["status"] in ("unblocked", "blocked", "conditional")


def test_decisions_have_required_fields():
    report = _load_report()
    for d in report.get("decisions", []):
        for field in ("decision_id", "topic", "status", "owner"):
            assert field in d, f"decision missing field: {field}"
        assert d["status"] in ("draft", "accepted", "superseded")


def test_open_questions_have_required_fields():
    report = _load_report()
    for oq in report.get("open_questions", []):
        for field in ("question_id", "question", "owner", "impact", "blocking", "status"):
            assert field in oq, f"open question missing field: {field}"
        assert oq["status"] in ("open", "answered", "deferred")
        assert isinstance(oq["blocking"], bool)


def test_redistribution_note_present():
    report = _load_report()
    assert report.get("redistribution_note"), "missing redistribution_note"
    assert "NON-PRODUCTION" in report["redistribution_note"]


def test_report_validates_against_the_published_schema():
    from jsonschema import Draft202012Validator

    schema_path = (
        REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "gate-report.schema.json"
    )
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    errors = sorted(
        Draft202012Validator(schema).iter_errors(_load_report()),
        key=lambda error: list(error.path),
    )
    assert not errors, "\n".join(f"{list(error.path)}: {error.message}" for error in errors)
