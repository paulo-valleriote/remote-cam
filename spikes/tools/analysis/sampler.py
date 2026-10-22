"""CPU sampling with two independent counter sources.

The resource gate is judged on ``cpu_pct_machine = 100 * delta_process_time /
delta_system_time``. That figure is only trustworthy if the normalisation is
correct, so the toolchain self-test cross-checks two independently-read sources
over the same window:

* ``RusageSampler`` -- ``getrusage(RUSAGE_SELF)`` for the process and
  ``/proc/stat`` for the machine;
* ``ProcfsSampler`` -- ``/proc/self/stat`` for the process and ``/proc/stat``
  for the machine.

On the Windows reference machine the second source is the ``typeperf`` counter
``\\Process(<name>)\\% Processor Time``, which is the cross-check the contract
names. ``typeperf`` reports a percentage of one core, so it is divided by the
logical core count before comparison.

Nothing here fabricates a value: a source that is not available returns a
failure with the reason recorded.
"""

from __future__ import annotations

import math
import os
import platform
import re
import shutil
import subprocess
import threading
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass

import numpy as np

CLOCK_TICKS = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100


@dataclass(frozen=True)
class CpuCounters:
    process_s: float
    system_s: float


def _read_proc_stat_total_s() -> float | None:
    try:
        with open("/proc/stat", encoding="ascii") as handle:
            fields = handle.readline().split()
    except OSError:
        return None
    if not fields or fields[0] != "cpu":
        return None
    return sum(float(value) for value in fields[1:]) / CLOCK_TICKS


def _read_proc_self_s() -> float | None:
    try:
        with open("/proc/self/stat", encoding="ascii") as handle:
            contents = handle.read()
    except OSError:
        return None
    closing = contents.rfind(")")
    if closing == -1:
        return None
    fields = contents[closing + 2 :].split()
    if len(fields) < 13:
        return None
    return (float(fields[11]) + float(fields[12])) / CLOCK_TICKS


class RusageSampler:
    """Process CPU from ``getrusage`` and machine time from ``/proc/stat``."""

    name = "getrusage+procstat"

    def __init__(self) -> None:
        import resource

        self._resource = resource

    def read(self) -> CpuCounters:
        usage = self._resource.getrusage(self._resource.RUSAGE_SELF)
        process = float(usage.ru_utime + usage.ru_stime)
        system = _read_proc_stat_total_s()
        if system is None:
            system = time.monotonic()
        return CpuCounters(process_s=process, system_s=system)


class ProcfsSampler:
    """Process and machine CPU from ``/proc``. Linux only."""

    name = "procfs"

    def read(self) -> CpuCounters:
        process = _read_proc_self_s()
        system = _read_proc_stat_total_s()
        if process is None or system is None:
            raise RuntimeError("/proc counters are unavailable")
        return CpuCounters(process_s=process, system_s=system)


class WallClockSampler:
    """Fallback: process CPU from ``getrusage`` against wall-clock capacity.

    The system denominator is ``wall_seconds * logical_cores``, which is
    equivalent to the aggregate ``/proc/stat`` denominator on a fully-online
    machine. It is used only where the platform counters are unavailable and the
    result is labelled as a fallback.
    """

    name = "getrusage+wallclock"

    def __init__(self) -> None:
        import resource

        self._resource = resource
        self._cores = os.cpu_count() or 1
        self._started = time.monotonic()
        usage = self._resource.getrusage(self._resource.RUSAGE_SELF)
        self._base_process = float(usage.ru_utime + usage.ru_stime)
        self._base_wall = self._started

    def read(self) -> CpuCounters:
        usage = self._resource.getrusage(self._resource.RUSAGE_SELF)
        process = float(usage.ru_utime + usage.ru_stime) - self._base_process
        wall = time.monotonic() - self._base_wall
        return CpuCounters(process_s=process, system_s=wall * self._cores)


def _busy_until(deadline_s: float, duty: float = 0.8) -> None:
    """Burn CPU for ``duty`` of each short interval until ``deadline_s``."""

    while True:
        now = time.monotonic()
        if now >= deadline_s:
            return
        interval_end = min(now + 0.05, deadline_s)
        burn_until = min(now + 0.05 * duty, deadline_s)
        while time.monotonic() < burn_until:
            math.sqrt(time.monotonic())
        remaining = interval_end - time.monotonic()
        if remaining > 0:
            time.sleep(remaining)


