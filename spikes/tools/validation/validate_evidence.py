"""Validate F0 spike evidence against the versioned contracts.

Two layers are applied:

1. **JSON Schema** validation of the artefact-only contracts
   (``gate-report.schema.json`` and ``injection-matrix.schema.json``,
   including their ``$defs`` for the environment and the hardware-decode
   verification).
2. **Field rules** from ``contracts/measurement-formats.md`` and the
   ``x-invariants`` of the gate report: semantics the schema cannot express,
   such as "the latency gate is judged on the raw p95", "a corrected figure
   equals the raw figure minus the harness floor", "a drift interval that
   straddles the threshold does not pass" and "both residue inspections use the
   same method".

Usage::

    python spikes/tools/validation/validate_evidence.py [paths...]
    python spikes/tools/validation/validate_evidence.py --evidence-root spikes/evidence --json

Exit status is 1 when at least one **error** is found. Warnings report a
reference-environment or completeness problem without making a present
artefact structurally invalid; ``--strict`` promotes them to failures.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from dataclasses import dataclass
from datetime import datetime
from functools import lru_cache
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT202012

_TOOLS_DIR = Path(__file__).resolve().parents[1]
if str(_TOOLS_DIR) not in sys.path:  # allow direct execution: python .../validate_evidence.py
    sys.path.insert(0, str(_TOOLS_DIR))

from metrics.criteria import (  # noqa: E402
    CPU_P95_PCT,
    DRIFT_MS_PER_HOUR,
    LATENCY_P95_MS,
    MEMORY_MAX_MB,
    MIN_DROPOUT_MS,
    drift_verdict,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
SCHEMA_DIR = REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts"
GATE_SCHEMA_PATH = SCHEMA_DIR / "gate-report.schema.json"
INJECTION_SCHEMA_PATH = SCHEMA_DIR / "injection-matrix.schema.json"
REASON_CODES_PATH = REPO_ROOT / "protocol" / "reason-codes.json"

ALLOWED_STAGES = {
    "capture",
    "encode",
    "network_and_jitter_buffer",
    "decode",
    "frame_handoff",
    "consumer_presentation",
}
ALLOWED_PAIRING_CATEGORIES = {"valid", "adversarial"}
RESIDUE_CASES = {"after_session_end", "after_application_restart"}
ARTEFACT_KINDS = {"session_key", "pairing_token", "fingerprint", "pairing_record"}
ALLOWED_LICENSE_MODELS = {
    "donationware",
    "proprietary_paid",
    "permissive_open_source",
    "own_driver",
    "plan_b_video_only",
}
ALLOWED_REDISTRIBUTION = {"yes_with_conditions", "no", "unverified"}
ALLOWED_SUPPORT_BURDEN = {"low", "medium", "high"}
REQUIRED_LICENSE_OPTIONS = {
    "redistribution_licensing",
    "user_installed_official_driver",
    "plan_b_video_only",
}

ENVIRONMENT = "environment"
TOOLCHAIN_SELFTEST = "toolchain_selftest"
INJECTION_MATRIX = "injection_matrix"
GATE_REPORT = "gate_report"
LATENCY_SUMMARY = "latency_summary"
STAGE_BUDGET = "stage_budget"
RESOURCE_SUMMARY = "resource_summary"
DRIFT_SUMMARY = "drift_summary"
DROPOUTS = "dropouts"
INTELLIGIBILITY = "intelligibility"
AUDIO_RUN_CAPTURE = "audio_run_capture"
PAIRING_SCENARIOS = "pairing_scenarios"
PAIRING_THROTTLING = "pairing_throttling"
PAIRING_RESIDUE = "pairing_residue"
IMPAIRMENT_PROFILE = "impairment_profile"
LICENSING_BUNDLE = "licensing_bundle"
RELAY_LOG = "relay_log"
OPEN_QUESTIONS = "open_questions"
LATENCY_SAMPLES = "latency_samples_csv"
RESOURCE_SAMPLES = "resource_samples_csv"
DRIFT_SAMPLES = "drift_samples_csv"

KNOWN_NAMES = {
    "environment.json": ENVIRONMENT,
    "toolchain_selftest.json": TOOLCHAIN_SELFTEST,
    "injection_matrix.json": INJECTION_MATRIX,
    "gate-report.json": GATE_REPORT,
    "gate_report.json": GATE_REPORT,
    "stage_budget.json": STAGE_BUDGET,
    "resource_summary.json": RESOURCE_SUMMARY,
    "drift_summary.json": DRIFT_SUMMARY,
    "dropouts.json": DROPOUTS,
    "intelligibility.json": INTELLIGIBILITY,
    "audio_run_capture.json": AUDIO_RUN_CAPTURE,
    "pairing_scenarios.json": PAIRING_SCENARIOS,
    "pairing_throttling.json": PAIRING_THROTTLING,
    "pairing_residue.json": PAIRING_RESIDUE,
    "impairment_profile.json": IMPAIRMENT_PROFILE,
    "licensing_bundle.json": LICENSING_BUNDLE,
    "relay_log.jsonl": RELAY_LOG,
    "open_questions.json": OPEN_QUESTIONS,
    "latency_samples.csv": LATENCY_SAMPLES,
    "resource_samples.csv": RESOURCE_SAMPLES,
    "drift_samples.csv": DRIFT_SAMPLES,
}

CSV_COLUMNS = {
    LATENCY_SAMPLES: [
        "sample_id",
        "run_id",
        "consumer_app",
        "source_frame_no",
        "source_epoch_ms",
        "capture_host_ms",
        "clock_offset_ms",
        "latency_ms",
        "harness_floor_ms",
    ],
    RESOURCE_SAMPLES: [
        "sample_id",
        "run_id",
        "elapsed_ms",
        "cpu_pct_machine",
        "cpu_pct_single_core",
        "private_working_set_bytes",
        "total_working_set_bytes",
        "peak_working_set_bytes",
        "thread_count",
        "handle_count",
    ],
    DRIFT_SAMPLES: [
        "sample_id",
        "run_id",
        "elapsed_s",
        "source_skew_ms",
        "audio_playout_delay_ms",
        "video_playout_delay_ms",
        "playout_skew_ms",
    ],
}


@dataclass(frozen=True)
class Finding:
    """One validation finding. ``level`` is ``error`` or ``warning``."""

    level: str
    code: str
    message: str
    path: str

    def as_dict(self) -> dict[str, str]:
        return {
            "level": self.level,
            "code": self.code,
            "message": self.message,
            "path": self.path,
        }


class _Report:
    def __init__(self, path: str) -> None:
        self.path = path
        self.findings: list[Finding] = []

    def error(self, code: str, message: str) -> None:
        self.findings.append(Finding("error", code, message, self.path))

    def warning(self, code: str, message: str) -> None:
        self.findings.append(Finding("warning", code, message, self.path))


@lru_cache(maxsize=1)
def _schemas() -> tuple[dict[str, Any], dict[str, Any], Registry]:
    gate = json.loads(GATE_SCHEMA_PATH.read_text(encoding="utf-8"))
    injection = json.loads(INJECTION_SCHEMA_PATH.read_text(encoding="utf-8"))
    registry = Registry().with_resources(
        [
            (gate["$id"], Resource.from_contents(gate, default_specification=DRAFT202012)),
            (
                injection["$id"],
                Resource.from_contents(injection, default_specification=DRAFT202012),
            ),
        ]
    )
    return gate, injection, registry


def _schema_findings(
    instance: Any, schema: dict[str, Any], registry: Registry, report: _Report, prefix: str
) -> None:
    validator = Draft202012Validator(schema, registry=registry, format_checker=FormatChecker())
    errors = sorted(
        validator.iter_errors(instance),
        key=lambda error: tuple(str(part) for part in error.absolute_path),
    )
    for error in errors:
        location = "/".join(str(part) for part in error.absolute_path) or "<root>"
        report.error("schema.invalid", f"{prefix}{location}: {error.message}")


@lru_cache(maxsize=4)
def _reason_codes(repo_root: str) -> frozenset[str]:
    document = json.loads((Path(repo_root) / "protocol" / "reason-codes.json").read_text("utf-8"))
    return frozenset(entry["code"] for entry in document["codes"])


def _is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _iso_with_offset(value: Any) -> bool:
    if not isinstance(value, str) or not value:
        return False
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return False
    return parsed.tzinfo is not None and parsed.utcoffset() is not None


def _timestamp(document: dict[str, Any], key: str, report: _Report) -> datetime | None:
    value = document.get(key)
    if not _iso_with_offset(value):
        report.error("timestamp.invalid", f"{key} MUST be ISO 8601 with an explicit UTC offset")
        return None
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def kind_for_path(path: Path) -> str | None:
    """Return the artefact kind for a path, or ``None`` when it is not validated."""

    name = path.name
    if name in KNOWN_NAMES:
        return KNOWN_NAMES[name]
    if name.startswith("latency_") and name.endswith("_summary.json"):
        return LATENCY_SUMMARY
    if "latency" in name and name.endswith("_summary.json"):
        return LATENCY_SUMMARY
    return None


def discover_evidence(root: Path) -> list[Path]:
    """Return every validatable artefact under ``root`` in a stable order."""

    files = [path for path in root.rglob("*") if path.is_file() and kind_for_path(path) is not None]
    return sorted(files, key=lambda path: (kind_for_path(path) != ENVIRONMENT, str(path)))


# ---------------------------------------------------------------------------
# Environment and toolchain
# ---------------------------------------------------------------------------


def _rule_environment(document: Any, report: _Report) -> None:
    gate, _, registry = _schemas()
    wrapper = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": f"{gate['$id']}#/$defs/environment",
    }
    _schema_findings(document, wrapper, registry, report, "environment: ")
    if not isinstance(document, dict):
        return

    verification = document.get("hardware_decode_verification")
    if not isinstance(verification, dict):
        return

    accelerated = verification.get("hardware_accelerated")
    output_format = verification.get("output_format_supported")
    decoder_found = verification.get("decoder_found")

    if accelerated is True and decoder_found is False:
        report.error(
            "environment.hardware_decode_inconsistent",
            "hardware_accelerated is true but decoder_found is false",
        )
    if accelerated is True and output_format != "nv12":
        report.error(
            "environment.output_format_not_nv12",
            "a hardware-accelerated reference requires output_format_supported = nv12",
        )
    if accelerated is not True or output_format != "nv12":
        report.warning(
            "environment.hardware_decode_unverified",
            "PC-1 is not satisfied: hardware decode / NV12 is unverified, so latency-gate results "
            "from this environment cannot be cited",
        )

    network = document.get("network")
    if not isinstance(network, dict):
        report.warning("environment.network_missing", "network baseline is absent")
        return
    if network.get("within_reference_conditions") is True:
        problems = []
        if network.get("band") == "2_4_ghz":
            problems.append("2.4 GHz")
        if network.get("wifi_power_save") == "enabled":
            problems.append("Wi-Fi power save enabled")
        if network.get("internet_enabled") is True:
            problems.append("internet enabled")
        if problems:
            report.error(
                "environment.reference_conditions_inconsistent",
                "within_reference_conditions is true but the network record reports "
                + ", ".join(problems),
            )
    else:
        report.warning(
            "environment.outside_reference",
            "within_reference_conditions is false: runs from this environment MUST be labelled "
            "outside-reference-environment and cannot support a go verdict",
        )


def _rule_toolchain_selftest(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("selftest.invalid", "the self-test artefact MUST be a JSON object")
        return
    _timestamp(document, "run_at", report)

    checks = document.get("checks")
    if not isinstance(checks, list):
        report.error("selftest.checks_missing", "checks MUST be a list")
        return

    expected_names = {"silence_fixture", "qr_roundtrip", "sampler_agreement", "harness_floor"}
    names = [check.get("name") for check in checks if isinstance(check, dict)]
    if len(checks) != 4:
        report.error(
            "selftest.check_count", f"exactly four checks are required, found {len(checks)}"
        )
    if set(names) != expected_names:
        report.error(
            "selftest.check_names",
            f"checks MUST be exactly {sorted(expected_names)}, found {sorted(names)}",
        )
    if len(names) != len(set(names)):
        report.error("selftest.duplicate_checks", "check names MUST be unique")

    for check in checks:
        if not isinstance(check, dict):
            report.error("selftest.invalid_check", "every check MUST be an object")
            continue
        name = check.get("name", "<unnamed>")
        if not isinstance(check.get("passed"), bool):
            report.error("selftest.passed_missing", f"check {name} has no boolean passed field")
        if "expected" not in check or "observed" not in check:
            report.error(
                "selftest.evidence_missing",
                f"check {name} MUST record both expected and observed values",
            )

    all_passed = document.get("all_passed")
    computed = all(isinstance(c, dict) and c.get("passed") is True for c in checks)
    if all_passed is not computed:
        report.error(
            "selftest.all_passed_inconsistent",
            f"all_passed is {all_passed!r} but the checks compute to {computed}",
        )
    if all_passed is not True:
        report.error(
            "selftest.failed",
            "all_passed is not true: no measurement result is valid until the toolchain "
            "self-test passes",
        )


# ---------------------------------------------------------------------------
# Injection matrix
# ---------------------------------------------------------------------------


def _rule_injection_matrix(document: Any, report: _Report) -> None:
    _, injection, registry = _schemas()
    _schema_findings(document, injection, registry, report, "injection_matrix: ")
    if not isinstance(document, dict):
        return

    applications = document.get("applications")
    backends = document.get("backends_evaluated")
    cells = document.get("cells")
    if not isinstance(applications, list) or not isinstance(backends, list):
        return

    app_ids = {
        application.get("application_id")
        for application in applications
        if isinstance(application, dict)
    }
    bitnesses = {
        application.get("bitness") for application in applications if isinstance(application, dict)
    }
    if bitnesses != {"32_bit", "64_bit"}:
        report.error(
            "injection.bitness_coverage",
            "both a 32-bit and a 64-bit consumer MUST be present in the matrix",
        )

    if not isinstance(cells, list):
        return

    pairs: set[tuple[Any, Any]] = set()
    for cell in cells:
        if not isinstance(cell, dict):
            report.error("injection.invalid_cell", "every matrix cell MUST be an object")
            continue
        pair = (cell.get("backend"), cell.get("application_id"))
        if pair in pairs:
            report.error(
                "injection.duplicate_cell",
                f"backend {pair[0]} / application {pair[1]} appears more than once",
            )
        pairs.add(pair)
        if cell.get("outcome") == "not_tested" and not cell.get("not_tested_reason"):
            report.error(
                "injection.not_tested_reason",
                f"not_tested cell {pair[0]}/{pair[1]} MUST carry not_tested_reason",
            )

    for backend in backends:
        if not isinstance(backend, dict):
            continue
        backend_id = backend.get("backend")
        for app_id in app_ids:
            if (backend_id, app_id) not in pairs:
                report.error(
                    "injection.missing_cell",
                    f"evaluated backend {backend_id} has no cell for application {app_id}",
                )

    selected = document.get("selected_backend")
    selected_cells = [
        cell for cell in cells if isinstance(cell, dict) and cell.get("backend") == selected
    ]
    if len(selected_cells) != len(app_ids):
        report.error(
            "injection.selected_backend_incomplete",
            f"the selected backend {selected} MUST have one cell per application",
        )
    for cell in selected_cells:
        app_id = cell.get("application_id")
        if cell.get("outcome") != "frames_delivered":
            report.error(
                "injection.selected_backend_failed",
                f"selected backend {selected} did not deliver frames to {app_id}",
            )
            continue
        duration = cell.get("observed_duration_s")
        if not _is_number(duration) or duration < 1800:
            report.error(
                "injection.duration_too_short",
                f"{app_id}: observed_duration_s MUST be at least 1800",
            )
        stall = cell.get("longest_stall_ms")
        if not _is_number(stall) or stall > 2000:
            report.error(
                "injection.stall_too_long",
                f"{app_id}: longest_stall_ms MUST not exceed 2000",
            )
        if cell.get("delivered_size_ok") is not True:
            report.error("injection.size_mismatch", f"{app_id}: delivered_size_ok is not true")
        if cell.get("delivered_rate_ok") is not True:
            report.error("injection.rate_mismatch", f"{app_id}: delivered_rate_ok is not true")


# ---------------------------------------------------------------------------
# Gate report
# ---------------------------------------------------------------------------


def _rule_gate_report(document: Any, report: _Report, repo_root: Path) -> None:
    gate_schema, _, registry = _schemas()
    _schema_findings(document, gate_schema, registry, report, "gate_report: ")
    if not isinstance(document, dict):
        return

    review_at = _timestamp(document, "phase_review_at", report)
    gates = document.get("gates")
    if not isinstance(gates, list):
        return

    gate_ids = [gate.get("gate_id") for gate in gates if isinstance(gate, dict)]
    if len(gate_ids) != len(set(gate_ids)):
        report.error("gate_report.duplicate_gate", "each gate MUST appear exactly once")

    environment = document.get("reference_environment")
    reference_ok = isinstance(environment, dict) and (
        environment.get("hardware_decode_verification", {}).get("hardware_accelerated") is True
        and environment.get("network", {}).get("within_reference_conditions") is True
    )

    for gate in gates:
        if not isinstance(gate, dict):
            continue
        gate_id = gate.get("gate_id", "<unnamed>")
        verdict = gate.get("verdict")
        issued_at = gate.get("issued_at")
        if review_at is not None and _iso_with_offset(issued_at):
            issued = datetime.fromisoformat(issued_at.replace("Z", "+00:00"))
            if abs((issued - review_at).total_seconds()) > 60:
                report.error(
                    "gate_report.review_time_mismatch",
                    f"{gate_id}: issued_at is more than one minute from phase_review_at",
                )

        if verdict in {"no_go", "conditional"} and not gate.get("failure_code"):
            report.error(
                "gate_report.failure_code_missing",
                f"{gate_id}: a {verdict} verdict MUST carry a failure_code",
            )
        if verdict == "go":
            if not gate.get("evidence_refs"):
                report.error(
                    "gate_report.evidence_missing",
                    f"{gate_id}: a go verdict MUST cite at least one evidence file",
                )
            for reference in gate.get("evidence_refs", []):
                if isinstance(reference, str) and not (repo_root / reference).exists():
                    report.error(
                        "gate_report.evidence_not_found",
                        f"{gate_id}: evidence reference does not exist: {reference}",
                    )
            if reference_ok is False:
                report.warning(
                    "gate_report.outside_reference_environment",
                    f"{gate_id}: a go verdict is supported by an environment that is not the "
                    "verified reference environment",
                )
        if verdict == "conditional" and not gate.get("conditions"):
            report.error(
                "gate_report.condition_missing",
                f"{gate_id}: a conditional verdict MUST have at least one condition",
            )

        if gate_id == "desktop_receive_pipeline":
            _gate_latency(gate, report)
        elif gate_id == "pairing_handshake":
            _gate_pairing(gate, report)
        elif gate_id == "virtual_audio_and_licensing":
            _gate_audio(gate, report)


def _gate_latency(gate: dict[str, Any], report: _Report) -> None:
    raw = gate.get("raw_latency_p95_ms")
    floor = gate.get("harness_floor_ms")
    corrected = gate.get("floor_corrected_latency_p95_ms")
    if all(_is_number(value) for value in (raw, floor, corrected)):
        if abs((raw - floor) - corrected) > 0.15:
            report.error(
                "gate_report.latency_correction",
                "floor_corrected_latency_p95_ms MUST equal raw_latency_p95_ms minus "
                "harness_floor_ms",
            )
        if gate.get("verdict") == "go" and raw > LATENCY_P95_MS:
            report.error(
                "gate_report.latency_threshold",
                f"a go verdict requires raw_latency_p95_ms <= {LATENCY_P95_MS}",
            )
    if gate.get("verdict") == "go":
        included = gate.get("included_latency_stages")
        excluded = gate.get("excluded_latency_stages")
        if not included or not excluded:
            report.error(
                "gate_report.latency_stages",
                "the latency gate MUST state which stages were included and which were excluded",
            )
        if gate.get("latency_attribution") is None and (raw or 0) > LATENCY_P95_MS:
            report.error(
                "gate_report.latency_attribution",
                "a non-passing latency outcome MUST state its attribution",
            )


def _gate_pairing(gate: dict[str, Any], report: _Report) -> None:
    throttling = gate.get("throttling_summary")
    if isinstance(throttling, dict):
        if throttling.get("attempt_limit_per_minute") != 5:
            report.error(
                "gate_report.pairing_attempt_limit",
                "throttling_summary.attempt_limit_per_minute MUST be 5",
            )
        if throttling.get("limit_exceeded_at_attempt") != 6:
            report.error(
                "gate_report.pairing_limit_attempt",
                "throttling_summary.limit_exceeded_at_attempt MUST be 6",
            )
        if throttling.get("consecutive_failures_at_listener_close") != 10:
            report.error(
                "gate_report.pairing_listener_failures",
                "throttling_summary.consecutive_failures_at_listener_close MUST be 10",
            )
        if gate.get("verdict") == "go" and (
            throttling.get("listener_closed") is not True
            or throttling.get("legitimate_pairing_unaffected") is not True
        ):
            report.error(
                "gate_report.pairing_throttle_evidence",
                "a go verdict requires listener_closed and legitimate_pairing_unaffected",
            )

    residue = gate.get("residue_summary")
    if isinstance(residue, dict):
        for key in (
            "after_session_end_artefacts_found",
            "after_application_restart_artefacts_found",
        ):
            if residue.get(key):
                report.error(
                    "gate_report.pairing_residue",
                    f"{key} MUST be empty; any residue is a hard failure",
                )
        if not residue.get("coverage"):
            report.error(
                "gate_report.pairing_coverage",
                "residue_summary.coverage MUST state what was searched",
            )


def _gate_audio(gate: dict[str, Any], report: _Report) -> None:
    # The schema forces gate 5 to publish an audio_measurement_summary even when no
    # live run exists (the summary records honest not-measured values in that case).
    # The pass-level invariants below are therefore only binding for a go verdict:
    # a conditional/no_go gate must record the measured truth (zero counts when
    # nothing was recorded, detectors_agree=false meaning agreement was not
    # evaluated) and explain it in the rationale.
    if gate.get("verdict") != "go":
        return
    audio = gate.get("audio_measurement_summary")
    if not isinstance(audio, dict):
        report.error(
            "gate_report.audio_summary_missing",
            "a go verdict for the audio gate requires an audio_measurement_summary",
        )
        return
    if audio.get("dropout_count") != 0:
        report.error(
            "gate_report.audio_dropouts",
            "a go verdict requires zero dropouts",
        )
    if audio.get("detectors_agree") is not True:
        report.error(
            "gate_report.audio_detectors_disagree",
            "detectors_agree MUST be true under a go verdict; a disagreement is a "
            "finding, never resolved silently",
        )
    if audio.get("consumer_renegotiations") != 0:
        report.error(
            "gate_report.audio_renegotiations",
            "a go verdict requires zero consumer renegotiations",
        )
    if (audio.get("phrases_exact") or 0) < 18:
        report.error(
            "gate_report.audio_phrases",
            "a go verdict requires at least 18 exact phrases",
        )
    if (audio.get("corpus_wer") or 0) > 0.05:
        report.error(
            "gate_report.audio_wer",
            "a go verdict requires corpus_wer <= 0.05",
        )


# ---------------------------------------------------------------------------
# Pipeline artefacts
# ---------------------------------------------------------------------------


def _rule_latency_summary(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("latency.invalid", "the latency summary MUST be a JSON object")
        return
    criterion = document.get("criterion")
    if not isinstance(criterion, dict):
        report.error("latency.criterion_missing", "criterion is required")
    else:
        if criterion.get("basis") != "raw":
            report.error("latency.basis", "criterion.basis MUST be 'raw'")
        if criterion.get("metric") != "raw.p95_ms":
            report.error("latency.metric", "criterion.metric MUST be 'raw.p95_ms'")
        if criterion.get("threshold") != LATENCY_P95_MS:
            report.error(
                "latency.threshold",
                f"criterion.threshold MUST be {LATENCY_P95_MS}",
            )

    raw = document.get("raw")
    corrected = document.get("corrected")
    floor = document.get("harness_floor_ms")
    if not all(isinstance(section, dict) for section in (raw, corrected)) or not _is_number(floor):
        report.error(
            "latency.figures_missing",
            "harness_floor_ms, raw and corrected MUST all be present",
        )
        return

    raw_p95 = raw.get("p95_ms")
    corrected_p95 = corrected.get("p95_ms")
    if not _is_number(raw_p95) or not _is_number(corrected_p95):
        report.error("latency.p95_missing", "raw.p95_ms and corrected.p95_ms are required")
        return
    if abs((raw_p95 - floor) - corrected_p95) > 0.15:
        report.error(
            "latency.correction",
            "corrected.p95_ms MUST equal raw.p95_ms minus harness_floor_ms within rounding",
        )

    verdict = document.get("verdict")
    if verdict not in {"pass", "conditional", "fail"}:
        report.error("latency.verdict", "verdict MUST be pass, conditional or fail")
    if verdict == "pass" and raw_p95 > LATENCY_P95_MS:
        report.error(
            "latency.threshold",
            f"a pass verdict requires raw.p95_ms <= {LATENCY_P95_MS}",
        )
    if verdict != "pass" and not str(document.get("attribution", "")).strip():
        report.error(
            "latency.attribution",
            "a non-passing latency result MUST state its attribution",
        )
    sample_count = document.get("sample_count")
    if not _is_int(sample_count) or sample_count < 1:
        report.error("latency.samples", "sample_count MUST be a positive integer")


def _rule_stage_budget(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("stage.invalid", "the stage budget MUST be a JSON object")
        return
    stages = document.get("stages")
    if not isinstance(stages, list) or not stages:
        report.error("stage.missing", "stages MUST be a non-empty list")
        return
    for stage in stages:
        if not isinstance(stage, dict):
            report.error("stage.invalid_entry", "every stage MUST be an object")
            continue
        name = stage.get("stage")
        if name not in ALLOWED_STAGES:
            report.error("stage.name", f"unknown stage {name!r}")
        if not isinstance(stage.get("included_in_gate"), bool):
            report.error("stage.included", f"{name}: included_in_gate MUST be a boolean")
        if not str(stage.get("measurement_method", "")).strip():
            report.error("stage.method", f"{name}: measurement_method is required")
        measured = stage.get("measured_ms")
        if stage.get("included_in_gate") is True:
            if not _is_number(measured):
                report.error(
                    "stage.measured_missing",
                    f"{name}: an included stage MUST record measured_ms",
                )
        elif measured is not None and not _is_number(measured):
            report.error("stage.measured_invalid", f"{name}: measured_ms MUST be numeric or null")


def _rule_resource_summary(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("resource.invalid", "the resource summary MUST be a JSON object")
        return
    criterion = document.get("criterion")
    if not isinstance(criterion, dict):
        report.error("resource.criterion_missing", "criterion is required")
    else:
        cpu = criterion.get("cpu", {})
        memory = criterion.get("memory", {})
        if not isinstance(cpu, dict) or cpu.get("metric") != "cpu_pct_machine.p95":
            report.error("resource.cpu_metric", "criterion.cpu.metric MUST be cpu_pct_machine.p95")
        elif cpu.get("threshold") != CPU_P95_PCT:
            report.error("resource.cpu_threshold", f"criterion.cpu.threshold MUST be {CPU_P95_PCT}")
        if not isinstance(memory, dict) or memory.get("metric") != "private_working_set_mb.max":
            report.error(
                "resource.memory_metric",
                "criterion.memory.metric MUST be private_working_set_mb.max",
            )
        elif memory.get("threshold") != MEMORY_MAX_MB:
            report.error(
                "resource.memory_threshold",
                f"criterion.memory.threshold MUST be {MEMORY_MAX_MB}",
            )

    cpu_stats = document.get("cpu_pct_machine")
    memory_stats = document.get("private_working_set_mb")
    if not isinstance(cpu_stats, dict) or not isinstance(memory_stats, dict):
        report.error(
            "resource.statistics_missing",
            "cpu_pct_machine and private_working_set_mb statistics are required",
        )
        return
    cpu_p95 = cpu_stats.get("p95")
    memory_max = memory_stats.get("max")
    if not _is_number(cpu_p95) or not _is_number(memory_max):
        report.error("resource.gated_statistics", "cpu p95 and memory max are required")
        return

    verdict = document.get("verdict")
    if verdict == "pass":
        if cpu_p95 > CPU_P95_PCT:
            report.error("resource.cpu_threshold", f"a pass requires cpu p95 <= {CPU_P95_PCT}")
        if memory_max > MEMORY_MAX_MB:
            report.error(
                "resource.memory_threshold",
                f"a pass requires private working set max <= {MEMORY_MAX_MB} MB",
            )
        cross_check = document.get("cross_check")
        if (
            not isinstance(cross_check, dict)
            or cross_check.get("agreement_within_one_point") is not True
        ):
            report.error(
                "resource.cross_check",
                "a pass requires the typeperf cross-check to agree within one percentage point",
            )
    if not _is_int(document.get("sample_count")) or document.get("sample_count") < 1:
        report.error("resource.samples", "sample_count MUST be a positive integer")


def _rule_drift_summary(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("drift.invalid", "the drift summary MUST be a JSON object")
        return
    regression = document.get("regression")
    if not isinstance(regression, dict):
        report.error("drift.regression_missing", "regression is required")
        return
    drift = regression.get("drift_ms_per_hour")
    interval = regression.get("slope_ci95_ms_per_hour")
    if not _is_number(drift) or not (
        isinstance(interval, list)
        and len(interval) == 2
        and all(_is_number(value) for value in interval)
    ):
        report.error(
            "drift.statistics_missing",
            "drift_ms_per_hour and slope_ci95_ms_per_hour are required",
        )
        return
    try:
        expected = drift_verdict(float(drift), (float(interval[0]), float(interval[1])))
    except ValueError as error:
        report.error("drift.statistics_invalid", str(error))
        return
    verdict = document.get("verdict")
    if verdict != expected:
        report.error(
            "drift.verdict",
            f"verdict is {verdict!r} but the criterion computes {expected!r} "
            f"(threshold {DRIFT_MS_PER_HOUR} ms/h, interval {interval})",
        )
    components = document.get("components")
    if not isinstance(components, dict) or not components:
        report.error("drift.components", "components MUST break the drift down by cause")


def _rule_dropouts(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("dropouts.invalid", "the dropout analysis MUST be a JSON object")
        return
    criterion = document.get("criterion")
    if not isinstance(criterion, dict):
        report.error("dropouts.criterion_missing", "criterion is required")
    else:
        minimum = criterion.get("min_dropout_ms")
        if not _is_number(minimum) or minimum < MIN_DROPOUT_MS:
            report.error(
                "dropouts.min_duration",
                f"min_dropout_ms MUST be at least {MIN_DROPOUT_MS}",
            )
        if not str(criterion.get("threshold_source", "")).strip():
            report.error(
                "dropouts.threshold_source",
                "threshold_dbfs MUST carry a calibrated threshold_source",
            )

    detectors = document.get("detectors")
    if not isinstance(detectors, dict):
        report.error("dropouts.detectors_missing", "detectors are required")
        return
    for name, detector in detectors.items():
        if not isinstance(detector, dict):
            report.error("dropouts.detector_invalid", f"{name}: detector MUST be an object")
            continue
        intervals = detector.get("intervals")
        count = detector.get("dropout_count")
        if not _is_int(count) or count < 0:
            report.error("dropouts.count", f"{name}: dropout_count MUST be a non-negative integer")
            continue
        if not isinstance(intervals, list):
            report.error("dropouts.intervals", f"{name}: intervals MUST be a list")
            continue
        if len(intervals) != count:
            report.error(
                "dropouts.count_mismatch",
                f"{name}: dropout_count {count} does not match {len(intervals)} intervals",
            )
        for interval in intervals:
            if not isinstance(interval, dict):
                report.error("dropouts.interval_invalid", f"{name}: interval MUST be an object")
                continue
            duration = interval.get("duration_ms")
            if _is_number(duration) and duration < MIN_DROPOUT_MS:
                report.error(
                    "dropouts.interval_too_short",
                    f"{name}: an interval of {duration} ms is shorter than {MIN_DROPOUT_MS} ms and "
                    "MUST NOT be counted as a dropout",
                )

    if document.get("verdict") == "pass" and document.get("detectors_agree") is not True:
        report.error(
            "dropouts.detectors_disagree",
            "a pass verdict requires detectors_agree = true",
        )


def _rule_intelligibility(document: Any, report: _Report, repo_root: Path) -> None:
    if not isinstance(document, dict):
        report.error("intelligibility.invalid", "the intelligibility check MUST be a JSON object")
        return
    if not str(document.get("observer", "")).strip():
        report.error("intelligibility.observer", "observer MUST name the person who listened")
    manifest = str(document.get("manifest", "")).strip()
    if not manifest:
        report.error("intelligibility.manifest", "manifest MUST reference the phrase script")
    else:
        path = Path(manifest)
        if not path.is_absolute():
            path = repo_root / path
        if not path.exists():
            report.warning(
                "intelligibility.manifest_not_found",
                f"the referenced phrase script does not exist: {manifest}",
            )
    if document.get("phrase_count") != 20:
        report.error(
            "intelligibility.phrase_count",
            "the committed phrase manifest carries exactly 20 phrases",
        )
    if not _is_int(document.get("non_anchor_count")) or document["non_anchor_count"] < 18:
        report.error(
            "intelligibility.non_anchor_count",
            "the manifest MUST carry at least 18 non-anchor phrases",
        )

    phrases_exact = document.get("phrases_exact")
    nonces_matched = document.get("nonces_matched")
    nonces_total = document.get("nonces_total")
    corpus_wer = document.get("corpus_wer")
    if not _is_int(phrases_exact) or not _is_int(nonces_matched) or not _is_int(nonces_total):
        report.error(
            "intelligibility.counts",
            "phrases_exact, nonces_matched and nonces_total MUST be integers",
        )
        return
    if nonces_matched > nonces_total or nonces_total < 0:
        report.error("intelligibility.nonce_counts", "nonces_matched MUST not exceed nonces_total")
    if document.get("anchors_match") is not True:
        report.error("intelligibility.anchors", "the opening and closing anchors MUST agree")
    if nonces_matched != nonces_total:
        report.error(
            "intelligibility.nonces",
            "every nonce-bearing phrase MUST match; a mismatch invalidates the capture",
        )
    if document.get("verdict") == "pass":
        if phrases_exact < 18:
            report.error("intelligibility.phrases", "a pass requires at least 18 exact phrases")
        if not _is_number(corpus_wer) or corpus_wer > 0.05:
            report.error("intelligibility.wer", "a pass requires corpus_wer <= 0.05")


def _rule_audio_run_capture(document: Any, report: _Report, repo_root: Path) -> None:
    if not isinstance(document, dict):
        report.error("audio_run.invalid", "audio_run_capture.json MUST be a JSON object")
        return

    for field in (
        "capture_id",
        "run_id",
        "capture_file",
        "device_name",
        "consumer_app",
        "dropout_analysis_ref",
        "intelligibility_ref",
        "intelligibility_observer",
        "phrase_script_ref",
    ):
        if not str(document.get(field, "")).strip():
            report.error("audio_run.field", f"{field} MUST be a non-empty string")

    if document.get("capture_format") != "wav":
        report.error("audio_run.format", "capture_format MUST be 'wav'")
    if document.get("sample_rate_hz") != 48_000:
        report.error("audio_run.sample_rate", "sample_rate_hz MUST be 48000")
    if document.get("channels") != 1:
        report.error("audio_run.channels", "channels MUST be 1 (mono)")
    if document.get("device_pin") != "VB-CABLE":
        report.error("audio_run.device_pin", "device_pin MUST be 'VB-CABLE'")
    if document.get("device_version") != "driver pack 45":
        report.error("audio_run.device_version", "device_version MUST be 'driver pack 45'")
    if document.get("signature_verified") is not True:
        report.error(
            "audio_run.signature",
            "signature_verified MUST be true on an admissible run",
        )

    duration = document.get("duration_s")
    if not _is_number(duration) or duration < 600.0:
        report.error("audio_run.duration", "duration_s MUST be at least 600 s")

    renegotiations = document.get("consumer_renegotiations")
    if not _is_int(renegotiations) or renegotiations < 0:
        report.error(
            "audio_run.renegotiations",
            "consumer_renegotiations MUST be a non-negative integer",
        )
    elif renegotiations > 0:
        report.error(
            "audio_run.renegotiations_nonzero",
            "the gate requires zero consumer renegotiations",
        )

    for field in ("dropout_analysis_ref", "intelligibility_ref", "phrase_script_ref"):
        reference = str(document.get(field, "")).strip()
        if not reference:
            continue
        path = Path(reference)
        if not path.is_absolute():
            path = repo_root / path
        if not path.exists():
            report.warning(
                "audio_run.reference_not_found",
                f"{field} does not exist yet: {reference}",
            )


# ---------------------------------------------------------------------------
# Pairing artefacts
# ---------------------------------------------------------------------------


def _rule_pairing_scenarios(document: Any, report: _Report, repo_root: Path) -> None:
    if not isinstance(document, dict):
        report.error("pairing.invalid", "pairing_scenarios.json MUST be a JSON object")
        return
    scenarios = document.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        report.error("pairing.scenarios_missing", "scenarios MUST be a non-empty list")
        return

    codes = _reason_codes(str(repo_root))
    summary = document.get("summary")
    total = accepted = rejected = inconclusive = 0
    with_media = 0
    with_residue = 0

    for scenario in scenarios:
        if not isinstance(scenario, dict):
            report.error("pairing.scenario_invalid", "every scenario MUST be an object")
            continue
        total += 1
        scenario_id = scenario.get("scenario_id", "<unnamed>")
        category = scenario.get("category")
        if category not in ALLOWED_PAIRING_CATEGORIES:
            report.error("pairing.category", f"{scenario_id}: unknown category {category!r}")
        outcome = scenario.get("outcome")
        if outcome not in {"accepted", "rejected", "inconclusive"}:
            report.error("pairing.outcome", f"{scenario_id}: unknown outcome {outcome!r}")
        if outcome == "accepted":
            accepted += 1
            if scenario.get("reason_code") is not None:
                report.error(
                    "pairing.accepted_with_reason",
                    f"{scenario_id}: an accepted scenario MUST have reason_code null",
                )
        elif outcome == "rejected":
            rejected += 1
            if scenario.get("reason_code") not in codes:
                report.error(
                    "pairing.reason_code",
                    f"{scenario_id}: reason_code MUST be a member of the closed set",
                )
        else:
            inconclusive += 1

        if scenario.get("media_flowed_before_authentication") is not False:
            with_media += 1
            report.error(
                "pairing.media_before_authentication",
                f"{scenario_id}: media MUST NOT flow before authentication",
            )
        if scenario.get("residual_credentials"):
            with_residue += 1
            report.error(
                "pairing.residual_credentials",
                f"{scenario_id}: residual_credentials MUST be empty",
            )
        if not str(scenario.get("preconditions", "")).strip():
            report.error("pairing.preconditions", f"{scenario_id}: preconditions are required")
        if not str(scenario.get("expected_result", "")).strip():
            report.error("pairing.expected", f"{scenario_id}: expected_result is required")
        if not str(scenario.get("observed_result", "")).strip():
            report.error("pairing.observed", f"{scenario_id}: observed_result is required")

    if isinstance(summary, dict):
        expected = {
            "total": total,
            "accepted": accepted,
            "rejected": rejected,
            "inconclusive": inconclusive,
            "scenarios_with_media_before_authentication": with_media,
            "scenarios_with_residual_credentials": with_residue,
        }
        for key, value in expected.items():
            if key in summary and summary[key] != value:
                report.error(
                    "pairing.summary_mismatch",
                    f"summary.{key} is {summary[key]!r} but the scenarios count {value}",
                )


def _rule_pairing_throttling(document: Any, report: _Report, repo_root: Path) -> None:
    if not isinstance(document, dict):
        report.error("throttle.invalid", "pairing_throttling.json MUST be a JSON object")
        return
    scenarios = document.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        report.error("throttle.scenarios_missing", "scenarios MUST be a non-empty list")
        return
    codes = _reason_codes(str(repo_root))

    for scenario in scenarios:
        if not isinstance(scenario, dict):
            report.error("throttle.scenario_invalid", "every scenario MUST be an object")
            continue
        throttle_id = scenario.get("throttle_id", "<unnamed>")
        if scenario.get("attempt_limit_per_minute") != 5:
            report.error("throttle.attempt_limit", f"{throttle_id}: attempt limit MUST be 5")
        if scenario.get("limit_exceeded_at_attempt") != 6:
            report.error(
                "throttle.limit_attempt", f"{throttle_id}: the 6th attempt MUST be refused"
            )
        if scenario.get("session_established") is not False:
            report.error(
                "throttle.session_established",
                f"{throttle_id}: a throttled origin MUST NOT establish a session",
            )
        if scenario.get("legitimate_pairing_unaffected") is not True:
            report.error(
                "throttle.legitimate_origin",
                f"{throttle_id}: a second legitimate origin MUST be unaffected",
            )
        decisions = scenario.get("decisions")
        if not isinstance(decisions, list) or not decisions:
            report.error("throttle.decisions", f"{throttle_id}: decisions MUST be recorded")
        else:
            for decision in decisions:
                if not isinstance(decision, dict):
                    report.error("throttle.decision_invalid", f"{throttle_id}: invalid decision")
                    continue
                if (
                    decision.get("reason_code") is not None
                    and decision.get("reason_code") not in codes
                ):
                    report.error(
                        "throttle.reason_code",
                        f"{throttle_id}: every decision reason code MUST be in the closed set",
                    )
                if not str(decision.get("outcome", "")).strip():
                    report.error(
                        "throttle.outcome",
                        f"{throttle_id}: every decision MUST record an observed outcome",
                    )
        if scenario.get("listener_closed") is True:
            if scenario.get("consecutive_failures_at_listener_close") != 10:
                report.error(
                    "throttle.listener_failures",
                    f"{throttle_id}: listener closure MUST follow 10 consecutive failures",
                )
            if scenario.get("security_event") is not True:
                report.error(
                    "throttle.security_event",
                    f"{throttle_id}: listener closure MUST be recorded as a security event",
                )


def _rule_pairing_residue(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("residue.invalid", "pairing_residue.json MUST be a JSON object")
        return
    method = document.get("method")
    if not str(method or "").strip():
        report.error("residue.method", "both inspections MUST name the same method")
    coverage = document.get("coverage")
    if not isinstance(coverage, list) or not coverage:
        report.error("residue.coverage", "coverage MUST state what was searched")
    devices = document.get("devices_inspected")
    if not isinstance(devices, list) or len(devices) < 2:
        report.error(
            "residue.devices",
            "both the desktop and the mobile MUST be inspected",
        )

    inspections = document.get("inspections")
    if not isinstance(inspections, list):
        report.error("residue.inspections_missing", "inspections MUST be a list")
        return
    cases = {inspection.get("case") for inspection in inspections if isinstance(inspection, dict)}
    if cases != RESIDUE_CASES:
        report.error(
            "residue.cases",
            f"both cases are required: {sorted(RESIDUE_CASES)}; found {sorted(cases)}",
        )
    for inspection in inspections:
        if not isinstance(inspection, dict):
            report.error("residue.inspection_invalid", "every inspection MUST be an object")
            continue
        inspection_id = inspection.get("inspection_id", "<unnamed>")
        if inspection.get("artefacts_found"):
            report.error(
                "residue.found",
                f"{inspection_id}: artefacts_found MUST be empty; residue is a hard failure",
            )
        kinds = set(inspection.get("artefact_kinds_checked") or [])
        if kinds != ARTEFACT_KINDS:
            report.error(
                "residue.kinds",
                f"{inspection_id}: all four artefact kinds MUST be checked",
            )
        devices_here = inspection.get("devices_inspected")
        if not isinstance(devices_here, list) or len(devices_here) < 2:
            report.error(
                "residue.device_coverage",
                f"{inspection_id}: both devices MUST be inspected",
            )
        if not str(inspection.get("evidence_ref", "")).strip():
            report.error("residue.evidence", f"{inspection_id}: evidence_ref is required")


def _rule_impairment_profile(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("impairment.invalid", "impairment_profile.json MUST be a JSON object")
        return
    expected = {
        "profile_name": "nominal-3pct-30ms-10ms",
        "loss_pct": 3,
        "loss_distribution": "uniform",
        "added_latency_ms": 30,
        "jitter_ms": 10,
        "applies_to": "media_only",
    }
    for key, value in expected.items():
        if document.get(key) != value:
            report.error(
                "impairment.pinned_value",
                f"{key} MUST be {value!r}; there is exactly one pinned profile",
            )
    if not _is_int(document.get("seed")):
        report.error("impairment.seed", "seed MUST be an integer so the run is reproducible")
    if not str(document.get("relay_log_ref", "")).strip():
        report.error("impairment.log", "relay_log_ref is required")


def _rule_licensing_bundle(document: Any, report: _Report) -> None:
    if not isinstance(document, dict):
        report.error("licensing.invalid", "licensing_bundle.json MUST be a JSON object")
        return
    for role_key in ("decision_owner", "approver"):
        role = document.get(role_key)
        if not isinstance(role, dict) or not str(role.get("person_name", "")).strip():
            report.error(
                "licensing.named_role",
                f"{role_key} MUST name an individual, not a role title",
            )

    options = document.get("candidate_options")
    option_ids: set[str] = set()
    if not isinstance(options, list) or not options:
        report.error(
            "licensing.candidates", "candidate_options MUST include every device considered"
        )
    else:
        for option in options:
            if not isinstance(option, dict):
                report.error("licensing.option_invalid", "every candidate option MUST be an object")
                continue
            option_id = str(option.get("option_id", "<unnamed>"))
            option_ids.add(option_id)
            if not str(option.get("terms_captured_text", "")).strip():
                report.error(
                    "licensing.terms",
                    f"{option_id}: terms_captured_text is required",
                )
            if not str(option.get("terms_url", "")).startswith("https://"):
                report.error(
                    "licensing.terms_url",
                    f"{option_id}: terms_url MUST be an official https URL",
                )
            if option.get("license_model") not in ALLOWED_LICENSE_MODELS:
                report.error("licensing.model", f"{option_id}: unknown license_model")
            if option.get("redistribution_allowed") not in ALLOWED_REDISTRIBUTION:
                report.error(
                    "licensing.redistribution",
                    f"{option_id}: redistribution_allowed MUST be yes_with_conditions, no or "
                    "unverified",
                )
            if option.get("support_burden") not in ALLOWED_SUPPORT_BURDEN:
                report.error("licensing.support", f"{option_id}: unknown support_burden")
            if not _is_number(option.get("unit_cost")) or option["unit_cost"] < 0:
                report.error("licensing.unit_cost", f"{option_id}: unit_cost MUST be non-negative")
            if not isinstance(option.get("commercial_use_allowed"), bool):
                report.error(
                    "licensing.commercial",
                    f"{option_id}: commercial_use_allowed MUST be a boolean",
                )
            if not isinstance(option.get("signature_verified"), bool):
                report.error(
                    "licensing.signature",
                    f"{option_id}: signature_verified MUST be a boolean",
                )

    measurement = document.get("measurement_device")
    measurement_id = measurement.get("option_id") if isinstance(measurement, dict) else None
    if not measurement_id:
        report.error("licensing.measurement_device", "measurement_device MUST carry an option_id")
    elif option_ids and measurement_id not in option_ids:
        report.error(
            "licensing.measurement_candidate",
            "the measurement device MUST also appear in candidate_options",
        )

    compared = document.get("options_compared")
    if not isinstance(compared, list) or not REQUIRED_LICENSE_OPTIONS.issubset(set(compared)):
        report.error(
            "licensing.coverage",
            "options_compared MUST cover redistribution licensing, a user-installed official "
            "driver and a plan B alternative",
        )

    selected = document.get("selected_option")
    remeasurement = document.get("remeasurement_required")
    if not isinstance(remeasurement, bool):
        report.error("licensing.remeasurement", "remeasurement_required MUST be a boolean")
    elif selected is not None:
        if selected not in option_ids:
            report.error("licensing.selected", f"selected_option {selected!r} is not a candidate")
        elif (selected != measurement_id) is not remeasurement:
            report.error(
                "licensing.remeasurement_rule",
                "remeasurement_required MUST be true exactly when the selected device is not "
                "the measurement device",
            )

    if document.get("bundle_status") == "final":
        if not document.get("selected_option"):
            report.error("licensing.final_selection", "a final bundle MUST select an option")
        if not document.get("decided_at"):
            report.error("licensing.final_date", "a final bundle MUST record decided_at")
    elif document.get("bundle_status") == "draft" and document.get("gate_verdict") == "go":
        report.error(
            "licensing.draft_go",
            "an unsigned draft MUST NOT report a go verdict",
        )


def _rule_open_questions(document: Any, report: _Report) -> None:
    questions = document.get("open_questions") if isinstance(document, dict) else document
    if not isinstance(questions, list):
        report.error("questions.invalid", "open_questions MUST be a list")
        return
    seen: set[str] = set()
    for question in questions:
        if not isinstance(question, dict):
            report.error("questions.entry", "every open question MUST be an object")
            continue
        question_id = question.get("question_id")
        if not isinstance(question_id, str) or not question_id:
            report.error("questions.id", "every open question MUST carry a question_id")
            continue
        if question_id in seen:
            report.error("questions.duplicate", f"duplicate question_id {question_id}")
        seen.add(question_id)
        if question.get("status") not in {"open", "answered", "deferred"}:
            report.error("questions.status", f"{question_id}: invalid status")


# ---------------------------------------------------------------------------
# CSV and JSONL artefacts
# ---------------------------------------------------------------------------


def _read_csv_rows(path: Path, kind: str, report: _Report) -> list[dict[str, str]]:
    expected = CSV_COLUMNS[kind]
    try:
        handle = path.open(encoding="utf-8", newline="")
    except OSError as error:
        report.error("csv.read", f"cannot read artefact: {error}")
        return []
    with handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames != expected:
            report.error(
                "csv.columns",
                f"header MUST be {expected!r}; found {reader.fieldnames!r}",
            )
            return []
        return list(reader)


def _float(row: dict[str, str], key: str) -> float | None:
    try:
        return float(row[key])
    except (KeyError, TypeError, ValueError):
        return None


def _rule_latency_samples(path: Path, report: _Report) -> None:
    rows = _read_csv_rows(path, LATENCY_SAMPLES, report)
    if not rows:
        return
    for index, row in enumerate(rows, start=1):
        if row.get("sample_id") != str(index):
            report.error(
                "latency_samples.sample_id",
                f"sample_id MUST be sequential from 1; row {index} is {row.get('sample_id')!r}",
            )
            break
    for index, row in enumerate(rows, start=1):
        latency = _float(row, "latency_ms")
        capture = _float(row, "capture_host_ms")
        epoch = _float(row, "source_epoch_ms")
        offset = _float(row, "clock_offset_ms")
        if None in (latency, capture, epoch, offset):
            report.error("latency_samples.numeric", f"row {index} has a non-numeric measurement")
            continue
        computed = capture - epoch - offset
        if abs(computed - latency) > 0.01:
            report.error(
                "latency_samples.formula",
                f"row {index}: latency_ms MUST equal capture_host_ms - source_epoch_ms - "
                f"clock_offset_ms ({computed:.3f})",
            )


def _rule_resource_samples(path: Path, report: _Report, cpu_logical_cores: int | None) -> None:
    rows = _read_csv_rows(path, RESOURCE_SAMPLES, report)
    if not rows:
        return
    previous_elapsed: float | None = None
    for index, row in enumerate(rows, start=1):
        elapsed = _float(row, "elapsed_ms")
        machine = _float(row, "cpu_pct_machine")
        single = _float(row, "cpu_pct_single_core")
        if None in (elapsed, machine, single):
            report.error("resource_samples.numeric", f"row {index} has a non-numeric sample")
            continue
        if previous_elapsed is not None and elapsed <= previous_elapsed:
            report.error(
                "resource_samples.monotonic",
                f"row {index}: elapsed_ms MUST be strictly increasing",
            )
        previous_elapsed = elapsed
        if machine < 0 or machine > 100:
            report.error("resource_samples.range", f"row {index}: cpu_pct_machine MUST be 0..100")
        if cpu_logical_cores:
            expected = single / cpu_logical_cores
            if abs(expected - machine) > 0.1:
                report.error(
                    "resource_samples.normalisation",
                    f"row {index}: cpu_pct_machine MUST equal cpu_pct_single_core / "
                    f"{cpu_logical_cores} ({expected:.3f})",
                )


def _rule_drift_samples(path: Path, report: _Report) -> None:
    rows = _read_csv_rows(path, DRIFT_SAMPLES, report)
    if not rows:
        return
    previous_elapsed: float | None = None
    for index, row in enumerate(rows, start=1):
        elapsed = _float(row, "elapsed_s")
        source = _float(row, "source_skew_ms")
        audio = _float(row, "audio_playout_delay_ms")
        video = _float(row, "video_playout_delay_ms")
        skew = _float(row, "playout_skew_ms")
        if None in (elapsed, source, audio, video, skew):
            report.error("drift_samples.numeric", f"row {index} has a non-numeric sample")
            continue
        if previous_elapsed is not None and elapsed <= previous_elapsed:
            report.error(
                "drift_samples.monotonic",
                f"row {index}: elapsed_s MUST be strictly increasing",
            )
        previous_elapsed = elapsed
        expected = source + audio - video
        if abs(expected - skew) > 0.01:
            report.error(
                "drift_samples.formula",
                f"row {index}: playout_skew_ms MUST equal source_skew_ms + "
                f"audio_playout_delay_ms - video_playout_delay_ms ({expected:.3f})",
            )


def _rule_relay_log(path: Path, report: _Report) -> None:
    actions = {"drop", "delay", "stall_hold", "stall_release", "pass"}
    for index, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError as error:
            report.error("relay_log.json", f"line {index}: {error}")
            continue
        if not isinstance(entry, dict):
            report.error("relay_log.entry", f"line {index}: an entry MUST be an object")
            continue
        if entry.get("action") not in actions:
            report.error(
                "relay_log.action", f"line {index}: unknown action {entry.get('action')!r}"
            )
        for key in ("t_ms", "delay_ms"):
            if not _is_number(entry.get(key)):
                report.error("relay_log.numeric", f"line {index}: {key} MUST be numeric")


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def validate_document(
    document: Any,
    kind: str,
    *,
    path: str = "<memory>",
    repo_root: Path = REPO_ROOT,
) -> list[Finding]:
    """Validate one already-parsed artefact and return its findings."""

    report = _Report(path)
    if kind == ENVIRONMENT:
        _rule_environment(document, report)
    elif kind == TOOLCHAIN_SELFTEST:
        _rule_toolchain_selftest(document, report)
    elif kind == INJECTION_MATRIX:
        _rule_injection_matrix(document, report)
    elif kind == GATE_REPORT:
        _rule_gate_report(document, report, repo_root)
    elif kind == LATENCY_SUMMARY:
        _rule_latency_summary(document, report)
    elif kind == STAGE_BUDGET:
        _rule_stage_budget(document, report)
    elif kind == RESOURCE_SUMMARY:
        _rule_resource_summary(document, report)
    elif kind == DRIFT_SUMMARY:
        _rule_drift_summary(document, report)
    elif kind == DROPOUTS:
        _rule_dropouts(document, report)
    elif kind == INTELLIGIBILITY:
        _rule_intelligibility(document, report, repo_root)
    elif kind == AUDIO_RUN_CAPTURE:
        _rule_audio_run_capture(document, report, repo_root)
    elif kind == PAIRING_SCENARIOS:
        _rule_pairing_scenarios(document, report, repo_root)
    elif kind == PAIRING_THROTTLING:
        _rule_pairing_throttling(document, report, repo_root)
    elif kind == PAIRING_RESIDUE:
        _rule_pairing_residue(document, report)
    elif kind == IMPAIRMENT_PROFILE:
        _rule_impairment_profile(document, report)
    elif kind == LICENSING_BUNDLE:
        _rule_licensing_bundle(document, report)
    elif kind == OPEN_QUESTIONS:
        _rule_open_questions(document, report)
    else:
        report.error("kind.unknown", f"no validator is registered for artefact kind {kind!r}")
    return report.findings


@dataclass
class ValidationResult:
    """The findings for one evidence root."""

    files: list[str]
    findings: list[Finding]

    @property
    def errors(self) -> list[Finding]:
        return [finding for finding in self.findings if finding.level == "error"]

    @property
    def warnings(self) -> list[Finding]:
        return [finding for finding in self.findings if finding.level == "warning"]

    @property
    def ok(self) -> bool:
        return not self.errors

    def as_dict(self) -> dict[str, Any]:
        return {
            "files": self.files,
            "errors": len(self.errors),
            "warnings": len(self.warnings),
            "ok": self.ok,
            "findings": [finding.as_dict() for finding in self.findings],
        }


def validate_evidence_root(
    root: Path = REPO_ROOT / "spikes" / "evidence",
    *,
    repo_root: Path = REPO_ROOT,
) -> ValidationResult:
    """Validate every recognised artefact under ``root``."""

    files = discover_evidence(root) if root.exists() else []
    result = ValidationResult(files=[str(path) for path in files], findings=[])

    cores: int | None = None
    documents: dict[Path, Any] = {}

    for path in files:
        kind = kind_for_path(path)
        assert kind is not None
        report = _Report(str(path))

        if kind in CSV_COLUMNS:
            if kind == LATENCY_SAMPLES:
                _rule_latency_samples(path, report)
            elif kind == RESOURCE_SAMPLES:
                if cores is None:
                    environment_path = root / "environment.json"
                    if environment_path.exists():
                        try:
                            environment = json.loads(environment_path.read_text("utf-8"))
                            value = environment.get("cpu_logical_cores")
                            cores = value if isinstance(value, int) else None
                        except json.JSONDecodeError:
                            cores = None
                _rule_resource_samples(path, report, cores)
            else:
                _rule_drift_samples(path, report)
        elif kind == RELAY_LOG:
            _rule_relay_log(path, report)
        else:
            try:
                document = json.loads(path.read_text(encoding="utf-8"))
            except json.JSONDecodeError as error:
                report.error("json.parse", f"invalid JSON: {error}")
                document = None
            if document is not None:
                documents[path] = document
                result.findings.extend(
                    validate_document(document, kind, path=str(path), repo_root=repo_root)
                )
        result.findings.extend(report.findings)

        if kind == ENVIRONMENT and path in documents:
            value = documents[path].get("cpu_logical_cores")
            if isinstance(value, int):
                cores = value

    _cross_check_stage_budget(root, documents, result.findings)
    return result


def _cross_check_stage_budget(
    root: Path, documents: dict[Path, Any], findings: list[Finding]
) -> None:
    """Check the stage sum against the raw p95 of a sibling latency summary."""

    summaries = [
        document
        for path, document in documents.items()
        if kind_for_path(path) == LATENCY_SUMMARY and isinstance(document, dict)
    ]
    for path, document in documents.items():
        if kind_for_path(path) != STAGE_BUDGET or not isinstance(document, dict):
            continue
        raw = None
        for summary in summaries:
            if summary.get("run_id") and summary.get("run_id") == document.get("run_id"):
                raw = (summary.get("raw") or {}).get("p95_ms")
                break
        if raw is None and len(summaries) == 1:
            raw = (summaries[0].get("raw") or {}).get("p95_ms")
        if not _is_number(raw):
            findings.append(
                Finding(
                    "warning",
                    "stage.no_latency_summary",
                    "no latency summary with a raw p95 was found; the stage sum rule could not be "
                    "checked",
                    str(path),
                )
            )
            continue
        total = 0.0
        missing = False
        for stage in document.get("stages", []):
            if not isinstance(stage, dict) or stage.get("included_in_gate") is not True:
                continue
            measured = stage.get("measured_ms")
            if not _is_number(measured):
                missing = True
                continue
            total += float(measured)
        if not missing and total > raw:
            findings.append(
                Finding(
                    "error",
                    "stage.sum_exceeds_raw",
                    f"included stages sum to {total:.1f} ms, above the raw p95 of {raw:.1f} ms",
                    str(path),
                )
            )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate the F0 spike evidence against its contracts."
    )
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="specific artefacts to validate; defaults to every recognised file under the root",
    )
    parser.add_argument(
        "--evidence-root",
        type=Path,
        default=REPO_ROOT / "spikes" / "evidence",
        help="evidence root used when no explicit paths are given",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=REPO_ROOT,
        help="repository root used to resolve schema, reason-code and evidence paths",
    )
    parser.add_argument("--json", action="store_true", help="print a JSON report")
    parser.add_argument("--strict", action="store_true", help="treat warnings as failures")
    args = parser.parse_args(argv)

    if args.paths:
        findings: list[Finding] = []
        files: list[str] = []
        for path in args.paths:
            files.append(str(path))
            kind = kind_for_path(path)
            if kind is None:
                findings.append(
                    Finding("error", "kind.unknown", "unrecognised artefact name", str(path))
                )
                continue
            report = _Report(str(path))
            if kind in CSV_COLUMNS:
                if kind == LATENCY_SAMPLES:
                    _rule_latency_samples(path, report)
                elif kind == RESOURCE_SAMPLES:
                    _rule_resource_samples(path, report, None)
                else:
                    _rule_drift_samples(path, report)
            elif kind == RELAY_LOG:
                _rule_relay_log(path, report)
            else:
                try:
                    document = json.loads(path.read_text(encoding="utf-8"))
                except (OSError, json.JSONDecodeError) as error:
                    report.error("json.parse", f"cannot read artefact: {error}")
                else:
                    findings.extend(
                        validate_document(document, kind, path=str(path), repo_root=args.repo_root)
                    )
            findings.extend(report.findings)
        result = ValidationResult(files=files, findings=findings)
    else:
        result = validate_evidence_root(args.evidence_root, repo_root=args.repo_root)

    if args.json:
        print(json.dumps(result.as_dict(), indent=2, sort_keys=True))
    else:
        for finding in result.findings:
            print(f"{finding.level.upper():7} {finding.path} [{finding.code}] {finding.message}")
        if result.files:
            print(
                f"\nvalidated {len(result.files)} artefact(s): "
                f"{len(result.errors)} error(s), {len(result.warnings)} warning(s)"
            )
        else:
            print(f"no evidence artefacts found under {args.evidence_root}")

    failed = bool(result.errors) or (args.strict and bool(result.warnings))
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
