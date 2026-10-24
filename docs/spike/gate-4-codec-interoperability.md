# Gate 4 Procedure: Codec Interoperability

**Gate ID**: `gate-4-codec-interoperability`
**Wave**: 3
**Depends on**: `gate-3-desktop-receive-pipeline`
**Accountable Role**: Technical Lead
**Review**: 2026-10-23 — F0 closed with exceptions (gate `conditional`, C-04 waived)
**Preconditions**: PC-1 (hardware decode verified for selected codec)

---

## Purpose

Confirm that live H.264 video from the Android reference device decodes on the desktop with agreed profile/level/format and no visible corruption for 10 continuous minutes, or that the VP8 fallback is approved with measured CPU, latency, quality trade-offs and hardware-decode status recorded.

---

## Procedure Overview

| Step | Action | Duration | Output |
|------|--------|----------|--------|
| 1 | Start desktop interop receiver | 1 min | `spikes/f0-codec-interop` running on Windows 11 |
| 2 | Start mobile capture/transmit app | 1 min | Android app streaming to desktop |
| 3 | Verify negotiated codec parameters | 1 min | Profile/level/format logged |
| 4 | Run 10-minute interop measurement | 10 min | Video capture + metrics |
| 5 | Run corruption detection | 2 min | `interop_corruption.py` results |
| 6 | If H.264 fails, run VP8 fallback comparison | 10 min | `codec_comparison.json` |
| 7 | Record codec decision | 5 min | `D-codec-baseline.md` |

**Total**: ~20 minutes (plus 10 min fallback if needed)

---

## Prerequisites

- [ ] Gate 3 (Desktop Receive Pipeline) has a verdict (even `no_go` with recorded blocking dependency)
- [ ] `make preflight` passed — `environment.json` shows `hardware_accelerated: true` for H.264
- [ ] `make selftest` passed — toolchain verified
- [ ] Windows 11 reference machine (build 22000+) with hardware H.264 decode
- [ ] Android reference device (API 29+, CameraX 1.6.2, libwebrtc 150.7871.01)
- [ ] Local 5 GHz Wi-Fi, internet disabled, Wi-Fi power save disabled
- [ ] Both devices on same network, distance ≤ 5 m from AP

---

## Step-by-Step Procedure

### Step 1: Start Desktop Interop Receiver

On the Windows 11 reference machine:

```powershell
cd spikes\f0-codec-interop
cargo run --release
```

Expected output:
```
=== F0 Codec Interoperability Receiver ===
Starting receive-only WebRTC peer connection...
Starting signaling server on 0.0.0.0:8443
Waiting for mobile connection...
```

The receiver listens for WebSocket signaling connections on port 8443 (TLS 1.3, fingerprint-pinned).

### Step 2: Start Mobile App

On the Android reference device:

1. Install the F0 spike APK. The F0 `:app` module is now quarantined at
   `spikes/f0-mobile-app` and is no longer part of the `mobile/` Gradle project;
   reproduce this step from the F0 commit with `./gradlew :app:installDebug`.
2. Launch "RemoteCam" app
3. Grant Camera and Microphone permissions
4. Tap "Start Session"

The app will:
- Initialize CameraX ImageAnalysis (no Preview surface)
- Create PeerConnection with SEND_ONLY transceivers
- Display QR code payload (session ID, endpoints, TLS fingerprint, token)
- Wait for desktop to scan QR and complete handshake

### Step 3: Complete Pairing Handshake

On the desktop, use the pairing handshake harness to scan the mobile's QR code and establish the authenticated channel:

```powershell
cd spikes\f0-handshake
cargo run --release --bin pairing_client -- --qr <mobile_qr_payload>
```

Or scan manually using the desktop app's QR scanner.

**Verification**: The desktop receiver logs should show:
```
Negotiated codec: video/H264
  Clock rate: 90000
  fmtp: profile-level-id=42e01f;packetization-mode=1
```

### Step 4: Verify Negotiated Parameters

Confirm the following are logged in the desktop receiver output and recorded in `spikes/evidence/interop/interop_record.json`:

| Parameter | Expected Value | Validation |
|-----------|----------------|------------|
| Codec | `H264` (preferred) or `VP8` (fallback) | Must match SDP |
| Profile | `High` (for H.264) | `profile-level-id=64xxxx` |
| Level | `3.1` (for 720p30) | `profile-level-id=xx001f` |
| Format | `NV12` (HW) or `I420` (SW) | From decoder output |
| Hardware Accelerated | `true` for H.264 | Must match `environment.json` |

**Q-7 Level Finding**: 1080p30 requires H.264 Level 4.0 (`profile-level-id=xx0028`), but many receivers (especially conference room systems) only accept Level 3.1. **Ship 720p30 as default; treat 1080p30 as negotiated upgrade.**

### Step 5: Run 10-Minute Interop Measurement

Let the stream run for **10 continuous minutes (600 seconds)**.

The desktop receiver automatically:
- Counts decoded frames
- Logs frame rate
- Records hardware acceleration status
- Writes `interop_record.json` on completion

**Minimum requirements**:
- `duration_s ≥ 600`
- `frame_count ≥ 17000` (allowing for some loss)
- `corruption_detected = false`
- `hardware_accelerated = true` (for H.264)

