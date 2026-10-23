"""Capture-at-consumer latency analysis (T076, T078).

The consumer's own output is captured with the same FFmpeg command used for the
harness-floor calibration, and the per-frame QR marker is decoded from that
capture. The gated statistic is the **raw** p95; the measured floor and the
floor-corrected value are published alongside it and are never substituted for
the raw value.
"""

from __future__ import annotations

import argparse
import csv
import json
import shutil
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Final

import numpy as np

from metrics.criteria import (
    LATENCY_P95_MS,
    VERDICT_FAIL,
    VERDICT_PASS,
    floor_corrected,
    latency_criterion,
    percentile_set,
)

HEADER: Final = [
    "sample_id",
    "run_id",
    "consumer_app",
    "source_frame_no",
    "source_epoch_ms",
    "capture_host_ms",
    "clock_offset_ms",
    "latency_ms",
    "harness_floor_ms",
]

INCLUDED_STAGES: Final = [
    "network_and_jitter_buffer",
    "decode",
    "frame_handoff",
    "consumer_presentation",
]
EXCLUDED_STAGES: Final = ["capture", "encode"]


@dataclass(frozen=True)
class LatencySample:
    sample_id: int
    run_id: str
    consumer_app: str
    source_frame_no: int
    source_epoch_ms: int
    capture_host_ms: float
    clock_offset_ms: float
    latency_ms: float
    harness_floor_ms: float


def read_latency_csv(path: Path | str) -> list[LatencySample]:
    """Read the contract CSV, failing on a malformed row."""

    with Path(path).open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames != HEADER:
            raise ValueError(f"unexpected header {reader.fieldnames!r}; expected {HEADER!r}")
        samples = []
        for row in reader:
            samples.append(
                LatencySample(
                    sample_id=int(row["sample_id"]),
                    run_id=row["run_id"],
                    consumer_app=row["consumer_app"],
                    source_frame_no=int(row["source_frame_no"]),
                    source_epoch_ms=int(row["source_epoch_ms"]),
                    capture_host_ms=float(row["capture_host_ms"]),
                    clock_offset_ms=float(row["clock_offset_ms"]),
                    latency_ms=float(row["latency_ms"]),
                    harness_floor_ms=float(row["harness_floor_ms"]),
                )
            )
    return samples


def summarize(
    samples: Sequence[LatencySample],
    *,
    harness_floor_ms: float,
    run_id: str | None = None,
) -> dict:
    """Produce ``latency_summary.json`` for one run.

    ``criterion.basis`` is fixed to ``raw`` and the verdict is judged on
    ``raw.p95_ms``. A summary with only one of raw/corrected is never produced.
    """

    if not samples:
        raise ValueError("a latency summary needs at least one sample")
    if harness_floor_ms < 0:
        raise ValueError("harness_floor_ms must be non-negative")
    run_id = run_id or samples[0].run_id

    latencies = [sample.latency_ms for sample in samples]
    raw = percentile_set(latencies, suffix="_ms")
    if harness_floor_ms > raw["p50_ms"]:
        raise ValueError(
            f"harness floor {harness_floor_ms} ms exceeds the raw p50 {raw['p50_ms']} ms; "
            "the captured marker is not the produced marker"
        )
    corrected = {key: value - harness_floor_ms for key, value in raw.items()}
    # Contract L-3: the corrected p95 is exactly the raw p95 minus the floor.
    corrected["p95_ms"] = floor_corrected(raw["p95_ms"], harness_floor_ms)
    criterion = latency_criterion(raw["p95_ms"], LATENCY_P95_MS)
    verdict = VERDICT_PASS if criterion["passed"] else VERDICT_FAIL

    summary = {
        "run_id": run_id,
        "sample_count": len(samples),
        "harness_floor_ms": harness_floor_ms,
        "raw": raw,
        "corrected": corrected,
        "criterion": {
            "metric": "raw.p95_ms",
            "threshold": LATENCY_P95_MS,
            "comparison": "<=",
            "basis": "raw",
        },
        "verdict": verdict,
        "included_stages": INCLUDED_STAGES,
        "excluded_stages": EXCLUDED_STAGES,
    }
    if verdict != VERDICT_PASS:
        summary["attribution"] = _attribution(raw["p95_ms"], harness_floor_ms, corrected["p95_ms"])
    return summary


def _attribution(raw_p95_ms: float, floor_ms: float, corrected_p95_ms: float) -> str:
    share = 0.0 if raw_p95_ms <= 0 else 100.0 * floor_ms / raw_p95_ms
    if corrected_p95_ms <= LATENCY_P95_MS < raw_p95_ms:
        return (
            f"The harness floor of {floor_ms:.1f} ms ({share:.0f} % of the raw p95) accounts for "
            "the failure: the floor-corrected value is inside the threshold, so this is a "
            "capture-procedure limitation, not a product regression. The procedure must be "
            "improved before the gate is judged."
        )
    return (
        f"The harness floor accounts for {share:.0f} % of the raw p95; the result is not "
        "attributable to the capture procedure and is treated as a pipeline result."
    )


# --------------------------------------------------------------------------- #
# QR marker decoding
# --------------------------------------------------------------------------- #


def decode_marker_payload(frame: np.ndarray) -> str | None:
    """Decode the QR marker in a single-channel frame, or return ``None``."""

    import zxingcpp  # local import keeps the CSV-only path dependency-free

    results = zxingcpp.read_barcodes(np.asarray(frame, dtype=np.uint8))
    if not results:
        return None
    return str(results[0].text)


