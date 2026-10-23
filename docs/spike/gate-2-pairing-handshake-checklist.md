# Gate 2 — security-reviewer checklist

**Gate**: pairing handshake · **Wave**: 1 · **Accountable role**: security reviewer
**Procedure**: [gate-2-pairing-handshake.md](./gate-2-pairing-handshake.md)
**Review**: 2026-10-23 — F0 closed with exceptions (gate `conditional`, C-02 waived)

The reviewer completes this sheet against the evidence produced by the procedure. Every "no" or
"not verified" needs a finding with a severity (`blocker`, `major`, `minor`), an owner and a
resolution deadline. The gate cannot be `go` while a blocker is open.

## Evidence under review

| Artefact | Present | Validated by |
| --- | --- | --- |
| `spikes/evidence/pairing/pairing_scenarios.json` | ☐ | `make validate-evidence` |
| `spikes/evidence/pairing/pairing_throttling.json` | ☐ | `make validate-evidence` |
| `spikes/evidence/pairing/pairing_residue.json` | ☐ | `make validate-evidence` |
| Scanner self-test (`pytest tests`) | ☐ | exit code 0 before accepting a clean scan |
| Traces for every scenario | ☐ | referenced by `trace_ref` |

## Scenario review

| # | Criterion | Yes / No | Finding |
| --- | --- | --- | --- |
| 1 | The valid scenario is accepted and reaches an authenticated channel. | ☐ | |
| 2 | Every adversarial scenario is rejected with the code pinned in the procedure. | ☐ | |
| 3 | `media_flowed_before_authentication` is `false` in all scenarios. | ☐ | |
| 4 | `residual_credentials` is empty in all scenarios. | ☐ | |
| 5 | No rejected scenario is followed by an unauthenticated fallback. | ☐ | |
| 6 | The interception proxy never receives the pairing token. | ☐ | |
| 7 | A one-byte pin mismatch yields `certificate_pin_mismatch`. | ☐ | |
| 8 | Chain-only trust yields `certificate_chain_rejected`, with no CA fallback. | ☐ | |
| 9 | TLS 1.2 is refused with `tls_version_unsupported`. | ☐ | |
| 10 | A tampered DTLS fingerprint aborts before media. | ☐ | |
| 11 | `established_channel_ms` and `pinning_added_delay_ms` are recorded. | ☐ | |
| 12 | No full token, session key or fingerprint appears in the evidence, trace or log. | ☐ | |

## Throttling review (PC-5)

| # | Criterion | Yes / No | Finding |
| --- | --- | --- | --- |
| 13 | The limit is scoped per origin, not globally. | ☐ | |
| 14 | The 6th attempt within a minute is `rate_limited`. | ☐ | |
| 15 | `block_duration_s` strictly increases across successive failures. | ☐ | |
| 16 | The 10th consecutive failure closes the listener with `listener_closed`. | ☐ | |
| 17 | The closure is recorded as a security event, not a product failure. | ☐ | |
| 18 | A second legitimate origin pairs while the first is throttled. | ☐ | |
| 19 | No throttled or blocked origin establishes a session. | ☐ | |
| 20 | Every enforcement decision carries a closed code and an observed outcome. | ☐ | |

## Residue review (PC-6)

| # | Criterion | Yes / No | Finding |
| --- | --- | --- | --- |
| 21 | Both cases exist: `after_session_end` and `after_application_restart`. | ☐ | |
| 22 | Both name the **same** `method` and text. | ☐ | |
| 23 | `coverage` states what was searched, including process memory and the credential/certificate store. | ☐ | |
| 24 | All four artefact kinds are checked in both cases. | ☐ | |
| 25 | Both devices are inspected in each case. | ☐ | |
| 26 | `artefacts_found` is empty in both cases. | ☐ | |
| 27 | The previous token is not reusable after the restart. | ☐ | |
| 28 | The scanner self-test detected a seeded artefact before the clean result was accepted. | ☐ | |

## Verdict

| Field | Value |
| --- | --- |
| Verdict | `go` / `no_go` / `conditional` |
| Blocking dependency (if any) | |
| Findings (severity, owner, deadline) | |
| Conditions or waiver reference (if conditional) | |
| Security reviewer name | |
| Date | |
| Signature | |
