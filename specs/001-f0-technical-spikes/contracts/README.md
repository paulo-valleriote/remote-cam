# Contracts: F0 — Risk-Reduction Spikes and Technical Decisions

**Feature**: [001-f0-technical-spikes](../spec.md) | **Plan**: [plan.md](../plan.md) | **Data model**: [../data-model.md](../data-model.md)

These files define the **interfaces and artefact formats** that the F0 phase exposes. They are the contract-first deliverable of the constitution's Principle I: the public shape is fixed here, before any harness code is written, and none of it leaks a specific technology choice.

| File | Contract | Consumer |
| --- | --- | --- |
| [`signaling-messages.schema.json`](./signaling-messages.schema.json) | The closed allowlist of signaling messages exchanged over the authenticated channel. | Kotlin and Rust implementations; the conformance vectors in `protocol/vectors/` |
| [`qr-payload.md`](./qr-payload.md) | The binary QR payload layout, version 1, and its validation rules. | The desktop encoder and the mobile decoder |
| [`reason-codes.md`](./reason-codes.md) | The closed set of pairing-rejection, session-close and error codes, plus the throttling rules. | Both implementations; the gate report |
| [`injection-matrix.schema.json`](./injection-matrix.schema.json) | The per-application virtual-camera compatibility matrix. | The camera-injection gate |
| [`gate-report.schema.json`](./gate-report.schema.json) | The five gate verdicts, their conditions and their waivers. | The phase review of 2026-11-06 |
| [`measurement-formats.md`](./measurement-formats.md) | The raw evidence file formats (latency, resource, dropout, intelligibility, drift, impairment profile, throttling, residue, relay log, environment). | The measurement tooling and the gate report |

## Rules that apply to every contract here

1. **Closed sets.** Every enumeration is closed. An unknown member is a protocol violation, not a forward-compatible extension. A future addition requires a new **version**, and a peer rejects a version it does not support rather than attempting an implicit compatibility path.
2. **No free text on the wire.** Every wire message carries a code from a closed set, never a human-readable reason. Human-readable text is a presentation concern and lives in the UI layer.
3. **No secrets, ever.** Tokens, private keys and full fingerprints never appear in a log, a report or an error message.
4. **Versioned.** Every contract carries a version. The version is checked before the payload is interpreted.
5. **Independently implemented.** The Kotlin and Rust implementations are written separately and are validated against the **same conformance vectors** in `protocol/vectors/`. Neither side imports the other's source.

## How the spec clarifications are carried here

| Clarification | Where it lands in these contracts |
| --- | --- |
| Q1 — hardware decode is a verified precondition | `gate-report.schema.json` → `environment.hardware_decode_verification`; a failed check makes the gate `no_go` with `precondition_failed` |
| Q2 — dependency-ordered waves, one phase review | `gate-report.schema.json` → `gate.wave`, `gate.depends_on`, the shared `phase_review_at` |
| Q3 — automated dropout detection **and** a scripted human check | `measurement-formats.md` §4 (`dropouts.json`) and §5 (`intelligibility.json`, with the observer and the phrase script); `gate-report.schema.json` → `audio_measurement_summary` |
| Q4 — one named accountable role; conditional verdict blocks the phases it conditions | `gate-report.schema.json` → `role`, `condition`, `waiver` |
| Q5 — one pinned device; candidate licence terms assessed in parallel | `measurement-formats.md` §9 (`device_pin`) and §9.1 (`licensing_bundle.json`, with `remeasurement_required`) |
| Q6 — gate the **raw** capture-at-consumer p95; publish the floor and the floor-corrected value | `measurement-formats.md` §2.1 (`latency_summary.json`); `gate-report.schema.json` → `raw_latency_p95_ms`, `harness_floor_ms`, `floor_corrected_latency_p95_ms`, `latency_attribution` |
| Q7 — one nominal impairment profile | `measurement-formats.md` §7.1 (`impairment_profile.json`); `gate-report.schema.json` → `impairment_profile` |
| Q8 — F0 gates video latency only | No audio-latency field exists in any contract; the metric table in [`../data-model.md`](../data-model.md) §7 marks it out of scope |
| Q9 — per-origin attempt limit, progressive blocking, listener shutdown | `reason-codes.md` §1.3; `measurement-formats.md` §8.1 (`pairing_throttling.json`); `gate-report.schema.json` → `throttling_summary` |
| Q10 — zero pairing artefacts after a session end **and** after a restart, one inspection method | `measurement-formats.md` §8.2 (`pairing_residue.json`); `gate-report.schema.json` → `residue_summary` |

## Relationship to the shared contract module

`protocol/` at the repository root is the **source of truth** for the two contracts that both applications need in production — the signaling message schema and the QR payload layout. The files in this directory are the **planning artefacts** for those contracts plus the F0-specific artefact formats.

When Phase 1 (protocol implementation) begins, the canonical versions move to `protocol/` and the files here become their source. To avoid divergence, the following rule applies: **`protocol/` is canonical; this directory is a snapshot taken at plan time.** Any change to a contract after F0 is made in `protocol/` first and then reflected here.

## Artefact-only contracts

`injection-matrix.schema.json`, `gate-report.schema.json` and `measurement-formats.md` describe **files produced by the spike**, not messages exchanged by the product. They have no counterpart in `protocol/` and exist only for the F0 phase.
