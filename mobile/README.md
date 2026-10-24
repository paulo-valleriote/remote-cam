# RemoteCam Mobile

Kotlin Android application that captures camera and microphone and sends them to
the desktop over the local network. F0 built the contract module and the
capture/transport harnesses; those harnesses are now quarantined under `spikes/`
(ADR-0003), and the production application is built in F2.

## Modules

| Module | Directory | Responsibility |
| --- | --- | --- |
| `:protocol` | `protocol/` | Kotlin implementation of the shared signaling and QR contract. Delivered package, coverage gate ≥ 90 %. |

The F0 `:core` and `:app` modules moved to `spikes/f0-mobile-core` and
`spikes/f0-mobile-app` in F1-01. They are source records: they are not in
`settings.gradle.kts`, are not built, and must not be imported from this tree.

## Prerequisites

- JDK 17.
- Android SDK with `compileSdk 36` and build-tools 36.0.0.
- Gradle 9.6.x and Android Gradle Plugin 9.4.1.
- Kotlin 2.4.x. AGP 9 provides built-in Kotlin support, so the
  `org.jetbrains.kotlin.android` plugin is deliberately **not** applied.

Versions are pinned in `gradle/libs.versions.toml`. CameraX, the WebRTC Android
SDK, ZXing core and kotlinx-serialization remain declared for the F2
application; only kotlinx-serialization is consumed by `:protocol` today.

## Build and test

```bash
make build-mobile
make test
make coverage
make lint
```

`make build-mobile` assembles the `:protocol` library
(`mobile/protocol/build/outputs/aar/`). `make coverage` runs the JaCoCo report
for `:protocol`. The 90 % threshold applies to the delivered contract package.

## Debugging

`make build-mobile` produces the contract AAR; there is no application APK in
this phase. `make test` runs the Kotlin unit tests without a device. The F0
capture and transport code is quarantined under `spikes/f0-mobile-*` and is
validated against `spikes/evidence/` produced by the desktop harnesses and the
Android interop scenario.

## Cleanup

```bash
make clean
```

This removes `mobile/.gradle/` and module build directories. It never removes
committed sources, fixtures or evidence.
