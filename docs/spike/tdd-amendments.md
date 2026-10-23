# TDD Amendments — F0 Phase 8 Polish

**Source:** `specs/001-f0-technical-spikes/research.md` §4 — "Findings that change the source technical design"
**Gate:** Phase review on or before 2026-11-06
**Status:** All 13 findings require a TDD amendment before F1 can be declared complete.

The table below lists each finding, the decision record that carries it, and the
TDD section it amends. Each finding is a correction or reduction of scope that the
F0 plan must carry.

| # | Finding (research.md §4) | Decision record | TDD section amended | Status |
|---|--------------------------|-----------------|---------------------|--------|
| 1 | The virtual camera primary backend changes from a new DirectShow filter to `MFCreateVirtualCamera` (D-10). Microsoft classifies DirectShow as legacy and advises against new code on it. | D-10-injection-backend | TDD camera-injection backend selection | Open — D-10 is `draft` until the injection matrix exists |
| 2 | The audio half of V1 is not fully self-ownable (D-12, D-13). Windows 11 provides no virtual microphone; V1 therefore depends on a user-installed third-party driver, with **video-only as the approved fallback**. | D-audio-licensing | TDD audio device sourcing | Open — D-audio-licensing is `draft` |
| 3 | The Windows 11 virtual camera requires an MSI, not MSIX (D-10) — MSIX virtual-camera registration is not supported and the WinAppSDK request was closed in favour of Feedback Hub. | D-10-injection-backend | TDD installer/package format | Open — D-10 is `draft` |
| 4 | `IMFHardwareBuffer` does not exist (D-05). The correct interfaces are `IMF2DBuffer2` and `IMFGetService(MR_BUFFER_SERVICE)` → `IMFDXGIBackBuffer`. | D-05 (media-core adapter) | TDD media decoder buffer handling | Open |
| 5 | `MFT_ENUM_FLAG_HARDWARE` is unreliable (D-05). Hardware capability must be verified empirically, and it is a gate precondition under FR-044. | D-quality-default / environment.json | TDD hardware-decode precondition PC-1 | Open — PC-1 not satisfied on this host |
| 6 | FR-014 is at risk from the Opus resampler alone (D-24). The audio renderer must be paced from the RTP timestamps. | D-24 (audio-bridge readiness) | TDD audio renderer pacing | Open |
| 7 | The latency gate has an unresolved uncertainty of the same order as the budget (D-20, D-29). The harness floor must be measured and reported, the **raw** capture-at-consumer p95 is the value the gate is judged on, and the floor-corrected value is published alongside it. A failure caused by the capture procedure is distinguished from a product regression. | D-quality-default / gate-report | TDD latency gate criterion | Open |
| 8 | libwebrtc's Java API has changed materially since most tutorials were written (D-17). `CapturerObserver` is now top-level with three methods; `OfferToReceive*` is gone. | D-17 (mobile transport) | TDD mobile transport API | Open |
| 9 | The `muteka.com` domain in the TDD's audio-device references is dead (D-12). | D-audio-licensing | TDD audio device references | Open |
| 10 | The TDD's ≤ 120 ms audio-latency target cannot be gated in F0 (D-31). End-to-end audio latency requires the real mobile capture path; it is carried forward to F2/F3 as a V1 product target. F0 gates video latency only. | D-31 (audio scope) | TDD audio latency target | Open — carried forward to F2/F3 |
| 11 | The TDD's "3 % packet loss with jitter" is not a reproducible condition until the profile is pinned (D-30). One nominal profile exists — 3 % uniform loss, 30 ms added latency, ±10 ms jitter, media only — and any deviation from it cannot pass a gate. | D-30 (impairment profile) | TDD impairment profile | Open |
| 12 | The TDD's pairing rate limit is a design assertion until the gate proves it (D-32). The per-origin attempt limit, the progressive blocking and the listener shutdown are evidence, not configuration; a global limit would be a denial-of-service defect rather than a control. | D-32 (pairing throttling) | TDD pairing rate limit | Open |
| 13 | Proving no residue after a normal session end does not prove no residue after an application restart (D-33). Both cases are inspected with the same method; the restart path is where a persistence bug hides. | gate-2 residue condition C-02-residue-restart | TDD residue inspection method | Open — requires two reference devices |

## Amendment procedure

Each finding above must be incorporated into the source technical design document (TDD) before F1 begins. The TDD amendment must:

1. Record the finding verbatim from research.md §4.
2. Reference the decision record that carries it.
3. State the revised design behaviour.
4. Be signed by the accountable role named in the gate report.

Until all 13 amendments are recorded in `docs/spike/tdd-amendments.md` and the
corresponding TDD sections are updated, F0 cannot be declared complete (SC-001…SC-014).
