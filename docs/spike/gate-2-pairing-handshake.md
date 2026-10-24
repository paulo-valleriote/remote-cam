# Gate 2 — Pairing handshake

**Gate**: authenticated pairing handshake · **Wave**: 1 · **Depends on**: Foundational (protocol
contract) · **Accountable role**: security reviewer · **Review**: 2026-10-23 — F0 closed with exceptions (gate `conditional`, C-02 waived)

**Preconditions**: PC-5 (per-origin throttling proved, not asserted) and PC-6 (zero pairing
artefacts after a normal session end **and** after an application restart, same method).

**Procedure owner**: the engineer running the gate. **Sign-off**: the security reviewer named in
`spikes/evidence/gate_report.json`.

---

## 1. What this gate proves

1. The channel is TLS 1.3 with an ephemeral certificate generated per session; no downgrade and no
   unauthenticated fallback exists (FR-001).
2. The mobile validates the desktop certificate by exact SHA-256 over the **DER-encoded leaf** and
   aborts on a mismatch, a missing pin or chain-only trust (FR-002).
3. Media fingerprints travel only inside the authenticated channel, and a tampered fingerprint
   aborts before media starts (FR-003).
4. Every adversarial attempt is rejected with a code from the closed set in
   [`contracts/reason-codes.md`](../../specs/001-f0-technical-spikes/contracts/reason-codes.md)
   (FR-004).
5. No session key, token, fingerprint or pairing record survives a session end or an application
   restart, using the same inspection method and stating its coverage (FR-005, PC-6).
6. The scenario harness records preconditions, expected result, observed result and a trace
   reference per scenario (FR-006).
7. Pairing time and the delay exact pinning adds are measured and reported (FR-007).
8. The per-origin limit of 5 attempts/minute, the progressive block and the listener shutdown at 10
   consecutive failures are observed, with a legitimate second origin unaffected (FR-049, PC-5).

**Out of scope**: the full protocol session state machine, the production QR experience and any
cloud or non-local-network scenario.

---

## 2. Preconditions

| # | Check | If it fails |
| --- | --- | --- |
| PC-5 | The listener observes the 6th attempt in a minute as `rate_limited`, a strictly increasing block, and `listener_closed` at the 10th consecutive failure. | The gate cannot pass; report `evidence_incomplete`. |
| PC-6 | Both residue inspections exist, name the same method and coverage, cover `session_key`, `pairing_token`, `fingerprint` and `pairing_record`, inspect both devices, and find nothing. | Any residue is a hard failure of the gate, independent of the verdict. |

The security reviewer confirms the thresholds are properties of the implementation, not
configuration values written into the procedure.

---

## 3. Run the automated matrix

```bash
make test                      # protocol, signaling and harness unit tests
make spike-handshake           # 13 handshake + 3 throttling scenarios over real TLS 1.3
make validate-evidence         # schema and contract checks on the produced evidence
```

Expected artefacts:

* `spikes/evidence/pairing/pairing_scenarios.json`
* `spikes/evidence/pairing/pairing_throttling.json`
* one `spikes/evidence/pairing/<scenario_id>.trace` per scenario (local-only, gitignored)

The harness runs the real listener (`spikes/f0-signaling`) and the real pinned client over
loopback TLS 1.3. Nothing in the TLS, pinning or token path is mocked. It exits non-zero if any
observed code differs from the pinned expectation, and each entry carries `expectation_met`.

### 3.1 Scenario table

