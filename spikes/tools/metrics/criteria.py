"""Pure criterion functions for every gated F0 measurement.

The gate values live here once, so the analysers, the report assembly and the
tests cannot drift apart. Every function is pure: given the same inputs it
returns the same result and writes nothing.

The criteria are those of ``specs/001-f0-technical-spikes/contracts/
measurement-formats.md`` and ``spec.md``:

* latency gate -- **raw** capture-at-consumer p95 <= 150 ms;
* resource gate -- machine-normalised CPU p95 <= 15 % and private working-set
  maximum <= 400 MB;
* drift gate -- <= 20 ms/h, and a 95 % interval that straddles 20 ms/h does
  **not** pass;
* audio gate -- a dropout is a silence interval of at least 50 ms.
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping, Sequence
from typing import Any, Final

import numpy as np

LATENCY_P95_MS: Final = 150.0
CPU_P95_PCT: Final = 15.0
MEMORY_MAX_MB: Final = 400.0
DRIFT_MS_PER_HOUR: Final = 20.0
MIN_DROPOUT_MS: Final = 50.0

VERDICT_PASS: Final = "pass"
VERDICT_CONDITIONAL: Final = "conditional"
VERDICT_FAIL: Final = "fail"


def _as_float_array(values: Iterable[float], *, name: str) -> np.ndarray:
    array = np.asarray(list(values), dtype=float)
    if array.size == 0:
        raise ValueError(f"{name} requires at least one value")
    if not np.all(np.isfinite(array)):
        raise ValueError(f"{name} received a non-finite value")
    return array


def percentile(values: Iterable[float], q: float) -> float:
    """Linear-interpolated percentile, matching numpy's default definition."""

    if not 0.0 <= q <= 100.0:
        raise ValueError("q must be within [0, 100]")
    array = _as_float_array(values, name="percentile")
    return float(np.percentile(array, q, method="linear"))


def percentile_set(values: Iterable[float], *, suffix: str = "") -> dict[str, float]:
    """Return p50, p95, p99 and max, optionally suffixed with a unit.

    ``suffix="_ms"`` produces ``{"p50_ms": ..., "p95_ms": ..., "p99_ms": ...,
    "max_ms": ...}``, which is the shape the evidence contracts use.
    """

    array = _as_float_array(values, name="percentile_set")
    p50, p95, p99 = np.percentile(array, [50, 95, 99], method="linear")
    return {
        f"p50{suffix}": float(p50),
        f"p95{suffix}": float(p95),
        f"p99{suffix}": float(p99),
        f"max{suffix}": float(array.max()),
    }


def _lte(value: float, threshold: float) -> bool:
    """Inclusive comparison, with a tolerance that absorbs four-decimal rounding."""

    return value <= threshold + 1e-9


def latency_criterion(raw_p95_ms: float, threshold_ms: float = LATENCY_P95_MS) -> dict[str, Any]:
    """Judge the latency gate on the **raw** p95; the corrected value is never judged."""

    if raw_p95_ms < 0:
        raise ValueError("raw_p95_ms must be non-negative")
    return {
        "metric": "raw.p95_ms",
        "threshold": threshold_ms,
        "comparison": "<=",
        "basis": "raw",
        "value": raw_p95_ms,
        "passed": _lte(raw_p95_ms, threshold_ms),
    }


def cpu_criterion(cpu_p95_pct: float, threshold_pct: float = CPU_P95_PCT) -> dict[str, Any]:
    """Judge the resource gate's CPU statistic, which is the p95 and never the mean."""

    if cpu_p95_pct < 0:
        raise ValueError("cpu_p95_pct must be non-negative")
    return {
        "metric": "cpu_pct_machine.p95",
        "threshold": threshold_pct,
        "comparison": "<=",
        "value": cpu_p95_pct,
        "passed": _lte(cpu_p95_pct, threshold_pct),
    }


def memory_criterion(
    private_working_set_max_mb: float, threshold_mb: float = MEMORY_MAX_MB
) -> dict[str, Any]:
    """Judge the resource gate's memory statistic, which is the maximum, not the mean."""

    if private_working_set_max_mb < 0:
        raise ValueError("private_working_set_max_mb must be non-negative")
    return {
        "metric": "private_working_set_mb.max",
        "threshold": threshold_mb,
        "comparison": "<=",
        "value": private_working_set_max_mb,
        "passed": _lte(private_working_set_max_mb, threshold_mb),
    }


