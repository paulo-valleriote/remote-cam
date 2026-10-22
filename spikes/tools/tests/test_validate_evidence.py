from __future__ import annotations

import json
from pathlib import Path

import pytest

from validation.validate_evidence import (
    ENVIRONMENT,
    INJECTION_MATRIX,
    LATENCY_SUMMARY,
    PAIRING_RESIDUE,
    PAIRING_SCENARIOS,
    REPO_ROOT,
    STAGE_BUDGET,
    TOOLCHAIN_SELFTEST,
    validate_document,
    validate_evidence_root,
)

FULL_HEX = "9f2c1e4b6a8d0f3c5e7a9b1d2f4c6e8a0b2d4f6a8c0e2b4d6f8a0c1e3b5d7f9a"


def environment(**overrides: object) -> dict[str, object]:
    document: dict[str, object] = {
        "environment_id": "ref-win11-01",
        "captured_at": "2026-11-03T09:14:22+00:00",
        "cpu_model": "AMD Ryzen 7 5800X",
        "cpu_logical_cores": 16,
        "gpu_model": "NVIDIA GeForce RTX 3060",
        "os_name": "Windows 11",
        "os_build": "22631.4169",
        "network": {
            "band": "5_ghz",
            "wifi_power_save": "disabled",
            "internet_enabled": False,
            "within_reference_conditions": True,
        },
        "hardware_decode_verification": {
            "codec": "H.264",
            "verified_at": "2026-11-03T09:20:01+00:00",
            "discovery_method": "mft_enum_syncmft",
            "decoder_found": True,
            "hardware_accelerated": True,
            "output_format_supported": "nv12",
            "evidence": "MFTEnumEx(SYNCMFT|SORTANDFILTER) -> MSH264DecoderMFT",
        },
    }
    document.update(overrides)
    return document


def codes(findings: list) -> set[str]:
    return {finding.code for finding in findings}


def test_reference_environment_is_clean() -> None:
    assert validate_document(environment(), ENVIRONMENT) == []


def test_outside_reference_environment_warns_but_is_not_invalid() -> None:
    document = environment()
    document["network"]["within_reference_conditions"] = False

    findings = validate_document(document, ENVIRONMENT)

    assert codes(findings) == {"environment.outside_reference"}
    assert findings[0].level == "warning"


def test_reference_conditions_cannot_claim_24ghz() -> None:
    document = environment()
    document["network"]["band"] = "2_4_ghz"

    assert "environment.reference_conditions_inconsistent" in codes(
        validate_document(document, ENVIRONMENT)
    )


def test_hardware_acceleration_requires_nv12() -> None:
    document = environment()
    document["hardware_decode_verification"]["output_format_supported"] = "i420"

    findings = validate_document(document, ENVIRONMENT)

    assert "environment.output_format_not_nv12" in codes(findings)
    assert "environment.hardware_decode_unverified" in codes(findings)


def selftest(passed: bool = True) -> dict[str, object]:
    checks = [
        {
            "name": "silence_fixture",
            "expected": {"dropout_count": 2},
            "observed": {"dropout_count": 2},
            "passed": passed,
        },
        {
            "name": "qr_roundtrip",
            "expected": {"sha256": "x"},
            "observed": {"sha256": "x"},
            "passed": True,
        },
        {
            "name": "sampler_agreement",
            "expected": {"delta_p95_cpu_pct": "<= 1.0"},
            "observed": {"delta_p95_cpu_pct": 0.3},
            "passed": True,
        },
        {
            "name": "harness_floor",
            "expected": {"floor_ms": "measured"},
            "observed": {"floor_ms": 33.4},
            "passed": True,
        },
    ]
    return {
        "run_at": "2026-11-03T09:02:00+00:00",
        "checks": checks,
        "all_passed": all(check["passed"] for check in checks),
    }


def test_toolchain_selftest_accepts_exactly_four_checks() -> None:
    assert validate_document(selftest(), TOOLCHAIN_SELFTEST) == []


def test_toolchain_selftest_failure_invalidates_measurements() -> None:
    findings = validate_document(selftest(passed=False), TOOLCHAIN_SELFTEST)

    assert "selftest.failed" in codes(findings)


def test_toolchain_selftest_requires_the_four_named_checks() -> None:
    document = selftest()
    document["checks"] = document["checks"][:3]

    findings = validate_document(document, TOOLCHAIN_SELFTEST)

    assert {"selftest.check_count", "selftest.check_names"} <= codes(findings)


