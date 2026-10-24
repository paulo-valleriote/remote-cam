# ADR-0003 — F0 code provenance and quarantine

- **Status:** accepted
- **Date:** 2026-10-24
- **Closes:** deviation D4 in `docs/spike/deviations.md`

## Context

FR-039 says spike artifacts must not be merged into product modules, but the F0
implementations were authored inside the product trees so the spike harnesses
could consume them by path. Deviation D4 recorded that and deferred the fix to
the phase that owns each crate. F1 starts now, and leaving `NON-PRODUCTION` code
under `desktop/` or `mobile/` would make the product trees claim code they do
not own.

## Decision

Every F0 implementation marked `NON-PRODUCTION` is moved out of the product
trees and quarantined under `spikes/`. The product trees keep only the delivered
shared contracts and the product shell:

| Before | After | Status |
| --- | --- | --- |
| `desktop/crates/signaling` | `spikes/f0-signaling` | quarantined |
| `desktop/crates/media-core` | `spikes/f0-media-core` | quarantined |
| `desktop/crates/injection` | `spikes/f0-injection` | quarantined |
| `desktop/crates/audio-bridge` | `spikes/f0-audio-bridge` | quarantined |
| `desktop/vendor/dshow-vcam-fallback` | `spikes/f0-dshow-vcam-fallback` | quarantined |
| `mobile/core` | `spikes/f0-mobile-core` | quarantined |
| `mobile/app` | `spikes/f0-mobile-app` | quarantined |
| `desktop/crates/protocol` | unchanged | delivered contract |
| `mobile/protocol` | unchanged | delivered contract |
| `desktop/crates/app` | unchanged | product shell and structured logging; never marked non-production |

Deviation D4 listed `desktop/crates/app` in its scope, but the crate carries no
`NON-PRODUCTION` marker and owns the structured-logging implementation the
product keeps. It therefore stays in the product tree; this ADR narrows D4's
scope explicitly rather than silently.

Quarantine rules:

- Quarantined crates have standalone `Cargo.toml` manifests with exact
  dependency pins. They are not members of the `desktop/` workspace.
- The spike harnesses (`f0-handshake`, `f0-pipeline`, `f0-virtual-audio`) consume
  them by relative path. Nothing under `desktop/` or `mobile/` imports them.
- Quarantined mobile modules are retained as source records. They are not part
  of the `mobile/` Gradle build; `:protocol` is the only module in that build.
- Quarantined code is exempt from the 90 % coverage gate and is not a candidate
  for promotion. Reusing an idea requires a new implementation in the product
  under the normal review rules.
- The F0 evidence, decision records and `specs/001-f0-technical-spikes/` are
  untouched. They describe what was measured before the move; this ADR records
  where the code lives afterwards.

## Consequences

- `make spike-handshake` and the other `spike-*` targets still run, with updated
  path dependencies.
- `make build-desktop` builds the two remaining product crates;
  `make build-mobile` builds the `:protocol` library.
- `desktop/Cargo.toml` no longer carries workspace dependencies that only the
  quarantined crates used.
- D4 is closed. A future "promote the F0 code" request is a product
  implementation task, not a merge from `spikes/`.
