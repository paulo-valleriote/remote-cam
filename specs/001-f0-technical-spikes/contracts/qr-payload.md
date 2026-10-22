# Contract: QR Payload, version 1

**Status**: proposed · **Canonical location after F0**: `protocol/qr-payload.md` · **Consumers**: the desktop encoder, the mobile decoder

The QR payload carries everything the mobile side needs to open an authenticated channel to the desktop: where to connect, which certificate to trust, and the proof that the user scanned a QR this desktop is currently displaying.

It is a **compact binary payload**, not JSON in a URL. The binary form keeps the QR small enough to be read reliably at a distance, and it makes the payload versioned at the byte level rather than at a schema level.

---

## 1. Layout

All multi-byte integers are **big-endian**. All byte strings have the stated fixed length unless a length prefix is given.

| Offset | Size | Field | Description |
| --- | --- | --- | --- |
| 0 | 4 B | `magic` | `0x52 0x43 0x4D 0x31` — the ASCII bytes `RCM1`. |
| 4 | 1 B | `version` | Payload version. `1` for this contract. |
| 5 | 1 B | `flags` | Reserved feature bits. **All bits MUST be 0 in version 1.** A receiver MUST reject a payload with any bit set. |
| 6 | 2 B | `endpoints_len` | Length in bytes of the `endpoints` block. |
| 8 | 16 B | `session_id` | Random session identifier. |
| 24 | 4 B | `epoch` | Monotonic session generation, assigned by the desktop when the QR is created. Always strictly greater than the epoch of the active session. |
| 28 | 8 B | `expires_at` | Absolute expiry, seconds since the Unix epoch. |
| 36 | 32 B | `pairing_token` | 256-bit single-use token. Compared in constant time. |
| 68 | 32 B | `tls_fingerprint` | SHA-256 of the DER-encoded leaf certificate of the ephemeral session certificate. |
| 100 | 2 B | `capabilities` | Bitmask of supported codecs and limits. |
| 102 | variable | `endpoints` | The `endpoints` block, `endpoints_len` bytes. |

**Total minimum size**: 102 bytes + the endpoints block.

### 1.1 Maximum size

The **encoded** payload MUST NOT exceed **512 bytes**. This bound exists so the QR stays large enough to be read reliably from a phone screen at arm's length. The endpoints block is therefore bounded, and a desktop that would exceed the bound MUST reduce the number of advertised endpoints rather than exceed the limit.

### 1.2 Endpoints block

A sequence of endpoint records, each describing one place the mobile can reach the desktop's TLS listener.

| Size | Field | Description |
| --- | --- | --- |
| 1 B | `family` | `4` = IPv4, `6` = IPv6. |
| 16 B | `address` | The address, in network byte order, in the last 4 bytes for IPv4 and in all 16 bytes for IPv6. The remaining bytes MUST be zero. |
| 2 B | `port` | The TCP port of the ephemeral TLS listener. |

**Record size**: 19 bytes. The block therefore holds at most `(512 − 102) / 19 = 21` endpoints in the worst case; in practice the desktop advertises only the addresses on the current private sub-network.

**Address rules**

- Only **private** addresses are permitted: RFC 1918 for IPv4, link-local and unique-local for IPv6.
- Every advertised address MUST belong to the **same sub-network** as the interface the listener is bound to.
- A desktop that has no usable private address MUST NOT display a QR.

---

## 2. Capabilities bitmask

| Bit | Name | Meaning when set |
| --- | --- | --- |
| 0 | `video_h264` | The desktop can decode H.264. |
| 1 | `video_vp8` | The desktop can decode VP8. |
| 2 | `audio_opus` | The desktop can decode Opus. |
| 3 | `supports_1080p30` | The desktop accepts 1920×1080 at 30 frames per second. |
| 4 | `supports_720p30` | The desktop accepts 1280×720 at 30 frames per second. |
| 5 | `supports_480p30` | The desktop accepts 640×480 at 30 frames per second. |
| 6–15 | — | Reserved. **MUST be 0 in version 1.** A receiver MUST reject a payload with a reserved bit set. |

**Codec preference**: the mobile MUST offer the preferred codec first. H.264 is preferred; VP8 is the fallback and is acceptable without an architectural change, at the cost of higher CPU usage on the desktop.

---

## 3. Validation rules

The mobile MUST validate the payload **before** opening any network connection.

| # | Rule | Rejection reason code |
| --- | --- | --- |
| Q-1 | `magic` MUST equal `RCM1`. | `invalid_qr_payload` |
| Q-2 | `version` MUST be a version the mobile supports. A version **greater** than the highest supported MUST be rejected outright; no implicit compatibility is attempted. | `unsupported_qr_version` |
| Q-3 | Every reserved bit in `flags` MUST be 0. | `invalid_qr_payload` |
| Q-4 | Every reserved bit in `capabilities` MUST be 0. | `invalid_qr_payload` |
| Q-5 | The total decoded length MUST equal `102 + endpoints_len` exactly. A trailing byte is an error, not padding. | `invalid_qr_payload` |
| Q-6 | `endpoints_len` MUST be a non-zero multiple of 19. | `invalid_qr_payload` |
| Q-7 | At least one endpoint MUST be present, and every endpoint MUST carry a private address. | `no_private_endpoint` |
| Q-8 | `expires_at` MUST be strictly in the future. | `qr_expired` |
| Q-9 | The mobile MUST read `session_id`, `epoch` and `pairing_token` into memory only. None of them may be written to a log, a file, the clipboard or a shared preference. | — (a rule of behaviour, verified by inspection) |

