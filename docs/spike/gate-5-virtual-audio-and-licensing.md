# Gate 5 — Virtual audio and licensing

**Gate**: virtual audio and licensing · **Wave**: 3 · **Depends on**: desktop receive pipeline (gate 3) ·
**Accountable role**: product owner · **Target review**: 2026-11-06

**Preconditions**: PC-3 (audio continuity is measured by dropout analysis **and** a scripted human check),
PC-4 (one pinned impairment profile, media only).

**Procedure owner**: the engineer running the gate. **Sign-off**: the product owner named in
`spikes/evidence/gate_report.json` and in `spikes/evidence/decisions/D-audio-licensing.md`.

---

## 1. What this gate proves

1. Decoded audio reaches a **consumer application** through a pinned third-party virtual audio device
   (VB-CABLE, driver pack 45) and is **intelligible and continuous for at least ten minutes** under the
   pinned impairment profile (3 % uniform loss, 30 ms added latency, ±10 ms jitter) (FR-028, FR-029).
2. Continuity is established by **two independent detectors** that agree: FFmpeg `silencedetect` at a
   calibrated threshold, cross-checked against an RMS-window scan, with zero intervals ≥ 50 ms (FR-030).
3. Intelligibility is established by a **scripted human check**: at least 18 of the 18 non-anchor
   phrases are transcribed exactly, corpus WER ≤ 0.05, all 10 nonces and both anchors match, with a
   **named observer** and the **phrase script reference** recorded (FR-029).
4. The device is detected by exact product identity, its driver signature is verified on a
   Secure-Boot-enabled machine, and an audible 1 kHz test tone is played before the session is
   audio-ready (FR-031).
5. When the device is missing, unsigned or unexpected, the application reports an **actionable** state
   and **video-only operation remains fully usable** (FR-032).
6. Every candidate virtual audio device has its **licence terms captured from the official source**, and
   the gate closes with a **decision** or an explicit `conditional`/`no_go` with a plan B (FR-033, FR-034).

**Out of scope**: end-to-end audio latency (Q8 — deferred to F2/F3; no latency figure is produced or
gated here), audio quality beyond intelligibility, and any redistribution of a third-party driver.

---

## 2. Preconditions

| # | Check | If it fails |
| --- | --- | --- |
| PC-3 | The phrase manifest exists (`spikes/fixtures/phrases-v1.csv`, 20 phrases: 2 anchors + 8 static + 10 nonce) and the observer is available for the run. | The human half cannot be produced. The gate is `blocked_by_environment`; do not substitute an ASR result for the named observer. |
| PC-4 | The relay applies exactly one profile: 3 % uniform loss, 30 ms added latency, ±10 ms jitter. | A deviating profile is labelled and **cannot pass**. Re-run with the pinned profile. |
| PC-1 (inherited) | Gate 3 has a verdict. The audio run consumes the gate-3 receive pipeline. | Record `blocked_by_dependency` against gate 3. |
| Host | Windows 11 x64 build 22000+ with Secure Boot enabled, VB-CABLE driver pack 45 installed, FFmpeg ≥ 8.1, a consumer application (OBS or equivalent) and `signtool.exe`. | The live run is impossible. Record `blocked_by_environment`; the apparatus still validates on Linux CI. |

`make preflight` and `make selftest` must have been run on the reference host before any evidence is
written (`spikes/evidence/environment.json`, `spikes/evidence/toolchain_selftest.json`).

---

## 3. Procedure overview

| Step | Action | Duration | Output |
| --- | --- | --- | --- |
| 1 | Confirm the reference environment | 2 min | `environment.json`, `toolchain_selftest.json` |
| 2 | Verify the VB-CABLE driver signature (Q-2) | 2 min | `audio/aud-01/signature_probe.json` |
| 3 | Render the 20-phrase script and open the transcript template | 2 min | `phrase_playback_manifest.json`, `transcriptions.json`, `phrase_audio.wav` |
| 4 | Prove readiness and degraded operation | 5 min | readiness states + video-only check |
| 5 | Record `CABLE Output` in the consumer application under the pinned profile | ≥ 10 min | `consumer_capture.wav` (local only) |
| 6 | Emit the audio run index | 1 min | `audio/aud-01/audio_run_capture.json` |
| 7 | Run the two-detector dropout analysis | 2 min | `audio/aud-01/dropouts.json` |
| 8 | Score the observer transcript | 5 min | `audio/aud-01/intelligibility.json` |
| 9 | Build the licensing bundle (in parallel, same steps) | 30 min | `licensing/licensing_bundle.json` |
| 10 | Validate the evidence and record the decision | 5 min | `make validate-evidence`, `D-audio-licensing.md` |

