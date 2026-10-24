# ADR-0005 — `superseded` and `listener_closed` mean different things

- **Status:** accepted
- **Date:** 2026-10-24

## Context

The v1 reason-code set contains both `superseded` and `listener_closed`. The
description of `listener_closed` in `protocol/reason-codes.json` says it covers
"the listener was closed after repeated failures, **or the session was
superseded**". That conflates two unrelated events:

- a session being replaced by a newer pairing, which is normal operation and the
  user can act on; and
- the listener refusing further connections after repeated failures, which is a
  security response.

The wire contract already separates them: `close` accepts `superseded` but has
no `listener_closed` code, because a closed listener has no session to send a
message to.

## Decision

- `superseded` is the only code for replacement. The desktop sends
  `close { code: "superseded" }` to the previous device, tears down its state,
  and only then completes the new pairing. It is user-actionable, is not a
  security event, and may appear as a pairing rejection or a session close.
- `listener_closed` is a local pairing rejection only. It is emitted when the
  listener stops accepting connections after repeated failures or throttling.
  It is never sent on the wire, is a security event, and is not user-actionable.
- The two are never used interchangeably, and the descriptive sentence for
  `listener_closed` is corrected to remove the supersession clause. The code
  spelling, category and flags do not change, so this is not a wire change
  under ADR-0002.

## Consequences

- The F1 state machine has one transition for replacement (`SUPERSEDED`) and one
  terminal listener condition, with no ambiguity about which code to report.
- The conformance vectors keep the existing `hs-superseded` scenario and the
  listener-close scenario as separate expectations.
- A future reader of `reason-codes.json` no longer sees a code that claims to
  mean two things.
