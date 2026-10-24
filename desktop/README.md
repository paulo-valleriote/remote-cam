# RemoteCam Desktop

Rust workspace for the Windows 11 receive-and-inject application. F0 built
disposable harnesses around the risk surfaces; those harnesses and the F0
implementations are now quarantined under `spikes/` (ADR-0003). The product
application shell is not part of this phase.

## Crates

| Crate | Directory | Responsibility |
| --- | --- | --- |
| `remotecam-protocol` | `crates/protocol` | Rust implementation of the shared signaling and QR contract. Delivered package, coverage gate ≥ 90 %. |
| `remotecam-app` | `crates/app` | Desktop shell, diagnostics and structured logging. |

The F0 crates `remotecam-signaling`, `remotecam-media-core`,
`remotecam-injection` and `remotecam-audio-bridge` moved to `spikes/f0-*` in
F1-01. They are non-production, are not workspace members, and must not be
imported from this tree.

## Prerequisites

- Rust 1.99.1, pinned by `rust-toolchain.toml`; the workspace MSRV is 1.92 and
  the edition is 2024.
- Windows 11 build 22000 or later when the product shell starts using Media
  Foundation or WASAPI (not yet in this phase).
- Windows 11 SDK and MSVC build tools when building the `windows` crate on
  Windows.
- Linux builds the protocol crate and the quarantined spike harnesses for CI;
  the `windows` dependency is target-gated.

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
