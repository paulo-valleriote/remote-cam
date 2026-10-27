# ADR-0021 — Media Core crate, webrtc-rs and OpenH264 dependencies

- **Status:** accepted
- **Date:** 2026-10-27
- **Relates to:** ADR-0008, ADR-0019, REC-4, REC-50, `specs/001-f0-technical-spikes/research.md` D-03

## Context

F3-01 (REC-50) puts a real Media Core behind the Session Core's `MediaPort`:
a receive-only peer connection takes synthetic H.264 RTP, decodes it to NV12,
passes it through the Frame Ring and delivers a `MediaFrame`. The constitution
requires every external dependency to be justified (problem, alternatives,
maintenance cost, license) before it is added. The F0 spike already chose
webrtc-rs (D-03); that code is quarantined (ADR-0003), so the dependency is
re-justified here for the product crate.

## Decision

### Crate and boundary

- `desktop/crates/media-core` (`remotecam-media-core`) is a new product crate in
  the desktop workspace. It depends on `remotecam-session-core` (for the
  `MediaPort` and `Clock` traits) and never the reverse: the Session Core knows
  neither webrtc-rs nor OpenH264.
- `MediaFrame` is the arrival event (sequence and injected-clock timestamp).
  Pixels travel through the Frame Ring and stay readable on the port
  (`RingMediaPort::current_frame`).
- The Frame Ring has 3 slots; a full ring drops its oldest frame; `push` takes
  one short mutex and never waits for the consumer.
- The network task only depacketizes. Complete access units go to a dedicated
  decode thread through a bounded queue (8 units); a full queue drops the unit.
- `media_core::testing::SyntheticPhone` is the `sendonly` mobile stand-in used by
  tests and the harness. It lives in the crate (not in `harness/`) so the crate's
  own tests can reach the coverage gate; it is compiled into the library because
  the harness consumes it.
- The harness signaling frames still carry placeholder SDP (REC-23). The
  real-media scenario negotiates the real offer/answer in-process between the
  synthetic phone and the Media Core, after the pairing handshake. Carrying the
  real SDP over the pinned TLS channel is future work.

### Dependencies (all pinned with `=`)

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `webrtc` | `=0.21.1` | ICE, DTLS-SRTP, RTP/RTCP, interceptors for the receive-only connection | A bespoke QUIC/SRTP pipeline (reimplements loss handling and crypto); `webrtc-sys` bindings to libwebrtc (heavy C++ build); `str0m` (different API, re-plumbing). See D-03 | Medium — 0.x crate, minor bumps may break the API; exact pin, upgrades are deliberate | MIT OR Apache-2.0 |
| `rtc` | `=0.21.1` | Sans-I/O core of webrtc-rs; provides the RTP types and the H.264 depacketizer (`rtc::rtp::codec::h264`) | Writing an RFC 6184 depacketizer (FU-A/STAP-A edge cases) | Medium — always pinned to the same version as `webrtc` | MIT OR Apache-2.0 |
| `openh264` | `=0.9.9` | Software H.264 decode (and encode for the synthetic phone) so Linux/CI can exercise the whole path | `ffmpeg-next` (LGPL obligations, heavy native build); a bespoke decoder; no decoder (the walking skeleton would not produce a real frame) | Low — thin wrapper over Cisco OpenH264, built from vendored source (`source` feature) | BSD-2-Clause |
| `async-trait` | `=0.1.92` | `PeerConnectionEventHandler` is an async trait | Boxing futures by hand | Low — stable, ubiquitous | MIT OR Apache-2.0 |

`tokio` (already pinned, ADR-0009) gains the `sync` feature for this crate. The
harness workspace adds the `time` feature to `tokio` and a path dependency on
`remotecam-media-core`.

### Limits that stay open

- OpenH264 decodes constrained-baseline streams only. It is the Linux/CI decoder;
  the product decode on Windows is Media Foundation (F3-07), and software-decode
  results are never cited for the latency budget (research D-05).
- Building OpenH264 from source does not carry Cisco's patent grant, which covers
  only Cisco's own binaries. So the software decoder is never distributed:
  `openh264` is a `cfg(not(windows))` dependency, `software_decoder` and
  `testing` are not compiled on Windows, and the Media Core takes its decoder
  through the `VideoDecoder` trait (`MediaCore::with_decoder`). The Windows
  product supplies the Media Foundation decoder in F3-07. If a software
  fallback is ever needed in the product, it must load Cisco's binary at run
  time (`openh264` `libloading`) or carry its own patent licence; that falls
  under C-05 (2027-01-15). This is not legal advice.
- Out of scope here: loss recovery (NACK/PLI), adaptive jitter buffer (F3-03),
  audio (F3-04), Media Foundation (F3-07).

### Residue finding (resolved)

`webrtc` forces tokio's `rt-multi-thread` feature. In the harness workspace,
`cargo test --workspace` unifies that feature into the `remotecam-residue`
binary, and its `session-end` sweep found the pairing token in the heap in
about 8-15 % of runs (6 of 40 and 6 of 80; 0 of 40 without the feature).

Cause: the retained block is the `ScheduledIo` (272 bytes) of a `TcpStream`
registration. Tokio does not free it when the socket drops; it parks it in the
I/O driver's `pending_release` list and frees it on the next driver turn. The
probe parked after `drop(session)` and never took one, so the block stayed
allocated through the sweep. The token is not a field of the registration: it
sits in uninitialised padding that `Arc::new(ScheduledIo)` copied from stale
stack frames of the pairing. The feature only changes frame layout, hence which
stale bytes land in the padding. Neither the Session Core nor the runtime
retains the token.

Fix: the probe takes one driver turn (`runtime.block_on(yield_now())`) after the
session drops and keeps its runtime alive. 0 of 100 failures; `sleep(1 ms)` also
gives 0 of 100. Dropping the runtime also gives 0, but it frees everything the
runtime retains and would hide a real retention, so it was rejected.

Method change (ADR-0017, C-02): `session-end` now measures a process that has
taken one driver turn after the session, as the product does continuously, not
a parked one. The `live-control` and canary controls are unchanged and still
prove the method sees leaks. Linux evidence generated before this change is not
comparable on the `session-end` scope.

Residual risk, not fixed here: in the product there is a window of one driver
turn between a socket drop and the release of its registration, during which
stale stack bytes in the padding stay allocated. `scrub_stack()` before the
registration is built would close it; track it as a separate ticket.

`make test` runs `cargo test --workspace` in `harness/` again.

## Consequences

- `make coverage` measures `remotecam-media-core` (≥ 90 % lines).
- Harness report moves to version 5: ten scenarios, the new `real-media-frame`
  scenario, and an optional `real_media` observation (pixel format, size, frames
  decoded and dropped, ring slots, receive-only). Every field is deterministic;
  wall-clock time is used only to wait for loopback delivery.
- The harness runtime now enables the tokio time driver.
- Nothing under `spikes/` is imported; the logic of `spikes/f0-media-core` was
  reimplemented against the product contract.
