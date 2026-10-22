"""Silence/dropout detection shared by the fixture self-test and the audio gate.

A dropout is a contiguous silent interval of at least ``min_dropout_ms``. The
detector computes short-window RMS in dBFS, marks windows below the calibrated
threshold, merges adjacent silent windows and reports only runs long enough to
count. Shorter runs are returned separately so they can be recorded as
``below_threshold_intervals`` instead of being silently discarded.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class SilenceInterval:
    """One detected silent interval, in seconds."""

    start_s: float
    end_s: float

    @property
    def duration_ms(self) -> float:
        return (self.end_s - self.start_s) * 1000.0


@dataclass(frozen=True)
class DropoutResult:
    """Dropouts (at or above the minimum) and shorter silent intervals."""

    dropouts: list[SilenceInterval]
    below_threshold: list[SilenceInterval]
    window_ms: float
    threshold_dbfs: float

    @property
    def dropout_count(self) -> int:
        return len(self.dropouts)


def _dbfs(rms: float, reference: float) -> float:
    if rms <= 0.0 or reference <= 0.0:
        return float("-inf")
    return 20.0 * float(np.log10(rms / reference))


def detect_dropouts(
    samples: np.ndarray | list[float],
    sample_rate_hz: int,
    *,
    threshold_dbfs: float = -50.0,
    min_dropout_ms: float = 50.0,
    window_ms: float = 5.0,
    hold_ms: float = 0.0,
) -> DropoutResult:
    """Detect silent intervals in a mono signal.

    ``samples`` MUST be normalised to ``[-1, 1]``. ``hold_ms`` bridges
    non-silent gaps shorter than the given duration, matching the contract's
    ``rms_window`` detector shape; the default does not bridge.
    """

    signal = np.asarray(samples, dtype=float).flatten()
    if sample_rate_hz <= 0:
        raise ValueError("sample_rate_hz must be positive")
    if signal.size == 0:
        raise ValueError("samples must not be empty")
    if min_dropout_ms <= 0 or window_ms <= 0:
        raise ValueError("min_dropout_ms and window_ms must be positive")
    if hold_ms < 0:
        raise ValueError("hold_ms must be non-negative")

    window = max(1, int(round(sample_rate_hz * window_ms / 1000.0)))
    usable = (signal.size // window) * window
    if usable == 0:
        raise ValueError("the signal is shorter than one analysis window")

    frames = signal[:usable].reshape(-1, window)
    rms = np.sqrt(np.mean(np.square(frames), axis=1))
    reference = float(np.max(np.abs(signal[:usable])))
    silent = np.array([_dbfs(float(value), reference) < threshold_dbfs for value in rms])

    intervals: list[tuple[int, int]] = []
    start: int | None = None
    for index, is_silent in enumerate(silent):
        if is_silent and start is None:
            start = index
        elif not is_silent and start is not None:
            intervals.append((start, index))
            start = None
    if start is not None:
        intervals.append((start, len(silent)))

    if hold_ms > 0 and len(intervals) > 1:
        bridge_windows = int(round(hold_ms / window_ms))
        merged: list[tuple[int, int]] = [intervals[0]]
        for current_start, current_end in intervals[1:]:
            previous_start, previous_end = merged[-1]
            if current_start - previous_end <= bridge_windows:
                merged[-1] = (previous_start, current_end)
            else:
                merged.append((current_start, current_end))
        intervals = merged

    seconds_per_window = window / sample_rate_hz
    dropouts: list[SilenceInterval] = []
    short: list[SilenceInterval] = []
    for start_window, end_window in intervals:
        interval = SilenceInterval(
            start_s=start_window * seconds_per_window,
            end_s=end_window * seconds_per_window,
        )
        if interval.duration_ms >= min_dropout_ms:
            dropouts.append(interval)
        else:
            short.append(interval)

    return DropoutResult(
        dropouts=dropouts,
        below_threshold=short,
        window_ms=window_ms,
        threshold_dbfs=threshold_dbfs,
    )


__all__ = ["DropoutResult", "SilenceInterval", "detect_dropouts"]
