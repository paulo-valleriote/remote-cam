"""Continuity-analyzer tests for the virtual camera injection gate (T028).

The analyzer turns what a consumer application actually delivered — frame
timestamps, frame markers, announced size and rate — into the per-cell verdict
the matrix contract requires. These tests pin the gate boundaries.
"""

from __future__ import annotations

import pytest

from analysis.injection import (
    DEFAULT_REQUESTED,
    MAX_STALL_MS,
    MIN_OBSERVED_DURATION_S,
    CellAnalysis,
    FrameObservation,
    RequestedFormat,
    analyse_capture,
    derive_selected_backend,
)


def steady_run(
    *,
    duration_s: float = 1800.0,
    fps: int = 30,
    width: int = 1280,
    height: int = 720,
    marker_every: int = 1,
) -> list[FrameObservation]:
    """A capture with perfectly spaced frames."""

    interval_ms = 1000.0 / fps
    count = int(round(duration_s * fps)) + 1
    return [
        FrameObservation(
            capture_host_ms=index * interval_ms,
            frame_no=index if index % marker_every == 0 else None,
            width=width,
            height=height,
        )
        for index in range(count)
    ]


def test_a_steady_run_passes_every_criterion() -> None:
    analysis = analyse_capture(steady_run())

    assert analysis.outcome == "frames_delivered"
    assert analysis.frame_size_ok is True
    assert analysis.frame_rate_ok is True
    assert analysis.longest_stall_ms <= MAX_STALL_MS
    assert analysis.observed_duration_s >= MIN_OBSERVED_DURATION_S
    assert analysis.reasons == ()


def with_stall(
    observations: list[FrameObservation], *, stall_ms: float, after_ms: float, fps: int
) -> list[FrameObservation]:
    """Insert a delivery gap of exactly ``stall_ms`` between two frames."""

    shift_ms = stall_ms - 1000.0 / fps
    return [
        FrameObservation(
            capture_host_ms=item.capture_host_ms
            + (shift_ms if item.capture_host_ms > after_ms else 0.0),
            frame_no=item.frame_no,
            width=item.width,
            height=item.height,
        )
        for item in observations
    ]


def test_a_two_second_stall_sits_on_the_contract_boundary() -> None:
    """The contract's schema allows ``longest_stall_ms`` up to 2 000 ms."""

    observations = with_stall(
        steady_run(duration_s=60, fps=10), stall_ms=2_000.0, after_ms=30_000.0, fps=10
    )

    analysis = analyse_capture(
        observations, requested=RequestedFormat(frame_rate=10), min_duration_s=60
    )

    assert analysis.longest_stall_ms == pytest.approx(2_000.0)
    assert analysis.outcome == "frames_delivered"


@pytest.mark.parametrize("stall_ms", [2_000.5, 2_001.0, 5_000.0])
def test_a_stall_longer_than_two_seconds_fails(stall_ms: float) -> None:
    observations = with_stall(
        steady_run(duration_s=60, fps=10), stall_ms=stall_ms, after_ms=30_000.0, fps=10
    )

    analysis = analyse_capture(
        observations, requested=RequestedFormat(frame_rate=10), min_duration_s=60
    )

    assert analysis.outcome == "frames_unstable"
    assert "longest_stall_exceeds_limit" in analysis.reasons
    assert analysis.longest_stall_ms == pytest.approx(stall_ms)


def test_duplicate_frames_are_counted() -> None:
    observations = steady_run(duration_s=60, fps=10)
    duplicated: list[FrameObservation] = []
    for item in observations:
        duplicated.append(item)
        if item.frame_no == 100:
            duplicated.append(
                FrameObservation(
                    capture_host_ms=item.capture_host_ms + 5.0,
                    frame_no=item.frame_no,
                    width=item.width,
                    height=item.height,
                )
            )

    analysis = analyse_capture(
        duplicated, requested=RequestedFormat(frame_rate=10), min_duration_s=60
    )

    assert analysis.duplicate_frames == 1
    assert analysis.outcome == "frames_delivered", "duplicates are recorded, not gated"


def test_delivered_size_mismatch_is_recorded() -> None:
    analysis = analyse_capture(steady_run(duration_s=60, fps=10, width=640, height=480))

    assert analysis.frame_size_ok is False
    assert analysis.delivered_width == 640
    assert analysis.delivered_height == 480
    assert analysis.outcome == "frames_unstable"
    assert "delivered_size_mismatch" in analysis.reasons