**Total**: ~55 minutes, of which 10 are the measured run. Steps 9 runs in parallel with steps 1–8.

---

## 4. Step-by-step procedure

### Step 1 — Confirm the reference environment

```powershell
make preflight
make selftest
```

Stop if `environment.json` reports `within_reference_conditions: false` or if the self-test fails: no
measurement taken on that host is admissible.

### Step 2 — Verify the driver signature (Q-2)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File spikes\f0-virtual-audio\signature_probe.ps1 `
  -DriverPath "C:\Program Files\VB\CABLE" `
  -OutFile "spikes\evidence\audio\aud-01\signature_probe.json"
```

The probe runs **both** `Get-AuthenticodeSignature` and `signtool verify /pa /v` over every `.sys`,
`.cat` and `.inf` file, records the Secure Boot state and the SHA-256 of each file, and exits `0` only
when every file passes both checks. A `Valid` Authenticode status alone is **not** sufficient: the
kernel-mode policy check is what the gate needs.

If `all_valid` is `false` or `secure_boot_enabled` is not `true`, the session stays **video-only** and
the audio gate cannot proceed. This is the Q-2 blocker.

### Step 3 — Render the scripted phrase manifest

```powershell
python spikes\f0-virtual-audio\play_phrases.py `
  --run-id aud-01 --out-dir spikes\evidence\audio\aud-01
```

This writes:

* `phrase_playback_manifest.json` — the plan (20 phrases, 48 kHz mono 16-bit, 1.5 s pause);
* `transcriptions.json` — a blank transcript template the observer fills in **without** seeing the
  reference column;
* `phrase_audio.wav` — the SAPI synthesis of every phrase, concatenated with digital silence.

On a non-Windows host the helper writes the plan and template and refuses to claim audio was produced.
That refusal is the correct outcome on Linux CI.

### Step 4 — Readiness and degraded operation

On the reference host, with the harness built:

```powershell
cargo run --manifest-path spikes\f0-virtual-audio\Cargo.toml --release -- `
  --render spikes\evidence\audio\aud-01\phrase_audio.wav --signature-verified
```

The render refuses to start without `--signature-verified`; that flag may be passed only after step 2
reports `all_valid: true`.

Verify, and record in the run notes:

| Case | Expected state | Expected user-facing next step |
| --- | --- | --- |
| Device present, signed, expected identity | `Ready` after the 1 kHz tone is audible | none |
| Device missing | `DeviceMissing` | install VB-CABLE driver pack 45 from the official page, then retry |
| Device unsigned or signature invalid | `SignatureInvalid` | reinstall from the official signed package; do not bypass |
| Device present but unexpected identity (different vendor/product) | `UnexpectedDevice` | install the pinned VB-CABLE package; the detected endpoint is not used |
| Any of the above | Video-only session **starts and delivers video** | audio is disabled, video is unaffected |

A readiness state that does not name the next step is a failure of FR-032. The unit tests in
`desktop/crates/audio-bridge/tests/readiness.rs` pin these states on Linux CI; the live check is what
proves them on the reference host.

### Step 5 — Record the consumer application for ≥ 10 minutes

1. Start the relay with the pinned profile (gate-3 relay): 3 % uniform loss, 30 ms added latency,
   ±10 ms jitter.
2. In the consumer application (OBS or equivalent), select **CABLE Output (VB-Audio Virtual Cable)** as
   the audio capture source and start recording **before** the harness renders.
3. Render the phrase audio through the harness (step 4) and let the session run for **at least 600
   seconds**. Do not stop, pause or reconfigure the consumer application; a renegotiation is recorded
   and must be zero.
4. Stop the recording and save it as `spikes/evidence/audio/aud-01/consumer_capture.wav` (48 kHz mono;
   local only — raw captures are not committed).

The capture must be the closed recording of the **whole** run: a partial capture cannot prove
continuity, and a re-encoded or resampled capture invalidates the dropout threshold calibration.

### Step 6 — Emit the audio run index

