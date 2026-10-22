# Feature Specification: F0 — Risk-Reduction Spikes and Technical Decisions

**Feature Branch**: `main` (no branch hook configured; work performed on the current branch)

**Created**: 2026-10-22

**Status**: Draft

**Input**: User description: "https://paulovalleriotedev.atlassian.net/browse/REC-1"

**Traceability**: Jira epic REC-1 — "F0 — Spikes e decisões técnicas (V1)". Source decisions and budgets are defined in `docs/tdd-remotecam-v1.md` (§5.5, §5.6, §5.7, §7, §14, §15, §16, §18).

## Clarifications

### Session 2026-10-22

- Q: What has to be true of the reference machine before the latency and resource gates count as valid, and what happens when it is not? → A: Hardware H.264 decode is a verified precondition; verify it before starting and swap the reference machine if absent
- Q: How are the five gates sequenced, and does a failure in one of them stop the others? → A: Dependency-ordered waves, with all five verdicts issued at one phase review
- Q: How is "continuous and intelligible audio" measured so that a second engineer can reproduce the result? → A: Automated silence/dropout detection on the recording, plus a scripted human intelligibility check
- Q: Who signs off on each gate verdict, and what does a "conditional" verdict allow the next phase to start with? → A: One accountable role per gate, named in the report; a conditional verdict blocks the phases it conditions unless that role records an explicit waiver with a deadline
- Q: Which third-party virtual audio device does the spike use, and how do the audio results stay valid if the licensing decision selects a different one? → A: Pin one named device for the technical measurement, and evaluate the candidate alternatives' license terms in parallel on the same test steps
- Q: Which latency definition and gate rule apply to the video latency gate, given that the phase research found a virtual camera has no "glass" and that the measurement rig's own capture overhead is 17–50 ms against a ~100 ms budget? → A: Gate the raw capture-at-consumer p95 (≤ 150 ms); publish the measured harness floor and the floor-corrected value alongside it
- Q: What exact network impairment profile must the loss-and-jitter test use, so that "3% packet loss with jitter" becomes a reproducible, pass/fail-able condition? → A: One nominal scenario only: 3% uniform loss, 30 ms added latency, ±10 ms jitter
- Q: Must the F0 desktop pipeline gate also measure and gate end-to-end audio latency, and if so against what threshold? → A: No — F0 gates video latency only; audio latency is measured in F2/F3 once the real mobile capture path exists
- Q: Must the pairing handshake gate also prove that repeated pairing attempts are throttled and that the listener stops accepting connections after repeated failures? → A: Yes — add a scenario proving a per-origin attempt limit, progressive blocking, and listener shutdown after consecutive failures, using the source thresholds
- Q: Does the pairing gate have to prove that no pairing state survives an application restart, or only that nothing persists after a session ends normally? → A: Both — require zero pairing artefacts after a normal session end and after an application restart, with the inspection method stated

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Prove virtual camera injection across the target application matrix (Priority: P1)

As the technical lead, I need evidence that the virtual camera is discoverable and usable by every application category the product must support, so we can commit to an injection approach — or switch to the approved fallback — before any product code is written.

**Why this priority**: If consumer applications cannot see or receive frames from the virtual camera, the product's core promise fails regardless of how good the transport is. This gate also determines the Windows injection backend that the media and application phases depend on.

**Independent Test**: On a clean Windows 11 x64 machine, register the prototype for the current user without administrator rights, open each application in the minimum matrix, select the virtual camera, and confirm stable frames for 30 continuous minutes in each. The result is a per-application matrix plus a recorded backend decision.

**Acceptance Scenarios**:

1. **Given** a clean Windows 11 x64 machine and a non-administrator account, **When** the prototype virtual camera is registered for that user, **Then** the camera is enumerable and usable without elevation.
2. **Given** the registered virtual camera, **When** a Chromium-based browser, a video conferencing client, OBS, the Windows Camera application, and a screen recorder each open it, **Then** live video is visible and remains stable for 30 minutes in every application.
3. **Given** both 32-bit and 64-bit consumer applications on 64-bit Windows, **When** each enumerates capture devices, **Then** both bitnesses see the virtual camera and receive frames.
4. **Given** a format requested by a consumer application, **When** the application selects 640×480 or 1280×720 at 30 frames per second, **Then** frames are delivered at the announced size and rate with no unexpected freeze longer than 2 seconds.
5. **Given** one or more target applications cannot see or receive frames, **When** the alternative Windows 11 virtual camera backend is evaluated, **Then** the report records per-application results for both backends and identifies the selected backend with rationale.
6. **Given** two consumer applications open the virtual camera at the same time, **When** both attempt capture, **Then** the observed behavior (both work, one fails, or undefined) is recorded with evidence.

