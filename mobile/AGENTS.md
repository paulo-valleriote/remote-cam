# mobile/ — AGENTS.md

> Mobile-specific rules. Do NOT apply to `desktop/`, `protocol/`, or `spikes/`.

> Worktree: `remotecam-investigation/mobile/` (branch `main`, commit `cd5865b`)
> Mobile-specific rules. Do NOT apply to `desktop/`, `protocol/`, or `spikes/`.

**Quick start:** Scope = mobile-only (Kotlin, AGP 9.4.1, compileSdk 36, minSdk 29, 3 modules). Hard rules: `kotlinx-serialization-json` (no `.proto`), strict parsing (unknown keys / version >1 rejected), TLS 1.3 cert pinning (`X509TrustManager` + SHA-256, `hostnameVerifier` disabled), `CameraXCaptureSource` only `ImageAnalysis`, `Non-production` in all source. Before edit: read `protocol/AGENTS.md`; build via `make build-mobile`.

**Context sources:** `docs/tdd-remotecam-v1.md`, `specs/001-f0-technical-spikes/plan.md`, `.specify/init-options.json`, `.claude/skills/technical-design-doc-creator/`, `.opencode/commands/speckit.*`.

## Context and architecture

- **F0 non-production app:** all modules (`:app`, `:core`, `:protocol`) are spike harnesses, not production.
- **Gradle:** `gradle/libs.versions.toml` version catalog; `build.gradle.kts` (AGP 9.4.1, Kotlin 2.4.10); `compileSdk` 36; `minSdk` 29.
- **Modules (3):** `:protocol` (contract), `:core` (capture + transport), `:app` (shell + service).
- **Build:** `make build-mobile` = `gradlew -p mobile :app:assembleDebug`.

## Mobile-specific rules

### 1. Kotlin Serialization (not Protobuf)

- **Format:** `kotlinx-serialization-json` (1.12.0). Zero `.proto` files exist.
- **Usage in `:protocol`:** `buildJsonObject`, `Json.parseToJsonElement`, `JsonPrimitive`, `encodeToString`. `SignalingEncoder` produces canonical JSON; `SignalingParser` rejects unknown fields and versions `> 1`.
- **Usage in `:core`:** `StructuredLog.toJson()` builds JSON records.
- **Rule:** never use `getText()` for QR reading; always `Result.getBytes()` + `QrPayload.decode()`.

### 2. Strict parsing (no deviations tolerated)

- `SignalingParser` rejects:
  - Unknown keys (`REJECTED` / `ReasonCode.INVALID_MESSAGE`).
  - `version > 1` (`UNSUPPORTED_VERSION`).
  - Messages `> 16 KiB` (`MAX_MESSAGE_BYTES`).
  - Uppercase hex (`0123456789ABCDEF` rejected; only `abcdef` accepted).
- `QrPayload.decode()` validates magic (`RCM1`), version (`1`), capabilities mask (`0xffc0` reserved bits must be zero), endpoint count, private-address rules, and expiry (`expiresAt > nowUnix`).
- `Addresses` — documented IPv4/IPv6 regex; APIPA (`169.254.*`) is excluded despite regex presence.

### 3. TLS 1.3 certificate pinning (`:core`)

- `SignalingClient` uses custom `X509TrustManager`; verifies leaf certificate DER SHA-256 fingerprint; `hostnameVerifier` disabled (`{ _, _ -> true }`).
- No root store in product path.
- Ephemeral certificate (`rcgen` equivalent on Kotlin side).
- Rejects certificates with different fingerprint (constant pin, not constant-time in Kotlin but equivalent logic).

### 4. `:protocol` module — shared contract

- `namespace = "remotecam.protocol"`, `compileSdk` 36, `minSdk` 29, `compileOptions` Java 17.
- Dependencies: `kotlinx.serialization.json`, `junit.jupiter` (JUnit 5 via `useJUnitPlatform()`), `kotlin.test`.
- Coverage: `jacoco` enabled (`jacocoTestReport` produces XML + HTML). Gate ≥90%.
- `build.gradle.kts`: `testOptions { unitTests.all { it.useJUnitPlatform() } }`.

### 5. `:core` module — capture and transport

- `CameraXCaptureSource` — only `ImageAnalysis` (no `Preview`) to minimize ISP work, latency, and heat.
- `PeerConnectionFactoryProvider` — `PeerConnection.RTCConfiguration(emptyList())` (host-candidates only, no STUN/TURN).
- `StructuredLog` — JSON lines, mandatory redaction (correlation IDs 32 lowercase hex).
- `SignalingClient` — TLS 1.3 WSS client.
- `TransportConfig` — unit tests (`assertTrue(config.iceServers.isEmpty())`).
- `CaptureTest` — byte-level YUV copy (`ByteBuffer.allocateDirect`).
- Robolectric (`4.13`) for unit tests.

### 6. `:app` module — shell and service

- `MainActivity`, `RemoteCamApplication`, `StreamingService` (referenced in `AndroidManifest.xml`; source file may be stub).
- Permissions: `CAMERA`, `RECORD_AUDIO`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CAMERA`, `FOREGROUND_SERVICE_MICROPHONE`, `INTERNET`, `ACCESS_NETWORK_STATE`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_MULTICAST_STATE`.
- Service: `StreamingService` (`foreground`, `camera|microphone`, `exported=false`).
- **Repeated in all source files:** `/** NON-PRODUCTION — F0 risk-reduction spike (FR-039) */`.

### 7. Before modifying

- Before changing `protocol/src/main/kotlin/remotecam/protocol/SignalingParser.kt`: re-read `protocol/AGENTS.md` (version 1, strict rejection).
- Before modifying `core/src/main/kotlin/remotecam/core/signaling/SignalingClient.kt`: check TLS pinning and absence of root store (`mobile/AGENTS.md` §3).
- Before adding any `.proto`: **none exist**; all wire serialization is JSON (`kotlinx.serialization`).
- Before using `spikes/`: read `spikes/AGENTS.md` (FR-039, no merge).
