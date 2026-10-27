# desktop/ — AGENTS.md

> Desktop-specific rules. Do NOT apply to `mobile/`, `protocol/`, or `spikes/`.

**Quick start:**
- **Scope:** `desktop/` only — Rust workspace, 6 crates, MSRV 1.92; pinned toolchain 1.99.1 via `rust-toolchain.toml`.
- **Hard rules:**
  - All dependencies are pinned with `=`; `Cargo.lock` is versioned.
  - Windows APIs only behind `cfg(windows)`, targeting the Windows 11 SDK.
  - Tokens are zeroized; TLS uses ephemeral certs with SHA-256 pinning.
  - No `NON-PRODUCTION` code lives here; F0 implementations are quarantined under `spikes/` (ADR-0003).
- **Before editing:** read `docs/constitution.md` and `protocol/AGENTS.md` if touching contracts; build with `make build-desktop`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/research.md`, `.agents/skills/`.

## Context and architecture

- **Rust workspace:** `Cargo.toml` resolver 3, edition 2024, MSRV 1.92, version 0.1.0.
- **Crates (6):** `media-core` (receive-only webrtc-rs peer connection, OpenH264 decode to NV12, Frame Ring and the real `MediaPort`; ADR-0021), `protocol` (delivered contract), `session-core` (product session state machine), `pairing` (ephemeral certificate, pairing material, pinned TLS 1.3 listener/client; ADR-0009), `app` (product shell and structured logging) and `scrub` (zeroizing global allocator and stack scrub; the only crate allowed `unsafe`; ADR-0017).
- **Toolchain:** `rust-toolchain.toml` — `1.99.1`, components `clippy` + `rustfmt`, profile `minimal`.
- **Build:** `make build-desktop` = `cargo build --manifest-path desktop/Cargo.toml --workspace`. It does not build `harness/`, which is a separate workspace.
- **No executable entry point** (`main`) in this phase; the product harness is under `harness/` and the F0 harnesses are under `spikes/`.

## Desktop-specific rules

### 1. Pinned exact dependencies

All dependencies are pinned with `=` (for example, `slint = "=1.18.1"`, `tokio = "=1.54.1"`, `windows = "=0.62.2"`).
- A version change is a deliberate change and must be recorded in `specs/001-f0-technical-spikes/research.md` §2.
- `Cargo.lock` is versioned; do not remove without justification.

### 2. Windows-specific code (`cfg(windows)`)

- Windows APIs are gated with `cfg(windows)` and target the Windows 11 SDK and MSVC build tools.
- **Linux CI:** `cargo check` works because `windows` is behind `cfg(windows)`.

### 3. Quarantined F0 code

The F0 implementations moved out of this tree in F1-01:

| Crate | Quarantined at |
| --- | --- |
| `remotecam-signaling` | `spikes/f0-signaling` |
| `remotecam-media-core` | `spikes/f0-media-core` |
| `remotecam-injection` | `spikes/f0-injection` |
| `remotecam-audio-bridge` | `spikes/f0-audio-bridge` |
| DirectShow fallback | `spikes/f0-dshow-vcam-fallback` |

They are non-production, are not workspace members, and MUST NOT be imported
from `desktop/` (FR-039, ADR-0003). `spikes/f0-pipeline` still consumes the
injection and media-core crates by relative path.

### 4. Security and privacy (`protocol` and `app`)

- `protocol/src/hex.rs` — hex via `0123456789abcdef` (lowercase); uppercase rejected.
- `protocol/src/reason.rs` — closed reason codes (see `protocol/reason-codes.json`); no additions without a contract update.
- `app/src/logging.rs` — mandatory redaction: hex ≥32 chars, absolute paths (`/home/`, `/Users/`, `C:\Users\` → `<path>`), correlation IDs = 32 lowercase hex, operations = `[a-z][a-z0-9_.]{0,63}`.

### 5. Tests and coverage

- `protocol/` — gate ≥90%. `tests/contract.rs` (vectors from `protocol/vectors/`), `tests/signaling_encoding.rs` (canonical emission and round-trip), `tests/signaling_refusal.rs` (SDP direction, data channel, candidate and parse refusals), `bin/conformance.rs`. `OfferSdp` and `AnswerSdp` enforce the SDP policy at construction (ADR-0015).
- `session-core/` — gate ≥90%. `conformance` (and the `handshake_conformance` binary) replays the handshake scenarios of `protocol/vectors` on the core; the Kotlin `:session-core` mirrors every decision and `make conformance` compares both (ADR-0018). Deterministic tests through `handle(event)`. The clock and randomness enter only through the `Clock` and `RandomSource` traits; `testing::{ManualClock, SeededRandomSource}` provide the reproducible implementations used by tests and the harness. `admit_signaling_frame` validates inbound frames in the order of ADR-0015 and returns a `SignalingRefusal` with exactly one closed code (an oversized frame ends the session with `close { security_violation }`). A rejected event carries the local closed code `invalid_transition`; it is a state-machine code, not part of the closed wire reason set in `protocol/reason-codes.json`.
- `pairing/` — gate ≥90%. `ActiveSession` owns the channel of the peer that holds the session and performs the replacement effects (`close { superseded }` best effort within `SUPERSEDE_BUDGET`, drop the predecessor, then `paired` to the successor); `frame` reads and writes length-prefixed signaling frames (4-byte big-endian length, 16 KiB limit, ADR-0014; an oversized prefix is `security_violation`); `PairingListener::close()` makes later connections fail with the local `listener_closed`; the listener also closes itself after 10 consecutive failed handshakes (a success resets the count), and the refused peer receives no frame. `throttle::OriginThrottle` limits attempts per source address (ADR-0016): 5 per 60 s, the sixth is `rate_limited`, then blocks of 60 s doubling up to 15 min, reset after 30 quiet minutes; IPv6 collapses to /64, the table holds at most 4096 origins, and state is per origin, never global. The clock is injected (`PairingListener::with_clock`). `accept_socket` returns an `AdmittedSocket` and `handshake` accepts only that type, so the throttle cannot be bypassed. The listener keeps a bounded log of `DiagnosticEntry` values (closed code, origin, block length; no token or fingerprint) read with `drain_diagnostics()` and counts drops in `dropped_diagnostics()`; `app::diagnostics::log_record` turns an entry into a structured log line with the address masked. `tests/supersession.rs` covers the replacement order and the Race Loser over TLS loopback. Unit tests cover the certificate, material and error mapping; `tests/loopback.rs` runs the real TLS 1.3 handshake over a loopback socket (exact pin accepted, divergent pin aborted with `certificate_pin_mismatch`, TLS 1.2 refused with `tls_version_unsupported`, no fallback on a closed port) and exchanges bytes in both directions. Tokens and full fingerprints never appear in `Debug` or error text.
- `app/` — log formatting/redaction tests and the diagnostic-entry mapping.
- `media-core/` — gate ≥90%. `FrameRing` (3 slots, drops the oldest, `push` never waits), `nv12`, `H264Decoder` (OpenH264, Linux/CI decoder only), `MediaCore` (receive-only connection; the network task depacketizes, a decode thread publishes into the ring) and `RingMediaPort` (the `MediaPort` implementation; `MediaFrame` is the event, pixels via `current_frame`). `testing::SyntheticPhone` is the `sendonly` stand-in used by tests and the harness. `tests/walking_skeleton.rs` runs real RTP over loopback. The Session Core must not depend on this crate.
- `scrub/` — `ZeroizingAllocator` (wipes on free, always moves on `realloc`) and `scrub_stack()`. When the desktop executable exists (none yet), it must install the allocator and call `scrub_stack()` on the session thread after dropping the session; hold session material behind a `Box`. `PairingMaterial` wipes on drop. Proven by `make residue` (ADR-0017).
- No `spikes/` module is imported by any crate here.

### 6. Before modifying

- Before editing `crates/protocol/src/lib.rs`: check `protocol/AGENTS.md` (versioned contracts, wire freeze).
- Before using the `windows` crate: confirm it is behind `cfg(windows)` and document the decision.
- Before copying code from `spikes/`: stop — quarantined code is not a merge source (ADR-0003).
