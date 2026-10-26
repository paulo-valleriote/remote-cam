# ADR-0013 — `SUPERSEDED` is the predecessor's retired state; reuse while a session is active is `superseded`

- **Status:** accepted
- **Date:** 2026-10-26
- **Amends:** ADR-0012 (the `single-use-refused` expectation only)
- **Amended by:** REC-38 (a peer that connects with no pending QR while a session is active is refused as `superseded` before the handshake), REC-46 (a first frame that reuses no consumed token while a session is active is `protocol_violation`)

## Context

ADR-0005 and the glossary describe replacement as one transition to
`SUPERSEDED`. A single `SessionCore` holds one `SessionState`, and the successor
pairing must be able to run while the predecessor is still `CONNECTED` (REC-37,
REC-11 T1). The core therefore cannot rest in `SUPERSEDED` and also report the
successor's `NEGOTIATING` state.

REC-37 also says that the second use of a consumed token, in a race for the
same QR, resolves as `superseded`. ADR-0012 pinned `token_already_used` for
`single-use-refused`, which replays a consumed token while the first session is
still `NEGOTIATING`. Under REC-37 that replay is the race's losing attempt.

## Decision

- `SessionState::Superseded` exists with the label `superseded`. The core never
  rests in it.
- After a valid successor pair, `state()` reports the successor's position
  (`NEGOTIATING`). The predecessor's terminal position is reported by
  `retired_state()`, which returns `Some(SessionState::Superseded)` until the
  retired session is cleared.
- The predecessor is closed through `Effect::SendClose { reason: superseded }`
  followed by `Effect::End`, in that order, before `Effect::SendPaired`.
- A successor `StartPairing` while `CONNECTED` keeps the predecessor
  `CONNECTED`. A pending QR that expires unused leaves it connected.
- A consumed token presented while a session is active (`NEGOTIATING` or
  `CONNECTED`) is refused with `PairingError::Superseded`
  (`ReasonCode::Superseded`). The active session is not changed.
- A consumed token presented with no active session (`IDLE`, `PAIRING_READY`,
  `HANDSHAKING`, `ENDED`) is still refused with `token_already_used`.
- The out-of-order test case `(CONNECTED, StartPairing)` is replaced by
  `(ENDED, PeerConnected)`. `StartPairing` is now valid in `CONNECTED`, so
  the old case no longer tests an invalid transition. (REC-38: it was first
  replaced by `(CONNECTED, PeerConnected)`, which stopped being invalid when
  a peer without a pending QR became a refusal; see below.)
- Harness: `single-use-refused` expects `superseded`. `replay-refused` (the
  token replayed after the first session timed out) stays `token_already_used`.
  Report version stays 4; the golden fixture and `report.schema.json` change
  only the expected reason of `single-use-refused`.

## Consequences

- A reader of `state()` alone never sees `SUPERSEDED`. Adapters that must report
  the predecessor's end use `retired_state()` and the emitted effects.
- `retired_state()` has no consumer outside the tests yet. It stays as the
  adapter API named above (REC-44 decision). Removing it needs a new ADR. A
  harness consumer is added when the diagnostic needs the predecessor's end.
- `token_already_used` is a security event in `protocol/` (`is_security_event`).
  A consumed-token replay during an active session is now `superseded`, a
  user-actionable pairing rejection, so it no longer reaches the security
  section of the local diagnostic. This is a deliberate trade-off that follows
  REC-37.
- Decision (REC-43): the trade-off stands. `superseded` stays
  `security_event: false` and `is_security_event` is unchanged. The replay
  leaves the active session untouched and cannot be told apart from a user who
  scanned the same QR twice, so it is a pairing rejection, not a security
  signal. No counter is added: nothing consumes one today (the security section
  of the diagnostic does not exist yet) and Principle III rejects a metric
  without a declared purpose and consumer. The structured log record that
  carries `reason_code=superseded` is the only trace, and the count stays
  derivable from it. Reopen this when the security section of the diagnostic is
  designed and names a consumer for these attempts.
- The glossary entry for the state machine (`ENDED/SUPERSEDED`) should say that
  `SUPERSEDED` is the predecessor's terminal state, not a resting state of the
  core.
- No wire change (ADR-0002). ADR-0005's decision is unchanged; it only gains an
  Amended by pointer to this ADR.

## Amendment (REC-38)

- `PeerConnected` in `NEGOTIATING`, or in `CONNECTED` with no pending QR, is
  answered with `Effect::RejectPeer { reason: superseded }` and does not change
  the state. The peer is the loser of the race for the active session. With a
  pending QR in `CONNECTED`, `PeerConnected` stays valid.
- The refusal happens before the handshake, so it follows ADR-0011: the socket
  closes without a reason frame and `superseded` is recorded locally only.
- Harness: `single-use-refused` replays the consumed token while a session is
  `NEGOTIATING`. That peer is now refused at the socket, so its
  `message_sequence` is empty instead of `["pair"]`. The expected reason stays
  `superseded`. Report version stays 4; only the golden fixture changes.
- `close { superseded }` to the predecessor is best effort within `SUPERSEDE_BUDGET`
  (250 ms). A predecessor that is gone or does not read never delays the
  successor past that budget; the predecessor is then dropped and the successor
  is paired.

## Amendment (REC-46)

- A first frame that does not reuse a consumed token, presented in
  `NEGOTIATING` or in `CONNECTED` with no pending QR (a token never issued, a
  frame that is not `pair`, a frame that does not parse), is refused with
  `PairingError::ProtocolViolation` (`ReasonCode::ProtocolViolation`). It was
  `token_already_used` before, which claimed a consumption that did not happen.
- The refusal changes no state and does not discard anything: the active
  session, its identity and `retired_state()` are unchanged. Falling through to
  the generic path is not an option, because `refuse_pairing` would end a
  `NEGOTIATING` session.
- `protocol_violation` is not a security event, so this refusal no longer
  reaches the security section of the local diagnostic. Same trade-off as
  REC-43: nothing consumes a counter today, and the structured log record with
  `reason_code=protocol_violation` is the only trace. Reopen when that section
  is designed.
- No wire change (ADR-0002): `protocol_violation` is already in the closed set.
  The harness has no scenario for this case and the report is unchanged.
  The listener already refuses a late peer before the handshake (REC-38),
  so the path is reached only by a peer that connected before the race was
  decided, or by a direct driver of the core.