### Step 6: Capture Video for Corruption Analysis

While the interop run is active, capture the desktop's virtual camera output using one of the target applications (e.g., OBS via `obs-websocket`, or browser via `getUserMedia`):

```bash
# Example: OBS capture
cd spikes/f0-camera-injection/apps/obs
python capture.py --duration 600 --output interop_capture.mkv
```

Save the capture as `spikes/evidence/interop/interop_capture.mkv` (not committed, referenced by path).

### Step 7: Run Corruption Detection

```bash
cd spikes/tools/analysis
python interop_corruption.py spikes/evidence/interop/interop_capture.mkv \
  --output spikes/evidence/interop/corruption_analysis.json \
  --observer "Technical Lead Name"
```

**Pass criteria**:
- `black_frames = 0`
- `green_frames = 0` (chroma swap)
- `magenta_frames = 0` (chroma swap)
- `frozen_frames = 0`
- `longest_freeze_ms < 2000`
- `corruption_detected = false`

**Q-10 Pixel-Stride Finding**: If green/magenta frames are detected, it indicates the CameraX `YUV_420_888` pixel stride was 2 on the reference device and de-interleaving failed. This is guarded at runtime in `CameraXCaptureSource.kt` but must be reported.

### Step 8: Fallback Comparison (If H.264 Fails)

If H.264 interop fails (corruption detected OR hardware acceleration not available):

1. Force VP8 on the mobile side (modify `PeerConnectionFactoryProvider` to prefer VP8)
2. Re-run Steps 1-5 with VP8
3. Run the fallback comparison:

```powershell
cd spikes\f0-codec-interop
cargo run --release --bin compare
```

This produces `spikes/evidence/interop/codec_comparison.json` with:
- CPU p95 for both codecs
- Latency p95 for both codecs
- Quality metric (PSNR/VMAF) for both
- Hardware acceleration status for VP8

### Step 9: Record Codec Decision

Update `spikes/evidence/decisions/D-codec-baseline.md` with:
- Selected codec (H.264 or VP8)
- Measured trade-offs (CPU, latency, quality, HW accel)
- Profile/level used
- Explicit Technical Lead approval (required before F1)

---

## Evidence Checklist

All files must exist and validate against their schemas:

- [ ] `spikes/evidence/interop/interop_record.json` — main interop evidence
- [ ] `spikes/evidence/interop/corruption_analysis.json` — corruption detection results
- [ ] `spikes/evidence/interop/codec_comparison.json` — fallback comparison (if VP8 tested)
- [ ] `spikes/evidence/interop/interop_capture.mkv` — raw capture (local only, not committed)
- [ ] `spikes/evidence/decisions/D-codec-baseline.md` — decision record
- [ ] `spikes/evidence/environment.json` — with `hardware_decode_verification.hardware_accelerated: true`

---

## Gate Verdict Criteria

| Criterion | Pass Condition |
|-----------|----------------|
| Negotiated codec | H.264 or VP8 with profile/level/format logged |
| Duration | `duration_s ≥ 600` |
| Corruption | `corruption_detected = false` |
| Hardware acceleration | `true` for selected codec (PC-1) |
| Fallback (if used) | CPU, latency, quality, HW accel recorded in `codec_comparison.json` |
| Decision record | `D-codec-baseline.md` complete with approval |

---

## Failure Modes

| Failure | Verdict | Action |
|---------|---------|--------|
| H.264 corruption detected | `no_go` | Run VP8 fallback comparison |
| H.264 no HW acceleration | `no_go` (PC-1 failed) | Replace reference machine, re-run |
| VP8 corruption detected | `no_go` | Conditional verdict, escalate |
| VP8 no HW acceleration | `conditional` | Record trade-offs, require approval for SW decode |
| Pairing handshake fails | `blocked_by_dependency` | Record blocking dependency (Gate 2) |

---

## Open Questions Recorded

| Question | Status | Reference |
|----------|--------|-----------|
| **Q-7**: Is 720p30 sufficient as default? Is 1080p30 viable given Level 4.0 vs 3.1 receiver limits? | **Answered in this procedure** — 720p30 default, 1080p30 negotiated upgrade | `D-codec-baseline.md`, `open_questions.json` |
| **Q-10**: Does CameraX `YUV_420_888` pixel stride = 2 on reference device? | **Guarded at runtime** — de-interleaving implemented; report finding | `corruption_analysis.json`, `CameraXCaptureSource.kt` |

---

## Reproducibility

A second engineer can reproduce this gate by:

1. Running `make preflight` and `make selftest`
2. Starting desktop receiver: `cargo run --release` in `spikes/f0-codec-interop`
3. Installing and starting mobile app on Android device
4. Completing pairing handshake via QR scan
5. Running 10-minute capture
6. Running `python interop_corruption.py` on the capture
7. Verifying `interop_record.json` meets all pass criteria

---

## Non-Production Statement

**All code in `spikes/f0-codec-interop/`, `spikes/tools/analysis/interop_corruption.py`, and the mobile app modifications for this gate are NON-PRODUCTION spike harnesses. They MUST NOT be merged into `desktop/` or `mobile/` product modules (FR-039).**