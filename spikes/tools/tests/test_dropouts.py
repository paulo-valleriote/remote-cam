from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest
import soundfile as sf

from analysis.dropouts import detect_dropouts

FIXTURE = Path(__file__).resolve().parents[2] / "fixtures" / "calibration-40-60-120.wav"


def tone(seconds: float, sample_rate: int = 48_000) -> np.ndarray:
    time = np.arange(int(seconds * sample_rate)) / sample_rate
    return 0.4 * np.sin(2.0 * np.pi * 440.0 * time)


def test_calibration_fixture_yields_exactly_two_dropouts() -> None:
    samples, sample_rate = sf.read(FIXTURE)

    result = detect_dropouts(samples, sample_rate)

    assert result.dropout_count == 2
    assert [round(interval.duration_ms) for interval in result.dropouts] == [60, 120]
    assert len(result.below_threshold) == 1
    assert round(result.below_threshold[0].duration_ms) == 40


def test_dropouts_are_measured_from_the_threshold() -> None:
    sample_rate = 48_000
    samples = np.concatenate(
        [
            tone(0.4, sample_rate),
            np.zeros(int(0.045 * sample_rate)),
            tone(0.4, sample_rate),
            np.zeros(int(0.055 * sample_rate)),
            tone(0.4, sample_rate),
        ]
    )

    result = detect_dropouts(samples, sample_rate)

    assert result.dropout_count == 1
    assert len(result.below_threshold) == 1


def test_minimum_duration_is_configurable() -> None:
    sample_rate = 48_000
    samples = np.concatenate(
        [
            tone(0.2, sample_rate),
            np.zeros(int(0.02 * sample_rate)),
            tone(0.2, sample_rate),
        ]
    )

    assert detect_dropouts(samples, sample_rate, min_dropout_ms=10).dropout_count == 1
    assert detect_dropouts(samples, sample_rate, min_dropout_ms=50).dropout_count == 0


def test_invalid_input_is_rejected() -> None:
    with pytest.raises(ValueError):
        detect_dropouts([], 48_000)
    with pytest.raises(ValueError):
        detect_dropouts([0.0, 0.1], 0)
    with pytest.raises(ValueError):
        detect_dropouts([0.0, 0.1], 48_000, min_dropout_ms=0)
