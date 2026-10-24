# ADR-0007 — Product harness location and status

- **Status:** accepted
- **Date:** 2026-10-24
- **Applies from:** F1-02 (walking skeleton)

## Context

Two kinds of executable driver now exist:

- the F0 **spike harnesses** under `spikes/`, which are disposable, evidence
  oriented and exempt from the coverage gate (ADR-0003); and
- the **product harness** F1 needs, which drives the session core through the
  happy path and emits a deterministic report on Linux/CI.

The F0 harness cannot become the product harness: it is non-production, it
depends on quarantined code, and FR-039 forbids merging it into the product.
Keeping the product harness under `spikes/` would also make it look disposable
when it is part of the F1 gate.

## Decision

- The product harness lives in a top-level `harness/` directory, not under
  `spikes/`. It has its own `AGENTS.md` and its own Makefile target.
- It is product-owned: production code quality, deterministic output, tests in
  the normal suite, and the same review rules as `desktop/` and `mobile/`.
- It drives the Session Core through the real contract interfaces with an
  injected clock and RNG, and emits a machine-readable report containing the
  contract version, commit, seed, injected clock start, environment, per-scenario
  verdict and reason code, and a summary.
- It is headless and runs on Linux/CI. It is not shipped to users.
- `spikes/` keeps the quarantined F0 harnesses and their evidence. Nothing in
  `spikes/` is imported by `harness/`; shared behaviour is reimplemented against
  the product contract.

## Consequences

- The two driver classes have different rules, locations and Makefile targets,
  and the difference is visible in the repository layout.
- REC-9 creates `harness/` and its `AGENTS.md`; until then the directory does
  not exist and no target claims to run it.
- The F1 exit gate is judged on the product harness, not on the F0 spike
  evidence.
