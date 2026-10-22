# F0 — pairing handshake (gate 2)

**NON-PRODUCTION** — F0 risk-reduction spike. Nothing here merges into
`desktop/` or `mobile/` (FR-039).

Gate 2 asks whether the pairing handshake authenticates both sides, resists
interception and replay, throttles repeated failures, and leaves no pairing
state behind. The procedure is
[`docs/spike/gate-2-pairing-handshake.md`](../../docs/spike/gate-2-pairing-handshake.md).

## Layout

| Path | What it is |
| --- | --- |
| `src/main.rs` | Harness entry point: runs the matrix, writes the evidence, dispatches residue |
| `src/scenarios.rs` | The 13 handshake scenarios, driven over real TLS 1.3 on loopback |
| `src/throttle.rs` | The three throttling scenarios, including a legitimate second origin |
| `src/residue.rs` | Residue byte scanner and the state-reset token replay |
| `src/trace.rs` | Per-scenario trace writer (traces are local-only) |
| `residue_scan.py` | Standalone scanner for the reference-device procedure |
| `tests/test_residue_scanner.py` | Proves the scanner detects a seeded artefact and then a clean tree |
| `scenarios/` | The pinned scenario table and expected codes |

## What runs

`make spike-handshake` executes every handshake and throttling scenario against
the real listener (`desktop/crates/signaling`) and the real pinned client. No
TLS or token check is mocked. It writes:

* `spikes/evidence/pairing/pairing_scenarios.json` — one entry per scenario, with
  outcome, closed reason code, `media_flowed_before_authentication: false`,
  empty `residual_credentials`, `established_channel_ms` and
  `pinning_added_delay_ms` for the valid path;
* `spikes/evidence/pairing/pairing_throttling.json` — the per-origin decision
  stream, the 6th-attempt refusal, the progressive block, listener closure and
  the unaffected second origin;
* one `.trace` per scenario (raw capture, gitignored).

The residue evidence (`pairing_residue.json`) is only emitted when the run names
both reference devices and passes `--restart-observed`:

```bash
cargo run --manifest-path spikes/f0-handshake/Cargo.toml -- \
  --device desktop:windows-11-reference:/path/to/app-data \
  --device mobile:android-reference:/path/to/app-data \
  --needle pairing_token=<64 hex> --needle fingerprint=<64 hex> \
  --restart-observed
```

Without both devices the harness prints why it did not emit the file and exits
without writing a contract artefact. That is deliberate: the contract requires
both devices inspected with the same method, and a single-device file would be
inadmissible evidence.

## Reading the result

A scenario "passes" only when the **observed** closed code is the one the
contract names. The evidence keeps `expectation_met` next to each scenario so a
reviewer can see a mismatch instead of inferring it. `make spike-handshake`
exits non-zero when any expectation is unmet; the JSON is still written so the
failure can be diagnosed.
