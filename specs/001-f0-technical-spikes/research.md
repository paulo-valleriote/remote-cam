# Research: F0 — Risk-Reduction Spikes and Technical Decisions

**Feature**: [001-f0-technical-spikes](./spec.md) | **Date**: 2026-10-22 | **Plan**: [plan.md](./plan.md)

Every *NEEDS CLARIFICATION* marker in the plan template is resolved here. Each decision records the choice, the rationale, and the alternatives that were evaluated and discarded.

---

## 1. Decisions

### D-01 — Language split: Kotlin for mobile, Rust for desktop

- **Decision**: The Android capture/transmit application is **native Kotlin**; the Windows 11 receive/inject application is **Rust** across media core, signaling and UI.
- **Rationale**: Mandated by the product owner (*"use kotlin para mobile e rust para desktop"*) and already fixed by the source technical design (`docs/tdd-remotecam-v1.md` §5.1, §14). Rust gives a single static binary with no managed runtime, memory safety, and direct COM/Win32 interop through the `windows` crate. Kotlin gives direct CameraX/MediaCodec control, which is what the capture path needs.
- **Alternatives discarded**: Flutter and React Native for Android (plugins duplicate the capture pipeline and add latency and unpredictability — TDD §14); .NET 8 + Avalonia, Tauri, egui and WinUI 3 for desktop (Tauri keeps a WebView, the others break the single-language Rust requirement or add a managed runtime — TDD §14).
- **Consequence for F0**: Two independent toolchains and two independent test suites. The only shared artefact is the `protocol/` contract plus conformance vectors.

### D-02 — Rust toolchain: edition 2024, MSRV 1.92

- **Decision**: `edition = "2024"`, `rust-version = "1.92"`, pinned in `desktop/rust-toolchain.toml`; `Cargo.lock` is committed and updated only deliberately.
- **Rationale**: `webrtc` 0.21.1 and `slint` 1.18.1 both require edition 2024; `slint`'s MSRV (1.92) is the binding constraint. `webrtc` is 0.x and its own README warns that a minor bump may carry breaking changes, so exact pinning is mandatory, not optional.
- **Alternatives discarded**: Edition 2021 (incompatible with the required crate versions); tracking `webrtc` `main` (0.x churn plus no stable API guarantee).

### D-03 — Media transport: `webrtc` 0.21.1 (webrtc-rs), pinned exactly

- **Decision**: Use the `webrtc` crate 0.21.1 for ICE, DTLS-SRTP, RTP/RTCP, NACK and the receiver-side jitter buffer.
- **Rationale**: It covers exactly the transport layer the TDD chose and, critically, **host-candidate-only ICE is the default** when no ICE servers are configured — so "no STUN/TURN by construction" needs no extra enforcement. It also provides `JitterBufferInterceptor` with per-SSRC playout, duplicate collapsing and out-of-order handling.
- **Alternatives discarded**: A bespoke pipeline over QUIC/SRTP (reimplements loss handling, jitter buffering, synchronisation and crypto — TDD §14); `webrtc-sys` 0.3.50 bindings to libwebrtc (the most robust implementation, but it requires a heavy C++ build and duplicates a large part of the platform stack — kept as a V2 option).
- **Known gaps carried into F0**: **no NetEq**, **no decoder**, no ULP-FEC recovery in the receive path (the default codec registration no longer advertises `video/ulpfec`), and PLI generation is available but **off by default** — the interceptor chain must opt in explicitly.

### D-04 — Jitter buffer policy is ours, on top of the webrtc-rs interceptor

- **Decision**: Use `JitterBufferInterceptor` for packet ordering and playout, and implement the **adaptive policy** (target depth, playout-delay adaptation, fast-forward after a stall) in the media core.
- **Rationale**: The crate ships a *mechanism*, not the NetEq *policy* the product needs (RFC 3550 §4.5 target delay, adaptive playout, PLC/concealment). FR-015 additionally demands a fast recovery to baseline latency within 2 s after a 5 s stall, which no off-the-shelf buffer provides.
- **Alternatives discarded**: GStreamer `gstreamer-rs` (supplies a ready jitter buffer and synchroniser but adds a large plugin runtime and is explicitly rejected in TDD §14); `str0m` (Sans-I/O, but its API differs from the planned `webrtc` API and it would require re-plumbing).

### D-05 — H.264 decode: Media Foundation hardware path, NV12 output

- **Decision**: Decode on Windows through the Media Foundation H.264 transform, discovering the decoder with `MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER`, attaching an `IMFDXGIDeviceManager` for DXVA, and requesting **NV12** output.
- **Rationale**: NV12 is the **only 8-bit output format available while DXVA hardware decode is active**; requesting I420/IYUV silently forces the MFT back to software decode. Buffer access follows the documented order `IMF2DBuffer2::Lock2DSize` → `IMF2DBuffer::Lock` → `IMFMediaBuffer::Lock`, because `IMFMediaBuffer::Lock` on a 2-D buffer can force two copies.
- **Alternatives discarded**: `openh264` as the product path (it is constrained-baseline only and cannot decode the High-profile streams real encoders produce; kept for the Linux CI path only); `ffmpeg-next` (LGPL obligations for a closed-source product plus a heavy native build).
- **Finding that changes the plan**: `MFT_ENUM_FLAG_HARDWARE` returns **zero results on some platforms**. The `SYNCMFT` discovery path is therefore the documented procedure, and hardware capability must be **verified empirically on the reference machine** (FR-044) rather than assumed.
- **Precondition (Q1, PC-1)**: the check runs **before** the pipeline gate starts, and its result is recorded as gate evidence. If the reference machine does not provide hardware-accelerated decoding for the codec under test, the machine is **replaced** — not measured on. Results measured on a machine without hardware decode are reported separately and **can never pass the latency gate** or the resource gate.
- **Correction to the source TDD**: `IMFHardwareBuffer` **does not exist** as a Media Foundation interface. The correct GPU path is `IMF2DBuffer2` for CPU access and `IMFGetService(MR_BUFFER_SERVICE)` → `IMFDXGIBackBuffer` → `ID3D11Texture2D` for zero-copy GPU access.

### D-06 — Signaling: WebSocket over TLS 1.3 with an ephemeral, fingerprint-pinned certificate

- **Decision**: `tokio` 1.54.1 + `tokio-tungstenite` 0.30.0 for the WebSocket framing, `rustls` 0.23.46 for TLS 1.3, `rcgen` 0.14.10 to generate a **per-session ECDSA P-256** self-signed certificate in memory.
- **Rationale**: Matches TDD §5.1 and §5.3 exactly. `tokio-tungstenite`'s `accept_async` accepts any `AsyncRead + AsyncWrite` stream, so the custom server TLS configuration applies with no extra glue. `rcgen` 0.14.10's `KeyPair::generate()` produces ECDSA P-256 and `CertificateParams::self_signed` emits a `rustls-pki-types` certificate directly, with no PEM round-trip.
- **Alternatives discarded**: Plain HTTP with a token (violates the encrypted-connection requirement — TDD §14); WebRTC data channel (would require an unauthenticated bootstrap channel); mDNS discovery (adds discovery with no product gain).

