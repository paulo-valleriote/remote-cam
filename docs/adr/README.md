# Architecture Decision Records

Durable decisions that are hard to reverse, surprising without context, or the
result of a real trade-off. Each ADR is small on purpose: it records that a
decision was made and why.

| ADR | Decision |
| --- | --- |
| [0001](0001-jira-source-of-truth.md) | Jira is the source of truth; the speckit flow is removed |
| [0002](0002-wire-freeze.md) | Protocol version 1 is frozen for F1 |
| [0003](0003-f0-code-quarantine.md) | F0 code provenance and quarantine |
| [0004](0004-epoch-baseline.md) | Epoch baseline is 1 and resets with the desktop process |
| [0005](0005-superseded-vs-listener-closed.md) | `superseded` and `listener_closed` mean different things |
| [0006](0006-handshake-vectors.md) | Handshake vectors are a non-wire addition |
| [0007](0007-harness-location.md) | Product harness location and status |
| [0008](0008-harness-dependency-baseline.md) | Session Core crate location and harness dependency baseline |
| [0009](0009-pairing-crate-and-tls-baseline.md) | Pairing crate and TLS dependency baseline |
| [0010](0010-harness-report-contract-and-coverage.md) | Versioned harness report schema and ≥90% Harness coverage gate |
| [0011](0011-pre-handshake-refusal-closes-without-reason-frame.md) | Pre-handshake refusals close the socket without a reason frame |
| [0012](0012-rec-10-acceptance-scenarios-and-report-v4.md) | REC-10 acceptance scenarios and harness report version 4 |
| [0013](0013-superseded-as-retired-state.md) | `SUPERSEDED` is the predecessor's retired state, not a resting state |
| [0014](0014-length-prefixed-signaling-frames.md) | Signaling frames on the pairing channel are length-prefixed |
| [0015](0015-signaling-refusal-order-and-codes.md) | Signaling refusals follow one validation order and carry one code |
| [0016](0016-origin-throttle-and-listener-lifecycle.md) | Pairing attempts are throttled per origin; the listener lifecycle stays listener-wide |
| [0017](0017-zero-persistence-inspection-and-memory-scrubbing.md) | Zero persistence is proven by one inspection method; the desktop scrubs freed memory |
| [0018](0018-kotlin-session-core-parity.md) | The Kotlin session core is a pure-JVM mirror, compared by handshake scenarios |
| [0019](0019-f3-core-exit-separate-from-device-exit.md) | The F3 exit is split into a Core Exit and a Device Exit |
| [0020](0020-f1-core-exit-separate-from-device-residue.md) | The F1 core exit is separate from the C-02 device residue inspection |
| [0021](0021-media-core-dependencies-and-walking-skeleton.md) | Media Core crate, webrtc-rs and OpenH264 dependencies |
| [0022](0022-video-under-loss-jitter-buffer-recovery-and-output-modes.md) | Video under loss: jitter buffer, loss recovery and output modes |
| [0023](0023-continuous-audio-opus-jitter-buffer-and-silence.md) | Continuous audio: libopus, audio jitter buffer and uninterrupted silence |
| [0024](0024-av-sync-audio-master-clock-and-drift-limit.md) | A/V sync: audio as the master clock and the hard drift limit |

Add a new ADR by scanning this directory for the highest number and
incrementing it. To revisit a decision, write a new ADR that supersedes the old
one; do not rewrite history.
