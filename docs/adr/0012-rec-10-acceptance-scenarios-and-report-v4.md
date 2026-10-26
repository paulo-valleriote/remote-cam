# ADR-0012 — REC-10 acceptance scenarios and harness report version 4

- **Status:** accepted
- **Supersedes:** ADR-0010 for the report version and scenario set only
- **Date:** 2026-10-26
- **Applies from:** REC-35
- **Amended by:** ADR-0013 (`single-use-refused` expects `superseded`)

## Context

REC-10 has acceptance criteria for QR expiry, token expiry, single use, replay,
stale epochs, backward clock steps and regeneration after refusal. REC-35 requires
one harness scenario per criterion, end to end, with an injected clock and a
deterministic report. ADR-0010 fixes the report at two scenarios and version 3.

## Decision

- The report advances to version 4. Protocol contract version stays 1; the
  change adds no wire field and no reason code.
- Nine scenarios are reported in a fixed order: the two existing scenarios, then
  `qr-expiry-refused`, `token-expiry-refused`, `single-use-refused`,
  `replay-refused`, `stale-epoch-discarded`, `clock-backwards-window` and
  `regeneration-after-refusal`.
- Two fields are added to each scenario result. `issued_epochs` lists the
  epochs of every QR the Session Core presented, in order; it is omitted when
  empty. `protocol_violations` is set only by `stale-epoch-discarded`.
- `harness/report.schema.json` is updated to match. Scenario contracts (outcome,
  reason code, verdict) stay per-scenario `allOf` definitions.
- A peer that connects while the Session Core is not ready to pair is no longer
  refused with `invalid_transition` before TLS. It completes TLS and presents its
  Pair frame, so the Session Core decides with `token_already_used` for a
  consumed token in any state. Other cases map to `protocol_violation`.
- The stale-epoch scenario exercises `SessionCore::admit_frame_epoch` directly.
  No epoch-tagged media frame path exists in the harness yet.

## Consequences

- Golden fixture, binary and report tests are regenerated for version 4.
- The QR window starts at issuance in the harness and the Session Core window
  starts at `begin_pairing`. Scenarios issue the QR without advancing the
  clock, so the two windows coincide. The drift tracked in REC-34 is not
  exercised.
- REC-32 (listener executes `RejectPeer`) is implemented in commit `b4f461f`;
  `qr-expiry-refused` and `token-expiry-refused` observe it end to end.
