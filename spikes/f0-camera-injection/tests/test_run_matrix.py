"""Runner and harness-contract tests for the injection gate (T040).

The runner is the only component that turns harness output into gate evidence,
so its refusal rules are the ones that matter: it must never conclude a cell
from an absent capture, never accept a product identity the plan did not
authorise, and never write a matrix that the contract rejects.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator, FormatChecker

REPO_ROOT = Path(__file__).resolve().parents[3]
INJECTION_DIR = REPO_ROOT / "spikes" / "f0-camera-injection"
APPS_DIR = INJECTION_DIR / "apps"
for path in (INJECTION_DIR, APPS_DIR, REPO_ROOT / "spikes" / "tools"):
    if str(path) not in sys.path:
        sys.path.insert(0, str(path))

from harness_common import HarnessError, finalise_run, write_observations  # noqa: E402

import run_matrix  # noqa: E402
from analysis.injection import FrameObservation  # noqa: E402

SCHEMA_PATH = (
    REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "injection-matrix.schema.json"
)


def plan_document() -> dict:
    """A complete plan, with the identity fields the live mode demands."""

    return {
        "plan_version": 1,
        "run_duration_s": 1800,
        "requested": {"width": 1280, "height": 720, "frame_rate": 30},
        "backends": [
            {
                "backend": "mf_virtual_camera",
                "version": "0.1.0-spike",
                "registration": "per_user",
                "requires_admin": False,
                "installer_required": "none",
                "licensing": "MIT",
                "license_file_present": True,
                "source_reference": "spikes",
            }
        ],
        "applications": [
            {
                "application_id": "chrome",
                "category": "chromium_browser",
                "bitness": "64_bit",
                "product_name": "Google Chrome",
                "version": "142.0.7444.60",
                "harness": "browser",
                "capture_method": "getusermedia_mediarecorder",
            },
            {
                "application_id": "chrome32",
                "category": "chromium_browser",
                "bitness": "32_bit",
                "product_name": "Google Chrome (32-bit)",
                "version": "142.0.7444.60",
                "harness": "browser32",
                "capture_method": "getusermedia_mediarecorder",
            },
            {
                "application_id": "conferencing",
                "category": "video_conferencing_client",
                "bitness": "64_bit",
                "product_name": "Contoso Meet",
                "version": "7.0.0",
                "harness": "window_capture",
                "capture_method": "gdigrab_window_capture",
            },
            {
                "application_id": "obs",
                "category": "obs",
                "bitness": "64_bit",
                "product_name": "OBS Studio",
                "version": "32.0.0",
                "harness": "obs",
                "capture_method": "obs_websocket",
            },
            {
                "application_id": "windows_camera",
                "category": "windows_camera_app",
                "bitness": "64_bit",
                "product_name": "Windows Camera",
                "version": "2026.0",
                "harness": "window_capture",
                "capture_method": "gdigrab_window_capture",
            },
            {
                "application_id": "screen_recorder",
                "category": "screen_recorder",
                "bitness": "64_bit",
                "product_name": "Contoso Recorder",
                "version": "1.2.3",
                "harness": "window_capture",
                "capture_method": "app_native_recording",
            },
        ],
    }


def write_plan(tmp_path: Path, plan: dict) -> Path:
    path = tmp_path / "applications.json"
    path.write_text(json.dumps(plan), encoding="utf-8")
    return path


def write_observations_csv(directory: Path, frames: int = 60, *, start_ms: float = 0.0) -> None:
    rows = ["capture_host_ms,width,height,frame_no"]
    for index in range(frames):
        rows.append(f"{start_ms + index * 1000 / 30:.3f},1280,720,{index}")
    (directory / "observations.csv").write_text("\n".join(rows) + "\n", encoding="utf-8")


def write_run_json(directory: Path, application: dict, backend: str = "mf_virtual_camera") -> None:
    payload = {
        "backend": backend,
        "application_id": application["application_id"],
        "category": application["category"],
        "bitness": application["bitness"],
        "capture_method": application["capture_method"],
        "product_name": application["product_name"],
        "version": application["version"],
    }
    (directory / "run.json").write_text(json.dumps(payload), encoding="utf-8")


# ---------------------------------------------------------------------------
# Plan rules
# ---------------------------------------------------------------------------


def test_a_live_run_requires_the_exact_product_and_version(tmp_path: Path) -> None:
    plan = plan_document()
    plan["applications"][0]["version"] = ""
    path = write_plan(tmp_path, plan)

    with pytest.raises(run_matrix.MatrixError, match="exact version"):
        run_matrix.load_plan(path, require_identity=True)

    # Offline assembly tolerates an unfilled plan: the identity comes from the
    # recorded runs instead, which is the point of the two modes.
    run_matrix.load_plan(path)


def test_an_unknown_harness_is_refused(tmp_path: Path) -> None:
    plan = plan_document()
    plan["applications"][2]["harness"] = "telepathy"
    path = write_plan(tmp_path, plan)

    with pytest.raises(run_matrix.MatrixError, match="unknown harness"):
        run_matrix.load_plan(path)


def test_the_shipped_plan_covers_every_category_and_both_bitnesses() -> None:
    plan = run_matrix.load_plan(run_matrix.DEFAULT_PLAN)

    categories = {item["category"] for item in plan["applications"]}
    bitnesses = {item["bitness"] for item in plan["applications"]}

    assert categories == {
        "chromium_browser",
        "video_conferencing_client",
        "obs",
        "windows_camera_app",
        "screen_recorder",
    }
    assert bitnesses == {"32_bit", "64_bit"}
    # Every shipped application must name a harness the runner can invoke.
    assert {item["harness"] for item in plan["applications"]} <= set(run_matrix.HARNESSES)


# ---------------------------------------------------------------------------
# Run loading rules
# ---------------------------------------------------------------------------


def test_a_cell_cannot_be_concluded_without_a_capture(tmp_path: Path) -> None:
    plan = plan_document()
    application = plan["applications"][0]
    directory = tmp_path / "chrome"
    directory.mkdir()
    write_run_json(directory, application)

    with pytest.raises(run_matrix.MatrixError, match="observations.csv"):
        run_matrix.load_run(directory, plan=plan, backend="mf_virtual_camera")


def test_the_backend_in_the_run_must_match_the_directory(tmp_path: Path) -> None:
    plan = plan_document()
    application = plan["applications"][0]
    directory = tmp_path / "chrome"
    directory.mkdir()
    write_run_json(directory, application, backend="dshow_filter")
    write_observations_csv(directory)

    with pytest.raises(run_matrix.MatrixError, match="run.json reports backend"):
        run_matrix.load_run(directory, plan=plan, backend="mf_virtual_camera")


def test_the_product_version_must_be_the_one_the_plan_authorised(tmp_path: Path) -> None:
    plan = plan_document()
    application = plan["applications"][0]
    directory = tmp_path / "chrome"
    directory.mkdir()
    write_run_json(directory, {**application, "version": "141.0.0.0"})
    write_observations_csv(directory)

    with pytest.raises(run_matrix.MatrixError, match="plan authorises"):
        run_matrix.load_run(directory, plan=plan, backend="mf_virtual_camera")


def test_a_matching_run_is_accepted(tmp_path: Path) -> None:
    plan = plan_document()
    application = plan["applications"][0]
    directory = tmp_path / "chrome"
    directory.mkdir()
    write_run_json(directory, application)
    write_observations_csv(directory)

    record = run_matrix.load_run(directory, plan=plan, backend="mf_virtual_camera")

    assert record.application_id == "chrome"
    assert record.analysis.frames_observed == 60


def test_the_live_mode_refuses_a_non_windows_host() -> None:
    plan = plan_document()

    if sys.platform == "win32":
        pytest.skip("this assertion only applies off the reference machine")

    with pytest.raises(run_matrix.MatrixError, match="Windows 11 reference machine"):
        run_matrix.run_live(plan, duration_s=1800, only=["chrome"])


# ---------------------------------------------------------------------------
# Finalisation rules
# ---------------------------------------------------------------------------


def test_a_harness_observation_wins_over_decoding_the_file(tmp_path: Path) -> None:
    """The browser page times frames as they are presented; that beats a file."""

    directory = tmp_path / "chrome"
    directory.mkdir()
    capture = {
        "application_id": "chrome",
        "category": "chromium_browser",
        "bitness": "64_bit",
        "capture_method": "getusermedia_mediarecorder",
        "capture_file": "capture.webm",
        "product_name": "Google Chrome",
        "version": "142.0.7444.60",
        "recorded_duration_s": 2.0,
    }
    (directory / "capture.json").write_text(json.dumps(capture), encoding="utf-8")
    (directory / "capture.webm").write_bytes(b"not decoded, because the CSV exists")
    write_observations_csv(directory, frames=45)

    run = json.loads(
        finalise_run(directory, backend="mf_virtual_camera").read_text(encoding="utf-8")
    )

    assert run["observations_source"] == "harness_observation"
    assert run["decoded_frames"] == 45
    assert run["decoded_markers"] == 45


def test_an_empty_capture_is_not_a_clean_run(tmp_path: Path) -> None:
    directory = tmp_path / "chrome"
    directory.mkdir()
    capture = {
        "application_id": "chrome",
        "category": "chromium_browser",
        "bitness": "64_bit",
        "capture_method": "getusermedia_mediarecorder",
        "capture_file": "capture.webm",
        "product_name": "Google Chrome",
        "version": "142.0.7444.60",
        "recorded_duration_s": 0.0,
    }
    (directory / "capture.json").write_text(json.dumps(capture), encoding="utf-8")
    (directory / "capture.webm").write_bytes(b"")
    write_observations_csv(directory, frames=0)

    with pytest.raises(HarnessError, match="empty capture"):
        finalise_run(directory, backend="mf_virtual_camera")


def test_a_recorded_failure_becomes_an_empty_observations_file(tmp_path: Path) -> None:
    directory = tmp_path / "chrome32"
    directory.mkdir()
    failure = {
        "application_id": "chrome32",
        "product_name": "Google Chrome (32-bit)",
        "version": "142.0.7444.60",
        "reason": "NotReadableError: could not start the video source",
    }
    (directory / "failure.json").write_text(json.dumps(failure), encoding="utf-8")

    run = json.loads(
        finalise_run(directory, backend="mf_virtual_camera").read_text(encoding="utf-8")
    )

    assert run["observations_source"] == "harness_failure"
    assert run["bitness"] == "32_bit"
    assert run["decoded_frames"] == 0
    assert "NotReadableError" in run["notes"]
    # An empty observation set must analyse as a visible failure, not a pass.
    analysis = run_matrix.analyse_capture(
        run_matrix.read_observations_csv(directory / "observations.csv")
    )
    assert analysis.outcome == "device_not_visible"
    assert not analysis.passed


def test_a_failure_without_an_identity_is_refused(tmp_path: Path) -> None:
    directory = tmp_path / "chrome32"
    directory.mkdir()
    (directory / "failure.json").write_text(
        json.dumps({"application_id": "chrome32", "reason": "camera missing"}), encoding="utf-8"
    )

    with pytest.raises(HarnessError, match="exact product"):
        finalise_run(directory, backend="mf_virtual_camera")


def test_a_harness_that_recorded_nothing_is_refused(tmp_path: Path) -> None:
    directory = tmp_path / "chrome"
    directory.mkdir()

    with pytest.raises(HarnessError, match="did not record a run"):
        finalise_run(directory, backend="mf_virtual_camera")


# ---------------------------------------------------------------------------
# End-to-end offline assembly
# ---------------------------------------------------------------------------


def build_capture_set(tmp_path: Path, plan: dict, *, backend: str) -> Path:
    root = tmp_path / "runs"
    for application in plan["applications"]:
        directory = root / backend / application["application_id"]
        directory.mkdir(parents=True)
        # 1800 s at 30 fps, which is the minimum the gate accepts.
        observations = []
        for index in range(1800 * 30 + 1):
            observations.append(
                FrameObservation(
                    capture_host_ms=index * 1000 / 30,
                    width=1280,
                    height=720,
                    frame_no=index % 65_536,
                )
            )
        write_observations(directory / "observations.csv", observations)
        write_run_json(directory, application, backend=backend)
    return root


def test_offline_assembly_produces_a_valid_matrix(tmp_path: Path, monkeypatch) -> None:
    plan = plan_document()
    root = build_capture_set(tmp_path, plan, backend="mf_virtual_camera")
    # The runner refuses evidence outside the repository, so the test pretends
    # the temporary directory is the repository root.
    monkeypatch.setattr(run_matrix, "REPO_ROOT", tmp_path)

    runs = run_matrix.collect_runs(root, plan)
    matrix = run_matrix.assemble_matrix(
        plan=plan,
        runs=runs,
        environment_id="win11-reference",
        captured_at="2026-11-02T12:00:00+00:00",
    )

    Draft202012Validator(
        json.loads(SCHEMA_PATH.read_text(encoding="utf-8")), format_checker=FormatChecker()
    ).validate(matrix)
    assert matrix["selected_backend"] == "mf_virtual_camera"
    assert len(matrix["cells"]) == len(plan["applications"])
    assert matrix["multi_consumer_observation"]["outcome"] == "not_tested"
    assert {cell["outcome"] for cell in matrix["cells"]} == {"frames_delivered"}


def test_offline_assembly_without_any_run_is_refused(tmp_path: Path) -> None:
    plan = plan_document()
    empty = tmp_path / "runs"
    empty.mkdir()

    with pytest.raises(run_matrix.MatrixError, match="no completed runs"):
        run_matrix.collect_runs(empty, plan)