```powershell
cargo run --manifest-path spikes\f0-virtual-audio\Cargo.toml --release -- `
  --capture spikes\evidence\audio\aud-01\consumer_capture.wav `
  --run-id aud-01 --consumer-app obs --observer "<named observer>" `
  --signature-verified --out spikes\evidence\audio\aud-01\audio_run_capture.json
```

The harness validates the capture (WAV, 48 kHz, mono, ≥ 600 s) and writes the index with
`device_pin: "VB-CABLE"`, `device_version: "driver pack 45"`, `signature_verified`,
`consumer_renegotiations`, the observer and `phrase_script_ref`. A capture that is short, stereo,
44.1 kHz or unsigned is marked **not admissible** and the command exits non-zero.

`audio_run_capture.json` is only written from a real capture. It is not hand-edited.

### Step 7 — Two-detector dropout analysis

```powershell
cd spikes\tools
python -m analysis.dropouts `
  --capture ..\evidence\audio\aud-01\consumer_capture.wav `
  --run-id aud-01 --duration-s 610 `
  --threshold-dbfs -42 --threshold-source "noise floor -54 dBFS + 6 dB, calibration-40-60-120.wav" `
  --rtp-gaps ..\evidence\audio\aud-01\rtp_gaps.csv `
  --out ..\evidence\audio\aud-01\dropouts.json
```

The analyser:

* runs FFmpeg ≥ 8.1 `silencedetect` at the calibrated threshold and records `threshold_source`;
* runs an independent RMS-window scan and compares interval counts and boundaries;
* correlates every interval with the RTP sequence gaps;
* sets `detectors_agree` and records a **finding** on any disagreement — a disagreement is not a pass.

**Pass**: `dropout_count = 0` for intervals ≥ 50 ms, `detectors_agree = true`, and every RTP gap
explained by an analysed interval or by a retransmission.

**A missing calibration is a hard error**, not a default threshold. A 49 ms interval is recorded as
`below_threshold` and does not fail the gate; the 50 ms boundary is fixed by the contract.

### Step 8 — Score the observer transcript

1. Give the observer `transcriptions.json` and play the capture (or let them listen live in step 5).
2. The observer writes exactly what they heard for every phrase. A blank row means "heard nothing" and
   is scored as a failure — never as missing data.
3. Score it:

```powershell
cd spikes\tools
python -m analysis.intelligibility `
  --run-id aud-01 `
  --transcript ..\evidence\audio\aud-01\transcriptions.json `
  --observer "<named observer>" `
  --script-ref spikes/fixtures/phrases-v1.csv `
  --out ..\evidence\audio\aud-01\intelligibility.json
```

**Pass**: `phrases_exact ≥ 18` over the 18 non-anchor phrases, `corpus_wer ≤ 0.05`, all 10 nonces
matched, both anchors matched, a non-empty observer and a recorded `phrase_script_ref`.

Any nonce mismatch invalidates the capture as **stale or spliced**: re-record the run. An unnamed
observer or a missing script makes the result `unverifiable`, which cannot gate.

### Step 9 — Licensing bundle (parallel with steps 1–8)

The licence assessment is a decision, not a measurement. It runs on the same test steps so that a
change of device does not invalidate the technical evidence.

```powershell
cd spikes\tools
python -m analysis.licensing `
  --bundle-id lic-01 `
  --selected-option vb-cable-base `
  --decision-owner "<named individual>" `
  --approver "<named individual>" `
  --decided-at 2026-11-05 `
  --final `
  --out ..\evidence\licensing\licensing_bundle.json
```

* `candidate_options` covers **every** device considered — VB-CABLE (measured), Virtual Audio Cable
  (OEM/custom), VoiceMeeter, Scream, SteelSeries Sonar, an own WDM driver (rejected for V1) and
  plan B video-only — each with official `terms_url`, quoted `terms_captured_text`,
  `redistribution_allowed`, `commercial_use_allowed`, `unit_cost`, `cost_model`, `support_burden`,
  `user_experience`, `signature_verified` and `fallback`.
* `options_compared` covers redistribution licensing, a user-installed official driver and plan B.
* `--final` refuses to write unless the owner, approver and date are present and a selection is made.
  Without `--final` the bundle is `draft` and its verdict is `conditional` — never `go`.
* If the selected device is **not** the measured device, `remeasurement_required` is `true`: the newly
  selected device must be re-measured against these same steps before it becomes the shipping option.

**Legal note**: this is engineering research, not legal advice. Sign-off by qualified counsel is
required before any distribution decision.

