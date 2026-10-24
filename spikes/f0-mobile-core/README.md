# Quarantined F0 Android core module

**NON-PRODUCTION — provenance only.** This is the `mobile/core` module from F0,
moved here in F1-01. It is not part of the `mobile/` Gradle build, has no
`settings.gradle.kts` of its own, and is not built.

It is retained because it records the F0 capture, transport and signaling
approach: `CameraXCaptureSource` (ImageAnalysis only), `YuvPlaneCopier`,
`StructuredLog`, `SignalingClient` (TLS 1.3 pinning) and
`PeerConnectionFactoryProvider` (host candidates only).

Do not import it from `desktop/` or `mobile/`, and do not promote it by
copying. Reusing an idea requires a new implementation in the product under the
normal review rules (FR-039, ADR-0003).
