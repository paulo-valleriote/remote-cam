"""Assemble the F0 gate report from recorded evidence.

Produces ``spikes/evidence/gate_report.json`` conforming to
``specs/001-f0-technical-spikes/contracts/gate-report.schema.json``.

NON-PRODUCTION — F0 risk-reduction spike (FR-039). This script and its output
are disposable spike artefacts and must never be merged into desktop/ or mobile/.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
EVIDENCE_ROOT = REPO_ROOT / "spikes" / "evidence"
SCHEMA_PATH = (
    REPO_ROOT / "specs" / "001-f0-technical-spikes" / "contracts" / "gate-report.schema.json"
)

PHASE_REVIEW_AT = "2026-11-06T00:00:00+00:00"


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
        "issued_at": PHASE_REVIEW_AT,
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


def _open_question(
    qid: str,
    question: str,
    owner: str,
    impact: str,
    blocking: bool,
    status: str,
    context: str | None = None,
) -> dict:
    oq = {
        "question_id": qid,
        "question": question,
        "owner": owner,
        "impact": impact,
        "blocking": blocking,
        "status": status,
    }
    if context is not None:
        oq["context"] = context
    return oq


def _decision_index(
    decision_id: str,
    topic: str,
    status: str,
    owner: str,
    date: str | None = None,
    approver: str | None = None,
    path: str | None = None,
    fallback: str | None = None,
) -> dict:
    entry = {
        "decision_id": decision_id,
        "topic": topic,
        "status": status,
        "owner": owner,
        "date": date or PHASE_REVIEW_AT[:10],
    }
    if approver is not None:
        entry["approver"] = approver
    if path is not None:
        entry["path"] = path
    if fallback is not None:
        entry["fallback"] = fallback
    return entry


def build_report() -> dict:
    env = _load(EVIDENCE_ROOT / "environment.json") or {}

    # --- Gate 1: camera_injection (wave 1) ---
    gate1 = _gate(
        gate_id="camera_injection",
        title="Virtual camera injection across the target application matrix",
        wave=1,
        depends_on=["foundational"],
        verdict="no_go",
        accountable_role=_role("technical_lead", "not_yet_named"),
        criterion=(
            "100 % frames_delivered on the selected backend, observed_duration_s >= 1800, "
            "longest_stall_ms <= 2000, both bitnesses, multi-consumer observation"
        ),
        measured_result={
            "matrix_file": "spikes/evidence/injection/injection_matrix.json",
            "matrix_exists": False,
            "selected_backend": "not_determined",
            "frames_delivered_cells": 0,
            "total_cells": 0,
        },
        rationale=(
            "The injection matrix requires the Windows 11 reference machine with "
            "Media Foundation, FFmpeg, OBS, the conferencing client, Windows Camera, "
            "a screen recorder and a 32-bit consumer. None are available on this host "
            "(environment.json records within_reference_conditions: false and no camera "
            "or FFmpeg). injection_matrix.json does not exist, so the backend cannot be "
            "selected and gate 1 has no measured evidence."
        ),
        evidence_refs=[],
        measurement_conditions=(
            "Clean Windows 11 x64, non-administrator account, five application categories "
            "plus a 32-bit consumer, 30 minutes per cell. Not reproducible on this host."
        ),
        preconditions=[],
        blocking_dependency=None,
        failure_code="environment_out_of_reference",
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
        accountable_role=_role("security_reviewer", "not_yet_named"),
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
        accountable_role=_role("technical_lead", "not_yet_named"),
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
        accountable_role=_role("technical_lead", "not_yet_named"),
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
        accountable_role=_role("product_owner", "not_yet_named"),
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

    # --- Downstream status ---
    # F1 unblocked only if all gates go; F2/F3 depend on earlier gates.
    go_gates = [g for g in gates if g["verdict"] == "go"]
    any_no_go = any(g["verdict"] == "no_go" for g in gates)

    if any_no_go:
        f1_status = "blocked"
        f1_reason = "One or more F0 gates are no_go; F1 cannot start until they are resolved."
    elif len(go_gates) < len(gates):
        f1_status = "conditional"
        f1_reason = (
            "Some F0 gates are conditional; F1 may proceed under waivers with resolution deadlines."
        )
    else:
        f1_status = "unblocked"
        f1_reason = "All F0 gates are go."

    blocked = any_no_go
    unresolved = len(go_gates) < len(gates)
    f1_conditions = [] if f1_status == "unblocked" else ["resolve all no_go/conditional F0 gates"]
    f2_or_f3_status = "blocked" if blocked else ("conditional" if unresolved else "unblocked")

    downstream = {
        "F1": {
            "status": f1_status,
            "reason": f1_reason,
            "conditions": f1_conditions,
        },
        "F2": {
            "status": f2_or_f3_status,
            "reason": "F2 depends on F1 and on the gates it consumes (camera injection, "
            "pipeline, codec interop).",
            "conditions": f1_conditions,
        },
        "F3": {
            "status": f2_or_f3_status,
            "reason": "F3 depends on F2 and on all F0 gates.",
            "conditions": f1_conditions,
        },
    }

    # --- Open questions ---
    open_questions = [
        _open_question(
            "Q-1",
            "Does per-user (HKCU) COM registration of the MF virtual camera media source "
            "actually work, and is an MSI installer therefore required despite MSIX being "
            "the planned package format?",
            "Technical lead",
            "Decides the installer design and the no-admin promise for the camera",
            True,
            "open",
            "Open question 1 in TDD; recorded as not answered because the registration "
            "matrix has not run on Windows 11.",
        ),
        _open_question(
            "Q-2",
            "Is the VB-CABLE driver signed in a way that Windows 11 with Secure Boot "
            "enabled accepts?",
            "Technical lead",
            "Determines whether the audio gate can pass at all",
            True,
            "open",
            "Unresolved; requires signature_probe.ps1 on the Secure-Boot Windows 11 runner.",
        ),
        _open_question(
            "Q-3",
            "What is the actual per-unit cost of a VB-CABLE OEM/redistribution agreement, "
            "and does the user population qualify as a 'professional domain' under "
            "VB-Audio's terms?",
            "Product owner + legal",
            "Determines whether V1 ships link-only or bundled, and the unit economics",
            True,
            "open",
            "Partially answered by published volume prices; professional-domain "
            "classification remains open.",
        ),
        _open_question(
            "Q-4",
            "Will VAC's 'companion application' / 'auxiliary transport' characterisation "
            "cover RemoteCam, and at what price?",
            "Product owner + legal",
            "The cleaner long-term audio model",
            False,
            "open",
            "Parallel track, not a gate blocker.",
        ),
        _open_question(
            "Q-5",
            "Does the reference machine expose an H.264 decoder through the SYNCMFT "
            "discovery path, and is it actually hardware-accelerated?",
            "Technical lead",
            "FR-044 precondition — without it, no latency or resource result is admissible",
            True,
            "open",
            "Partial verification: hardware_accelerated=true but output_format=other "
            "(not NV12). NV12 verification pending.",
        ),
        _open_question(
            "Q-6",
            "Can two consumer applications open the virtual camera simultaneously, and "
            "what does each observe?",
            "Technical lead",
            "Affects the shared-buffer design",
            False,
            "open",
            "Recorded, not gating; requires the multi-consumer observation on Windows 11.",
        ),
        _open_question(
            "Q-7",
            "Is 720p30 sufficient as the default quality, and is 1080p30 viable given that "
            "1080p needs H.264 level >= 4.0 while many receivers accept only level 3.1?",
            "Product owner",
            "Affects bandwidth, CPU and the negotiated level",
            False,
            "open",
            "Open question 3 in TDD; default is 720p30 Level 3.1 with 1080p optional.",
        ),
        _open_question(
            "Q-8",
            "Can a 32-bit consumer application see the virtual camera on 64-bit Windows?",
            "Technical lead",
            "The DirectShow path needs separate per-bitness CLSIDs; the MF path behaviour "
            "is unknown",
            True,
            "open",
            "Recorded as a matrix failure criterion; requires a 32-bit consumer on Windows 11.",
        ),
        _open_question(
            "Q-9",
            "What is the residue inspection coverage and method that satisfies the gate "
            "for both session end and application restart?",
            "Security reviewer",
            "Defines the no-residue proof for the pairing gate",
            True,
            "open",
            "Open question 09 in TDD §18; residue restart scan not yet performed.",
        ),
        _open_question(
            "Q-10",
            "Does the CameraX YUV_420_888 chroma pixel stride equal 2 on any device in the "
            "reference set?",
            "Mobile engineer",
            "A stride-2 device with JavaI420Buffer.wrap produces corrupt colour",
            False,
            "open",
            "Guarded at runtime but must be reported; requires the reference Android device.",
        ),
    ]

    # --- Decision index ---
    decisions = [
        _decision_index(
            "D-10",
            "Injection backend: MF virtual camera vs DirectShow filter",
            "draft",
            "Technical lead",
            path="spikes/evidence/decisions/D-10-injection-backend.md",
            fallback="dshow_filter (DirectShow capture filter)",
        ),
        _decision_index(
            "D-codec-baseline",
            "Video codec baseline for F1: H.264 vs VP8",
            "draft",
            "Technical lead",
            date="2026-10-22",
            approver="Technical lead",
            path="spikes/evidence/decisions/D-codec-baseline.md",
            fallback="VP8 software fallback",
        ),
        _decision_index(
            "D-quality-default",
            "Default video quality: 1280x720@30 default, 1920x1080@30 optional maximum",
            "draft",
            "Technical lead",
            path="spikes/evidence/decisions/D-quality-default.md",
            fallback="1280x720@30 only (Option C)",
        ),
        _decision_index(
            "D-audio-licensing",
            "Virtual audio device sourcing and licensing",
            "draft",
            "Product owner",
            path="spikes/evidence/decisions/D-audio-licensing.md",
            fallback="Video-only V1 (plan B)",
        ),
    ]

    report = {
        "report_version": 1,
        "feature": "001-f0-technical-spikes",
        "phase_review_at": PHASE_REVIEW_AT,
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
