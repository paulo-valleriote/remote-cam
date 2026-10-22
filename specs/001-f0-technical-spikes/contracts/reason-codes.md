# Contract: Reason Codes

**Status**: proposed · **Canonical location after F0**: `protocol/reason-codes.json` · **Consumers**: both implementations, the UI layer, the gate report

Every rejection, every session termination and every error carries **exactly one code from a closed set**. There is no free-text reason on the wire and no free-text reason in the diagnostic log for a protocol event.

A code is a **stable identifier**. Its human-readable text lives in the presentation layer and may be reworded at any time. Its **meaning** may not change without a protocol version bump, because the pairing gate's evidence and the security reviewer's findings are written in terms of these codes.

---

## 1. Pairing rejection codes

Emitted when a pairing attempt is refused. The session returns to `IDLE` with one of these codes.

| Code | Meaning | User-actionable | Typical trigger |
| --- | --- | --- | --- |
| `invalid_qr_payload` | The scanned payload failed structural validation. | yes | Bad magic, reserved bits set, length mismatch, trailing byte. |
| `unsupported_qr_version` | The payload version is newer than the highest this build supports. | yes | A newer desktop is pairing with an older phone build. |
| `qr_expired` | The payload's expiry is in the past. | yes | The user took longer than the validity window. |
| `no_private_endpoint` | The payload advertises no reachable private address. | yes | The desktop is not on a private network, or every advertised address is public. |
| `desktop_unreachable` | The advertised endpoint could not be reached. | yes | The listener was closed, or the firewall blocked the connection. |
| `tls_handshake_failed` | The TLS 1.3 handshake did not complete. | no | Protocol or network fault; the listener was still up. |
| `certificate_pin_mismatch` | The presented certificate's fingerprint did not match the pinned value. | no | **A potential interception attempt.** Escalated to the security event log. |
| `certificate_chain_rejected` | The peer presented a chain-only trust attempt instead of the exact pinned certificate. | no | A peer that tries to be trusted by a public CA. Rejected without fallback. |
| `tls_version_unsupported` | The peer offered a TLS version other than 1.3, or attempted a downgrade. | no | **A downgrade attempt.** Escalated to the security event log. |
| `token_invalid` | The pairing token did not match. | no | A wrong or forged token. |
| `token_expired` | The pairing token's window had closed. | no | A late scan. |
| `token_already_used` | The pairing token had already been consumed. | no | **A replay attempt.** Escalated to the security event log. |
| `source_address_not_private` | The connection came from an address outside the permitted private ranges. | no | A connection attempt from outside the local network. Escalated. |
| `source_subnet_mismatch` | The connection came from an address outside the announced sub-network. | no | A connection attempt from an unexpected host. Escalated. |
| `rate_limited` | Too many attempts from this origin. | yes | The per-origin attempt limit of 5 attempts per minute was exceeded. Escalated to the local diagnostic. |
| `listener_closed` | The listener was closed after repeated failures, or the session was superseded. | no | Repeated failures trigger a progressive lockout and then closure. Recorded as a security event. |
| `superseded` | A newer pairing replaced this one. | yes | Another phone scanned a newer QR. The previous session is terminated. |
| `protocol_violation` | A message violated the contract — an unknown field, a stale epoch, an oversized frame. | no | Counted; a low-epoch message is discarded silently and counted. |
| `internal_error` | An unexpected internal fault. | yes | A defect. Reported in the local diagnostic, never to a remote service. |

### 1.1 Escalation rule

These codes are **security events** and MUST be written to the security section of the local diagnostic, and MUST always be visible in the diagnostic panel:

`certificate_pin_mismatch` · `certificate_chain_rejected` · `tls_version_unsupported` · `token_already_used` · `source_address_not_private` · `source_subnet_mismatch` · `listener_closed`

`rate_limited` is a **throttling decision**, not an escalation: it is recorded in the throttling evidence with its reason code and outcome, and is visible in the diagnostic panel, but it is not by itself a security event.

### 1.2 Rules

1. **No fallback.** A rejection MUST NOT be followed by an attempt to connect without authentication, with a different transport, or with a relaxed verification setting. The absence of any fallback path is itself a test assertion.
2. **No leakage.** The error message text MUST NOT contain host data, file paths, user names, public IP addresses or the internal state of the peer.
3. **Constant-time comparison.** The token is compared in constant time. A timing difference is a defect, not an optimisation opportunity.
4. **Attempt limits.** At most **5 attempts per minute** per origin, with progressive blocking, and the listener closes after **10 consecutive failures**.
5. **One code per rejection.** If several conditions fail simultaneously, the **first** applicable code in the validation order is reported, so that the outcome is deterministic and testable.

### 1.3 Throttling rules (proved by the pairing gate)

