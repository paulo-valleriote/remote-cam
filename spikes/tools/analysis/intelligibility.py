"""Scripted intelligibility check for the virtual audio gate (T099, T106).

The gate plays the 20-phrase manifest through the virtual cable and a human
observer records what they heard. Two independent things are proved:

* the audio is intelligible — at least 18 of the 18 non-anchor phrases are heard
  exactly, with a corpus WER of at most 0.05;
* the capture is the one that was just played — every nonce matches and both
  anchors agree. A single nonce mismatch invalidates the capture as stale or
  spliced, and the run must be repeated.

A phrase the observer heard nothing for is a **failure**, never a missing row.
The observer must be named and the phrase script referenced; without either the
result is ``unverifiable`` and cannot gate.

**NON-PRODUCTION** — F0 risk-reduction spike tooling.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final

import jiwer

ANALYSER_VERSION: Final = "1.0.0"
MANIFEST: Final = Path(__file__).resolve().parents[2] / "fixtures" / "phrases-v1.csv"
PHRASES_EXACT_MIN: Final = 18
CORPUS_WER_MAX: Final = 0.05
KINDS: Final = frozenset({"anchor", "static", "nonce"})
SUPPLEMENTARY_NOTE: Final = "supplementary trend evidence only; never gated"
MANIFEST_COLUMNS: Final = ("phrase_id", "kind", "reference", "nonce")


class ManifestError(ValueError):
    """The phrase manifest is malformed or incomplete."""


@dataclass(frozen=True)
class Phrase:
    """One row of the phrase manifest."""

    phrase_id: str
    kind: str
    reference: str
    nonce: str = ""


def normalise(text: str) -> str:
    """Lowercase, strip punctuation and collapse whitespace before comparison."""

    lowered = text.lower().replace("'", "").replace("’", "")
    without_punctuation = re.sub(r"[^a-z0-9\s]", " ", lowered)
    return " ".join(without_punctuation.split())


def load_manifest(path: Path | str = MANIFEST) -> list[Phrase]:
    """Read and validate the phrase script."""

    manifest_path = Path(path)
    if not manifest_path.exists():
        raise ManifestError(f"phrase manifest not found: {manifest_path}")

    with manifest_path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        header = tuple(reader.fieldnames or ())
        missing = [column for column in MANIFEST_COLUMNS if column not in header]
        if missing:
            raise ManifestError(f"{manifest_path}: missing columns {missing}")
        rows = list(reader)

    if not rows:
        raise ManifestError(f"{manifest_path}: the manifest has no phrases")

    phrases: list[Phrase] = []
    seen: set[str] = set()
    for line_number, row in enumerate(rows, start=2):
        phrase_id = (row.get("phrase_id") or "").strip()
        kind = (row.get("kind") or "").strip()
        reference = (row.get("reference") or "").strip()
        nonce = (row.get("nonce") or "").strip()
        if not phrase_id:
            raise ManifestError(f"{manifest_path}:{line_number}: phrase_id is empty")
        if phrase_id in seen:
            raise ManifestError(f"{manifest_path}:{line_number}: duplicate {phrase_id}")
        if kind not in KINDS:
            raise ManifestError(f"{manifest_path}:{line_number}: unknown kind {kind!r}")
        if not reference:
            raise ManifestError(f"{manifest_path}:{line_number}: reference is empty")
        if kind == "nonce" and not nonce:
            raise ManifestError(f"{manifest_path}:{line_number}: nonce row without a nonce")
        if kind != "nonce" and nonce:
            raise ManifestError(f"{manifest_path}:{line_number}: non-nonce row carries a nonce")
        seen.add(phrase_id)
        phrases.append(Phrase(phrase_id=phrase_id, kind=kind, reference=reference, nonce=nonce))

    closing = phrases[-1]
    if phrases[0].kind != "anchor" or closing.kind != "anchor":
        raise ManifestError(
            f"{manifest_path}: the first and last phrases MUST be the opening and closing anchors"
        )
    return phrases


def _word_error_rate(reference: str, transcribed: str) -> float:
    if not transcribed.strip():
        return 1.0
    return float(jiwer.wer(normalise(reference), normalise(transcribed)))


def build_document(
    *,
    run_id: str,
    phrases: Sequence[Phrase],
    transcriptions: Mapping[str, str],
    observer: str,
    phrase_script_ref: str,
    manifest_ref: str | None = None,
    supplementary: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Assemble the contract ``intelligibility.json`` document.

    ``transcriptions`` maps a phrase id to what the observer wrote down. A
    missing key is treated as "heard nothing" and fails the phrase.
    """

    if not phrases:
        raise ManifestError("the phrase manifest is empty")

    findings: list[str] = []
    verifiable = True
    if not observer.strip():
        verifiable = False
        findings.append(
            "the observer is not named; a named observer is required before the result can gate"
        )
    if not phrase_script_ref.strip():
        verifiable = False
        findings.append(
            "the phrase script reference is missing; record phrase_script_ref so the run is "
            "auditable"
        )

    per_phrase: list[dict[str, Any]] = []
    references: list[str] = []
    hypotheses: list[str] = []
    non_anchor_count = 0
    phrases_exact = 0
    nonces_total = 0
    nonces_matched = 0

    for phrase in phrases:
        transcribed = (transcriptions.get(phrase.phrase_id) or "").strip()
        exact = bool(transcribed) and normalise(phrase.reference) == normalise(transcribed)
        wer = _word_error_rate(phrase.reference, transcribed)
        references.append(phrase.reference)
        hypotheses.append(transcribed)

        if phrase.kind != "anchor":
            non_anchor_count += 1
            if exact:
                phrases_exact += 1

        nonce_matched: bool | None = None
        if phrase.kind == "nonce":
            nonces_total += 1
            nonce_matched = normalise(phrase.nonce) in normalise(transcribed).split()
            if nonce_matched:
                nonces_matched += 1
            else:
                findings.append(
                    f"{phrase.phrase_id}: nonce {phrase.nonce!r} was not heard; the capture may be "
                    "stale or spliced and the run MUST be repeated"
                )
        if not transcribed:
            findings.append(
                f"{phrase.phrase_id}: not heard — no transcription was recorded (a silent phrase "
                "is a failure, not a missing row)"
            )

        entry: dict[str, Any] = {
            "id": phrase.phrase_id,
            "kind": phrase.kind,
            "reference": phrase.reference,
            "transcribed": transcribed,
            "exact": exact,
            "wer": round(wer, 4),
        }
        if nonce_matched is not None:
            entry["nonce_matched"] = nonce_matched
        per_phrase.append(entry)

    anchors = [phrase for phrase in phrases if phrase.kind == "anchor"]
    anchors_match = len(anchors) >= 2 and all(
        bool(transcriptions.get(phrase.phrase_id, "").strip())
        and normalise(phrase.reference)
        == normalise(transcriptions.get(phrase.phrase_id, "").strip())
        for phrase in anchors
    )
    if not anchors_match:
        findings.append(
            "the opening and closing anchors do not both match; the observer was not attending at "
            "both ends of the run"
        )

    corpus_wer = float(jiwer.wer(references, hypotheses))
    if phrases_exact < PHRASES_EXACT_MIN:
        findings.append(
            f"{phrases_exact} non-anchor phrases are exact; at least {PHRASES_EXACT_MIN} of "
            f"{non_anchor_count} are required"
        )
    if corpus_wer > CORPUS_WER_MAX:
        findings.append(f"corpus_wer {corpus_wer:.4f} exceeds the {CORPUS_WER_MAX:.2f} maximum")

    verdict = "unverifiable" if not verifiable else ("fail" if findings else "pass")

    supplementary_document: dict[str, Any] = {"note": SUPPLEMENTARY_NOTE}
    if supplementary:
        supplementary_document.update(supplementary)
        supplementary_document["note"] = SUPPLEMENTARY_NOTE

    return {
        "run_id": run_id,
        "manifest": manifest_ref or phrase_script_ref,
        "phrase_script_ref": phrase_script_ref,
        "phrase_count": len(phrases),
        "non_anchor_count": non_anchor_count,
        "phrases_exact": phrases_exact,
        "corpus_wer": round(corpus_wer, 4),
        "nonces_matched": nonces_matched,
        "nonces_total": nonces_total,
        "anchors_match": anchors_match,
        "observer": observer,
        "verifiable": verifiable,
        "criterion": {
            "phrases_exact_min": PHRASES_EXACT_MIN,
            "corpus_wer_max": CORPUS_WER_MAX,
            "nonces_must_all_match": True,
            "anchors_must_match": True,
        },
        "verdict": verdict,
        "findings": findings,
        "supplementary": supplementary_document,
        "per_phrase": per_phrase,
        "analyser_version": ANALYSER_VERSION,
    }


