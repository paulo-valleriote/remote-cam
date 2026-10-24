# Quarantined F0 Android app module

**NON-PRODUCTION — provenance only.** This is the `mobile/app` module from F0,
moved here in F1-01. It is not part of the `mobile/` Gradle build, has no
`settings.gradle.kts` of its own, and is not built.

It is retained because it records the F0 shell: `MainActivity`, the permission
flow, the QR display and the `RemoteCamApplication` entry point. The product
Android application is built in F2.

Do not import it from `desktop/` or `mobile/`, and do not promote it by
copying (FR-039, ADR-0003).
