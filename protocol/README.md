# Shared Protocol Contract — version 1

This directory is the **single source of truth** for the contract shared by the
Rust desktop application and the Kotlin Android application. It contains data
and vectors only: neither application imports the other's source.

| File | Purpose |
| --- | --- |
| `signaling.schema.json` | Closed allowlist of signaling messages. Unknown fields are rejected. |
| `qr-payload.md` | Binary QR payload layout, version 1, with validation rules Q-1…Q-9. |
| `reason-codes.json` | Closed reason-code set with stable spellings, categories and escalation flags. |
| `vectors/qr/` | QR golden vectors: exact bytes plus the encoder input for valid payloads. |
| `vectors/signaling/scenarios.json` | Message and handshake scenarios with the expected accept/reject outcome and reason code. |
| `vectors/signaling/encoding.json` | Canonical frames for the emitting direction, pinned byte for byte, with an encode→parse round-trip. |
| `vectors/reason-codes/all-codes.json` | Pinned enumeration used by both implementations. |
| `vectors/run_conformance.sh` | Runs both implementations against every vector and fails on divergence. |

## Conformance

```bash
make conformance
```

The same vectors are also executed by `cargo test -p remotecam-protocol` and by
the Kotlin `ContractTest`. The dedicated target additionally compares the two
canonical result documents, so an implementation that silently skips a vector
is caught.

## Deviations from the proposed contract

Two defects in `specs/001-f0-technical-spikes/contracts/` were corrected when
the contract became canonical. Both are behaviour-preserving with respect to the
prose and are recorded here so the change is reviewable:

1. **`signaling.schema.json` used `allOf` over the message definitions.** Each
   definition pins a different `const` for `type`, so no message could satisfy
   all of them. The canonical schema uses `oneOf` instead; the definitions
   themselves are unchanged.
2. **The `privateAddress` regex is IPv4-only** while its description and the QR
   contract permit IPv6 link-local and unique-local addresses. Both consumers
   accept IPv4 RFC 1918 and IPv6 `fe80::/10` / `fc00::/7`. APIPA
   `169.254.0.0/16` is rejected: the proposed regex lists it, but it is not
   reachable across a local network.

Adding a reason code or changing the meaning of an existing code requires a
protocol version bump.
