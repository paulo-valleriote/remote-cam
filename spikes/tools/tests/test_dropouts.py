from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest
import soundfile as sf

from analysis.dropouts import (
    Calibration,
    CalibrationError,
    SilenceInterval,
    build_document,
    compare_detectors,
    detect_dropouts,
)

FIXTURE = Path(__file__).resolve().parents[2] / "fixtures" / "calibration-40-60-120.wav"


def tone(seconds: float, sample_rate: int = 48_000) -> np.ndarray:
    time = np.arange(int(seconds * sample_rate)) / sample_rate
    return 0.4 * np.sin(2.0 * np.pi * 440.0 * time)


def test_calibration_fixture_yields_exactly_two_dropouts() -> None:
    samples, sample_rate = sf.read(FIXTURE)

    result = detect_dropouts(samples, sample_rate)

    assert result.dropout_count == 2
    # The 5 ms RMS window and the segment fades move the detected edges by up to
    # one window; the fixture contract only fixes the count.
    assert result.dropouts[0].duration_ms == pytest.approx(60, abs=10)
    assert result.dropouts[1].duration_ms == pytest.approx(120, abs=10)
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


def test_interval_shorter_than_50_ms_is_below_threshold() -> None:
    sample_rate = 48_000
    samples = np.concatenate(
        [
            tone(0.2, sample_rate),
            np.zeros(int(0.049 * sample_rate)),
            tone(0.2, sample_rate),
        ]
    )

    result = detect_dropouts(samples, sample_rate, window_ms=1.0)

    assert result.dropout_count == 0
    assert len(result.below_threshold) == 1
    assert round(result.below_threshold[0].duration_ms) == 49


def test_detector_disagreement_is_a_finding() -> None:
    primary = [SilenceInterval(start_s=1.0, end_s=1.1)]
    cross_check = [SilenceInterval(start_s=1.0, end_s=1.0)]

    agree, findings = compare_detectors(primary, cross_check)

    assert agree is False
    assert findings
    assert "interval 1 differs" in findings[0]


def test_detector_count_mismatch_is_a_finding() -> None:
    agree, findings = compare_detectors(
        [SilenceInterval(start_s=1.0, end_s=1.1)],
        [],
    )

    assert agree is False
    assert "detector count mismatch" in findings[0]


def test_agreeing_detectors_produce_no_findings() -> None:
    interval = SilenceInterval(start_s=1.0, end_s=1.1)

    agree, findings = compare_detectors([interval], [interval])

    assert agree is True
    assert findings == []


def test_missing_calibration_invalidates_the_threshold() -> None:
    with pytest.raises(CalibrationError):
        build_document(
            run_id="aud-01",
            capture_file="consumer_capture.wav",
            sample_rate_hz=48_000,
            channels=1,
            duration_s=600.0,
        )


def test_document_records_calibrated_threshold_and_disagreement() -> None:
    calibration = Calibration(noise_floor_dbfs=-56.0, threshold_dbfs=-50.0)
    interval = SilenceInterval(start_s=10.0, end_s=10.06)

    document = build_document(
        run_id="aud-01",
        capture_file="consumer_capture.wav",
        sample_rate_hz=48_000,
        channels=1,
        duration_s=600.0,
        calibration=calibration,
        silencedetect=[interval],
        rms_window=[],
    )

    assert document["criterion"]["threshold_dbfs"] == -50.0
    assert "noise floor -56.0 dBFS" in document["criterion"]["threshold_source"]
    assert document["detectors_agree"] is False
    assert document["findings"]
    assert document["verdict"] == "fail"


def test_document_passes_a_clean_calibrated_run() -> None:
    calibration = Calibration(noise_floor_dbfs=-56.0, threshold_dbfs=-50.0)

    document = build_document(
        run_id="aud-01",
        capture_file="consumer_capture.wav",
        sample_rate_hz=48_000,
        channels=1,
        duration_s=600.0,
        calibration=calibration,
        below_threshold_intervals=3,
    )

    assert document["detectors_agree"] is True
    assert document["below_threshold_intervals"] == 3
    assert document["verdict"] == "pass"


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
