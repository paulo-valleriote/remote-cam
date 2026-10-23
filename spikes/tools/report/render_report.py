"""Render the F0 gate report to Markdown.

Reads ``spikes/evidence/gate_report.json`` and produces
``docs/spike/gate-report-f0.md`` with the five gate verdicts, evidence
summaries, conditions/waivers, downstream status, open questions and the
non-production statement.

NON-PRODUCTION — F0 risk-reduction spike (FR-039).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
REPORT_PATH = REPO_ROOT / "spikes" / "evidence" / "gate_report.json"
OUT_PATH = REPO_ROOT / "docs" / "spike" / "gate-report-f0.md"


def _verdict_badge(verdict: str) -> str:
    return {
        "go": "**go**",
        "no_go": "**no_go**",
        "conditional": "**conditional**",
    }.get(verdict, f"**{verdict}**")


def _role(role: dict) -> str:
    return f"{role.get('role_type', 'unknown')} — {role.get('person_name', 'not named')}"


def render(report: dict) -> str:
    lines: list[str] = []

    lines.append("# F0 Gate Report — Phase 8 Polish")
    lines.append("")
    lines.append(f"**Feature:** {report['feature']}")
    lines.append(f"**Phase review at:** {report['phase_review_at']}")
    lines.append(f"**Report version:** {report['report_version']}")
    lines.append("")

    env = report.get("reference_environment", {})
    lines.append("## Reference environment")
    lines.append("")
    lines.append(f"- Environment ID: {env.get('environment_id', 'unknown')}")
    lines.append(f"- Captured at: {env.get('captured_at', 'not recorded')}")
    cpu_cores = env.get("cpu_logical_cores", "?")
    lines.append(f"- CPU: {env.get('cpu_model', 'unknown')} ({cpu_cores} logical cores)")
    lines.append(f"- GPU: {env.get('gpu_model', 'unknown')}")
    lines.append(f"- OS: {env.get('os_name', 'unknown')} {env.get('os_build', '')}")
    lines.append(f"- Android device: {env.get('android_device_model', 'not connected')}")
    within_ref = env.get("network", {}).get("within_reference_conditions", False)
    lines.append(f"- Within reference conditions: {within_ref}")
    hdv = env.get("hardware_decode_verification", {})
    lines.append(
        f"- Hardware decode: {hdv.get('codec', 'H.264')} | "
        f"decoder_found: {hdv.get('decoder_found', False)} | "
        f"hardware_accelerated: {hdv.get('hardware_accelerated', False)} | "
        f"output_format: {hdv.get('output_format_supported', 'unknown')}"
    )
    lines.append("")

    lines.append("## Gate verdicts")
    lines.append("")
    lines.append("| Gate | Wave | Depends on | Verdict | Accountable role | Failure code |")
    lines.append("|------|------|------------|---------|------------------|--------------|")
    for g in report["gates"]:
        lines.append(
            "| {gate_id} | {wave} | {depends_on} | {verdict} | {role} | {failure_code} |".format(
                gate_id=g["gate_id"],
                wave=g["wave"],
                depends_on=", ".join(g.get("depends_on", [])) or "—",
                verdict=_verdict_badge(g["verdict"]),
                role=_role(g["accountable_role"]),
                failure_code=g.get("failure_code") or "—",
            )
        )
    lines.append("")

    lines.append("## Gate details")
    lines.append("")
    for g in report["gates"]:
        lines.append(f"### {g['gate_id']} — {g['title']}")
        lines.append("")
        lines.append(f"- **Wave:** {g['wave']}")
        lines.append(f"- **Verdict:** {_verdict_badge(g['verdict'])}")
        if g.get("failure_code"):
            lines.append(f"- **Failure code:** {g['failure_code']}")
        if g.get("blocking_dependency"):
            lines.append(f"- **Blocking dependency:** {g['blocking_dependency']}")
        lines.append(f"- **Accountable role:** {_role(g['accountable_role'])}")
        lines.append(f"- **Criterion:** {g['criterion']}")
        lines.append(f"- **Measurement conditions:** {g['measurement_conditions']}")
        lines.append(f"- **Rationale:** {g['rationale']}")
        lines.append(f"- **Evidence refs:** {', '.join(g.get('evidence_refs', [])) or 'none'}")
        if g.get("preconditions"):
            lines.append(f"- **Preconditions:** {', '.join(g['preconditions'])}")
        lines.append("")

        # Latency-specific fields (gate 3)
        if g["gate_id"] == "desktop_receive_pipeline":
            included = ", ".join(g.get("included_latency_stages", []))
            excluded = ", ".join(g.get("excluded_latency_stages", [])) or "none"
            lines.append(f"- **Included latency stages:** {included}")
            lines.append(f"- **Excluded latency stages:** {excluded}")
            raw_p95 = g.get("raw_latency_p95_ms", "not measured")
            lines.append(f"- **Harness floor (ms):** {g.get('harness_floor_ms', 'not measured')}")
            lines.append(f"- **Raw latency p95 (ms):** {raw_p95}")
            corrected = g.get("floor_corrected_latency_p95_ms", "not measured")
            lines.append(f"- **Floor-corrected latency p95 (ms):** {corrected}")
            if g.get("latency_attribution"):
                lines.append(f"- **Latency attribution:** {g['latency_attribution']}")
            lines.append("")

        # Pairing-specific fields (gate 2)
        if g["gate_id"] == "pairing_handshake":
            ts = g.get("throttling_summary", {})
            rs = g.get("residue_summary", {})
            lines.append("- **Throttling summary:**")
            limit = ts.get("attempt_limit_per_minute", "n/a")
            exceeded = ts.get("limit_exceeded_at_attempt", "n/a")
            close = ts.get("consecutive_failures_at_listener_close", "n/a")
            unaffected = ts.get("legitimate_pairing_unaffected", False)
            lines.append(f"  - Attempt limit per minute: {limit}")
            lines.append(f"  - Limit exceeded at attempt: {exceeded}")
            lines.append(f"  - Consecutive failures at listener close: {close}")
            lines.append(f"  - Listener closed: {ts.get('listener_closed', False)}")
            lines.append(f"  - Session established: {ts.get('session_established', False)}")
            lines.append(f"  - Legitimate pairing unaffected: {unaffected}")
            lines.append(f"  - Evidence ref: {ts.get('evidence_ref', 'none')}")
            lines.append("- **Residue summary:**")
            lines.append(f"  - Method: {rs.get('method', 'not recorded')}")
            lines.append(f"  - Coverage: {', '.join(rs.get('coverage', []))}")
            after_end = rs.get("after_session_end_artefacts_found", [])
            after_restart = rs.get("after_application_restart_artefacts_found", [])
            lines.append(f"  - After session end artefacts found: {after_end}")
            lines.append(f"  - After application restart artefacts found: {after_restart}")
            lines.append("")

        # Audio-specific fields (gate 5)
        if g["gate_id"] == "virtual_audio_and_licensing":
            ams = g.get("audio_measurement_summary", {})
            lines.append("- **Audio measurement summary:**")
            lines.append(f"  - Dropout count: {ams.get('dropout_count', 'not measured')}")
            renegotiations = ams.get("consumer_renegotiations", "not measured")
            lines.append(f"  - Detectors agree: {ams.get('detectors_agree', False)}")
            lines.append(f"  - Consumer renegotiations: {renegotiations}")
            lines.append(f"  - Phrases exact: {ams.get('phrases_exact', 'not measured')}")
            lines.append(f"  - Corpus WER: {ams.get('corpus_wer', 'not measured')}")
            lines.append(f"  - Observer: {ams.get('observer', 'not named')}")
            lines.append(f"  - Phrase script ref: {ams.get('phrase_script_ref', 'none')}")
            lines.append(f"  - Device pin: {ams.get('device_pin', 'not recorded')}")
            lines.append(f"  - Signature verified: {ams.get('signature_verified', False)}")
            lines.append("")

        # Impairment profile
        if g.get("impairment_profile"):
            ip = g["impairment_profile"]
            lines.append("- **Impairment profile:**")
            lines.append(f"  - Profile name: {ip.get('profile_name')}")
            lines.append(f"  - Loss: {ip.get('loss_pct')}% ({ip.get('loss_distribution')})")
            lines.append(f"  - Added latency: {ip.get('added_latency_ms')} ms")
            lines.append(f"  - Jitter: {ip.get('jitter_ms')} ms")
            lines.append(f"  - Applies to: {ip.get('applies_to')}")
            lines.append("")

        # Conditions
        if g.get("conditions"):
            lines.append("- **Conditions:**")
            for c in g["conditions"]:
                phases = ", ".join(c["conditioned_phases"])
                lines.append(
                    f"  - {c['condition_id']}: {c['description']} "
                    f"(status: {c['status']}, conditioned phases: {phases})"
                )
                if c.get("waiver"):
                    w = c["waiver"]
                    who = w.get("waived_by", {}).get("person_name", "unknown")
                    deadline = w.get("resolution_deadline", "none")
                    lines.append(
                        f"    - Waiver: waived by {who} on {w.get('waived_at', 'unknown')}, "
                        f"deadline {deadline}"
                    )
            lines.append("")

    lines.append("## Downstream status (F1 / F2 / F3)")
    lines.append("")
    ds = report.get("downstream_status", {})
    for phase in ("F1", "F2", "F3"):
        st = ds.get(phase, {})
        no_reason = st.get("reason", "no reason recorded")
        lines.append(f"- **{phase}:** {st.get('status', 'unknown')} — {no_reason}")
        if st.get("conditions"):
            lines.append(f"  - Conditions: {', '.join(st['conditions'])}")
    lines.append("")

    lines.append("## Decision index")
    lines.append("")
    lines.append("| Decision | Topic | Status | Owner | Approver | Date | Path | Fallback |")
    lines.append("|----------|-------|--------|-------|----------|------|------|----------|")
    for d in report.get("decisions", []):
        line = (
            "| {decision_id} | {topic} | {status} | {owner} | {approver} | {date} "
            "| {path} | {fallback} |"
        ).format(
            decision_id=d.get("decision_id", ""),
            topic=d.get("topic", ""),
            status=d.get("status", ""),
            owner=d.get("owner", ""),
            approver=d.get("approver", "not named"),
            date=d.get("date", "not set"),
            path=d.get("path", "not recorded"),
            fallback=d.get("fallback", "none"),
        )
        lines.append(line)
    lines.append("")

    lines.append("## Open questions")
    lines.append("")
    lines.append("| Question | Owner | Impact | Blocking | Status |")
    lines.append("|----------|-------|--------|----------|--------|")
    for oq in report.get("open_questions", []):
        lines.append(
            "| {qid} | {owner} | {impact} | {blocking} | {status} |".format(
                qid=oq.get("question_id", ""),
                owner=oq.get("owner", ""),
                impact=oq.get("impact", ""),
                blocking="yes" if oq.get("blocking") else "no",
                status=oq.get("status", ""),
            )
        )
    lines.append("")

    lines.append("## Non-production statement")
    lines.append("")
    lines.append(report.get("redistribution_note", ""))
    lines.append("")

    return "\n".join(lines)


def main() -> int:
    if not REPORT_PATH.exists():
        print(
            f"error: gate report not found at {REPORT_PATH}; run `make gate-report` first.",
            file=sys.stderr,
        )
        return 1

    with REPORT_PATH.open("r", encoding="utf-8") as fh:
        report = json.load(fh)

    markdown = render(report)

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(markdown, encoding="utf-8")

    print(f"Wrote rendered gate report to {OUT_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
