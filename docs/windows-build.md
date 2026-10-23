# Windows-native Rust builds from WSL2

This document describes how to build the Windows-native Rust artefacts from a
WSL2 environment, and why a plain build inside the distribution filesystem is
not possible.

## Why the mirror exists

Two constraints combine:

1. **The `windows` dependency is target-gated.** `desktop/` compiles for
   Linux CI with the Windows-only code (Media Foundation virtual camera,
   WASAPI) compiled out. Anything that exercises the Windows paths must be
   built with the `x86_64-pc-windows-msvc` toolchain on a Windows host.
2. **The Windows toolchain cannot build from the WSL filesystem.** Invoking
   the Windows `cargo.exe` from WSL with the repository checked out on ext4
   makes rustc operate across the `\\wsl.localhost` UNC share. The
   incremental compilation session fails creating its lock file there
   (`ERROR_INVALID_FUNCTION` / "Função incorreta", `os error -2147024895`),
   and the 9P protocol makes large builds impractically slow even when it
   does not fail outright.

The supported pattern is therefore: **mirror the working tree to the NTFS
filesystem and build it natively on the Windows side**, where MSVC, the
Windows SDK and the pinned Rust toolchain already live.

## Prerequisites

On the WSL2 side:

- WSL2 (not WSL1) with Windows interop enabled (`WSLInterop` binfmt).
- `rsync`, `git` and GNU `make` (standard on Debian/Ubuntu).

On the Windows host:

- Rust installed via rustup (`C:\Users\<user>\.cargo\bin\cargo.exe`); the
  repository's `rust-toolchain.toml` pins 1.99.1 and rustup installs it on
  demand — first build may download the toolchain.
- MSVC Build Tools (Visual Studio 2022) and the Windows 11 SDK for the
  `windows` crate.

## Usage

```bash
make build-desktop-win
```

This target does two things:

1. `sync-desktop-win` — deletes and recreates the mirror at
   `/mnt/c/Users/<WINDOWS-USERNAME>/remotecam-win` with `rsync`
   (`--delete`, excluding `.git/`, all `target/` and `build/` directories,
   `node_modules/` and `.coverage/`). The mirror is a snapshot of the
   **current working tree**, including uncommitted changes.
2. Builds it with the Windows toolchain: `cargo build
   --manifest-path desktop\Cargo.toml --workspace` executed
   natively on the NTFS filesystem.

The artefacts land in
`/mnt/c/Users/<user>/remotecam-win/desktop/target/debug/` (or `release/`),
i.e. `C:\Users\<user>\remotecam-win\...` as seen from Windows.

### Overrides

| Variable | Default | Meaning |
|----------|---------|---------|
| `WIN_USER` | detected via `cmd.exe %USERNAME%` | Windows username used for the mirror path |
| `WIN_MIRROR` | `/mnt/c/Users/<WIN_USER>/remotecam-win` | Where the mirror is created |
| `WIN_RELEASE` | `0` | Set to `1` for a `--release` build (`target/release/`) |

```bash
make build-desktop-win WIN_RELEASE=1
make build-desktop-win WIN_MIRROR=/mnt/d/builds/remotecam
```

The sync step is neither incremental nor mergeable: every build first wipes
and rewrites the mirror. There is no manual step between sync and build, so
the built artefacts are always generated from the working tree as it stands.

## Building the F0 measurement harnesses

The five spike harnesses live in separate workspaces outside `desktop/`
(`spikes/f0-pipeline`, `f0-handshake`, `f0-virtual-audio`, `f0-codec-interop`
— camera injection is Python + PowerShell). To build them natively on
Windows, run them inside the mirror:

```powershell
cd C:\Users\<user>\remotecam-win
cargo build --release --manifest-path spikes\f0-pipeline\Cargo.toml
cargo build --release --manifest-path spikes\f0-handshake\Cargo.toml
cargo build --release --manifest-path spikes\f0-virtual-audio\Cargo.toml
cargo build --release --manifest-path spikes\f0-codec-interop\Cargo.toml
```

Executables land in `spikes\f0-*\target\release\*.exe`. The camera-injection
matrix procedure is `spikes/f0-camera-injection/run_matrix.py`, driven by
`make spike-injection` on the Windows host.

## Caution

- The mirror is **generated output**. Never edit files inside it; every
  change is overwritten on the next sync. All edits belong in the WSL
  working tree, and commits are made from there.
- The mirror path must not be added to a repository; it shares the same
  gitignore rules (`.git/` is not copied) but keep it out of VCS by location.
- `make build-desktop` stays the Linux/CI build (target-gated subset).
  The `-win` target is additive, not a replacement.
- Non-WSL Linux hosts simply keep using `make build-desktop`; the `-win`
  targets fail with a clear error without interop.