---

### User Story 2 - Validate the desktop receive pipeline against latency and resilience gates (Priority: P2)

As the technical lead, I need measured latency, resource usage, and behavior under loss and jitter for the desktop receive pipeline, so we know whether the chosen architecture can deliver the product experience or must change before the media phase starts.

**Why this priority**: The receive pipeline is a top technical risk: it combines third-party transport, a custom jitter buffer, hardware and software decoding, and audio/video synchronization. A failure here forces an architecture change with schedule impact across F3 and F4.

**Independent Test**: Feed timestamped synthetic media through the complete receive and injection path in isolation, capture raw samples for repeated runs, and report p95 capture-at-consumer latency, CPU, memory, drift, and behavior under the nominal impairment profile and under emulated stalls.

**Acceptance Scenarios**:

1. **Given** the reference environment described in Assumptions, **When** 1280×720 at 30 frames per second is transmitted for three runs of at least 10 minutes, **Then** p95 capture-at-consumer video latency is at most 150 ms, the raw measured value is the one judged against that threshold, and the raw samples support the calculation.
2. **Given** the nominal impairment profile of 3% uniform packet loss, 30 ms added latency, and ±10 ms jitter, **When** audio is received for at least 10 minutes, **Then** audio remains continuous with no dropout as defined in FR-030 and no renegotiation in the consuming application.
3. **Given** a 30-minute run at 1280×720 and 30 frames per second, **When** desktop resource usage is sampled, **Then** CPU stays at or below 15% and memory at or below 400 MB on the reference environment.
4. **Given** a continuous session of at least 30 minutes, **When** audio/video drift is measured, **Then** drift is at most 20 ms per hour, measured or extrapolated from documented evidence.
5. **Given** a media stall of 5 seconds, **When** the stream resumes, **Then** the session continues, buffered latency returns to the pre-stall baseline within 2 seconds, and no unbounded buffer growth occurs.
6. **Given** the measured stages, **When** results are compared with the latency budget, **Then** the report shows actual versus budgeted time for capture, encode, network and jitter buffer, decode, frame handoff, and consumer presentation.

---

### User Story 3 - Confirm codec interoperability between mobile and desktop (Priority: P3)

As the technical lead, I need to know whether the mobile and desktop stacks can exchange the preferred video codec, so the codec baseline is fixed before encoding, decoding, and quality work begins.

**Why this priority**: Codec choice affects CPU, quality, bandwidth, and the effort in both application phases. It must be settled before F1 defines the protocol and before F2 and F3 implement media paths.

**Independent Test**: Send live video from the Android reference device to the desktop receive pipeline using the preferred codec and decode it; if interoperability fails, repeat with the fallback codec and quantify the difference.

**Acceptance Scenarios**:

1. **Given** the Android reference device and the desktop receiver, **When** the preferred video codec is negotiated, **Then** both sides report compatible profile, level, and format parameters and the decoded image is free of visible corruption for 10 continuous minutes.
2. **Given** the preferred codec cannot be negotiated or decoded reliably, **When** the fallback codec is exercised, **Then** it decodes correctly and the CPU, latency, and quality differences are measured and reported.
3. **Given** the interop results, **When** the gate is reviewed, **Then** the codec decision is recorded and approved before F1 starts.

---

### User Story 4 - Prove the authenticated pairing handshake (Priority: P4)

As the security reviewer, I need evidence that pairing authenticates both sides, resists interception and replay, and leaves no persistent state, so the protocol phase can implement the handshake on a validated design.

**Why this priority**: Pairing is the only trust boundary in a local-network product. An unproven or weakened handshake would invalidate the security model and force rework after the protocol is built.

**Independent Test**: Execute the handshake harness across valid and adversarial scenarios and confirm that only the valid path succeeds, that no media or control data flows before authentication, and that no credentials or pairing records survive the session or an application restart.

**Acceptance Scenarios**:

