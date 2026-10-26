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
- **Workspace:** `harness/Cargo.toml` (resolver 3, edition 2024, MSRV 1.92, version 0.1.0) with two members: `crates/harness` and `crates/residue`.
- **Toolchain:** `harness/rust-toolchain.toml` — `1.99.1`, components `clippy` + `rustfmt`, profile `minimal`.
- **Build and run:** `make harness` runs `cargo run --quiet --locked --package remotecam-harness` from inside `harness/`, so rustup honours the pinned toolchain; GNU `timeout` bounds execution externally. `make harness HARNESS_ARGS="--seed 42 --clock-start-millis 100"` overrides the deterministic defaults.
- **Dependency direction:** the harness consumes `desktop/crates/pairing`, `desktop/crates/session-core` and `desktop/crates/protocol` by relative path. It never reaches into `desktop/crates/app` and never defines the contract, which belongs to `protocol/`.
- **Output:** one JSON line on stdout per run, containing report version 4, protocol contract version 1, commit, seed, logical clock start, an allowlisted environment, the ordered nine scenario results (`pairing-happy-path`, `pin-mismatch-aborts`, then the seven REC-10 acceptance scenarios), diagnostic state/message/frame/resource/epoch observations, per-scenario verdict and reason code, and a summary. `harness/report.schema.json` is the report contract. Exit code 0 means all scenarios passed; 1 means at least one failed. The REC-10 scenarios are defined in `src/acceptance.rs` and recorded in ADR-0012.

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
- Runs with the same commit, seed and logical clock start produce byte-identical output except for the allowlisted `environment` block.
- The report metadata reads the current commit with `git rev-parse HEAD` and the compiler version with `rustc --version`; failure to obtain either is explicit and produces no report.
- The report is English-only JSON with no tokens, fingerprints, secrets or personal data.

### 5. Tests and lint

- `make test` runs `cargo test --workspace` from inside `harness/`; `make lint` runs `rustfmt --check` and Clippy with `-D warnings` for the same workspace.
- Integration tests exercise the public harness API and the binary's exit code and output. Unit tests may cover private helpers only when that behavior cannot be exercised through a public surface.
- The harness is not shipped, but REC-25 requires ≥90% line coverage for `remotecam-harness`; `make coverage` enforces it alongside the delivered-package thresholds.

### 6. Residue inspection (`crates/residue`)

- `make residue` is the single command of the zero-persistence method (REC-15, ADR-0017) and writes `docs/evidence/f1/rec-15-zero-persistence-linux.json`. It spawns itself as a probe in a sandboxed `HOME`/`XDG_*`/`TMPDIR`, then sweeps memory, state and credential stores at `live-control`, `session-end`, `process-exit` and `restart`.
- The controls are part of the verdict: `live-control` must find the secrets, the credential-store sweep must find a planted secret, the canary leak must be detected, and no memory region or store may be unreadable. A green run without them is a broken method. The TLS ticket scope is `swept: false` with its unit test as `basis`.
- The method is the one C-02 requires on the reference devices; the Linux evidence does not close that device run.
- The binary installs `remotecam_scrub::ZeroizingAllocator`; it is outside the REC-25 coverage gate (`remotecam-harness` only).

### 7. Before modifying

- Before changing the report shape: update the tests, the documented JSON line and any consumer in the same delivery.
- Before adding a dependency: stop and write the ADR entry first.
- Before copying from `spikes/`: stop — quarantined code is not a merge source (ADR-0003).
