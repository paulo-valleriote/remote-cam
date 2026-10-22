# RemoteCam Mobile

Kotlin Android application that captures camera and microphone and sends them to
the desktop over the local network. F0 builds the contract module and the
capture/transport harnesses; the production UI is not part of this phase.

## Modules

| Module | Directory | Responsibility |
| --- | --- | --- |
| `:app` | `app/` | Application shell, permissions, foreground streaming service and session screen. |
| `:core` | `core/` | CameraX capture adapter, libwebrtc transport, signaling client and session state. |
| `:protocol` | `protocol/` | Kotlin implementation of the shared signaling and QR contract. Delivered package, coverage gate ≥ 90 %. |

## Prerequisites

- JDK 17.
- Android SDK with `compileSdk 36` and build-tools 36.0.0.
- Gradle 9.6.x and Android Gradle Plugin 9.4.1.
- Kotlin 2.4.x. AGP 9 provides built-in Kotlin support, so the
  `org.jetbrains.kotlin.android` plugin is deliberately **not** applied.

Versions are pinned in `gradle/libs.versions.toml`. CameraX 1.6.2,
`io.github.webrtc-sdk:android` 150.7871.01, ZXing core 3.5.4 and
kotlinx-serialization are declared there and consumed by the modules that need
them.

## Build and test

```bash
make build-mobile
make test
make coverage
make lint
```

`make coverage` runs the JaCoCo report for `:protocol`. The 90 % threshold
applies to the delivered contract package.

## Debugging

`make build-mobile` produces a debuggable APK. Attach Android Studio to
`mobile/` for a device debug session; `make test` runs the unit tests without a
device. The capture and transport modules are validated against
`spikes/evidence/` produced by the desktop harnesses and the Android interop
scenario.

## Cleanup

```bash
make clean
```

This removes `mobile/.gradle/` and module build directories. It never removes
committed sources, fixtures or evidence.
