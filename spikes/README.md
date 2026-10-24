# F0 Spikes — NON-PRODUCTION

**NON-PRODUCTION.** Every file under `spikes/` is a disposable F0 spike
artefact. Spike code MUST NOT be merged into the product modules under
`desktop/` or `mobile/` (FR-039). It is excluded from the ≥ 90 % unit-coverage
gate because its quality evidence is the recorded procedure plus the per-gate
evidence, not unit coverage; that deviation is recorded in
`specs/001-f0-technical-spikes/plan.md` (Complexity Tracking) and requires
maintainer approval.

## Layout

| Directory | Purpose |
| --- | --- |
| `f0-handshake/` | TLS 1.3 pairing scenarios, throttling, residue inspection and the security traces. |
| `f0-pipeline/` | Synthetic source, seeded impairment relay, latency, resource, drift and stall harnesses. |
| `f0-camera-injection/` | Per-application virtual camera matrix and consumer harnesses. |
| `f0-codec-interop/` | Android-to-desktop codec negotiation and corruption comparison. |
| `f0-virtual-audio/` | Virtual audio render, capture, dropout and intelligibility workflow. |
| `f0-signaling/` | Quarantined F0 signaling crate: TLS 1.3 WSS listener, pinned client, token store, throttling. |
| `f0-media-core/` | Quarantined F0 media crate: receive-only peer connection, jitter policy, decode adapters, handoff. |
| `f0-injection/` | Quarantined F0 injection crate: NV12 ring buffer, Media Foundation virtual camera, registration. |
| `f0-audio-bridge/` | Quarantined F0 audio crate: WASAPI render, readiness and degraded operation. |
| `f0-dshow-vcam-fallback/` | Quarantined DirectShow virtual-camera fallback (C++/CMake). |
| `f0-mobile-core/` | Quarantined F0 Android core module (source record, not built). |
| `f0-mobile-app/` | Quarantined F0 Android app module (source record, not built). |
| `tools/` | Python measurement tooling: analysis, validation, metrics and report assembly. |
| `fixtures/` | Versioned synthetic fixtures, including the 40/60/120 ms calibration capture. |
| `evidence/` | Generated evidence. Raw captures are local-only; derived metrics, scripts and procedures are versioned. |

## Quarantine

The `f0-signaling/`, `f0-media-core/`, `f0-injection/`, `f0-audio-bridge/`,
`f0-dshow-vcam-fallback/`, `f0-mobile-core/` and `f0-mobile-app/` directories
were moved out of `desktop/` and `mobile/` in F1-01. They are retained for
provenance and reproducibility only:

- the Rust crates are standalone packages with exact dependency pins, consumed
  by the spike harnesses by relative path;
- the mobile modules are source records, are not part of the `mobile/` Gradle
  build, and are not built;
- none of them may be imported by `desktop/` or `mobile/`, and none is a merge
  source for the product.

See [ADR-0003](../docs/adr/0003-f0-code-quarantine.md).

## Evidence layout

The evidence root follows
`specs/001-f0-technical-spikes/contracts/measurement-formats.md` §10:

```text
spikes/evidence/
├── environment.json
├── toolchain_selftest.json
├── pairing/
│   ├── pairing_scenarios.json
│   ├── pairing_throttling.json
│   ├── pairing_residue.json
│   └── <scenario_id>.trace
├── pipeline/
│   ├── latency_run1_samples.csv
│   ├── latency_run1_summary.json
│   ├── latency_run2_samples.csv
│   ├── latency_run2_summary.json
│   ├── latency_run3_samples.csv
│   ├── latency_run3_summary.json
│   ├── stage_budget.json
│   ├── resource_samples.csv
│   ├── resource_summary.json
│   ├── drift_samples.csv
│   ├── drift_summary.json
│   ├── impairment_profile.json
│   ├── stall.json
│   └── relay_log.jsonl
├── injection/
│   └── injection_matrix.json
├── audio/
│   ├── aud-01/
│   │   ├── consumer_capture.wav     # local only, not committed
│   │   ├── dropouts.json
│   │   └── intelligibility.json
│   └── relay_log.jsonl
├── licensing/
│   └── licensing_bundle.json
├── decisions/
│   └── <decision_id>.md
└── open_questions.json
```

## Rules

1. **Raw captures stay local.** Uncompressed audio, video, screen recordings,
   traces and Event Trace Log files are listed in `.gitignore`. Derived metrics,
   analysis scripts and procedures are versioned.
2. **No secrets and no personal data.** No tokens, private keys, full
   fingerprints, absolute user paths, user names or real user media may appear
   in any artefact. The redaction filter in `tools/redaction.py` is applied to
   every generated file.
3. **Reproducible.** Each gate records the exact command, the environment and the
   procedure reference. A second engineer must be able to repeat it from
   `docs/spike/`.
4. **Non-production.** No spike module is imported by a product crate or module,
   and no spike code is promoted without a new implementation task in the
   product tree.

## Running

The gate harnesses are invoked through the root Makefile only:

```bash
make preflight
make selftest
make spike-handshake
make spike-pipeline
make spike-injection
make spike-interop
make spike-audio
make gate-report
```

The procedures and the expected evidence are in `docs/spike/` and
`specs/001-f0-technical-spikes/quickstart.md`.
