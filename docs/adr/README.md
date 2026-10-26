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

Add a new ADR by scanning this directory for the highest number and
incrementing it. To revisit a decision, write a new ADR that supersedes the old
one; do not rewrite history.
