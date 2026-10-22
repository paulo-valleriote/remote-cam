"""Tests for the capture-marker decoder shared with the Rust producer."""

from __future__ import annotations

import numpy as np
import pytest

from analysis.marker import (
    MARKER_MARGIN,
    MarkerLayout,
    decode_marker,
    render_marker_gray,
)


@pytest.mark.parametrize(("width", "height"), [(1280, 720), (640, 480)])
@pytest.mark.parametrize("frame_no", [0, 1, 42, 1234, 65_535])
def test_the_marker_round_trips_at_both_supported_sizes(
    width: int, height: int, frame_no: int
) -> None:
    frame = render_marker_gray(width, height, frame_no)

    assert decode_marker(frame, width, height) == frame_no


def test_the_marker_geometry_matches_the_rust_producer() -> None:
    assert MarkerLayout.for_frame(1280, 720).block == 40
    assert MarkerLayout.for_frame(640, 480).block == (640 - 2 * MARKER_MARGIN) // 20
    assert MarkerLayout.for_frame(1280, 720).width == 800


def test_an_undecodable_marker_is_rejected_instead_of_read_as_zero() -> None:
    blank = np.zeros((480, 640), dtype=np.uint8)
    assert decode_marker(blank, 640, 480) is None

    white = np.full((480, 640), 255, dtype=np.uint8)
    assert decode_marker(white, 640, 480) is None


def test_the_duplicate_marker_survives_a_cropped_primary_marker() -> None:
    frame = render_marker_gray(1280, 720, 4242)
    layout = MarkerLayout.for_frame(1280, 720)
    frame[
        0 : layout.margin + layout.block + 2,
        0 : layout.margin + layout.width + 2,
    ] = 0

    assert decode_marker(frame, 1280, 720) == 4242


def test_a_frame_size_mismatch_is_refused() -> None:
    frame = render_marker_gray(640, 480, 1)

    with pytest.raises(ValueError, match="does not match"):
        decode_marker(frame, 1280, 720)


def test_a_frame_too_small_for_the_marker_is_refused() -> None:
    with pytest.raises(ValueError, match="too small"):
        MarkerLayout.for_frame(20, 20)