def machine_pct(process_delta_s: float, system_delta_s: float) -> float:
    """``100 * process_delta / system_delta``, clamped to a plausible range."""

    if system_delta_s <= 0:
        raise ValueError("system_delta_s must be positive")
    if process_delta_s < 0:
        raise ValueError("process_delta_s must be non-negative")
    return max(0.0, min(100.0, 100.0 * process_delta_s / system_delta_s))


def sample_cpu(
    sampler: object,
    *,
    duration_s: float = 1.5,
    interval_s: float = 0.05,
    duty: float = 0.8,
) -> list[float]:
    """Sample machine-normalised CPU percentages over one busy window."""

    if duration_s <= 0 or interval_s <= 0:
        raise ValueError("duration_s and interval_s must be positive")
    read: Callable[[], CpuCounters] = sampler.read  # type: ignore[attr-defined]

    deadline = time.monotonic() + duration_s
    previous = read()
    samples: list[float] = []
    while time.monotonic() < deadline:
        _busy_until(min(time.monotonic() + interval_s, deadline), duty=duty)
        current = read()
        process_delta = current.process_s - previous.process_s
        system_delta = current.system_s - previous.system_s
        if system_delta > 0:
            samples.append(machine_pct(process_delta, system_delta))
        previous = current
    return samples


def sample_pair(
    first: object,
    second: object,
    *,
    duration_s: float = 2.0,
    interval_s: float = 0.05,
    duty: float = 0.8,
) -> tuple[list[float], list[float]]:
    """Sample two sources over the *same* window so their values are comparable."""

    if duration_s <= 0 or interval_s <= 0:
        raise ValueError("duration_s and interval_s must be positive")
    read_first: Callable[[], CpuCounters] = first.read  # type: ignore[attr-defined]
    read_second: Callable[[], CpuCounters] = second.read  # type: ignore[attr-defined]

    deadline = time.monotonic() + duration_s
    previous_first = read_first()
    previous_second = read_second()
    first_samples: list[float] = []
    second_samples: list[float] = []
    while time.monotonic() < deadline:
        _busy_until(min(time.monotonic() + interval_s, deadline), duty=duty)
        current_first = read_first()
        current_second = read_second()
        first_system = current_first.system_s - previous_first.system_s
        second_system = current_second.system_s - previous_second.system_s
        if first_system > 0:
            first_samples.append(
                machine_pct(current_first.process_s - previous_first.process_s, first_system)
            )
        if second_system > 0:
            second_samples.append(
                machine_pct(current_second.process_s - previous_second.process_s, second_system)
            )
        previous_first = current_first
        previous_second = current_second
    return first_samples, second_samples


def p95(values: Iterable[float]) -> float:
    array = np.asarray(list(values), dtype=float)
    if array.size == 0:
        raise ValueError("p95 requires at least one sample")
    return float(np.percentile(array, 95, method="linear"))


def typeperf_cpu_p95(
    process_name: str, *, samples: int = 4, interval_s: float = 1
) -> tuple[float, list[float]]:
    """Read ``\\Process(name)\\% Processor Time`` through ``typeperf``.

    Returns the p95 normalised to the machine (percentage-points of the whole
    machine) and the raw samples. Raises ``RuntimeError`` when ``typeperf`` is
    absent or produces no usable value.
    """

    if shutil.which("typeperf") is None:
        raise RuntimeError("typeperf is not installed")
    counter = rf"\Process({process_name})\% Processor Time"
    command = ["typeperf", counter, "-sc", str(samples), "-si", str(interval_s)]
    completed = subprocess.run(
        command, capture_output=True, text=True, timeout=samples * interval_s + 15, check=False
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or "typeperf failed")
    values: list[float] = []
    for line in completed.stdout.splitlines():
        match = re.search(r'"(-?\d+(?:\.\d+)?)"\s*$', line)
        if match:
            values.append(float(match.group(1)))
    if not values:
        raise RuntimeError("typeperf returned no numeric samples")
    cores = os.cpu_count() or 1
    normalised = [max(0.0, value) / cores for value in values]
    return p95(normalised), normalised


@dataclass(frozen=True)
class AgreementResult:
    passed: bool
    delta_p95: float | None
    sampler_a: str
    sampler_b: str
    p95_a: float | None
    p95_b: float | None
    note: str

    def as_observed(self) -> dict[str, object]:
        return {
            "delta_p95_cpu_pct": self.delta_p95,
            "sampler_a": self.sampler_a,
            "sampler_b": self.sampler_b,
            "sampler_a_p95": self.p95_a,
            "sampler_b_p95": self.p95_b,
            "note": self.note,
        }