### D-07 — Fingerprint pinning: custom `ServerCertVerifier`, isolated behind our own trait

- **Decision**: Implement a `rustls::client::danger::ServerCertVerifier` that hashes the **leaf certificate DER** with SHA-256 and compares it to the pinned digest in constant time. TLS 1.3 only, session resumption disabled, no root store consulted. The implementation sits behind a project-owned trait (`PinnedCertificateVerifier`) so a future `rustls` 0.24 migration is a single-file change.
- **Rationale**: The fingerprint is **SHA-256 over the DER-encoded leaf certificate** — not the SPKI, not the PEM. `verify_tls13_signature` must still validate the `CertificateVerify` signature against the pinned certificate's public key; returning success unconditionally would break signature binding.
- **Alternatives discarded**: Public CA / PKI (contradicts "no external PKI" — TDD §5.3); trust-on-first-use (the QR already carries the authoritative fingerprint — TOFU adds state, and state-free is an explicit invariant).

### D-08 — Desktop UI: Slint 1.18.1 under the Royalty-Free Licence

- **Decision**: Slint 1.18.1 for the Windows shell, under **LicenseRef-Slint-Royalty-free-2.0**.
- **Rationale**: Builds and debugs on Linux and ships the same UI on Windows, which is what makes the desktop code developable in the CI environment. The Royalty-Free Licence permits a **proprietary commercial** application at zero cost provided the `AboutSlint` widget (or the attribution badge on the download page) is displayed. No embedded system, so no per-device royalty.
- **Alternatives discarded**: **GPLv3 Slint** (would force the whole product to be GPL — unacceptable); Slint Commercial (paid, unnecessary); Tauri (keeps a WebView; TDD §18 question 12 records it as the V2 alternative if the design demands a web UI).

### D-09 — Windows interop crate: pin `windows` 0.62.2

- **Decision**: Pin the `windows` umbrella crate to **0.62.2**.
- **Rationale**: The `windows` 0.100.0 line is edition 2024 / MSRV 1.95, but Microsoft **deliberately did not publish the umbrella crates** for it; only `windows-core` and friends were published. `windows` 0.62.2 already exposes every Media Foundation symbol the project needs: `MFTEnumEx`, `IMFTransform`, `IMF2DBuffer2`, `IMFSample`, `IMFDXGIDeviceManager`, `MFCreateVirtualCamera`, `IMFVirtualCamera`, `MFVideoFormat_H264`, `MFVideoFormat_NV12`.
- **Alternatives discarded**: `windows-bindgen` 0.100.0 codegen (proc-macro build time and MSRV 1.95 for no extra API coverage); `windows-sys` (unsafe, lower-level, unnecessary).

### D-10 — Virtual camera backend: Windows 11 `MFCreateVirtualCamera` is primary; DirectShow is fallback

- **Decision**: The **primary** backend is the Windows 11 virtual camera API (`MFCreateVirtualCamera`, header `mfvirtualcamera.h`, library `mfsensorgroup.lib`, **minimum build 22000** / Windows 11 21H2), implemented from the MIT-licensed `microsoft/Windows-Camera` `Samples/VirtualCamera` reference. The **DirectShow** user-mode filter is retained only as a **fallback backend** for applications that cannot see the MF camera.
- **Rationale**: This is the single largest risk reduction in the phase. The API creates a **user-mode software component that is discovered and used by apps as if it were a hardware capture device** — no kernel driver, no WHQL, no attestation, no driver signing, no kernel-mode maintenance. It also removes the TDD's C++-exception risk: the MF path needs one COM media source (which we implement), whereas the DirectShow path needs a *new* DirectShow filter.
- **Alternatives discarded**: **Building a new DirectShow filter as the primary path** — Microsoft states plainly that *"DirectShow is a legacy feature… Microsoft strongly recommends that new code use MediaPlayer, IMFMediaEngine and Audio/Video Capture in Media Foundation instead"*; it would build new product code on a deprecated API using a codebase that is 2–4 years unmaintained. Kernel-mode camera driver (certification and months of work — explicitly out of scope). OBS Virtual Camera (GPL-2.0, no public injection API).
- **New risk introduced, and why F0 must test it**: `MFVirtualCameraAccess_CurrentUser` is documented as admin-free for the **camera registration scope**, but the **media source CLSID must be machine-wide COM-registered**, because the Frame Server services run as SYSTEM. Per-user (`HKCU`) COM registration is *plausible but undocumented*. F0 must resolve: (a) whether per-user registration actually works, (b) whether an **MSI** installer is required (MSIX registration of a virtual camera is **not supported** — the WinAppSDK request was closed and moved to Feedback Hub), and (c) whether a DirectShow/COM filter can be registered from MSIX at all.
- **Correction to the source TDD**: there is **no `Windows.Media.Capture` virtual-camera provider model**. The documented model is the Win32 Media Foundation `mfvirtualcamera` API. TDD §5.6 already names `MFCreateVirtualCamera` correctly; the wording "media source próprio" is retained and the `Windows.Media.Capture` phrasing is removed.

### D-11 — Do not vendor `schellingb/UnityCapture`

- **Decision**: The DirectShow fallback will **not** be based on `schellingb/UnityCapture`.
- **Rationale**: The repository asserts MIT in its README but **ships no LICENSE file**, and the GitHub API reports no detected licence — an ambiguous provenance that cannot be vendored into a commercial product on a good-faith reading. It was last pushed 2023-05-25.
- **Alternatives discarded**: `mrayy/UnityCam` (MIT **with** a LICENSE file, 2022) and `huahuahuage/Virtual-Camera-For-Windows` (MIT with a LICENSE file, 2025) — acceptable provenance if a DirectShow fallback is required; `microsoft/Windows-Camera` and `smourier/VCamSample` (MIT, both actively maintained) for the MF path; `obsproject/obs-studio` (GPL-2.0 — excluded).

### D-12 — Virtual audio device: VB-CABLE, user-installed, link-only, no redistribution (V1)

