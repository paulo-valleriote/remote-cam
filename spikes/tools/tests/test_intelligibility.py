"""Tests for the scripted phrase check (T099)."""

from __future__ import annotations

import csv
from pathlib import Path

import pytest

from analysis.intelligibility import (
    MANIFEST,
    Phrase,
    analyse,
    build_document,
    load_manifest,
    normalise,
)

OBSERVER = "Observer A (named, consented)"


def exact_transcriptions(phrases: list[Phrase]) -> dict[str, str]:
    return {phrase.phrase_id: phrase.reference for phrase in phrases}


@pytest.fixture(scope="module")
def phrases() -> list[Phrase]:
    return load_manifest(MANIFEST)


def test_the_committed_manifest_has_the_required_composition(phrases: list[Phrase]) -> None:
    assert len(phrases) == 20
    assert [phrase.kind for phrase in phrases].count("anchor") == 2
    assert [phrase.kind for phrase in phrases].count("static") == 8
    assert [phrase.kind for phrase in phrases].count("nonce") == 10
    assert sum(1 for phrase in phrases if phrase.kind != "anchor") == 18
    assert all(phrase.nonce for phrase in phrases if phrase.kind == "nonce")


def test_a_clean_run_passes(phrases: list[Phrase]) -> None:
    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=exact_transcriptions(phrases),
        observer=OBSERVER,
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["phrase_count"] == 20
    assert document["non_anchor_count"] == 18
    assert document["phrases_exact"] == 18
    assert document["corpus_wer"] == 0.0
    assert document["nonces_matched"] == 10
    assert document["nonces_total"] == 10
    assert document["anchors_match"] is True
    assert document["verdict"] == "pass"
    assert document["findings"] == []


def test_fewer_than_18_exact_phrases_fails(phrases: list[Phrase]) -> None:
    transcriptions = exact_transcriptions(phrases)
    transcriptions["P05"] = "We walked along the road until the sun rose."

    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=transcriptions,
        observer=OBSERVER,
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["phrases_exact"] == 17
    assert document["verdict"] == "fail"
    assert any("at least 18" in finding for finding in document["findings"])


def test_corpus_wer_above_five_percent_fails_while_18_phrases_are_exact(
    tmp_path: Path,
) -> None:
    rows: list[dict[str, str]] = [
        {
            "phrase_id": "P01",
            "kind": "anchor",
            "reference": "The opening anchor phrase is clear.",
            "nonce": "",
        }
    ]
    words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet"
    for index in range(20):
        rows.append(
            {
                "phrase_id": f"P{index + 2:02d}",
                "kind": "static",
                "reference": f"{words} {index}.",
                "nonce": "",
            }
        )
    rows.append(
        {
            "phrase_id": "P22",
            "kind": "anchor",
            "reference": "The opening anchor phrase is clear.",
            "nonce": "",
        }
    )
    manifest = tmp_path / "phrases.csv"
    with manifest.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["phrase_id", "kind", "reference", "nonce"])
        writer.writeheader()
        writer.writerows(rows)

    loaded = load_manifest(manifest)
    transcriptions = exact_transcriptions(loaded)
    transcriptions["P02"] = "zulu"
    transcriptions["P03"] = "zulu"

    document = analyse(
        run_id="aud-01",
        manifest_path=manifest,
        transcriptions=transcriptions,
        observer=OBSERVER,
        phrase_script_ref=str(manifest),
    )

    assert document["phrases_exact"] == 18
    assert document["corpus_wer"] > 0.05
    assert document["verdict"] == "fail"
    assert any("corpus_wer" in finding for finding in document["findings"])


def test_a_nonce_mismatch_invalidates_a_stale_capture(phrases: list[Phrase]) -> None:
    transcriptions = exact_transcriptions(phrases)
    transcriptions["P12"] = "Write quimberax in the first empty field."

    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=transcriptions,
        observer=OBSERVER,
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["nonces_matched"] == 9
    assert document["nonces_total"] == 10
    assert document["verdict"] == "fail"
    joined = " ".join(document["findings"]).lower()
    assert "stale" in joined and "spliced" in joined
    assert any("must be repeated" in finding.lower() for finding in document["findings"])


def test_an_anchor_mismatch_fails(phrases: list[Phrase]) -> None:
    transcriptions = exact_transcriptions(phrases)
    transcriptions["P20"] = "The remote camera anchor phrase was not heard."

    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=transcriptions,
        observer=OBSERVER,
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["anchors_match"] is False
    assert document["verdict"] == "fail"
    assert any("anchor" in finding for finding in document["findings"])


def test_an_unnamed_observer_makes_the_result_unverifiable(phrases: list[Phrase]) -> None:
    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=exact_transcriptions(phrases),
        observer="   ",
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["verifiable"] is False
    assert document["verdict"] == "unverifiable"
    assert any("observer" in finding for finding in document["findings"])


def test_a_missing_phrase_script_makes_the_result_unverifiable(phrases: list[Phrase]) -> None:
    document = build_document(
        run_id="aud-01",
        phrases=phrases,
        transcriptions=exact_transcriptions(phrases),
        observer=OBSERVER,
        phrase_script_ref="",
    )

    assert document["verifiable"] is False
    assert document["verdict"] == "unverifiable"
    assert any("phrase script" in finding for finding in document["findings"])


def test_a_phrase_heard_as_nothing_is_a_failure_not_a_missing_row(
    phrases: list[Phrase],
) -> None:
    transcriptions = exact_transcriptions(phrases)
    del transcriptions["P07"]

    document = analyse(
        run_id="aud-01",
        manifest_path=MANIFEST,
        transcriptions=transcriptions,
        observer=OBSERVER,
        phrase_script_ref="spikes/fixtures/phrases-v1.csv",
    )

    assert document["phrases_exact"] < 18
    assert document["verdict"] == "fail"
    row = next(entry for entry in document["per_phrase"] if entry["id"] == "P07")
    assert row["transcribed"] == ""
    assert row["exact"] is False
    assert any("P07" in finding and "not heard" in finding for finding in document["findings"])


def test_normalisation_is_punctuation_and_case_insensitive() -> None:
    assert normalise("  The QUICK brown fox, jumps!  ") == "the quick brown fox jumps"
    assert normalise("word—dash and /slash") == "word dash and slash"