def injection_matrix() -> dict[str, object]:
    applications = [
        {
            "application_id": "chrome",
            "category": "chromium_browser",
            "product_name": "Chrome",
            "version": "131",
            "bitness": "64_bit",
        },
        {
            "application_id": "zoom",
            "category": "video_conferencing_client",
            "product_name": "Zoom",
            "version": "6.3",
            "bitness": "64_bit",
        },
        {
            "application_id": "obs",
            "category": "obs",
            "product_name": "OBS",
            "version": "31",
            "bitness": "64_bit",
        },
        {
            "application_id": "camera",
            "category": "windows_camera_app",
            "product_name": "Camera",
            "version": "11",
            "bitness": "64_bit",
        },
        {
            "application_id": "recorder32",
            "category": "screen_recorder",
            "product_name": "Recorder",
            "version": "1",
            "bitness": "32_bit",
        },
    ]
    cells = [
        {
            "backend": "mf_virtual_camera",
            "application_id": application["application_id"],
            "outcome": "frames_delivered",
            "observed_duration_s": 1800,
            "delivered_size_ok": True,
            "delivered_rate_ok": True,
            "longest_stall_ms": 900,
        }
        for application in applications
    ]
    return {
        "matrix_version": 1,
        "environment_id": "ref-win11-01",
        "captured_at": "2026-11-03T12:00:00+00:00",
        "backends_evaluated": [
            {
                "backend": "mf_virtual_camera",
                "registration": "per_user",
                "requires_admin": False,
                "licensing": "MIT",
            }
        ],
        "applications": applications,
        "cells": cells,
        "selected_backend": "mf_virtual_camera",
        "decision_rationale": "Every cell delivered frames.",
    }


def test_injection_matrix_accepts_full_coverage() -> None:
    assert validate_document(injection_matrix(), INJECTION_MATRIX) == []


def test_injection_matrix_requires_both_bitnesses() -> None:
    document = injection_matrix()
    document["applications"][4]["bitness"] = "64_bit"

    assert "injection.bitness_coverage" in codes(
        validate_document(document, INJECTION_MATRIX, repo_root=REPO_ROOT)
    )


def test_injection_matrix_rejects_a_short_run() -> None:
    document = injection_matrix()
    document["cells"][0]["observed_duration_s"] = 1200

    assert "injection.duration_too_short" in codes(validate_document(document, INJECTION_MATRIX))


def latency_summary() -> dict[str, object]:
    return {
        "run_id": "lat-01",
        "sample_count": 17812,
        "harness_floor_ms": 33.4,
        "raw": {"p50_ms": 71.2, "p95_ms": 118.6, "p99_ms": 141.0, "max_ms": 187.3},
        "corrected": {"p50_ms": 37.8, "p95_ms": 85.2, "p99_ms": 107.6, "max_ms": 153.9},
        "criterion": {
            "metric": "raw.p95_ms",
            "threshold": 150,
            "comparison": "<=",
            "basis": "raw",
        },
        "verdict": "pass",
    }


def test_latency_summary_is_judged_on_the_raw_p95() -> None:
    assert validate_document(latency_summary(), LATENCY_SUMMARY) == []

    document = latency_summary()
    document["criterion"]["basis"] = "corrected"

    assert "latency.basis" in codes(validate_document(document, LATENCY_SUMMARY))


def test_latency_correction_must_match_the_floor() -> None:
    document = latency_summary()
    document["corrected"]["p95_ms"] = 100.0

    assert "latency.correction" in codes(validate_document(document, LATENCY_SUMMARY))


def test_latency_pass_above_threshold_is_rejected() -> None:
    document = latency_summary()
    document["raw"]["p95_ms"] = 151.0
    document["corrected"]["p95_ms"] = 117.6

    assert "latency.threshold" in codes(validate_document(document, LATENCY_SUMMARY))


def test_non_passing_latency_requires_attribution() -> None:
    document = latency_summary()
    document["raw"]["p95_ms"] = 180.0
    document["corrected"]["p95_ms"] = 146.6
    document["verdict"] = "fail"

    assert "latency.attribution" in codes(validate_document(document, LATENCY_SUMMARY))


def stage_budget() -> dict[str, object]:
    return {
        "run_id": "lat-01",
        "total_budget_ms": 100,
        "stages": [
            {
                "stage": "capture",
                "budget_ms": 33,
                "measured_ms": None,
                "included_in_gate": False,
                "measurement_method": "excluded",
            },
            {
                "stage": "network_and_jitter_buffer",
                "budget_ms": 20,
                "measured_ms": 21.4,
                "included_in_gate": True,
                "measurement_method": "p95",
            },
            {
                "stage": "decode",
                "budget_ms": 12,
                "measured_ms": 9.8,
                "included_in_gate": True,
                "measurement_method": "p95",
            },
            {
                "stage": "frame_handoff",
                "budget_ms": 3,
                "measured_ms": 1.6,
                "included_in_gate": True,
                "measurement_method": "p95",
            },
            {
                "stage": "consumer_presentation",
                "budget_ms": 17,
                "measured_ms": 41.9,
                "included_in_gate": True,
                "measurement_method": "p95",
            },
        ],
    }


