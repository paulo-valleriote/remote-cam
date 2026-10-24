"""Matrix-contract tests for the injection gate (T027).

The gate is only meaningful if the artefact it produces satisfies the versioned
contract: complete coverage of every evaluated backend, a reason on every
``not_tested`` cell, and a backend decision that can be derived from the cells
rather than asserted.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator, FormatChecker

from analysis.injection import (
    DEFAULT_REQUESTED,
    CellAnalysis,
    FrameObservation,
    RequestedFormat,
    analyse_capture,
    build_matrix,
    cell_record,
    derive_selected_backend,
    not_tested_cell,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
SCHEMA_PATH = (
    REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "injection-matrix.schema.json"
)


def schema_validator() -> Draft202012Validator:
    return Draft202012Validator(
        json.loads(SCHEMA_PATH.read_text(encoding="utf-8")),
        format_checker=FormatChecker(),
    )


APPLICATIONS = [
    {
        "application_id": "chrome",
        "category": "chromium_browser",
        "product_name": "Google Chrome",
        "version": "142.0.7444.60",
        "bitness": "64_bit",
        "capture_method": "getusermedia_mediarecorder",
    },
    {
        "application_id": "conferencing",
        "category": "video_conferencing_client",
        "product_name": "Reference conferencing client",
        "version": "7.0.0",
        "bitness": "64_bit",
        "capture_method": "gdigrab_window_capture",
    },
    {
        "application_id": "obs",
        "category": "obs",
        "product_name": "OBS Studio",
        "version": "32.0.0",
        "bitness": "64_bit",
        "capture_method": "obs_websocket",
    },
    {
        "application_id": "windows_camera",
        "category": "windows_camera_app",
        "product_name": "Windows Camera",
        "version": "2026.0",
        "bitness": "64_bit",
        "capture_method": "gdigrab_window_capture",
    },
    {
        "application_id": "browser32",
        "category": "screen_recorder",
        "product_name": "Reference 32-bit recorder",
        "version": "1.2.3",
        "bitness": "32_bit",
        "capture_method": "app_native_recording",
    },
]

BACKENDS = [
    {
        "backend": "mf_virtual_camera",
        "version": "0.1.0-spike",
        "registration": "per_user",
        "requires_admin": False,
        "installer_required": "none",
        "licensing": "MIT",
        "license_file_present": True,
        "source_reference": "microsoft/Windows-Camera Samples/VirtualCamera",
    },
    {
        "backend": "dshow_filter",
        "version": "0.1.0-spike",
        "registration": "per_user",
        "requires_admin": False,
        "installer_required": "none",
        "licensing": "proprietary",
        "license_file_present": False,
        "source_reference": "spikes/f0-dshow-vcam-fallback",
    },
]


def passing_analysis() -> CellAnalysis:
    interval_ms = 1000.0 / 30.0
    observations = [
        FrameObservation(
            capture_host_ms=index * interval_ms,
            frame_no=index % 65_536,
            width=1280,
            height=720,
        )
        for index in range(int(1800 * 30) + 1)
    ]
    return analyse_capture(observations)


def failing_analysis() -> CellAnalysis:
    return analyse_capture([])


def passing_cells(backend: str) -> list[dict[str, object]]:
    analysis = passing_analysis()
    return [
        cell_record(
            backend=backend,
            application_id=application["application_id"],
            analysis=analysis,
            evidence_ref=f"spikes/evidence/injection/{backend}-{application['application_id']}.json",
        )
        for application in APPLICATIONS
    ]


def matrix(cells: list[dict[str, object]], **overrides: object) -> dict[str, object]:
    arguments = {
        "environment_id": "win11-reference",
        "backends": BACKENDS,
        "applications": APPLICATIONS,
        "cells": cells,
        "licensing_impact": "MF virtual camera is MIT; the DirectShow fallback is project-owned.",
        "media_phase_consequences": "The selected backend fixes the frame handoff contract.",
        "multi_consumer_observation": {"tested": True, "outcome": "both_worked"},
        "captured_at": "2026-10-30T12:00:00+00:00",
    }
    arguments.update(overrides)
    return build_matrix(**arguments)


def test_a_produced_matrix_validates_against_the_contract() -> None:
    produced = matrix(passing_cells("mf_virtual_camera") + passing_cells("dshow_filter"))

    schema_validator().validate(produced)

    assert produced["matrix_version"] == 1
    assert produced["selected_backend"] == "mf_virtual_camera"


def test_every_evaluated_backend_covers_every_application() -> None:
    incomplete = passing_cells("mf_virtual_camera")[:-1] + passing_cells("dshow_filter")

    with pytest.raises(ValueError, match="incomplete"):
        matrix(incomplete)


def test_a_not_tested_cell_must_carry_a_reason() -> None:
    with pytest.raises(ValueError, match="reason"):
        not_tested_cell(
            backend="dshow_filter",
            application_id="chrome",
            reason="",
            evidence_ref="spikes/evidence/injection/not-tested.json",
        )

    cells = passing_cells("mf_virtual_camera")
    cells[-1] = not_tested_cell(
        backend="mf_virtual_camera",
        application_id="browser32",
        reason="the 32-bit build was not available on the run date",
        evidence_ref="spikes/evidence/injection/mf-browser32.json",
    )
    cells += passing_cells("dshow_filter")

    produced = matrix(cells)

    schema_validator().validate(produced)
    assert produced["selected_backend"] == "dshow_filter"
    assert produced["cells"][4]["not_tested_reason"]


def test_the_selected_backend_is_derivable_from_the_cells() -> None:
    primary_failure = []
    for application in APPLICATIONS:
        failure = failing_analysis()
        primary_failure.append(
            cell_record(
                backend="mf_virtual_camera",
                application_id=application["application_id"],
                analysis=failure,
                evidence_ref=f"spikes/evidence/injection/mf-{application['application_id']}.json",
            )
        )
    cells = primary_failure + passing_cells("dshow_filter")

    produced = matrix(cells)

    schema_validator().validate(produced)
    assert produced["selected_backend"] == "dshow_filter"
    assert derive_selected_backend(cells) == "dshow_filter"


def test_no_backend_is_selected_when_every_backend_failed() -> None:
    cells = []
    for backend in ("mf_virtual_camera", "dshow_filter"):
        for application in APPLICATIONS:
            cells.append(
                cell_record(
                    backend=backend,
                    application_id=application["application_id"],
                    analysis=failing_analysis(),
                    evidence_ref=(
                        f"spikes/evidence/injection/{backend}-{application['application_id']}.json"
                    ),
                )
            )

    # selected_backend has no null member in the closed enumeration: a matrix
    # without a decision cannot satisfy the contract.
    with pytest.raises(ValueError, match="decided backend"):
        matrix(cells)


def test_the_minimum_matrix_requires_all_five_categories() -> None:
    with pytest.raises(ValueError, match="five application categories"):
        matrix(passing_cells("mf_virtual_camera"), applications=APPLICATIONS[:4])


def test_both_bitnesses_must_be_represented() -> None:
    all_64 = [dict(application, bitness="64_bit") for application in APPLICATIONS]

    with pytest.raises(ValueError, match="32-bit and a 64-bit"):
        matrix(passing_cells("mf_virtual_camera"), applications=all_64)


def test_a_two_thousand_millisecond_stall_is_accepted_by_the_schema() -> None:
    analysis = analyse_capture(
        [
            FrameObservation(capture_host_ms=0.0, width=1280, height=720, frame_no=0),
            FrameObservation(capture_host_ms=33.0, width=1280, height=720, frame_no=1),
            FrameObservation(capture_host_ms=2033.0, width=1280, height=720, frame_no=2),
        ],
        requested=RequestedFormat(frame_rate=1),
        min_duration_s=2.0,
    )

    cell = cell_record(
        backend="mf_virtual_camera",
        application_id="chrome",
        analysis=analysis,
        evidence_ref="spikes/evidence/injection/stall.json",
    )

    assert analysis.longest_stall_ms == pytest.approx(2000.0)
    assert cell["outcome"] == "frames_delivered"


def test_the_analyzer_uses_the_product_default_format() -> None:
    assert DEFAULT_REQUESTED.width == 1280
    assert DEFAULT_REQUESTED.height == 720
    assert DEFAULT_REQUESTED.frame_rate == 30
