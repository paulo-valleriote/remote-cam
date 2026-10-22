"""Decode the injection test-pattern marker from a consumer capture (T039).

The producer draws a 20-block binary marker in the luma plane: a 4-bit sync
prefix followed by the 16-bit frame number, duplicated top-left and bottom-right
(because a consumer preview may crop a corner). This module is the Python twin
of ``desktop/crates/injection/src/test_pattern.rs`` and must use the same
geometry:

* margin 8 px, block edge ``min(40, (width - 16) // 20)``;
* set bit = luma above 128, clear bit = luma at or below 128;
* the top-left copy is read first, the bottom-right copy is the fallback.

A capture that cannot be decoded fails the continuity check rather than being
silently treated as a clean run.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from collections.abc import Iterator, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Final

import numpy as np

from .injection import FrameObservation

MARKER_MARGIN: Final = 8
MARKER_BLOCK: Final = 40
MARKER_BITS: Final = 16
MARKER_SYNC: Final = 0b1010
MARKER_SYNC_BITS: Final = 4
MARKER_TOTAL_BITS: Final = MARKER_SYNC_BITS + MARKER_BITS
BIT_THRESHOLD: Final = 128


@dataclass(frozen=True)
class MarkerLayout:
    block: int
    margin: int = MARKER_MARGIN

    @classmethod
    def for_frame(cls, width: int, height: int) -> MarkerLayout:
        if width < 2 * MARKER_MARGIN or height < 2 * MARKER_MARGIN + MARKER_BLOCK:
            raise ValueError(f"a {width}x{height} frame is too small for the marker")
        block = max(1, min(MARKER_BLOCK, (width - 2 * MARKER_MARGIN) // MARKER_TOTAL_BITS))
        return cls(block=block)

    @property
    def width(self) -> int:
        return self.block * MARKER_TOTAL_BITS


def decode_marker(frame: np.ndarray, width: int, height: int) -> int | None:
    """Return the frame number drawn in a single-channel frame, or ``None``."""

    image = np.asarray(frame)
    if image.ndim != 2:
        raise ValueError("the marker decoder expects a single-channel frame")
    if image.shape != (height, width):
        raise ValueError(f"frame shape {image.shape} does not match the announced {height}x{width}")

    layout = MarkerLayout.for_frame(width, height)
    for origin in _origins(width, height, layout):
        value = _read_at(image, layout, origin)
        if value is not None:
            return value
    return None


def _origins(width: int, height: int, layout: MarkerLayout) -> Iterator[tuple[int, int]]:
    yield layout.margin, layout.margin
    yield width - layout.margin - layout.width, height - layout.margin - layout.block


def _read_at(image: np.ndarray, layout: MarkerLayout, origin: tuple[int, int]) -> int | None:
    origin_x, origin_y = origin
    centre_y = origin_y + layout.block // 2
    raw = 0
    for bit in range(MARKER_TOTAL_BITS):
        centre_x = origin_x + bit * layout.block + layout.block // 2
        if not (0 <= centre_x < image.shape[1] and 0 <= centre_y < image.shape[0]):
            return None
        raw = (raw << 1) | int(image[centre_y, centre_x] > BIT_THRESHOLD)
    if raw >> MARKER_BITS != MARKER_SYNC:
        return None
    return raw & 0xFFFF


def render_marker_gray(width: int, height: int, frame_no: int) -> np.ndarray:
    """Render the producer's marker into a grayscale frame (test support)."""

    layout = MarkerLayout.for_frame(width, height)
    image = np.zeros((height, width), dtype=np.uint8)
    raw = (MARKER_SYNC << MARKER_BITS) | (frame_no & 0xFFFF)
    for origin_x, origin_y in _origins(width, height, layout):
        for bit in range(MARKER_TOTAL_BITS):
            value = 235 if (raw >> (MARKER_TOTAL_BITS - 1 - bit)) & 1 else 16
            x = origin_x + bit * layout.block
            image[origin_y : origin_y + layout.block, x : x + layout.block] = value
    return image