def drift_verdict(
    drift_ms_per_hour: float,
    ci95_ms_per_hour: tuple[float, float],
    threshold_ms_per_hour: float = DRIFT_MS_PER_HOUR,
) -> str:
    """Return ``pass``, ``conditional`` or ``fail`` for the drift criterion.

    A point estimate above the threshold fails outright. A point estimate within
    the threshold whose interval reaches above it is ``conditional``: the
    measurement cannot certify the criterion and the marker density must be
    increased. An interval wholly above the threshold also fails.
    """

    low, high = ci95_ms_per_hour
    if low > high:
        raise ValueError("ci95_ms_per_hour must be ordered low, high")
    if drift_ms_per_hour < 0:
        raise ValueError("drift_ms_per_hour must be non-negative")

    if drift_ms_per_hour > threshold_ms_per_hour or low > threshold_ms_per_hour:
        return VERDICT_FAIL
    if high > threshold_ms_per_hour:
        return VERDICT_CONDITIONAL
    return VERDICT_PASS


def floor_corrected(raw_p95_ms: float, harness_floor_ms: float) -> float:
    """Subtract the measured harness floor from the raw p95.

    The corrected figure is reported next to the raw one and is never the value
    the gate is judged on.
    """

    if harness_floor_ms < 0:
        raise ValueError("harness_floor_ms must be non-negative")
    if harness_floor_ms > raw_p95_ms:
        raise ValueError("harness_floor_ms cannot exceed raw_p95_ms")
    return raw_p95_ms - harness_floor_ms


def stage_budget_total(stages: Sequence[Mapping[str, Any]]) -> float:
    """Sum ``measured_ms`` over the stages included in the gate.

    A stage with ``included_in_gate`` false, or with ``measured_ms`` null, does
    not contribute.
    """

    total = 0.0
    for stage in stages:
        if not stage.get("included_in_gate", False):
            continue
        measured = stage.get("measured_ms")
        if measured is None:
            raise ValueError(
                f"stage {stage.get('stage', '<unnamed>')} is included but has no measured_ms"
            )
        value = float(measured)
        if value < 0:
            raise ValueError("measured_ms must be non-negative")
        total += value
    return total


def stage_budget_within(
    raw_p95_ms: float,
    stages: Sequence[Mapping[str, Any]],
    *,
    tolerance_ms: float = 0.0,
) -> bool:
    """Check the contract's rule: included stages sum to no more than the raw p95."""

    if tolerance_ms < 0:
        raise ValueError("tolerance_ms must be non-negative")
    return stage_budget_total(stages) <= raw_p95_ms + tolerance_ms


def cpu_pct_machine(cpu_pct_single_core: float, cpu_logical_cores: int) -> float:
    """Normalise a single-core percentage to the whole machine.

    The resource gate is judged on this value: a process using one core fully on
    a 16-core machine consumes 6.25 % of the machine.
    """

    if cpu_logical_cores < 1:
        raise ValueError("cpu_logical_cores must be at least 1")
    if cpu_pct_single_core < 0:
        raise ValueError("cpu_pct_single_core must be non-negative")
    return cpu_pct_single_core / cpu_logical_cores


def realised_loss_pct(dropped: int, total: int) -> float:
    """Compute the realised packet loss from the relay log."""

    if total <= 0:
        raise ValueError("total must be positive")
    if dropped < 0 or dropped > total:
        raise ValueError("dropped must be within [0, total]")
    return 100.0 * dropped / total


__all__ = [
    "CPU_P95_PCT",
    "DRIFT_MS_PER_HOUR",
    "LATENCY_P95_MS",
    "MEMORY_MAX_MB",
    "MIN_DROPOUT_MS",
    "VERDICT_CONDITIONAL",
    "VERDICT_FAIL",
    "VERDICT_PASS",
    "cpu_criterion",
    "cpu_pct_machine",
    "drift_verdict",
    "floor_corrected",
    "latency_criterion",
    "memory_criterion",
    "percentile",
    "percentile_set",
    "realised_loss_pct",
    "stage_budget_total",
    "stage_budget_within",
]