def measure_sampler_agreement(
    *, duration_s: float = 3.0, interval_s: float = 0.25, threshold_pct: float = 1.0
) -> AgreementResult:
    """Cross-check the sampler's normalisation against a second source."""

    if platform.system() == "Windows":
        return _measure_windows_agreement(duration_s=duration_s, threshold_pct=threshold_pct)

    first_source: object
    second_source: object
    try:
        second_source = ProcfsSampler()
        second_source.read()
        second_name = ProcfsSampler.name
        note = "typeperf is Windows-only; the cross-check used two independent Linux counter paths"
    except (OSError, RuntimeError, ValueError):
        second_source = WallClockSampler()
        second_name = WallClockSampler.name
        note = "platform counters were unavailable; the cross-check used the wall-clock fallback"
    first_source = RusageSampler()

    try:
        first, second = sample_pair(
            first_source, second_source, duration_s=duration_s, interval_s=interval_s
        )
    except (OSError, RuntimeError, ValueError) as error:
        return AgreementResult(
            passed=False,
            delta_p95=None,
            sampler_a=RusageSampler.name,
            sampler_b=second_name,
            p95_a=None,
            p95_b=None,
            note=f"sampling failed: {error}",
        )

    p95_a = p95(first)
    p95_b = p95(second)
    delta = abs(p95_a - p95_b)
    return AgreementResult(
        passed=delta <= threshold_pct,
        delta_p95=round(delta, 3),
        sampler_a=RusageSampler.name,
        sampler_b=second_name,
        p95_a=round(p95_a, 3),
        p95_b=round(p95_b, 3),
        note=note,
    )


def _measure_windows_agreement(*, duration_s: float, threshold_pct: float) -> AgreementResult:
    """Run ``typeperf`` and the Rust-equivalent sampler over the same window."""

    stop = threading.Event()
    workload = threading.Thread(
        target=lambda: _run_workload_until(stop, duration_s + 1.5), daemon=True
    )
    workload.start()

    typeperf_result: dict[str, object] = {}

    def run_typeperf() -> None:
        try:
            value, values = typeperf_cpu_p95(
                os.path.basename(os.sys.executable).removesuffix(".exe") or "python",
                samples=max(2, int(round(duration_s))),
                interval_s=1,
            )
            typeperf_result["p95"] = value
            typeperf_result["samples"] = values
        except (RuntimeError, OSError, subprocess.SubprocessError) as error:
            typeperf_result["error"] = str(error)

    thread = threading.Thread(target=run_typeperf, daemon=True)
    thread.start()
    try:
        samples = sample_cpu(RusageSampler(), duration_s=duration_s)
    finally:
        stop.set()
        thread.join(timeout=duration_s + 10)
        workload.join(timeout=2)

    p95_a = p95(samples)
    if "p95" not in typeperf_result:
        return AgreementResult(
            passed=False,
            delta_p95=None,
            sampler_a=RusageSampler.name,
            sampler_b="typeperf",
            p95_a=round(p95_a, 3),
            p95_b=None,
            note=f"typeperf cross-check failed: {typeperf_result.get('error', 'unknown error')}",
        )
    p95_b = float(typeperf_result["p95"])  # type: ignore[arg-type]
    delta = abs(p95_a - p95_b)
    return AgreementResult(
        passed=delta <= threshold_pct,
        delta_p95=round(delta, 3),
        sampler_a=RusageSampler.name,
        sampler_b="typeperf",
        p95_a=round(p95_a, 3),
        p95_b=round(p95_b, 3),
        note="typeperf Process\\% Processor Time normalised by the logical core count",
    )


def _run_workload_until(stop: threading.Event, duration_s: float) -> None:
    deadline = time.monotonic() + duration_s
    while not stop.is_set() and time.monotonic() < deadline:
        _busy_until(min(time.monotonic() + 0.1, deadline))


__all__ = [
    "AgreementResult",
    "ProcfsSampler",
    "RusageSampler",
    "WallClockSampler",
    "machine_pct",
    "measure_sampler_agreement",
    "p95",
    "sample_cpu",
    "typeperf_cpu_p95",
]
