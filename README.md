# RemoteCam

RemoteCam turns an Android phone into a camera and microphone for a Windows 11
computer over the local network. F0 — the risk-reduction phase — is closed; F1
(the session core, protocol implementation and headless harness) is in progress.

> **Non-production code.** Everything under `spikes/` is a disposable spike
> harness or quarantined F0 code. It is clearly marked non-production and MUST
> NOT be merged into the product modules under `desktop/` or `mobile/`
> (FR-039, [ADR-0003](docs/adr/0003-f0-code-quarantine.md)).

## Status

F0 is **closed with exceptions** as of 2026-10-23. The five verdicts, the
evidence and the carried obligations are recorded in the
[F0 gate report](docs/spike/gate-report-f0.md) and in
[`D-EXIT-f0`](spikes/evidence/decisions/D-EXIT-f0.md):

| Gate | Verdict | Disposition |
| --- | --- | --- |
| Virtual camera injection | `no_go` | carried to F3 — only one Google Chrome cell was collected (33.2845 s) |
| Pairing handshake | `conditional` | C-02 waived for F1, deadline 2026-11-20 |
| Desktop receive pipeline | `no_go` | carried to F3 — PC-1 failed on the F0 host |
| Codec interoperability | `conditional` | C-04 waived for F1/F2, deadline 2026-12-20 |
| Virtual audio and licensing | `conditional` | C-05 waived for F2/F3, deadline 2027-01-15 |

F1/F2/F3 are `conditional`, not unblocked. No new matrix evidence was collected
at closure; the remaining injection cells and the reference-host runs are carried
obligations. The phase status, the 13 TDD amendments and the approved deviations
are listed in [`specs/001-f0-technical-spikes/tasks.md`](specs/001-f0-technical-spikes/tasks.md),
[`docs/spike/tdd-amendments.md`](docs/spike/tdd-amendments.md) and
[`docs/spike/deviations.md`](docs/spike/deviations.md).

## Prerequisites

| Tool | Version | Used by |
| --- | --- | --- |
| Rust | 1.99.1 (pinned by `desktop/rust-toolchain.toml` and `harness/rust-toolchain.toml`, MSRV 1.92) | `desktop/`, `harness/` |
| JDK | 17 | `mobile/` |
| Android SDK | compileSdk 36, build-tools 36.0.0 | `mobile/` |
| Gradle | 9.6.x | `mobile/` |
| Python | 3.12 or later | `spikes/tools/` |
| FFmpeg | 8.1 or later | gate measurements |
| GNU Make | current | every documented command |
| GNU coreutils | current | external `timeout` watchdog for the product harness |
| Git | current | clone and review |

On Windows, the Media Foundation crates additionally require the Windows 11 SDK
and the MSVC build tools. The quarantined DirectShow fallback under
`spikes/f0-dshow-vcam-fallback/` is only built when a camera-matrix cell fails
and is not part of the default build.

## Install

```bash
git clone git@github.com:paulo-valleriote/remote-cam.git remotecam
cd remotecam
make help
```

`make help` is the source of truth for the available entry points. Install the
toolchains listed above, then build and test:

```bash
make build-desktop
make build-mobile
make test
```

The Python measurement package is declared in `spikes/tools/pyproject.toml` and
installed through the root Makefile:

```bash
make install
```

## Targets

Every command documented in a README exists as a Makefile target, and every
target below is documented here.

