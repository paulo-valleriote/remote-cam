# RemoteCam — AGENTS.md (General)

> Full context: F0 is closed with exceptions (see `docs/spike/gate-report-f0.md`); F1 is in progress; `spikes/` are non-production harnesses.

**Quick start:**
- **Scope:** cross-cutting rules for the five sub-parts (`desktop/`, `mobile/`, `protocol/`, `harness/`, `spikes/`).
- **Hard rules:** see "Absolute rule" below — non-negotiable.
- **Before editing:** read `docs/constitution.md` and the sub-part `AGENTS.md`; build with `make build-desktop` / `make build-mobile`; run the product harness with `make harness`; for spikes, be evidence-first (`spikes/evidence/`).

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.agents/skills/`. Planning and issues live in Jira board `REC` (see `docs/agents/issue-tracker.md`).

## Constitution (non-negotiable)

`docs/constitution.md` supersedes any conflicting project practice. When the
constitution, a README, the Makefile, an ADR or local convention conflict, the
constitution prevails. Every review verifies Principles I–III and the module
boundaries.

## Absolute rule (non-negotiable)

- `spikes/` is **non-production**. Nothing under `spikes/` may be merged into `desktop/`, `mobile/` or `harness/` (FR-039).
- All `spikes/` code is disposable. Spike quality = documented procedure + evidence, **not** test coverage.
- Quarantined F0 code under `spikes/` is provenance only; reuse requires a new product implementation (ADR-0003).
- `protocol/` is the shared, versioned contract (v1 only, frozen for F1 per ADR-0002).

## Repository layout

```
remotecam/
  desktop/      # Rust receive/inject application (contract, session core, product shell)
  harness/      # Product-owned headless harness (own workspace)
  mobile/       # Kotlin capture/transmit application (contract module)
  protocol/     # Shared, versioned, language-neutral contracts
  spikes/       # NON-PRODUCTION — F0 harnesses, quarantined F0 code, fixtures, tooling
  specs/        # Immutable F0 specification record
  docs/         # Constitution, ADRs, spike procedures, gate reports, logging schema
  scripts/      # Makefile fragments and CI helpers
  GLOSSARY.md   # Shared domain vocabulary
```

## Cross-cutting conventions

- **Language:** All documentation, code, logs, error messages = English. No free-form text on the wire.
- **Documentation:** Every command documented in `README.md` must exist as a Makefile target.
- **Structured logs:** JSON lines, no tokens/fingerprints/secrets/personal data. Redaction mandatory (hex ≥32 chars, absolute paths, correlation IDs = 32 lowercase hex).
- **Dependencies:** `protocol/` is the source of truth. `desktop/`, `mobile/` and `harness/` consume, not define.
- **Coverage:** ≥90% for the delivered packages (`desktop/crates/protocol`, `desktop/crates/session-core` and `mobile/protocol`). `spikes/` is exempt; the harness is not a shipped package (ADR-0008).
- **Build:** `make help` is the source of truth. Use `make build-desktop`, `make build-mobile`, `make harness`, `make test`, `make coverage`, `make lint`.
- **Decisions:** record durable choices in `docs/adr/`; use `GLOSSARY.md` vocabulary in issues, code and tests.

## Each complex part

Every complex subdirectory (`desktop/`, `mobile/`, `protocol/`, `harness/`, `spikes/`) has its own `AGENTS.md` with rules that **do not** apply to other parts.

- `desktop/AGENTS.md` — Rust workspace (3 crates), Windows Media Foundation boundaries, `rust-toolchain.toml` (1.99.1), `rustls`, `subtle` constant-time pinning, token zeroization, structured-logging redaction.
- `mobile/AGENTS.md` — Kotlin Serialization JSON, strict parsing (unknown keys rejected), TLS 1.3 certificate pinning, `compileSdk` 36, the `:protocol` module.
- `protocol/AGENTS.md` — Versioned contracts (v1), `signaling.schema.json`, `reason-codes.json` (closed reason codes), conformance harness, `protocol/vectors/`.
- `harness/AGENTS.md` — Product-owned headless harness, own workspace and toolchain, FR-039 (no `spikes/` imports), deterministic JSON report, `make harness`.
- `spikes/AGENTS.md` — FR-039 (no merge), quarantined F0 code, evidence in `spikes/evidence/`, `validate_evidence.py`, `redaction.py`, Python tooling (`pyproject.toml`), Rust harnesses (`Cargo.toml` non-production).

## When to create/update

- When modifying contracts in `protocol/`, check `protocol/AGENTS.md` and update `signaling.schema.json` / `reason-codes.json`. F1 is under a wire freeze (ADR-0002).
- When creating a new spike, put it in `spikes/` (never `desktop/` or `mobile/`), register evidence, update `spikes/AGENTS.md`.
- When modifying `desktop/` with Windows-specific code, consult `desktop/AGENTS.md`.
- When modifying `mobile/`, verify strict JSON parsing and certificate pinning (`mobile/AGENTS.md`).
- When modifying `harness/`, read `harness/AGENTS.md`; the harness is product code and MUST NOT import `spikes/` (FR-039).
- When a durable decision is made, add an ADR under `docs/adr/`. When a domain term is resolved, update `GLOSSARY.md`.

## Agent skills

### Issue tracker

Jira via Atlassian MCP (board REC: to do / in progress / done). See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context (root GLOSSARY.md + docs/adr/). See `docs/agents/domain.md`.
