# RemoteCam — AGENTS.md (General)

> Full context: F0 risk-reduction phase; `spikes/` are non-production harnesses.

**Quick start:**
- **Scope:** cross-cutting rules for the four sub-parts (`desktop/`, `mobile/`, `protocol/`, `spikes/`).
- **Hard rules:** see "Absolute rule" below — non-negotiable.
- **Before editing:** read the sub-part `AGENTS.md`; build with `make build-desktop` / `make build-mobile`; for spikes, be evidence-first (`spikes/evidence/`).

**Context sources:** `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.specify/init-options.json`, `.claude/skills/technical-design-doc-creator/`, `.opencode/commands/speckit.*`. Agent commands: `speckit.plan`, `speckit.implement`, `speckit.checklist` (see `.opencode/`).

## Absolute rule (non-negotiable)

- `spikes/` is **non-production**. Nothing under `spikes/` may be merged into `desktop/` or `mobile/` (FR-039).
- All `spikes/` code is disposable. Spike quality = documented procedure + evidence, **not** test coverage.
- `protocol/` is the shared, versioned contract (v1 only).

## Repository layout

```
remotecam/
  desktop/      # Rust receive/inject application
  mobile/       # Kotlin capture/transmit application
  protocol/     # Shared, versioned, language-neutral contracts
  spikes/       # NON-PRODUCTION — F0 measurement harnesses, fixtures, tooling
  specs/        # Specification, plan, research, contracts
  docs/         # Spike procedures, gate reports, logging schema
  scripts/      # Makefile fragments and CI helpers
```

## Cross-cutting conventions

- **Language:** All documentation, code, logs, error messages = English. No free-form text on the wire.
- **Documentation:** Every command documented in `README.md` must exist as a Makefile target.
- **Structured logs:** JSON lines, no tokens/fingerprints/secrets/personal data. Redaction mandatory (hex ≥32 chars, absolute paths, correlation IDs = 32 lowercase hex).
- **Dependencies:** `protocol/` is the source of truth. `desktop/` and `mobile/` consume, not define.
- **Coverage:** ≥90% for `protocol/` (`desktop/crates/protocol` and `mobile/protocol`). `spikes/` is exempt.
- **Build:** `make help` is the source of truth. Use `make build-desktop`, `make build-mobile`, `make test`, `make coverage`, `make lint`.

## Each complex part

Every complex subdirectory (`desktop/`, `mobile/`, `protocol/`, `spikes/`) has its own `AGENTS.md` with rules that **do not** apply to other parts.

- `desktop/AGENTS.md` — Windows Media Foundation, DirectShow fallback, `rust-toolchain.toml` (1.99.1), `rustls`, `subtle` constant-time pinning, token zeroization.
- `mobile/AGENTS.md` — Kotlin Serialization JSON, strict parsing (unknown keys rejected), TLS 1.3 certificate pinning, `compileSdk` 36, `:protocol`/`:core`/`:app` modules, non-production rules.
- `protocol/AGENTS.md` — Versioned contracts (v1), `signaling.schema.json`, `reason-codes.json` (closed reason codes), conformance harness, `protocol/vectors/`.
- `spikes/AGENTS.md` — FR-039 (no merge), evidence in `spikes/evidence/`, `validate_evidence.py`, `redaction.py`, Python tooling (`pyproject.toml`), Rust harnesses (`Cargo.toml` non-production).

## When to create/update

- When modifying contracts in `protocol/`, check `protocol/AGENTS.md` and update `signaling.schema.json` / `reason-codes.json`.
- When creating a new spike, put it in `spikes/` (never `desktop/` or `mobile/`), register evidence, update `spikes/AGENTS.md`.
- When modifying `desktop/` with Windows-specific code, consult `desktop/AGENTS.md` before using the `windows` crate or DirectShow.
- When modifying `mobile/`, verify strict JSON parsing and certificate pinning (`mobile/AGENTS.md`).
