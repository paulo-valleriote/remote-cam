# D-audio-licensing — Virtual audio device sourcing and licensing

| Field | Value |
| --- | --- |
| `decision_id` | `D-audio-licensing` |
| `topic` | Which virtual audio device V1 depends on, whether RemoteCam may redistribute it, and what happens if the licensing decision is not closed by the gate date |
| `status` | `draft` |
| `chosen_option` | `vb-cable-link-only` (recommended; **not yet accepted**) |
| `decision_owner` | Product owner — **not yet signed** |
| `approver` | Product owner — **not yet signed** |
| `decided_at` | — |
| `gate_verdict` | `conditional` until a named owner and approver sign; `no_go` for the audio scope if the decision is still open on the gate date |
| `fallback` | V1 ships **video-only** (research D-13) |
| `evidence_refs` | `spikes/tools/analysis/licensing.py`, `spikes/tools/tests/test_licensing.py`, `spikes/f0-virtual-audio/src/lib.rs`, `spikes/f0-virtual-audio/signature_probe.ps1`, `spikes/evidence/audio/run_matrix.json` |
| `missing_evidence` | `spikes/evidence/licensing/licensing_bundle.json` (requires a named owner, approver and date); the live audio measurement (requires the Windows 11 reference runner) |

## Status note

This record is **`draft`**, deliberately. The contract makes an accepted
licensing decision conditional on named individuals, a date and a selected
option, and none of them can be produced on this host. Inventing a signatory
would turn a conditional gate into a fabricated pass.

What *is* settled and recorded here: every candidate considered, the official
terms text captured on 2026-10-23, the per-unit prices published by the vendors,
the recommendation (detect and verify a user-installed VB-CABLE, link to the
official download, redistribute nothing), the plan B, and the escalation path.

The machine-readable bundle is produced by:

```bash
cd spikes/tools
python -m analysis.licensing \
  --bundle-id lic-01 \
  --selected-option vb-cable-base \
  --decision-owner "<named individual>" \
  --approver "<named individual>" \
  --decided-at 2026-11-05 \
  --final \
  --out ../evidence/licensing/licensing_bundle.json
```

`--final` refuses to write without all four fields. Without `--final` the tool
writes a `draft` bundle whose gate verdict is `conditional`, never `go`.

## Options considered

| Option | Licence model | Redistribution | Unit cost | Support burden | Why it is / is not the V1 choice |
| --- | --- | --- | --- | --- | --- |
| **VB-CABLE, link-only** (`vb-cable-base`) | donationware | `yes_with_conditions` | €3.61 at 100 units (€2.92 at 1,000; €2.50 at 10,000) | low | **Recommended.** The user installs the official package; RemoteCam detects `CABLE Input`/`CABLE Output` and links to the vendor. No driver is redistributed, so the donationware redistribution condition is not engaged. |
| VB-CABLE, bundled/redistributed | donationware | `yes_with_conditions` | same, plus a significant distributor donation | low | Allowed only while the donationware model stays applicable: the user must see VB-CABLE as a VB-Audio application and be able to pay. It adds a legal review and an installer obligation for no technical gain. |
| Virtual Audio Cable, custom/OEM (`vac-oem`) | proprietary paid | `yes_with_conditions` | $50 per running instance (Business, 1); $15 at 500; custom build quoted separately | medium | Cleaner long-term shape, but the custom build must be tied to a "companion" application and used only as auxiliary transport. Whether RemoteCam qualifies is Q-4 and needs a quote plus legal opinion. |
| VoiceMeeter | donationware | `yes_with_conditions` | €10.0 at 100 units (Standard) | high | A mixer, not a cable: it takes over default devices, which conflicts with the rule that RemoteCam never changes the consumer's device selection. Potato cannot be bundled. |
| Scream | MS-PL | `yes_with_conditions` | 0 | high | Architecturally wrong: it publishes PCM to the LAN and exposes no capture endpoint. Windows 11 installation requires Secure Boot off and Test Mode. |
| SteelSeries Sonar | proprietary | `unverified` | 0 | high | No redistribution or OEM licence is published. It also grabs default playback and microphone devices. Recorded `unverified`, never guessed. |
| Own WDM/WaveRT driver | own driver | `yes_with_conditions` | 60–90 dev-days + EV certificate + attestation | high | **Rejected for V1.** A kernel audio driver has the same signing requirements as hardware and roughly a third of the V1 budget; deferred to V2 behind a new TDD. |
| Video-only V1 (`plan-b-video-only`) | plan B | `no` | 0 | low | **Approved fallback.** A coherent, sellable product; the audio half is deferred. |

The full quoted text for every row is in
`spikes/tools/analysis/licensing.py::CANDIDATES`, captured from the official
pages:

* <https://vb-audio.com/Services/licensing.htm> — VB-CABLE and VoiceMeeter
  distribution rules, professional-domain rule, volume prices.
* <https://vb-audio.com/Cable/index.htm> — the CABLE Input → CABLE Output
  topology and driver pack 45.