- **Decision**: For V1, RemoteCam **detects and verifies** a user-installed **VB-CABLE** base package and links the user to the official download. It does **not** redistribute the driver.
- **Rationale**: VB-Audio's official terms explicitly permit redistribution *with* an application if the donationware model stays applicable (the user must see and identify it as a VB-Audio application and be able to donate), but a link-only model avoids that condition entirely and therefore the legal review of it. VB-CABLE provides exactly the required topology: audio rendered to **CABLE Input** is captured by consumer applications from **CABLE Output**. It is actively maintained (driver pack 45, October 2024) with a 28-year track record, and its per-unit volume price (€3.61 at 100 units, €2.92 at 1 000) makes redistribution affordable if the decision later moves that way.
- **Alternatives discarded**: **Virtual Audio Cable (VAC)** — a proprietary paid shareware (BUSINESS licence $50, one per running instance) with tightly restricted redistribution; it does have a sanctioned **custom/OEM version** ("compiled with unique vendor, product and driver/device names… works only together with your companion application"), which is the better long-term shape but requires a quote and a legal opinion on the "auxiliary transport" characterisation. **VoiceMeeter** — a mixer, not a cable; it takes over default-device selection, which conflicts with the rule that RemoteCam never changes consumer application device selection; Potato cannot be bundled and uses activation codes. **Scream** — architecturally wrong (it publishes PCM to the LAN and exposes **no capture endpoint**), MS-PL not MIT, and its installation instructions require Secure Boot off and test-signing mode. **SteelSeries Sonar** — no redistribution licence and grabs default devices. **Open-source permissive drivers** (MikeTheTech, DeckX, AudioMirror) — licence is fine but the blocker is **kernel signing**: every one is test-sign-only or self-described as unfinished.
- **Unresolved, carried to the gate**: the **WHQL / attestation signature status of the VB-CABLE driver is not stated by the vendor**. F0 must verify it empirically (`Get-AuthenticodeSignature` on the `.cat`/`.sys`, plus `signtool verify /pa /v`) on a Secure-Boot-enabled Windows 11 runner. The other candidate devices have the same gap.
- **Pinning rule (Q5)**: the device used for the **technical measurement** is pinned by product and version in the recorded procedure — here, **VB-CABLE, base package, driver pack 45**. The licence terms of **every candidate device considered** are assessed **in parallel on the same test steps**, including the candidates that were **not** used for the measurement. A later change of the selected device therefore does not invalidate the technical evidence: the evidence stays valid for the device actually measured, and the newly selected device must be **re-measured against the same recorded steps** before it becomes the shipping option.
- **Correction to the source TDD**: the official domain `muteka.com` is dead (parked). The live official site is `vac.muzychenko.net`. Any support link must use the live domain.

### D-13 — Plan B: video-only V1, with an own-driver track deferred to V2

- **Decision**: The approved fallback is **V1 ships video-only** if the licensing decision is not closed by the gate date. Writing our own signed WDM virtual audio driver is **rejected for V1** and scheduled for V2 behind a new TDD.
- **Rationale**: A custom WDM/WaveRT virtual audio driver is estimated at **60–90 dev-days** (driver 25–40, user-mode engine 8–12, INF and install 4–6, signing pipeline 5–10, HLK optional 10–20, test matrix and soak 10–15) plus an EV code-signing certificate and a Partner Center attestation pipeline — roughly a third of the entire V1 budget (≈ 205 dev-days). Microsoft states that *"virtual drivers have the same requirements as actual hardware drivers"*, and attestation signing cannot publish to Windows Update for retail audiences. Video-only is a coherent, sellable product ("phone as a webcam"), and the camera half is now fully self-owned thanks to D-10.
- **Alternatives discarded**: Shipping our own driver in V1 (cost, signing and HVCI/WDAC maintenance risk — TDD §6 already lists "driver scope growth" as a High/Medium risk with the mitigation "any change requires a new TDD and approval"); shipping the driver unshipped and unsigned (Windows 11 rejects it with Secure Boot enabled).

### D-14 — Android toolchain: Kotlin 2.4.x, AGP 9.4.x, Gradle 9.6.x, JDK 17

- **Decision**: Kotlin 2.4.x, AGP 9.4.x, Gradle 9.6.x, JDK 17, `minSdk` 29, `targetSdk` 36, `compileSdk` 36, `jvmTarget` 17, with `coreLibraryDesugaring` enabled. Versions pinned in `mobile/gradle/libs.versions.toml`.
- **Rationale**: `minSdk` 29 already provides everything the capture path needs (MediaCodec async callbacks, `KEY_I_FRAME_INTERVAL` as float, `KEY_MAX_B_FRAMES`, `KEY_PRIORITY`, thermal APIs). `targetSdk` 36 introduces **no new camera/microphone foreground-service restrictions** — only a quota on background jobs started from a foreground service, which does not affect a streaming service.
- **Alternatives discarded**: `targetSdk` 37 / Android 17 (it makes local-network access a runtime permission; that is a V2 concern, but the permission is declared now so the migration is mechanical); JDK 21 (not required and not what AGP tests against).
- **Build-system note**: AGP 9 enables built-in Kotlin support — the `org.jetbrains.kotlin.android` plugin must be **removed** from the `plugins {}` block.

### D-15 — Android capture: CameraX 1.6.2 `ImageAnalysis` → `JavaI420Buffer.wrap`

- **Decision**: Bind **only** `ImageAnalysis` (no `Preview` surface) and feed `ImageProxy` planes into libwebrtc via `JavaI420Buffer.wrap(width, height, dataY, strideY, dataU, strideU, dataV, strideV, null)`, then hand the `VideoFrame` to the capturer observer.
- **Rationale**: No preview surface means no extra ISP work, lower latency and less heat — which is exactly what the CameraX thermal guidance recommends. `wrap` copies nothing, so the capture path is effectively zero-copy.
- **Mandatory runtime guard**: `ImageProxy.PlaneProxy.getPixelStride()` for the chroma planes is **1 on planar I420 devices but 2 on devices whose `YUV_420_888` is really NV21/NV12**. `JavaI420Buffer.wrap` assumes pixel stride 1, so a device with stride 2 produces swapped chroma (green/magenta frames). The capture adapter **must** check the pixel stride at runtime and de-interleave into a scratch I420 buffer when it is 2. This is the highest-risk single item in the mobile plan.
- **Alternatives discarded**: A self-managed MediaCodec encoder fed by a CameraX `SurfaceProcessor` (`CameraEffect` / OpenGL) — lower copy count, but an OES texture cannot be handed to libwebrtc from application code, so it forces a separate encode path and loses libwebrtc's RTP packetisation and PLI handling; libwebrtc's native Camera2 capturer (the plan B if the conversion cost busts the latency budget — TDD §5.4).

### D-16 — Encoding and audio: use libwebrtc's own encoders