The attempt limit is a **property of the implementation**, not a configuration note, and the pairing gate must prove it (FR-049).

| Rule | Requirement |
| --- | --- |
| **Per origin** | The limit is scoped to the origin address. A **global** limit is a defect: it turns one hostile origin into a denial of service against the legitimate user. |
| **Limit value** | At most **5 attempts per minute** per origin. The **6th** attempt within the window is refused with `rate_limited`. |
| **Progressive blocking** | Each further failure extends the block for that origin. The block duration MUST increase across successive failures. |
| **Listener shutdown** | After **10 consecutive failures** the pairing listener is closed with `listener_closed`. This is recorded as a **security event**, not as a product failure. |
| **Legitimate pairing unaffected** | While one origin is being throttled, a legitimate pairing from a **second** origin MUST still succeed. A flood MUST NOT deny service to the legitimate user. |
| **No session from a throttled origin** | A throttled or blocked origin MUST NOT establish a session. |
| **Recorded decisions** | Every throttling decision MUST be recorded with its reason code and its observed outcome. The evidence is `pairing_throttling.json` (see [`measurement-formats.md`](./measurement-formats.md) §8.1). |

---

## 2. Session close codes

Emitted when an established session terminates. This set is carried by the `close` signaling message.

| Code | Meaning | User-actionable |
| --- | --- | --- |
| `ended_by_mobile` | The user ended the session on the phone. | yes |
| `ended_by_desktop` | The user ended the session on the desktop. | yes |
| `superseded` | A newer pairing invalidated this session. | yes |
| `timeout` | A session timeout elapsed — handshake, ICE or first frames. | yes |
| `media_failure` | The media pipeline failed irrecoverably, including an audio/video drift beyond the hard limit. | yes |
| `security_violation` | A security invariant was violated — for example, media traffic on a channel that is not protected by DTLS-SRTP, or an outbound media packet originating from the desktop. | no |
| `internal_error` | An unexpected internal fault. | yes |

### 2.1 Rules

1. The desktop **MUST NOT** send media. A single outbound media RTP packet from the desktop is a `security_violation` and terminates the session immediately. RTCP — sender reports, receiver reports, NACK, PLI — is control traffic and is permitted.
2. The mobile **MUST NOT** capture playback audio. A violation is a `security_violation`.
3. A `close` message MUST NOT carry a media payload, a file path, a device name, a screen, clipboard content or any system information beyond the code.

---

## 3. Signaling error codes

Emitted for a malformed or unsupported signaling message that does not terminate the session.

| Code | Meaning | User-actionable |
| --- | --- | --- |
| `invalid_message` | The message did not satisfy the schema — an unknown field, a missing required field, a wrong type, or an oversized frame. | no |
| `unsupported_version` | The message carried a protocol version this peer does not support. | no |
| `rate_limited` | The peer exceeded the message rate limit. | no |
| `internal_error` | An unexpected internal fault while processing the message. | yes |

The `error` message MAY carry a short generic `message` of at most 200 characters for the user-facing text. It MUST NOT contain technical detail.

---

## 4. Gate-report codes

The `no_go` and `conditional` verdicts use a separate, report-level vocabulary. This is not on the wire; it exists so that a verdict can be aggregated across the five gates.

| Code | Meaning |
| --- | --- |
| `criterion_not_met` | A measured value missed its threshold. The measured value is stated. |
| `harness_invalid` | The measurement apparatus failed its own self-test, so the result is not usable. |
| `precondition_failed` | A gate precondition was not satisfied — for example, hardware decode was not available. |
| `dependency_failed` | A gate this one depends on received `no_go`. The blocking dependency is named. |
| `environment_out_of_reference` | The run was outside the reference environment and cannot be used to pass a gate. |
| `evidence_missing` | The required evidence was not produced. |
| `evidence_incomplete` | The evidence exists but does not cover the criterion. |
| `reproduction_failed` | A second engineer could not reproduce the result within one working day. |
| `decision_unresolved` | A required decision (for example, licensing) is not closed on the gate date. |
| `scope_reduced` | The gate is deliberately reduced in scope with an approved plan; the reduction and its consequences are stated. |

---

## 5. Conformance

| Vector set | What it pins |
| --- | --- |
| `protocol/vectors/reason-codes/` | Every code in sections 1–3 appears exactly once in the enumeration, with a stable spelling. A typo in a code string is a protocol break, so the enumeration is pinned byte for byte. |
| `protocol/vectors/signaling/` | For every rejection scenario in the pairing gate, the expected code is pinned. Both implementations MUST produce the same code for the same input. |

**Adding a code** requires a protocol version bump and a corresponding entry in the UI layer's message table. **Changing the meaning of an existing code** requires a protocol version bump and a note in the gate report.
