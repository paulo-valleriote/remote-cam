"""Two-detector silence/dropout analysis for the virtual audio gate (T098, T105).

A dropout is a contiguous silent interval of **at least 50 ms**. The gate
requires two independent detectors that agree:

* the primary detector is FFmpeg ``silencedetect`` at the **calibrated**
  threshold — the capture's noise floor plus 6 dB, with the calibration
  recorded as ``threshold_source``;
* the cross-check is a short-window RMS detector over the decoded samples.

A disagreement is a finding in its own right and can never be silently resolved
by picking one detector (FR-030, measurement-formats §4). Detected intervals are
cross-referenced against the receiver's RTP sequence-gap log, so a transport
loss is distinguishable from a render or consumer-side defect.

**NON-PRODUCTION** — F0 risk-reduction spike tooling.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final

import numpy as np

ANALYSER_VERSION: Final = "1.0.0"
MIN_DROPOUT_MS: Final = 50.0
CALIBRATION_MARGIN_DB: Final = 6.0
DIGITAL_SILENCE_FLOOR_DBFS: Final = -90.0
#: Two intervals are the same drop when their boundaries are this close.
INTERVAL_TOLERANCE_MS: Final = 15.0
#: An RTP gap correlates with a dropout when it falls inside this window.
RTP_CORRELATION_TOLERANCE_MS: Final = 100.0


class CalibrationError(ValueError):
    """The threshold is not backed by a calibration record."""


class AnalyzerUnavailable(RuntimeError):
    """A required external analyzer (FFmpeg) is not installed."""


@dataclass(frozen=True)
class SilenceInterval:
    """One detected silent interval, in seconds."""

    start_s: float
    end_s: float

    @property
    def duration_ms(self) -> float:
        return (self.end_s - self.start_s) * 1000.0


@dataclass(frozen=True)
class Calibration:
    """Threshold derived from the capture's own noise floor."""

    noise_floor_dbfs: float
    threshold_dbfs: float
    margin_db: float = CALIBRATION_MARGIN_DB
    window_ms: float = 20.0

    @property
    def threshold_source(self) -> str:
        return f"calibrated: noise floor {self.noise_floor_dbfs:.1f} dBFS + {self.margin_db:.1f} dB"


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


@dataclass(frozen=True)
class RtpGap:
    """One receiver-side sequence-number gap."""

    t_s: float
    lost_packets: int = 1


def _dbfs(rms: float) -> float:
    if rms <= 0.0:
        return float("-inf")
    return 20.0 * float(np.log10(rms))


