# ADR-0026 — Virtual camera publication seam and the CI recorder

- **Status:** accepted
- **Date:** 2026-10-28
- **Relates to:** ADR-0003, ADR-0019, ADR-0021, ADR-0022, REC-57, REC-83

## Context

F3-08 (REC-57) publishes the phone's video to the Windows 11 **Virtual Device**
(`MFCreateVirtualCamera`, per-user, hosted by the Frame Server). That adapter needs the
Reference Device and a cross-process transport decision (REC-84, REC-85). The producer
side (Frame Ring to publication) can be built and proven on Linux/CI first.

## Decision

- **One seam.** `VirtualCameraPublisher` (`start(modes)`, `publish(header, frame)`,
  `stop()`) is the only boundary between the Frame Ring producer and the Virtual Device.
  The Windows adapter (REC-84) implements it behind `cfg(windows)`.
- **Producer.** `VirtualCameraProducer::pump` takes only the newest frame from the
  `FrameSource` (the Frame Ring drains to its last element), paces it with `FramePacer`
  to the consumer-selected `OutputMode` and publishes. It never waits on the network
  thread; `FrameRing::push` stays non-blocking (ADR-0021).
- **Modes.** `start` always announces `OutputMode::ALL` (640x480, 720p, 1080p at 30 fps);
  the consumer's choice selects the published size.
- **Header.** `FrameHeader { width, height, stride, format, timestamp, sequence }`.
  `timestamp` is the RTP timestamp; `sequence` grows only after a successful publish, so
  a failed publish keeps the frame in the pacer and does not consume a number.
- **Lifecycle.** `stop` is idempotent and releases every resource; a restart resets the
  sequence and never republishes a frame from before the stop.
- **CI stand-in.** `RecordingPublisher` (`cfg(not(windows))`) records event order
  (`Started`, `Frame`, `Stopped`), headers and open resources. The unit tests that use
  it carry the same gate.

## Consequences

- The Windows adapter and the real-app matrix are not covered here; their evidence
  comes from the Reference Device (REC-84, REC-85).
- `FrameHeader::stride` equals `width` (NV12 luma, packed frames). If `Nv12Frame` ever
  carries padding, the header must read the real stride.
- `dropped()` counts Frame Ring evictions only, not frames discarded by the producer
  draining to the newest.
