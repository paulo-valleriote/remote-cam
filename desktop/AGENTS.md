# desktop/ — AGENTS.md

> Desktop-specific rules. Do NOT apply to `mobile/`, `protocol/`, or `spikes/`.

**Quick start:**
- **Scope:** `desktop/` only — Rust workspace, 3 crates, MSRV 1.92; pinned toolchain 1.99.1 via `rust-toolchain.toml`.
- **Hard rules:**
  - All dependencies are pinned with `=`; `Cargo.lock` is versioned.
  - Windows APIs only behind `cfg(windows)`, targeting the Windows 11 SDK.
  - Tokens are zeroized; TLS uses ephemeral certs with SHA-256 pinning.
  - No `NON-PRODUCTION` code lives here; F0 implementations are quarantined under `spikes/` (ADR-0003).
- **Before editing:** read `docs/constitution.md` and `protocol/AGENTS.md` if touching contracts; build with `make build-desktop`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/research.md`, `.agents/skills/`.

## Context and architecture

- **Rust workspace:** `Cargo.toml` resolver 3, edition 2024, MSRV 1.92, version 0.1.0.
- **Crates (3):** `protocol` (delivered contract), `session-core` (product session state machine) and `app` (product shell and structured logging).
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

- `protocol/` — gate ≥90%. `tests/contract.rs` (vectors from `protocol/vectors/`), `tests/signaling_encoding.rs` (canonical emission and round-trip), `bin/conformance.rs`.
- `session-core/` — gate ≥90%. Deterministic tests through `handle(event)`. The clock and randomness enter only through the `Clock` and `RandomSource` traits; `testing::{ManualClock, SeededRandomSource}` provide the reproducible implementations used by tests and the harness. A rejected event carries the local closed code `invalid_transition`; it is a state-machine code, not part of the closed wire reason set in `protocol/reason-codes.json`.
- `app/` — log formatting/redaction tests.
- No `spikes/` module is imported by any crate here.

### 6. Before modifying

- Before editing `crates/protocol/src/lib.rs`: check `protocol/AGENTS.md` (versioned contracts, wire freeze).
- Before using the `windows` crate: confirm it is behind `cfg(windows)` and document the decision.
- Before copying code from `spikes/`: stop — quarantined code is not a merge source (ADR-0003).