def calibrate(
    samples: np.ndarray | list[float],
    sample_rate_hz: int,
    *,
    window_ms: float = 20.0,
    margin_db: float = CALIBRATION_MARGIN_DB,
    quiet_percentile: float = 10.0,
) -> Calibration:
    """Derive the detection threshold from the capture's quietest windows.

    ``quiet_percentile`` of the window RMS values stands in for the noise floor;
    the threshold is that floor plus ``margin_db``. A digitally silent capture
    floors at ``DIGITAL_SILENCE_FLOOR_DBFS`` so the threshold remains
    representable.
    """

    signal = np.asarray(samples, dtype=float).flatten()
    if signal.size == 0:
        raise CalibrationError("cannot calibrate a threshold from an empty capture")
    if sample_rate_hz <= 0 or window_ms <= 0:
        raise CalibrationError("sample_rate_hz and window_ms must be positive")

    window = max(1, int(round(sample_rate_hz * window_ms / 1000.0)))
    usable = (signal.size // window) * window
    if usable == 0:
        raise CalibrationError("the capture is shorter than one calibration window")

    frames = signal[:usable].reshape(-1, window)
    levels = np.array([_dbfs(float(np.sqrt(np.mean(np.square(frame))))) for frame in frames])
    finite = levels[np.isfinite(levels)]
    noise_floor = (
        float(np.percentile(finite, quiet_percentile))
        if finite.size
        else DIGITAL_SILENCE_FLOOR_DBFS
    )
    noise_floor = max(noise_floor, DIGITAL_SILENCE_FLOOR_DBFS)
    threshold = noise_floor + margin_db

    if not -100.0 <= threshold <= -20.0:
        raise CalibrationError(
            f"calibrated threshold {threshold:.1f} dBFS is outside the usable range "
            "[-100, -20]; record a capture with audible speech and real pauses"
        )

    return Calibration(
        noise_floor_dbfs=noise_floor,
        threshold_dbfs=threshold,
        margin_db=margin_db,
        window_ms=window_ms,
    )


def detect_dropouts(
    samples: np.ndarray | list[float],
    sample_rate_hz: int,
    *,
    threshold_dbfs: float = -50.0,
    min_dropout_ms: float = MIN_DROPOUT_MS,
    window_ms: float = 5.0,
    hold_ms: float = 0.0,
) -> DropoutResult:
    """Detect silent intervals in a mono signal.

    Levels are absolute dBFS (full scale = 1.0), matching the calibration and
    the contract. ``hold_ms`` bridges non-silent gaps shorter than the given
    duration, matching the contract's ``rms_window`` detector shape.
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
    silent = np.array([_dbfs(float(value)) < threshold_dbfs for value in rms])

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


def parse_silencedetect(stderr: str, duration_s: float) -> list[SilenceInterval]:
    """Parse FFmpeg ``silencedetect`` output into intervals.

    FFmpeg prints ``silence_start`` and, when the file ends during silence,
    omits ``silence_end``; the capture duration closes that interval.
    """

    intervals: list[SilenceInterval] = []
    start: float | None = None
    for line in stderr.splitlines():
        if "silence_start:" in line:
            try:
                start = float(line.split("silence_start:", 1)[1].strip().split()[0])
            except (IndexError, ValueError):
                continue
        elif "silence_end:" in line and start is not None:
            try:
                end = float(line.split("silence_end:", 1)[1].strip().split()[0])
            except (IndexError, ValueError):
                continue
            intervals.append(SilenceInterval(start_s=start, end_s=end))
            start = None
    if start is not None:
        intervals.append(SilenceInterval(start_s=start, end_s=duration_s))
    return intervals


def run_silencedetect(
    capture: Path,
    *,
    duration_s: float,
    threshold_dbfs: float,
    min_dropout_ms: float = MIN_DROPOUT_MS,
    ffmpeg: str = "ffmpeg",
) -> list[SilenceInterval]:
    """Run FFmpeg's primary detector on a recorded capture."""

    binary = shutil.which(ffmpeg)
    if binary is None:
        raise AnalyzerUnavailable(
            f"`{ffmpeg}` was not found; the primary silencedetect detector is required "
            "for the audio gate (FFmpeg >= 8.1)"
        )
    filter_spec = f"silencedetect=noise={threshold_dbfs:.1f}dB:d={min_dropout_ms / 1000.0:.3f}"
    command = [
        binary,
        "-nostdin",
        "-hide_banner",
        "-i",
        str(capture),
        "-af",
        filter_spec,
        "-f",
        "null",
        "-",
    ]
    result = subprocess.run(  # noqa: S603 - fixed argument vector, no shell
        command,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise AnalyzerUnavailable(f"FFmpeg failed on {capture}: {result.stderr.strip()}")
    return parse_silencedetect(result.stderr, duration_s)


def compare_detectors(
    silencedetect: Sequence[SilenceInterval],
    rms_window: Sequence[SilenceInterval],
    *,
    tolerance_ms: float = INTERVAL_TOLERANCE_MS,
) -> tuple[bool, list[str]]:
    """Compare the two detectors and return ``(agree, findings)``."""

    findings: list[str] = []
    if len(silencedetect) != len(rms_window):
        findings.append(
            f"detector count mismatch: silencedetect={len(silencedetect)}, "
            f"rms_window={len(rms_window)}"
        )
    for index, (primary, cross_check) in enumerate(zip(silencedetect, rms_window, strict=False)):
        start_delta = abs(primary.start_s - cross_check.start_s) * 1000.0
        end_delta = abs(primary.end_s - cross_check.end_s) * 1000.0
        if start_delta > tolerance_ms or end_delta > tolerance_ms:
            findings.append(
                f"interval {index + 1} differs: silencedetect={primary.start_s:.3f}-"
                f"{primary.end_s:.3f}s, rms_window={cross_check.start_s:.3f}-"
                f"{cross_check.end_s:.3f}s"
            )
    return (not findings, findings)


def parse_rtp_gaps(path: Path) -> list[RtpGap]:
    """Parse the receiver's RTP sequence-gap log (JSONL or CSV)."""

    text = path.read_text(encoding="utf-8")
    gaps: list[RtpGap] = []
    if path.suffix in {".jsonl", ".ndjson"} or text.lstrip().startswith("{"):
        for line in text.splitlines():
            if not line.strip():
                continue
            entry = json.loads(line)
            t_s = entry.get("t_s")
            if t_s is None and entry.get("t_ms") is not None:
                t_s = float(entry["t_ms"]) / 1000.0
            if t_s is None:
                raise ValueError(f"{path}: every gap needs t_s or t_ms")
            gaps.append(RtpGap(t_s=float(t_s), lost_packets=int(entry.get("lost_packets", 1))))
        return gaps

    lines = [line for line in text.splitlines() if line.strip()]
    if not lines:
        return gaps
    header = [field.strip() for field in lines[0].split(",")]
    if "t_s" not in header:
        raise ValueError(f"{path}: the CSV header must contain t_s")
    t_index = header.index("t_s")
    lost_index = header.index("lost_packets") if "lost_packets" in header else None
    for line in lines[1:]:
        fields = [field.strip() for field in line.split(",")]
        if len(fields) <= t_index:
            continue
        lost = int(fields[lost_index]) if lost_index is not None else 1
        gaps.append(RtpGap(t_s=float(fields[t_index]), lost_packets=lost))
    return gaps


def correlate_rtp_gaps(
    intervals: Iterable[SilenceInterval],
    gaps: Sequence[RtpGap],
    *,
    tolerance_ms: float = RTP_CORRELATION_TOLERANCE_MS,
) -> list[bool]:
    """Mark each interval that overlaps an RTP gap inside the tolerance."""

    tolerance = tolerance_ms / 1000.0
    return [
        any(
            gap.t_s >= interval.start_s - tolerance and gap.t_s <= interval.end_s + tolerance
            for gap in gaps
        )
        for interval in intervals
    ]


def _interval_document(
    interval: SilenceInterval, detector: str, correlated: bool
) -> dict[str, Any]:
    return {
        "start_s": round(interval.start_s, 3),
        "end_s": round(interval.end_s, 3),
        "duration_ms": round(interval.duration_ms, 1),
        "detector": detector,
        "correlated_rtp_gap": correlated,
    }


def build_document(
    *,
    run_id: str,
    capture_file: str,
    sample_rate_hz: int,
    channels: int,
    duration_s: float,
    calibration: Calibration | None = None,
    threshold_dbfs: float | None = None,
    threshold_source: str | None = None,
    silencedetect: Sequence[SilenceInterval] = (),
    rms_window: Sequence[SilenceInterval] = (),
    below_threshold_intervals: int = 0,
    rtp_gaps: Sequence[RtpGap] = (),
    consumer_renegotiations: int = 0,
) -> dict[str, Any]:
    """Assemble the contract ``dropouts.json`` document.

    Raises :class:`CalibrationError` when a threshold is supplied without a
    calibration record: a hard-coded threshold is not admissible evidence.
    """

    if calibration is not None:
        threshold = calibration.threshold_dbfs
        source = calibration.threshold_source
    elif threshold_dbfs is not None and threshold_source:
        threshold = threshold_dbfs
        source = threshold_source
    else:
        raise CalibrationError(
            "the detection threshold must come from a calibration record "
            "(noise floor + 6 dB) or carry an explicit threshold_source"
        )

    correlated_primary = correlate_rtp_gaps(silencedetect, rtp_gaps)
    correlated_rms = correlate_rtp_gaps(rms_window, rtp_gaps)
    agree, findings = compare_detectors(silencedetect, rms_window)

    primary_intervals = [
        _interval_document(interval, "silencedetect", correlated)
        for interval, correlated in zip(silencedetect, correlated_primary, strict=True)
    ]
    rms_intervals = [
        _interval_document(interval, "rms_window", correlated)
        for interval, correlated in zip(rms_window, correlated_rms, strict=True)
    ]

    verdict = (
        "pass"
        if agree and not silencedetect and not rms_window and consumer_renegotiations == 0
        else "fail"
    )

    return {
        "run_id": run_id,
        "capture_file": capture_file,
        "sample_rate_hz": sample_rate_hz,
        "channels": channels,
        "duration_s": round(duration_s, 3),
        "criterion": {
            "min_dropout_ms": MIN_DROPOUT_MS,
            "threshold_dbfs": round(threshold, 1),
            "threshold_source": source,
        },
        "detectors": {
            "silencedetect": {
                "dropout_count": len(primary_intervals),
                "intervals": primary_intervals,
            },
            "rms_window": {
                "dropout_count": len(rms_intervals),
                "intervals": rms_intervals,
                "window_ms": 5.0,
                "hold_ms": 10.0,
            },
        },
        "detectors_agree": agree,
        "findings": findings,
        "below_threshold_intervals": below_threshold_intervals,
        "correlated_rtp_gaps": sum(1 for flag in correlated_primary if flag),
        "consumer_renegotiations": consumer_renegotiations,
        "verdict": verdict,
        "analyser_version": ANALYSER_VERSION,
    }


def analyse_capture(
    *,
    capture: Path,
    run_id: str,
    duration_s: float,
    sample_rate_hz: int,
    channels: int,
    threshold_dbfs: float | None = None,
    threshold_source: str | None = None,
    rtp_gaps_path: Path | None = None,
    consumer_renegotiations: int = 0,
    ffmpeg: str = "ffmpeg",
    capture_ref: str | None = None,
) -> dict[str, Any]:
    """Run both detectors on a capture and return the contract document."""

    import soundfile as sf

    samples, file_rate = sf.read(capture, dtype="float64", always_2d=False)
    if getattr(samples, "ndim", 1) > 1:
        samples = samples.mean(axis=1)
    if file_rate != sample_rate_hz:
        raise CalibrationError(
            f"capture sample rate is {file_rate} Hz but the run records {sample_rate_hz} Hz"
        )

    if threshold_dbfs is None and threshold_source is None:
        calibration = calibrate(samples, sample_rate_hz)
    else:
        calibration = None
        if threshold_dbfs is None or not threshold_source:
            raise CalibrationError(
                "a threshold without its calibration source cannot be used: pass a "
                "Calibration record or both threshold_dbfs and threshold_source"
            )

    primary = run_silencedetect(
        capture,
        duration_s=duration_s,
        threshold_dbfs=calibration.threshold_dbfs if calibration else float(threshold_dbfs),
        ffmpeg=ffmpeg,
    )
    cross_check = detect_dropouts(
        samples,
        sample_rate_hz,
        threshold_dbfs=calibration.threshold_dbfs if calibration else float(threshold_dbfs),
        window_ms=5.0,
        hold_ms=10.0,
    )

    gaps = parse_rtp_gaps(rtp_gaps_path) if rtp_gaps_path else []

    return build_document(
        run_id=run_id,
        capture_file=capture_ref or str(capture),
        sample_rate_hz=sample_rate_hz,
        channels=channels,
        duration_s=duration_s,
        calibration=calibration,
        threshold_dbfs=threshold_dbfs,
        threshold_source=threshold_source,
        silencedetect=primary,
        rms_window=cross_check.dropouts,
        below_threshold_intervals=len(cross_check.below_threshold),
        rtp_gaps=gaps,
        consumer_renegotiations=consumer_renegotiations,
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Two-detector audio dropout analysis")
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--duration-s", type=float, required=True)
    parser.add_argument("--sample-rate-hz", type=int, default=48_000)
    parser.add_argument("--channels", type=int, default=1)
    parser.add_argument("--rtp-gaps", type=Path, default=None)
    parser.add_argument("--consumer-renegotiations", type=int, default=0)
    parser.add_argument("--threshold-dbfs", type=float, default=None)
    parser.add_argument("--threshold-source", default=None)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--capture-ref", default=None)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    try:
        document = analyse_capture(
            capture=args.capture,
            run_id=args.run_id,
            duration_s=args.duration_s,
            sample_rate_hz=args.sample_rate_hz,
            channels=args.channels,
            threshold_dbfs=args.threshold_dbfs,
            threshold_source=args.threshold_source,
            rtp_gaps_path=args.rtp_gaps,
            consumer_renegotiations=args.consumer_renegotiations,
            ffmpeg=args.ffmpeg,
            capture_ref=args.capture_ref,
        )
    except (CalibrationError, AnalyzerUnavailable, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


__all__ = [
    "ANALYSER_VERSION",
    "AnalyzerUnavailable",
    "Calibration",
    "CalibrationError",
    "DropoutResult",
    "MIN_DROPOUT_MS",
    "RtpGap",
    "SilenceInterval",
    "analyse_capture",
    "build_document",
    "calibrate",
    "compare_detectors",
    "correlate_rtp_gaps",
    "detect_dropouts",
    "main",
    "parse_rtp_gaps",
    "parse_silencedetect",
    "run_silencedetect",
]