| Scenario | Expected outcome | Closed code |
| --- | --- | --- |
| Valid pairing | Accepted | — |
| Wrong fingerprint (one byte differs) | Rejected | `certificate_pin_mismatch` |
| Chain-only trust attempt | Rejected | `certificate_chain_rejected` |
| TLS downgrade attempt | Rejected | `tls_version_unsupported` |
| Expired pairing token | Rejected | `token_expired` |
| Reused pairing token (replay) | Rejected | `token_already_used` |
| Wrong token | Rejected | `token_invalid` |
| Connection from a non-private address | Rejected | `source_address_not_private` |
| Interception proxy | Rejected | `certificate_pin_mismatch` |
| QR expired before scanning | Rejected | `qr_expired` |
| Malformed QR payload | Rejected | `invalid_qr_payload` |
| QR with a public endpoint | Rejected | `no_private_endpoint` |
| Tampered DTLS fingerprint | Rejected | `protocol_violation` |

Every rejected scenario must also show `media_flowed_before_authentication: false` and an empty
`residual_credentials` list. The harness has no media path, and the listener constructs its
authenticated session only after the token and fingerprint checks pass; both facts are stated in the
observed result rather than asserted in prose.

### 3.2 Timing (FR-007)

`established_channel_ms` is measured from pairing start to the authenticated channel. The harness
also runs an acceptance-only control connection (never selectable on a product path) and reports
`pinning_added_delay_ms = pinned − control`. A negative difference is reported as observed; the
gate judges only that the delay is recorded, not that it is below a threshold.

### 3.3 Throttling (PC-5)

| Scenario | What it proves |
| --- | --- |
| `throttle-rate-limit` | 5 attempts allowed, the 6th refused with `rate_limited`, the block recorded; a second origin pairs successfully. |
| `throttle-listener-close` | 10 consecutive failures from one origin close the listener with `listener_closed`, recorded as a security event; a second origin pairs successfully before the closure. |
| `throttle-legitimate-origin` | A longer flood from the first origin does not deny service to the second origin. |

An allowed attempt is recorded as an observation with a null reason code; every enforcement decision
carries a closed code and an observed outcome. The scope is per origin address, and the two origins
are distinct loopback source addresses so the scope is actually observable.

---

## 4. Review the evidence

The security reviewer checks each item and records a finding with severity and owner for any miss.

- [ ] The valid scenario is accepted; every adversarial scenario is rejected with the table's code.
- [ ] `media_flowed_before_authentication` is `false` in all 13 scenarios.
- [ ] `residual_credentials` is empty in all 13 scenarios.
- [ ] No rejection is followed by an unauthenticated fallback. The absence of a fallback path is an
      assertion: the client refuses the peer before the pairing message is sent.
- [ ] `established_channel_ms` and `pinning_added_delay_ms` are present on the valid scenario.
- [ ] `attempt_limit_per_minute` is 5, `limit_exceeded_at_attempt` is 6.
- [ ] `block_duration_s` strictly increases across successive violations.
- [ ] `consecutive_failures_at_listener_close` is 10, `listener_closed` is `true`,
      `security_event` is `true`.
- [ ] `session_established` is `false` for the throttled origin.
- [ ] `legitimate_pairing_unaffected` is `true`.
- [ ] The tokens are never present in the evidence, traces or logs in full; only fingerprints are
      referenced through their artefacts and never logged in full.
- [ ] `make validate-evidence` reports zero errors.

---

## 5. Residue inspection (PC-6)

This is a **two-device** procedure. The automated scan is not evidence until both the Windows 11
desktop and the Android reference device have been inspected with the same method.

**Method** (recorded verbatim in `pairing_residue.json`): a recursive byte scan of each named
app-data directory, including files, journal and crash-recovery stores, for the session key, the
pairing token and the TLS fingerprint, in raw and lowercase-hex form; process memory, the platform
credential/certificate store and `HKCU\Software\<vendor>\<app>` are inspected by commands whose
combined output is scanned with the same needles, on both devices.

`residue_scan.py` runs the platform probes with `--probe label=command`, so a registry query, a
credential-store listing, an `adb` enumeration or a memory-dump scan uses the same needles and the
same reporting as the filesystem scan.

### 5.1 Desktop (Windows 11)

1. Before teardown, capture the session key, pairing token and fingerprint as hex (they are not
   written to disk). Pass them as `--needle label=hex`.
