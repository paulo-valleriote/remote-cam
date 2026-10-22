"""Continuity analysis and matrix assembly for the injection gate (T039).

The gate is judged on what a consumer application *actually received*: frame
timestamps, the per-frame marker decoded from its capture, the delivered size
and the delivered rate. This module is pure — it never opens a device, a file
or a socket — so the gate rules can be tested without a Windows machine.

Contract references:

* ``specs/001-f0-technical-spikes/contracts/injection-matrix.schema.json``
* ``specs/001-f0-technical-spikes/contracts/measurement-formats.md`` §10
* ``specs/001-f0-technical-spikes/data-model.md`` §3.1-§3.2
"""

from __future__ import annotations

import csv
from collections import Counter
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final

MATRIX_VERSION: Final = 1
MIN_OBSERVED_DURATION_S: Final = 1800.0
MAX_STALL_MS: Final = 2000.0
FRAME_RATE_TOLERANCE: Final = 0.10
MARKER_WRAP: Final = 1 << 16

REQUIRED_CATEGORIES: Final = frozenset(
    {
        "chromium_browser",
        "video_conferencing_client",
        "obs",
        "windows_camera_app",
        "screen_recorder",
    }
)
REQUIRED_BITNESSES: Final = frozenset({"32_bit", "64_bit"})

MF_VIRTUAL_CAMERA: Final = "mf_virtual_camera"
DSHOW_FILTER: Final = "dshow_filter"

FRAMES_DELIVERED: Final = "frames_delivered"
DEVICE_NOT_VISIBLE: Final = "device_not_visible"
FRAMES_UNSTABLE: Final = "frames_unstable"
NOT_TESTED: Final = "not_tested"


@dataclass(frozen=True)
class RequestedFormat:
    width: int = 1280
    height: int = 720
    frame_rate: int = 30

    def __post_init__(self) -> None:
        if self.width <= 0 or self.height <= 0 or self.frame_rate <= 0:
            raise ValueError("a requested format needs positive dimensions and frame rate")


DEFAULT_REQUESTED: Final = RequestedFormat()


@dataclass(frozen=True)
class FrameObservation:
    """One frame as observed by the consumer application."""

    capture_host_ms: float
    width: int
    height: int
    frame_no: int | None = None


@dataclass(frozen=True)
class CellAnalysis:
    outcome: str
    observed_duration_s: float
    observed_fps: float
    delivered_width: int
    delivered_height: int
    frame_size_ok: bool
    frame_rate_ok: bool
    longest_stall_ms: float
    duplicate_frames: int
    missing_frames: int
    unreadable_markers: int
    frames_observed: int
    reasons: tuple[str, ...]

    @property
    def passed(self) -> bool:
        return self.outcome == FRAMES_DELIVERED