1. **Given** a new session, **When** the mobile side pairs with the desktop, **Then** the connection uses TLS 1.3 with a certificate generated for that session and no protocol downgrade is possible.
2. **Given** the desktop certificate, **When** the mobile side validates it, **Then** validation uses exact SHA-256 fingerprint pinning and a mismatch aborts the session.
3. **Given** the media transport fingerprints, **When** they are exchanged, **Then** they travel only inside the authenticated channel and a tampered fingerprint aborts before any media flows.
4. **Given** a wrong fingerprint, an expired or reused pairing token, a replay attempt, a downgrade attempt, or an interception proxy, **When** pairing is attempted, **Then** the handshake fails with a reason from a closed code set and never falls back to an unauthenticated channel.
5. **Given** a session that has ended, or an application that has been restarted, **When** both devices are inspected with the same method, **Then** no session key, token, fingerprint, or pairing record remains, and no pairing from a previous session is reusable without a new one-time token.
6. **Given** the harness results, **When** the security reviewer evaluates the gate, **Then** the reviewer signs off or records blocking findings with severity and owner.
7. **Given** an origin that submits invalid pairing attempts, **When** the limit of 5 attempts per minute is exceeded and then 10 consecutive failures occur, **Then** every attempt is refused with a reason from the closed code set, the origin is progressively blocked, the pairing listener is closed, and no session is established.

---

### User Story 5 - Close the virtual audio path and its licensing decision (Priority: P5)

As the product owner, I need proof that audio can reach consumer applications through a third-party virtual device and a decision on how that device is licensed or replaced, so audio scope is either confirmed for V1 or explicitly reduced with an approved plan.

**Why this priority**: Audio is half of the product promise, and the third-party driver is a known commercial blocker. Both the technical path and the licensing path must close before audio work is planned.

**Independent Test**: Render decoded audio into an installed third-party virtual audio device, record from the paired capture endpoint in a target application, and confirm intelligible continuous audio under the nominal impairment profile; produce a licensing decision with owner, cost, and fallback.

**Acceptance Scenarios**:

1. **Given** a supported third-party virtual audio device is installed, **When** the prototype renders decoded audio, **Then** a target application recording from the corresponding capture endpoint receives intelligible, continuous audio.
2. **Given** the nominal impairment profile, **When** a consumer application records for at least 10 minutes, **Then** there is no intermittent silence and no renegotiation of the audio stream.
3. **Given** the virtual device is missing, unsigned, or reports an unexpected identity, **When** the application checks readiness, **Then** it reports an actionable state and video-only operation remains available.
4. **Given** the licensing options, **When** product and legal review the gate, **Then** a decision is recorded covering redistribution rights, cost, support burden, and the approved fallback if no license is granted.
5. **Given** licensing is unresolved on the gate date, **When** the gate is reported, **Then** the blocker, escalation owner, and interim audio scope are documented and F1/F2/F3 impact is stated.

---

### Edge Cases

- A single target application cannot see the virtual camera while the others can. The fallback backend must be evaluated and the decision must state which applications each backend serves.
- The preferred codec negotiates successfully but produces visible corruption, green frames, or unacceptable CPU usage. The fallback codec is exercised and the decision reflects observed quality, not negotiation alone.
- The virtual audio device disappears, is renamed by a driver update, or fails signature validation mid-spike. Readiness detection must surface the condition and the video-only path must remain usable.
- Packet loss arrives in bursts rather than uniformly. The gate scenario is the nominal impairment profile; any run that deviates from it, including burst loss, is judged on detected dropouts rather than on average loss percentage, and is not used to pass the gate.
- The mobile device throttles thermally or another application takes the camera during the interop run. The run is invalidated or annotated, and the impact on measured results is stated.
- The network is 2.4 GHz, congested, or drops below the reference conditions. Results are labeled as outside the reference environment and are not used to pass a gate.
- Windows policy or driver signature enforcement blocks per-user registration. The failure mode, required privilege, and fallback are documented.
- A 32-bit consumer runs on 64-bit Windows and does not see the device. This is treated as a matrix failure, not an acceptable limitation.
- The reference machine lacks hardware-accelerated decoding for the codec under test. The machine is replaced before the pipeline gate runs, the decode precondition check is recorded as evidence, and software decode results are reported separately and cannot be used to pass the latency gate.
- Latency measurement excludes mobile capture and encoding. The report must state which stages are included so the gate result can be compared with the stated budget.
- The measured harness floor of the capture procedure is a large fraction of the 150 ms budget. The raw capture-at-consumer value remains the judged value, and the report MUST show the floor-corrected value and state that a failure caused by the capture procedure is distinguished from a product regression.
- An origin floods the pairing listener with invalid attempts while a legitimate second origin pairs. Throttling applies per origin, the legitimate pairing is unaffected by the flood, and the listener closure is recorded as a security event rather than a product failure.
- The operating system restores a suspended application process after a restart, or a pairing token from the previous session is replayed. The restored state is treated as a new session that requires a fresh one-time pairing token, and the previous pairing is never reusable.
- The licensing decision selects a virtual audio device other than the one used for the technical measurement. The technical evidence remains valid for the measured device, and the selected device MUST be re-measured against the same recorded steps before it becomes the shipping option.
- Licensing remains unresolved on the target gate date. The gate cannot pass silently; it must be recorded as failed or conditional with an approved plan B.