def analyse(
    *,
    run_id: str,
    manifest_path: Path | str = MANIFEST,
    transcriptions: Mapping[str, str],
    observer: str,
    phrase_script_ref: str | None = None,
    supplementary: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Load the manifest and score one observer transcription."""

    phrases = load_manifest(manifest_path)
    script_ref = phrase_script_ref if phrase_script_ref is not None else str(manifest_path)
    return build_document(
        run_id=run_id,
        phrases=phrases,
        transcriptions=transcriptions,
        observer=observer,
        phrase_script_ref=script_ref,
        manifest_ref=str(manifest_path),
        supplementary=supplementary,
    )


def load_transcriptions(path: Path) -> dict[str, str]:
    """Read observer transcriptions from JSON or ``phrase_id,transcribed`` CSV."""

    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() in {".json"} or text.lstrip().startswith("{"):
        document = json.loads(text)
        if isinstance(document, dict) and isinstance(document.get("transcriptions"), dict):
            document = document["transcriptions"]
        if not isinstance(document, dict):
            raise ValueError(f"{path}: expected an object mapping phrase ids to transcriptions")
        return {str(key): str(value) for key, value in document.items()}

    rows = list(csv.DictReader(text.splitlines()))
    if not rows:
        return {}
    if "phrase_id" not in (rows[0].keys() if rows else ()) or "transcribed" not in rows[0]:
        raise ValueError(f"{path}: the CSV header must contain phrase_id and transcribed")
    return {row["phrase_id"].strip(): (row["transcribed"] or "") for row in rows}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Score the scripted intelligibility check")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--transcript", type=Path, required=True)
    parser.add_argument("--observer", default="")
    parser.add_argument(
        "--script-ref",
        default=None,
        help="reference recorded in the document; defaults to the manifest path",
    )
    parser.add_argument("--asr-wer", type=float, default=None)
    parser.add_argument("--asr-model", default=None)
    parser.add_argument("--estoi-median", type=float, default=None)
    parser.add_argument("--out", type=Path, default=None)
    args = parser.parse_args(argv)

    supplementary: dict[str, Any] = {}
    if args.asr_wer is not None:
        supplementary["asr_wer"] = args.asr_wer
    if args.asr_model is not None:
        supplementary["asr_model"] = args.asr_model
    if args.estoi_median is not None:
        supplementary["estoi_median"] = args.estoi_median

    try:
        document = analyse(
            run_id=args.run_id,
            manifest_path=args.manifest,
            transcriptions=load_transcriptions(args.transcript),
            observer=args.observer,
            phrase_script_ref=args.script_ref,
            supplementary=supplementary or None,
        )
    except (ManifestError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    rendered = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(rendered, encoding="utf-8")
        print(f"wrote {args.out}")
    else:
        print(rendered, end="")

    if document["verdict"] == "pass":
        return 0
    return 2 if document["verdict"] == "unverifiable" else 1


if __name__ == "__main__":
    raise SystemExit(main())


__all__ = [
    "ANALYSER_VERSION",
    "CORPUS_WER_MAX",
    "MANIFEST",
    "ManifestError",
    "PHRASES_EXACT_MIN",
    "Phrase",
    "analyse",
    "build_document",
    "load_manifest",
    "load_transcriptions",
    "main",
    "normalise",
]
