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
| `make harness` | Run the headless product harness end to end under an external watchdog (one deterministic JSON line on stdout; exit code 0 means every scenario passed). |
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

`make coverage` reports `cargo-llvm-cov` coverage for `remotecam-protocol` and
`remotecam-session-core`, and the JaCoCo report for `mobile/protocol`. The 90 %
threshold applies to those delivered product packages; `spikes/` is exempt as
recorded in the plan's Complexity Tracking, and the harness is not a shipped
package (ADR-0008).

## Product harness

`make harness` runs the product-owned headless harness in `harness/`. It is a
separate Cargo workspace pinned to Rust 1.99.1, so `make build-desktop` never
compiles it. The harness drives `desktop/crates/pairing`,
`desktop/crates/session-core` and `desktop/crates/protocol` and prints one
deterministic JSON line per run. GNU `timeout` externally bounds the command to
120 seconds. Exit code 0 means every scenario passed; exit code 1 means at least
one failed.

The product harness generates and decodes a version 1 QR payload in memory,
connects a synthetic mobile client to the loopback TLS 1.3 listener using the
exact pinned certificate, and exchanges `pair`, `paired`, `offer` and `answer`
over the authenticated channel. Since QR v1 excludes loopback addresses, the
synthetic client routes the valid advertised private endpoint's port to the
loopback listener. After `answer`, the harness supplies two deterministic,
in-process synthetic frames 33 ms apart; the Session Core reaches `connected`
only after the first frame, then the session closes cleanly. The opaque
version-1 answer is not parsed for media negotiation. The report includes the
state trace, frame sequence/timestamps, and post-close resource counts; it
excludes the pairing token, fingerprints, and frame payload bytes. The public
`run_with` API also supports a first-frame-timeout scenario: it returns
`outcome: timed_out` at `idle`, records a frame arriving 33 ms after the
3-second deadline as discarded, and reports released resources. Normal runtime
uses real timers while tests drive the logical clock virtually. An integration
test also verifies that a divergent QR pin is rejected after one connection
attempt, with no fallback. The injected clock and RNG make the scenario
reproducible. Nothing under `spikes/` is imported. A failed scenario includes
its reason code in the report (FR-039,
[ADR-0007](docs/adr/0007-harness-location.md),
[ADR-0008](docs/adr/0008-harness-dependency-baseline.md),
[ADR-0009](docs/adr/0009-pairing-crate-and-tls-baseline.md)).
The harness uses newline-delimited signaling messages on its TCP stream solely
to delimit this local scenario; the versioned protocol contract is unchanged.

Successful report line (`outcome: connected` means the state was reached; the
final `observed_state` is `ended` after cleanup):

```json
{"report_version":2,"scenario":"pairing","outcome":"connected","observed_state":"ended","message_sequence":["pair","paired","offer","answer"],"state_trace":[{"from":"idle","to":"pairing_ready","at_millis":0},{"from":"pairing_ready","to":"handshaking","at_millis":0},{"from":"handshaking","to":"negotiating","at_millis":0},{"from":"negotiating","to":"connected","at_millis":33},{"from":"connected","to":"ended","at_millis":66}],"frames":[{"sequence":0,"at_millis":33},{"sequence":1,"at_millis":66}],"resources":{"listeners":{"created":1,"active_after_close":0},"connections":{"created":1,"active_after_close":0},"media_ports":{"created":1,"active_after_close":0},"tasks":{"created":1,"active_after_close":0}},"verdict":"pass","summary":{"scenarios":1,"passed":1,"failed":0}}
```

Timeout scenario report line:

```json
{"report_version":2,"scenario":"first_frame_timeout","outcome":"timed_out","observed_state":"idle","message_sequence":["pair","paired","offer","answer"],"state_trace":[{"from":"idle","to":"pairing_ready","at_millis":0},{"from":"pairing_ready","to":"handshaking","at_millis":0},{"from":"handshaking","to":"negotiating","at_millis":0},{"from":"negotiating","to":"idle","at_millis":3000,"cause":"timeout"}],"frames":[],"late_frame_discarded":{"sequence":0,"at_millis":3033,"reason":"timeout"},"resources":{"listeners":{"created":1,"active_after_close":0},"connections":{"created":1,"active_after_close":0},"media_ports":{"created":1,"active_after_close":0},"tasks":{"created":1,"active_after_close":0}},"verdict":"pass","summary":{"scenarios":1,"passed":1,"failed":0}}
```

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
