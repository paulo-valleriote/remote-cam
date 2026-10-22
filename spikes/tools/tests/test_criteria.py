from __future__ import annotations

import math

import pytest

from metrics.criteria import (
    VERDICT_CONDITIONAL,
    VERDICT_FAIL,
    VERDICT_PASS,
    cpu_criterion,
    cpu_pct_machine,
    drift_verdict,
    floor_corrected,
    latency_criterion,
    memory_criterion,
    percentile,
    percentile_set,
    realised_loss_pct,
    stage_budget_total,
    stage_budget_within,
)


def test_percentile_uses_linear_interpolation() -> None:
    values = range(1, 101)

    assert percentile(values, 50) == pytest.approx(50.5)
    assert percentile(values, 95) == pytest.approx(95.05)
    assert percentile(values, 99) == pytest.approx(99.01)
    assert percentile(values, 100) == pytest.approx(100.0)


def test_percentile_set_returns_the_contract_shape() -> None:
    result = percentile_set([10, 20, 30, 40], suffix="_ms")

    assert set(result) == {"p50_ms", "p95_ms", "p99_ms", "max_ms"}
    assert result["max_ms"] == 40.0
    assert result["p50_ms"] == pytest.approx(25.0)


def test_percentile_rejects_empty_and_non_finite_input() -> None:
    with pytest.raises(ValueError):
        percentile([], 50)
    with pytest.raises(ValueError):
        percentile([1.0, math.nan], 50)
    with pytest.raises(ValueError):
        percentile([1.0], 101)


def test_single_value_percentiles_collapse_to_that_value() -> None:
    assert percentile_set([7.5]) == {
        "p50": 7.5,
        "p95": 7.5,
        "p99": 7.5,
        "max": 7.5,
    }


def test_latency_gate_is_judged_on_the_raw_p95_inclusively() -> None:
    passing = latency_criterion(150.0)

    assert passing["basis"] == "raw"
    assert passing["metric"] == "raw.p95_ms"
    assert passing["passed"] is True
    assert latency_criterion(150.1)["passed"] is False


def test_latency_gate_rejects_negative_input() -> None:
    with pytest.raises(ValueError):
        latency_criterion(-1.0)


def test_resource_gate_uses_cpu_p95_and_memory_max() -> None:
    assert cpu_criterion(15.0)["metric"] == "cpu_pct_machine.p95"
    assert cpu_criterion(15.0)["passed"] is True
    assert cpu_criterion(15.01)["passed"] is False
    assert memory_criterion(400.0)["metric"] == "private_working_set_mb.max"
    assert memory_criterion(400.0)["passed"] is True
    assert memory_criterion(400.01)["passed"] is False


def test_drift_passes_when_the_whole_interval_is_within_the_threshold() -> None:
    assert drift_verdict(4.03, (1.4, 6.7)) == VERDICT_PASS
    assert drift_verdict(20.0, (18.0, 20.0)) == VERDICT_PASS


def test_drift_interval_straddling_the_threshold_is_conditional() -> None:
    assert drift_verdict(19.5, (18.0, 21.0)) == VERDICT_CONDITIONAL


def test_drift_above_the_threshold_fails() -> None:
    assert drift_verdict(20.1, (19.0, 22.0)) == VERDICT_FAIL
    assert drift_verdict(4.0, (20.5, 25.0)) == VERDICT_FAIL


def test_drift_rejects_reversed_interval_and_negative_drift() -> None:
    with pytest.raises(ValueError):
        drift_verdict(4.0, (6.0, 1.0))
    with pytest.raises(ValueError):
        drift_verdict(-4.0, (1.0, 6.0))


def test_floor_corrected_subtracts_the_harness_floor() -> None:
    assert floor_corrected(118.6, 33.4) == pytest.approx(85.2)


def test_floor_corrected_rejects_impossible_inputs() -> None:
    with pytest.raises(ValueError):
        floor_corrected(100.0, -1.0)
    with pytest.raises(ValueError):
        floor_corrected(10.0, 33.4)


def test_stage_budget_sums_only_included_stages() -> None:
    stages = [
        {"stage": "capture", "included_in_gate": False, "measured_ms": None},
        {"stage": "network_and_jitter_buffer", "included_in_gate": True, "measured_ms": 21.4},
        {"stage": "decode", "included_in_gate": True, "measured_ms": 9.8},
        {"stage": "frame_handoff", "included_in_gate": True, "measured_ms": 1.6},
        {"stage": "consumer_presentation", "included_in_gate": True, "measured_ms": 41.9},
    ]

    assert stage_budget_total(stages) == pytest.approx(74.7)
    assert stage_budget_within(118.6, stages) is True
    assert stage_budget_within(70.0, stages) is False


def test_stage_budget_requires_a_measurement_for_an_included_stage() -> None:
    with pytest.raises(ValueError):
        stage_budget_total([{"stage": "decode", "included_in_gate": True, "measured_ms": None}])


def test_stage_budget_tolerance() -> None:
    stages = [{"stage": "decode", "included_in_gate": True, "measured_ms": 10.0}]

    assert stage_budget_within(9.9, stages) is False
    assert stage_budget_within(9.9, stages, tolerance_ms=0.1) is True


def test_cpu_normalisation_matches_the_contract_rule() -> None:
    assert cpu_pct_machine(48.0, 16) == pytest.approx(3.0)
    assert cpu_pct_machine(100.0, 4) == pytest.approx(25.0)
    with pytest.raises(ValueError):
        cpu_pct_machine(10.0, 0)
    with pytest.raises(ValueError):
        cpu_pct_machine(-1.0, 4)


def test_realised_loss_is_computed_from_the_relay_log_counts() -> None:
    assert realised_loss_pct(30, 1000) == pytest.approx(3.0)
    assert realised_loss_pct(0, 1000) == pytest.approx(0.0)
    with pytest.raises(ValueError):
        realised_loss_pct(1, 0)
    with pytest.raises(ValueError):
        realised_loss_pct(11, 10)