| Target | Purpose |
| --- | --- |
| `make help` | List the available targets. |
| `make install` | Install the Python measurement package and its development dependencies. |
| `make build-desktop` | Build the Rust workspace in `desktop/`. |
| `make harness` | Run both headless product-harness scenarios under an external watchdog (one versioned JSON line on stdout; exit code 0 means both scenarios passed). |
| `make harness HARNESS_ARGS="--seed 42 --clock-start-millis 100"` | Re-run the scenarios with explicit deterministic inputs. |
| `make build-desktop-win` | Mirror the workspace to the Windows filesystem (`/mnt/c/Users/<user>/remotecam-win`) and build natively from WSL2; avoids the UNC filesystem error (see `docs/windows-build.md`). |
| `make sync-desktop-win` | Mirror the working tree to the Windows filesystem without building (helper used by `build-desktop-win`). |
| `make build-mobile` | Assemble the Android contract library (`mobile/protocol/build/outputs/aar`). |
| `make test` | Run the desktop, harness, Android and measurement-tooling tests. |
| `make coverage` | Report coverage for the delivered product packages. |
| `make lint` | Check Rust formatting, run Clippy and lint the Python tooling. |
| `make preflight` | Record the reference environment and verify hardware decode. |
| `make selftest` | Run the measurement-toolchain self-test. |
| `make spike-handshake` | Run the pairing, throttling and residue scenarios. |
| `make spike-pipeline` | Run the full desktop receive pipeline scenario matrix. |
| `make spike-synthetic-source` | Run the timestamped synthetic media source. |
| `make spike-latency` | Run one latency measurement run. |
| `make spike-loss` | Run the pinned loss-and-jitter measurement. |
| `make spike-resources` | Run the resource measurement. |
| `make spike-drift` | Run the drift measurement. |
| `make spike-stall` | Run the media stall recovery scenario. |
| `make spike-injection` | Run the virtual camera injection matrix. |
| `make spike-interop` | Run the codec interoperability scenario. |
| `make spike-audio` | Run the virtual audio scenario. |
| `make gate-report` | Assemble the F0 gate report from the evidence. |
| `make generate-fixtures` | Regenerate the committed synthetic fixtures. |
| `make conformance` | Run the protocol vectors against the Rust and Kotlin implementations. |
| `make validate-evidence` | Validate the recorded evidence against the contracts. |
| `make clean` | Remove generated build artefacts, never committed evidence. |

The gate targets validate their prerequisites and fail with an actionable
message until their harness lands in the phase named in `tasks.md`.

## Repository layout

```text
desktop/    Rust receive/inject application (contract, session core and product shell)
harness/    Product-owned headless harness (own Cargo workspace and toolchain)
mobile/     Kotlin capture/transmit application (contract module)
protocol/   Shared, versioned, language-neutral contract and conformance vectors
spikes/     Non-production F0 harnesses, quarantined F0 code, fixtures, tooling and evidence
docs/       Constitution, ADRs, technical design, spike procedures and the gate report
specs/      Immutable F0 specification record
scripts/    Makefile fragments and CI helpers
GLOSSARY.md Shared domain vocabulary
```

## Testing

`make test` runs five suites:

1. `cargo test` over the `desktop/` workspace;
2. `cargo test` over the `harness/` workspace;
3. `cargo test` over each quarantined Rust crate under `spikes/` and each Rust
   spike harness;
4. the Android contract module's unit tests with JUnit 5;
5. `pytest` over the spike tooling.

`make coverage` reports `cargo-llvm-cov` coverage for `remotecam-protocol`,
`remotecam-session-core`, `remotecam-pairing` and `remotecam-harness`, and the
JaCoCo report for `mobile/protocol`. The ≥90% line threshold applies to the
delivered packages and, as a REC-25-specific gate, to the non-shipped Harness.
`spikes/` remains exempt as recorded in the plan's Complexity Tracking.

## Product harness

`make harness` runs the product-owned headless Harness in its separate Cargo
workspace, pinned to Rust 1.99.1; `make build-desktop` never compiles it. The
Harness drives `desktop/crates/pairing`, `desktop/crates/session-core` and
`desktop/crates/protocol`, then prints one compact JSON line with
`report_version: 4` and `contract_version: 1`. GNU `timeout` bounds execution
to 120 seconds. Exit code 0 means all nine scenarios passed; exit code 1 means
at least one scenario failed.

The report envelope contains the Git commit, seed, injected logical clock
start, an allowlisted environment (`os`, `architecture`, `rustc_version`),
ordered scenario results, verdict and summary. It excludes wall-clock time,
hostnames, ephemeral ports, absolute paths, Pairing Tokens, certificate
fingerprints and frame payloads. Failure to resolve the commit or run
`rustc --version` is explicit and emits no report.

The `pairing-happy-path` scenario decodes a version 1 QR payload, completes the
loopback TLS 1.3 handshake with the exact pinned certificate, exchanges `pair`,
`paired`, `offer` and `answer`, supplies two synthetic frames 33 logical
milliseconds apart, then closes cleanly. The Session Core reaches `connected`
only after the first frame. Its expected result is `verdict: pass`,
`outcome: accepted`, `reason_code: null`.

