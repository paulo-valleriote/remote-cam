"""Licensing assessment for the virtual audio gate (T107).

Every candidate device is recorded with the **official** terms page and the
quoted text that supports the claim. A claim without captured text is
unverifiable, and an unverified redistribution position is recorded explicitly
as ``unverified`` rather than guessed.

The bundle is a *draft* until a named decision owner, a named approver, a date
and a selected option exist. An unresolved decision is never a silent pass: it
is recorded as ``conditional`` with the video-only fallback and the escalation
path. ``--final`` refuses to write a bundle that cannot be signed.

Terms were captured on 2026-10-23 from the live official pages. Signature state
is recorded ``false`` on this host because ``Get-AuthenticodeSignature`` and
``signtool verify /pa /v`` require the Secure-Boot Windows 11 runner; the field
means "not verified", never "verified by assumption".

**NON-PRODUCTION** — F0 risk-reduction spike tooling.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Any, Final

ASSESSMENT_VERSION: Final = "1.0.0"
TERMS_CAPTURED_AT: Final = "2026-10-23"
MEASUREMENT_OPTION: Final = "vb-cable-base"
DEFAULT_FALLBACK: Final = "video-only V1"
ESCALATION_PATH: Final = (
    "Product owner escalates to legal review; if the decision is still unresolved on the gate "
    "date, ship V1 video-only and record the gate as conditional with this condition blocking "
    "the audio scope only."
)
SIGNATURE_NOTE: Final = (
    "Not verified on the Linux host. Verification requires Get-AuthenticodeSignature on the "
    ".cat/.sys plus signtool verify /pa /v on the Secure-Boot-enabled Windows 11 runner. "
    "Recorded false (unverified), never assumed."
)
SIGNATURE_NOTE_OWN_DRIVER: Final = (
    "No driver exists yet; signing would require an EV code-signing certificate, Partner Center "
    "attestation and HLK testing. Recorded false."
)
SIGNATURE_NOTE_NO_DEVICE: Final = (
    "No third-party driver is shipped; the signature check does not apply."
)

VB_CABLE_TERMS: Final = """
"We allow to distribute VB-CABLE package (available one www.vb-cable.com) with your
Application (free or commercial) and possibly embed it in your installation package
(VB-CABLE silent installation) if our donationware model is applicable."

"Donationware model is applicable", means the End User can see and identify VB-CABLE as
VB-Audio application AND this End User must be in a position to donate/pay license if
finding it useful.

"If you distribute VB-CABLE in a professional domain (to companies, institutions,
organizations) where users / employees are not in position to see VB-CABLE and pay for a
license, then you will have to buy the requested number of VB-CABLE licenses directly on
our webshop."

Volume licensing base prices range for VB-CABLE: 100 Licenses: 3.61 EUR / unit; 1000
Licenses: 2.92 EUR / unit; 10.000 Licenses: 2.50 EUR / unit.
""".strip()

VAC_TERMS: Final = """
"A custom VAC version has the same functionality as the common product but is intended
specially for application developers. Each version is compiled with unique vendor,
product, driver/device names and parameters provided by a customer and works only together
with customer's application. A custom version can only be distributed together with such
companion application."

"The companion application must provide a specific functionality to a user, using VAC
driver only as an auxiliary audio stream transport tool in particular software
configurations."

"One license for the full-featured version of VAC costs 15-50 USD, depending on the type
and quantity." Business license: $50 for 1, $20 for 100, $15 for 500.
""".strip()

VOICEMEETER_TERMS: Final = """
"It's free to distribute or promote Voicemeeter (or Banana) AS IS to End Users if our
donationware model stays applicable."

"Voicemeeter Potato package cannot be distributed or bundled with other product but can be
pre-installed on PCs for free."

Volume licensing base prices range for Voicemeeter Standard: 100 Licenses: 10.0 EUR / unit.
""".strip()

SCREAM_TERMS: Final = """
"Scream is a virtual device driver for Windows that provides a discrete sound device.
Audio played through this device is published on your local network as a PCM multicast
stream."

