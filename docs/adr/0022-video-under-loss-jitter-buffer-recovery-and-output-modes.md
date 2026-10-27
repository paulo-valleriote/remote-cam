# ADR-0022 — Video under loss: jitter buffer, loss recovery and output modes

- **Status:** accepted
- **Date:** 2026-10-27
- **Relates to:** ADR-0015, ADR-0019, ADR-0021, REC-4, REC-52

## Context

F3-03 (REC-52) keeps video usable under the TDD loss profile: 3 % uniform loss,
30 ms added latency, ±10 ms jitter, media only. webrtc-rs has no NetEq-style
jitter buffer, so the Media Core owns one. No new dependency is added.

## Decision

### Receive engine

- `engine::ReceiveEngine` is the synchronous core: jitter buffer, depacketizer,
  decoder and `Recovery`. Time enters only as an injected `Duration`; the
  decode worker takes it from `pipeline::ElapsedClock` (`DecodePipeline::with_clock`),
  and `SystemElapsed` is the production implementation.
- The jitter buffer reorders by extended sequence number and releases on a
  playout schedule anchored at the first packet. Target depth is twice the RFC 3550
  interarrival jitter, capped at 200 ms. A gapless stream is never held.
- A gap holds the stream back for at most 120 ms, counted from the arrival of the
  packet that revealed it: a bit over one round trip of the nominal profile
  (60 ms ±20 ms). Gaps seen together wait together. A skipped gap counts as lost.
- An access unit is dropped when the RTP timestamp changes before its marker
  packet arrives, so a lost marker never merges two pictures.

### Loss recovery

- `Recovery` is the single NACK source. The default webrtc-rs NACK generator is
  not registered in `receive_only_registry`; the SDP still advertises `nack` and
  `nack pli` feedback. NACK and PLI leave only through `Recovery` and
  `TrackRemote::write_rtcp`.
- NACK is retried every 50 ms, at most 3 times per packet; a packet given up on
  asks for a keyframe. A packet the jitter buffer skipped as lost, or a decoder
  error, also asks for a keyframe. PLI is rate limited to one per 500 ms.

### Output modes

- `OutputMode` publishes 640×480, 1280×720 and 1920×1080 at 30 fps.
  `FramePacer.tick` yields the newest frame (`Fresh`), the previous frame
  (`Repeated`, for at most 2 s) or `Absent`. A frame is rescaled only when its size
  differs from the mode. A frozen picture is never presented as live video.
- `RingMediaPort::with_output_mode` enables the pacer; each `next_frame` call is one
  tick. The 30 Hz cadence belongs to the consumer, the virtual camera (REC-57); the
  Media Core does not own a timer thread.

### Harness

- `real-media-frame` reports `loss_recovery` from a virtual-time run of the single
  nominal profile (`report_version` 6). Any other profile is labelled
  `out_of_profile` and cannot pass the gate. The numbers come from the simulation,
  not from a real WebRTC session with loss.

## Consequences

- Removing the default NACK generator is a wire-behaviour change; RTCP delivery to a
  real phone is not yet proven (F3 Device Exit).
- Harness report consumers must accept the required `real_media.loss_recovery`.