### Step 10 — Validate and record the decision

```bash
make validate-evidence
```

Then complete `spikes/evidence/decisions/D-audio-licensing.md` with the decision owner, approver, date,
chosen option, unit cost and fallback. An unresolved decision on the gate date is recorded as
`conditional`/`no_go` with the blocker, the escalation owner and the interim audio scope — never a
silent pass.

---

## 5. Evidence checklist

| Artefact | Contract | Required |
| --- | --- | --- |
| `audio/run_matrix.json` | planned scenarios + environment verdict | yes |
| `audio/aud-01/signature_probe.json` | `signature_probe.ps1` output, Secure Boot state | yes |
| `audio/aud-01/phrase_playback_manifest.json` + `transcriptions.json` | 20 phrases, blank template | yes |
| `audio/aud-01/consumer_capture.wav` | 48 kHz mono, ≥ 600 s (local only) | yes |
| `audio/aud-01/audio_run_capture.json` | `contracts/measurement-formats.md` §9 | yes |
| `audio/aud-01/dropouts.json` | §4 | yes |
| `audio/aud-01/intelligibility.json` | §5 | yes |
| `licensing/licensing_bundle.json` | §9.1 | yes |
| `decisions/D-audio-licensing.md` | data-model §1.5 | yes |
| `environment.json` / `toolchain_selftest.json` | §1, §11 | yes |

---

## 6. Gate verdict criteria

| Criterion | Pass condition |
| --- | --- |
| Duration | `duration_s ≥ 600` in a single closed capture |
| Dropouts | `dropout_count = 0` at ≥ 50 ms, `detectors_agree = true` |
| RTP correlation | every gap explained; residual gaps = 0 |
| Phrases | `phrases_exact ≥ 18` (non-anchor), `corpus_wer ≤ 0.05` |
| Nonces and anchors | all 10 nonces matched, both anchors matched |
| Observer | named, non-empty; `phrase_script_ref` recorded |
| Consumer | `consumer_renegotiations = 0` |
| Device | `device_pin = "VB-CABLE"`, driver pack 45, `signature_verified = true` on Secure Boot |
| Degraded operation | missing/unsigned/unexpected device gives an actionable state; video-only works |
| Licensing | decision signed with named owner/approver/date, or `conditional`/`no_go` with blocker and plan B |
| Remeasurement | `remeasurement_required = true` when the selected device ≠ the measured device |
| Audio latency | **not gated** — no figure is produced in F0 |

---

## 7. Failure modes

| Failure | Verdict | Action |
| --- | --- | --- |
| Any dropout ≥ 50 ms | `no_go` | Fix the pipeline or the device path, re-record the full run |
| Detectors disagree | `conditional` | Record the finding; resolve the threshold/calibration before re-running |
| Missing calibration | `no_go` | Recalibrate against `calibration-40-60-120.wav`; never use a default threshold |
| `phrases_exact < 18` or WER > 0.05 | `no_go` | Re-run with a quieter room / clearer level, or fix the pipeline |
| Nonce or anchor mismatch | `no_go` | The capture is stale or spliced; discard it and re-record |
| Unnamed observer / no script | `unverifiable` | The human half did not happen; cannot gate |
| Consumer renegotiated | `conditional` | Record the renegotiation; re-run with a stable consumer configuration |
| Signature invalid or Secure Boot off | `no_go` for audio | Session is video-only; replace or reinstall the driver |
| Device absent / unexpected | `conditional` | Video-only ships; record the degraded state as the expected behaviour |
| Licensing unresolved on the gate date | `conditional`/`no_go` | Remove audio from V1 scope, ship video-only, escalate to legal |
| Live run impossible on the host | `blocked_by_environment` | Record the blocked matrix; the apparatus still validates on CI |

---

## 8. Environmental limitations

Recorded honestly, because they determine what this host can and cannot prove:

* **No Windows 11 reference host on this run.** The gate needs Windows 11 build 22000+, Secure Boot,
  VB-CABLE driver pack 45, `signtool.exe`, FFmpeg ≥ 8.1 and a consumer application. This Linux host
  has none of them. `spikes/evidence/environment.json` records
  `within_reference_conditions: false` and `hardware_accelerated: false`.
* **No live audio capture.** `f0-virtual-audio --render` is `#[cfg(windows)]`; on Linux it returns a
  clear refusal instead of writing a capture. There is therefore no `consumer_capture.wav`,
  `dropouts.json`, `intelligibility.json` or `audio_run_capture.json` in the evidence tree.
