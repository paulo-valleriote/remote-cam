# ADR-0015 — Signaling refusals follow one validation order and carry one code

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** ADR-0002 (wire freeze), ADR-0013, REC-12

## Context

`specs/001-f0-technical-spikes/contracts/reason-codes.md` requires that when
several conditions fail at once, the first applicable code in the validation
order is reported. No document declared that order, and three places disagreed
about an oversized frame: the schema invariant closes the session with
`security_violation`, the parser and framing returned `invalid_message`, and
the reason-code notes listed it under `protocol_violation`.

## Decision

An inbound frame on an established session is validated in this order, and the
first failing step is the only code reported:

1. **Size.** More than 16 KiB is `security_violation`. The Session Core emits
   `close { security_violation }`, then `End`, and the session is `ENDED`. The
   pairing framing layer refuses an oversized length prefix with the same code
   before reading any payload byte.
2. **Strict parse.** Malformed JSON, a missing or mistyped field, an unknown
   field, uppercase hex, an unknown type or a value outside its range is
   `invalid_message`; a version above 1 is `unsupported_version`.
3. **Contract policy of the SDP and candidates**, applied inside the parse:
   an `offer` may contain only `audio` and `video` sections whose direction is
   `sendonly` or `inactive`; an `answer` only `recvonly` or `inactive`. A
   missing direction attribute is `sendrecv` (RFC 3264) and is refused. A
   `a=candidate` line must be UDP, `typ host` and on a private address (APIPA
   excluded). Any violation is `invalid_message` and the session continues.
4. **Direction.** `paired`, `answer` and `error` received by the desktop are
   `invalid_message`. A `pair` after authentication is `protocol_violation`.

Before the first `pair` frame the same size rule applies and yields
`security_violation`; a first frame that is not a valid `pair` stays
`protocol_violation` (ADR-0013).

`OfferSdp` and `AnswerSdp` replace the single `Sdp` type so the policy is a
property of the typed value: the desktop cannot construct an answer with a send
track.

## Consequences

- No wire change: the schema, message set and reason-code set are unchanged
  (ADR-0002). `parse_signaling` alone still returns `invalid_message` for an
  oversized input; the size decision belongs to the Session Core.
- The canonical `offer` vector declares `a=sendonly`.
- The Kotlin parser does not enforce the SDP policy yet; parity is REC-14.
- A candidate with an mDNS `.local` address is refused; the contract is
  host-only on literal private addresses.
