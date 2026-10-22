"""Shared plumbing for the injection gate harnesses (T035-T038).

Every harness is deliberately dumb: it captures what one consumer application
actually received and writes ``capture.json`` next to the capture file. Turning
that into gate evidence — ``observations.csv`` and ``run.json`` — happens in one
place, [`finalise_run`], so no harness can invent a frame, a product version or
a duration.

The split matters for honesty: a harness cannot conclude anything, and the
decoder that judges a capture is the same Python code the matrix tests exercise.

**NON-PRODUCTION** — F0 risk-reduction spike.
"""

from __future__ import annotations

import argparse
import csv
import json
import shutil
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Final

_HERE = Path(__file__).resolve().parent
INJECTION_DIR: Final = _HERE.parent
REPO_ROOT: Final = INJECTION_DIR.parents[1]
_TOOLS_DIR: Final = REPO_ROOT / "spikes" / "tools"
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))

from analysis.injection import (  # noqa: E402
    DEFAULT_REQUESTED,
    FrameObservation,
    RequestedFormat,
)
from analysis.marker import observations_from_video  # noqa: E402

CAPTURE_JSON: Final = "capture.json"
RUN_JSON: Final = "run.json"
OBSERVATIONS_CSV: Final = "observations.csv"
PLAN_PATH: Final = INJECTION_DIR / "applications.json"

BACKENDS: Final = ("mf_virtual_camera", "dshow_filter")


class HarnessError(RuntimeError):
    """The harness cannot produce evidence for this run."""


@dataclass(frozen=True)
class HarnessOptions:
    backend: str
    application_id: str
    output_dir: Path
    duration_s: int
    product_name: str
    version: str
    extra: dict[str, Any]


def load_plan(path: Path | str = PLAN_PATH) -> dict[str, Any]:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def application(plan: dict[str, Any], application_id: str) -> dict[str, Any]:
    for item in plan["applications"]:
        if item["application_id"] == application_id:
            return item
    raise HarnessError(f"{application_id!r} is not in {PLAN_PATH.name}")


def requested_format(plan: dict[str, Any]) -> RequestedFormat:
    requested = plan["requested"]
    return RequestedFormat(
        width=int(requested["width"]),
        height=int(requested["height"]),
        frame_rate=int(requested["frame_rate"]),
    )


def harness_parser(description: str, *, default_duration_s: int = 1800) -> argparse.ArgumentParser:
    """The argument surface `run_matrix.py` invokes every harness with."""

    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--backend", required=True, choices=BACKENDS)
    parser.add_argument("--application-id", required=True)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--duration-s", type=int, default=default_duration_s)
    parser.add_argument(
        "--product-name",
        default="",
        help="the exact product that was exercised; recorded verbatim in run.json",
    )
    parser.add_argument(
        "--version",
        default="",
        help="the exact product version; recorded verbatim in run.json",
    )
    parser.add_argument(
        "--notes",
        default="",
        help="free-form observation recorded with the cell",
    )
    return parser


def options_from(
    namespace: argparse.Namespace, extra: dict[str, Any] | None = None
) -> HarnessOptions:
    if namespace.backend not in BACKENDS:
        raise HarnessError(f"unknown backend {namespace.backend!r}")
    if namespace.duration_s <= 0:
        raise HarnessError("--duration-s must be positive")
    return HarnessOptions(
        backend=namespace.backend,
        application_id=namespace.application_id,
        output_dir=Path(namespace.output_dir),
        duration_s=int(namespace.duration_s),
        product_name=namespace.product_name.strip(),
        version=namespace.version.strip(),
        extra={**(extra or {}), "notes": namespace.notes.strip()},
    )


def require_identity(options: HarnessOptions) -> None:
    """Refuse to record a run whose exact product and version are unknown.

    The gate's whole point is per-application compatibility, so "Chrome" is not
    evidence; "Google Chrome 142.0.7444.60" is.
    """

    missing = [
        name
        for name, value in (
            ("--product-name", options.product_name),
            ("--version", options.version),
        )
        if not value
    ]
    if missing:
        raise HarnessError(
            "the exact product and version must be supplied ("
            + ", ".join(missing)
            + f"); they are recorded in {RUN_JSON} and cannot be inferred after the fact"
        )


