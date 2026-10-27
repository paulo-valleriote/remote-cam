# ADR-0023 — Continuous audio: libopus, audio jitter buffer and uninterrupted silence

- **Status:** accepted
- **Date:** 2026-10-27
- **Relates to:** ADR-0019, ADR-0021, ADR-0022, REC-4, REC-53

## Context

F3-04 (REC-53) receives the phone microphone as Opus 48 kHz mono and hands 16-bit
PCM to an output that the Media Core does not know (WASAPI belongs to F3-09). The
stream must never be interrupted: a muted microphone or a flapping network yields
continuous silence, never a hole. Audio is never retransmitted.

## Dependency: libopus through the `opus` crate

- **Problem:** decode Opus and conceal lost packets (PLC). Only libopus is the
  reference implementation of RFC 6716 and the only one that provides PLC with the
  quality the acceptance criteria assume.
- **Decision:** `opus = "=0.4.0"` (safe bindings) over `opusic-sys`, which builds the
  bundled libopus with `cmake`. No system libopus is needed; `cmake` and a C compiler
  are build tools.
- **Alternatives:** `audiopus_sys` (older, system libopus on Linux, extra setup in
  CI); `opus-rs` and `opus-pure` (pure Rust, young, no PLC parity proof, conformance
  not established); a hand-written decoder (rejected by the constitution).
- **Maintenance cost:** one C library compiled from source on every clean build; the
  version is pinned with `=` and moves only by a deliberate change. The `opus` crate
  is small and the libopus API has been stable for a decade.
- **License:** `opus` MIT/Apache-2.0; `opusic-sys` and libopus BSD-3-Clause. Unlike
  OpenH264 there is no patent-licensing obligation on the distributed binary, so the
  dependency is not restricted to `cfg(not(windows))`. The open C-05 licensing item
  does not block this story.

## Decision

- `audio::OpusDecoder` decodes one packet to `i16` PCM at 48 kHz mono; `conceal`
  asks libopus for one frame of PLC. An empty payload is rejected: libopus would read
  it as a loss and conceal silently, so loss enters only through `conceal`.
- `audio_engine::AudioEngine` is the synchronous core, as `ReceiveEngine` is for
  video. Time enters as an injected `Duration`; PCM leaves through the injectable
  `AudioSink` trait, which knows nothing about WASAPI.
- The cadence comes from RTP timestamps. The first packet anchors the schedule; frame
  `T` plays at `anchor + (T - T0) / 48 000 + depth`. The buffer is keyed by extended
  RTP timestamp and has a fixed depth (60 ms), which covers the ±10 ms jitter of the
  nominal profile without ever stretching the schedule. No resampler exists; clock
  drift is F3-09.
- Every frame slot produces exactly one 20 ms frame: decoded audio, PLC (at most
  5 consecutive frames) or digital silence. A packet that arrives after its slot is
  counted late and dropped. There is no NACK and no retransmission for audio.
- The v1 phone sends 20 ms frames; a packet that decodes to any other length counts
  as a decode error and is concealed.
- A timestamp jump beyond 2 s re-anchors the schedule instead of emitting an
  unbounded run of silence.

## Dropout measurement

A dropout is a stretch of ≥ 50 ms in which the output stopped being a live signal.
Two independent detectors measure it and must agree: one watches **delivery timing**
(the gap between consecutive sink writes), the other watches **content** (runs of
exact digital zero in a source that is never silent). The harness reports both.

## Consequences

- Clean builds compile libopus (needs `cmake`).
- A fixed depth adds 60 ms of mouth-to-ear latency; adaptive depth would stretch the
  schedule and is left to the drift story (F3-09).
- The numbers come from a virtual-time simulation, not from a real WebRTC session.