- **Decision**: Let **libwebrtc** perform the H.264 encode (its `MediaCodecVideoEncoder`) and the Opus encode (its built-in Opus encoder). Do not implement a MediaCodec encoder or an Opus encoder in the app.
- **Rationale**: libwebrtc's Opus defaults are already exactly the product specification: 48 kHz, FEC **on** (`useinbandfec=1`), DTX **off**, 32 kbps mono fullband, `OPUS_APPLICATION_VOIP` for mono. Its H.264 path already negotiates profile/level, emits SPS/PPS via `sprop-parameter-sets`, requests PLI on loss and packetises RTP. Building either encoder ourselves would re-create work libwebrtc already does correctly.
- **Consequence to state in the report**: mono forces the VOIP application mode, which limits audio bandwidth — acceptable and desirable for a microphone, but it must be documented rather than discovered later.
- **Alternatives discarded**: `io.element.android:opusencoder` (wraps libopusenc and writes **Ogg files**, not RTP packets — wrong tool); `io.voxkit:kopus` (viable but a 4-star project, unnecessary while libwebrtc is already a dependency); `MediaCodec` Opus (the platform ships an Opus **decoder** only — there is no public Opus encoder).

### D-17 — libwebrtc artefact: `io.github.webrtc-sdk:android:150.7871.01`

- **Decision**: Use the community AAR `io.github.webrtc-sdk:android:150.7871.01` (Maven Central), pinned exactly. Use the `-stripped` variant only if the software-codec size is a problem.
- **Rationale**: `org.webrtc:google-webrtc` has been **dead since 2020** (last version `1.0.32006`). The WebRTC team publishes **source only** — there is no official prebuilt AAR. The community build is the de-facto standard. The licence is BSD-3-Clause, and libwebrtc delegates H.264 to the **platform MediaCodec encoder**, so no patent-encumbered codec binary is bundled.
- **API facts that break most tutorials**: `CapturerObserver` is now a **top-level interface** with only `onCapturerStarted`, `onCapturerStopped` and `onFrameCaptured` — `onByteBufferFrameCaptured` and `onTextureFrameCaptured` **no longer exist**. `MediaConstraints.OfferToReceiveAudio`/`OfferToReceiveVideo` **no longer exist**. `SessionDescription.copy(...)` appears removed. `NV21Buffer` appears absent from the public Java API.
- **Alternatives discarded**: Building libwebrtc from source (build cost and the H.264 patent obligations that come with bundling a software encoder); the dead `org.webrtc:google-webrtc`.

### D-18 — Send-only direction via `RtpTransceiver`

- **Decision**: Create both transceivers with `RtpTransceiver.RtpTransceiverInit(RtpTransceiverDirection.SEND_ONLY, listOf("remotecam"))` and `addTransceiver(track, init)`.
- **Rationale**: `addTrack` alone produces **`a=sendrecv`**, which violates invariant I2. `RtpTransceiver` with `SEND_ONLY` produces `a=sendonly`. `PeerConnection.RTCConfiguration` with an **empty `iceServers` list** yields host candidates only — no STUN, no TURN — which is the "disabled by construction" requirement, and `networkIgnoreMask` can additionally exclude cellular and VPN adapters.
- **Alternatives discarded**: SDP munging after the fact (fragile); `OfferToReceive*` constraints (removed from the API).

### D-19 — QR: `com.google.zxing:core` 3.5.4 on Android, `qrcode` 0.14.1 on desktop

- **Decision**: On Android, use **only** `com.google.zxing:core` 3.5.4 driven by the app's own CameraX `ImageAnalysis`, decoding the Y plane and using `Result.getBytes()` (not `getText()`, which assumes UTF-8 and would mangle a binary payload). On the desktop, use `qrcode` 0.14.1 with the **explicit `Bits` byte-mode API**, `EcLevel::M`, and a fixed version.
- **Rationale**: `com.journeyapps:zxing-android-embedded` 4.3.0 was last released in 2021, is built on the **deprecated Camera1 API**, and bundles zxing core 3.4.1 — it must not be used. The desktop `qrcode` crate supports raw byte-mode payloads and the low-level `Bits` API makes the encoding mode deterministic and identical on both sides.
- **Alternatives discarded**: `com.google.mlkit:barcode-scanning` (Play Services dependency or a ~3 MB bundled model for one code); JSON-in-URL QR payloads (larger, and the TDD chose a compact binary payload with a 512-byte ceiling).

### D-20 — Capture-at-consumer latency: per-frame QR, decoded from the consumer's own capture

