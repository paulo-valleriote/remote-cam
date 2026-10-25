# ADR-0010 — Versioned harness report contract and coverage gate

- **Status:** accepted
- **Date:** 2026-10-25
- **Applies from:** REC-25

The product Harness report is a versioned machine-readable contract, so REC-25
adds a committed JSON Schema and validates emitted reports in tests with
`jsonschema` `=0.60.1` (MIT, Rust 1.85 minimum); default resolver features are
disabled because the local schema needs no remote or file references. This is
preferable to hand-written partial field checks, which could silently diverge
from the published report shape.

REC-25 also requires ≥90% line coverage for the Harness, measured by
`cargo-llvm-cov` and enforced by `make coverage`. This is a deliberate,
Harness-specific exception to ADR-0008's original exclusion: the Harness remains
non-shipped, but its report and scenario behavior are part of the F1 quality
gate. Existing delivered-package thresholds are unchanged.

The report format advances from version 2 to version 3 and carries protocol
contract version 1 separately. Protocol v1 remains frozen; the report adds no
wire fields or reason codes.
