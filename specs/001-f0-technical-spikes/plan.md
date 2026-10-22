# Implementation Plan: F0 — Risk-Reduction Spikes and Technical Decisions

**Branch**: `main` (git; the speckit identifier `001-f0-technical-spikes` is a derived directory label, not a git branch — no branch hook is configured) | **Date**: 2026-10-22 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-f0-technical-spikes/spec.md`

**Note**: This plan covers **Phase 0 (research) and Phase 1 (design + contracts)**. It does not contain the task breakdown; that belongs to `/speckit.tasks` (`tasks.md`).

## Summary

F0 is the risk-reduction phase for RemoteCam V1. It produces **five gate verdicts** (virtual camera injection, desktop receive pipeline, codec interoperability, pairing handshake, virtual audio + licensing) issued at a single phase review no later than **2026-11-06**, plus the durable artefacts those verdicts depend on: the decision records, the measurement procedures, and the raw evidence.

The technical approach is **Kotlin for the Android capture/transmit app and Rust for the Windows 11 receive/inject app**, as mandated by the product owner and already fixed by the source technical design (`docs/tdd-remotecam-v1.md` §5.1, §14). F0 does **not** build the product. It builds **disposable, clearly non-production harnesses** that exercise the real risk surfaces — WebRTC receive-only pipeline with a custom jitter buffer, Media Foundation hardware H.264 decode, the Windows 11 virtual camera backend, a TLS 1.3 fingerprint-pinned handshake, and a third-party virtual audio device — and it records measurements that a second engineer can reproduce in one working day.

Two decisions from Phase 0 research change the shape of the phase materially and are recorded as **D-01** and **D-02** in [research.md](./research.md):

- **The virtual camera backend changes.** The Windows 11 `MFCreateVirtualCamera` API (Windows 11 build 22000+, user-mode, no kernel driver, MIT-licensed Microsoft sample) becomes the **primary** backend; the DirectShow filter the source TDD planned as primary is demoted to a **fallback only**. This removes an entire driver/WHQL/licensing risk from V1.
- **The virtual audio path cannot be fully owned in V1.** Windows 11 provides no OS-level virtual microphone. A third-party virtual audio driver is structurally unavoidable for the audio half of V1, and shipping our own signed WDM driver costs 60–90 dev-days plus an EV certificate and a Partner Center attestation pipeline — roughly a third of the entire V1 budget. The recommended V1 shape is therefore **VB-CABLE, user-installed, link-only, no redistribution**, with a **video-only V1** as the approved fallback.

The user input *"use kotlin para mobile e rust para desktop"* is consistent with the source technical design and is adopted as **D-01**; it is recorded here so the decision is traceable rather than implicit.

---

## Clarifications adopted

The spec carries **nine clarifications** from the session of 2026-10-22. Each is adopted here as a plan constraint and traced to the artefact that carries it. None of them is left implicit, and none is re-opened by the plan.

| # | Clarification (abridged) | Plan anchor | Artefacts that carry it |
| --- | --- | --- | --- |
| Q1 | Hardware H.264 decode is a **verified precondition**: verify it before starting, and swap the reference machine if absent. | Precondition **PC-1**; Technical Context *Performance Goals* | `research.md` D-05; `data-model.md` §2.4; `quickstart.md` §3; `contracts/measurement-formats.md` §1 |
| Q2 | The five gates run in **dependency-ordered waves**, and all five verdicts are issued at **one phase review**. | **Gate waves** table below; Technical Context *Scale/Scope* | `data-model.md` §1.1; `contracts/gate-report.schema.json`; `quickstart.md` §0 |
| Q3 | "Continuous and intelligible audio" is measured by **automated silence/dropout detection on the recording** plus a **scripted human intelligibility check**. | Technical Context *Performance Goals*; Precondition **PC-3** | `research.md` D-21, D-22; `data-model.md` §2.10–§2.11; `quickstart.md` §7.3, §9.1 |
| Q4 | **One accountable role per gate**, named in the report; a conditional verdict **blocks the phases it conditions** unless that role records an explicit waiver with a deadline. | **Gate waves** table (role column); Constitution Check | `data-model.md` §1.2, §1.4; `contracts/gate-report.schema.json` |
| Q5 | **One named** third-party virtual audio device is pinned for the technical measurement; candidate alternatives' licence terms are evaluated **in parallel on the same test steps**. | Technical Context *Primary Dependencies* (VB-CABLE pin) | `research.md` D-12; `data-model.md` §3.3; `quickstart.md` §1.4, §9.3 |
| Q6 | The video latency gate judges the **raw capture-at-consumer p95 (≤ 150 ms)**; the **measured harness floor** and the **floor-corrected value** are published alongside it. | Precondition **PC-2**; Technical Context *Performance Goals* | `research.md` D-20, D-29; `data-model.md` §2.6–§2.7; `contracts/measurement-formats.md` §2 |
| Q7 | The loss-and-jitter test uses exactly **one nominal scenario**: 3 % uniform loss, 30 ms added latency, ±10 ms jitter. | Precondition **PC-4** | `research.md` D-30; `data-model.md` §2.3; `quickstart.md` §7.3 |
| Q8 | F0 gates **video latency only**; audio latency is measured in F2/F3 once the real mobile capture path exists. | Technical Context *Performance Goals* (audio latency is **not** gated); *Scope exclusions* below | `research.md` D-31; `data-model.md` §7 metrics table |
| Q9 | The pairing gate also proves a **per-origin attempt limit**, **progressive blocking** and **listener shutdown** after consecutive failures, using the source thresholds. | Precondition **PC-5** | `research.md` D-32; `data-model.md` §3.7; `contracts/reason-codes.md` §1.3; `quickstart.md` §6.3 |
| Q10 | The pairing gate proves **zero pairing artefacts** after a normal session end **and** after an application restart, with the **inspection method stated**. | Precondition **PC-6**; Technical Context *Storage* (no session state) | `research.md` D-33; `data-model.md` §3.8; `quickstart.md` §6.4 |

---

## Gate waves

Fixed by FR-045 and FR-046. A `no-go` in one wave **does not** stop a gate that does not depend on it; a gate whose dependency failed still receives a recorded verdict naming the blocking dependency (FR-047).

| Wave | Gate | Depends on | Accountable role | Target date |
| --- | --- | --- | --- | --- |
| 1 | Virtual camera injection | — | Technical lead | 2026-11-06 |
| 1 | Pairing handshake | — | Security reviewer | 2026-11-06 |
| 2 | Desktop receive pipeline | Virtual camera injection, Pairing handshake | Technical lead | 2026-11-06 |
| 3 | Codec interoperability | Desktop receive pipeline | Technical lead | 2026-11-06 |
| 3 | Virtual audio and licensing | Desktop receive pipeline | Product owner | 2026-11-06 |

All five verdicts are issued at **one** phase review on or before **2026-11-06**, sharing the same review timestamp.

## Technical Context

**Language/Version**

| Component | Language / version | Notes |
| --- | --- | --- |
| Mobile app | **Kotlin 2.4.x** (JVM target 17) | Native Android, single language for UI, camera, encode and signaling. |
| Desktop app | **Rust edition 2024**, MSRV **1.92** | MSRV is driven by `slint` 1.18.1. Pinned by `rust-toolchain.toml`. |
| Shared contract | **Language-neutral** (JSON Schema + binary layout spec) | Source of truth in `protocol/`; Kotlin and Rust each carry their own generated/hand-written consumer. No cross-language imports. |
| Vendor component | **C++ (MSVC)** | *Fallback only* — DirectShow virtual camera filter. Not built on the primary path. |
| Measurement tooling | **Python 3.12+**, FFmpeg ≥ 8.1 | Offline analysis of recorded evidence. Not shipped. |

**Primary Dependencies**

*Desktop (Rust):* `webrtc` 0.21.1 (pinned exactly — 0.x minor bumps are breaking), `rustls` 0.23.46, `rcgen` 0.14.10, `tokio` 1.54.1 (1.54.0 is yanked), `tokio-tungstenite` 0.30.0, `slint` 1.18.1, `windows` 0.62.2 (the 0.100.0 umbrella crate is **not published**), `qrcode` 0.14.1, `sha2` 0.11.1, `rand` (seeded), `openh264` 0.9.9 (CI/software fallback only), `criterion` 0.8.2.

*Mobile (Kotlin):* `androidx.camera` 1.6.2 (CameraX), `io.github.webrtc-sdk:android:150.7871.01` (libwebrtc — there is **no official prebuilt AAR**), `com.google.zxing:core` 3.5.4, `kotlinx-serialization`, Android Gradle Plugin 9.4.x / Gradle 9.6.x / JDK 17, `minSdk` 29 / `targetSdk` 36 / `compileSdk` 36.

*Windows vendor components:* MIT `microsoft/Windows-Camera` `Samples/VirtualCamera` (primary camera backend). Third-party virtual audio driver: **VB-CABLE** base package, user-installed (no redistribution in V1).

*Measurement tooling:* FFmpeg ≥ 8.1 (`silencedetect`, `qrencode`, `gdigrab`, `astats`), Python `numpy` 2.5.4 / `scipy` 1.18.1 / `zxing-cpp` 3.1.1 / `jiwer` 4.0.0 / `soundfile` 0.14.0 / `pystoi` 0.4.1 / `pyloudnorm` 0.2.0, `obs-websocket` 5.7.x, `iperf3`, `iproute2` (`tc netem`, Linux alternative).

**Storage**: **None for session state — by design.** Invariant I3 and FR-005 forbid persisting any session key, token, fingerprint, pairing record or media, both after a normal session end **and** after an application restart (Q10, PC-6). The inspection method used to prove it is the same in both cases and is recorded with the evidence. The only bytes written to disk are (a) measurement evidence under `spikes/evidence/` and (b) the COM registration artifact for the virtual camera, which is an installation artifact and not session state.

**Testing**: `cargo test` + `criterion` benches (desktop, `cargo-llvm-cov` for coverage), JUnit 5 / `kotlin-test` + Robolectric (mobile, JaCoCo for coverage), **cross-language conformance vectors** executed by both the Kotlin and Rust protocol implementations, Linux CI integration harness, Windows 11 runner for injection and audio gates. Coverage gate **≥ 90 %** per delivered package (constitution II).

**Target Platform**: Android 10 (API 29) or later phone; Windows 11 x64 (build 22000+ minimum for the MF virtual camera, 22621+ recommended); Linux x64 CI for the protocol and media core.

**Project Type**: mobile app + desktop app + shared versioned protocol contract + non-production spike harness.

**Performance Goals** (from `docs/tdd-remotecam-v1.md` §5.7 and §16, restricted to the F0 scope):

| Metric | Target | Gated in F0? |
| --- | --- | --- |
| Capture-at-consumer video latency (raw p95) | ≤ 150 ms (budget ≈ 100 ms) | **Yes** — the raw measured value is the one judged against the threshold |
| Measured harness floor | reported alongside the raw value | No — reported as the uncertainty on the gate figure |
| Floor-corrected capture-at-consumer latency | reported alongside the raw value | No — formerly "glass-to-glass"; reported, never substituted for the raw value |
| Audio latency | ≤ 120 ms p95 | **No** — out of F0 scope (Q8); measured in F2/F3 once the real mobile capture path exists |
| Pairing time (scan → first frame) | ≤ 5 s p95 | No — reported (FR-007) |
| Desktop CPU @ 720p30 | ≤ 15 % | **Yes** |
| Desktop memory @ 720p30 | ≤ 400 MB | **Yes** |
| A/V drift | ≤ 20 ms per hour | **Yes** |
| Injection continuity | 30 min @ 1280×720@30, no stall > 2 s | **Yes** |
| Audio continuity | 10 min under the nominal impairment profile, zero dropouts ≥ 50 ms | **Yes** |

**Preconditions** — a precondition that fails stops the gate it guards; it does not stop the gates that do not depend on it.

| # | Precondition | Source | If it fails |
| --- | --- | --- | --- |
| PC-1 | The reference machine provides **hardware-accelerated H.264 decode** for the codec under test, **verified before** the pipeline gate runs and recorded as evidence. | Q1, FR-044 | The reference machine is **replaced**, not measured on. Software-decode results are reported separately and can never pass the latency gate. |
| PC-2 | The video latency figure is judged on the **raw capture-at-consumer p95**; the measured harness floor and the floor-corrected value are published alongside it. | Q6, FR-010, FR-011 | A figure published without the floor is invalid evidence. A failure caused by the capture procedure is distinguished from a product regression. |
| PC-3 | Audio continuity and intelligibility are judged by **automated silence/dropout detection on the recorded consumer capture** plus a **scripted human intelligibility check** whose phrase script, observer and outcome are recorded. | Q3, FR-029, FR-030 | A run with no dropout analysis, or with no recorded observer, is not admissible. |
| PC-4 | The loss-and-jitter test uses exactly **one nominal impairment profile**: 3 % uniform loss, 30 ms added latency, ±10 ms jitter, applied to the media stream only. | Q7, FR-012, FR-030 | A run that deviates from the profile — **including burst loss** — is judged on detected dropouts rather than on average loss percentage and is never used to pass the gate. |
| PC-5 | The pairing gate proves a **per-origin attempt limit of 5 attempts per minute**, **progressive blocking**, and **listener shutdown after 10 consecutive failures**, each throttling decision recorded with its reason code and outcome. | Q9, FR-049 | The pairing gate cannot pass; the missing evidence is reported as `evidence_incomplete`. |
| PC-6 | The pairing gate proves **zero pairing artefacts** after a normal session end **and** after an application restart, using the **same stated inspection method** in both cases. | Q10, FR-005 | A residue is a hard failure of the pairing gate, independent of the verdict. |

**Constraints**: local network only (no cloud, no STUN/TURN, no relay); TLS 1.3 + DTLS-SRTP with no insecure fallback; no kernel-mode driver of our own; no session persistence; default 1280×720@30 with 1920×1080@30 as optional maximum; no secrets, no personal data, no real user media in any artefact; all deliverables in English.

**Scale/Scope**: one-to-one sessions; five gates in three dependency-ordered waves; ≈ 20 dev-days (TDD §7); 5 target application categories; 1 Android reference device; 1 Windows 11 reference machine; all five verdicts issued at a single phase review by 2026-11-06.

**Scope exclusions** (from the spec's *Out of Scope (F0)*, restated as plan constraints so that no artefact drifts back into them):

- **End-to-end audio latency** is not measured and not gated in F0 (Q8). It requires the real mobile capture path and is deferred to F2/F3. F0 measures audio **continuity**, **intelligibility** and **drift** — never audio latency.
- No production user interfaces, installers, updaters, signing, store distribution or firewall configuration.
- No full protocol implementation, session state machine or pairing QR experience — F0 validates the handshake design only.
- No complete Android application, camera controls, foreground service or thermal policy, and no device matrix beyond the interop reference device.
- No full desktop application shell, diagnostics panel, driver wizard or packaging.
- No long-duration soak testing, accessibility validation, fuzzing, or the full device compatibility matrix.
- No kernel-mode driver development and no custom audio driver.
- No cloud services, internet transport, relaying or non-local-network scenarios.

**Clarifications resolved in Phase 0**: all unknowns listed in the template (language/version, dependencies, storage, testing, platform, project type, performance goals, constraints, scale) were marked *NEEDS CLARIFICATION* before research. Every one is now resolved above and backed by a decision entry in [research.md](./research.md). The nine clarifications from the spec session of 2026-10-22 are adopted in the **Clarifications adopted** table above and are carried by the preconditions PC-1…PC-6; no `NEEDS CLARIFICATION` marker remains.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle / rule | Requirement | Status | Evidence / action |
| --- | --- | --- | --- |
| I. Code Quality — language | All production code and **all documentation** in English | ✅ PASS | This plan, `research.md`, `data-model.md`, `quickstart.md` and every file under `contracts/` are in English. FR-041 enforces the same for the report. |
| I. Code Quality — interface-first | Public contract defined before implementation; must not leak technology choices | ✅ PASS | `protocol/` is the versioned contract; `contracts/` under this plan defines the message schema, QR layout, reason codes, matrix and report formats **before** any harness code. `data-model.md` defines the domain entities independently of Rust/Kotlin types. |
| I. Code Quality — dependency injection | No module imports SDKs/clients/drivers/UI frameworks directly | ✅ PASS | The desktop pipeline isolates `windows`-crate Media Foundation and WASAPI calls behind adapter traits (see `desktop/crates/media-core` boundary in Project Structure); the mobile app isolates CameraX and libwebrtc behind capture/transport interfaces. |
| I. Code Quality — dependency justification | No external library without a recorded justification (problem, alternatives, maintenance cost, license) | ✅ PASS (action) | A **dependency ledger** is recorded in [research.md](./research.md) §"Dependency ledger" with problem solved, alternatives discarded, maintenance cost and license for every non-trivial dependency. Mandatory in the change description before merge. |
| I. Code Quality — naming, comments, docs | Meaningful complete names; comments avoided; extensive docs in `docs/` | ✅ PASS | Prototype code follows the same rules. Procedure write-ups live in `docs/spike/`, not in comment blocks. |
| I. Code Quality — accidental complexity | Simpler solution preferred | ⚠️ JUSTIFIED EXCEPTION | Two deviations are justified below (custom jitter buffer; custom MF media source). Both are unavoidable: `webrtc-rs` ships no NetEq and no decoder, and no supported user-mode virtual camera source exists other than our own COM implementation. |
| II. Testability — coverage | ≥ 90 % in every **delivered** package, verified automatically before merge | ⚠️ JUSTIFIED EXCEPTION | `protocol/` is a delivered shared contract → ≥ 90 % enforced in CI. `spikes/` is a **non-production prototype** (FR-039) and is explicitly *not* merged into product modules, so the unit-coverage gate does not apply to it; its quality gate is the recorded procedure + evidence per gate (FR-006, FR-038). See Complexity Tracking. |
| II. Testability — deterministic unit tests | Domain logic tested without network, real clock or shared state | ✅ PASS | Protocol, jitter-buffer policy, drift estimator, dropout detector and QR codec are all pure and clock/RNG-injected. |
| II. Testability — integration/E2E for user-facing features | Both success and error paths covered | ✅ PASS | The five gates *are* the integration/E2E suite for F0: every adversarial pairing scenario, every loss/jitter/stall path, every target application in the injection matrix. The throttling scenarios (Q9) and the residue inspections (Q10) are part of that suite, not separate unit tests. |
| II. Testability — concurrency test | Code with shared state or concurrency must have a race test | ✅ PASS | The receiver pipeline (jitter buffer, ring buffer producer/consumer, session replacement) is concurrent; a concurrency test exercises event ordering and idempotency. |
| II. Testability — leak test | Long-lived components must have a release test | ✅ PASS | Session teardown, COM object release and ring-buffer shutdown are asserted to release handles/memory (also FR-005's no-residue requirement). |
| II. Testability — synthetic fixtures | No production data; fixtures versioned alongside the test | ✅ PASS | Synthetic timestamped video, scripted speech and the **calibration fixture with known 40/60/120 ms gaps** are committed and versioned. |
| III. Maintainability — structured logs | Consistent, English, no secrets or personal data | ✅ PASS | One structured-log schema shared by desktop and mobile; tokens and full fingerprints are never logged (TDD §8.4). |
| III. Maintainability — metric purpose | Every metric has a declared purpose and consumer | ✅ PASS | Each metric in `data-model.md` names its consumer (gate criterion, report section, or local alert). Speculative metrics rejected. |
| III. Maintainability — READMEs and Makefile | Root, `desktop/` and `mobile/` READMEs; root Makefile with run/test/coverage/lint/debug targets | ✅ PASS (action) | `README.md`, `desktop/README.md`, `mobile/README.md` and the root `Makefile` are part of the F0 delivery scope and are updated in the same delivery as any behaviour change. |
| III. Maintainability — diagnosability | Failure diagnosis possible from documented logs/metrics/tests only | ✅ PASS | Every gate artifact links to a log file, a sample file and a procedure section; ad-hoc instrumentation is not required. |
| Structure — first-level directories | New first-level directories must be justified in review | ⚠️ JUSTIFIED EXCEPTION | `protocol/` and `spikes/` are new first-level directories. Justification recorded in Complexity Tracking and requires maintainer approval. |
| Structure — self-containment | `desktop/` and `mobile/` must not import each other's source | ✅ PASS | The shared contract lives in `protocol/` as data + vectors, not as importable source. Each side implements it independently against the same conformance vectors. |
| Structure — docs location | Platform docs in `platform/docs/`; README stays an execution guide | ✅ PASS | `desktop/docs/`, `mobile/docs/`, `docs/spike/`. |
| Structure — .gitignore | Secrets, env files, build artifacts, caches, generated files ignored | ✅ PASS | Evidence captures are large binaries → ignored by default, committed only as derived metrics + scripts. No secret is ever versioned; examples use explicitly invalid values. |
| Workflow — review, gates, dependency justification, proportionality | Every change reviewed against I–III; merge blocked on failing tests, coverage < 90 %, lint error, or stale docs | ✅ PASS | F0 artefacts are reviewed as a unit; the five gate verdicts are the review outcome. Complexity is proportional: F0 builds harnesses, not product features. |
| Governance — deviations | Deviations must record justification, scope and rollback plan and be approved by the responsible maintainer | ⚠️ ACTION REQUIRED | The two exceptions above (spike coverage, custom jitter buffer + media source) are recorded in Complexity Tracking with scope and rollback. **They require maintainer approval before merge.** |
| Governance — spec clarifications are adopted, not re-opened | The nine clarifications of the 2026-10-22 session are adopted as plan constraints and carried by an artefact | ✅ PASS | Each clarification has a row in the **Clarifications adopted** table with a plan anchor and the artefacts that carry it. None is re-opened, none is resolved differently, and none lacks an owner. |

**Gate result: PASS, conditional on maintainer approval of the two recorded deviations.** No unresolved clarification remains.

## Project Structure

### Documentation (this feature)

```text
specs/001-f0-technical-spikes/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output — decisions, dependency ledger, open questions
├── data-model.md        # Phase 1 output — entities, fields, validation rules, state transitions
├── quickstart.md        # Phase 1 output — runnable end-to-end validation guide
├── contracts/           # Phase 1 output — interface and artefact contracts
│   ├── README.md
│   ├── signaling-messages.schema.json
│   ├── qr-payload.md
│   ├── reason-codes.md
│   ├── injection-matrix.schema.json
│   ├── gate-report.schema.json
│   └── measurement-formats.md
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
remotecam/
├── desktop/                    # Rust desktop application (Windows 11 receiver)
│   ├── Cargo.toml              # workspace root
│   ├── rust-toolchain.toml     # pins edition-2024 toolchain, MSRV 1.92
│   ├── crates/
│   │   ├── protocol/           # Rust implementation of the shared signaling contract
│   │   ├── signaling/          # WSS server over TLS 1.3, ephemeral cert, token validation
│   │   ├── media-core/         # recvonly peer connection, jitter buffer, decode, A/V sync
│   │   │   └── src/adapters/   # Media Foundation / WASAPI / null-sink adapters (DI seams)
│   │   ├── injection/          # NV12 ring buffer producer + MF virtual camera media source
│   │   ├── audio-bridge/       # WASAPI render into the virtual audio endpoint
│   │   └── app/                # Slint UI shell: QR, diagnostics, device readiness
│   ├── vendor/
│   │   └── dshow-vcam-fallback/# C++ DirectShow filter — FALLBACK backend only, not primary
│   ├── docs/
│   └── README.md
├── mobile/                     # Kotlin Android application (capture + transmit)
│   ├── settings.gradle.kts
│   ├── gradle/libs.versions.toml  # pinned dependency versions (single source)
│   ├── app/                    # UI, permissions, foreground service, QR scan screen
│   ├── core/                   # camera capture, encoder wiring, transport, session state
│   │   └── src/main/kotlin/…/capture/  # CameraX adapter behind a CaptureSource interface
│   ├── protocol/                # Kotlin implementation of the shared signaling contract
│   └── README.md
├── protocol/                   # Shared, versioned, tested contract (single source of truth)
│   ├── signaling.schema.json   # closed allowlist of signaling messages
│   ├── qr-payload.md           # binary QR payload layout, version 1
│   ├── reason-codes.json       # closed reason-code set (pairing rejection + close)
│   └── vectors/                # cross-language conformance test vectors
├── spikes/                     # F0 non-production spike harnesses and evidence
│   ├── README.md               # NON-PRODUCTION marker (FR-039)
│   ├── f0-handshake/           # TLS 1.3 + pinning + adversarial scenario harness
│   ├── f0-pipeline/            # synthetic media → receive → decode → inject harness
│   ├── f0-camera-injection/    # per-application injection matrix harness
│   ├── f0-codec-interop/       # Android ↔ desktop codec negotiation harness
│   ├── f0-virtual-audio/       # virtual device render/capture/dropout harness
│   ├── tools/                  # Python measurement tooling (latency, dropout, drift, score)
│   ├── fixtures/               # versioned synthetic fixtures + calibration WAV
│   └── evidence/               # generated artefacts (gitignored except derived metrics)
├── docs/
│   ├── tdd-remotecam-v1.md
│   └── spike/                  # gate procedures, runbooks, report templates
├── Makefile
├── README.md
└── .gitignore
```

**Structure Decision**: two product applications (`desktop/` Rust, `mobile/` Kotlin) matching the constitution's mandated layout, plus **two new first-level directories**, both justified below:

- **`protocol/`** — the constitution requires that contracts shared between `desktop/` and `mobile/` be "extracted into a dedicated, versioned, and tested module, or exposed through a documented network interface". The signaling contract *is* shared, and the implementations must be independent (the TDD requires the same contract implemented separately in Kotlin and Rust with the same conformance tests). A dedicated, language-neutral contract directory is therefore the only shape that satisfies both rules: it is versioned, tested, and it cannot leak one language's source into the other.
- **`spikes/`** — FR-039 requires spike artefacts to be "clearly marked as non-production and MUST NOT be merged into product modules". Keeping them in their own top-level tree, outside `desktop/` and `mobile/`, makes that structural rather than a convention, and keeps the durable outputs (procedures, decisions) in `docs/`.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| `spikes/` is exempt from the ≥ 90 % unit-coverage gate (constitution II). Scope: F0 prototype packages only. | FR-039 mandates that spike artefacts are disposable and must not be merged into product modules. Their correctness evidence is the recorded procedure and per-gate evidence (FR-006, FR-038), not unit coverage. Measuring coverage of a throwaway harness consumes the 20 dev-day budget without reducing any product risk. | Applying the gate would require unit-testing Windows COM adapters and FFmpeg-driven analysis scripts that are deleted after F0 — cost without benefit. **Rollback:** if the maintainer rejects the exemption, apply the gate to `protocol/` only and treat `spikes/` as non-delivered tooling; `protocol/` already meets ≥ 90 %. |
| Custom jitter buffer and custom virtual-camera media source are built in-house rather than taken from a library. | `webrtc-rs` 0.21.1 provides ICE/DTLS-SRTP/RTP/RTCP but ships **no NetEq and no decoder**; the jitter buffer and the Media Foundation decode path are the two highest-risk components of the product. Windows provides **no** supported user-mode virtual camera source other than one we implement as a COM media source. | GStreamer (`gstreamer-rs`) would supply a jitter buffer but adds a large runtime with plugins and is explicitly rejected in TDD §14. `openh264` cannot decode High-profile streams and is unusable as the product path. For the camera, `MFCreateVirtualCamera` requires a custom `IMFMediaSource`, so the code is unavoidable. **Rollback:** GStreamer remains the documented fallback for the media pipeline; the camera has no fallback that avoids custom code, which is why the DirectShow filter is retained as a second backend. |
| New first-level directories `protocol/` and `spikes/` (constitution: "New first-level directories MUST be justified in review"). | `protocol/` is the shared-contract module the constitution itself mandates; `spikes/` is the structural expression of FR-039's isolation requirement. | Folding `protocol/` into either application would make the contract non-shared and violate self-containment; folding `spikes/` into `desktop/` would merge prototype code into a product module, violating FR-039. **Rollback:** both can be relocated under `docs/`-adjacent trees without changing their content, at the cost of the structural guarantees above. |

## Post-Design Constitution Re-check (Phase 1)

*Re-checked after generating `data-model.md`, `contracts/` and `quickstart.md`, and after adopting the nine clarifications of the 2026-10-22 session.*

| Check | Result |
| --- | --- |
| All documentation in English | ✅ `data-model.md`, `quickstart.md`, all six files in `contracts/` are English. |
| Contract defined before implementation, technology-neutral | ✅ `contracts/signaling-messages.schema.json` is a JSON Schema with no language or platform constructs; `contracts/qr-payload.md` defines a byte layout independent of any codec library; `contracts/measurement-formats.md` defines evidence file formats independent of any tool. |
| Domain model independent of implementation | ✅ `data-model.md` defines entities, fields, validation rules and state transitions without referencing Rust types, Kotlin classes, `windows` crate symbols or Media Foundation identifiers. |
| No secrets or personal data in artefacts | ✅ Every artefact schema in `contracts/` defines the redaction rules. `reason-codes.json` is a closed enumeration with no free-text reason field; `gate-report.schema.json` forbids absolute user paths and raw fingerprints. |
| Every metric has a declared consumer | ✅ Each metric in `data-model.md` §7 names its consumer (a gate criterion, a report section, or a local diagnostic alert). `audio_latency_ms` is explicitly declared out of F0 scope. |
| Reproducible by a second engineer in one working day | ✅ `quickstart.md` sequences the full F0 run into an ≈ 9.5 h schedule with preflight, toolchain self-test, per-gate commands and expected outcomes; `contracts/measurement-formats.md` pins the file formats so results are diffable. |
| Coverage and deviation approvals tracked | ⚠️ Two deviations still require maintainer sign-off (Complexity Tracking). Tracked as an open approval item. |
| Clarification Q1 — hardware decode precondition | ✅ PC-1; `environment.hardware_decode_verification` is mandatory in the report schema; `quickstart.md` §3 has a stop condition and the machine is replaced rather than measured on. |
| Clarification Q2 — waves and one phase review | ✅ The **Gate waves** table in the plan, `gate.wave` / `gate.depends_on` in the report schema, and the shared `phase_review_at` invariant. |
| Clarification Q3 — automated + human audio measurement | ✅ `dropouts.json` and `intelligibility.json` with the observer and phrase script mandatory; `audio_measurement_summary` is required on the virtual audio gate. |
| Clarification Q4 — one named role; conditional blocks its phases | ✅ `role.person_name` is mandatory; `condition.status` and `waiver.resolution_deadline` are enforced; the waiver MUST be signed by the gate's accountable role. |
| Clarification Q5 — pinned device, parallel licence assessment | ✅ `device_pin` anchors the evidence to the measured device; `licensing_bundle.json` carries every candidate and the `remeasurement_required` rule. |
| Clarification Q6 — raw p95 gated, floor and floor-corrected published | ✅ `latency_summary.json` publishes all three; `criterion.basis` is fixed to `raw`; `gate-report.schema.json` requires `raw_latency_p95_ms`, `harness_floor_ms` and `floor_corrected_latency_p95_ms` on the pipeline gate. |
| Clarification Q7 — one nominal impairment profile | ✅ `impairment_profile.json` with a single pinned profile; a deviation is labelled and cannot pass a gate; the report schema pins the profile constants. |
| Clarification Q8 — F0 gates video latency only | ✅ No audio-latency metric is gated anywhere; the ≤ 120 ms target is carried forward to F2/F3 as a V1 product goal. |
| Clarification Q9 — throttling proved, not asserted | ✅ `pairing_throttling.json` with the limit enforced at attempt 6, the listener closed at 10 consecutive failures, and a legitimate pairing unaffected; required on the pairing gate. |
| Clarification Q10 — residue after session end and after restart | ✅ `pairing_residue.json` requires both cases with the same method, stated coverage and empty artefact lists; required on the pairing gate. |

**Post-design gate result: PASS with two tracked deviations requiring maintainer approval.**
