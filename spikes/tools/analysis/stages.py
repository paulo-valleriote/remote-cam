"""Latency stage budget (T079).

The source design budgets ≈ 100 ms across six stages. The isolated pipeline
measurement includes the four receiver-side stages and excludes mobile capture
and encode, and the report must say so. The sum rule is enforced here: the
included stages may not add up to more than the raw p95 they are part of.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Final

from metrics.criteria import stage_budget_total, stage_budget_within

STAGE_BUDGETS: Final[dict[str, float]] = {
    "capture": 33.0,
    "encode": 15.0,
    "network_and_jitter_buffer": 20.0,
    "decode": 12.0,
    "frame_handoff": 3.0,
    "consumer_presentation": 17.0,
}

EXCLUDED_METHODS: Final[dict[str, str]] = {
    "capture": "excluded: mobile capture is out of scope for the isolated pipeline measurement",
    "encode": "excluded: mobile encode is out of scope for the isolated pipeline measurement",
}

INCLUDED_METHODS: Final[dict[str, str]] = {
    "network_and_jitter_buffer": (
        "jitter buffer playout timestamp minus packet arrival timestamp, p95"
    ),
    "decode": "media foundation transform output timestamp minus input timestamp, p95",
    "frame_handoff": "ring buffer publish timestamp minus decode output timestamp, p95",
    "consumer_presentation": "consumer capture timestamp minus ring buffer publish timestamp, p95",
}


def build_stage_budget(
    run_id: str,
    measured_ms: Mapping[str, float],
) -> dict:
    """Build ``stage_budget.json`` from measured stage values.

    Every stage appears; an included stage without a measurement is an error,
    because a missing stage would silently shrink the budget.
    """

    stages = []
    for stage, budget in STAGE_BUDGETS.items():
        included = stage in INCLUDED_METHODS
        value = measured_ms.get(stage)
        if included and value is None:
            raise ValueError(f"stage {stage} is included in the gate but was not measured")
        if value is not None and value < 0:
            raise ValueError(f"stage {stage} has a negative measurement")
        stages.append(
            {
                "stage": stage,
                "budget_ms": budget,
                "measured_ms": None if value is None else float(value),
                "included_in_gate": included,
                "measurement_method": INCLUDED_METHODS.get(stage, EXCLUDED_METHODS.get(stage, "")),
            }
        )

    return {
        "run_id": run_id,
        "total_budget_ms": sum(STAGE_BUDGETS.values()),
        "stages": stages,
        "included_total_ms": stage_budget_total(stages),
    }


def enforce_sum_rule(document: dict, raw_p95_ms: float, *, tolerance_ms: float = 0.0) -> dict:
    """Fail when the included stages exceed the raw p95 for the same run."""

    if not stage_budget_within(raw_p95_ms, document["stages"], tolerance_ms=tolerance_ms):
        raise ValueError(
            "the included stages sum to "
            f"{stage_budget_total(document['stages']):.2f} ms, above the raw p95 "
            f"{raw_p95_ms:.2f} ms; state the missing stage or fix the instrumentation"
        )
    document["raw_p95_ms"] = raw_p95_ms
    document["sum_rule_holds"] = True
    return document


def write_document(path: Path | str, document: dict) -> None:
    Path(path).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--raw-p95-ms", type=float, required=True)
    parser.add_argument(
        "--measured",
        required=True,
        help="JSON object of stage -> measured_ms (excluded stages may be omitted)",
    )
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)

    measured = json.loads(args.measured)
    document = build_stage_budget(args.run_id, measured)
    enforce_sum_rule(document, args.raw_p95_ms)
    write_document(args.out, document)
    print(json.dumps(document, indent=2))
    return 0


__all__ = [
    "EXCLUDED_METHODS",
    "INCLUDED_METHODS",
    "STAGE_BUDGETS",
    "build_stage_budget",
    "enforce_sum_rule",
    "main",
    "write_document",
]


if __name__ == "__main__":
    sys.exit(main())
