# F0 Quickstart Reproduction Rehearsal

**Purpose:** Validate that a second engineer, who has never seen the spike, can
reproduce every F0 gate result within one working day using only the recorded
procedure and environment description (FR-038, SC-009).

**Rehearsal engineer:** not yet assigned (requires a second engineer)
**Rehearsal date:** not yet scheduled
**Quickstart:** `specs/001-f0-technical-spikes/quickstart.md`
**Phase review deadline:** 2026-11-06

**Status:** planned — live execution requires a second engineer and the Windows 11
reference host. This file records the rehearsal plan and the outcome template.

---

## Rehearsal phases (from quickstart.md)

| Phase | Name | Approx. time | Makefile target |
|-------|------|--------------|-----------------|
| A | Preflight | ~45 min | `make preflight` |
| B | Toolchain self-test | ~45 min | `make selftest` |
| C | Gate 1 — virtual camera injection | ~90 min | `make spike-injection` |
| D | Gate 2 — pairing handshake | ~60 min | `make spike-handshake` |
| E | Gate 3 — desktop receive pipeline | ~3 h | `make spike-pipeline` |
| F | Gate 4 — codec interoperability | ~45 min | `make spike-interop` |
| G | Gate 5 — virtual audio and licensing | ~2 h | `make spike-audio` |
| H | Assemble the report | ~90 min | `make gate-report` |

**Total:** ≈ 7 h of hands-on execution within one working day.

---

## Rehearsal outcome template

The rehearsing engineer completes this section during the rehearsal.

### Phase A — Preflight

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| `environment.json` exists | yes | | |
| `hardware_decode_verification.hardware_accelerated` | `true` | | |
| `hardware_decode_verification.output_format_supported` | `nv12` | | |
| `network.wifi_power_save` | `disabled` | | |
| `network.within_reference_conditions` | `true` | | |
| `make preflight` exits 0 | yes | | |

### Phase B — Toolchain self-test

| Check | Expected | Observed | Pass |
|-------|----------|----------|------|
| `silence_fixture` | exactly 2 dropouts (60, 120 ms) | | |
| `qr_roundtrip` | byte-identical, SHA-256 matches | | |
| `sampler_agreement` | within 1 percentage point | | |
| `harness_floor` | measured and recorded | | |
| `all_passed` | `true` | | |
| `make selftest` exits 0 | yes | | |

### Phase C — Gate 1: camera injection

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| `injection_matrix.json` exists | yes | | |
| Selected backend cells all `frames_delivered` | 100 % | | |
| `observed_duration_s ≥ 1800` | yes | | |
| `longest_stall_ms ≤ 2000` | yes | | |
| 32-bit consumer present | yes | | |
| `make spike-injection` exits 0 | yes | | |

**Gate 1 verdict:** go / no_go / conditional: _______

### Phase D — Gate 2: pairing handshake

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| Valid scenario accepted | yes | | |
| All adversarial rejected | 12/12 | | |
| No media before authentication | yes | | |
| Throttling: attempt 6 refused | yes | | |
| Listener closed at 10 failures | yes | | |
| Second origin unaffected | yes | | |
| Zero residue after session end | yes | | |
| Zero residue after restart | yes | | |
| `make spike-handshake` exits 0 | yes | | |

**Gate 2 verdict:** go / no_go / conditional: _______

### Phase E — Gate 3: desktop receive pipeline

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| Raw p95 ≤ 150 ms (3 runs) | yes | | |
| Harness floor published | yes | | |
| Floor-corrected p95 published | yes | | |
| CPU p95 ≤ 15 % | yes | | |
| Memory max ≤ 400 MB | yes | | |
| Drift ≤ 20 ms/h | yes | | |
| Zero dropouts ≥ 50 ms | yes | | |
| Stall recovery ≤ 2 s | yes | | |
| `make spike-pipeline` exits 0 | yes | | |

**Gate 3 verdict:** go / no_go / conditional: _______

### Phase F — Gate 4: codec interoperability

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| 10 min live video, no corruption | yes | | |
| Negotiated profile/level/format recorded | yes | | |
| VP8 fallback trade-offs measured | yes | | |
| Hardware-decode status recorded | yes | | |
| `make spike-interop` exits 0 | yes | | |

**Gate 4 verdict:** go / no_go / conditional: _______

### Phase G — Gate 5: virtual audio and licensing

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| Zero dropouts ≥ 50 ms | yes | | |
| Detectors agree | yes | | |
| phrases_exact ≥ 18 | yes | | |
| corpus_wer ≤ 0.05 | yes | | |
| All nonces/anchors matched | yes | | |
| Named observer recorded | yes | | |
| Device signature verified | yes | | |
| Licensing decision or conditional | yes | | |
| `make spike-audio` exits 0 | yes | | |

**Gate 5 verdict:** go / no_go / conditional: _______

### Phase H — Assemble the report

| Criterion | Expected | Observed | Pass |
|-----------|----------|----------|------|
| `make gate-report` exits 0 | yes | | |
| `gate_report.json` schema-valid | yes | | |
| All five verdicts recorded | yes | | |
| Shared `phase_review_at` | yes | | |
| `docs/spike/gate-report-f0.md` rendered | yes | | |
| All referenced evidence files exist | yes | | |

**Rehearsal overall:** pass / fail: _______

---

## Deviations found during rehearsal

| # | Phase | Deviation | Fix applied | Re-run result |
|---|-------|-----------|-------------|---------------|
| | | | | |
| | | | | |

---

## Sign-off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Rehearsing engineer | | | |
| Technical lead | | | |

The rehearsal is **pass** only when all eight phases complete with the expected
outcomes, `make gate-report` emits a schema-valid report, and every referenced
evidence file exists inside `spikes/evidence/`.
