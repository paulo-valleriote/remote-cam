# ADR-0019 — The F3 exit is split into a Core Exit and a Device Exit

- **Status:** accepted
- **Date:** 2026-10-26
- **Relates to:** ADR-0020, REC-4, F0 gate report

## Context

The F3 exit gate in REC-4 mixes criteria that run on the Linux harness (outbound
media RTP at zero, 3 % loss without a freeze above 2 s, A/V drift, latency
budget) with criteria that need the Windows 11 Reference Device (the minimum
application matrix, glass-to-glass latency, the virtual audio path). The F0
gates `camera_injection` and `desktop_receive_pipeline` closed `no_go` because
that environment is not available, and C-05 (audio licensing) is waived until
2027-01-15. A single F3 date would depend on hardware and on a licensing
decision that the Media Core does not need.

## Decision

- The **F3 Core Exit** covers stories F3-01 to F3-06 (the Media Core). It keeps
  the REC-4 target of 2026-12-18 and has no device dependency.
- The **F3 Device Exit** covers stories F3-07 to F3-10 (Media Foundation decode,
  virtual camera, WASAPI bridge, virtual audio device). Its deadline is
  2027-01-15, aligned with C-05. F4 depends on it.
- Time-based Core Exit criteria (30 minutes without outbound media RTP, one
  hour of A/V drift) are proven in the harness with an injected clock and a
  deterministic impairment profile (3 % uniform loss, 30 ms added latency,
  ±10 ms jitter, media only). `make soak` runs the same criteria in real time
  once, as evidence before the Core Exit; it is not part of the default CI.
- Audio is a separate chain: it never blocks the video stories, and only the
  virtual audio device story is subject to C-05. Video-only mode is the
  approved plan B.
- VP8 is out of F3. If C-04 fails, a new story is opened.
- No F0 gate verdict changes. The gate report amendment that records this split
  is tracked in Jira.

## Consequences

- The 2027-01-15 date was proposed by the author and accepted by the project
  owner on 2026-10-26.
- The REC-4 exit criteria are each assigned to exactly one of the two exits.