- **Decision**: Encode `{epoch_ms}|{frame_no}` into a QR **burned into every frame** of the synthetic source, capture the consumer application's own output, decode the QR from each captured frame, and compute `host_capture_time − source_epoch`.
- **Rationale**: This is the established software technique (the AWS sample burns `Date.now()` and a QR per frame and decodes at the viewer). It is self-describing, survives H.264 compression, needs no OCR, and gives both wall-clock and frame identity.
- **Definition that must be stated in the report**: a virtual camera has **no glass**. The optical LED/photodiode method is inapplicable; the only defensible definition is **capture-at-consumer latency**, which **upper-bounds** the user-perceived latency because it includes the consumer application's render path. The figure is named *capture-at-consumer latency* throughout, not "glass-to-glass" — that term now denotes only the floor-corrected value (D-29).
- **Mandatory calibration**: the screen-grab path adds its own latency (DWM + compositor, typically 1–3 frames at 60 Hz ≈ 17–50 ms — the same order as the entire budget). The procedure **must** measure the harness floor by rendering the same QR in a local HTML page and capturing it with the identical command, and the report **must** state the raw value, the floor and the floor-corrected value (D-29).
- **Clock alignment**: the synthetic source runs **on the PC**, so source and capture share one clock and the offset is zero by construction. For the phone→PC path, NTP is not viable (the reference environment has internet disabled, so the phone's wall clock is unreliable) — instead an **application-level sync frame** carries `elapsedRealtimeNanos` and `currentTimeMillis`, repeated every 30 s, and the offset is regressed against time.
- **Alternatives discarded**: LED + phototransistor (no physical display or camera in the loop); NTP/PTP clock alignment (no internet, no PTP grandmaster, no hardware timestamping on a phone); frame-count-only latency (cannot separate a pipeline stall from a consumer drop).

### D-21 — Dropout detection: `ffmpeg silencedetect` plus an RMS cross-check, with a calibration fixture

- **Decision**: Primary detector is `ffmpeg -af silencedetect=noise=<calibrated>dB:d=0.05,ametadata=mode=print:file=…`; a Python RMS-windowing script with hysteresis is the cross-check; a committed fixture with known 40/60/120 ms gaps validates the toolchain before any measurement. This is the **automated half** of the "continuous and intelligible audio" criterion (Q3); the scripted human check is D-22.
- **Rationale**: `d=0.05` **is** the FR-030 criterion, and in FFmpeg ≥ 5 the filter is sample-accurate (in ≤ 4.x it was per-frame, so FFmpeg ≥ 8.1 is pinned). `silencedetect` uses a **peak** metric, which is the weakness: a single click resets the silence timer and can mask a genuine gap, and a single stray sample splits one 100 ms gap into two 49 ms gaps. The RMS cross-check exists precisely to catch that disagreement.
- **Threshold calibration is mandatory**: too high (−30 dBFS) produces false positives on quiet speech; too low (−70 dBFS) produces **false negatives**, because Opus packet-loss-concealment comfort noise typically sits around −55…−45 dBFS and would never trip the detector — the dangerous direction, because the gate would report "zero dropouts" on a broken stream. The threshold is set at **noise floor + 6 dB** measured through the real consumer chain, and a sensitivity sweep is published.
- **Alternatives discarded**: `sox silence` (**removes** silence rather than detecting it, and its threshold is relative to full scale, not to the recording); `librosa.effects.split` (threshold is relative to the loudest frame, so it moves with programme level — the wrong semantics); `pyloudnorm` (integrated LUFS, a loudness metric, not a silence detector).

### D-22 — Intelligibility: scripted phrase manifest, human exact-rate as the primary metric

- **Decision**: A committed manifest of **20 phrases** — 1 opening anchor, 8 static, 10 nonce-bearing, 1 closing anchor — played through the chain and transcribed by a human observer. Primary criterion: **≥ 18 of 20 non-anchor phrases transcribed with zero errors** and corpus WER ≤ 0.05, with every nonce word matching. ASR word-error-rate and per-phrase ESTOI are recorded as **supplementary trend evidence only**. The **phrase script, the observer and the outcome** are recorded (Q3, FR-029) — this is the **human half** of the "continuous and intelligible audio" criterion, paired with the automated dropout analysis of D-21.
- **Rationale**: Nonce words (real English words, not nonsense strings) detect a stale or spliced recording — the cheapest defence against an observer transcribing from memory. A per-phrase exact rate is the number a gate can actually be argued about; a corpus WER over 120 words is too coarse to catch a single gap. ESTOI requires an aligned clean reference and resamples to 10 kHz, discarding exactly the band where Opus artefacts live, so it can only be a trend.
- **Alternatives discarded**: ESTOI as a threshold metric (alignment error corrupts the score; the signal length changes through the jitter buffer and resampler); ASR WER as the sole metric (it conflates intelligibility with recogniser accuracy and hallucinates on silence).

### D-23 — CPU and memory: `GetProcessTimes` + `GetProcessMemoryInfo`, machine-normalised, with a `typeperf` cross-check

- **Decision**: Sample with the `windows` crate 0.62.2: `GetProcessTimes` for the process, `GetSystemTimes` for the machine, `GetProcessMemoryInfo` for the working set and private usage. Report **CPU as a percentage of the whole machine** (`100 × Δproc / Δsys`) and gate on **p95 ≤ 15 %**. Cross-check against `typeperf` on the same run.
- **Rationale**: Three different CPU numbers exist and confusing them is the most likely error in the whole procedure: Task Manager shows 0–100 across the machine; PDH's `\Process(x)\% Processor Time` shows 0–100 × N. **A `typeperf` value of 45 on a 12-thread machine means 3.75 %.** The `sysinfo` crate is unsuitable for 30 fps sampling on Windows because its `MINIMUM_CPU_UPDATE_INTERVAL` is **200 ms** and it early-returns a stale value below that.
- **Pitfalls that must be in the procedure**: take a burn-in sample before the measurement window (the first delta against a zero baseline is garbage); report p50/p95/p99/max, never only the mean (a 3-second 100 % spike hides in a 30-minute mean); name the memory metric explicitly (the 400 MB gate uses **private working set**, which is what Task Manager shows, while `WorkingSetSize` includes shared pages); run the sampler from the **same non-elevated session** as the app.
- **Alternatives discarded**: WPR/WPA (requires administrator, produces hundreds of MB of ETL for a 30-minute run, and its GUI analysis breaks the one-working-day reproducibility requirement — kept as a diagnostic tool for when the gate fails); `sysinfo` 0.39.6 as the primary (200 ms floor).

### D-24 — A/V drift: measure the receiver, not the sender

- **Decision**: Carry the **source capture timestamp in an RTP header extension**, so at the receiver the source-time difference between an audio packet and a video packet is directly computable; add the receiver's per-stream playout delay; regress the result against time to obtain ms/hour. A marker-based method (a video flash plus a 1 kHz audio tone burst, cross-correlated per marker) provides independent confirmation.
- **Rationale**: The sender's two RTP clocks come from the same Android monotonic clock, so the **sender-side skew is exactly zero and never drifts — all measurable drift is in the receiver**. The RTCP sender-report offset cannot detect receiver-side drift for the same reason, and the sender's wall clock is untrusted.
- **The finding that most threatens FR-014**: libopus's fixed-point resampler has a relative frequency error of about **±1e-5**, which over one hour is **±36 ms** — **more than the 20 ms/hour criterion on its own**. A free-running resampler therefore fails the gate by design. The audio renderer **must be paced from the RTP timestamps** (a PLL-style ratio adjustment, or a resampler ratio derived from `ΔRTP / Δwall-clock` over a sliding window). F0 must measure this explicitly rather than discover it after the gate.
- **Measurement resolution**: with a 30 s marker period over 30 minutes there are only 60 markers, giving a ±267 ms/hour 95 % confidence interval — useless for certifying 20 ms/hour. Cross-correlation per marker against a ±2 s window, or a 2–5 s marker period, is required.
- **Alternatives discarded**: RTCP sender-report anchoring alone (cannot see receiver-side drift); a single marker at the start and end (no regression power).

### D-25 — Impairment: a seeded, RTP-aware userspace relay in Rust

- **Decision**: Implement the impairment injector as a Rust/tokio **userspace UDP relay** that parses RTP and impairs **only the media SSRCs**, leaving ICE, DTLS and RTCP untouched. Loss and jitter come from a seeded PRNG (`StdRng::seed_from_u64(seed)`); the seed is recorded in the report. A 5-second stall is implemented by holding media packets and releasing them in a burst. `tc netem` is documented as the Linux alternative.
- **Rationale**: An IP-level tool would also drop DTLS and ICE packets, which breaks the connection and presents as a product bug rather than as media loss. The userspace relay is **RTP-aware**, **deterministic** (a second engineer reproduces the exact loss pattern from the seed), runs **on the same Windows machine** so the phone↔PC link stays real Wi-Fi, and models a 5-second stall exactly.
- **Mandatory cross-check**: the report must state the **injected** loss (from the relay's JSONL log) and the **residual** loss at the application after NACK retransmission, and must note that the product performs **no audio retransmission**, so audio losses are permanent — which is exactly what FR-030's continuity check exists to catch.
- **Pinned profile (Q7)**: the relay is configured with exactly **one** impairment profile — **3 % uniform loss, 30 ms added latency, ±10 ms jitter** — and the profile is recorded with the run evidence. A run that deviates from it, **including burst loss**, is judged on detected dropouts rather than on average loss percentage and is not used to pass the gate (D-30).
- **Also required**: a 0 %-loss control run, and a real-Wi-Fi baseline (`iperf3` + `ping`) recorded before and after each media run, so the 3 % figure is correctly labelled a **stress scenario** rather than a reproduction of the reference environment.
- **Alternatives discarded**: `tc netem` directly (Linux only; IP-level; the default `limit 1000` packets is ≈ 3.8 s of buffer at 2.5 Mbps, which conflates queue overflow with the intended stall — `limit 10000` is required); `tc netem` inside WSL2 (host↔LAN traffic bypasses the WSL virtual switch); WinDivert-based IP-level tools (Windows-capable but not scriptable or not RTP-aware).

### D-26 — Repository layout: `protocol/` and `spikes/`

- **Decision**: Add two new first-level directories: `protocol/` (shared versioned contract) and `spikes/` (non-production F0 harnesses and evidence).
- **Rationale**: The constitution requires shared contracts to be "extracted into a dedicated, versioned, and tested module, or exposed through a documented network interface", and requires `desktop/` and `mobile/` to be self-contained with no cross-imports. A language-neutral contract directory satisfies both. FR-039 requires spike artefacts to be "clearly marked as non-production and MUST NOT be merged into product modules" — a separate tree makes that structural rather than a convention.
- **Alternatives discarded**: Putting the contract in either application (breaks self-containment); putting the contract in a shared Rust crate consumed by Kotlin via FFI (reintroduces cross-language coupling and a build-toolchain dependency in the mobile app); keeping spike code inside `desktop/` (violates FR-039).

### D-27 — Coverage scope: the gate applies to delivered packages

- **Decision**: Enforce **≥ 90 % coverage in CI** for `protocol/` and, from F1 onward, for every product package. `spikes/` is exempt from the unit-coverage gate and is validated instead by the recorded procedure and per-gate evidence.
- **Rationale**: The constitution scopes the gate to "every **delivered** package". FR-039 classifies the spike harnesses as disposable, non-production prototypes that must not be merged into product modules. Measuring unit coverage of Windows COM adapters and FFmpeg-driven analysis scripts that are deleted after F0 would consume the 20 dev-day budget without reducing product risk.
- **Fallback**: if the maintainer rejects the exemption, `protocol/` still meets ≥ 90 % on its own and the spike packages are reclassified as non-delivered tooling rather than product packages.

### D-28 — The root `Makefile` is a constitution requirement, not a convenience

- **Decision**: The root `Makefile` provides `run`, `test`, `coverage`, `lint` and `debug` targets for both applications plus the spike and measurement targets, and **every command documented in a README must exist as a Makefile target**.
- **Rationale**: Constitution Principle III requires it explicitly and blocks a merge when the execution documentation is out of date.

### D-29 — The latency gate judges the raw capture-at-consumer p95, and publishes the floor and the floor-corrected value alongside it

- **Decision**: The gated statistic is the **raw p95 capture-at-consumer latency**, threshold **≤ 150 ms**, over three runs of at least 10 minutes at 1280×720@30. Every latency artefact carries three figures: the **raw** p95, the **measured harness floor**, and the **floor-corrected** p95 (raw minus the floor). The floor-corrected figure is reported and is **never** the value the gate is judged on.
- **Rationale**: A virtual camera has no glass, so capture-at-consumer latency is the only defensible definition (D-20). The capture procedure adds its own delay — 17–50 ms, the same order as the entire ≈ 100 ms budget — and that delay is attributable to the measurement apparatus, not to the product under test. Substituting the floor-corrected figure for the raw one would silently discount the product's own contribution; substituting the raw figure for the corrected one without publishing the floor would hide the apparatus's contribution. Publishing all three separates the two causes and makes the verdict arguable.
- **Alternatives discarded**: Judging the floor-corrected value (it forgives a product regression that happens to be masked by the apparatus floor); judging the raw value alone (it makes a harness-limited result indistinguishable from a product-limited one); dropping the budget entirely because the floor dominates it (the budget is the only way to locate the time).
- **Consequence for F0**: A failure is attributed to the capture procedure or to the product, and the attribution is stated. The report MUST state which stages are inside and which are outside the measured figure (FR-010).

### D-30 — The impairment profile is one pinned nominal scenario, and a deviation is never used to pass a gate

- **Decision**: Exactly **one** impairment profile exists: **3 % uniform loss, 30 ms added latency, ±10 ms jitter**, applied to the **media stream only**. It is named in the procedure, recorded with every loss-and-jitter run, and is the sole condition under which a loss-and-jitter gate scenario may pass. A run that deviates from it — **including burst loss** — is judged on detected dropouts rather than on average loss percentage, is labelled as a deviation, and never passes a gate.
- **Rationale**: "3 % packet loss with jitter" is not reproducible until the loss distribution, the added delay and the jitter amplitude are pinned. Uniform loss plus a fixed delay plus a bounded jitter amplitude is the smallest parameter set that still exercises concealment, and pinning it means two engineers run the same condition. The **media-only** scope is a correctness requirement, not a convenience: an IP-level tool would also drop ICE and DTLS packets, which breaks the connection and presents as a product defect rather than as media loss (D-25).
- **Alternatives discarded**: Burst loss as an additional gate scenario (it exercises a different failure mode — it cannot be judged by an average loss percentage, and it needs a separate concealment criterion that F0 has no budget for); a sweep over several loss percentages (the parameter space is spent on the one scenario the gate names); `tc netem` at the IP level (impairs control traffic).

### D-31 — F0 gates video latency only; audio latency is deferred to F2/F3

- **Decision**: **No** end-to-end audio latency threshold is measured or gated in F0. The ≤ 120 ms p95 audio latency target from the source TDD remains a **V1 product target**, carried forward to F2/F3, where the real mobile capture path exists. F0 measures audio **continuity** and **intelligibility** under the impairment profile, and **drift** — but not audio latency.
- **Rationale**: End-to-end audio latency requires the real mobile capture path (microphone capture, AudioRecord, and the mobile-side encode path). F0's pipeline measurement uses a synthetic source on the PC, so an audio latency figure produced in F0 would omit the mobile stages and would not be comparable with the F2/F3 figure. Gating on it would create a number that looks authoritative and is not. Audio **continuity** under loss, in contrast, is fully measurable in F0 with a synthetic source and is a real risk: the product performs no audio retransmission, so a lost audio packet is permanent (D-25).
- **Alternatives discarded**: Measuring audio latency in F0 against a synthetic source and labelling it provisional (a provisional number on the critical path of the phase review invites it being treated as final); gating audio continuity on a mobile capture path (out of F0 scope, and it would invalidate the reproducibility requirement FR-038).

### D-32 — Pairing throttling is a proven property of the gate, not a documentation claim

- **Decision**: The pairing gate **proves** the per-origin attempt limit of **5 attempts per minute**, the **progressive blocking** that follows it, and the **pairing listener shutdown after 10 consecutive failures**, with every throttling decision recorded with its reason code and its outcome. The progressive blocking is also exercised under a **concurrent legitimate pairing** from a second origin, to prove that the throttling of one origin does not deny service to another.
- **Rationale**: These thresholds come from the source design (TDD §5.3 / §8) and are the only defence of the pairing listener against a token brute-force. A design document asserting them is not evidence; the gate is the only place the property can be observed. The per-origin scope is the part most likely to be implemented incorrectly — a global limit would let one hostile origin deny pairing to the legitimate user, which is a denial-of-service bug rather than a security control.
- **Alternatives discarded**: Treating the rate limit as a configuration value verified only by unit test (it does not prove the listener actually shuts down, and it does not prove the per-origin scoping); a global attempt limit (it converts an attack into a denial of service against the legitimate user); testing only the 6th attempt (it skips the progressive-blocking escalation and the listener shutdown, which are the parts that can leave a listener exposed).

### D-33 — Pairing residue is proven after a normal session end **and** after an application restart, with one inspection method

- **Decision**: The pairing gate proves **zero** pairing artefacts — session key, token, fingerprint, pairing record — in **both** cases: after a normal session end, and after an application restart. The **same inspection method** is used in both cases, and the method and its coverage are recorded with the evidence.
- **Rationale**: A normal session end exercises the clean teardown path. An application restart exercises the path where the process died without unwinding — the case where an implementation that only clears state on a graceful close leaves artefacts behind. Proving only the clean path is the weaker claim, and the restart path is exactly where a persistence bug hides. One method used twice makes the two results comparable and keeps the procedure reproducible by a second engineer.
- **Alternatives discarded**: Proving only the normal session end (leaves the unclean path untested); using a different, deeper inspection for the restart case (the two results would not be comparable, and the deeper method would not have been exercised against the clean case); inspecting the registry only (the artefacts may live in memory-mapped files, in a crash-recovery journal, or in the platform certificate store).

---

## 2. Dependency ledger

Required by constitution Principle I: *"problem solved, alternatives discarded, maintenance cost, and license"*.

### Desktop (Rust)

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `webrtc` | 0.21.1 (exact) | ICE host-only, DTLS-SRTP, RTP/RTCP, NACK, jitter buffer | `webrtc-sys` (heavy C++ build); GStreamer; custom QUIC/SRTP | High — 0.x, breaking minor bumps; exact pin + committed `Cargo.lock` | MIT OR Apache-2.0 |
| `rustls` | 0.23.46 | TLS 1.3 with a custom pinning verifier | OpenSSL bindings; `native-tls` | Medium — 0.24 renames the verifier trait; isolated behind our own trait | MIT OR Apache-2.0 |
| `rcgen` | 0.14.10 | Ephemeral per-session ECDSA P-256 certificate in memory | `openssl`; hand-rolled X.509 | Low | MIT OR Apache-2.0 |
| `tokio` | 1.54.1 | Async runtime for signaling and the media core | `async-std`; OS threads only | Low | MIT |
| `tokio-tungstenite` | 0.30.0 | WebSocket framing over TLS | Raw WebSocket implementation | Low | MIT |
| `slint` | 1.18.1 | Native, WebView-free desktop UI that also builds on Linux | Tauri (WebView); egui; WinUI 3 (Windows-only) | Medium | GPL-3.0 **or** Royalty-Free 2.0 **or** Commercial — **Royalty-Free 2.0 chosen**, which requires displaying the `AboutSlint` widget or the attribution badge |
| `windows` | 0.62.2 (exact) | Media Foundation, WASAPI, COM, DXGI | `windows-sys` (unsafe); `windows-bindgen` 0.100.0 (unpublished umbrella) | Medium — umbrella 0.100.0 unpublished; 0.62.2 is edition 2021 | MIT OR Apache-2.0 |
| `qrcode` | 0.14.1 | Deterministic binary-mode QR generation on the desktop | `qrcodegen`; image rendering by hand | Low | MIT OR Apache-2.0 |
| `sha2` | 0.11.1 | Fingerprint computation | `ring` (already in the tree via `webrtc`) | Low — 0.11 was a breaking bump | MIT OR Apache-2.0 |
| `serde` | 1.0.229 (exact) | Derive-based parsing of the JSON signaling contract and its vectors | Hand-written JSON traversal; `simd-json` (no benefit at this size) | Low — mature 1.x, exact pin for reproducible builds | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 (exact) | JSON parsing for the contract consumer and the conformance binary | `simd-json`; `json` | Low — exact pin, no unsafe parsing | MIT OR Apache-2.0 |
| `openh264` | 0.9.9 | Software H.264 decode **in CI only** | `ffmpeg-next` (LGPL) | Low — constrained baseline only, never the product path | BSD-2-Clause (Cisco), patent royalties covered |
| `criterion` | 0.8.2 | Latency and jitter micro-benchmarks | `iai` | Low | MIT OR Apache-2.0 |

### Mobile (Kotlin)

| Dependency | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `androidx.camera` (CameraX) | 1.6.2 | Camera lifecycle, lens selection, YUV frame delivery | Camera2 directly; libwebrtc's native Camera2 capturer | Low — 1.6 required from 2026-11-01 for Extensions | Apache-2.0 |
| `io.github.webrtc-sdk:android` | 150.7871.01 (exact) | Peer connection, H.264 and Opus encoding, RTP packetisation, PLI | Dead `org.webrtc:google-webrtc`; building from source | Medium — community build; API churn between M-versions | BSD-3-Clause |
| `com.google.zxing:core` | 3.5.4 | Offline QR decoding | `zxing-android-embedded` (unmaintained, Camera1); ML Kit (Play Services) | Low | Apache-2.0 |
| `kotlinx-serialization` | 1.12.0 JSON runtime, Kotlin 2.4.10 compiler | Strict-schema JSON for signaling messages | `org.json`; Moshi | Low — the compiler plugin is not applied because AGP 9's built-in Kotlin (KGP 2.2.10) predates metadata 2.4.0; the build pins KGP 2.4.10 in the root buildscript for the runtime only | Apache-2.0 |
| CameraX `camera-camera2` | 1.6.2 | `Camera2Interop` for fps range and video-call stream use case | Camera2 only | Low | Apache-2.0 |

### Windows vendor components

| Component | Version | Problem solved | Alternatives discarded | Maintenance cost | License |
| --- | --- | --- | --- | --- | --- |
| `microsoft/Windows-Camera` → `Samples/VirtualCamera` | pinned commit | Reference implementation of the MF virtual camera source | Writing from scratch; DirectShow filter | Low — Microsoft-maintained, updated 2026-09 | MIT |
| VB-CABLE | driver pack 45 (Oct 2024) | Virtual microphone endpoint | VAC (paid, restricted); VoiceMeeter (mixer); Scream (wrong architecture); own driver (60–90 dev-days) | Low — we ship no binary; the user installs it | Donationware; redistribution permitted only if the donationware model stays applicable — **V1 uses link-only** |
| `smourier/VCamSample` | pinned commit | Second reference for the MF virtual camera | `mrayy/UnityCam`; `schellingb/UnityCapture` (**no LICENSE file — rejected**) | Low | MIT |

### Measurement tooling (not shipped)

| Tool | Version | Purpose | License |
| --- | --- | --- | --- |
| FFmpeg | ≥ 8.1 (pin 8.1.3 or 9.0.2) | Synthetic source, `silencedetect`, `gdigrab`, `astats` | LGPL/GPL depending on build — used only offline, never redistributed |
| `zxing-cpp` | 3.1.1 | QR decoding of captured frames | Apache-2.0 |
| `qrcode` (Python) | 8.2 | QR generation for the self-test round-trip and the harness-floor calibration | BSD-3-Clause |
| `pillow` | 13.0.0 | Rasterising the generated QR symbol for the decoder | HPND |
| `jsonschema` | 4.26.0 | Validating evidence against the contracts' JSON Schemas | MIT |
| `pytest` / `ruff` | 9.1.1 / 0.17.1 | Test runner and linter for the measurement package | MIT |
| `numpy` / `scipy` | 2.5.4 / 1.18.1 | Signal analysis | BSD-3-Clause |
| `jiwer` | 4.0.0 | Word error rate | MIT |
| `soundfile` / `pystoi` / `pyloudnorm` | 0.14.0 / 0.4.1 / 0.2.0 | Audio I/O and supplementary intelligibility metrics | MIT / BSD / MIT |
| `obs-websocket` | 5.7.x | Programmatic OBS recording for the injection matrix | GPL-2.0 (tool, not shipped) |

---

## 3. Open questions carried forward

These are **not** silently resolved. Each is an entry in the F0 open-questions register with an owner (FR-040).

| # | Question | Why it matters | Owner | Blocking? |
| --- | --- | --- | --- | --- |
| Q-1 | Does per-user (`HKCU`) COM registration of the MF virtual camera media source actually work, and is an MSI installer therefore required despite MSIX being the planned package format? | Decides the installer design and the no-admin promise for the camera | Technical lead | Yes, for the camera gate |
| Q-2 | Is the VB-CABLE driver signed in a way that Windows 11 with Secure Boot enabled accepts? | Determines whether the audio gate can pass at all | Technical lead | Yes, for the audio gate |
| Q-3 | What is the actual per-unit cost of a VB-CABLE OEM/redistribution agreement, and does the user population qualify as a "professional domain" under VB-Audio's terms? | Determines whether V1 ships link-only or bundled, and the unit economics | Product owner + legal | Yes, for the licensing gate |
| Q-4 | Will VAC's "companion application" / "auxiliary transport" characterisation cover RemoteCam, and at what price? | The cleaner long-term audio model | Product owner + legal | No — parallel track |
| Q-5 | Does the reference machine expose an H.264 decoder through the `SYNCMFT` discovery path, and is it actually hardware-accelerated? | FR-044 precondition — without it, no latency or resource result is admissible | Technical lead | Yes, for the pipeline gate |
| Q-6 | Can two consumer applications open the virtual camera simultaneously, and what does each observe? | Open question 6 in TDD §18; affects the shared-buffer design | Technical lead | No — recorded, not gating |
| Q-7 | Is 720p30 sufficient as the default quality, and is 1080p30 viable given that 1080p needs H.264 level ≥ 4.0 while many receivers accept only level 3.1? | Open question 3 in TDD §18; affects bandwidth, CPU and the negotiated level | Product owner | No — recorded |
| Q-8 | Can a 32-bit consumer application see the virtual camera on 64-bit Windows? | The DirectShow path needs separate per-bitness CLSIDs; the MF path behaviour is unknown | Technical lead | Yes, as a matrix failure criterion |
| Q-09 | Can Android host candidates be resolved without mDNS, or is a `MulticastLock` required? | If host candidates do not resolve, pairing fails on Android | Mobile engineer | Yes, for the pairing gate |
| Q-10 | Does the CameraX `YUV_420_888` chroma pixel stride equal 2 on any device in the reference set? | A stride-2 device with `JavaI420Buffer.wrap` produces corrupt colour | Mobile engineer | No — guarded at runtime, but must be reported |

---

## 4. Findings that change the source technical design

These are corrections or reductions of scope that the F0 plan must carry. Each needs a TDD amendment in the F0 report.

1. **The virtual camera primary backend changes** from a new DirectShow filter to `MFCreateVirtualCamera` (D-10). Microsoft classifies DirectShow as legacy and advises against new code on it.
2. **The audio half of V1 is not fully self-ownable** (D-12, D-13). Windows 11 provides no virtual microphone; V1 therefore depends on a user-installed third-party driver, with **video-only as the approved fallback**.
3. **The Windows 11 virtual camera requires an MSI, not MSIX** (D-10) — MSIX virtual-camera registration is not supported and the WinAppSDK request was closed in favour of Feedback Hub.
4. **`IMFHardwareBuffer` does not exist** (D-05). The correct interfaces are `IMF2DBuffer2` and `IMFGetService(MR_BUFFER_SERVICE)` → `IMFDXGIBackBuffer`.
5. **`MFT_ENUM_FLAG_HARDWARE` is unreliable** (D-05). Hardware capability must be verified empirically, and it is a gate precondition under FR-044.
6. **FR-014 is at risk from the Opus resampler alone** (D-24). The audio renderer must be paced from the RTP timestamps.
7. **The latency gate has an unresolved uncertainty of the same order as the budget** (D-20, D-29). The harness floor must be measured and reported, the **raw** capture-at-consumer p95 is the value the gate is judged on, and the floor-corrected value is published alongside it. A failure caused by the capture procedure is distinguished from a product regression.
8. **libwebrtc's Java API has changed materially since most tutorials were written** (D-17). `CapturerObserver` is now top-level with three methods; `OfferToReceive*` is gone.
9. **The `muteka.com` domain in the TDD's audio-device references is dead** (D-12).
10. **The TDD's ≤ 120 ms audio-latency target cannot be gated in F0** (D-31). End-to-end audio latency requires the real mobile capture path; it is carried forward to F2/F3 as a V1 product target. F0 gates video latency only.
11. **The TDD's "3 % packet loss with jitter" is not a reproducible condition until the profile is pinned** (D-30). One nominal profile exists — 3 % uniform loss, 30 ms added latency, ±10 ms jitter, media only — and any deviation from it cannot pass a gate.
12. **The TDD's pairing rate limit is a design assertion until the gate proves it** (D-32). The per-origin attempt limit, the progressive blocking and the listener shutdown are evidence, not configuration; a global limit would be a denial-of-service defect rather than a control.
13. **Proving no residue after a normal session end does not prove no residue after an application restart** (D-33). Both cases are inspected with the same method; the restart path is where a persistence bug hides.