* **No signature result.** `signature_probe.ps1` needs the Windows runner and the installed driver.
  `signature_verified` stays `false`, so no run would be admissible even if audio existed.
* **No named signatories.** The licensing decision requires a named product owner and approver. The
  engineer cannot invent them, so `licensing_bundle.json` is absent and `D-audio-licensing.md` stays
  `draft` with a `conditional` verdict.
* **No FFmpeg on this host.** The dropout analyser keeps an explicit `AnalyzerUnavailable` path and its
  parser is unit-tested without invoking FFmpeg. That proves the apparatus, not the measurement.
* **What the host *does* prove**: the audio-bridge readiness/degraded/pacing units, the run-index and
  capture validation, the phrase playback plan and transcript template, the two-detector analysis and
  its calibration rules, the intelligibility scoring rules, the licensing catalogue and the validator
  rules. All of it runs on Linux CI and all of it is regression-tested.

The blocked state is recorded in `spikes/evidence/audio/run_matrix.json` with
`status: "blocked_by_environment"` and the exact required environment.

---

## 9. Q-2 / Q-3 / Q-4 findings

| Question | Status | Finding |
| --- | --- | --- |
| **Q-2** — is VB-CABLE accepted by Windows 11 with Secure Boot enabled? | **Unresolved** | The official pages do not state the WHQL/attestation status. The gate must verify it empirically with `signature_probe.ps1`; a failed or unverified signature keeps the session video-only. Hard blocker for the audio gate. |
| **Q-3** — what is the per-unit cost of an OEM/redistribution agreement, and do our users qualify as a "professional domain"? | **Partially answered** | Published volume prices: €3.61/unit at 100, €2.92 at 1,000, €2.50 at 10,000; professional-domain deployments must buy licences. The link-only model avoids the redistribution condition and most of the legal review. The classification of RemoteCam's user population and any bespoke OEM quote remain open with the product owner and legal. |
| **Q-4** — will VAC's "companion application" / "auxiliary transport" characterisation cover RemoteCam? | **Open** | The custom-version terms say the build is compiled with unique names and a key, works only with the companion application, and the companion must provide specific functionality using VAC only as auxiliary transport. Whether RemoteCam qualifies, and at what price, needs a vendor quote and a legal opinion. Parallel track, not a gate blocker. |

Full quoted terms and prices: `spikes/tools/analysis/licensing.py::CANDIDATES`, captured from
<https://vb-audio.com/Cable/index.htm>, <https://vb-audio.com/Services/licensing.htm>,
<https://vac.muzychenko.net/en/purchase.htm>, <https://vac.muzychenko.net/en/customversion.htm>,
<https://github.com/duncanthrax/scream>, <https://steelseries.com/gg/sonar> and the Microsoft
kernel-mode code-signing policy. SteelSeries Sonar is recorded `unverified` because no redistribution
or OEM terms are published — it is never guessed.

---

## 10. Reproducibility

A second engineer can reproduce this gate by:

1. Running `make preflight` and `make selftest` on the Windows 11 reference host.
2. Running `signature_probe.ps1` against the installed VB-CABLE driver pack 45.
3. Running `play_phrases.py` to produce the plan, the transcript template and the phrase audio.
4. Rendering into `CABLE Input` with `f0-virtual-audio --render ... --signature-verified`.
5. Recording `CABLE Output` in a consumer application for ≥ 10 minutes under the pinned profile.
6. Emitting `audio_run_capture.json` from the closed capture.
7. Running `analysis.dropouts` and `analysis.intelligibility` over the capture and transcript.
8. Running `analysis.licensing --final` with the named owner and approver.
9. Running `make validate-evidence` and recording the decision.

Steps 3–8 use the same phrase script and the same threshold rules every time; a changed script or
threshold invalidates comparability.

---

## 11. Non-production statement

**All code in `spikes/f0-virtual-audio/`, `spikes/tools/analysis/dropouts.py`,
`spikes/tools/analysis/intelligibility.py`, `spikes/tools/analysis/licensing.py` and the audio-bridge
Windows adapters used by this gate are NON-PRODUCTION spike harnesses. They MUST NOT be merged into
`desktop/` or `mobile/` product modules (FR-039).** The `remotecam-audio-bridge` crate is the product
seam exercised by the spike; promoting it requires a new implementation task in the product plan.
