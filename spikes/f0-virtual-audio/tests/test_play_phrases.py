"""Tests for the scripted phrase playback helper (T106)."""

from __future__ import annotations

import csv
import json
import platform
import sys
import wave
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from play_phrases import (
    DEFAULT_MANIFEST,
    PlaybackError,
    build_playback_plan,
    build_transcript_template,
    concatenate_phrase_wavs,
    load_phrases,
    main,
    synthesise_phrases,
)


def test_the_committed_manifest_loads_all_20_phrases() -> None:
    phrases = load_phrases(DEFAULT_MANIFEST)

    assert len(phrases) == 20
    assert phrases[0]["kind"] == "anchor"
    assert phrases[-1]["kind"] == "anchor"
    assert sum(1 for phrase in phrases if phrase["kind"] == "nonce") == 10


def test_the_transcript_template_marks_every_phrase_as_heard_nothing() -> None:
    phrases = load_phrases(DEFAULT_MANIFEST)

    template = build_transcript_template(phrases, run_id="aud-01")

    assert template["observer"] == ""
    assert len(template["transcriptions"]) == 20
    assert set(template["transcriptions"].values()) == {""}
    assert "failure" in template["instructions"]


def test_the_playback_plan_records_the_order_and_pause(tmp_path: Path) -> None:
    phrases = load_phrases(DEFAULT_MANIFEST)

    plan = build_playback_plan(
        phrases,
        manifest=DEFAULT_MANIFEST,
        out_dir=tmp_path,
        pause_ms=1_500,
    )

    assert plan["phrase_count"] == 20
    assert plan["sample_rate_hz"] == 48_000
    assert plan["channels"] == 1
    assert plan["pause_ms"] == 1_500
    assert [phrase["id"] for phrase in plan["phrases"]] == [
        f"P{index:02d}" for index in range(1, 21)
    ]


def tiny_wav(path: Path, frames: int = 480) -> None:
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(48_000)
        handle.writeframes(b"\x01\x00" * frames)


def test_concatenation_inserts_the_requested_silence(tmp_path: Path) -> None:
    first = tmp_path / "P01.wav"
    second = tmp_path / "P02.wav"
    tiny_wav(first)
    tiny_wav(second)
    out = tmp_path / "phrase_audio.wav"

    concatenate_phrase_wavs([first, second], out=out, pause_ms=1_000)

    with wave.open(str(out), "rb") as handle:
        assert handle.getframerate() == 48_000
        assert handle.getnchannels() == 1
        assert handle.getsampwidth() == 2
        assert handle.getnframes() == 480 + 48_000 + 480


def test_concatenation_refuses_a_wrong_format(tmp_path: Path) -> None:
    wrong = tmp_path / "wrong.wav"
    with wave.open(str(wrong), "wb") as handle:
        handle.setnchannels(2)
        handle.setsampwidth(2)
        handle.setframerate(48_000)
        handle.writeframes(b"\x00\x00" * 960)

    with pytest.raises(PlaybackError, match="48 kHz mono"):
        concatenate_phrase_wavs([wrong], out=tmp_path / "out.wav", pause_ms=0)


def test_synthesis_refuses_outside_windows(tmp_path: Path) -> None:
    if platform.system() == "Windows":
        pytest.skip("this test asserts the non-Windows refusal")

    with pytest.raises(PlaybackError, match="Windows SAPI"):
        synthesise_phrases(
            load_phrases(DEFAULT_MANIFEST),
            out_dir=tmp_path,
            pause_ms=0,
        )


def test_dry_run_writes_a_plan_and_a_blank_transcript(tmp_path: Path) -> None:
    exit_code = main(["--out-dir", str(tmp_path), "--dry-run"])

    assert exit_code == 0
    plan = json.loads((tmp_path / "phrase_playback_manifest.json").read_text())
    transcript = json.loads((tmp_path / "transcriptions.json").read_text())
    assert plan["phrase_count"] == 20
    assert transcript["transcriptions"]["P10"] == ""
    assert not (tmp_path / "phrase_audio.wav").exists()


def test_a_manifest_missing_an_anchor_is_refused(tmp_path: Path) -> None:
    manifest = tmp_path / "phrases.csv"
    with manifest.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["phrase_id", "kind", "reference", "nonce"])
        writer.writeheader()
        writer.writerow(
            {
                "phrase_id": "P01",
                "kind": "static",
                "reference": "Only a static phrase.",
                "nonce": "",
            }
        )

    with pytest.raises(PlaybackError, match="anchors"):
        load_phrases(manifest)


def test_a_negative_pause_is_refused(tmp_path: Path) -> None:
    assert main(["--out-dir", str(tmp_path), "--dry-run", "--pause-ms", "-1"]) == 2
