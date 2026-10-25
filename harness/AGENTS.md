# harness/ — AGENTS.md

> Product-harness rules. Do NOT apply to `desktop/`, `mobile/`, `protocol/` or `spikes/`.

**Quick start:**
- **Scope:** the product-owned, headless harness that drives the Session Core end to end and emits a deterministic, machine-readable report.
- **Hard rules:**
  - Nothing under `spikes/` is imported here (FR-039, ADR-0003); shared behaviour is reimplemented against the product contract.
  - The harness is product code, not a spike: it follows the constitution, lives in the normal test and lint suites, and is reviewed like `desktop/`.
  - It is its own Cargo workspace with its own `rust-toolchain.toml` (1.99.1) and its own `Cargo.lock`; dependencies are pinned with `=`.
  - It is headless, runs on Linux/CI and is never shipped to users.
- **Before editing:** read `docs/constitution.md`, `docs/adr/0007-harness-location.md` and `docs/adr/0008-harness-dependency-baseline.md`; run `make harness`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `desktop/crates/session-core`.

## Context and architecture

- **Location:** top-level `harness/`, decided in ADR-0007. The F0 harnesses stay under `spikes/` and are disposable.
- **Workspace:** `harness/Cargo.toml` (resolver 3, edition 2024, MSRV 1.92, version 0.1.0) with one member, `crates/harness`.
- **Toolchain:** `harness/rust-toolchain.toml` — `1.99.1`, components `clippy` + `rustfmt`, profile `minimal`.
- **Build and run:** `make harness` runs `cargo run --quiet --locked --package remotecam-harness` from inside `harness/`, so rustup honours the pinned toolchain; GNU `timeout` bounds execution externally.
- **Dependency direction:** the harness consumes `desktop/crates/pairing`, `desktop/crates/session-core` and `desktop/crates/protocol` by relative path. It never reaches into `desktop/crates/app` and never defines the contract, which belongs to `protocol/`.
- **Output:** one JSON line on stdout per run, containing the report version, scenario, observed state, the observed `pair` → `paired` → `offer` → `answer` sequence, per-scenario verdict, an optional failure reason code, and a summary. Exit code 0 means every scenario passed; 1 means at least one failed.

## Harness-specific rules

### 1. Product-owned, not a spike

- The harness is part of the F1 gate (ADR-0007). It has production quality, deterministic output and tests in `make test`.
- It is not shipped to users and has no UI, no installer and no persistence.
- Spike code is provenance only: reusing an F0 implementation requires a new product implementation, not an import (ADR-0003).

### 2. Isolation from `spikes/` (FR-039)

- No path dependency, `include!`, copy-paste module or generated file may come from `spikes/`.
- The harness may not appear in any `spikes/` workspace and no `spikes/` crate may depend on it.
- Shared behaviour with a spike harness is reimplemented against the `protocol/` contract and the Session Core.

### 3. Pinned dependencies and toolchain

- Every dependency is pinned exactly with `=` in `harness/Cargo.toml`; `harness/Cargo.lock` is versioned.
- The toolchain is pinned in `harness/rust-toolchain.toml` and matches `desktop/` (1.99.1).
- A new external dependency needs the constitution's justification (problem, alternatives, maintenance cost, license) recorded in an ADR before it is added. The current baseline is in ADR-0008.

### 4. Determinism and observability

- The clock and the random source are injected; the harness never reads the wall clock or a global RNG.
- The same scenario with the same seed and the same injected clock start produces byte-identical output.
- The report is English-only JSON with no tokens, fingerprints, secrets or personal data.

### 5. Tests and lint

- `make test` runs `cargo test --workspace` from inside `harness/`; `make lint` runs `rustfmt --check` and Clippy with `-D warnings` for the same workspace.
- Tests exercise the public harness API and the binary's exit code and output, not private helpers.
- The harness is not a delivered package, so the 90 % coverage gate stays on `remotecam-protocol`, `remotecam-session-core` and `mobile/protocol`; new harness logic still needs deterministic tests.

### 6. Before modifying

- Before changing the report shape: update the tests, the documented JSON line and any consumer in the same delivery.
- Before adding a dependency: stop and write the ADR entry first.
- Before copying from `spikes/`: stop — quarantined code is not a merge source (ADR-0003).
