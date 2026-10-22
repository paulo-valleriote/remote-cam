# Structured Log Schema — desktop and mobile

**Status**: canonical for F0 · **Consumers**: `desktop/crates/app/src/logging.rs`, `mobile/core/src/main/kotlin/remotecam/core/logging/StructuredLog.kt`, the diagnostic panel

Every log line is one JSON object, in English, with the same field names on both platforms. The schema exists so that a failure can be diagnosed from the recorded evidence alone, and so that secrets cannot leak into a log by accident.

## Fields

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `timestamp` | string | yes | ISO 8601 / RFC 3339 in UTC, e.g. `2026-11-03T09:14:22Z`. |
| `severity` | string | yes | One of `debug`, `info`, `warning`, `error`. |
| `operation` | string | yes | Stable English identifier matching `^[a-z][a-z0-9_.]{0,63}$`, e.g. `pairing.attempt`. Never a sentence. |
| `correlation_id` | string | yes | 32 lowercase hex characters identifying one operation across components. Never a pairing token, session key or fingerprint. |
| `outcome` | string | yes | One of `started`, `success`, `rejected`, `failed`, `stopped`. |
| `reason_code` | string | no | A code from `protocol/reason-codes.json`. No free-text reason is permitted for a protocol event. |
| `detail` | string | no | Short English diagnostic text, at most 200 characters after redaction. |

Additional fields are not permitted. If a new field is needed, this schema is updated first.

## Redaction rules

The logger applies these rules before a line is written, and a test proves each one:

1. **Tokens and fingerprints.** Any run of 32 or more hexadecimal characters — a 256-bit token, a SHA-256 fingerprint, a session key — is replaced with `[redacted]`.
2. **Paths.** Absolute user paths (`/home/<name>/…`, `/Users/<name>/…`, `C:\Users\<name>\…`) are replaced with `<path>`. Artefacts use repository-relative paths only.
3. **Personal data.** User names, e-mail addresses, device serial numbers and real media file names are never fields of this schema. The logger does not accept arbitrary key/value blobs.
4. **Control characters.** Control characters (including newlines) are replaced with a space so one record is always one line.
5. **No unauthenticated fallback.** A rejection is logged with its reason code; the log never records a connection detail that could be replayed.

## Examples

```json
{"timestamp":"2026-11-03T09:14:22Z","severity":"info","operation":"pairing.attempt","correlation_id":"00112233445566778899aabbccddeeff","outcome":"rejected","reason_code":"certificate_pin_mismatch","detail":"presented certificate [redacted]"}
```

```json
{"timestamp":"2026-11-03T09:15:01Z","severity":"info","operation":"session.close","correlation_id":"ffeeddccbbaa99887766554433221100","outcome":"stopped","reason_code":"ended_by_mobile"}
```

## What is never logged

- A pairing token, session key, private key or certificate body.
- A full certificate fingerprint. The security section may show the first eight
  hex characters when identifying which certificate was presented.
- An absolute path, user name, public IP address or device serial number.
- Audio, video, transcripts or any real user media.
- A free-text reason for a protocol rejection.