* <https://vac.muzychenko.net/en/customversion.htm> — VAC custom-version terms.
* <https://vac.muzychenko.net/en/purchase.htm> — VAC Business prices.
* <https://github.com/duncanthrax/scream> — MS-PL and the Windows 11 Test Mode
  requirement.
* <https://steelseries.com/gg/sonar> — Sonar download page; no redistribution
  terms published.
* <https://learn.microsoft.com/en-us/windows-hardware/drivers/install/kernel-mode-code-signing-policy--windows-vista-and-later->
  — the driver-signing policy that rejects an own driver for V1.

## Selection rule

1. If a named product owner and approver sign the link-only option by the gate
   date, V1 ships **VB-CABLE detection + official-download link**. RemoteCam
   redistributes nothing. The measurement device stays pinned to VB-CABLE driver
   pack 45, so the technical evidence remains valid.
2. If they select a different device, `remeasurement_required` MUST be `true`:
   the newly selected device is re-measured against the same recorded steps
   before it becomes the shipping option. The old evidence stays valid for the
   device actually measured, not for the new one.
3. If no decision is signed by the gate date, the gate is **`conditional`** with
   the condition `lic-decision-open` blocking the audio scope, and V1 ships
   **video-only**. It is never a silent pass.

## Escalation path

The product owner escalates to legal review. If the decision is still unresolved
on the gate date, the audio scope is removed from V1, the gate is recorded as
`conditional`/`no_go`, and the video-only product ships. A later decision lifts
the condition only with a written waiver that names the accountable role, a
date and a resolution deadline (data-model §1.4).

## Evidence that exists today

| Evidence | Where | What it proves |
| --- | --- | --- |
| 10 licensing tests | `spikes/tools/tests/test_licensing.py` | every candidate carries official terms text and the required fields; a draft never reports `go`; `--final` refuses an unsigned bundle; selecting a non-measured device forces remeasurement |
| Candidate catalogue | `spikes/tools/analysis/licensing.py` | the quoted terms and prices for all seven options, with `unverified` recorded explicitly for Sonar |
| Validator rules | `spikes/tools/validation/validate_evidence.py` | a final bundle needs named roles, a date and a selection; `remeasurement_required` is consistent with the selection; a draft cannot report `go` |
| Signature probe | `spikes/f0-virtual-audio/signature_probe.ps1` | `Get-AuthenticodeSignature` plus `signtool verify /pa /v` on every driver file, with Secure Boot state and SHA-256 hashes |
| Audio harness tests | `spikes/f0-virtual-audio/tests/run_plan.rs`, `tests/test_play_phrases.py` | the run index refuses a short, stereo, 44.1 kHz or unsigned run; the phrase playback plan and transcript template are reproducible |

None of that is gate evidence. It is the apparatus the gate will use.

## Evidence that is missing

| Missing | Why | Consequence |
| --- | --- | --- |
| Named decision owner and approver, with a date | cannot be invented by the engineer | the bundle stays `draft` and the gate `conditional` |
| `licensing_bundle.json` | the CLI refuses to write a final bundle without the signatures | the machine-readable decision is absent |
| Q-2 signature result | requires the Secure-Boot Windows 11 runner | `signature_verified` stays `false`; no audio run is admissible |
| Q-3 OEM/redistribution quote and professional-domain classification | requires the vendor and legal | the link-only recommendation stands; bundling stays unapproved |
| Q-4 VAC companion/auxiliary-transport opinion | requires the vendor and legal | the VAC track stays a parallel, non-gating investigation |
| Live audio measurement | requires Windows 11, VB-CABLE, FFmpeg and a consumer application | no `dropouts.json`, `intelligibility.json` or `audio_run_capture.json` |

## Q-2/Q-3/Q-4 findings

* **Q-2 — is VB-CABLE accepted by Windows 11 with Secure Boot enabled?**
  **Unresolved.** The official pages do not state the WHQL/attestation status.
  The gate must verify it empirically with `signature_probe.ps1`; a failed or
  unverified signature keeps the session video-only. This is a hard blocker for
  the audio gate.
* **Q-3 — what is the per-unit cost of an OEM/redistribution agreement, and do
  our users qualify as a "professional domain"?** **Partially answered.** The
  published volume prices are €3.61/unit at 100, €2.92 at 1,000 and €2.50 at
  10,000; professional-domain deployments must buy licenses. The link-only model
  avoids the redistribution condition and therefore most of the legal review.
  The classification of RemoteCam's user population and any bespoke OEM quote
  remain open with the product owner and legal.
* **Q-4 — will VAC's "companion application" / "auxiliary transport"
  characterisation cover RemoteCam?** **Open.** The official custom-version terms
  say the build is compiled with unique names and a unique key, works only with
  the companion application, and that the companion must provide specific
  functionality using VAC only as auxiliary transport. Whether RemoteCam meets
  that description and at what price needs a vendor quote and a legal opinion.
  This is a parallel track, not a gate blocker.
