# spikes/ — AGENTS.md

> Spike-specific rules. Do NOT apply to `desktop/`, `mobile/`, or `protocol/`.

**Quick start:**
- **Scope:** non-production F0 harnesses (`f0-pipeline/`, `f0-handshake/`, `f0-camera-injection/`, `f0-codec-interop/`, `f0-virtual-audio/`) and quarantined F0 code (`f0-signaling/`, `f0-media-core/`, `f0-injection/`, `f0-audio-bridge/`, `f0-dshow-vcam-fallback/`, `f0-mobile-core/`, `f0-mobile-app/`).
- **Hard rules:** see "Absolute rule (FR-039)" below — non-negotiable.
- **Before editing:** read `spikes/README.md`; run `preflight` + `selftest`; register `environment.json`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.agents/skills/`.

## Absolute rule (FR-039) — NON-NEGOTIABLE

> "Everything under `spikes/` is a disposable F0 spike artefact. Spike code MUST NOT be merged into the product modules under `desktop/` or `mobile/` (FR-039)."

- No `spikes/` file may be imported by `desktop/` or `mobile/`.
- No spike is promoted without a new implementation task in the product (`README.md` rule 4).
- Quality = documented procedure + evidence (`spikes/evidence/`), **not** test coverage (`spikes/` exempt from ≥90% gate).
- Each spike crate or module is marked `NON-PRODUCTION — F0 risk-reduction spike (FR-039)` in its manifest, entry module or README. The marker need not repeat in every file; quarantined code is marked once and covered by ADR-0003.
- Each spike `Cargo.toml`: `publish = false`, `edition = "2024"`, and a `description` naming it NON-PRODUCTION.
- Python `pyproject.toml`: `remotecam-spike-tools`, `python >= 3.12`, `publish = false`.

## Context and architecture

- **Active spikes:** `f0-pipeline/`, `f0-handshake/`, `f0-camera-injection/`, `f0-codec-interop/`, `f0-virtual-audio/`. All five are built harnesses. Live measurement runs require the Windows reference host.
- **Quarantined F0 code:** `f0-signaling/`, `f0-media-core/`, `f0-injection/`, `f0-audio-bridge/`, `f0-dshow-vcam-fallback/`, `f0-mobile-core/`, `f0-mobile-app/`. Moved out of `desktop/` and `mobile/` in F1-01; provenance only (ADR-0003). The Rust crates are standalone packages consumed by the spike harnesses; the mobile modules are source records and are not built.
- **Fixtures:** `fixtures/` (calibration WAV 48kHz mono 16-bit, CSV phrases with nonce).
- **Tooling:** `tools/` (Python: analysis, metrics, validation, redaction, preflight, selftest, report assembly).
- **Evidence:** `spikes/evidence/` — derived only; raw captures (`.wav/.mp4/.etl/.webm/.trace`) are `.gitignore`.
- **Makefile integration:** `scripts/make/spikes.mk` → `preflight`, `selftest`, `validate-evidence`, `gate-report`, `spike-*`.

## Spike-specific rules

### 1. Non-production and isolation

- No `spikes/` module importable by `desktop/` or `mobile/`.
- `f0-pipeline/Cargo.toml` references `remotecam-injection`/`remotecam-media-core` at `../f0-injection` and `../f0-media-core` — spike **consumes** quarantined code, never the product.
- `f0-handshake` references `remotecam-signaling` at `../f0-signaling`; `f0-virtual-audio` references `remotecam-audio-bridge` at `../f0-audio-bridge`.
- No `spikes/` code is merged into the product. Reuse requires a new implementation task in `desktop/` or `mobile/` (ADR-0003).

### 2. Evidence and validation

- `validate_evidence.py` validates evidence artefacts against JSON Schema (`gate-report.schema.json`, `injection-matrix.schema.json`, `signaling-messages.schema.json`) and semantic rules.
- Semantic rules (examples): latency `basis=raw`, stage sum ≤ raw, pairing `residue` empty, dropout = 0, detectors agree, phrases ≥18, WER ≤0.05.
- `redaction.py` applied to every generated file; no secrets, fingerprints, or personal data.
- `preflight.py` — verifies reference environment (`hardware_accelerated`, `within_reference_conditions`). If either check is false, evidence must be flagged.
- `selftest.py` — mandatory checks (silence fixture, QR roundtrip, sampler agreement, harness floor). If any check fails, no measurement is valid.

### 3. Each spike

| Spike | Stack | Purpose / Gate | Evidence | Makefile |
|-------|-------|----------------|----------|----------|
| `f0-pipeline/` | Rust | Desktop receive pipeline (Gate 3): latency, dropout, resources, drift, stall | `spikes/evidence/pipeline/` | `spike-pipeline`, `spike-latency`, `spike-loss`, `spike-resources`, `spike-drift`, `spike-stall`, `spike-synthetic-source` |
| `f0-handshake/` | Rust | Pairing + throttling + residue (Gate 2) | `spikes/evidence/pairing/` | `spike-handshake` |
| `f0-camera-injection/` | Python + PowerShell | Virtual camera injection (Gate 1): per-app matrix, MF vs DirectShow backends | `spikes/evidence/injection/` | `spike-injection` |
| `f0-codec-interop/` | Rust | Codec interop (Gate 4): H.264/VP8 negotiation, hardware acceleration | `spikes/evidence/` (via `InteropMetrics`) | `spike-interop` |
| `f0-virtual-audio/` | Rust + Python + PowerShell | Virtual audio + licensing (Gate 5): two-detector dropouts, scripted intelligibility, device signature, degraded video-only, licence catalogue | `spikes/evidence/audio/`, `spikes/evidence/licensing/` | `spike-audio` |

### 4. Python tooling (`tools/`)

- `analysis/` — dropouts, floor, injection, interop_corruption, intelligibility, latency, licensing, marker, sampler, stages.
- `metrics/criteria.py` — quality criteria.
- `validation/` — `validate_evidence.py`, `preflight.py`, `selftest.py`.
- `redaction.py` — mandatory filter.
- `windows/probe_hardware_decode.ps1` — `MFTEnumEx` for NV12.
- `tests/test_*.py` — tool tests.

### 5. Before modifying

- Before creating a new spike, read `spikes/README.md` (non-production policy, FR-039).
- Before modifying `f0-pipeline/`: check `specs/001-f0-technical-spikes/plan.md` (gate waves 1→2→3).
- Before generating evidence: run `preflight` and `selftest`; register `environment.json` and `toolchain_selftest.json`.
- Before merging any `spikes/` file into `desktop/` or `mobile/`: **stop immediately** — violates FR-039.
