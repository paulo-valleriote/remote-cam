# mobile/ — AGENTS.md

> Mobile-specific rules. Do NOT apply to `desktop/`, `protocol/`, or `spikes/`.

**Quick start:**
- **Scope:** `mobile/` only — Kotlin 2.4.10, pure-JVM contract modules (Java 17 target), no Android dependency until F2.
- **Hard rules:**
  - Serialization is `kotlinx-serialization-json` (no `.proto`).
  - Parsing is strict: unknown keys and `version > 1` are rejected.
  - `:protocol` and `:session-core` are the delivered contract and must each stay at ≥90 % coverage.
  - `:session-core` mirrors the Rust session core decision for decision (ADR-0018); a divergence fails `make conformance`.
  - No `NON-PRODUCTION` code lives here; the F0 app and core modules are quarantined under `spikes/` (ADR-0003).
- **Before editing:** read `docs/constitution.md` and `protocol/AGENTS.md` if touching contracts; build with `make build-mobile`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.agents/skills/`.

## Context and architecture

- **Gradle:** `gradle/libs.versions.toml` version catalog; `build.gradle.kts` (Kotlin 2.4.10); both modules apply the `org.jetbrains.kotlin.jvm` plugin with `jvmTarget` 17. AGP is not applied in F1.
- **Modules (2):** `:protocol` (the shared contract) and `:session-core` (pure-JVM mirror of the Rust session core; depends on `:protocol`, no Android, no WebRTC). The F0 `:core` and `:app` modules are quarantined.
- **Build:** `make build-mobile` = `mobile/gradlew -p mobile :protocol:assemble :session-core:assemble`.
- **Product app:** the capture/transmit application is built in F2, not in F1.

## Mobile-specific rules

### 1. Kotlin Serialization (not Protobuf)

- **Format:** `kotlinx-serialization-json` (1.12.0). Zero `.proto` files exist.
- **Usage in `:protocol`:** `buildJsonObject`, `Json.parseToJsonElement`, `JsonPrimitive`, `encodeToString`. `SignalingEncoder` produces canonical JSON; `SignalingParser` rejects unknown fields and versions `> 1`.
- **Rule:** never use `getText()` for QR reading; always `Result.getBytes()` + `QrPayload.decode(bytes, nowUnix)`.

### 2. Strict parsing (no deviations tolerated)

- `SignalingParser` rejects:
  - Unknown keys (`REJECTED` / `ReasonCode.INVALID_MESSAGE`).
  - `version > 1` (`UNSUPPORTED_VERSION`).
  - Messages `> 16 KiB` (`MAX_MESSAGE_BYTES`).
  - Uppercase hex (`0123456789ABCDEF` rejected; only `abcdef` accepted).
- `QrPayload.decode(bytes, nowUnix)` validates magic (`RCM1`), version (`1`), capabilities mask (`0xffc0` reserved bits must be zero), endpoint count, private-address rules, and expiry (`expiresAt > nowUnix`). `isStructurallyValid()` is the clock-free subset; `isExpiredAt(nowUnix)` is the explicit expiry check.
- `Addresses` — documented IPv4/IPv6 regex; APIPA (`169.254.*`) is excluded despite regex presence.

### 3. `:protocol` module — shared contract

- Package `remotecam.protocol`, pure JVM, Java 17 target.
- Dependencies: `kotlinx.serialization.json`, `junit.jupiter` (JUnit 5 via `tasks.test { useJUnitPlatform() }`), `kotlin.test`.
- Coverage: `jacoco` enabled (`:protocol:jacocoTestReport` produces XML + HTML under `build/reports/jacoco/test`). Gate ≥90%.
- `Conformance` also executes `signaling/encoding.json` and reports it in the `encoding` section.

### 3b. `:session-core` module — Kotlin session core

- Package `remotecam.session`: `SessionCore` (`handle`, `authenticateFirstFrame`, `admitSignalingFrame`, `admitFrameEpoch`), the vocabulary in `Vocabulary.kt`, and `ManualClock` / `SeededRandomSource` in `Testing.kt` (same stream as the Rust `testing` module).
- `HandshakeConformance` replays the `kind = handshake` scenarios of `protocol/vectors/signaling/scenarios.json` (layers `core`, `qr`, `transport`) and `HandshakeConformanceTest` writes the result document to `CONFORMANCE_HANDSHAKE_OUTPUT`.
- Behaviour changes in the Rust `session-core` must be mirrored here in the same change; the scenarios are the executable proof.
- Coverage: `:session-core:jacocoTestReport`, gate ≥90%.

### 4. Quarantined F0 modules

The F0 Android harness moved out of this tree in F1-01:

| Module | Quarantined at | Contents |
| --- | --- | --- |
| `:core` | `spikes/f0-mobile-core` | `CameraXCaptureSource`, `YuvPlaneCopier`, `StructuredLog`, `SignalingClient`, `PeerConnectionFactoryProvider` and their tests |
| `:app` | `spikes/f0-mobile-app` | `MainActivity`, `RemoteCamApplication`, manifest and resources |

They are source records only: they are not in `settings.gradle.kts`, are not
built, and MUST NOT be imported from `mobile/` (FR-039, ADR-0003). The TLS 1.3
pinning and capture rules they demonstrate are design input for F2, not live
rules in this tree.

### 5. Before modifying

- Before changing `protocol/src/main/kotlin/remotecam/protocol/SignalingParser.kt`: re-read `protocol/AGENTS.md` (version 1, strict rejection, wire freeze).
- Before adding any `.proto`: **none exist**; all wire serialization is JSON (`kotlinx.serialization`).
- Before using `spikes/`: read `spikes/AGENTS.md` (FR-039, no merge).
