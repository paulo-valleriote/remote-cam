# F0 Deviations — Phase 8 Polish

**Recorded:** 2026-10-23
**Closed:** 2026-10-23
**Status:** All five deviations are **approved by the maintainer** at F0 closure (constitution II / FR-039).

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

**Approval:** Approved by Paulo Valleriote on 2026-10-23.

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

**Approval:** Approved by Paulo Valleriote on 2026-10-23.

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

**Approval:** Approved by Paulo Valleriote on 2026-10-23.

---

## D4 — F0 harness implementations live under product crates until replaced

**Status:** closed on 2026-10-24 by [ADR-0003](../adr/0003-f0-code-quarantine.md); the text below records the deviation as approved at F0 closure.

**Deviation:** The F0 implementations remain under
`desktop/crates/{signaling,media-core,injection,audio-bridge,app}` and
`mobile/core` even though they are marked `NON-PRODUCTION`. FR-039 says spike
artefacts must not be merged into product modules, and this code was authored
there for the spikes rather than imported from `spikes/`.

**Justification:** `spikes/f0-handshake`, `spikes/f0-pipeline` and
`spikes/f0-virtual-audio` consume those crates by path. Moving them into
`spikes/` now would break the spike builds and the CI lanes, and the product
phase that owns each crate will replace it anyway. The `NON-PRODUCTION` marker
and the review rule remain in force in the meantime.

**Scope:**
- `desktop/crates/signaling`, `desktop/crates/media-core`,
  `desktop/crates/injection`, `desktop/crates/audio-bridge`,
  `desktop/crates/app`
- `mobile/core` spike classes (`SignalingClient`, `PeerConnectionFactoryProvider`,
  `CameraXCaptureSource`)

**Rollback:** Quarantine each crate/module under `spikes/` when its owning phase
starts, updating the path dependencies and CI lanes in the same delivery.

**Resolution (F1-01, 2026-10-24):** The quarantine landed as the rollback
described. The Rust crates moved to `spikes/f0-signaling`, `spikes/f0-media-core`,
`spikes/f0-injection` and `spikes/f0-audio-bridge`; the DirectShow fallback moved
to `spikes/f0-dshow-vcam-fallback`; `mobile/core` and `mobile/app` moved to
`spikes/f0-mobile-core` and `spikes/f0-mobile-app`. `desktop/crates/app` remains
in the product tree: it carries no `NON-PRODUCTION` marker and owns the
structured-logging implementation, so D4's listing of it is superseded by
ADR-0003. Path dependencies, the Makefile and the READMEs were updated in the
same delivery.

**Approval:** Approved by Paulo Valleriote on 2026-10-23.

---

## D5 — The source technical design remains in Portuguese

**Deviation:** `docs/tdd-remotecam-v1.md` is Portuguese. The constitution
requires documentation in English, and the F0 amendments keep the document's
language so the corrections read in place.

**Justification:** The TDD is the translated source artifact the F0 spec traces
to; rewriting it in place during closure would produce a large, risky diff
unrelated to the phase exit. The amendments index and the gate report are in
English, so the closure evidence is reviewable without the translation.

**Scope:** `docs/tdd-remotecam-v1.md` only.

**Rollback:** Translate the TDD to English before the V1 release (F5), keeping
the amendment blocks and their decision references.

**Approval:** Approved by Paulo Valleriote on 2026-10-23.

---

## Maintainer approval

| Deviations | Approver | Date | Decision |
|------------|----------|------|----------|
| D1, D2, D3, D4, D5 | Paulo Valleriote | 2026-10-23 | Approved |

This record is signed. Any new deviation requires a new row, a justification, a
scope, a rollback path and maintainer approval before merge.