The `pin-mismatch-aborts` scenario presents the client with a deliberately
different TLS pin. It expects the handshake to abort before the client sends
the Pairing Token, with `verdict: pass`, `outcome: rejected` and
`reason_code: certificate_pin_mismatch`. The reason code already belongs to
protocol v1; the scenario adds no wire change.

The seven REC-10 acceptance scenarios run after those two, each with an
injected logical clock and a fixed expected result (see ADR-0012):

- `qr-expiry-refused`: a peer that connects after the 120 s window is closed
  before TLS, with no bytes sent; the desktop regenerates a QR at epoch 2.
- `token-expiry-refused`: a valid QR whose token is presented after the window
  closed is refused with `token_expired`.
- `single-use-refused`: a second use of a token, while the first session is
  still negotiating, is refused with `superseded`.
- `replay-refused`: a captured QR replayed after its session timed out is
  refused with `token_already_used`.
- `stale-epoch-discarded`: a frame with an epoch older than the active session
  is discarded and counted as one protocol violation; the session continues.
- `clock-backwards-window`: a backward clock step neither invalidates the
  window nor extends it.
- `regeneration-after-refusal`: after `qr_expired`, a fresh QR is accepted and
  completes the pairing handshake.

Pass `HARNESS_ARGS` to reproduce a run with chosen inputs:

```bash
make harness HARNESS_ARGS="--seed 42 --clock-start-millis 100"
```

Defaults are seed `0` and logical clock start `0`. Runs with the same commit,
seed and clock start are byte-identical except for `environment`. The complete
report shape is specified by [`harness/report.schema.json`](harness/report.schema.json)
and checked in tests against the emitted JSON; the versioned golden fixture is
[`report.json`](harness/crates/harness/tests/fixtures/report.json). The public
`run_with` API retains the first-frame-timeout scenario for deterministic
Session Core diagnostics; it is not part of the default two-scenario report.
Nothing under `spikes/` is imported (FR-039,
[ADR-0007](docs/adr/0007-harness-location.md),
[ADR-0008](docs/adr/0008-harness-dependency-baseline.md),
[ADR-0009](docs/adr/0009-pairing-crate-and-tls-baseline.md),
[ADR-0010](docs/adr/0010-harness-report-contract-and-coverage.md)).
The harness uses newline-delimited signaling messages on its TCP stream solely
to delimit this local scenario; the versioned protocol contract is unchanged.

## Debugging

- `make build-desktop` uses the debug profile, so the Rust workspace keeps
  symbols and full backtraces. Failures of the gate targets point to the
  procedure and the evidence file named in
  [`specs/001-f0-technical-spikes/quickstart.md`](specs/001-f0-technical-spikes/quickstart.md) §12.
- `make build-mobile` produces the contract AAR. `make test` runs the Kotlin
  unit tests without a device; the product Android application arrives in F2.
- The F0 harnesses are debugged from the evidence they emit under
  `spikes/evidence/`. Each gate procedure in `docs/spike/` names the file to
  inspect when a measurement fails.
- The product harness prints its JSON report to stdout; when `make harness`
  exits non-zero, the report names the failing scenario and the observed state.
- Structured logs are English-only and never contain tokens, full fingerprints,
  secrets or personal data.

## Documentation

- [Constitution](docs/constitution.md)
- [Glossary](GLOSSARY.md)
- [Architecture decisions](docs/adr/README.md)
- [F0 specification](specs/001-f0-technical-spikes/spec.md)
- [F0 plan](specs/001-f0-technical-spikes/plan.md)
- [F0 research and decisions](specs/001-f0-technical-spikes/research.md)
- [F0 quickstart](specs/001-f0-technical-spikes/quickstart.md)
- [F0 contracts](specs/001-f0-technical-spikes/contracts/README.md)
- [F0 tasks](specs/001-f0-technical-spikes/tasks.md)
- [Desktop instructions](desktop/README.md)
- [Mobile instructions](mobile/README.md)
- [Spike policy and evidence layout](spikes/README.md)