### 3.1 Constants and their lifetime

| Constant | Lifetime | Rule |
| --- | --- | --- |
| `pairing_token` | The QR's validity window (120 s) | Single use. The desktop invalidates it on first successful use. A second presentation with the same token MUST be rejected. |
| `session_id` | The session | Random per session. Never reused. |
| `epoch` | Monotonic across sessions | Strictly increasing. A message with a lower epoch is discarded silently and counted as a protocol violation. |
| `tls_fingerprint` | The session | The SHA-256 digest of the ephemeral certificate's DER encoding. The mobile MUST compare it **exactly**; a mismatch aborts the connection with no fallback. |

---

## 4. Encoding and decoding

### 4.1 Encoding

The QR **byte mode** is used, not numeric or alphanumeric mode. The payload is arbitrary binary, and the mode MUST be chosen explicitly on both sides so that a decoder never has to guess.

- **Error correction level**: `M` (approximately 15 % recovery). This survives H.264 compression at the target bitrate and keeps the symbol small.
- **Version**: the smallest standard version that fits the payload. Micro QR is NOT usable — it supports numeric and alphanumeric modes only.
- The decoder MUST read the payload as **raw bytes**. A decoder that assumes UTF-8 and returns text will corrupt the payload, because the payload contains arbitrary bytes.

**Endpoint ordering**: endpoints MUST be emitted in a **stable order** (by address family, then by address bytes) so that the same desktop state always produces the same payload, which makes the encoder's output testable against a golden vector.

### 4.2 Decoding

The mobile decodes the QR into a byte array, then applies the validation rules in section 3 in order. **Validation happens before any socket is opened.**

### 4.3 Failure behaviour

| Failure | Behaviour |
| --- | --- |
| The payload is unreadable | The scanner reports a generic "not a RemoteCam code" message and stays on the scanner screen. No network activity. |
| The payload fails validation | The scanner reports the specific reason code mapped to a user-facing message and stays on the scanner screen. No network activity. |
| The payload has expired | The scanner reports that the code has expired and asks the user to scan a fresh one. |
| The desktop's listener is gone | The connection attempt fails; the reason code `desktop_unreachable` is shown. The mobile does **not** retry automatically — a new session requires a new scan. |

---

## 5. Worked example

A version 1 payload for a desktop on `192.168.1.42` with the listener on port 51820, supporting H.264, Opus, 720p30 and 1080p30.

| Offset | Bytes | Value | Meaning |
| --- | --- | --- | --- |
| 0 | `52 43 4D 31` | `RCM1` | Magic |
| 4 | `01` | | Version 1 |
| 5 | `00` | | No flags |
| 6 | `00 13` | 19 | One endpoint record |
| 8 | `…` | 16 bytes | `session_id` |
| 24 | `…` | 4 bytes | `epoch` |
| 28 | `…` | 8 bytes | `expires_at` |
| 36 | `…` | 32 bytes | `pairing_token` |
| 68 | `…` | 32 bytes | `tls_fingerprint` |
| 100 | `1B 00` | `0x001B` | Bits 0, 1, 3, 4 set → H.264, VP8, 1080p30, 720p30 |
| 102 | `04` | | IPv4 |
| 103 | `00 00 00 00 00 00 00 00 00 00 00 00 C0 A8 01 2A` | | `192.168.1.42` |
| 119 | `CA 94` | 51820 | Port |

Total decoded size: **121 bytes**, comfortably inside the 512-byte bound.

---

## 6. Conformance

The following golden vectors MUST exist in `protocol/vectors/qr/` and MUST pass identically in the Kotlin and Rust implementations.

| Vector | What it pins |
| --- | --- |
| `minimal-v1` | A payload with a single IPv4 endpoint and the minimum capabilities. |
| `multi-endpoint-v1` | Several endpoints in the stable order; the encoder MUST produce byte-identical output. |
| `ipv6-v1` | An IPv6 link-local endpoint. |
| `unknown-version` | `version = 2` → rejected with `unsupported_qr_version`. |
| `bad-magic` | Rejected with `invalid_qr_payload`. |
| `reserved-bit-set` | A reserved bit in `flags` or `capabilities` → rejected. |
| `truncated` | A length that does not match `102 + endpoints_len` → rejected. |
| `trailing-byte` | One extra byte → rejected. |
| `public-endpoint` | A public address → rejected with `no_private_endpoint`. |
| `expired` | `expires_at` in the past → rejected with `qr_expired`. |
| `empty-endpoints` | `endpoints_len = 0` → rejected. |