"Scream is based on Microsoft's MSVAD audio driver sample code. The original code is
licensed under MS-PL, as are my changes and additions."

Windows 11 install: "Ensure that Secure Boot is not enabled. Enable Test Mode: ...
bcdedit /set testsigning on."
""".strip()

SONAR_TERMS: Final = """
"Free Download" - Sonar is distributed only as part of SteelSeries GG; Windows 10 (RS5+).
No redistribution, bundling or OEM licence is published on the official Sonar page or in
the SteelSeries terms of use. The redistribution position is recorded unverified.
""".strip()

OWN_DRIVER_TERMS: Final = """
"Starting with Windows 10, version 1607, Windows will not load any new kernel-mode drivers
which are not signed by the Dev Portal. To get your driver signed, first Register for the
Windows Hardware Dev Center program. Note that an EV code signing certificate is required
to establish a dashboard account."
""".strip()

PLAN_B_TERMS: Final = """
"The approved fallback is V1 ships video-only if the licensing decision is not closed by
the gate date." Own WDM driver work is deferred to V2 behind a new technical design.
""".strip()


def _option(
    *,
    option_id: str,
    device_name: str,
    vendor: str,
    version: str,
    license_model: str,
    terms_url: str,
    terms_captured_text: str,
    redistribution_allowed: str,
    redistribution_note: str,
    commercial_use_allowed: bool,
    unit_cost: float,
    currency: str,
    cost_model: str,
    support_burden: str,
    support_burden_justification: str,
    user_experience: str,
    signature_verified: bool,
    signature_note: str,
    fallback: str,
    status: str = "candidate",
) -> dict[str, Any]:
    return {
        "option_id": option_id,
        "device_name": device_name,
        "vendor": vendor,
        "version": version,
        "license_model": license_model,
        "terms_url": terms_url,
        "terms_captured_text": terms_captured_text,
        "terms_captured_at": TERMS_CAPTURED_AT,
        "redistribution_allowed": redistribution_allowed,
        "redistribution_note": redistribution_note,
        "commercial_use_allowed": commercial_use_allowed,
        "unit_cost": unit_cost,
        "currency": currency,
        "cost_model": cost_model,
        "support_burden": support_burden,
        "support_burden_justification": support_burden_justification,
        "user_experience": user_experience,
        "signature_verified": signature_verified,
        "signature_note": signature_note,
        "fallback": fallback,
        "status": status,
    }


CANDIDATES: Final[tuple[dict[str, Any], ...]] = (
    _option(
        option_id=MEASUREMENT_OPTION,
        device_name="VB-CABLE",
        vendor="VB-Audio",
        version="base package, driver pack 45",
        license_model="donationware",
        terms_url="https://vb-audio.com/Services/licensing.htm",
        terms_captured_text=VB_CABLE_TERMS,
        redistribution_allowed="yes_with_conditions",
        redistribution_note=(
            "Distribution with the application is allowed while the donationware model stays "
            "applicable: the user must see VB-CABLE as a VB-Audio application and be able to "
            "pay. Professional-domain deployments require purchased licenses."
        ),
        commercial_use_allowed=True,
        unit_cost=3.61,
        currency="EUR",
        cost_model=("one-off per unit; 3.61 EUR at 100, 2.92 EUR at 1,000, 2.50 EUR at 10,000"),
        support_burden="low",
        support_burden_justification=(
            "We ship no driver code: the user installs the official signed package and RemoteCam "
            "only detects and verifies it. Support is limited to install guidance."
        ),
        user_experience=(
            "User extracts the official package, runs setup as administrator and reboots. No "
            "default-device change. RemoteCam renders to CABLE Input; the consumer records from "
            "CABLE Output."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE,
        fallback="Link the user to the official download; video-only if the device is absent.",
        status="measurement_device",
    ),
    _option(
        option_id="vac-oem",
        device_name="Virtual Audio Cable",
        vendor="Muzychenko (VAC)",
        version="custom/OEM build of VAC 4.x",
        license_model="proprietary_paid",
        terms_url="https://vac.muzychenko.net/en/customversion.htm",
        terms_captured_text=VAC_TERMS,
        redistribution_allowed="yes_with_conditions",
        redistribution_note=(
            "Only a custom build may be distributed, and only together with the companion "
            "application. Whether RemoteCam qualifies as a companion using VAC as auxiliary "
            "transport is Q-4 and requires a quote plus a legal opinion."
        ),
        commercial_use_allowed=True,
        unit_cost=50.0,
        currency="USD",
        cost_model=(
            "Business licence, one per running instance: $50 for 1, $20 for 100, $15 for 500; "
            "custom/OEM build requires a separate quote"
        ),
        support_burden="medium",
        support_burden_justification=(
            "A custom build needs a vendor-issued key and a companion handshake; failures become "
            "vendor-supported integration work."
        ),
        user_experience=(
            "Installed as part of the product installer. No default-device takeover, but the "
            "custom build is tied to the companion application and adds an activation handshake."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE,
        fallback="VB-CABLE link-only.",
    ),
    _option(
        option_id="voicemeeter",
        device_name="VoiceMeeter",
        vendor="VB-Audio",
        version="Standard / Banana (Potato excluded from bundling)",
        license_model="donationware",
        terms_url="https://vb-audio.com/Services/licensing.htm",
        terms_captured_text=VOICEMEETER_TERMS,
        redistribution_allowed="yes_with_conditions",
        redistribution_note=(
            "Standard and Banana may be distributed AS IS while the donationware model applies; "
            "Potato cannot be bundled. This is a mixer, not a plain cable."
        ),
        commercial_use_allowed=True,
        unit_cost=10.0,
        currency="EUR",
        cost_model="one-off per unit; 10.0 EUR at 100, 7.14 EUR at 1,000",
        support_burden="high",
        support_burden_justification=(
            "A mixer takes over default playback/recording devices and its routing matrix becomes "
            "part of the support surface; this conflicts with the rule that RemoteCam never "
            "changes the consumer's device selection."
        ),
        user_experience=(
            "Adds a mixer UI and virtual devices, changes default-device selection and needs "
            "per-application routing. High friction for a webcam-only product."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE,
        fallback="VB-CABLE link-only; video-only if no cable is present.",
    ),
    _option(
        option_id="scream",
        device_name="Scream",
        vendor="duncanthrax (open source)",
        version="latest signed Windows 10 build; Windows 11 requires Test Mode",
        license_model="permissive_open_source",
        terms_url="https://github.com/duncanthrax/scream",
        terms_captured_text=SCREAM_TERMS,
        redistribution_allowed="yes_with_conditions",
        redistribution_note=(
            "MS-PL permits source redistribution, but the driver is cross-signed: Windows 11 "
            "installation requires Secure Boot off and Test Mode, so it cannot be shipped as a "
            "production device."
        ),
        commercial_use_allowed=True,
        unit_cost=0.0,
        currency="USD",
        cost_model="no licence fee; engineering and support cost only",
        support_burden="high",
        support_burden_justification=(
            "Architecturally wrong for this product: it publishes PCM to the LAN and exposes no "
            "capture endpoint, and Windows 11 requires disabling Secure Boot."
        ),
        user_experience=(
            "Requires Secure Boot off, Test Mode and a separate receiver. Unacceptable for a "
            "consumer install."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE,
        fallback="VB-CABLE link-only; video-only.",
    ),
    _option(
        option_id="steelseries-sonar",
        device_name="SteelSeries Sonar",
        vendor="SteelSeries",
        version="SteelSeries GG / Sonar (Windows 10 RS5+)",
        license_model="proprietary_paid",
        terms_url="https://steelseries.com/gg/sonar",
        terms_captured_text=SONAR_TERMS,
        redistribution_allowed="unverified",
        redistribution_note=(
            "No redistribution or OEM licence is published. Recorded unverified; it must not be "
            "bundled on this evidence."
        ),
        commercial_use_allowed=False,
        unit_cost=0.0,
        currency="USD",
        cost_model="free download; no commercial redistribution right identified",
        support_burden="high",
        support_burden_justification=(
            "Sonar grabs default playback and microphone devices and is tied to SteelSeries GG; "
            "it is not a plain virtual cable."
        ),
        user_experience=(
            "Requires the full SteelSeries GG suite, account and device routing; changes default "
            "audio devices."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE,
        fallback="VB-CABLE link-only; video-only.",
        status="rejected_unverified_terms",
    ),
    _option(
        option_id="own-wdm-driver",
        device_name="Own WDM/WaveRT virtual audio driver",
        vendor="RemoteCam",
        version="not built",
        license_model="own_driver",
        terms_url=(
            "https://learn.microsoft.com/en-us/windows-hardware/drivers/install/"
            "kernel-mode-code-signing-policy--windows-vista-and-later-"
        ),
        terms_captured_text=OWN_DRIVER_TERMS,
        redistribution_allowed="yes_with_conditions",
        redistribution_note=(
            "We would own the code, but shipping requires Microsoft Dev Portal signing, an EV "
            "certificate and HLK/attestation; not achievable in V1."
        ),
        commercial_use_allowed=True,
        unit_cost=0.0,
        currency="USD",
        cost_model=(
            "no per-unit fee; 60-90 dev-days, EV code-signing certificate, Partner Center "
            "attestation pipeline and ongoing HLK maintenance"
        ),
        support_burden="high",
        support_burden_justification=(
            "A kernel audio driver carries signing, HVCI/WDAC and per-Windows-release maintenance "
            "risk; roughly a third of the V1 budget."
        ),
        user_experience=(
            "Cleanest long-term install (bundled, no third party), but it does not exist for V1."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE_OWN_DRIVER,
        fallback="V1 ships video-only; own driver deferred to V2 behind a new TDD.",
        status="rejected_for_v1",
    ),
    _option(
        option_id="plan-b-video-only",
        device_name="Video-only V1",
        vendor="RemoteCam",
        version="V1 scope",
        license_model="plan_b_video_only",
        terms_url=(
            "https://github.com/paulo-valleriote/remote-cam/blob/main/"
            "specs/001-f0-technical-spikes/research.md"
        ),
        terms_captured_text=PLAN_B_TERMS,
        redistribution_allowed="no",
        redistribution_note="No third-party audio device is shipped or required in this option.",
        commercial_use_allowed=True,
        unit_cost=0.0,
        currency="USD",
        cost_model="no audio licence cost; the audio feature is not shipped in V1",
        support_burden="low",
        support_burden_justification="No audio driver, no audio support surface.",
        user_experience=(
            "The product works as a phone-as-webcam with no system audio path. Coherent and "
            "sellable, but the audio half is deferred."
        ),
        signature_verified=False,
        signature_note=SIGNATURE_NOTE_NO_DEVICE,
        fallback="n/a - this is the fallback.",
        status="approved_fallback",
    ),
)

OPTIONS_COMPARED: Final[tuple[str, ...]] = (
    "redistribution_licensing",
    "user_installed_official_driver",
    "plan_b_video_only",
)


class LicenseDecisionError(ValueError):
    """A final bundle was requested without a complete, signed decision."""


def measurement_device(candidates: Sequence[dict[str, Any]] = CANDIDATES) -> dict[str, Any]:
    for candidate in candidates:
        if candidate["option_id"] == MEASUREMENT_OPTION:
            return candidate
    raise LicenseDecisionError(f"the catalogue is missing {MEASUREMENT_OPTION}")


def build_bundle(
    *,
    bundle_id: str = "lic-01",
    selected_option: str | None = None,
    decision_owner: str = "",
    approver: str = "",
    decided_at: str | None = None,
    fallback: str = DEFAULT_FALLBACK,
    candidates: Sequence[dict[str, Any]] = CANDIDATES,
    require_final: bool = False,
) -> dict[str, Any]:
    """Assemble the contract ``licensing_bundle.json`` document.

    A missing named owner, named approver, date or selected option keeps the
    bundle in ``draft`` and the gate ``conditional``. ``require_final`` raises
    instead of emitting an unsigned bundle.
    """

    option_ids = {candidate["option_id"] for candidate in candidates}
    if selected_option is not None and selected_option not in option_ids:
        raise LicenseDecisionError(
            f"selected option {selected_option!r} is not in the catalogue {sorted(option_ids)}"
        )

    unresolved: list[str] = []
    if not decision_owner.strip():
        unresolved.append("decision_owner is not a named individual")
    if not approver.strip():
        unresolved.append("approver is not a named individual")
    if not decided_at:
        unresolved.append("decided_at is not recorded")
    if selected_option is None:
        unresolved.append("no option has been selected")

    if require_final and unresolved:
        raise LicenseDecisionError(
            "a final licensing bundle requires a named owner, a named approver, a date and a "
            "selected option; unresolved: " + "; ".join(unresolved)
        )

    final = not unresolved
    remeasurement_required = selected_option is not None and selected_option != MEASUREMENT_OPTION

    conditions = []
    if not final:
        conditions.append(
            {
                "condition_id": "lic-decision-open",
                "description": (
                    "The audio licensing decision has no signed owner/approver/date or selected "
                    "option. Resolve before the audio scope ships; otherwise V1 ships video-only."
                ),
                "conditioned_phases": ["F1"],
                "status": "blocking",
            }
        )

    return {
        "bundle_id": bundle_id,
        "assessment_version": ASSESSMENT_VERSION,
        "bundle_status": "final" if final else "draft",
        "measurement_device": measurement_device(candidates),
        "candidate_options": list(candidates),
        "options_compared": list(OPTIONS_COMPARED),
        "decision_owner": {"role_type": "product_owner", "person_name": decision_owner},
        "approver": {"role_type": "product_owner", "person_name": approver},
        "decided_at": decided_at,
        "selected_option": selected_option,
        "decision": {
            "status": "recorded" if final else "pending",
            "verdict": "go" if final else "conditional",
            "fallback": fallback,
        },
        "gate_verdict": "go" if final else "conditional",
        "conditions": conditions,
        "fallback": fallback,
        "escalation": {
            "owner_role": "product_owner",
            "path": ESCALATION_PATH,
        },
        "unresolved": unresolved,
        "remeasurement_required": remeasurement_required,
        "remeasurement_rule": (
            "If the selected device is not the measurement device, it MUST be re-measured "
            "against the same recorded steps before it becomes the shipping option."
        ),
        "signature_verification": {
            "verified": False,
            "method": "Get-AuthenticodeSignature + signtool verify /pa /v",
            "environment": "Secure-Boot-enabled Windows 11 reference runner (not this host)",
            "note": "No candidate signature is recorded as verified on the Linux host.",
        },
        "notes": [
            "Terms were captured from the official pages on 2026-10-23.",
            "An unverified redistribution position is recorded as unverified, never guessed.",
            "An unresolved decision on the gate date is conditional/no_go, never a silent pass.",
        ],
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build the virtual audio licensing bundle")
    parser.add_argument("--bundle-id", default="lic-01")
    parser.add_argument("--selected-option", default=None)
    parser.add_argument("--decision-owner", default="")
    parser.add_argument("--approver", default="")
    parser.add_argument("--decided-at", default=None)
    parser.add_argument("--fallback", default=DEFAULT_FALLBACK)
    parser.add_argument(
        "--final",
        action="store_true",
        help="refuse to write unless the decision is signed and complete",
    )
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    try:
        bundle = build_bundle(
            bundle_id=args.bundle_id,
            selected_option=args.selected_option,
            decision_owner=args.decision_owner,
            approver=args.approver,
            decided_at=args.decided_at,
            fallback=args.fallback,
            require_final=args.final,
        )
    except LicenseDecisionError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(bundle, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out} ({bundle['bundle_status']})")
    return 0 if bundle["bundle_status"] == "final" else 1


if __name__ == "__main__":
    raise SystemExit(main())


__all__ = [
    "ASSESSMENT_VERSION",
    "CANDIDATES",
    "DEFAULT_FALLBACK",
    "ESCALATION_PATH",
    "LicenseDecisionError",
    "MEASUREMENT_OPTION",
    "OPTIONS_COMPARED",
    "TERMS_CAPTURED_AT",
    "build_bundle",
    "main",
    "measurement_device",
]