def analyse_capture(
    observations: Sequence[FrameObservation],
    *,
    requested: RequestedFormat = DEFAULT_REQUESTED,
    min_duration_s: float = MIN_OBSERVED_DURATION_S,
    max_stall_ms: float = MAX_STALL_MS,
    frame_rate_tolerance: float = FRAME_RATE_TOLERANCE,
) -> CellAnalysis:
    """Judge one application/backend capture against the gate criteria.

    A cell passes only when frames were delivered for at least
    ``min_duration_s``, at the requested size and rate, without a gap longer
    than ``max_stall_ms`` and without unreadable or missing frame markers.
    Duplicate frames are counted but are not a failure.
    """

    if min_duration_s < 0:
        raise ValueError("min_duration_s must be non-negative")
    if max_stall_ms <= 0:
        raise ValueError("max_stall_ms must be positive")
    if not 0.0 <= frame_rate_tolerance < 1.0:
        raise ValueError("frame_rate_tolerance must be within [0, 1)")

    frames = sorted(observations, key=lambda item: item.capture_host_ms)
    if not frames:
        return CellAnalysis(
            outcome=DEVICE_NOT_VISIBLE,
            observed_duration_s=0.0,
            observed_fps=0.0,
            delivered_width=0,
            delivered_height=0,
            frame_size_ok=False,
            frame_rate_ok=False,
            longest_stall_ms=0.0,
            duplicate_frames=0,
            missing_frames=0,
            unreadable_markers=0,
            frames_observed=0,
            reasons=("no_frames_observed",),
        )

    first = frames[0].capture_host_ms
    duration_s = max(0.0, (frames[-1].capture_host_ms - first) / 1000.0)
    observed_fps = (len(frames) - 1) / duration_s if duration_s > 0.0 else 0.0

    longest_stall_ms = 0.0
    for previous, current in zip(frames, frames[1:], strict=False):
        gap = current.capture_host_ms - previous.capture_host_ms
        longest_stall_ms = max(longest_stall_ms, gap)

    size_counts = Counter((item.width, item.height) for item in frames)
    delivered_width, delivered_height = size_counts.most_common(1)[0][0]
    frame_size_ok = (delivered_width, delivered_height) == (
        requested.width,
        requested.height,
    )

    rate_ok = (
        observed_fps > 0.0
        and abs(observed_fps - requested.frame_rate) / requested.frame_rate <= frame_rate_tolerance
    )

    duplicate_frames = 0
    missing_frames = 0
    unreadable_markers = 0
    previous_marker: int | None = None
    for item in frames:
        if item.frame_no is None:
            unreadable_markers += 1
            continue
        marker = item.frame_no % MARKER_WRAP
        if previous_marker is not None:
            if marker == previous_marker:
                duplicate_frames += 1
            else:
                expected = (previous_marker + 1) % MARKER_WRAP
                if marker != expected:
                    missing_frames += (marker - expected) % MARKER_WRAP
        previous_marker = marker

    reasons: list[str] = []
    if duration_s < min_duration_s:
        reasons.append("observed_duration_below_minimum")
    if longest_stall_ms > max_stall_ms:
        reasons.append("longest_stall_exceeds_limit")
    if not frame_size_ok:
        reasons.append("delivered_size_mismatch")
    if not rate_ok:
        reasons.append("delivered_rate_mismatch")
    if unreadable_markers:
        reasons.append("frame_marker_unreadable")
    if missing_frames:
        reasons.append("frame_gap_detected")

    outcome = FRAMES_UNSTABLE if reasons else FRAMES_DELIVERED

    return CellAnalysis(
        outcome=outcome,
        observed_duration_s=duration_s,
        observed_fps=observed_fps,
        delivered_width=delivered_width,
        delivered_height=delivered_height,
        frame_size_ok=frame_size_ok,
        frame_rate_ok=rate_ok,
        longest_stall_ms=longest_stall_ms,
        duplicate_frames=duplicate_frames,
        missing_frames=missing_frames,
        unreadable_markers=unreadable_markers,
        frames_observed=len(frames),
        reasons=tuple(reasons),
    )


def read_observations_csv(path: Path | str) -> list[FrameObservation]:
    """Read ``capture_host_ms,width,height[,frame_no]`` rows from a capture."""

    observations: list[FrameObservation] = []
    with Path(path).open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        required = {"capture_host_ms", "width", "height"}
        missing = required - set(reader.fieldnames or [])
        if missing:
            raise ValueError(f"{path} is missing columns: {', '.join(sorted(missing))}")
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


def cell_record(
    *,
    backend: str,
    application_id: str,
    analysis: CellAnalysis,
    requested: RequestedFormat = DEFAULT_REQUESTED,
    evidence_ref: str,
    discarded_frames: int = 0,
    notes: str | None = None,
) -> dict[str, Any]:
    """Shape one analysis as an ``injection-matrix.schema.json`` cell."""

    _require_backend(backend)
    record: dict[str, Any] = {
        "backend": backend,
        "application_id": application_id,
        "outcome": analysis.outcome,
        "observed_duration_s": round(analysis.observed_duration_s, 3),
        "requested_resolution": {
            "width": requested.width,
            "height": requested.height,
            "frame_rate": requested.frame_rate,
        },
        "delivered_size_ok": analysis.frame_size_ok,
        "delivered_rate_ok": analysis.frame_rate_ok,
        "longest_stall_ms": round(analysis.longest_stall_ms, 3),
        "duplicate_frames": analysis.duplicate_frames,
        "discarded_frames": discarded_frames,
        "evidence_ref": evidence_ref,
    }
    if notes:
        record["notes"] = notes
    if analysis.reasons:
        record["notes"] = "; ".join(
            filter(None, [record.get("notes"), f"reasons: {', '.join(analysis.reasons)}"])
        )
    return record


def not_tested_cell(
    *, backend: str, application_id: str, reason: str, evidence_ref: str
) -> dict[str, Any]:
    """A cell that was deliberately not measured, with the mandatory reason."""

    _require_backend(backend)
    if not reason.strip():
        raise ValueError("a not_tested cell must carry a reason")
    return {
        "backend": backend,
        "application_id": application_id,
        "outcome": NOT_TESTED,
        "not_tested_reason": reason,
        "evidence_ref": evidence_ref,
    }