def probe_video_geometry(path: Path | str, ffprobe: str = "ffprobe") -> tuple[int, int]:
    """Read the decoded frame size from the capture file."""

    completed = subprocess.run(
        [
            ffprobe,
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "json",
            str(path),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"ffprobe failed for {path}: {completed.stderr.strip()}")
    streams = json.loads(completed.stdout or "{}").get("streams") or []
    if not streams:
        raise RuntimeError(f"{path} has no video stream")
    return int(streams[0]["width"]), int(streams[0]["height"])


def frame_timestamps_ms(path: Path | str, ffprobe: str = "ffprobe") -> list[float]:
    """Presentation time of every frame in the capture, in milliseconds.

    These are the times the *consumer's* file carries, so a delivery stall shows
    up as a gap between consecutive entries. Timing the frames from the sample
    index instead would hide exactly the stall the gate is looking for.
    """

    if shutil.which(ffprobe) is None:
        raise RuntimeError(f"{ffprobe} is required to read capture timestamps")
    completed = subprocess.run(
        [
            ffprobe,
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "csv=p=0",
            str(path),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"ffprobe failed for {path}: {completed.stderr.strip()}")

    timestamps: list[float] = []
    for line in completed.stdout.splitlines():
        candidate = line.strip().rstrip(",").strip()
        if not candidate or candidate.upper() == "N/A":
            continue
        try:
            timestamps.append(float(candidate) * 1000.0)
        except ValueError:
            continue
    return timestamps


def observations_from_video(
    path: Path | str,
    *,
    ffmpeg: str = "ffmpeg",
    ffprobe: str = "ffprobe",
) -> list[FrameObservation]:
    """Decode one marker per frame of a consumer capture.

    Every frame is decoded: skipping frames would make a consecutive-marker
    comparison meaningless, because a sampled subsequence always looks like it
    has gaps. A frame whose marker cannot be decoded is kept with
    ``frame_no=None`` so the analyzer reports it, instead of dropping the
    evidence.

    Timestamps come from the capture's own presentation times when FFprobe
    reports one per decoded frame; otherwise the frames are spaced evenly at the
    average rate, and the count difference is visible as a mismatch between the
    decoded frame count and the reported duration.
    """

    if shutil.which(ffmpeg) is None:
        raise RuntimeError(f"{ffmpeg} is required to decode a capture")

    width, height = probe_video_geometry(path, ffprobe)
    frame_bytes = width * height
    process = subprocess.Popen(
        [
            ffmpeg,
            "-v",
            "error",
            "-i",
            str(path),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdout is not None
    frames: list[int | None] = []
    while True:
        buffer = process.stdout.read(frame_bytes)
        if len(buffer) < frame_bytes:
            break
        frame = np.frombuffer(buffer, dtype=np.uint8).reshape(height, width)
        frames.append(decode_marker(frame, width, height))

    stderr = process.stderr.read().decode("utf-8", "replace") if process.stderr else ""
    if process.wait() != 0:
        raise RuntimeError(f"ffmpeg failed for {path}: {stderr.strip()}")

    timestamps = frame_timestamps_ms(path, ffprobe)
    if len(timestamps) == len(frames):
        times = timestamps
    else:
        times = evenly_spaced_times(len(frames), timestamps)

    return [
        FrameObservation(
            capture_host_ms=times[index],
            width=width,
            height=height,
            frame_no=frame_no,
        )
        for index, frame_no in enumerate(frames)
    ]


def evenly_spaced_times(count: int, timestamps: Sequence[float]) -> list[float]:
    """Fallback timeline when FFprobe does not report one time per frame."""

    if count == 0:
        return []
    if len(timestamps) >= 2:
        first, last = timestamps[0], timestamps[-1]
        if last > first:
            step = (last - first) / (count - 1 if count > 1 else 1)
            return [first + index * step for index in range(count)]
    return [index * 1000.0 / 30.0 for index in range(count)]
