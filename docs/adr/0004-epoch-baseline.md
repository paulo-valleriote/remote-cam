# ADR-0004 — Epoch baseline is 1 and resets with the desktop process

- **Status:** accepted
- **Date:** 2026-10-24

## Context

The QR payload carries a 4-byte `epoch` and the contract says it is monotonic
and "always strictly greater than the epoch of the active session". The contract
does not say where the counter starts, whether it survives a desktop restart, or
what a harness should inject. Without a baseline, two implementations can
disagree on the first value and a deterministic harness cannot pin a report.

## Decision

- The first QR issued by a desktop process carries `epoch = 1`.
- Every subsequent QR issued by that process carries the previous value plus one.
  The value is assigned when the QR is created, before it is displayed.
- The counter is not persisted. A desktop restart starts a new baseline at 1,
  consistent with the rule that no session state is persisted.
- `epoch = 0` and negative values are invalid. The field is unsigned, and the
  wire vectors reject the negative case.
- A message whose epoch is lower than the active session's epoch is discarded
  silently and counted as a protocol violation, as the contract requires. That
  comparison is scoped to one listener lifetime; a fresh QR scan establishes the
  new baseline after a restart.
- A harness injects the clock and the RNG but not the epoch sequence: the epoch
  is derived from the process baseline, so the same seed and clock produce the
  same report.

## Consequences

- The F1 harness can assert an exact epoch sequence from a fresh process.
- Restart behaviour is explicit and testable: the second process starts at 1
  again, and the mobile accepts it only through a new QR pairing.
- A future decision to persist the counter would contradict the zero-persistence
  invariant and needs a new ADR.
