#!/usr/bin/env python3
"""Regenerate the committed calibration fixture.

The fixture is synthetic: four voiced, formant-shaped tone bursts separated by
40 ms, 60 ms and 120 ms of digital silence. It contains no recorded human
speech and no personal data. The generation is deterministic, so a second
engineer can reproduce the file byte for byte::

    python spikes/fixtures/tools/generate_calibration.py

The exact bytes are committed; running this script again must not change them.
"""

from __future__ import annotations

import json
import wave
from pathlib import Path

import numpy as np

SAMPLE_RATE_HZ = 48_000
CHANNELS = 1
SAMPLE_WIDTH_BYTES = 2
SEGMENT_S = 1.2
GAPS_MS = (40, 60, 120)
OUTPUT = Path(__file__).resolve().parents[1] / "calibration-40-60-120.wav"
METADATA = OUTPUT.with_suffix(".json")


def voiced_segment(seconds: float, seed: int) -> np.ndarray:
    """A deterministic speech-like burst: a vibrato fundamental plus formants."""

    rng = np.random.default_rng(seed)
    time = np.arange(int(seconds * SAMPLE_RATE_HZ)) / SAMPLE_RATE_HZ
    fundamental = 118.0 + 9.0 * np.sin(2.0 * np.pi * 0.8 * time + seed)
    formants = ((520.0, 90.0, 1.0), (1_480.0, 140.0, 0.6), (2_540.0, 220.0, 0.35))

    signal = np.zeros_like(time)
    for harmonic in range(1, 25):
        frequency = harmonic * fundamental
        amplitude = 1.0 / harmonic
        for centre, width, weight in formants:
            amplitude += weight * np.exp(-0.5 * ((frequency - centre) / width) ** 2)
        signal += (amplitude / harmonic) * np.sin(2.0 * np.pi * frequency * time)

    syllable = 0.55 + 0.45 * np.sin(2.0 * np.pi * 3.1 * time + 0.4)
    signal *= syllable
    signal += 0.008 * rng.standard_normal(time.size)

    fade = int(0.02 * SAMPLE_RATE_HZ)
    envelope = np.ones_like(time)
    envelope[:fade] = np.linspace(0.0, 1.0, fade)
    envelope[-fade:] = np.linspace(1.0, 0.0, fade)
    signal *= envelope

    peak = float(np.max(np.abs(signal)))
    return 0.45 * signal / peak


def main() -> None:
    pieces: list[np.ndarray] = []
    for index, gap_ms in enumerate((*GAPS_MS, None), start=1):
        pieces.append(voiced_segment(SEGMENT_S, seed=index))
        if gap_ms is not None:
            pieces.append(np.zeros(int(gap_ms / 1000.0 * SAMPLE_RATE_HZ)))

    signal = np.concatenate(pieces)
    pcm = np.clip(np.round(signal * 32767.0), -32768, 32767).astype("<i2")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(OUTPUT), "wb") as handle:
        handle.setnchannels(CHANNELS)
        handle.setsampwidth(SAMPLE_WIDTH_BYTES)
        handle.setframerate(SAMPLE_RATE_HZ)
        handle.writeframes(pcm.tobytes())

    METADATA.write_text(
        json.dumps(
            {
                "fixture": OUTPUT.name,
                "provenance": "synthetic; no recorded human speech",
                "generator": "spikes/fixtures/tools/generate_calibration.py",
                "sample_rate_hz": SAMPLE_RATE_HZ,
                "channels": CHANNELS,
                "sample_width_bytes": SAMPLE_WIDTH_BYTES,
                "segment_s": SEGMENT_S,
                "gaps_ms": list(GAPS_MS),
                "expected_dropouts": 2,
                "below_threshold_gap_ms": 40,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {OUTPUT} ({signal.size / SAMPLE_RATE_HZ:.3f} s)")


if __name__ == "__main__":
    main()
