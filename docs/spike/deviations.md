# F0 Deviations — Phase 8 Polish

**Recorded:** 2026-10-23
**Gate:** Phase review on or before 2026-11-06
**Status:** All three deviations require **written maintainer approval before merge** (constitution II / FR-039).

These are deliberate, recorded deviations from the standard project rules. Each
carries a justification, a defined scope, and a rollback path. None of them
may be silently extended; any expansion requires a new deviation record and
maintainer approval.

---

## D1 — Spike coverage exemption

**Deviation:** `spikes/` is exempt from the ≥ 90 % coverage gate that applies to
`desktop/crates/protocol` and `mobile/protocol`.

**Justification:** Spike quality is measured by documented procedure and
evidence (`spikes/evidence/`), not by unit test coverage. The spike harnesses
are disposable F0 risk-reduction artefacts (FR-039) and are never merged into
product modules. The gate exists to protect production code quality; spikes
are explicitly non-production.

**Scope:** All files under `spikes/`, including `spikes/tools/`, `spikes/f0-*`,
`spikes/fixtures/`, and `spikes/evidence/`.

**Rollback:** Remove `spikes/` from the coverage exemption and apply `cargo-llvm-cov`
(≥ 90 %) to `desktop/crates/protocol` and JaCoCo (≥ 90 %) to `mobile/protocol`
only; `spikes/` remains unmeasured.

**Approval:** Written maintainer approval required before merge.

---

## D2 — Custom jitter buffer and custom Media Foundation media source

**Deviation:** The desktop receive pipeline implements a custom adaptive jitter
buffer (`desktop/crates/media-core/src/jitter_policy.rs`) instead of adopting an
off-the-shelf NetEq-style buffer, and implements a custom Media Foundation
virtual camera media source (`desktop/crates/injection/src/mf_virtual_camera.rs`)
instead of using an existing capture filter.

**Justification:** No off-the-shelf buffer provides the FR-015 fast recovery to
baseline latency within 2 s after a 5 s stall (research.md §4 finding 1). The
Media Foundation virtual camera API (`MFCreateVirtualCamera`) is the preferred
per-user registration path (D-10), and Microsoft classifies DirectShow as
legacy, advising against new code on it (research.md §4 finding 1).

**Scope:**
- `desktop/crates/media-core/src/jitter_policy.rs` + tests
- `desktop/crates/injection/src/mf_virtual_camera.rs` + `registration.rs`
- `desktop/crates/injection/src/ring_buffer.rs` + tests

**Rollback:** Revert to a standard jitter buffer implementation and/or a
DirectShow-based capture filter; the fallback DirectShow filter lives in
`desktop/vendor/dshow-vcam-fallback/` and is only built when a primary backend
matrix cell fails.

**Approval:** Written maintainer approval required before merge.

---

## D3 — New first-level directories `protocol/` and `spikes/`

**Deviation:** The repository introduces two new first-level directories,
`protocol/` (shared, versioned, language-neutral contracts) and `spikes/`
(non-production F0 harnesses and evidence), which did not exist in the
pre-F0 repository layout.

**Justification:** `protocol/` is the source of truth for the versioned v1
contract (`signaling.schema.json`, `reason-codes.json`, QR payload format),
consumed by both `desktop/` and `mobile/` product crates/modules. `spikes/`
isolates disposable F0 measurement harnesses so they cannot be accidentally
merged into product code (FR-039).

**Scope:**
- `protocol/` — `signaling.schema.json`, `reason-codes.json`, `qr-payload.md`,
  `vectors/`, `run_conformance.sh`
- `spikes/` — all F0 spike harnesses, fixtures, tools, evidence, and procedures

**Rollback:** Remove `protocol/` and `spikes/` and inline the contract into the
product crates; this would break the shared-contract requirement and is not
recommended.

**Approval:** Written maintainer approval required before merge.

---

## Maintainer approval

| Deviations | Approver | Date | Decision |
|------------|----------|------|----------|
| D1, D2, D3 | — | — | Pending |

This record must be signed by a maintainer before any F0 artefact is merged
into `main`. Until signed, all three deviations remain **proposed**, not
**approved**.
