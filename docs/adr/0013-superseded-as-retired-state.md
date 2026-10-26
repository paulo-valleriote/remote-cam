# ADR-0013 — `SUPERSEDED` is the predecessor's retired state, not a resting state

- **Status:** proposed
- **Date:** 2026-10-26

## Context

ADR-0005 and the glossary describe replacement as one transition to
`SUPERSEDED`. A single `SessionCore` holds one `SessionState`, and the successor
pairing must be able to run while the predecessor is still `CONNECTED` (REC-37,
REC-11 T1). The core therefore cannot rest in `SUPERSEDED` and also report the
successor's `NEGOTIATING` state.

The successor's valid pair must emit `SendClose { superseded }` to the
predecessor, then `End`, then `SendPaired` to the successor. That order is what
proves that media never overlaps.

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
- A consumed token presented again stays `token_already_used`
  (`PairingError::TokenAlreadyUsed`). REC-37 asks for `superseded` here, but the
  REC-10 acceptance scenarios `single-use-refused` and `replay-refused` pin
  `token_already_used`. Changing it needs a spec or ADR update first.
- The out-of-order test case `(CONNECTED, StartPairing)` is replaced by
  `(CONNECTED, PeerConnected)`. `StartPairing` is now valid in `CONNECTED`, so
  the old case no longer tests an invalid transition.

## Consequences

- A reader of `state()` alone never sees `SUPERSEDED`. Adapters that must report
  the predecessor's end use `retired_state()` and the emitted effects.
- The glossary entry for the state machine (`ENDED/SUPERSEDED`) should say that
  `SUPERSEDED` is the predecessor's terminal state, not a resting state of the
  core.
- No wire change (ADR-0002). ADR-0005 is unchanged.
