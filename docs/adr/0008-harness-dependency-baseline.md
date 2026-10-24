# ADR-0008 — Session Core crate and harness dependency baseline

- **Status:** accepted
- **Date:** 2026-10-24
- **Applies from:** F1-02 (walking skeleton)

## Context

ADR-0007 places the product harness in a top-level `harness/` directory, and
REC-18 scaffolds it. Two structural questions came with that scaffold:

1. where the Session Core crate lives, given that the harness drives it but the
   core is product code; and
2. which external dependencies the new `harness/` workspace may carry, since the
   constitution requires every external dependency to be justified (problem
   solved, alternatives discarded, maintenance cost and license) before it is
   added.

The desktop workspace already pins `serde` and `serde_json` and the F0
dependency ledger records their justification. The harness workspace has its own
manifest and its own lockfile, so it needs an explicit decision rather than an
implicit inheritance.

## Decision

### Session Core location

- The Session Core is a crate in the **desktop workspace**:
  `desktop/crates/session-core` (`remotecam-session-core`). It is product code
  with the same review, test and dependency rules as the rest of `desktop/`.
- The harness consumes it by relative path from its own workspace. It is not
  duplicated and it is not vendored.
- The Session Core does not define the wire contract; `protocol/` remains the
  single source of truth. The core consumes the contract when it needs it.
- The scaffold has **no external dependencies** (standard library only). A
  dependency added by a later Session Core task needs its own ADR entry.

### Harness workspace dependencies

The harness workspace pins the toolchain to 1.99.1 and every dependency with `=`.
The baseline added by REC-18:

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `serde` | `=1.0.229` | Derive-based serialisation of the report struct so field names and ordering are defined once, next to the type | Hand-written `format!` calls (escaping and key-order drift); a bespoke serialiser | Low — mature 1.x, exact pin, same version already pinned in `desktop/` | MIT OR Apache-2.0 |
| `serde_json` | `=1.0.154` | Emitting the deterministic JSON report line the harness contract requires (ADR-0007) | Hand-written JSON emission (no escaping guarantees); `simd-json` (no benefit at report size); the unmaintained `json` crate | Low — exact pin, no unsafe parsing, same version already pinned in `desktop/` | MIT OR Apache-2.0 |

Both crates were already vetted for the desktop protocol crate in the F0
dependency ledger (`specs/001-f0-technical-spikes/research.md` §2). REC-18
re-uses the pinned versions rather than introducing new ones; the ledger stays
the historical record and this ADR is the harness-specific decision.

TLS, certificate and runtime dependencies (`rustls`, `rcgen`, `sha2`, `tokio`)
are **not** added here. The tasks that need them (REC-21 and the media
substitute) add them with their own justification, so the scaffold does not
carry anticipatory dependencies.

### Build isolation

- `harness/` is its own Cargo workspace. `make build-desktop` compiles the
  desktop workspace and therefore never compiles the harness.
- `harness/target/` is ignored by Git.
- The Makefile runs harness commands from inside `harness/`, so rustup honours
  `harness/rust-toolchain.toml`; `--manifest-path` from the repository root
  would silently use the default toolchain.
- `make harness` runs the headless binary with `--locked`; `make test` and
  `make lint` cover the harness workspace alongside the others.

## Alternatives considered

- **Harness inside the desktop workspace.** Rejected: `make build-desktop` would
  compile the harness, contradicting the REC-18 acceptance criterion, and the
  harness would inherit desktop-only target configuration.
- **Session Core inside `harness/`.** Rejected: the core is product code driven
  by a non-shipped driver; placing it in the harness would invert the dependency
  direction and make the core look disposable.
- **A duplicate Session Core per consumer.** Rejected: two implementations of
  the same state machine diverge, and the contract already exists once in
  `protocol/`.
- **Harness under `spikes/`.** Rejected by ADR-0007 and FR-039.

## Consequences

- The desktop workspace gains a third crate; the harness workspace stays a
  single member until a second harness crate is justified.
- `make coverage` measures `remotecam-session-core` alongside
  `remotecam-protocol`; the harness stays outside the gate because it is not a
  shipped package.
- Adding a harness dependency now requires an ADR entry, not just a manifest
  edit.
- The scaffold proves the build path only: the binary prints one JSON line and
  exits 0. The full report schema, injected clock and seed arrive with the rest
  of REC-9 (F1-02).
- Nothing under `spikes/` is imported, and the dependency direction is
  `harness → desktop/crates/session-core → protocol/`.
