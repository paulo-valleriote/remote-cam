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
- **Before editing:** update `signaling.schema.json` / `reason-codes.json` and `specs/001-f0-technical-spikes/research.md`; run `make spike-interop` / `run_conformance.sh`.

**Context sources:** `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/contracts/signaling-messages.schema.json`, `.specify/init-options.json`, `.claude/skills/technical-design-doc-creator/`, `.opencode/commands/speckit.*`.

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

- Contract is **v1**. Parsing rejects `version > 1` in `SignalingParser` and `protocol::Version`.
- If a contract changes:
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

### 4. Conformance harness

- Vectors in `protocol/vectors/`:
  - `qr/` — QR payload vectors (`qr/*.json`).
  - `signaling/scenarios.json` — signaling scenarios.
  - `reason-codes/all-codes.json` — reason code verification.
- `run_conformance.sh` runs Rust binary (`bin/conformance.rs`).
- `compare_conformance.py` compares Rust vs Kotlin results (ensures platforms do not diverge).
- Can be run via `make spike-interop` (if `f0-codec-interop` delivered) or directly `cargo run --bin conformance -- ...`.

### 5. Code and structure

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

### 6. Contract change control

- Never add an unknown field to schema without justification (must come from a decision in `specs/001-f0-technical-spikes/research.md`).
- Never add a reason code without a decision.
- No addition to `reason-codes.json` without updating `desktop/crates/protocol/src/reason.rs` and `mobile/protocol/src/ReasonCode.kt`.
- Not exempt from 90% gate — `desktop/crates/protocol` has gate 90%, `mobile/protocol` counts in Makefile coverage.
