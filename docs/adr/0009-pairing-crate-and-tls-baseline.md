# ADR-0009 — Pairing crate and TLS dependency baseline

- **Status:** accepted
- **Date:** 2026-10-24
- **Applies from:** F1-02 (walking skeleton)

## Context

REC-21 builds the pairing material and the channel it authorises:

- a 256-bit pairing token generated per session from the injected RNG;
- an ephemeral ECDSA P-256 certificate, kept in memory, whose SHA-256 leaf
  fingerprint goes into the QR payload;
- deterministic harness execution requires certificate-key generation to use
  the scenario's injected random source;
- a listener that speaks TLS 1.3 only, with a dedicated ALPN, no session
  resumption, and no fallback;
- a client that accepts exactly the pinned leaf and aborts otherwise.

Three constraints shape where that code can live:

1. `desktop/crates/session-core` is deliberately I/O-free (ADR-0008). Adding a
   TLS stack or a socket to it would invert its role: it is the session domain,
   not a transport adapter.
2. The constitution requires external dependencies behind adapters that
   implement domain interfaces, and requires every dependency to be justified
   before it is added.
3. The quarantine rule (ADR-0003) forbids promoting the F0 `remotecam-signaling`
   crate. Its behaviour can inform a new implementation; its code cannot be
   merged.

The Session Core still needs to own the token: it is session material, it is
generated from the injected `RandomSource`, and REC-22 compares and consumes it
inside the session lifecycle. Zeroization is therefore required there, in the
domain, not only in the adapter.

## Decision

### Crate boundary

- A fourth desktop crate, `desktop/crates/pairing` (`remotecam-pairing`), owns
  the pairing adapter: ephemeral certificate, pairing material, TLS 1.3
  listener and pinned client.
- It depends on `remotecam-protocol` for the QR payload and on
  `remotecam-session-core` for `SessionId`, `PairingToken` and `RandomSource`.
  The dependency direction stays
  `pairing → session-core → protocol`.
- `session-core` gains `PairingToken` and `SessionId::generate`. It gains no
  I/O and no TLS dependency.
- Nothing under `spikes/` is imported.

### Listener binding

- `PairingListener::bind` binds the address supplied by its caller, including
  wildcard and non-loopback addresses so a mobile peer can reach the desktop
  over a private network. `bind_loopback` remains available for loopback-only
  use, including tests.
- The caller chooses which interfaces to expose; the pairing listener does not
  silently restrict access to loopback.

### Session Core addition

`PairingToken` is a 32-byte secret that is generated from the injected
`RandomSource`, zeroizes its buffer on drop, and prints `[elided]` instead of
its bytes. `SessionId::generate` exposes the identifier generation the state
machine already performed, so the material and the state machine draw from the
same source.

### TLS policy

- TLS 1.3 only; earlier versions are refused and reported as
  `tls_version_unsupported`.
- Dedicated ALPN name `remotecam/1`, set on both sides.
- Session resumption disabled on the server: empty session storage and zero
  TLS 1.3 tickets.
- Client trust is the exact SHA-256 of the DER-encoded leaf, compared in
  constant time. No root store is consulted.
- A pin mismatch stops the connection with `certificate_pin_mismatch`; the
  client performs one connection attempt and has no retry or fallback path.
- The listener and the client never log the token or the full fingerprint.
  `Debug` implementations elide both.
- Certificate errors expose stable failure stages and retain diagnostic text
  without exposing concrete `rcgen` or `p256` error types through the public
  pairing API.
- `EphemeralCertificate::generate` uses `rcgen` and the system cryptographic
  random source for production. The harness uses
  `EphemeralCertificate::generate_with_random_source` with its injected
  `CertificateRandomSource`, retaining the same ECDSA P-256 key type and
  producing reproducible certificate bytes for the same source. Implementors
  must provide cryptographically secure bytes outside isolated test harnesses.

### Dependencies

Every version is pinned exactly in `desktop/Cargo.toml` and locked in
`desktop/Cargo.lock`. The original TLS dependencies were already present in the
F0 signaling lockfile and recorded in the dependency ledger
(`specs/001-f0-technical-spikes/research.md` §2); this ADR is the product
decision to depend on them directly. `p256` is added for injected certificate
key generation as justified below.

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `rustls` | `=0.23.46` | TLS 1.3 with a project-owned exact-pin verifier | OpenSSL bindings; `native-tls` | Medium — 0.24 renames the verifier trait; isolated in one module | MIT OR Apache-2.0 |
| `tokio` | `=1.54.1` | Async TCP and handshake driving for the listener and client | `async-std`; blocking OS threads | Low | MIT |
| `tokio-rustls` | `=0.26.6` | Adapter between `tokio` streams and `rustls` connections | Hand-written async `rustls` pump | Low | MIT OR Apache-2.0 |
| `rcgen` | `=0.14.10` | Ephemeral per-session ECDSA P-256 certificate in memory | `openssl`; hand-rolled X.509 | Low | MIT OR Apache-2.0 |
| `p256` | `=0.13.2` | ECDSA P-256 key generation from an injected `RandomSource` and deterministic signing for the harness certificate | `rcgen`'s internal system RNG alone, which cannot accept an injected source; a custom elliptic-curve implementation | Low — focused RustCrypto curve implementation, exact pin | Apache-2.0 OR MIT |
| `sha2` | `=0.11.1` | SHA-256 over the leaf certificate DER | `ring` (pulled in transitively anyway) | Low | MIT OR Apache-2.0 |
| `subtle` | `=2.6.1` | Constant-time fingerprint comparison | `==` on arrays (not constant time) | Low | BSD-3-Clause |
| `zeroize` | `=1.9.1` | Wiping the pairing token buffer on drop | A hand-written volatile wipe | Low | Apache-2.0 OR MIT |

No QR rendering crate is added: REC-21 only needs the version 1 binary payload,
which lives in `remotecam-protocol`.

## Alternatives considered

- **Put the listener in `session-core`.** Rejected: the core would stop being
  I/O-free and deterministic, and every consumer of the state machine would
  link a TLS stack.
- **Put the listener in `remotecam-app`.** Rejected: `app` is the product shell
  and diagnostics boundary. TLS and sockets are adapters, not shell code.
- **Promote `spikes/f0-signaling`.** Rejected by ADR-0003. The product crate is
  a new implementation against the frozen contract.
- **Keep the token in the pairing crate only.** Rejected: the token belongs to
  the session lifecycle, is drawn from the injected RNG, and REC-22 consumes it
  in the Session Core.
- **Use a public CA or a root store.** Rejected by TDD §5.3: the QR already
  carries the authoritative fingerprint, and a chain adds trust state that the
  product explicitly does not have.
- **Use only `rcgen` for the harness certificate.** Rejected because its
  `KeyPair::generate` uses an internal `SystemRandom` and offers no injected
  random-source interface.

## Consequences

- `make build-desktop` now compiles the TLS stack. Linux CI keeps working
  because the listener is platform-neutral; Windows-specific code is still
  behind `cfg(windows)`.
- `make coverage` adds `remotecam-pairing` to the delivered packages, so the
  90 % line gate applies to it.
- REC-23 can drive the listener with the synthetic phone client without
  reaching into `spikes/`.
- The pairing crate is the only place that knows `rustls`, `rcgen` or `sha2`.
  A future TLS upgrade is a change in that crate, not in the session domain.