## Requirements *(mandatory)*

### Functional Requirements

**Pairing and security**

- **FR-001**: The spike MUST demonstrate a pairing handshake over TLS 1.3 using a certificate generated per session, with no downgrade path and no unauthenticated fallback.
- **FR-002**: The mobile side MUST validate the desktop certificate by exact SHA-256 fingerprint pinning and MUST abort on any mismatch, missing pin, or chain-only trust.
- **FR-003**: Media transport fingerprints MUST be exchanged only inside the authenticated channel, and a tampered fingerprint MUST abort the session before media flows.
- **FR-004**: The handshake MUST reject wrong fingerprints, expired or reused pairing tokens, replays, downgrade attempts, and interception, and MUST record each rejection in a closed reason-code set.
- **FR-005**: The spike MUST verify that no session key, pairing token, fingerprint, or pairing record persists after the session ends, and that no pairing artefact survives an application restart, using the same inspection method for both cases, and MUST record what that inspection covered.
- **FR-006**: The handshake harness MUST produce reproducible evidence per scenario containing preconditions, expected result, observed result, and a trace or log reference.
- **FR-007**: The spike MUST measure the time from pairing start to an established authenticated channel and report whether fingerprint pinning adds perceptible delay.
- **FR-049**: The spike MUST demonstrate that repeated pairing attempts from a single origin are throttled at a limit of 5 attempts per minute per origin, are progressively blocked, and cause the pairing listener to be closed after 10 consecutive failures, and MUST record each throttling decision with its reason code and outcome.

**Desktop receive pipeline**

- **FR-008**: The desktop pipeline MUST receive only, with no media send tracks, and MUST process the mobile audio and video streams end to end.
- **FR-009**: The pipeline MUST include adaptive jitter buffering, hardware-accelerated decoding with a software fallback, and audio/video synchronization, and MUST report measured time per stage against the stated budget.
- **FR-010**: Latency measurement MUST use timestamped synthetic media that allows capture-at-consumer latency to be computed at the consumer application, with the included and excluded stages stated explicitly. The report MUST state the measured harness floor of the capture procedure and the floor-corrected latency alongside the raw value.
- **FR-011**: The spike MUST measure p95 capture-at-consumer video latency over at least three runs of at least 10 minutes at 1280×720 and 30 frames per second, MUST gate on the raw measured value against the 150 ms threshold, and MUST retain raw samples.
- **FR-012**: Under the nominal impairment profile of 3% uniform packet loss, 30 ms added latency, and ±10 ms jitter, audio MUST remain continuous and intelligible for at least 10 minutes, with no dropout as defined in FR-030 and no consumer-side stream renegotiation.
- **FR-013**: At 1280×720 and 30 frames per second on the reference environment, desktop CPU MUST stay at or below 15% and memory at or below 400 MB, with the measurement tooling stated.
- **FR-014**: Audio/video drift MUST be at most 20 ms per hour, measured over at least 30 minutes or extrapolated from documented evidence.
- **FR-015**: After a 5-second media stall, the pipeline MUST resume without terminating the session, without unbounded buffer growth, and with latency returning to baseline within 2 seconds.
- **FR-016**: The spike MUST assess 1920×1080 at 30 frames per second as an optional maximum and MUST recommend the default quality setting based on evidence.

**Codec interoperability**