def derive_selected_backend(cells: Iterable[dict[str, Any]]) -> str | None:
    """The backend whose cells all delivered frames.

    ``mf_virtual_camera`` is preferred when both backends pass, because the
    plan makes it the primary backend; the DirectShow filter only wins when the
    primary has at least one failing cell.
    """

    by_backend: dict[str, list[str]] = {}
    for cell in cells:
        by_backend.setdefault(str(cell["backend"]), []).append(str(cell["outcome"]))

    def passes(backend: str) -> bool:
        outcomes = by_backend.get(backend, [])
        return bool(outcomes) and all(outcome == FRAMES_DELIVERED for outcome in outcomes)

    for candidate in (MF_VIRTUAL_CAMERA, DSHOW_FILTER):
        if passes(candidate):
            return candidate
    return None


def build_matrix(
    *,
    environment_id: str,
    backends: Sequence[dict[str, Any]],
    applications: Sequence[dict[str, Any]],
    cells: Sequence[dict[str, Any]],
    licensing_impact: str,
    media_phase_consequences: str,
    multi_consumer_observation: dict[str, Any],
    decision_rationale: str | None = None,
    captured_at: str | None = None,
) -> dict[str, Any]:
    """Assemble and validate the complete matrix.

    Every evaluated backend must cover every application, and the selected
    backend must be derivable from the cells.
    """

    if not backends:
        raise ValueError("at least one backend must be evaluated")
    if len(applications) < 5:
        raise ValueError("the minimum matrix is five application categories")
    if not environment_id.strip():
        raise ValueError("environment_id is required")

    categories = {str(application["category"]) for application in applications}
    absent = REQUIRED_CATEGORIES - categories
    if absent:
        raise ValueError(
            "the matrix must cover all five application categories; missing: "
            + ", ".join(sorted(absent))
        )
    bitnesses = {str(application["bitness"]) for application in applications}
    if not REQUIRED_BITNESSES <= bitnesses:
        raise ValueError("both a 32-bit and a 64-bit consumer must be present in the matrix")

    backend_ids = [str(backend["backend"]) for backend in backends]
    application_ids = [str(application["application_id"]) for application in applications]
    if len(set(application_ids)) != len(application_ids):
        raise ValueError("application_id values must be unique")
    if len(set(backend_ids)) != len(backend_ids):
        raise ValueError("a backend may only be evaluated once")

    covered = {(str(cell["backend"]), str(cell["application_id"])) for cell in cells}
    missing = [
        (backend, application)
        for backend in backend_ids
        for application in application_ids
        if (backend, application) not in covered
    ]
    if missing:
        formatted = ", ".join(f"{backend}/{application}" for backend, application in missing)
        raise ValueError(f"the matrix is incomplete; missing cells: {formatted}")
    unknown = {cell["application_id"] for cell in cells} - set(application_ids)
    if unknown:
        raise ValueError(f"cells reference unknown applications: {sorted(unknown)}")

    selected = derive_selected_backend(cells)
    if selected is None:
        raise ValueError(
            "no backend delivered frames in every cell; the contract requires a decided backend"
        )
    rationale = decision_rationale or _default_rationale(cells, selected)

    matrix: dict[str, Any] = {
        "matrix_version": MATRIX_VERSION,
        "environment_id": environment_id,
        "backends_evaluated": list(backends),
        "applications": list(applications),
        "cells": list(cells),
        "selected_backend": selected,
        "decision_rationale": rationale,
        "licensing_impact": licensing_impact,
        "media_phase_consequences": media_phase_consequences,
        "multi_consumer_observation": multi_consumer_observation,
    }
    if captured_at:
        matrix["captured_at"] = captured_at
    return matrix


def _default_rationale(cells: Sequence[dict[str, Any]], selected: str | None) -> str:
    summary: dict[str, Counter[str]] = {}
    for cell in cells:
        summary.setdefault(str(cell["backend"]), Counter())[str(cell["outcome"])] += 1
    rendered = "; ".join(
        f"{backend}: "
        + ", ".join(f"{outcome}={count}" for outcome, count in sorted(counts.items()))
        for backend, counts in sorted(summary.items())
    )
    if selected is None:
        return f"No backend delivered frames in every cell ({rendered})."
    return f"Selected {selected} because every one of its cells delivered frames ({rendered})."


def _require_backend(backend: str) -> None:
    if backend not in (MF_VIRTUAL_CAMERA, DSHOW_FILTER):
        raise ValueError(f"unknown backend {backend!r}")
