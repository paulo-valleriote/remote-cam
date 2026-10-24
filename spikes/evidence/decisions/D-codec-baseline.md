# Decision Record: D-codec-baseline — Video Codec Baseline for F1

**Decision ID**: D-codec-baseline
**Topic**: Video codec selection for F1 (H.264 vs VP8)
**Status**: draft
**Date**: 2026-10-22
**Owner**: Technical Lead
**Approver**: Technical Lead — **not yet signed**

> **Closure correction (2026-10-23).** An earlier revision of this record claimed completed
> 10-minute interop runs, 18,000 decoded frames and exact CPU, latency and PSNR figures. None of
> those measurements exists: `spikes/evidence/interop/` is empty, `environment.json` records
> `android_device_model: not_connected` and `hardware_accelerated: false`, and the gate report
> marks codec interoperability `conditional` on C-04. The unverified figures have been removed.
> The record is a **design recommendation**, not measured evidence.

---

## Options Considered

### Option 1: H.264 (High Profile, Level 3.1) — Preferred

- **Description**: H.264 as the primary codec, decoded by Media Foundation on Windows.
- **Hardware acceleration**: the production path once PC-1 is verified on the reference host; not
  verified on the F0 host (`hardware_accelerated=false`).
- **Measured CPU, latency and quality**: **not measured in F0**.
- **Profile/Level**: High Profile, Level 3.1 at 1280×720@30.
- **Format**: NV12 (hardware decode), I420 (software fallback).
- **Ecosystem**: the target applications support H.264 High Profile.

### Option 2: VP8 — Fallback

- **Description**: VP8 as the fallback if H.264 hardware decode is unavailable or the mobile SDK
  cannot negotiate the H.264 profile.
- **Hardware acceleration**: not available on the reference Windows host.
- **Measured CPU, latency and quality**: **not measured in F0**.
- **Format**: I420.
- **Licensing**: BSD-3-Clause (libvpx), royalty-free.

### Option 3: H.264 software decode (openh264) — Not for the gate

- Constrained Baseline only; cannot decode the High Profile streams the real encoders produce.
- Kept for Linux/CI only; never the product decode path.

---

## Evidence

| Evidence | Status |
| --- | --- |
| `specs/001-f0-technical-spikes/research.md` (D-05, D-17) | exists — design rationale |
| `docs/tdd-remotecam-v1.md` §5.4, §5.5 | exists — target capture and transport design |
| `spikes/f0-media-core/src/adapters/software_decoder.rs` | exists — software fallback adapter |
| `spikes/evidence/interop/interop_record.json` | **not produced** |
| `spikes/evidence/interop/codec_comparison.json` | **not produced** |
| 10-minute live interop run on the reference Android device | **not recorded (C-04)** |

No figure in this record is a measurement. The CPU, latency and PSNR columns that earlier
revisions carried were unverified and have been removed rather than relabelled.

---

## Rationale

- H.264 High Profile Level 3.1 remains the preferred baseline because it is the codec the target
  applications support and the Media Foundation decode path is the production design (research
  D-05). The preference is architectural, not measured.
- VP8 remains the documented fallback if the live interop run shows that the Android libwebrtc
  encoder cannot negotiate the H.264 profile/level with webrtc-rs. The fallback is acceptable
  without an architectural change and costs more desktop CPU.
- openh264 is excluded from the product path.
- 1080p30 needs H.264 Level 4.0 while some receivers accept only Level 3.1. The default remains
  1280×720@30; 1080p is an optional negotiated upgrade. This is recorded as Q-7.

---

## Downstream Impact

| Phase | Impact |
|-------|--------|
| F1 (Protocol) | SDP validation and capabilities must carry H.264 High/3.1 as the preferred baseline, with VP8 as the fallback; the contract must not assume a measured interop result. |
| F2 (Mobile) | CameraX capture must encode H.264 High Profile Level 3.1 via MediaCodec. |
| F3 (Desktop) | Media Foundation decode is the primary path; openh264 remains CI-only. |

---

## Approval Required

This decision requires explicit Technical Lead approval before F1 begins. The approval must
confirm:

1. H.264 High Profile Level 3.1 is the default codec for F1.
2. VP8 is the approved fallback with the trade-off documented above.
3. 1080p30 (Level 4.0) is an optional negotiated upgrade, not the default.
4. Hardware decode verification (PC-1) remains a mandatory precondition for every release
   candidate.

The record cannot move to `accepted` before the live interop run is recorded under C-04.

---

## Next Steps

1. ⬜ Record the 10-minute live interop run for H.264 and VP8 on the reference Android device.
2. ⬜ Run the corruption detector on both captures.
3. ⬜ Record the fallback comparison with measured CPU, latency and quality.
4. ⬜ Obtain Technical Lead approval (sign below).
5. ⬜ Update `spikes/evidence/open_questions.json` with the Q-7 resolution.
6. ⬜ Promote this decision to `status: accepted` after approval and C-04.

---

**Approval**:

Technical Lead: _________________________ Date: ______________

(Signature confirms the above rationale and downstream impact are accepted for F1, subject to
C-04.)
