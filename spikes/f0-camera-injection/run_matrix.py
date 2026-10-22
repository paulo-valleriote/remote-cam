#!/usr/bin/env python3
"""Run or assemble the virtual camera injection matrix (T040).

Two modes, deliberately separate so that evidence is never invented:

``--captures-root DIR``
    Analyse captures that already exist. Every run directory must contain
    ``run.json`` (the exact product, version and bitness that was observed) and
    ``observations.csv`` (what the consumer actually received). Nothing is
    measured, nothing is fabricated.

*(no mode)*
    Drive the harnesses for the whole matrix, 30 minutes per cell, on the
    Windows 11 reference machine. The DirectShow fallback is evaluated only when
    at least one primary cell fails, and then for every application, so the
    matrix compares like with like.

The output is ``spikes/evidence/injection/injection_matrix.json`` validated
against ``contracts/injection-matrix.schema.json`` before it is written.

Usage::

    python spikes/f0-camera-injection/run_matrix.py
    python spikes/f0-camera-injection/run_matrix.py --captures-root spikes/evidence/injection/runs

**NON-PRODUCTION** — F0 risk-reduction spike.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Final

from jsonschema import Draft202012Validator, FormatChecker

_HERE = Path(__file__).resolve().parent
REPO_ROOT: Final = _HERE.parents[1]
_TOOLS_DIR = REPO_ROOT / "spikes" / "tools"
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))

from analysis.injection import (  # noqa: E402
    DSHOW_FILTER,
    MF_VIRTUAL_CAMERA,
    CellAnalysis,
    RequestedFormat,
    analyse_capture,
    build_matrix,
    cell_record,
    derive_selected_backend,
    read_observations_csv,
)

sys.path.insert(0, str(_HERE / "apps"))
from harness_common import HarnessError, finalise_run  # noqa: E402

DEFAULT_PLAN: Final = _HERE / "applications.json"
DEFAULT_OUTPUT: Final = REPO_ROOT / "spikes" / "evidence" / "injection" / "injection_matrix.json"
SCHEMA_PATH: Final = (
    REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "injection-matrix.schema.json"
)

HARNESSES: Final[dict[str, list[str]]] = {
    "browser": ["node", "apps/browser/capture.mjs"],
    "browser32": ["node", "apps/browser32/capture.mjs"],
    "obs": [sys.executable, "apps/obs/capture.py"],
    "window_capture": [sys.executable, "apps/window_capture.py"],
}

CAPTURE_METHODS: Final = {
    "browser": "getusermedia_mediarecorder",
    "browser32": "getusermedia_mediarecorder",
    "obs": "obs_websocket",
    "window_capture": "gdigrab_window_capture",
}

# Harness-specific options that the plan does not carry; a harness without an
# entry is invoked with the common argument surface only. Per-application
# `harness_args` from the plan are appended to these.
HARNESS_EXTRA_ARGS: Final[dict[str, list[str]]] = {
    "browser": ["--timeout-s", "10800"],
    "browser32": ["--timeout-s", "10800"],
    "obs": ["--frame-check-s", "10"],
    "window_capture": [],
}


class MatrixError(RuntimeError):
    """The matrix cannot be produced from the available evidence."""


@dataclass(frozen=True)
class RunRecord:
    backend: str
    application_id: str
    directory: Path
    metadata: dict[str, Any]
    analysis: CellAnalysis


def load_plan(path: Path | str, *, require_identity: bool = False) -> dict[str, Any]:
    plan = json.loads(Path(path).read_text(encoding="utf-8"))
    for key in ("applications", "backends", "requested", "run_duration_s"):
        if key not in plan:
            raise MatrixError(f"the plan is missing {key}")
    if len(plan["applications"]) < 5:
        raise MatrixError("the plan must cover the five application categories")

    for application in plan["applications"]:
        harness = str(application.get("harness", ""))
        if harness not in HARNESSES:
            raise MatrixError(
                f"{application['application_id']}: unknown harness {harness!r}; "
                f"expected one of {', '.join(sorted(HARNESSES))}"
            )
        if require_identity:
            # A live run must know exactly which product build it exercises; the
            # harness records it and the matrix repeats it as gate evidence.
            for field in ("product_name", "version"):
                if not str(application.get(field, "")).strip():
                    raise MatrixError(
                        f"{application['application_id']}: the plan must name the exact "
                        f"{field} before a live run, because it cannot be recovered afterwards"
                    )
    return plan


def requested_from(plan: dict[str, Any]) -> RequestedFormat:
    requested = plan["requested"]
    return RequestedFormat(
        width=int(requested["width"]),
        height=int(requested["height"]),
        frame_rate=int(requested["frame_rate"]),
    )


def load_run(
    directory: Path,
    *,
    plan: dict[str, Any],
    backend: str,
    allowed_backends: Iterable[str] = (MF_VIRTUAL_CAMERA, DSHOW_FILTER),
) -> RunRecord:
    """Load and cross-check one recorded run against the plan."""

    metadata_path = directory / "run.json"
    observations_path = directory / "observations.csv"
    if not metadata_path.is_file() or not observations_path.is_file():
        raise MatrixError(
            f"{directory} must contain run.json and observations.csv; "
            "a cell cannot be concluded from an absent capture"
        )

    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    if backend not in allowed_backends:
        raise MatrixError(f"unknown backend {backend!r}")
    if metadata.get("backend") != backend:
        raise MatrixError(
            f"{directory}: run.json reports backend {metadata.get('backend')!r}, "
            f"but the directory says {backend!r}"
        )

    application_id = str(metadata.get("application_id", ""))
    applications = {str(item["application_id"]): item for item in plan["applications"]}
    if application_id not in applications:
        raise MatrixError(f"{directory}: {application_id!r} is not in the plan")
    planned = applications[application_id]

    for field in ("category", "bitness"):
        if metadata.get(field) != planned[field]:
            raise MatrixError(
                f"{directory}: {field} is {metadata.get(field)!r} but the plan says "
                f"{planned[field]!r}"
            )
    for field in ("product_name", "version"):
        if not str(metadata.get(field, "")).strip():
            raise MatrixError(f"{directory}: the exact {field} must be recorded")
        planned_value = str(planned.get(field, "")).strip()
        if planned_value and metadata[field] != planned_value:
            raise MatrixError(
                f"{directory}: {field} is {metadata[field]!r} but the plan authorises "
                f"{planned_value!r}"
            )

    analysis = analyse_capture(
        read_observations_csv(observations_path),
        requested=requested_from(plan),
    )
    return RunRecord(
        backend=backend,
        application_id=application_id,
        directory=directory,
        metadata=metadata,
        analysis=analysis,
    )


def collect_runs(root: Path, plan: dict[str, Any]) -> list[RunRecord]:
    """Read every completed run under ``root/<backend>/<application_id>``."""

    runs: list[RunRecord] = []
    for backend in (MF_VIRTUAL_CAMERA, DSHOW_FILTER):
        backend_root = root / backend
        if not backend_root.is_dir():
            continue
        for directory in sorted(path for path in backend_root.iterdir() if path.is_dir()):
            runs.append(load_run(directory, plan=plan, backend=backend))
    if not runs:
        raise MatrixError(f"no completed runs were found under {root}")
    return runs


def cells_from_runs(
    runs: Iterable[RunRecord], *, requested: RequestedFormat
) -> list[dict[str, Any]]:
    return [
        cell_record(
            backend=run.backend,
            application_id=run.application_id,
            analysis=run.analysis,
            requested=requested,
            evidence_ref=relative(run.directory / "observations.csv"),
            discarded_frames=int(run.metadata.get("discarded_frames", 0)),
            notes=run.metadata.get("notes") or None,
        )
        for run in runs
    ]


def relative(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError as error:
        raise MatrixError(f"{path} is outside the repository") from error


def multi_consumer_observation(root: Path | None) -> dict[str, Any]:
    if root is not None:
        candidate = root / "multi_consumer.json"
        if candidate.is_file():
            observation = json.loads(candidate.read_text(encoding="utf-8"))
            return {
                "tested": bool(observation.get("tested", False)),
                "outcome": str(observation.get("outcome", "undefined")),
                **({"notes": str(observation["notes"])} if observation.get("notes") else {}),
            }
    return {
        "tested": False,
        "outcome": "not_tested",
        "notes": "the simultaneous-consumer scenario was not executed on this run",
    }


def licensing_impact(plan: dict[str, Any], selected: str | None) -> str:
    for backend in plan["backends"]:
        if backend["backend"] == selected:
            return (
                f"{selected} is distributed under {backend['licensing']}; "
                f"registration is {backend['registration']} and requires admin: "
                f"{backend['requires_admin']}."
            )
    return "No backend was selected, so no distribution impact can be stated."


MEDIA_PHASE_CONSEQUENCES: Final = (
    "The selected backend fixes the frame handoff contract between the decoded "
    "NV12 frame and the consumer: the ring buffer keeps NV12 1280x720@30 as the "
    "default and 1920x1080@30 as the optional maximum."
)


def assemble_matrix(
    *,
    plan: dict[str, Any],
    runs: list[RunRecord],
    environment_id: str,
    captured_at: str | None = None,
    multi_consumer: dict[str, Any] | None = None,
    decision_rationale: str | None = None,
) -> dict[str, Any]:
    requested = requested_from(plan)
    applications = [
        {
            "application_id": str(application["application_id"]),
            "category": str(application["category"]),
            "product_name": _recorded_field(runs, application, "product_name"),
            "version": _recorded_field(runs, application, "version"),
            "bitness": str(application["bitness"]),
            "capture_method": str(application["capture_method"]),
        }
        for application in plan["applications"]
        if any(run.application_id == application["application_id"] for run in runs)
    ]
    evaluated = {run.backend for run in runs}
    backends = [backend for backend in plan["backends"] if backend["backend"] in evaluated]
    cells = cells_from_runs(runs, requested=requested)

    matrix = build_matrix(
        environment_id=environment_id,
        backends=backends,
        applications=applications,
        cells=cells,
        decision_rationale=decision_rationale,
        captured_at=captured_at,
        licensing_impact=licensing_impact(plan, derive_selected_backend(cells)),
        media_phase_consequences=MEDIA_PHASE_CONSEQUENCES,
        multi_consumer_observation=multi_consumer or multi_consumer_observation(None),
    )
    validate_matrix(matrix)
    return matrix


def _recorded_field(runs: list[RunRecord], application: dict[str, Any], field: str) -> str:
    for run in runs:
        if run.application_id == application["application_id"]:
            return str(run.metadata[field])
    raise MatrixError(f"{application['application_id']} has no recorded run")


def validate_matrix(matrix: dict[str, Any]) -> None:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    errors = sorted(validator.iter_errors(matrix), key=lambda error: list(error.path))
    if errors:
        details = "; ".join(f"{list(error.path)}: {error.message}" for error in errors[:5])
        raise MatrixError(f"the matrix does not satisfy the contract: {details}")


def environment_id_from_evidence() -> str:
    path = REPO_ROOT / "spikes" / "evidence" / "environment.json"
    if path.is_file():
        return str(json.loads(path.read_text(encoding="utf-8")).get("environment_id", ""))
    return ""


def run_live(plan: dict[str, Any], *, duration_s: int, only: list[str] | None) -> list[RunRecord]:
    if sys.platform != "win32":
        raise MatrixError(
            "the injection matrix requires the Windows 11 reference machine: a virtual "
            "camera cannot be registered, measured or observed on this host. "
            "Pass --captures-root with a completed capture set to assemble a matrix offline."
        )

    applications = [
        application
        for application in plan["applications"]
        if only is None or application["application_id"] in only
    ]
    root = REPO_ROOT / "spikes" / "evidence" / "injection" / "runs"
    runs = _run_backend(plan, applications, MF_VIRTUAL_CAMERA, root, duration_s)

    if all(run.analysis.passed for run in runs):
        return runs

    print("at least one primary cell failed; evaluating the DirectShow fallback", file=sys.stderr)
    return runs + _run_backend(plan, applications, DSHOW_FILTER, root, duration_s)


def _run_backend(
    plan: dict[str, Any],
    applications: list[dict[str, Any]],
    backend: str,
    root: Path,
    duration_s: int,
) -> list[RunRecord]:
    runs: list[RunRecord] = []
    for application in applications:
        directory = root / backend / str(application["application_id"])
        directory.mkdir(parents=True, exist_ok=True)
        harness = str(application["harness"])
        command = HARNESSES[harness]
        invocation = [
            *command,
            "--backend",
            backend,
            "--application-id",
            str(application["application_id"]),
            "--duration-s",
            str(duration_s),
            "--output-dir",
            str(directory),
            # The plan names the exact product build, so the harness cannot
            # record a version the plan did not authorise.
            "--product-name",
            str(application["product_name"]),
            "--version",
            str(application["version"]),
            *HARNESS_EXTRA_ARGS.get(harness, []),
            *[str(item) for item in application.get("harness_args", [])],
        ]
        print(
            f"running {backend}/{application['application_id']}: {' '.join(invocation)}", flush=True
        )
        completed = subprocess.run(invocation, cwd=_HERE, check=False)
        if completed.returncode != 0:
            raise MatrixError(
                f"the harness for {application['application_id']} failed with "
                f"status {completed.returncode}; no cell is recorded for a failed run"
            )

        # The harness only records raw material; decoding and the run record
        # happen here, so every cell is judged by the same Python analyzer.
        try:
            finalise_run(directory, backend=backend)
        except HarnessError as error:
            raise MatrixError(f"{application['application_id']}: {error}") from error
        runs.append(load_run(directory, plan=plan, backend=backend))
    return runs


def write_matrix(matrix: dict[str, Any], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(matrix, indent=2, sort_keys=False) + "\n", encoding="utf-8")


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--captures-root", type=Path, default=None)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--duration-s", type=int, default=None, help="override the plan duration")
    parser.add_argument("--only", default=None, help="comma-separated application ids")
    parser.add_argument("--environment-id", default=None)
    parser.add_argument("--decision-rationale", default=None)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    try:
        live = args.captures_root is None
        plan = load_plan(args.plan, require_identity=live)
        duration_s = args.duration_s or int(plan["run_duration_s"])
        if duration_s < 1800:
            print(
                f"warning: a {duration_s}s run is shorter than the required 1800s; "
                "the cell cannot pass the gate",
                file=sys.stderr,
            )

        if not live:
            runs = collect_runs(args.captures_root, plan)
        else:
            only = [item.strip() for item in args.only.split(",")] if args.only else None
            runs = run_live(plan, duration_s=duration_s, only=only)

        matrix = assemble_matrix(
            plan=plan,
            runs=runs,
            environment_id=args.environment_id or environment_id_from_evidence(),
            captured_at=datetime.now(UTC).isoformat(timespec="seconds"),
            multi_consumer=multi_consumer_observation(args.captures_root),
            decision_rationale=args.decision_rationale,
        )
        write_matrix(matrix, args.output)
    except MatrixError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"wrote {relative(args.output)} with selected_backend={matrix['selected_backend']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
