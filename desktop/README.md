# RemoteCam Desktop

Rust workspace for the Windows 11 receive-and-inject application. F0 builds
disposable harnesses around the risk surfaces; the product application shell is
not part of this phase.

## Crates

| Crate | Directory | Responsibility |
| --- | --- | --- |
| `remotecam-protocol` | `crates/protocol` | Rust implementation of the shared signaling and QR contract. Delivered package, coverage gate ≥ 90 %. |
| `remotecam-signaling` | `crates/signaling` | TLS 1.3 WSS listener, pinned client, ephemeral certificate, QR generation, token store and throttling. |
| `remotecam-media-core` | `crates/media-core` | Receive-only peer connection, jitter policy, Media Foundation decode adapters, handoff, audio render and A/V sync. |
| `remotecam-injection` | `crates/injection` | NV12 ring buffer and Media Foundation virtual camera media source and registration. |
| `remotecam-audio-bridge` | `crates/audio-bridge` | WASAPI render into the virtual audio endpoint, readiness and degraded operation. |
| `remotecam-app` | `crates/app` | Desktop shell, diagnostics and structured logging. |

## Prerequisites

- Rust 1.99.1, pinned by `rust-toolchain.toml`; the workspace MSRV is 1.92 and
  the edition is 2024.
- Windows 11 build 22000 or later for the Media Foundation virtual camera and
  WASAPI crates.
- Windows 11 SDK and MSVC build tools when building the `windows` crate on
  Windows.
- Linux builds the protocol, media-core and spike harnesses for CI; the
  `windows` dependency is target-gated.

Dependency versions are pinned exactly in `Cargo.toml`; `Cargo.lock` is
committed. A dependency bump is a deliberate change and updates the dependency
ledger in `specs/001-f0-technical-spikes/research.md` §2 in the same delivery.

## Build and test

```bash
make build-desktop
make test
make coverage
make lint
```

`make coverage` requires `cargo-llvm-cov`; install it with Cargo if it is not
already available.

The pinned toolchain installs `clippy` and `rustfmt`; `make lint` fails on any
formatting difference or Clippy warning.

## Debugging

`make build-desktop` keeps debug symbols. The current phase has no binary entry
point; the executable gate harnesses live in `spikes/` and are run through
`make spike-handshake`, `make spike-pipeline` and the other `spike-*` targets.
When a harness fails, the procedure named in its `docs/spike/gate-*.md` file
points to the evidence file to inspect.

Structured logs, correlation identifiers and redaction rules are defined in
`docs/spike/logging-schema.md` (Phase 2). Logs never contain tokens, full
fingerprints, secrets or personal data.

## Cleanup

```bash
make clean
```

This removes `desktop/target/` and generated evidence scratch space. It never
removes committed evidence or procedures.
