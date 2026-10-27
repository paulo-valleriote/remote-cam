"""Assemble the F0 gate report from recorded evidence.

Produces ``spikes/evidence/gate_report.json`` conforming to
``specs/001-f0-technical-spikes/contracts/gate-report.schema.json``.

NON-PRODUCTION — F0 risk-reduction spike (FR-039). This script and its output
are disposable spike artefacts and must never be merged into desktop/ or mobile/.
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
EVIDENCE_ROOT = REPO_ROOT / "spikes" / "evidence"
SCHEMA_PATH = (
    REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "gate-report.schema.json"
)


def _load(path: Path, default=None):
    """Load a JSON file, returning ``default`` when it is missing or invalid."""
    try:
        with path.open("r", encoding="utf-8") as fh:
            return json.load(fh)
    except (FileNotFoundError, json.JSONDecodeError):
        return default


def _exists(path: Path) -> bool:
    return path.exists()


def _role(role_type: str, person_name: str) -> dict:
    return {"role_type": role_type, "person_name": person_name}


def _role_from_exit(exit_record: dict, role_type: str) -> dict:
    """Return the signed role from the exit record, falling back to an unnamed role."""
    for sign_off in exit_record.get("sign_offs", []):
        role = sign_off.get("role", {})
        if role.get("role_type") == role_type:
            return role
    return _role(role_type, "not_yet_named")


def _chrome_partial_run() -> dict | None:
    """Describe the Google Chrome injection run without claiming it is a gate cell.

    The run is the only matrix evidence collected. Its duration is derived from
    ``observations.csv``: the timestamps are microseconds even though the column
    is named ``capture_host_ms``, and the raw ``recorded_duration_s`` fields are
    milliseconds. Neither raw value is converted silently.
    """
    directory = EVIDENCE_ROOT / "injection" / "runs" / "mf_virtual_camera" / "chrome"
    run = _load(directory / "run.json")
    observations = directory / "observations.csv"
    if not run or not observations.is_file():
        return None
    with observations.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    stamps = [float(row["capture_host_ms"]) for row in rows]
    span_us = max(stamps) - min(stamps)
    return {
        "path": "spikes/evidence/injection/runs/mf_virtual_camera/chrome",
        "application_id": run.get("application_id", "chrome"),
        "product_name": run.get("product_name", "Google Chrome"),
        "version": run.get("version", "unknown"),
        "observed_duration_s": round(span_us / 1_000_000, 4),
        "required_duration_s": 1800,
        "observation_samples": len(rows),
        "media_duration_s": 60.85,
        "media_duration_source": "local raw capture (capture.webm), not versioned",
        "raw_recorded_duration_s": run.get("recorded_duration_s"),
        "raw_recorded_duration_unit": "milliseconds",
        "observation_timestamp_unit": "microseconds",
        "meets_duration": False,
    }


def _gate(
    gate_id: str,
    title: str,
    wave: int,
    depends_on: list,
    verdict: str,
    accountable_role: dict,
    criterion: str,
    measured_result: dict,
    rationale: str,
    evidence_refs: list,
    measurement_conditions: str,
    *,
    issued_at: str,
    preconditions: list | None = None,
    included_latency_stages: list | None = None,
    excluded_latency_stages: list | None = None,
    harness_floor_ms: float | None = None,
    raw_latency_p95_ms: float | None = None,
    floor_corrected_latency_p95_ms: float | None = None,
    latency_attribution: str | None = None,
    impairment_profile: dict | None = None,
    throttling_summary: dict | None = None,
    residue_summary: dict | None = None,
    audio_measurement_summary: dict | None = None,
    conditions: list | None = None,
    blocking_dependency: str | None = None,
    failure_code: str | None = None,
) -> dict:
    gate = {
        "gate_id": gate_id,
        "title": title,
        "wave": wave,
        "depends_on": depends_on,
        "verdict": verdict,
        "accountable_role": accountable_role,
        "issued_at": issued_at,
        "criterion": criterion,
        "measured_result": measured_result,
        "rationale": rationale,
        "evidence_refs": evidence_refs,
        "measurement_conditions": measurement_conditions,
    }
    if preconditions is not None:
        gate["preconditions"] = preconditions
    # Latency fields are always emitted for the latency gate (null when not measured);
    # omitted for other gates to keep the schema clean.
    if gate_id == "desktop_receive_pipeline":
        gate["included_latency_stages"] = included_latency_stages or []
        gate["excluded_latency_stages"] = excluded_latency_stages or []
        gate["harness_floor_ms"] = harness_floor_ms
        gate["raw_latency_p95_ms"] = raw_latency_p95_ms
        gate["floor_corrected_latency_p95_ms"] = floor_corrected_latency_p95_ms
        if latency_attribution is not None:
            gate["latency_attribution"] = latency_attribution
    if impairment_profile is not None:
        gate["impairment_profile"] = impairment_profile
    if throttling_summary is not None:
        gate["throttling_summary"] = throttling_summary
    if residue_summary is not None:
        gate["residue_summary"] = residue_summary
    if audio_measurement_summary is not None:
        gate["audio_measurement_summary"] = audio_measurement_summary
    if conditions is not None:
        gate["conditions"] = conditions
    if blocking_dependency is not None:
        gate["blocking_dependency"] = blocking_dependency
    if failure_code is not None:
        gate["failure_code"] = failure_code
    return gate


def build_report() -> dict:
    env = _load(EVIDENCE_ROOT / "environment.json") or {}
    exit_record = _load(EVIDENCE_ROOT / "f0_exit.json") or {}
    phase_review_at = exit_record.get("closed_at")
    if not phase_review_at:
        raise ValueError("spikes/evidence/f0_exit.json with closed_at is required to close F0")
    technical_lead = _role_from_exit(exit_record, "technical_lead")
    security_reviewer = _role_from_exit(exit_record, "security_reviewer")
    product_owner = _role_from_exit(exit_record, "product_owner")

    # --- Gate 1: camera_injection (wave 1) ---
    gate1 = _gate(
        gate_id="camera_injection",
        title="Virtual camera injection across the target application matrix",
        wave=1,
        depends_on=["foundational"],
        verdict="no_go",
        accountable_role=technical_lead,
        issued_at=phase_review_at,
        criterion=(
            "100 % frames_delivered on the selected backend, observed_duration_s >= 1800, "
            "longest_stall_ms <= 2000, both bitnesses, multi-consumer observation"
        ),
        measured_result={
            "matrix_file": "spikes/evidence/injection/injection_matrix.json",
            "matrix_exists": False,
            "selected_backend": "not_determined",
            "frames_delivered_cells": 0,
            "total_cells": 6,
            "partial_run": _chrome_partial_run(),
        },
        rationale=(
            "The only matrix evidence collected is one Google Chrome cell: 974 observations "
            "over 33.2845 s at 1280x720@30. The gate requires 1800 s per cell, five "
            "application categories and a 32-bit consumer, so the partial run cannot pass the "
            "criterion and no backend is selected. The remaining cells were not collected at "
            "closure; they are carried to F3. No injection_matrix.json exists, because the "
            "matrix contract requires a selected backend and a frames_delivered cell of at "
            "least 1800 s."
        ),
        evidence_refs=[
            "spikes/evidence/injection/runs/mf_virtual_camera/chrome/run.json",
            "spikes/evidence/injection/runs/mf_virtual_camera/chrome/capture.json",
            "spikes/evidence/injection/runs/mf_virtual_camera/chrome/observations.csv",
        ],
        measurement_conditions=(
            "Clean Windows 11 x64, non-administrator account, five application categories "
            "plus a 32-bit consumer, 30 minutes per cell. The Chrome run was collected on the "
            "Windows 11 host for 33.2845 s; the rest of the matrix was not collected."
        ),
        preconditions=[],
        blocking_dependency=None,
        failure_code="evidence_incomplete",
    )

    # --- Gate 2: pairing_handshake (wave 1) ---
    pairing_scenarios = _load(EVIDENCE_ROOT / "pairing" / "pairing_scenarios.json", {})
    pairing_throttling = _load(EVIDENCE_ROOT / "pairing" / "pairing_throttling.json", {})
    gate2 = _gate(
        gate_id="pairing_handshake",
        title="Authenticated pairing handshake (TLS 1.3, fingerprint-pinned, token-authenticated)",
        wave=1,
        depends_on=["foundational"],
        verdict="conditional",
        accountable_role=security_reviewer,
        issued_at=phase_review_at,
        criterion=(
            "Valid scenario accepted in <= 5 s; all adversarial scenarios rejected with "
            "closed reason codes; no media before authentication; per-origin throttling "
            "(5/minute, 6th refused, progressive blocks, listener close at 10 consecutive "
            "failures); second origin unaffected; zero residue after session end and after "
            "application restart using the same inspection method."
        ),
        measured_result={
            "scenarios_total": pairing_scenarios.get("summary", {}).get("total", 0),
            "scenarios_accepted": pairing_scenarios.get("summary", {}).get("accepted", 0),
            "scenarios_rejected": pairing_scenarios.get("summary", {}).get("rejected", 0),
            "media_before_authentication": pairing_scenarios.get("summary", {}).get(
                "scenarios_with_media_before_authentication", None
            ),
            "residual_credentials": pairing_scenarios.get("summary", {}).get(
                "scenarios_with_residual_credentials", None
            ),
            "throttling_attempt_limit": 5,
            "throttling_limit_exceeded_at": 6,
            "throttling_listener_close_at": 10,
            "throttling_legitimate_unaffected": pairing_throttling
            and all(
                s.get("legitimate_pairing_unaffected", False)
                for s in pairing_throttling.get("scenarios", [])
            ),
            "throttling_listener_closed": any(
                s.get("listener_closed", False) for s in pairing_throttling.get("scenarios", [])
            ),
            "residue_after_session_end": "not_recorded",
            "residue_after_application_restart": "not_recorded",
        },
        rationale=(
            "All 13 pairing scenarios are recorded (1 accepted, 12 rejected) with closed "
            "reason codes and zero media before authentication. Throttling evidence shows "
            "attempt 6 refused, progressive blocks, listener closed at 10 consecutive "
            "failures, and a second legitimate origin unaffected. The residue inspection "
            "after application restart requires two named reference devices (Windows 11 and "
            "Android) that are not available, so after_application_restart_artefacts_found "
            "cannot be produced with the identical method. Gate 2 is conditional on the "
            "residue-restart inspection."
        ),
        evidence_refs=[
            "spikes/evidence/pairing/pairing_scenarios.json",
            "spikes/evidence/pairing/pairing_throttling.json",
        ],
        measurement_conditions=(
            "Harness-only on the F0 host for pairing and throttling; residue restart scan "
            "requires the Windows 11 and Android reference devices."
        ),
        preconditions=["PC-5", "PC-6"],
        throttling_summary={
            "attempt_limit_per_minute": 5,
            "limit_exceeded_at_attempt": 6,
            "consecutive_failures_at_listener_close": 10,
            "listener_closed": any(
                s.get("listener_closed", False) for s in pairing_throttling.get("scenarios", [])
            ),
            "session_established": pairing_scenarios.get("summary", {}).get("accepted", 0) > 0,
            "legitimate_pairing_unaffected": pairing_throttling
            and all(
                s.get("legitimate_pairing_unaffected", False)
                for s in pairing_throttling.get("scenarios", [])
            ),
            "decisions_recorded": len(pairing_throttling.get("scenarios", [])),
            "evidence_ref": "spikes/evidence/pairing/pairing_throttling.json",
        },
        residue_summary={
            "method": "byte scanner plus named platform probes (residue_scan.py --probe)",
            "coverage": [
                "process memory",
                "app-data directory and journals",
                "platform credential/certificate store",
                "HKCU\\Software\\<vendor>\\<app> on both devices",
            ],
            "after_session_end_artefacts_found": [],
            "after_application_restart_artefacts_found": [],
            "evidence_ref": "spikes/evidence/pairing/pairing_residue.json (not produced)",
        },
        conditions=[
            {
                "condition_id": "C-02-residue-restart",
                "description": (
                    "Residue inspection after application restart must be empty using the "
                    "same method as the session-end inspection, on two named reference devices."
                ),
                "conditioned_phases": ["F1"],
                "status": "blocking",
            }
        ],
        failure_code="evidence_incomplete",
    )

    # --- Gate 3: desktop_receive_pipeline (wave 2) ---
    pipeline_run_matrix = _load(EVIDENCE_ROOT / "pipeline" / "run_matrix.json", {})
    gate3 = _gate(
        gate_id="desktop_receive_pipeline",
        title="Desktop receive pipeline: latency, resilience, resources, drift, stall recovery",
        wave=2,
        depends_on=["camera_injection", "pairing_handshake"],
        verdict="no_go",
        accountable_role=technical_lead,
        issued_at=phase_review_at,
        criterion=(
            "Raw capture-at-consumer p95 <= 150 ms across three >= 10-minute runs; "
            "harness floor and floor-corrected p95 published; CPU p95 <= 15 %; "
            "private working set max <= 400 MB; drift <= 20 ms/h with certifying interval; "
            "zero dropouts >= 50 ms under the pinned profile; stall recovery <= 2 s."
        ),
        measured_result={
            "run_matrix_file": "spikes/evidence/pipeline/run_matrix.json",
            "run_matrix_exists": _exists(EVIDENCE_ROOT / "pipeline" / "run_matrix.json"),
            "run_matrix_status": pipeline_run_matrix.get("status", "not_produced"),
            "raw_p95_ms": None,
            "harness_floor_ms": None,
            "floor_corrected_p95_ms": None,
            "cpu_p95": None,
            "private_working_set_max_mb": None,
            "drift_ms_per_hour": None,
            "drift_ci95_ms_per_hour": None,
            "dropout_count": None,
            "stall_recovery_s": None,
        },
        rationale=(
            "PC-1 (hardware decode, NV12 output) failed: environment.json records "
            "hardware_accelerated: false and output_format_supported: other. The pipeline "
            "apparatus is complete and tested on Linux CI, but the live measurement runs "
            "require the Windows 11 reference host with a verified hardware H.264 decoder. "
            "run_matrix.json is blocked_by_environment and no latency/resource/drift/stall "
            "evidence exists."
        ),
        evidence_refs=[
            "spikes/evidence/pipeline/run_matrix.json",
            "spikes/evidence/toolchain_selftest.json",
        ],
        measurement_conditions=(
            "Three >= 10-minute latency runs, one loss/jitter run, one 0 %-loss control, "
            "one 30-minute resource run, one 30-minute drift run, one 5 s stall. "
            "Not reproducible on this host (Windows 11 reference required)."
        ),
        preconditions=["PC-1", "PC-2", "PC-3", "PC-4"],
        included_latency_stages=[
            "capture",
            "encode",
            "network_and_jitter_buffer",
            "decode",
            "frame_handoff",
            "consumer_presentation",
        ],
        excluded_latency_stages=[],
        # The only capture-at-consumer p95 measured on this host is the harness floor
        # calibration run itself (empty harness, software-only selftest): raw == floor,
        # floor-corrected == 0.0 by construction. It is NOT a pipeline latency result
        # (no live capture exists); both figures are published together, and the report
        # is judged on the raw value as required.
        harness_floor_ms=10.98,
        raw_latency_p95_ms=10.98,
        floor_corrected_latency_p95_ms=0.0,
        latency_attribution=(
            "Not attributable to the product: the only measured capture-at-consumer p95 "
            "(10.98 ms) is the harness floor calibration itself, a software-only selftest "
            "baseline on this host; the floor-corrected value is 0.0 by construction. "
            "No live pipeline capture exists, so product latency is unmeasured and this "
            "figure must be replaced by the documented capture floor on the reference "
            "Windows machine."
        ),
        blocking_dependency="camera_injection",
        failure_code="precondition_failed",
    )

    # --- Gate 4: codec_interoperability (wave 3) ---
    gate4 = _gate(
        gate_id="codec_interoperability",
        title="Codec interoperability: mobile-to-desktop H.264/VP8 negotiation",
        wave=3,
        depends_on=["desktop_receive_pipeline"],
        verdict="conditional",
        accountable_role=technical_lead,
        issued_at=phase_review_at,
        criterion=(
            "Live video from the reference Android device decodes on the desktop for "
            "10 continuous minutes with agreed profile/level/format and no visible "
            "corruption; VP8 fallback approved with measured CPU, latency and quality "
            "trade-offs and hardware-decode status recorded."
        ),
        measured_result={
            "negotiated_profile": None,
            "negotiated_level": None,
            "negotiated_format": None,
            "duration_minutes": None,
            "corruption_detected": None,
            "hardware_accelerated": None,
            "interop_record_file": "spikes/evidence/interop/interop_record.json",
            "interop_record_exists": _exists(EVIDENCE_ROOT / "interop" / "interop_record.json"),
        },
        rationale=(
            "D-codec-baseline recommends H.264 High Profile Level 3.1 as the default "
            "with VP8 as the fallback. The interop harness (f0-codec-interop) is built "
            "and the corruption detector is implemented, but the live 10-minute run "
            "requires the reference Android device, which is not connected "
            "(environment.json records android_device_model: not_connected). Gate 4 is "
            "conditional on the live interop run and Q-7 (1080p level >= 4.0 vs level 3.1 "
            "receiver limits)."
        ),
        evidence_refs=[
            "spikes/evidence/decisions/D-codec-baseline.md",
        ],
        measurement_conditions=(
            "Reference Android device streaming live H.264/VP8 to the desktop receive "
            "pipeline for 10 continuous minutes; corruption detection automated; "
            "fallback comparison run recorded. Not reproducible on this host."
        ),
        preconditions=["PC-1"],
        conditions=[
            {
                "condition_id": "C-04-interop-live-run",
                "description": (
                    "10-minute live codec interop run on the reference Android device "
                    "with zero visible corruption."
                ),
                "conditioned_phases": ["F1", "F2"],
                "status": "blocking",
            }
        ],
        blocking_dependency="desktop_receive_pipeline",
        failure_code="environment_out_of_reference",
    )

    # --- Gate 5: virtual_audio_and_licensing (wave 3) ---
    audio_run_matrix = _load(EVIDENCE_ROOT / "audio" / "run_matrix.json", {})
    gate5 = _gate(
        gate_id="virtual_audio_and_licensing",
        title="Virtual audio path and licensing decision",
        wave=3,
        depends_on=["desktop_receive_pipeline"],
        verdict="conditional",
        accountable_role=product_owner,
        issued_at=phase_review_at,
        criterion=(
            "Zero dropouts >= 50 ms with agreeing detectors over >= 10 minutes under the "
            "pinned profile; phrases_exact >= 18 with all nonces/anchors matching; named "
            "observer and phrase script; consumer_renegotiations = 0; verified device "
            "signature; video-only degradation proven; licensing decision or explicit "
            "conditional/no_go with plan B."
        ),
        measured_result={
            "run_matrix_file": "spikes/evidence/audio/run_matrix.json",
            "run_matrix_exists": _exists(EVIDENCE_ROOT / "audio" / "run_matrix.json"),
            "run_matrix_status": audio_run_matrix.get("status", "not_produced"),
            "dropout_count": None,
            "detectors_agree": None,
            "phrases_exact": None,
            "corpus_wer": None,
            "observer": None,
            "phrase_script_ref": None,
            "device_pin": "VB-CABLE (driver pack 45)",
            "signature_verified": None,
            "consumer_renegotiations": None,
        },
        rationale=(
            "The audio harness, dropout analyser, intelligibility workflow and licensing "
            "assessment are complete and tested on Linux CI. The live measurement requires "
            "Windows 11 with Secure Boot, VB-CABLE driver pack 45, FFmpeg, a consumer "
            "application and a named product owner/approver for the licensing decision. "
            "run_matrix.json is blocked_by_environment; licensing_bundle.json is not "
            "emitted because the contract requires named signatories. In the summary "
            "below, zero counts mean nothing was recorded (no live run happened), "
            "detectors_agree=false means the two detectors were never run in tandem "
            "(agreement not evaluated, not a detected disagreement), and "
            "signature_verified=false means the VB-CABLE signature probe did not execute. "
            "Gate 5 is conditional "
            "on the licensing decision and the live audio run."
        ),
        evidence_refs=[
            "spikes/evidence/audio/run_matrix.json",
            "spikes/evidence/decisions/D-audio-licensing.md",
        ],
        measurement_conditions=(
            "Render scripted audio through VB-CABLE under the pinned impairment profile "
            "(3 % loss, 30 ms latency, +/- 10 ms jitter), record CABLE Output in a consumer "
            "application for >= 10 minutes, two-detector dropout analysis, 20-phrase "
            "intelligibility manifest with a named observer. Not reproducible on this host."
        ),
        preconditions=["PC-3", "PC-4"],
        audio_measurement_summary={
            "dropout_count": 0,
            "detectors_agree": False,
            "consumer_renegotiations": 0,
            "phrases_exact": 0,
            "corpus_wer": 0.0,
            "observer": "not named",
            "phrase_script_ref": "not recorded",
            "device_pin": "VB-CABLE (driver pack 45)",
            "signature_verified": False,
        },
        conditions=[
            {
                "condition_id": "C-05-licensing-decision",
                "description": (
                    "Named product owner and approver must sign the licensing decision "
                    "(VB-CABLE link-only with video-only plan B) by the gate date, or the "
                    "gate is conditional/no_go."
                ),
                "conditioned_phases": ["F1"],
                "status": "blocking",
            }
        ],
        blocking_dependency="desktop_receive_pipeline",
        failure_code="decision_unresolved",
    )

    gates = [gate1, gate2, gate3, gate4, gate5]

    # --- Apply the exit-record waivers to the gate conditions ---
    waivers_by_id = {waiver["condition_id"]: waiver for waiver in exit_record.get("waivers", [])}
    for gate in gates:
        for condition in gate.get("conditions", []):
            waiver = waivers_by_id.get(condition["condition_id"])
            if waiver is None:
                continue
            condition["status"] = "waived"
            condition["conditioned_phases"] = list(waiver["conditioned_phases"])
            condition["waiver"] = {
                "waived_by": waiver["waived_by"],
                "waived_at": waiver["waived_at"],
                "waiver_conditions": waiver["waiver_conditions"],
                "resolution_deadline": waiver["resolution_deadline"],
            }

    # --- Downstream status (waiver-aware) ---
    closure_mode = exit_record.get("closure_mode")
    if closure_mode == "closed_green":
        phase_status = "unblocked"
        phase_reason = "F0 is closed green; all gates are go."
    elif closure_mode == "closed_no_go":
        phase_status = "blocked"
        phase_reason = "F0 is closed no_go; the later phases cannot start."
    else:
        phase_status = "conditional"
        phase_reason = "F0 is closed with exceptions; the later phases may proceed under waivers."

    def waiver_label(waiver: dict) -> str:
        return f"{waiver['condition_id']} (deadline {waiver['resolution_deadline']})"

    f1_conditions = [
        waiver_label(waiver)
        for waiver in exit_record.get("waivers", [])
        if "F1" in waiver["conditioned_phases"]
    ]
    f2_conditions = ["F1 exit"] + [
        waiver_label(waiver)
        for waiver in exit_record.get("waivers", [])
        if "F2" in waiver["conditioned_phases"]
    ]
    f3_conditions = (
        ["F2 exit"]
        + [
            waiver_label(waiver)
            for waiver in exit_record.get("waivers", [])
            if "F3" in waiver["conditioned_phases"]
        ]
        + [
            f"{disposition['gate_id']} carried (deadline {disposition['deadline']})"
            for disposition in exit_record.get("gate_dispositions", [])
            if disposition["target_phase"] == "F3" and disposition["verdict"] == "no_go"
        ]
    )

    downstream = {
        "F1": {
            "status": phase_status,
            "reason": (
                "F0 is closed with exceptions; F1 may proceed under the C-02 and C-04 waivers."
                if phase_status == "conditional"
                else phase_reason
            ),
            "conditions": f1_conditions,
        },
        "F2": {
            "status": phase_status,
            "reason": "F2 depends on the F1 exit and on the codec and licensing conditions.",
            "conditions": f2_conditions,
        },
        "F3": {
            "status": phase_status,
            "reason": (
                "F3 depends on the F2 exit and on the carried injection and pipeline obligations."
            ),
            "conditions": f3_conditions,
        },
    }

    # --- Open questions (single source) ---
    question_document = _load(EVIDENCE_ROOT / "open_questions.json") or {}
    open_questions = []
    for question in question_document.get("open_questions", []):
        entry = {
            key: question[key]
            for key in (
                "question_id",
                "question",
                "owner",
                "impact",
                "blocking",
                "status",
            )
        }
        if question.get("context") is not None:
            entry["context"] = question["context"]
        if question.get("target_phase") is not None:
            entry["target_phase"] = question["target_phase"]
        open_questions.append(entry)

    # --- Decision index (single source) ---
    decision_document = _load(EVIDENCE_ROOT / "decisions" / "index.json") or {}
    decisions = []
    for decision in decision_document.get("decisions", []):
        entry = {key: decision[key] for key in ("decision_id", "topic", "status", "owner", "date")}
        for optional in ("approver", "path", "fallback"):
            if decision.get(optional) is not None:
                entry[optional] = decision[optional]
        decisions.append(entry)

    # Amendments are layered over the schema-pinned report: only the downstream status is
    # overridden here; render_report.py reads the same file for the dated narrative.
    amendments = (_load(EVIDENCE_ROOT / "gate_report_amendments.json") or {}).get("amendments", [])
    for amendment in amendments:
        for phase, override in amendment.get("downstream_status", {}).items():
            downstream[phase].update(override)

    report = {
        "report_version": 2,
        "feature": "001-f0-technical-spikes",
        "phase_review_at": phase_review_at,
        "report_prepared_by": technical_lead["person_name"],
        "reference_environment": {
            "environment_id": env.get("environment_id", "unknown"),
            "captured_at": env.get("captured_at"),
            "cpu_model": env.get("cpu_model", ""),
            "cpu_logical_cores": env.get("cpu_logical_cores", 0),
            "gpu_model": env.get("gpu_model", ""),
            "os_name": env.get("os_name", ""),
            "os_build": env.get("os_build", ""),
            "android_device_model": env.get("android_device_model"),
            "android_os_version": env.get("android_os_version"),
            "app_versions": env.get("app_versions", {}),
            "tool_versions": env.get("tool_versions", {}),
            "network": env.get("network", {}),
            "hardware_decode_verification": env.get("hardware_decode_verification", {}),
        },
        "gates": gates,
        "downstream_status": downstream,
        "decisions": decisions,
        "open_questions": open_questions,
        "exit": exit_record,
        "redistribution_note": (
            "All artefacts under spikes/ are NON-PRODUCTION F0 risk-reduction spike "
            "artefacts (FR-039). They must not be merged into desktop/ or mobile/ "
            "product modules. Raw captures remain local-only; only derived metrics, "
            "scripts and procedures are versioned."
        ),
    }
    return report


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Assemble the F0 gate report from recorded evidence."
    )
    parser.add_argument(
        "--out",
        default=str(EVIDENCE_ROOT / "gate_report.json"),
        help="Output path for gate_report.json",
    )
    parser.add_argument(
        "--schema",
        default=str(SCHEMA_PATH),
        help="Path to gate-report.schema.json (validation reference only)",
    )
    args = parser.parse_args()

    out_path = Path(args.out)
    report = build_report()

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as fh:
        json.dump(report, fh, indent=2, ensure_ascii=False)
        fh.write("\n")

    print(f"Wrote gate report to {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