- **FR-017**: The spike MUST negotiate the preferred video codec between the Android reference device and the desktop receiver and MUST verify agreement on profile, level, and format parameters.
- **FR-018**: Decoded preferred-codec video MUST be free of visible corruption for at least 10 continuous minutes.
- **FR-019**: If the preferred codec fails negotiation, decoding, or quality gates, the spike MUST evaluate the fallback codec and MUST quantify CPU, latency, and quality differences, and MUST record whether the fallback codec is hardware-accelerated on the reference environment, so that the decode precondition and the latency criterion can be applied to the codec actually selected.
- **FR-020**: The codec decision MUST be recorded as preferred codec or fallback codec with supporting evidence and MUST be approved before F1 begins.

**Virtual camera injection**

- **FR-021**: The prototype virtual camera MUST register for the current user on Windows 11 x64 without administrator rights.
- **FR-022**: The virtual camera MUST be visible and MUST deliver frames in 100% of the minimum target matrix: a Chromium-based browser, a video conferencing client, OBS, the Windows Camera application, and a screen recorder.
- **FR-023**: Both 32-bit and 64-bit consumer applications MUST see the virtual camera on 64-bit Windows.
- **FR-024**: Injection MUST sustain at least 30 continuous minutes at 1280×720 and 30 frames per second, with frame integrity, size, and rate verified and any stalls or duplicate frames counted.
- **FR-025**: If any target application fails, the spike MUST evaluate the alternative Windows 11 virtual camera backend and MUST record per-application results for both backends.
- **FR-026**: The injection backend decision MUST be recorded with rationale, evidence, licensing impact, and consequences for the media phase.
- **FR-027**: The spike MUST record whether multiple consumer applications can open the virtual camera simultaneously and what each observes.
- **FR-028**: When injection falls behind, the pipeline MUST continue receiving without blocking the network path, MUST discard the oldest pending frame, and MUST count discarded frames.

**Virtual audio and licensing**

- **FR-029**: The prototype MUST render decoded audio into a supported third-party virtual audio device and MUST remain audible and intelligible in a consumer application recording from the paired capture endpoint, where intelligibility is confirmed by a scripted spoken-phrase check whose phrase script, observer, and outcome are recorded. The device used for the technical measurement MUST be the one named in the recorded procedure.
- **FR-030**: Audio MUST remain continuous for at least 10 minutes under the nominal impairment profile, with no consumer-side renegotiation and no dropout detected by automated silence analysis of the recorded consumer capture, where a dropout is any contiguous detected silent interval of at least 50 ms.
- **FR-031**: The spike MUST verify virtual audio device detection, identity, signature status, and an audible test tone before a session is considered audio-ready.
- **FR-032**: When the virtual audio device is missing, unsigned, or unexpected, the application MUST report an actionable state and MUST keep video-only operation usable.
- **FR-033**: The licensing assessment MUST compare at least redistribution licensing, user-installed official driver, and a plan B alternative, documenting cost, legal constraints, support burden, and user experience, and MUST document the license terms of every candidate device considered, including candidates that were not used for the technical measurement.
- **FR-034**: A licensing decision or approved plan B MUST be recorded with decision owner, approver, date, and fallback; an unresolved decision on the gate date MUST be escalated rather than treated as passed.

**Reporting and decisions**

- **FR-035**: Each gate MUST have a go, no-go, or conditional verdict with evidence, measurement conditions, and a named accountable role: the technical lead for the camera injection, desktop receive pipeline, and codec interoperability gates, the security reviewer for the pairing handshake gate, and the product owner for the virtual audio and licensing gate.
- **FR-036**: The spike report MUST include video latency (p50, p95, maximum, measured harness floor, and floor-corrected value), CPU, memory, loss and jitter behavior, drift, the per-application injection matrix, codec results, audio results, and per-gate verdicts.
- **FR-037**: Every measurement MUST state hardware, operating system build, network conditions, device models, application versions, test duration, and tooling version.
- **FR-038**: A second engineer MUST be able to reproduce each gate measurement from the recorded procedure within one working day.
- **FR-039**: Spike artifacts MUST be clearly marked as non-production and MUST NOT be merged into product modules; reusable findings MUST be documented for the protocol, mobile, and media phases.
- **FR-040**: Open questions encountered during the spike MUST be recorded with owner, impact, and blocking status, including simultaneous consumers, default quality, and any new question.
- **FR-041**: All deliverables MUST be written in English and stored in the repository documentation structure, consistent with the project constitution.
- **FR-042**: The spike MUST NOT include secrets, personal data, production credentials, or real user media in artifacts, logs, or reports.
- **FR-043**: The gate report MUST state explicitly whether F1, F2, and F3 are unblocked and list any conditions attached to a conditional verdict, including the waiver status of each conditional verdict.

