# ADR-0017 — Zero persistence is proven by one inspection method; the desktop scrubs freed memory

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** REC-15, C-02-residue-restart, D-33, TDD §5.8

## Context

TDD §5.8 says no session state is persisted and that keys, tokens and fingerprints
die with the session. It also says the proof covers two cases with **one method**:
after a normal session end and after an application restart.

A first run of the inspection found real residue in a *live* process after the
session ended, with disk, keyring and restart already clean: the pairing token (raw
and hex), the fingerprint, the certificate DER and the QR payload sat in freed heap
blocks and in dead stack frames. Neither Rust nor its dependencies wipe freed
buffers, and a moved value leaves its old bytes behind.

## Decision

**Method.** `make residue` (`remotecam-residue`, in the harness workspace) runs a
probe process through a complete pairing and applies one inspection at four points:
`live-control`, `session-end`, `process-exit` and `restart`. Each inspection sweeps
process memory (writable mappings of the live probe), the state directories (the
probe's `HOME`, `XDG_*` and `TMPDIR` are a throwaway sandbox that must stay empty),
the credential stores under the real home, and the TLS ticket state. The report
declares what each inspection swept (D-33).

- The inspector derives the searched secrets from the seeds; the probe never holds a
  copy beyond the session.
- **Positive control.** `live-control` must find the secrets in the running session;
  otherwise the method is not sensitive and the run fails.
- **Negative control.** A canary probe leaks on purpose (file and memory); the run
  fails unless both are detected.
- TLS tickets are not scanned for: none are issued. That scope is a configuration
  invariant, asserted by `server_config_issues_no_tickets_and_keeps_no_resumable_session`.
- Exit code 0 means no residue and both controls held. The report is deterministic
  and is versioned at `docs/evidence/f1/rec-15-zero-persistence-linux.json`.

**Scrubbing.** New crate `remotecam-scrub` (the only crate with `unsafe`):

- `ZeroizingAllocator` wipes every block before it is freed and always moves on
  `realloc`. The desktop executable installs it as the global allocator.
- `scrub_stack()` overwrites 512 KiB of dead stack; call it on the session thread
  after the session is dropped.
- `PairingMaterial` wipes its fingerprint and QR on drop.
- A session is held behind a `Box`, so a move copies a pointer, not the secrets.

## Consequences

- Device inspection (C-02) uses this same method; running it on the reference
  devices stays open and is not claimed by the Linux evidence.
- Scope limit: memory owned by C libraries through `malloc` is covered only to the
  extent the sweep sees it. None was found on Linux; a device run may differ.
- Every worker thread of the future desktop shell needs its own `scrub_stack()`.
- Zeroing on free costs CPU proportional to freed bytes; acceptable at pairing scale.
