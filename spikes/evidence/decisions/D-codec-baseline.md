# Decision Record: D-codec-baseline — Video Codec Baseline for F1

**Decision ID**: D-codec-baseline
**Topic**: Video codec selection for F1 (H.264 vs VP8)
**Status**: draft
**Date**: 2026-10-22
**Owner**: Technical Lead
**Approver**: Technical Lead

---

## Options Considered

### Option 1: H.264 (High Profile, Level 3.1) — Preferred
- **Description**: Use H.264 as the primary codec with hardware decode on Windows via Media Foundation.
- **Hardware Acceleration**: ✅ Verified on reference machine (NV12 output via DXVA)
- **CPU (p95)**: 8.5% (with HW decode)
- **Latency (p95)**: 45 ms
- **Quality (PSNR)**: 42 dB
- **Profile/Level**: High Profile, Level 3.1 (1280×720@30)
- **Format**: NV12 (hardware decode), I420 (software fallback)
- **Licensing**: Cisco OpenH264 covers encoding; platform decoder for decode
- **Ecosystem**: Universal support across all target applications

### Option 2: VP8 — Fallback
- **Description**: Use VP8 as the primary codec if H.264 hardware decode is unavailable.
- **Hardware Acceleration**: ❌ Not available on reference Windows machine
- **CPU (p95)**: 22.0% (software decode via libvpx)
- **Latency (p95)**: 100 ms
- **Quality (PSNR)**: 38 dB
- **Profile/Level**: N/A (VP8 has no profile/level)
- **Format**: I420
- **Licensing**: BSD-3-Clause (libvpx), royalty-free
- **Ecosystem**: Good browser support; limited native app support

### Option 3: H.264 Software Decode (openh264) — Not for Gate
- **Description**: Use openh264 for software H.264 decode.
- **Hardware Acceleration**: ❌ Software only
- **CPU (p95)**: 25.0%
- **Latency (p95)**: 120 ms
- **Quality (PSNR)**: 30 dB (constrained baseline only)
- **Profile/Level**: Constrained Baseline only
- **Format**: I420
- **Note**: Cannot decode High Profile streams; excluded from gate evidence

---

## Evidence

| Metric | H.264 (HW) | VP8 (SW) | H.264 (SW) | Gate Threshold |
|--------|------------|----------|------------|----------------|
| CPU p95 | 8.5% | 22.0% | 25.0% | ≤ 15% |
| Latency p95 | 45 ms | 100 ms | 120 ms | ≤ 150 ms |
| Quality (PSNR) | 42 dB | 38 dB | 30 dB | — |
| HW Accelerated | ✅ | ❌ | ❌ | Required (PC-1) |
| Profile/Level | High/3.1 | N/A | Baseline | High/3.1 default |

**Interop Run Evidence**:
- H.264: 10 minutes, 18000 frames, 0 corruption detected, HW accelerated ✅
- VP8: 10 minutes, 18000 frames, 0 corruption detected, SW decode only

**Evidence References**:
- `spikes/evidence/interop/interop_record.json`
- `spikes/evidence/interop/codec_comparison.json`
- `spikes/evidence/environment.json` (hardware_decode_verification)

---

## Rationale

**H.264 with hardware acceleration is the clear choice for F1** because:

1. **Meets all gate criteria**: CPU 8.5% ≪ 15%, latency 45 ms ≪ 150 ms, hardware accelerated ✅
2. **Superior quality**: 42 dB PSNR vs 38 dB for VP8
3. **Universal compatibility**: Every target application (browser, conferencing, OBS, Windows Camera, screen recorder) supports H.264 High Profile Level 3.1
4. **Ecosystem standard**: H.264 is the de facto standard for video conferencing and streaming
5. **Windows integration**: Media Foundation provides robust, tested hardware decode path

**VP8 is retained as a documented fallback** for environments where H.264 hardware decode is genuinely unavailable (e.g., older GPUs, virtual machines). The fallback path is fully implemented and tested but requires explicit approval to become the primary codec.

**Key trade-off documented**: 1080p30 requires H.264 Level 4.0, but many receivers (especially older conference room systems) only accept Level 3.1. Therefore:
- **Default**: 1280×720@30 (Level 3.1) — guaranteed compatibility
- **Negotiated upgrade**: 1920×1080@30 (Level 4.0) — only when both ends confirm support

This is recorded as **open question Q-7** in `spikes/evidence/open_questions.json`.

---

## Downstream Impact

| Phase | Impact |
|-------|--------|
| F1 (Protocol) | H.264 profile/level negotiation logic must prefer High/3.1, offer 4.0 as upgrade |
| F2 (Mobile) | CameraX capture must encode H.264 High Profile Level 3.1 via MediaCodec |
| F3 (Desktop) | Media Foundation decode path is the primary; openh264 remains CI-only |
| F1-F3 | VP8 implementation maintained as fallback; not the default |

---

## Approval Required

**This decision requires explicit Technical Lead approval before F1 begins.**

The approval must confirm:
1. H.264 High Profile Level 3.1 is the default codec for F1
2. VP8 is the approved fallback with documented trade-offs
3. 1080p30 (Level 4.0) is an optional negotiated upgrade, not the default
4. Hardware decode verification (PC-1) remains a mandatory precondition for every release candidate

---

## Reusable Findings

- Hardware H.264 decode via Media Foundation (NV12, DXVA) is the production path
- openh264 is constrained-baseline only and **never** the product decode path
- VP8 software decode via libvpx is the fallback; VP8 hardware decode not available on reference Windows
- Corruption detection (black/green/magenta/frozen frames) is automated and part of the gate
- Pixel stride handling in CameraX (Q-10) is a runtime guard, not a codec issue

---

## Next Steps

1. ✅ Complete interop evidence collection (10-minute runs for both codecs)
2. ✅ Run corruption detection on captured video
3. ✅ Run fallback comparison (CPU, latency, quality, HW accel)
4. ⬜ Obtain Technical Lead approval (sign below)
5. ⬜ Update `spikes/evidence/open_questions.json` with Q-7 resolution
6. ⬜ Promote this decision to `status: accepted` upon approval

---

**Approval**:

Technical Lead: _________________________ Date: ______________

(Signature confirms the above rationale and downstream impact are accepted for F1)