def test_delivered_rate_mismatch_is_recorded() -> None:
    analysis = analyse_capture(steady_run(duration_s=60, fps=15))

    assert analysis.frame_rate_ok is False
    assert analysis.observed_fps == pytest.approx(15.0, rel=0.01)
    assert analysis.outcome == "frames_unstable"
    assert "delivered_rate_mismatch" in analysis.reasons


def test_observed_duration_below_the_thirty_minute_requirement_is_enforced() -> None:
    analysis = analyse_capture(steady_run(duration_s=1799.0))

    assert analysis.observed_duration_s < MIN_OBSERVED_DURATION_S
    assert analysis.outcome == "frames_unstable"
    assert "observed_duration_below_minimum" in analysis.reasons


def test_an_empty_capture_means_the_device_was_not_visible() -> None:
    analysis = analyse_capture([])

    assert analysis.outcome == "device_not_visible"
    assert analysis.frames_observed == 0
    assert analysis.observed_duration_s == 0.0
    assert analysis.reasons == ("no_frames_observed",)


def test_unreadable_frame_markers_make_the_cell_unstable() -> None:
    observations = [
        FrameObservation(capture_host_ms=index * 33.3, frame_no=None, width=1280, height=720)
        for index in range(10)
    ]

    analysis = analyse_capture(observations, min_duration_s=1)

    assert analysis.unreadable_markers == 10
    assert analysis.outcome == "frames_unstable"
    assert "frame_marker_unreadable" in analysis.reasons


def test_missing_frame_numbers_are_counted_as_a_continuity_defect() -> None:
    observations = steady_run(duration_s=10, fps=10)
    gapped = [item for item in observations if item.frame_no not in {10, 11, 12}]

    analysis = analyse_capture(gapped, requested=RequestedFormat(frame_rate=10), min_duration_s=5)

    assert analysis.missing_frames == 3
    assert analysis.outcome == "frames_unstable"
    assert "frame_gap_detected" in analysis.reasons


def test_default_requested_format_is_the_product_default() -> None:
    assert DEFAULT_REQUESTED.width == 1280
    assert DEFAULT_REQUESTED.height == 720
    assert DEFAULT_REQUESTED.frame_rate == 30


def test_a_selected_backend_is_derivable_from_the_cells() -> None:
    passing = CellAnalysis(
        outcome="frames_delivered",
        observed_duration_s=1800.0,
        observed_fps=30.0,
        delivered_width=1280,
        delivered_height=720,
        frame_size_ok=True,
        frame_rate_ok=True,
        longest_stall_ms=12.0,
        duplicate_frames=0,
        missing_frames=0,
        unreadable_markers=0,
        frames_observed=54_000,
        reasons=(),
    )
    failing = CellAnalysis(
        **{**passing.__dict__, "outcome": "device_not_visible", "reasons": ("no_frames_observed",)}
    )

    cells = [
        cell("mf_virtual_camera", "chrome", failing),
        cell("dshow_filter", "chrome", passing),
    ]

    assert derive_selected_backend(cells) == "dshow_filter"


def test_the_primary_backend_wins_when_both_pass() -> None:
    passing = CellAnalysis(
        outcome="frames_delivered",
        observed_duration_s=1800.0,
        observed_fps=30.0,
        delivered_width=1280,
        delivered_height=720,
        frame_size_ok=True,
        frame_rate_ok=True,
        longest_stall_ms=12.0,
        duplicate_frames=0,
        missing_frames=0,
        unreadable_markers=0,
        frames_observed=54_000,
        reasons=(),
    )
    cells = [
        cell("dshow_filter", "chrome", passing),
        cell("mf_virtual_camera", "chrome", passing),
    ]

    assert derive_selected_backend(cells) == "mf_virtual_camera"


def test_no_backend_is_derivable_when_every_backend_has_a_failure() -> None:
    failing = CellAnalysis(
        outcome="device_not_visible",
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
    cells = [
        cell("mf_virtual_camera", "chrome", failing),
        cell("dshow_filter", "chrome", failing),
    ]

    assert derive_selected_backend(cells) is None


def cell(backend: str, application_id: str, analysis: CellAnalysis) -> dict[str, object]:
    from analysis.injection import cell_record

    return cell_record(
        backend=backend,
        application_id=application_id,
        analysis=analysis,
        requested=DEFAULT_REQUESTED,
        evidence_ref=f"spikes/evidence/injection/{backend}-{application_id}.json",
    )