def parse_marker_payload(payload: str) -> tuple[int, int]:
    """Parse ``{epoch_ms}|{frame_no}`` into its two integers."""

    epoch_text, frame_text = payload.split("|", 1)
    return int(epoch_text), int(frame_text)


def observations_from_capture(
    capture: Path | str,
    *,
    run_id: str,
    consumer_app: str,
    capture_start_epoch_ms: float,
    harness_floor_ms: float,
    clock_offset_ms: float = 0.0,
    ffmpeg: str = "ffmpeg",
    ffprobe: str = "ffprobe",
) -> tuple[list[LatencySample], int]:
    """Decode every frame of a capture and build the latency rows.

    Returns the observations and the number of frames whose marker could not be
    decoded (kept as a count rather than silently dropped).
    """

    if shutil.which(ffmpeg) is None:
        raise RuntimeError(f"{ffmpeg} is required to decode a capture")
    width, height = probe_geometry(capture, ffprobe)
    timestamps = frame_timestamps_ms(capture, ffprobe)

    process = subprocess.Popen(
        [
            ffmpeg,
            "-v",
            "error",
            "-i",
            str(capture),
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
    frame_bytes = width * height
    samples: list[LatencySample] = []
    unreadable = 0
    index = 0
    while True:
        buffer = process.stdout.read(frame_bytes)
        if len(buffer) < frame_bytes:
            break
        frame = np.frombuffer(buffer, dtype=np.uint8).reshape(height, width)
        payload = decode_marker_payload(frame)
        capture_host_ms = (
            capture_start_epoch_ms + timestamps[index]
            if index < len(timestamps)
            else capture_start_epoch_ms + index * 1000.0 / 30.0
        )
        index += 1
        if payload is None:
            unreadable += 1
            continue
        source_epoch_ms, frame_no = parse_marker_payload(payload)
        samples.append(
            LatencySample(
                sample_id=len(samples) + 1,
                run_id=run_id,
                consumer_app=consumer_app,
                source_frame_no=frame_no,
                source_epoch_ms=source_epoch_ms,
                capture_host_ms=capture_host_ms,
                clock_offset_ms=clock_offset_ms,
                latency_ms=capture_host_ms - source_epoch_ms - clock_offset_ms,
                harness_floor_ms=harness_floor_ms,
            )
        )

    stderr = process.stderr.read().decode("utf-8", "replace") if process.stderr else ""
    if process.wait() != 0:
        raise RuntimeError(f"ffmpeg failed for {capture}: {stderr.strip()}")
    return samples, unreadable


def probe_geometry(path: Path | str, ffprobe: str = "ffprobe") -> tuple[int, int]:
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


def write_summary(path: Path | str, summary: dict) -> None:
    Path(path).write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    summary_parser = subparsers.add_parser("summary", help="summarise latency_samples.csv")
    summary_parser.add_argument("--samples", required=True)
    summary_parser.add_argument("--floor", required=True, help="floor JSON or a millisecond value")
    summary_parser.add_argument("--run-id", default=None)
    summary_parser.add_argument("--out", default=None)

    capture_parser = subparsers.add_parser("capture", help="decode a consumer capture")
    capture_parser.add_argument("--video", required=True)
    capture_parser.add_argument("--run-id", required=True)
    capture_parser.add_argument("--consumer-app", required=True)
    capture_parser.add_argument("--capture-start-epoch-ms", type=float, required=True)
    capture_parser.add_argument("--floor", required=True)
    capture_parser.add_argument("--out", required=True)

    args = parser.parse_args(argv)
    if args.command == "summary":
        floor = read_floor_value(args.floor)
        samples = read_latency_csv(args.samples)
        summary = summarize(samples, harness_floor_ms=floor, run_id=args.run_id)
        if args.out:
            write_summary(args.out, summary)
        print(json.dumps(summary, indent=2))
        return 0

    if args.command == "capture":
        floor = read_floor_value(args.floor)
        samples, unreadable = observations_from_capture(
            args.video,
            run_id=args.run_id,
            consumer_app=args.consumer_app,
            capture_start_epoch_ms=args.capture_start_epoch_ms,
            harness_floor_ms=floor,
        )
        write_samples(args.out, samples)
        print(
            json.dumps(
                {"samples": len(samples), "unreadable_frames": unreadable},
                indent=2,
            )
        )
        return 0 if samples else 1
    return 2


def read_floor_value(source: str) -> float:
    text = str(source).strip()
    try:
        return float(text)
    except ValueError:
        payload = json.loads(Path(source).read_text(encoding="utf-8"))
        return float(payload["harness_floor_ms"])


def write_samples(path: Path | str, samples: Sequence[LatencySample]) -> None:
    with Path(path).open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle)
        writer.writerow(HEADER)
        for sample in samples:
            writer.writerow(
                [
                    sample.sample_id,
                    sample.run_id,
                    sample.consumer_app,
                    sample.source_frame_no,
                    sample.source_epoch_ms,
                    f"{sample.capture_host_ms:.3f}",
                    f"{sample.clock_offset_ms:.3f}",
                    f"{sample.latency_ms:.3f}",
                    f"{sample.harness_floor_ms:.3f}",
                ]
            )


__all__ = [
    "EXCLUDED_STAGES",
    "HEADER",
    "INCLUDED_STAGES",
    "LatencySample",
    "decode_marker_payload",
    "frame_timestamps_ms",
    "observations_from_capture",
    "parse_marker_payload",
    "probe_geometry",
    "read_latency_csv",
    "read_floor_value",
    "summarize",
    "write_samples",
    "write_summary",
    "floor_corrected",
]


if __name__ == "__main__":
    sys.exit(main())