**Gate execution**

- **FR-044**: The spike MUST verify that the reference machine provides hardware-accelerated decoding for the codec under test before the pipeline gate runs, MUST record that check as gate evidence, and MUST NOT accept latency or resource results measured on a machine without hardware-accelerated decoding for that codec.
- **FR-045**: The spike MUST execute the five gates in dependency-ordered waves: camera injection and pairing handshake in the first wave, the desktop receive pipeline in the second wave, and codec interoperability and virtual audio in the third wave, which depend on the pipeline. For each gate the spike MUST state its wave and the gates it depends on.
- **FR-046**: A failed or conditional gate MUST NOT stop the gates that do not depend on it. For each gate the spike MUST record the verdict of its dependencies, so that a no-go is isolated and reported rather than collapsing the phase.
- **FR-047**: All five gate verdicts MUST be issued at a single phase review held no later than 2026-11-06, and a gate whose dependency failed MUST still receive a recorded verdict that states the blocking dependency.
- **FR-048**: A conditional verdict MUST block the phases it conditions, and MUST be lifted only by a written waiver from the accountable role of that gate, recording the waiver conditions and a resolution deadline.

### Out of Scope (F0)

- Production user interfaces, installers, updaters, signing, store distribution, and firewall configuration.
- The full protocol implementation, session state machine, and pairing QR experience; F0 validates the handshake design only.
- The complete Android application, camera controls, foreground service, thermal policy, and device matrix beyond the interop reference device.
- Audio end-to-end latency measurement and thresholding, which requires the real mobile capture path and is deferred to F2/F3.
- Full desktop application shell, diagnostics panel, driver wizard, and packaging.
- Long-duration soak testing, accessibility validation, fuzzing, and the full device compatibility matrix, which belong to later phases.
- Any kernel-mode driver development or a custom audio driver.
- Cloud services, internet transport, relaying, and non-local-network scenarios.

### Key Entities

- **Spike Gate**: A decision checkpoint with a pass criterion, measurement method, verdict, evidence reference, accountable role, target date, execution wave, and the set of gates it depends on.
- **Gate Role**: The accountable role for a gate verdict — the technical lead, the security reviewer, or the product owner — recorded by name in the gate record.
- **Measurement Run**: One execution of a gate scenario, including environment, duration, conditions, raw samples, and summary metrics.
- **Capture-at-Consumer Latency**: The end-to-end delay from the timestamped synthetic source to the moment the consumer application's own output exposes the corresponding frame. It is the video latency metric used by every gate and success criterion, is judged on its raw value, and is reported together with the measured harness floor — the delay added by the capture procedure itself rather than by the product under test — and the floor-corrected value (formerly referred to as "glass-to-glass latency").
- **Impairment Profile**: The named network condition applied to a loss-and-jitter measurement run — 3% uniform packet loss, 30 ms added latency, and ±10 ms jitter — applied to the media stream only, so that connection establishment and control traffic remain unaffected. It is the single condition used by every loss-and-jitter gate scenario and MUST be recorded with the run evidence.
- **Target Application**: A consumer application category with product, version, bitness, and observed injection outcome.
- **Decision Record**: A durable decision with options considered, evidence, rationale, approver, date, and downstream impact.
- **Reference Environment**: The documented hardware, operating system, network, and devices against which gates are judged, including the recorded hardware decode capability verification result for the codec under test.
- **Licensing Option**: A virtual audio device sourcing option with license terms, cost, redistribution rights, support burden, and user experience.
- **Audio Run Capture**: The recorded consumer-application capture for one measurement run, together with the dropout analysis output and the scripted intelligibility check result.
- **Prototype Artifact**: Non-production code, harness, or configuration used to produce evidence, with a clear non-production marker.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All five gates have a recorded verdict and a named accountable role by 2026-11-06, and the protocol, mobile, and media phases are either unblocked or have documented conditions.
- **SC-002**: Virtual camera video is visible and stable for 30 continuous minutes in 100% of the five target application categories, including both 32-bit and 64-bit consumers where applicable.
- **SC-003**: p95 capture-at-consumer video latency is at most 150 ms across three runs of at least 10 minutes at 1280×720 and 30 frames per second on the reference environment, judged on the raw measured value, with the measured harness floor and the floor-corrected value reported alongside it.
- **SC-004**: Audio is intelligible and continuous for 10 minutes under the nominal impairment profile of 3% uniform packet loss, 30 ms added latency, and ±10 ms jitter, with zero consumer-side renegotiations, zero dropouts detected by the automated silence analysis of the recorded capture, and intelligibility confirmed by the scripted spoken-phrase check.
- **SC-005**: Desktop CPU remains at or below 15% and memory at or below 400 MB during a 30-minute 720p30 run on the reference environment.
- **SC-006**: The preferred codec is confirmed interoperable, or the fallback codec is approved with measured tradeoffs, before the protocol phase begins.
- **SC-007**: The audio licensing decision or approved plan B is recorded with owner and date, and no unresolved licensing blocker is left without an escalation path.
- **SC-008**: 100% of adversarial pairing scenarios are rejected and the valid scenario succeeds, with no credential or pairing persistence after the session ends or after an application restart, and the attempt limit, the progressive blocking, and the listener shutdown are demonstrated against the source thresholds.
- **SC-009**: A second engineer reproduces the latency and injection gates within one working day using only the recorded procedure and environment description.
- **SC-010**: The injection backend and default quality setting are decided with evidence, removing the two largest open architectural choices for the media phase.
- **SC-011**: The reference environment is recorded with exact CPU model, GPU model, operating system build, and a recorded hardware decode capability verification result for the codec under test before any latency or resource measurement is accepted.
- **SC-012**: Every gate record states its execution wave and its dependencies, and all five verdicts are issued at a single phase review on or before 2026-11-06, including a recorded verdict for any gate whose dependency failed.
- **SC-013**: Each of the five gates records its accountable role, and every conditional verdict records either a written waiver with a resolution deadline or an explicit block on the phases it conditions.
- **SC-014**: The virtual audio device used for the technical measurement is pinned by product and version in the recorded procedure, and the license terms of every candidate device considered are documented, so a change of the selected device does not invalidate the audio evidence.

