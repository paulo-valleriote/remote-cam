# ADR-0016 — Pairing attempts are throttled per origin; the listener lifecycle stays listener-wide

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** ADR-0005, ADR-0011, REC-13, REC-47

## Context

REC-13 requires attempts limited per origin, progressive blocks, and a listener
that closes after 10 consecutive failed handshakes. It also requires that
throttle state is never global, because a global limit lets one origin deny
service to every other.

The 10-failure close (REC-47) is a property of the listener, not of an origin:
once closed, nobody is admitted. That is the security response the ticket asks
for, so the failure counter cannot be made per origin without removing the
rule.

## Decision

- **Origin key.** The source IP address. IPv4-mapped IPv6 addresses collapse to
  their IPv4 address and other IPv6 addresses collapse to their /64 prefix, so
  rotating addresses inside one prefix does not buy a fresh budget.
- **Limit.** 5 attempts in a sliding 60 s window; the sixth is refused with
  `rate_limited`. Attempts are counted at TCP accept, before TLS, so a refused
  origin costs no handshake.
- **Blocks.** The sixth attempt starts a block of 60 s. Each further block
  doubles, up to 15 min. An origin quiet for 30 min starts over.
- **Bounded state.** At most 4096 origins are tracked. A full table first drops
  quiet origins, then the least recently seen unblocked origin; blocked origins
  are evicted last.
- **Refusals are local.** `rate_limited` and `listener_closed` close the socket
  without a frame (ADR-0011). Rate-limited refusals never count as failed
  handshakes.
- **Listener lifecycle.** The consecutive-failure counter stays listener-wide
  and a success resets it. The per-origin limit bounds how fast one origin can
  feed it: at most 5 handshakes per window, so reaching 10 takes several blocks
  and a legitimate success in between resets the count. A determined LAN
  attacker can still close the listener; that is the accepted trade-off of the
  close-on-repeated-failures rule, and the user recovers by creating a new
  pairing.
- **Diagnostics.** The listener keeps a bounded in-memory log of
  `DiagnosticEntry` values (closed reason code, origin, block length; never a
  token or a fingerprint). It counts the entries it had to drop. The desktop
  shell turns each entry into a structured log line with the address masked to
  its network part.

## Consequences

- Policy numbers live in `ThrottlePolicy`; tests lower or lift them without a
  second code path.
- `PairingListener::handshake` accepts only an `AdmittedSocket`, which
  `accept_socket` returns after the throttle, so the limit cannot be bypassed.
- A legitimate origin that retries more than 5 times a minute is blocked for a
  minute; the mobile treats `rate_limited` like any closed socket.
