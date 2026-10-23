"""T066 — pipeline analyser tests."""

from __future__ import annotations

import json

import pytest

from analysis import floor as floor_module
from analysis import latency, stages
from metrics import criteria


def samples(latencies: list[float], *, floor_ms: float = 0.0) -> list[latency.LatencySample]:
    """Samples whose ``latency_ms`` is exactly the raw value given."""

    return [
        latency.LatencySample(
            sample_id=index + 1,
            run_id="lat-01",
            consumer_app="browser",
            source_frame_no=index,
            source_epoch_ms=1_700_000_000_000 + index * 33,
            capture_host_ms=1_700_000_000_000 + index * 33 + value,
            clock_offset_ms=0.0,
            latency_ms=value,
            harness_floor_ms=floor_ms,
        )
        for index, value in enumerate(latencies)
    ]


def test_raw_and_corrected_percentiles_are_published_together() -> None:
    summary = latency.summarize(samples([50.0] * 99 + [200.0]), harness_floor_ms=20.0)

    assert summary["criterion"]["basis"] == "raw"
    assert summary["raw"]["p95_ms"] == pytest.approx(50.0)
    assert summary["corrected"]["p95_ms"] == pytest.approx(30.0)
    assert summary["corrected"]["p95_ms"] == pytest.approx(
        summary["raw"]["p95_ms"] - summary["harness_floor_ms"]
    )
    assert summary["verdict"] == "pass"
    assert "attribution" not in summary


def test_a_summary_without_three_figures_is_never_produced() -> None:
    summary = latency.summarize(samples([40.0, 60.0, 80.0]), harness_floor_ms=15.0)
    for key in ("harness_floor_ms", "raw", "corrected", "criterion", "verdict"):
        assert key in summary
    assert summary["included_stages"] == latency.INCLUDED_STAGES
    assert summary["excluded_stages"] == latency.EXCLUDED_STAGES


def test_the_gate_is_judged_on_the_raw_value_not_the_corrected_one() -> None:
    # 160 ms raw, 60 ms floor: corrected 100 ms is inside the threshold, but the
    # verdict must still be driven by the raw p95.
    summary = latency.summarize(samples([160.0] * 10), harness_floor_ms=60.0)
    assert summary["raw"]["p95_ms"] == pytest.approx(160.0)
    assert summary["corrected"]["p95_ms"] == pytest.approx(100.0)
    assert summary["verdict"] == "fail"
    assert "capture-procedure limitation" in summary["attribution"]


def test_a_product_regression_is_not_blamed_on_the_harness() -> None:
    summary = latency.summarize(samples([200.0] * 10), harness_floor_ms=20.0)
    assert summary["verdict"] == "fail"
    assert "not attributable to the capture procedure" in summary["attribution"]


def test_cpu_normalisation_is_single_core_over_logical_cores() -> None:
    assert criteria.cpu_pct_machine(100.0, 16) == pytest.approx(6.25)
    assert criteria.cpu_pct_machine(50.0, 4) == pytest.approx(12.5)
    with pytest.raises(ValueError):
        criteria.cpu_pct_machine(10.0, 0)


def test_stage_budget_includes_and_excludes_the_right_stages() -> None:
    document = stages.build_stage_budget(
        "lat-01",
        {
            "network_and_jitter_buffer": 12.0,
            "decode": 8.0,
            "frame_handoff": 2.0,
            "consumer_presentation": 15.0,
        },
    )
    by_stage = {stage["stage"]: stage for stage in document["stages"]}
    assert by_stage["capture"]["included_in_gate"] is False
    assert by_stage["encode"]["included_in_gate"] is False
    assert by_stage["decode"]["included_in_gate"] is True
    assert document["included_total_ms"] == pytest.approx(37.0)
    assert document["total_budget_ms"] == pytest.approx(100.0)


def test_the_stage_sum_rule_is_enforced() -> None:
    document = stages.build_stage_budget(
        "lat-01",
        {
            "network_and_jitter_buffer": 60.0,
            "decode": 40.0,
            "frame_handoff": 5.0,
            "consumer_presentation": 20.0,
        },
    )
    with pytest.raises(ValueError, match="above the raw p95"):
        stages.enforce_sum_rule(document, 100.0)

    enforced = stages.enforce_sum_rule(document, 130.0)
    assert enforced["sum_rule_holds"] is True


def test_a_missing_included_stage_is_an_error() -> None:
    with pytest.raises(ValueError, match="included in the gate"):
        stages.build_stage_budget("lat-01", {"network_and_jitter_buffer": 10.0})


def test_floor_is_measured_as_a_conservative_p95() -> None:
    document = floor_module.measure_floor(samples([5.0] * 20, floor_ms=0.0))
    assert document["harness_floor_ms"] == pytest.approx(5.0)
    assert document["outside_expected_range"] is True


def test_floor_inside_the_expected_range_is_accepted() -> None:
    values = [30.0, 31.0, 32.0, 33.0] * 3
    document = floor_module.measure_floor(samples(values, floor_ms=0.0))
    assert document["outside_expected_range"] is False


def test_the_latency_csv_round_trips(tmp_path) -> None:
    path = tmp_path / "latency_samples.csv"
    original = samples([30.0, 40.0, 50.0])
    latency.write_samples(path, original)
    read_back = latency.read_latency_csv(path)
    assert [sample.latency_ms for sample in read_back] == pytest.approx(
        [sample.latency_ms for sample in original]
    )
    assert path.read_text(encoding="utf-8").splitlines()[0] == ",".join(latency.HEADER)


def test_the_corrected_p95_cannot_go_negative() -> None:
    with pytest.raises(ValueError):
        criteria.floor_corrected(10.0, 20.0)


def test_marker_payload_parses_the_contract_format() -> None:
    assert latency.parse_marker_payload("1700000000123|77") == (1_700_000_000_123, 77)


def test_summary_is_json_serialisable() -> None:
    summary = latency.summarize(samples([40.0]), harness_floor_ms=10.0)
    assert json.loads(json.dumps(summary))["criterion"]["metric"] == "raw.p95_ms"