## Assumptions

- The reference environment is a Windows 11 x64 machine whose exact CPU model, GPU model, and operating system build are recorded, a mid-range Android device from 2021 or later, and a Wi-Fi 5 network on 5 GHz at up to 5 meters, with internet connectivity disabled during measurements. The reference machine MUST provide hardware-accelerated decoding for the codec under test, and this precondition MUST be verified before the pipeline gate starts; a machine that does not satisfy it is replaced rather than measured on.
- The latency budget is the per-stage allocation defined in the source technical design: capture 33 ms, encode 15 ms, network and jitter buffer 20 ms, decode 12 ms, frame handoff 3 ms, consumer presentation 17 ms, approximately 100 ms total, with 150 ms as the accepted capture-at-consumer latency target. The budget covers the video path only; no audio latency threshold applies in F0. The report MUST state the budget used and any deviation from it, the measured harness floor of the capture procedure, and the floor-corrected latency alongside the raw value.
- The minimum target matrix is represented by one current product per category — Chromium-based browser, video conferencing client, OBS, Windows Camera, screen recorder — with exact products and versions selected by the team and recorded with results.
- The virtual camera prototype is based on a permissively licensed user-mode component with a separate frame producer; the alternative Windows 11 virtual camera backend is the fallback if any target application cannot use that user-mode component.
- A supported third-party virtual audio device is available for the technical spike. The device used for the technical measurement is pinned by product and version in the recorded procedure, and the license terms of the alternative candidate devices are assessed in parallel, so that a later change of selected device does not invalidate the technical evidence. Licensing may result in requiring the user to install it from the official source rather than bundling it.
- Synthetic timestamped media is acceptable for latency measurement, and the report states whether mobile capture and encoding are included in each measurement.
- Injection continuity is tested for 30 minutes and loss/drift runs for 10 to 30 minutes; the full 2-hour soak remains a later hardening activity.
- Prototype code is disposable and isolated from product modules; its findings, procedures, and decisions are the durable outputs.
- The fallback video codec is acceptable without architectural change, at the cost of higher CPU usage.
- The default target quality is 1280×720 at 30 frames per second, with 1920×1080 at 30 frames per second as an optional maximum.
- Product and legal stakeholders are available to decide audio licensing by the gate date; if not, the gate fails or becomes conditional with an approved plan B.
- Open questions from the source technical design remain tracked and are not silently resolved by this spike; any answer becomes a decision record.
- The project constitution applies: all deliverables are in English, no secrets are versioned, and prototypes do not bypass review or quality gates.
