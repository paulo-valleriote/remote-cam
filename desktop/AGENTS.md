# desktop/ — AGENTS.md

> Desktop-specific rules. Do NOT apply to `mobile/`, `protocol/`, or `spikes/`.

## Context and architecture

- **Rust workspace:** `Cargo.toml` resolver 3, edition 2024, MSRV 1.92, version 0.1.0.
- **Crates (6):** `protocol`, `signaling`, `media-core`, `injection`, `audio-bridge`, `app`.
- **Toolchain:** `rust-toolchain.toml` — `1.99.1`, components `clippy` + `rustfmt`, profile `minimal`.
- **Build:** `make build-desktop` = `cargo build --manifest-path desktop/Cargo.toml --workspace`.
- **No executable entry point** (`main`) in this phase; all are harnesses/receivers.

## Desktop-specific rules

### 1. Pinned exact dependencies

All dependencies are pinned with `=` (e.g., `webrtc = "=0.21.1"`, `tokio = "=1.54.1"`, `windows = "=0.62.2"`).
- A version change is a deliberate change and must be recorded in `specs/001-f0-technical-spikes/research.md` §2.
- `Cargo.lock` is versioned; do not remove without justification.

### 2. Windows-specific code (`cfg(windows)`)

- `media-core/src/adapters/mf_decoder.rs` — `MFTEnumEx`, `IMFDXGIDeviceManager`, `D3D11CreateDevice`, `IMF2DBuffer2::Lock2DSize`. Only runs on Windows 11 SDK + MSVC.
- `media-core/src/adapters/wasapi_render.rs` — WASAPI render.
- `injection/src/mf_virtual_camera.rs` — `IMFMediaSource`, `IMFMediaStream`, `IMFMediaEventGenerator`, `IClassFactory`, `DllGetClassObject`, `DllCanUnloadNow`. ~533 lines.
- `injection/src/registration.rs` — COM registration (`HKCU` per-user primary, `HKLM` fallback, `WOW6432Node`).
- `audio-bridge/src/lib.rs` — empty by design (Phase 2).
- **Linux CI:** `cargo check` works because `windows` is behind `cfg(windows)`. Never cite `software_decoder` (OpenH264) results as latency evidence.

### 3. DirectShow fallback (not built by default)

- `vendor/dshow-vcam-fallback/` — complete C++ project (`CMakeLists.txt`, `src/`, `include/`).
- **Rule:** only built when a camera-matrix cell fails; not part of default build.
- Do not trust claims about it until built and measured on a reference Windows 11 machine.

### 4. Security and privacy (protocol/signaling/app crates)

- `protocol/src/hex.rs` — hex via `0123456789abcdef` (lowercase); uppercase rejected.
- `protocol/src/reason.rs` — 26 closed codes; no additions without contract update.
- `signaling/src/token.rs` — mandatory zeroization (`ptr::write_volatile` + memory fence) for 32-byte pairing tokens. No token persists beyond session.
- `signaling/src/certificate.rs` — TLS 1.3 (`rustls`), ephemeral `rcgen` cert, SHA-256 pinning (`subtle` constant-time), no root store in product path.
- `app/src/logging.rs` — mandatory redaction: hex ≥32 chars, absolute paths (`/home/`, `/Users/`, `C:\Users\` → `<path>`), correlation IDs = 32 lowercase hex, operations = `[a-z][a-z0-9_.]{0,63}`.

### 5. Tests and coverage

- `protocol/` — gate ≥90%. `tests/contract.rs`, `bin/conformance.rs` (vectors from `protocol/vectors/`).
- `media-core/` — concurrency, drift, jitter policy, teardown tests.
- `injection/` — `ring_buffer.rs` (drop-oldest, never blocks writer), `test_pattern.rs` (binary marker). Tested on Linux CI.
- `signaling/` — pinning, throttling (5/min, progressive block, listener close at 10 failures), single-use token.
- `app/` — log formatting/redaction tests.
- No `spikes/` module is imported by any crate here.

### 6. Before modifying

- Before editing `crates/protocol/src/lib.rs`: check `protocol/AGENTS.md` (versioned contracts).
- Before editing `crates/signaling/src/server.rs`: re-read `desktop/AGENTS.md` §4 (security, TLS, zeroization).
- Before using `windows` crate or DirectShow: confirm intentional and document the decision (`specs/001-f0-technical-spikes/research.md`).
