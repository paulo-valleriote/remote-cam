# ADR-0009 — Dedicated pairing-token comparison and zeroization primitives

- **Status:** accepted
- **Date:** 2026-10-24

## Context

The desktop Session Core must compare the 256-bit pairing token without
data-dependent early exit and clear token buffers when they are consumed or
dropped. Hand-written comparison and ordinary slice clearing do not provide
the required guarantees.

## Decision

- Use `subtle = 2.6.1` for `ConstantTimeEq` when comparing pairing-token bytes.
  It provides a focused, pure-Rust constant-time comparison API (BSD-3-Clause).
- Use `zeroize = 1.9.1` to clear token storage with compiler-resistant
  zeroization (Apache-2.0 OR MIT). Store retained token bytes in `Zeroizing`
  and clear parsed candidate bytes after comparison.
- Pin both dependencies exactly in the desktop workspace and use them only in
  `remotecam-session-core`.

## Alternatives considered

- A hand-written XOR/accumulator comparison: rejected because a dedicated
  constant-time primitive is clearer and less likely to be weakened by future
  edits.
- `PartialEq` or an early-return byte loop: rejected because their timing
  behaviour is not a constant-time contract.
- Ordinary `fill(0)` or `clear`: rejected because the compiler may remove
  writes that are no longer observable.

## Maintenance cost

Both crates are small, focused security utilities with exact version pins.
Their APIs are limited to constant-time equality and zeroizing fixed-size
buffers; no runtime, platform, or transitive cryptographic stack is introduced.
