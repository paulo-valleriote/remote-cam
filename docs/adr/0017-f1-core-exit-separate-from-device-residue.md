# ADR-0017 — The F1 core exit is separate from the C-02 device residue inspection

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** ADR-0002, REC-16, F0 gate report amendment A-1

## Context

The F0 closure waived C-02 (residue inspection after application restart) for F1
with a single deadline, 2026-11-20, "before the F1 exit gate". The inspection
needs two named reference devices (Windows 11 and Android) that are not
available, so the F1 date depended on unavailable hardware.

## Decision

- The **F1 core exit** (protocol core, session core and product harness exit
  gate) keeps 2026-11-20 and has no device dependency.
- **C-02-residual** continues C-02-residue-restart: same inspection, same
  method, two named reference devices. Its deadline is 2026-12-20, aligned with
  C-04 and before the F2 exit gate. The owner role is `security_reviewer`.
- F1 stays `conditional`; F2 depends on the F1 core exit and on C-02-residual;
  F3 is unchanged. No gate verdict changes.
- The change is recorded as amendment A-1 in
  `spikes/evidence/gate_report_amendments.json`, which `make gate-report` layers
  over the F0 evidence record. The F0 records (`f0_exit.json`, `D-EXIT-f0.md`)
  are not rewritten.

## Consequences

- The 2026-12-20 date is an assumption by the amender, not given in REC-16.
- Regenerating the gate report keeps A-1; dropping the amendments file drops it.