2. **`after_session_end`**: end the session normally, then run:
   ```powershell
   cargo run --manifest-path spikes/f0-handshake/Cargo.toml -- `
     --device desktop:windows-11-reference:$env:APPDATA\RemoteCam `
     --device mobile:android-reference:C:\android-app-data `
     --needle pairing_token=<hex> --needle fingerprint=<hex>
   ```
3. Inspect the process memory with `procdump -ma <pid> <dump>` and scan the dump with
   `--root <dump-dir>`; inspect the certificate/credential stores with
   `--probe certs=certutil -store My` and `--probe credentials=cmdkey /list`; enumerate
   `HKCU\Software\<vendor>\<app>` with
   `--probe registry=reg query HKCU\Software\<vendor>\<app> /s`.
4. **`after_application_restart`**: start a session again, terminate both applications **without
   unwinding** (Task Manager / `Stop-Process -Force`, `adb shell am force-stop`), restart them, and
   replay the previous token. It must be refused with `token_invalid`.
5. Re-run the same command with `--restart-observed`. The harness writes both inspections with the
   same `method`, the stated `coverage`, all four `artefact_kinds_checked`, both devices, and an
   empty `artefacts_found` list.

### 5.2 Android

1. Enumerate the app data with `adb shell run-as <package> ls -R` and pull the files to the
   inspection root.
2. Search for the same needles with
   `spikes/f0-handshake/residue_scan.py --root <dir> --needle ... --probe package="adb shell run-as <package> ls -R"`.
3. Force-stop and restart the app between the two cases; replay the previous token; it must fail.

### 5.3 Scanner self-test (required)

```bash
cd spikes/f0-handshake && python -m pytest tests -q
```

`tests/test_residue_scanner.py` seeds a dummy token in a scratch store, detects it, removes it and
shows a clean tree. The self-test must pass **before** a clean residue result is accepted; otherwise
the scanner could be vacuously passing.

### 5.4 Expected file

`spikes/evidence/pairing/pairing_residue.json` with:

* `method` — the same text for both inspections;
* `coverage` — every location searched (app data, journals, credential/certificate store, registry,
  process memory, both devices);
* two inspections, `after_session_end` and `after_application_restart`;
* all four artefact kinds in both;
* `artefacts_found: []` in both;
* `devices_inspected` naming both devices in the document and in each inspection.

---

## 6. Verdict

| Outcome | Condition |
| --- | --- |
| **go** | All 13 scenarios match, throttling matches PC-5, both residue inspections are clean, the scanner self-test passed, and `make validate-evidence` has zero errors. |
| **no-go** | Any adversarial scenario is accepted, any code differs, any residue is found, or the listener is not closed at 10 consecutive failures. |
| **conditional** | A non-security criterion is incomplete (for example the reference-device residue run has not been performed). Requires a written waiver from the security reviewer with a resolution deadline, and it blocks F1's protocol implementation. |

Current status on the F0 development host: the 13 handshake and 3 throttling scenarios pass against
the real implementation; `pairing_scenarios.json` and `pairing_throttling.json` validate. The
two-device residue run has **not** been performed because this host is neither the Windows 11
reference machine nor the Android reference device, and the harness therefore does not emit
`pairing_residue.json`. The gate verdict is pending that run and the security reviewer's sign-off.

---

## 7. Findings carried into the protocol phase

* The pinned verifier distinguishes chain-only trust from a random wrong certificate, so the
  protocol phase must keep the error taxonomy and never collapse it to a generic TLS failure.
* The token is compared in constant time and is invalidated by the first success; the store is
  zeroed on teardown. A future persistence layer cannot be added without invalidating FR-005.
* Throttling keys on the source address. A global limit is a defect, and the scenario that proves
  the second origin is part of the gate, not an optional extra.
* The first signaling frame must be the `pair` message; anything else is a protocol violation. The
  product state machine should keep that strictness.
