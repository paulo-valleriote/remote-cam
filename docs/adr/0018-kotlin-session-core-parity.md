# ADR-0018 — The Kotlin session core is a pure-JVM mirror, compared by handshake scenarios

- **Status:** accepted
- **Date:** 2026-10-26

## Context

REC-14 requires the Rust and Kotlin implementations to be measured against the
same handshake scenarios, not only the same single-message vectors. Until now
`kind = handshake` scenarios in `protocol/vectors/signaling/scenarios.json`
carried only an outcome and a reason code, were ignored by both conformance
runners, and `compare_conformance.py` never looked at them, so a decision that
diverged between the cores (state, transition, epoch, reason code) could only
appear in the field. The Kotlin side also had no session core, and `:protocol`
was an Android library that a pure-JVM module cannot depend on.

## Decision

- `mobile/session-core` (`:session-core`) is a pure-JVM Kotlin module with no
  Android and no WebRTC dependency. It mirrors the Rust `SessionCore`:
  states, transitions, the epoch baseline of ADR-0004, first-frame
  authentication, signaling admission (ADR-0015), and the closed reason codes.
  Clock and randomness are injected; `ManualClock` and `SeededRandomSource`
  produce the same stream as the Rust `testing` module.
- `:protocol` becomes a pure-JVM Kotlin module too. It never used Android APIs,
  and a JVM library is consumable by the F2 Android application. The build task
  names change (`:protocol:test`, `jacocoTestReport` under `reports/jacoco/test`).
- Every handshake scenario declares a `layer`:
  - `core`: carries `steps`, replayed on an injected clock by both cores, and
    `expected_state`, `expected_transition` (`from->to` of the last step),
    `expected_epoch`, `expected_protocol_violations`, besides the existing
    `expected_outcome` and `expected_reason_code`.
  - `qr`: names a `qr_vector`; both implementations decode it and must report
    the pinned reason code.
  - `transport`: decided below the session core (TLS pinning, source address,
    throttle, listener). The cores report it with `executed = false`; it is
    exercised by the `pairing` crate tests.
- A scenario with an unknown layer, an unknown step, a missing layer or an
  observed outcome that differs from its expectation fails the run of that
  implementation. `compare_conformance.py` additionally fails when the two
  documents differ in any field, and when any id declared in the vector files
  (QR, message, encoding, handshake) is absent from either report. Nothing can
  be skipped silently.
- The Kotlin conformance also runs `signaling/encoding.json`, so the emitting
  direction is compared as well.
- The coverage gate requires at least 90 % line coverage for both Kotlin
  modules.

## Consequences

- Handshake scenarios stay test data (ADR-0006): no message, field, reason code
  or validation rule was added to the frozen v1 contract (ADR-0002).
- Transport scenarios remain Rust-only by nature; their reporting as not
  executed is explicit and compared, rather than implied by omission.
- Adding a scenario means updating `generate_vectors.py` and the committed JSON
  together; both cores then have to agree on it.
