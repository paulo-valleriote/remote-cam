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
- The pairing harness consumes both Session Core and the local protocol crate
  by relative path so it can construct a valid mobile `pair` frame using the
  canonical encoder. This adds no external dependency.
- At scaffold creation, the Session Core had **no external dependencies**
  (standard library only). Later dependencies need their own ADR justification;
  pairing-token primitives are recorded in ADR-0009.

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

### REC-23 loopback endpoint

REC-23 adds direct path dependencies on `remotecam-pairing`,
`remotecam-session-core` and `remotecam-protocol` so the harness can drive the
existing TLS listener and the shared QR/signaling contracts. It also adds two
direct external dependencies to the separate harness workspace:

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `tokio` | `=1.54.1` | Runs the pairing crate's async loopback listener and pinned client end to end | Blocking worker threads (more lifecycle and shutdown code); a fake transport (would not exercise TLS) | Low — same exact version already used by `remotecam-pairing` and justified in ADR-0009 | MIT |
| `zeroize` | `=1.9.1` | Clears temporary copies of the QR pairing token and encoded `pair` frame | Manual volatile writes (easy to omit on error paths) | Low — same exact version already used by `remotecam-session-core` and `remotecam-pairing`, justified in ADR-0009 | Apache-2.0 OR MIT |

No TLS, certificate or hashing dependency is added to the harness workspace;
those remain behind `remotecam-pairing` as decided in ADR-0009.

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
- The original scaffold proved the build path only. F1-02 extends it to run the
  pairing path with injected clock and RNG.
- Nothing under `spikes/` is imported. The harness drives
  `desktop/crates/session-core` and constructs peer frames from
  `desktop/crates/protocol`; the Session Core also consumes the protocol crate.
