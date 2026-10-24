# ADR-0006 — Handshake vectors are a non-wire addition

- **Status:** accepted
- **Date:** 2026-10-24

## Context

F1 needs conformance coverage for handshake sequencing: `pair` must be the first
message, a stale epoch is discarded, a second valid pairing supersedes the
first, and a closed listener rejects further attempts. Those behaviours are
observable through the frozen v1 contract, but they are not single-message
vectors, and adding them to `protocol/vectors/` could look like a contract
change under ADR-0002.

## Decision

- Handshake scenarios are added to
  `protocol/vectors/signaling/scenarios.json` (and its generator
  `protocol/vectors/tools/generate_vectors.py`) as **test data only**.
- A handshake vector may combine existing messages and state, but it may not
  introduce a message type, field, reason code or validation rule that is not
  already in the frozen v1 contract.
- The normative wire artifacts remain `protocol/signaling.schema.json`,
  `protocol/qr-payload.md` and `protocol/reason-codes.json`. Vectors are not
  normative for the wire; they are executable expectations for both
  implementations.
- A scenario that needs new wire vocabulary is rejected from the vector set and
  escalated as a contract decision, not smuggled in as a vector.

## Consequences

- The conformance suite can cover pairing order, epoch staleness and
  supersession without a protocol version bump.
- Both implementations must still agree on every vector; a divergence remains a
  contract break.
- Adding a vector updates the generator and the committed JSON in the same
  change, so the two cannot drift.
