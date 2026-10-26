# ADR-0011 — Pre-handshake refusals close the socket without a reason frame

- **Status:** accepted
- **Date:** 2026-10-26
- **Clarified by:** REC-38 (the loser of a race for the active session follows the same rule)

## Context

REC-10 requires a QR presented after its 120 s window to be refused with
`qr_expired` before any handshake. The protocol defines the reason code
`qr_expired` and a conformance vector (`hs-qr-expired`), but it defines no
frame that a desktop can send to a peer before TLS completes. Protocol v1 is
frozen for F1 (ADR-0002), so adding such a frame is a wire change.

## Decision

- A connection whose QR window has closed is refused before TLS starts. The
  desktop closes the TCP socket without sending a reason frame.
- The reason (`qr_expired`) is recorded in the local log only. It is not sent
  to the peer.
- The listener checks the window in `accept()` before it begins the TLS
  handshake, so an expired QR costs no handshake.
- The acceptance criteria of REC-32 are written to match this behaviour: the
  peer observes a closed socket, not a reason code.

## Consequences

- No change to the wire, the reason-code set or the conformance vectors.
- The mobile cannot tell an expired QR from a network failure at the socket
  level. It must ask the user to scan a new QR in either case.
- A future decision to report pre-handshake reasons on the wire needs a new
  ADR and a protocol change that the freeze must allow.

## Clarification (REC-38)

The rule covers every refusal decided before the handshake, not only
`qr_expired`. A **Race Loser** that connects after the race is decided is refused
with `superseded` at the socket: the desktop closes it without a reason frame
and records `superseded` locally. A Race Loser that already completed the
handshake has a channel, and the existing `close` frame carries `superseded`
to it. This needs no new wire frame, so it does not conflict with this ADR or
with ADR-0002.

`listener_closed` follows the same rule: it is a pre-handshake refusal, local
only, and never reaches the wire (ADR-0005).