def write_capture_json(
    options: HarnessOptions,
    *,
    capture_file: str,
    capture_method: str,
    recorded_duration_s: float,
    bitness: str | None = None,
    discarded_frames: int = 0,
    observed: dict[str, Any] | None = None,
) -> Path:
    """Write the raw record of one harness run. Nothing is judged here."""

    plan = load_plan()
    planned = application(plan, options.application_id)
    requested = requested_format(plan)

    if options.product_name and bitness not in (None, planned["bitness"]):
        raise HarnessError(
            f"{options.application_id} is planned as {planned['bitness']} but the run "
            f"observed {bitness}; a cell cannot change bitness mid-run"
        )

    record: dict[str, Any] = {
        "application_id": options.application_id,
        "category": planned["category"],
        "bitness": bitness or planned["bitness"],
        "planned_bitness": planned["bitness"],
        "capture_method": capture_method,
        "capture_file": capture_file,
        "product_name": options.product_name,
        "version": options.version,
        "requested": {
            "width": requested.width,
            "height": requested.height,
            "frame_rate": requested.frame_rate,
        },
        "recorded_duration_s": round(float(recorded_duration_s), 3),
        "discarded_frames": int(discarded_frames),
        "captured_at": datetime.now(UTC).isoformat(timespec="seconds"),
    }
    if options.extra.get("notes"):
        record["notes"] = options.extra["notes"]
    if observed:
        record["observed"] = observed

    options.output_dir.mkdir(parents=True, exist_ok=True)
    path = options.output_dir / CAPTURE_JSON
    path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return path


def environment_id() -> str:
    path = REPO_ROOT / "spikes" / "evidence" / "environment.json"
    if path.is_file():
        return str(json.loads(path.read_text(encoding="utf-8")).get("environment_id", ""))
    return ""


def finalise_run(directory: Path, *, backend: str) -> Path:
    """Turn ``capture.json`` plus a capture file into ``run.json`` + CSV.

    Fails when the capture cannot be decoded: the gate must see an unreadable
    capture as a failure, never as a clean run with no data.
    """

    if backend not in BACKENDS:
        raise HarnessError(f"unknown backend {backend!r}")
    capture_path = directory / CAPTURE_JSON
    failure_path = directory / "failure.json"
    if not capture_path.is_file():
        if failure_path.is_file():
            return _finalise_failure(directory, backend=backend, failure_path=failure_path)
        raise HarnessError(f"{directory} has no {CAPTURE_JSON}: the harness did not record a run")
    capture = json.loads(capture_path.read_text(encoding="utf-8"))

    for field in ("product_name", "version"):
        if not str(capture.get(field, "")).strip():
            raise HarnessError(
                f"{directory}: {field} is empty; the exact product and version are gate evidence"
            )

    capture_file = str(capture.get("capture_file", ""))
    if not capture_file:
        raise HarnessError(f"{directory}: {CAPTURE_JSON} does not name a capture file")
    media_path = directory / capture_file
    if not media_path.is_file():
        raise HarnessError(f"{directory}: the capture file {capture_file} does not exist")

    # A harness that can time the frames itself (the browser page uses
    # requestVideoFrameCallback) writes its own CSV; that observation is
    # stronger than anything recoverable from the file afterwards, so it wins.
    live_csv = directory / OBSERVATIONS_CSV
    if live_csv.is_file():
        observations = read_observations_rows(live_csv)
        source = "harness_observation"
    else:
        if shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None:
            raise HarnessError(
                "ffmpeg and ffprobe are required to decode a capture; install FFmpeg or copy "
                "the run to the reference machine and finalise it there"
            )
        observations = observations_from_video(media_path)
        write_observations(directory / OBSERVATIONS_CSV, observations)
        source = "capture_decode"

    if not observations:
        raise HarnessError(
            f"{directory}: the capture contains no decoded frame; an empty capture is not "
            "a clean run. Record a device_not_visible failure instead."
        )

    run = {
        "backend": backend,
        "application_id": capture["application_id"],
        "category": capture["category"],
        "bitness": capture["bitness"],
        "planned_bitness": capture.get("planned_bitness", capture["bitness"]),
        "capture_method": capture["capture_method"],
        "capture_file": capture_file,
        "product_name": capture["product_name"],
        "version": capture["version"],
        "requested": capture.get("requested"),
        "recorded_duration_s": capture.get("recorded_duration_s"),
        "observations_source": source,
        "decoded_frames": len(observations),
        "decoded_markers": sum(1 for item in observations if item.frame_no is not None),
        "discarded_frames": int(capture.get("discarded_frames", 0)),
        "environment_id": environment_id(),
        "completed_at": datetime.now(UTC).isoformat(timespec="seconds"),
    }
    for optional in ("notes", "observed"):
        if capture.get(optional):
            run[optional] = capture[optional]

    run_path = directory / RUN_JSON
    run_path.write_text(json.dumps(run, indent=2) + "\n", encoding="utf-8")
    return run_path


