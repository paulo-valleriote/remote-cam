"""Tests for the licensing assessment (T107/T108)."""

from __future__ import annotations

import pytest

from analysis.licensing import (
    CANDIDATES,
    MEASUREMENT_OPTION,
    OPTIONS_COMPARED,
    LicenseDecisionError,
    build_bundle,
    measurement_device,
)

REQUIRED_OPTIONS = {
    "vb-cable-base",
    "vac-oem",
    "voicemeeter",
    "scream",
    "steelseries-sonar",
    "own-wdm-driver",
    "plan-b-video-only",
}
ALLOWED_LICENSE_MODELS = {
    "donationware",
    "proprietary_paid",
    "permissive_open_source",
    "own_driver",
    "plan_b_video_only",
}
ALLOWED_REDISTRIBUTION = {"yes_with_conditions", "no", "unverified"}
ALLOWED_SUPPORT = {"low", "medium", "high"}


def test_every_candidate_carries_the_required_assessment_fields() -> None:
    assert {option["option_id"] for option in CANDIDATES} == REQUIRED_OPTIONS

    for option in CANDIDATES:
        option_id = option["option_id"]
        for field in (
            "device_name",
            "vendor",
            "license_model",
            "terms_url",
            "terms_captured_text",
            "redistribution_allowed",
            "commercial_use_allowed",
            "unit_cost",
            "cost_model",
            "support_burden",
            "support_burden_justification",
            "user_experience",
            "signature_verified",
            "fallback",
        ):
            assert field in option, f"{option_id}: missing {field}"
        assert option["terms_url"].startswith("https://"), option_id
        assert len(option["terms_captured_text"]) > 80, option_id
        assert '"' in option["terms_captured_text"], option_id
        assert option["license_model"] in ALLOWED_LICENSE_MODELS, option_id
        assert option["redistribution_allowed"] in ALLOWED_REDISTRIBUTION, option_id
        assert option["support_burden"] in ALLOWED_SUPPORT, option_id
        assert isinstance(option["commercial_use_allowed"], bool), option_id
        assert isinstance(option["signature_verified"], bool), option_id
        assert option["unit_cost"] >= 0, option_id


def test_the_assessment_covers_redistribution_user_install_and_plan_b() -> None:
    assert set(OPTIONS_COMPARED) == {
        "redistribution_licensing",
        "user_installed_official_driver",
        "plan_b_video_only",
    }


def test_vb_cable_is_the_pinned_measurement_device_at_the_recorded_price() -> None:
    device = measurement_device()

    assert device["option_id"] == MEASUREMENT_OPTION
    assert device["device_name"] == "VB-CABLE"
    assert "driver pack 45" in device["version"]
    assert device["unit_cost"] == pytest.approx(3.61)
    assert device["redistribution_allowed"] == "yes_with_conditions"
    assert device["signature_verified"] is False
    assert "not verified" in device["signature_note"].lower()


def test_an_unverified_redistribution_position_is_recorded_explicitly() -> None:
    sonar = next(option for option in CANDIDATES if option["option_id"] == "steelseries-sonar")

    assert sonar["redistribution_allowed"] == "unverified"
    assert "unverified" in sonar["redistribution_note"].lower()


def test_the_default_bundle_is_a_draft_and_never_a_silent_pass() -> None:
    bundle = build_bundle()

    assert bundle["bundle_status"] == "draft"
    assert bundle["gate_verdict"] == "conditional"
    assert bundle["decision"]["status"] == "pending"
    assert bundle["selected_option"] is None
    assert bundle["unresolved"]
    assert bundle["conditions"]
    assert bundle["fallback"] == "video-only V1"
    assert bundle["remeasurement_required"] is False
    assert bundle["escalation"]["owner_role"] == "product_owner"


def test_a_final_bundle_is_refused_without_names_dates_and_a_selection() -> None:
    with pytest.raises(LicenseDecisionError, match="named owner"):
        build_bundle(require_final=True)

    with pytest.raises(LicenseDecisionError):
        build_bundle(
            selected_option=MEASUREMENT_OPTION,
            decision_owner="Owner A",
            approver="Approver B",
            decided_at=None,
            require_final=True,
        )


def test_a_signed_decision_selecting_the_measurement_device_is_go() -> None:
    bundle = build_bundle(
        selected_option=MEASUREMENT_OPTION,
        decision_owner="Owner A",
        approver="Approver B",
        decided_at="2026-11-05",
        require_final=True,
    )

    assert bundle["bundle_status"] == "final"
    assert bundle["decision"]["status"] == "recorded"
    assert bundle["decision"]["verdict"] == "go"
    assert bundle["gate_verdict"] == "go"
    assert bundle["unresolved"] == []
    assert bundle["conditions"] == []
    assert bundle["remeasurement_required"] is False


def test_selecting_another_device_forces_remeasurement() -> None:
    bundle = build_bundle(
        selected_option="vac-oem",
        decision_owner="Owner A",
        approver="Approver B",
        decided_at="2026-11-05",
        require_final=True,
    )

    assert bundle["remeasurement_required"] is True
    assert "re-measured" in bundle["remeasurement_rule"]


def test_an_unknown_selected_option_is_rejected() -> None:
    with pytest.raises(LicenseDecisionError, match="not in the catalogue"):
        build_bundle(selected_option="some-other-cable")


def test_the_measurement_device_is_also_a_candidate() -> None:
    bundle = build_bundle()

    ids = [option["option_id"] for option in bundle["candidate_options"]]
    assert MEASUREMENT_OPTION in ids
    assert bundle["measurement_device"]["option_id"] == MEASUREMENT_OPTION