def test_stage_budget_accepts_the_contract_shape() -> None:
    assert validate_document(stage_budget(), STAGE_BUDGET) == []


def test_stage_sum_is_checked_against_the_sibling_latency_summary(tmp_path: Path) -> None:
    root = tmp_path / "evidence"
    root.mkdir()
    (root / "latency_run1_summary.json").write_text(json.dumps(latency_summary()), encoding="utf-8")
    budget = stage_budget()
    budget["stages"][1]["measured_ms"] = 200.0
    (root / "stage_budget.json").write_text(json.dumps(budget), encoding="utf-8")

    result = validate_evidence_root(root)

    assert "stage.sum_exceeds_raw" in codes(result.findings)


def test_evidence_root_reports_missing_evidence_as_empty(tmp_path: Path) -> None:
    result = validate_evidence_root(tmp_path / "missing")

    assert result.ok is True
    assert result.files == []


def test_pairing_scenarios_reject_media_before_authentication() -> None:
    document = {
        "harness_version": "1.0.0",
        "scenarios": [
            {
                "scenario_id": "valid-01",
                "category": "valid",
                "preconditions": "QR displayed.",
                "expected_result": "Channel established.",
                "observed_result": "Channel established.",
                "outcome": "accepted",
                "reason_code": None,
                "media_flowed_before_authentication": True,
                "residual_credentials": [],
            }
        ],
    }

    findings = validate_document(document, PAIRING_SCENARIOS, repo_root=REPO_ROOT)

    assert "pairing.media_before_authentication" in codes(findings)


def test_pairing_reason_code_must_be_closed() -> None:
    document = {
        "harness_version": "1.0.0",
        "scenarios": [
            {
                "scenario_id": "bad",
                "category": "adversarial",
                "preconditions": "Wrong fingerprint.",
                "expected_result": "Reject.",
                "observed_result": "Rejected.",
                "outcome": "rejected",
                "reason_code": "made_up_code",
                "media_flowed_before_authentication": False,
                "residual_credentials": [],
            }
        ],
    }

    assert "pairing.reason_code" in codes(
        validate_document(document, PAIRING_SCENARIOS, repo_root=REPO_ROOT)
    )


def test_residue_requires_both_cases_and_empty_findings() -> None:
    inspection = {
        "inspection_id": "residue-after-session-end",
        "case": "after_session_end",
        "devices_inspected": ["windows", "android"],
        "artefact_kinds_checked": ["session_key", "pairing_token", "fingerprint", "pairing_record"],
        "artefacts_found": ["pairing_token"],
        "evidence_ref": "spikes/evidence/pairing/residue.txt",
    }
    document = {
        "harness_version": "1.0.0",
        "method": "memory and filesystem sweep",
        "coverage": ["app data", "credential store"],
        "devices_inspected": ["windows", "android"],
        "inspections": [inspection],
    }

    findings = validate_document(document, PAIRING_RESIDUE)

    assert {"residue.cases", "residue.found"} <= codes(findings)


def test_latency_samples_formula_is_checked(tmp_path: Path) -> None:
    path = tmp_path / "latency_samples.csv"
    path.write_text(
        "sample_id,run_id,consumer_app,source_frame_no,source_epoch_ms,capture_host_ms,"
        "clock_offset_ms,latency_ms,harness_floor_ms\n"
        "1,lat-01,obs,10,1000,1120,0,120,33.4\n"
        "2,lat-01,obs,11,1033,1153,0,119,33.4\n",
        encoding="utf-8",
    )

    result = validate_evidence_root(tmp_path)

    assert result.ok is False
    assert "latency_samples.formula" in codes(result.findings)


def test_unrecognised_name_is_an_error_in_explicit_mode(tmp_path: Path) -> None:
    from validation.validate_evidence import main

    path = tmp_path / "notes.txt"
    path.write_text("hello", encoding="utf-8")

    assert main([str(path)]) == 1


@pytest.mark.parametrize(
    "filename",
    ["latency_run1_summary.json", "injection_matrix.json", "pairing_scenarios.json"],
)
def test_known_names_are_discovered(tmp_path: Path, filename: str) -> None:
    root = tmp_path / "evidence"
    root.mkdir()
    (root / filename).write_text("{}", encoding="utf-8")

    result = validate_evidence_root(root)

    assert str(root / filename) in result.files
    assert result.ok is False
