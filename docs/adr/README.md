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

Add a new ADR by scanning this directory for the highest number and
incrementing it. To revisit a decision, write a new ADR that supersedes the old
one; do not rewrite history.