def _finalise_failure(directory: Path, *, backend: str, failure_path: Path) -> Path:
    """Record a run where the consumer could not open the camera at all.

    The empty CSV is the evidence: `analyse_capture` turns it into a
    ``device_not_visible`` cell with the reason ``no_frames_observed``, which is
    a matrix failure rather than an absent measurement.
    """

    failure = json.loads(failure_path.read_text(encoding="utf-8"))
    application_id = str(failure.get("application_id", ""))
    if not application_id:
        raise HarnessError(f"{failure_path} does not name the application that failed")

    plan = load_plan()
    planned = application(plan, application_id)
    requested = requested_format(plan)
    for field in ("product_name", "version"):
        if not str(failure.get(field, "")).strip():
            raise HarnessError(
                f"{failure_path}: {field} is empty; even a failure must record the exact product"
            )

    write_observations(directory / OBSERVATIONS_CSV, [])
    run = {
        "backend": backend,
        "application_id": application_id,
        "category": planned["category"],
        "bitness": planned["bitness"],
        "planned_bitness": planned["bitness"],
        "capture_method": planned["capture_method"],
        "capture_file": None,
        "product_name": failure["product_name"],
        "version": failure["version"],
        "requested": {
            "width": requested.width,
            "height": requested.height,
            "frame_rate": requested.frame_rate,
        },
        "recorded_duration_s": 0.0,
        "observations_source": "harness_failure",
        "decoded_frames": 0,
        "decoded_markers": 0,
        "discarded_frames": 0,
        "environment_id": environment_id(),
        "completed_at": datetime.now(UTC).isoformat(timespec="seconds"),
        "notes": str(failure.get("reason", "")),
    }
    run_path = directory / RUN_JSON
    run_path.write_text(json.dumps(run, indent=2) + "\n", encoding="utf-8")
    return run_path


def read_observations_rows(path: Path) -> list[FrameObservation]:
    """Read a harness-written observation CSV without judging it."""

    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        required = {"capture_host_ms", "width", "height"}
        missing = required - set(reader.fieldnames or [])
        if missing:
            raise HarnessError(f"{path} is missing columns: {', '.join(sorted(missing))}")
        observations: list[FrameObservation] = []
        for row in reader:
            marker = row.get("frame_no")
            observations.append(
                FrameObservation(
                    capture_host_ms=float(row["capture_host_ms"]),
                    width=int(row["width"]),
                    height=int(row["height"]),
                    frame_no=int(marker) if marker not in (None, "") else None,
                )
            )
    return observations


def write_observations(path: Path, observations: Sequence[FrameObservation]) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(["capture_host_ms", "width", "height", "frame_no"])
        for item in observations:
            writer.writerow(
                [
                    f"{item.capture_host_ms:.3f}",
                    item.width,
                    item.height,
                    "" if item.frame_no is None else item.frame_no,
                ]
            )


def requested_from_plan_or_default() -> RequestedFormat:
    try:
        return requested_format(load_plan())
    except (OSError, KeyError, ValueError):
        return DEFAULT_REQUESTED
