# ADR-0024 — A/V sync: audio as the master clock and the hard drift limit

- **Status:** accepted
- **Date:** 2026-10-27
- **Relates to:** ADR-0002, ADR-0019, ADR-0022, ADR-0023, REC-4, REC-54

## Context

F3-05 (REC-54) presents video against the audio. The A/V Drift (GLOSSARY) targets
20 ms over one hour; above 100 ms the session ends with `close { media_failure }`.
Audio and video never share a buffer (TDD §5.5).

## Decision

- **Separate chains, one clock.** `AudioEngine` keeps its own buffer of Opus packets and
  `VideoSync` its own queue of decoded frames. The only thing that crosses is an
  `AudioClock`: the audio chain publishes `(position, at)` when a slot starts playing;
  the video chain reads it. No samples and no frames cross.
- **Presentation.** `VideoSync::tick` presents the newest frame whose media time is at
  most half a 30 fps frame (16.7 ms) ahead of the interpolated audio position and skips
  older ones; later frames wait. Media time is measured from the first packet of each
  stream (both start at the same capture instant on the v1 phone). Mapping through RTCP
  Sender Reports is not done; a phone that starts its streams apart would show a
  constant offset. That refinement belongs with the device bridge (F3-09).
- **Drift is sustained offset, not jitter.** The reported drift is the median of the
  signed offset over the last 31 presented frames (about 1 s). A frame delayed by loss
  recovery (NACK) is late by tens of milliseconds, which is jitter; judging each frame
  alone would end sessions under the nominal 3 % loss. A real offset moves the median
  within about half a second. A held frame more than 100 ms ahead of the audio is
  reported at once, unfiltered.
- **Both numbers are published.** The windowed `max_drift` is what the limit judges;
  the raw `max_frame_offset` of any single frame is reported beside it, so the loss
  jitter the median ignores stays visible. A held frame far ahead of the audio is
  reported unfiltered because it is a standing condition, not a transient.
- **Starvation is not drift.** Drift is measured when a frame is presented. A video
  that stops arriving is a liveness failure handled by the output path (a frame
  repeats for at most 2 s, then the video is `Absent`, ADR-0022); judging it as drift
  would end sessions on the 100 ms stalls the nominal loss profile produces.
- **No audio, no master.** If the clock was never published or has been silent for
  100 ms, the video is presented as it arrives (`SyncMode::VideoFree`), no drift is
  measured and the report marks the mode. A live clock takes the video back.
- **Hard limit.** `SessionEvent::AvDrift(AvDrift)` carries a measured drift to the
  Session Core. In `Connected`, a drift strictly above `AV_DRIFT_LIMIT` (100 ms) yields
  `[SendClose { MediaFailure }, End]` and `ENDED`; 100 ms exactly does nothing. Any
  other state rejects the event with `invalid_transition`. The existing `media_failure`
  code is used; the wire contract is untouched (ADR-0002).
- **Rust only.** The Kotlin `:session-core` does not mirror the event: the phone sends
  media and never presents it, so it has no drift to judge.

## Evidence

The harness (report version 8, `av_sync`) runs one logical hour of the nominal profile
(`max_drift_micros` ≤ 20 000, none above the target), ten seconds without audio
(`video_free`), and a 150 ms video delay that drives a connected Session Core to `ended`
with the close before the release. The video decoder is a stub there: only timestamps
matter, and an hour of real H.264 would not add evidence. Real-time soak and Windows
host measurement are out of scope (`make soak`).

## Not wired yet

Nothing in the product path calls `SessionEvent::AvDrift`: `MediaCore` does not yet
host an `AudioEngine`, since the audio output is the device bridge (F3-09). The seam
is the observer of `run_av_profile_observed`, which the harness connects to a real
Session Core. Connecting the live chains belongs to F3-09.

## Consequences

- The audio keeps its fixed 60 ms depth; the video waits for it instead of the audio
  stretching.
- Presenting at the 30 fps display cadence keeps the offset within half a frame.
