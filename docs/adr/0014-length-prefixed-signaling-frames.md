# ADR-0014 — Signaling frames on the pairing channel are length-prefixed

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** ADR-0002 (wire freeze), REC-38

## Context

The desktop must send `close` and `paired` frames over the pairing TLS channel
and read the `pair` frame from the peer. Neither `protocol/` nor the mobile
module defines a delimiter for these frames, and `main` has none. REC-38
introduced newline-terminated frames (commit `e78b23d`) without a record of
that choice.

## Decision

Each signaling frame on the pairing channel is a 4-byte big-endian unsigned
length followed by the JSON payload. A length above
`MAX_SIGNALING_MESSAGE_BYTES` (16 KiB) is refused before any payload byte is
read. The framing layer accepts a zero length; the JSON parser refuses an empty
payload.

## Consequences

- Payloads are binary-safe: no escaping rule is needed beyond the JSON itself.
- The reader allocates each frame at its exact length, so no reallocation
  leaves copies of a token-bearing `pair` frame behind.
- The signaling JSON schema and `protocol/` are unchanged.
- The mobile transport must implement the same prefix when it lands.
