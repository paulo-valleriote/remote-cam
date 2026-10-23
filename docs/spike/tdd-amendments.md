# TDD Amendments — F0 Closure

**Source:** `specs/001-f0-technical-spikes/research.md` §4 — "Findings that change the source technical design"
**Gate:** F0 closed with exceptions on 2026-10-23
**Status:** All 13 findings are disposed: **12 applied** to the TDD and **1 no-op**
(finding 9 — the TDD contains no `muteka.com` reference).

The table below lists each finding, the decision record that carries it, and the
TDD section it amends. Each finding is a correction or reduction of scope that the
F0 plan must carry.

| # | Finding (research.md §4) | Decision record | TDD section amended | Status |
|---|--------------------------|-----------------|---------------------|--------|
| 1 | The virtual camera primary backend changes from a new DirectShow filter to `MFCreateVirtualCamera` (D-10). Microsoft classifies DirectShow as legacy and advises against new code on it. | D-10-injection-backend | §5.6 camera-injection backend selection | **Applied** — `MFCreateVirtualCamera` is primary; DirectShow is the fallback |
| 2 | The audio half of V1 is not fully self-ownable (D-12, D-13). Windows 11 provides no virtual microphone; V1 therefore depends on a user-installed third-party driver, with **video-only as the approved fallback**. | D-audio-licensing | §5.6 audio device sourcing | **Applied** — VB-CABLE link-only; video-only is the approved plan B |
| 3 | The Windows 11 virtual camera requires an MSI, not MSIX (D-10) — MSIX virtual-camera registration is not supported and the WinAppSDK request was closed in favour of Feedback Hub. | D-10-injection-backend | §5.10 installer/package format, §7 F4 | **Applied** — MSI, per-user |
| 4 | `IMFHardwareBuffer` does not exist (D-05). The correct interfaces are `IMF2DBuffer2` and `IMFGetService(MR_BUFFER_SERVICE)` → `IMFDXGIBackBuffer`. | D-05 (media-core adapter) | §5.6 media decoder buffer handling | **Applied** — the correction is stated in the decode bullet |
| 5 | `MFT_ENUM_FLAG_HARDWARE` is unreliable (D-05). Hardware capability must be verified empirically, and it is a gate precondition under FR-044. | D-quality-default / environment.json | §5.6 hardware-decode precondition PC-1 | **Applied** — SYNCMFT discovery plus empirical NV12 verification |
| 6 | FR-014 is at risk from the Opus resampler alone (D-24). The audio renderer must be paced from the RTP timestamps. | D-24 (audio-bridge readiness) | §5.6 audio renderer pacing | **Applied** — pacing from RTP timestamps |
| 7 | The latency gate has an unresolved uncertainty of the same order as the budget (D-20, D-29). The harness floor must be measured and reported, the **raw** capture-at-consumer p95 is the value the gate is judged on, and the floor-corrected value is published alongside it. A failure caused by the capture procedure is distinguished from a product regression. | D-quality-default / gate-report | §5.7 latency gate criterion | **Applied** — raw p95 is judged; floor and corrected value published |
| 8 | libwebrtc's Java API has changed materially since most tutorials were written (D-17). `CapturerObserver` is now top-level with three methods; `OfferToReceive*` is gone. | D-17 (mobile transport) | §5.4 mobile transport API | **Applied** — implementation note for F2 |
| 9 | The `muteka.com` domain in the TDD's audio-device references is dead (D-12). | D-audio-licensing | TDD audio device references | **No-op** — the TDD has no `muteka.com` occurrence; D-12 already points at `vac.muzychenko.net` |
| 10 | The TDD's ≤ 120 ms audio-latency target cannot be gated in F0 (D-31). End-to-end audio latency requires the real mobile capture path; it is carried forward to F2/F3 as a V1 product target. F0 gates video latency only. | D-31 (audio scope) | §5.7 audio latency target, §12 | **Applied** — marked as an F2/F3 target, not an F0 gate |
| 11 | The TDD's "3 % packet loss with jitter" is not a reproducible condition until the profile is pinned (D-30). One nominal profile exists — 3 % uniform loss, 30 ms added latency, ±10 ms jitter, media only — and any deviation from it cannot pass a gate. | D-30 (impairment profile) | §5.5 impairment profile, §9 | **Applied** — one pinned profile |
| 12 | The TDD's pairing rate limit is a design assertion until the gate proves it (D-32). The per-origin attempt limit, the progressive blocking and the listener shutdown are evidence, not configuration; a global limit would be a denial-of-service defect rather than a control. | D-32 (pairing throttling) | §5.3 pairing rate limit, §9 | **Applied** — evidence, not configuration |
| 13 | Proving no residue after a normal session end does not prove no residue after an application restart (D-33). Both cases are inspected with the same method; the restart path is where a persistence bug hides. | gate-2 residue condition C-02-residue-restart | §5.8 residue inspection method, §9 | **Applied** — both cases, same method; restart inspection carried to F1 under C-02 |

## Amendment procedure

Each finding above was incorporated into the source technical design document
(TDD) at F0 closure. Each amendment:

1. Records the finding from research.md §4.
2. References the decision record that carries it.
3. States the revised design behaviour.
4. Is covered by the F0 exit record (`spikes/evidence/f0_exit.json`), signed by
   the technical lead.

Finding 9 needed no textual change because the TDD has no `muteka.com`
occurrence. Finding 4 was already carried by D-05 and is now also stated in §5.6.

The three conditional gates (pairing, codec, audio) remain conditional. Their
carried obligations are recorded in the gate report and in `D-EXIT-f0`; applying
the amendments does not turn any verdict into `go`.
