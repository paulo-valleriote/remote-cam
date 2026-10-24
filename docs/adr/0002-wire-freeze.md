# ADR-0002 — Protocol version 1 is frozen for F1

- **Status:** accepted
- **Date:** 2026-10-24

## Context

F1 implements the session core against the contract delivered in F0. The
contract is already version 1 and is implemented twice, in Rust and Kotlin,
with shared conformance vectors. Changing message shapes, QR fields or reason
codes while F1 is building the state machine would move the target under both
implementations and make a conformance failure ambiguous: is the implementation
wrong, or did the contract change?

## Decision

Protocol version 1 is frozen for the duration of F1:

- No message type, field, required key or `additionalProperties` rule changes in
  `protocol/signaling.schema.json`.
- No QR layout, capability bit or validation rule changes in
  `protocol/qr-payload.md`.
- No reason code is added, removed, renamed or re-categorised in
  `protocol/reason-codes.json`.
- F1 work is behaviour: the state machine, epoch handling, supersession,
  validation and the harness. It consumes the frozen contract; it does not
  extend it.
- Additions to `protocol/vectors/` are test data, not wire changes (see
  ADR-0006). A scenario that cannot be expressed with the frozen contract is
  rejected rather than added to the contract.

A wire change requires an explicit version decision outside F1, a new ADR, and
a matching update to both implementations and every vector.

## Consequences

- F1 cannot "fix" a contract gap by adding a field. Gaps are recorded as issues
  and, if they matter, as a version-2 decision after F1.
- ADR-0005 clarifies the meaning of two existing reason codes without changing
  their wire spellings or categories.
- The conformance suite is the guardrail: both implementations must agree with
  the frozen vectors before F1 can pass its gate.
