# protocol/ — AGENTS.md

> Protocol-specific rules. Implemented independently by `desktop/crates/protocol` (Rust) and `mobile/protocol` (Kotlin). Do NOT apply to `desktop/`, `mobile/`, or `spikes/`.

**Quick start:**
- **Scope:** the shared, versioned contract (v1 only).
- **Hard rules:**
  - `version > 1` is rejected.
  - Unknown fields are rejected (`INVALID_MESSAGE`), not ignored.
  - Hex is lowercase only (`0123456789abcdef`); uppercase is rejected.
  - Endpoints are ordered by `(family, hex(address))`.
  - Reason codes are closed (see `reason-codes.json`).
  - `MAX_MESSAGE_BYTES = 16 KiB`.
  - Emitted frames are canonical (`version`, `type`, then schema order) and never above 16 KiB.
- **Before editing:** update `signaling.schema.json` / `reason-codes.json` and `specs/001-f0-technical-spikes/research.md`; run `make spike-interop` / `run_conformance.sh`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/contracts/signaling-messages.schema.json`, `.agents/skills/`.

## Context and architecture

- **Shared, versioned contract:** v1. `version > 1` is rejected by strict parsing in Kotlin (`UNSUPPORTED_VERSION`) and by Rust logic (similar).
- **Implemented independently by both:** `desktop/crates/protocol` (Rust, full tests) and `mobile/protocol` (Kotlin, JUnit 5, ≥90% coverage).
- **Conformance:** `bin/conformance.rs` (Rust) + `tests/ContractTest` (Kotlin) run vectors in `vectors/` via `run_conformance.sh` / `compare_conformance.py`.

## Protocol-specific rules

### 1. Active contracts

| File | Type | Size limit | Restrictions |
|------|------|-----------|-------------|
| `signaling.schema.json` | SignalingMessage | `MAX_MESSAGE_BYTES = 16 KiB` | Version 1, unknown fields rejected, strict JSON schema |
| `reason-codes.json` | Reason codes | encoded in enum check | Closed — no additions without a decision |
| `qr-payload.md` | QR Payload v1 | `512 B payload` | Magic `RCM1`, version 1, capabilities mask `0xffc0` reserved zero, expiry |

### 2. Strict versioning

- Contract is **v1** and frozen for F1 (ADR-0002). Parsing rejects `version > 1` in `SignalingParser` and `protocol::Version`.
- If a contract changes (outside the F1 freeze):
  1. Update `signaling.schema.json`, `qr-payload.md`, `reason-codes.json`.
  2. Update `specs/001-f0-technical-spikes/research.md` (decision D-new-name).
  3. Update `mobile/protocol` contract consumption if needed.
  4. Re-export `v1`. No `v2` or greater.
- Both platforms implement **handmade** parsing (not code-gen), so diffs between parsing and schema are caught by conformance.

### 3. Strict parsing

- **Unknown fields:** rejected (`INVALID_MESSAGE`), not ignored. Protects against undetected mismatches.
- **Hex:** only `0123456789abcdef` (lowercase); uppercase rejected. Hex in endpoints and fingerprints must be consistent (`QrPayload.encode()` orders endpoints by `(family, hex(address))` for stable wire representation).
- **Endpoints:** ordered by `(family, hex(address))` for stable wire representation.
- **Expiry:** `expiresAt > nowUnix` — expired payloads invalid.

### 4. Emission direction

- **Canonical encoder:** each implementation emits frames in one place — Rust `SignalingMessage::encode()` in `desktop/crates/protocol/src/signaling.rs`, Kotlin `SignalingEncoder`. Both mirror the frozen v1 schema; the encoder performs no version negotiation.
- **Canonical order:** `version`, `type`, then the fields of the message in schema declaration order, with no whitespace. Optional fields that were never supplied are omitted. The order is pinned by `signaling/encoding.json`, not left to a map implementation.
- **Validated values:** `Limits`, `OfferSdp`, `AnswerSdp`, `Candidates`, `Stats`, `CloseCode`, `ErrorCode` and `ErrorText` reject out-of-contract values at construction. Parsing and application code share those constructors, so a message that exists as a typed value re-parses to itself and no code path can emit an invalid message.
- **Size:** `encode()` refuses a frame above `MAX_MESSAGE_BYTES = 16 KiB` (`EncodeError::FrameTooLarge`). A validated message can still be refused when JSON escaping grows the frame past the cap.
- **Refusals:** the validation order and the code of each refusal are in ADR-0015. `OfferSdp` accepts only `sendonly`/`inactive` audio and video sections, `AnswerSdp` only `recvonly`/`inactive`; data channels and non-host, non-UDP or non-private `a=candidate` lines are `invalid_message`. Kotlin does not enforce the SDP policy yet.
- **Parity:** both the Rust and the Kotlin encoder execute the encoding vectors, and `compare_conformance.py` compares the results.
- **Wire freeze:** ADR-0002 still applies. Adding a message type, field or ordering rule is a version decision, not an implementation change.

### 5. Conformance harness

- Vectors in `protocol/vectors/`:
  - `qr/` — QR payload vectors (`qr/*.json`).
  - `signaling/scenarios.json` — `kind = message` scenarios plus `kind = handshake` scenarios. Each handshake scenario has a `layer` (ADR-0018): `core` carries `steps` and the expected state, transition, epoch, violations and reason code; `qr` names a `qr_vector`; `transport` is decided below the session core and is reported as not executed.
  - `signaling/encoding.json` — canonical frames and encode→parse round-trip.
  - `reason-codes/all-codes.json` — reason code verification.
- `run_conformance.sh` runs the Rust binaries (`bin/conformance.rs` in `protocol`, `bin/handshake_conformance.rs` in `session-core`) and the Kotlin `ContractTest` and `HandshakeConformanceTest`.
- `compare_conformance.py` compares Rust vs Kotlin results for QR, message, encoding and handshake vectors. It fails on any divergence and on any vector id that either report omits, so nothing is skipped silently. Its own tests are in `vectors/tests/`.
- Can be run via `make spike-interop` (if `f0-codec-interop` delivered), `make conformance`, or directly `cargo run --bin conformance -- ...`.

### 6. Code and structure

```
protocol/
├── README.md                   # Rules and corrected deviations documented
├── signaling.schema.json       # Strict signaling message v1 schema
├── reason-codes.json           # Closed reason codes
├── qr-payload.md               # QR payload v1 documentation
├── vectors/
│   ├── run_conformance.sh
│   ├── compare_conformance.py
│   └── tools/generate_vectors.py  # Reproducible vector generation
└── .gitkeep
```

- `README.md` documents 2 corrected deviations (`allOf → oneOf`, IPv4-regex vs IPv6 consumption).
- `generate_vectors.py` is responsible for producing conformance vectors. Reproducible — tags must be updated when the vectors change version.
- No automatic code-gen. Both parsings are handmade.

### 7. Contract change control

- Never add an unknown field to schema without justification (must come from a decision in `specs/001-f0-technical-spikes/research.md`).
- Never add a reason code without a decision.
- No addition to `reason-codes.json` without updating `desktop/crates/protocol/src/reason.rs` and `mobile/protocol/src/ReasonCode.kt`.
- Not exempt from 90% gate — `desktop/crates/protocol` has gate 90%, `mobile/protocol` counts in Makefile coverage.
