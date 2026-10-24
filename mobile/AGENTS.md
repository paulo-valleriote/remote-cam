# mobile/ — AGENTS.md

> Mobile-specific rules. Do NOT apply to `desktop/`, `protocol/`, or `spikes/`.

**Quick start:**
- **Scope:** `mobile/` only — Kotlin, AGP 9.4.1, `compileSdk` 36, `minSdk` 29, one module.
- **Hard rules:**
  - Serialization is `kotlinx-serialization-json` (no `.proto`).
  - Parsing is strict: unknown keys and `version > 1` are rejected.
  - `:protocol` is the delivered contract and must stay at ≥90 % coverage.
  - No `NON-PRODUCTION` code lives here; the F0 app and core modules are quarantined under `spikes/` (ADR-0003).
- **Before editing:** read `docs/constitution.md` and `protocol/AGENTS.md` if touching contracts; build with `make build-mobile`.

**Context sources:** `docs/constitution.md`, `GLOSSARY.md`, `docs/adr/`, `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.agents/skills/`.

## Context and architecture

- **Gradle:** `gradle/libs.versions.toml` version catalog; `build.gradle.kts` (AGP 9.4.1, Kotlin 2.4.10); `compileSdk` 36; `minSdk` 29.
- **Modules (1):** `:protocol` (the shared contract). The F0 `:core` and `:app` modules are quarantined.
- **Build:** `make build-mobile` = `gradle -p mobile :protocol:assembleDebug`.
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

- `namespace = "remotecam.protocol"`, `compileSdk` 36, `minSdk` 29, `compileOptions` Java 17.
- Dependencies: `kotlinx.serialization.json`, `junit.jupiter` (JUnit 5 via `useJUnitPlatform()`), `kotlin.test`.
- Coverage: `jacoco` enabled (`jacocoTestReport` produces XML + HTML). Gate ≥90%.
- `build.gradle.kts`: `testOptions { unitTests.all { it.useJUnitPlatform() } }`.

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
