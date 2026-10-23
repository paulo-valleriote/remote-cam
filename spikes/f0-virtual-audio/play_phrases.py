#!/usr/bin/env python3
"""Render the scripted phrase manifest and prepare the observer transcript.

**NON-PRODUCTION** — F0 risk-reduction spike tooling (T106).

The gate needs the *same* phrases to be played through the virtual cable and
then transcribed by a named human observer. This helper does three things:

1. writes the playback plan and a transcript template
   (`transcriptions.json`) the observer fills in;
2. on Windows, synthesises every phrase with SAPI at 48 kHz mono 16-bit and
   concatenates them into `phrase_audio.wav` with a fixed pause between
   phrases — the harness then renders that file into `CABLE Input`;
3. refuses to claim audio was produced on a host that cannot produce it.

The transcript is deliberately a separate file: the observer must not see the
reference text while listening, and the analyser treats a blank row as heard
nothing (a failure), never as a missing row.
"""

from __future__ import annotations

import argparse
import csv
import json
import platform
import shutil
import subprocess
import sys
import tempfile
import wave
from pathlib import Path
from typing import Any, Final

SAMPLE_RATE_HZ: Final = 48_000
CHANNELS: Final = 1
SAMPLE_WIDTH_BYTES: Final = 2
DEFAULT_MANIFEST: Final = Path(__file__).resolve().parents[1] / "fixtures" / "phrases-v1.csv"
DEFAULT_PAUSE_MS: Final = 1_500
COLUMNS: Final = ("phrase_id", "kind", "reference", "nonce")

SAPI_SCRIPT: Final = r"""
param(
    [Parameter(Mandatory = $true)][string]$PlanPath
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Speech
$plan = Get-Content -Raw -LiteralPath $PlanPath | ConvertFrom-Json
$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    48000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono
)
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
if ($plan.voice) {
    $synth.SelectVoice($plan.voice)
}
foreach ($phrase in $plan.phrases) {
    $path = Join-Path $plan.out_dir ("{0}.wav" -f $phrase.id)
    $synth.SetOutputToWaveFile($path, $format)
    $synth.Speak($phrase.reference)
    $synth.SetOutputToNull()
}
$synth.Dispose()
"""


class PlaybackError(RuntimeError):
    """The phrase audio cannot be produced on this host."""


def load_phrases(path: Path) -> list[dict[str, str]]:
    """Read and validate the phrase manifest without importing the tools package."""

    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        missing = [column for column in COLUMNS if column not in (reader.fieldnames or ())]
        if missing:
            raise PlaybackError(f"{path}: missing columns {missing}")
        rows = [
            {
                "id": (row.get("phrase_id") or "").strip(),
                "kind": (row.get("kind") or "").strip(),
                "reference": (row.get("reference") or "").strip(),
                "nonce": (row.get("nonce") or "").strip(),
            }
            for row in reader
        ]

    if not rows:
        raise PlaybackError(f"{path}: the manifest has no phrases")
    if any(not row["id"] or not row["reference"] for row in rows):
        raise PlaybackError(f"{path}: every row needs a phrase_id and a reference")
    if rows[0]["kind"] != "anchor" or rows[-1]["kind"] != "anchor":
        raise PlaybackError(f"{path}: the first and last phrases must be the anchors")
    return rows


def build_transcript_template(phrases: list[dict[str, str]], *, run_id: str) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "observer": "",
        "instructions": (
            "Write exactly what you heard for every phrase. Leave the value empty only if you "
            "heard nothing; a blank row is recorded as a failure, not as missing data. Do not "
            "read the reference column while listening."
        ),
        "transcriptions": {phrase["id"]: "" for phrase in phrases},
    }


def build_playback_plan(
    phrases: list[dict[str, str]],
    *,
    manifest: Path,
    out_dir: Path,
    pause_ms: int,
) -> dict[str, Any]:
    return {
        "manifest": str(manifest),
        "phrase_count": len(phrases),
        "sample_rate_hz": SAMPLE_RATE_HZ,
        "channels": CHANNELS,
        "pause_ms": pause_ms,
        "audio_file": str(out_dir / "phrase_audio.wav"),
        "phrases": phrases,
    }


def concatenate_phrase_wavs(
    phrase_paths: list[Path],
    *,
    out: Path,
    pause_ms: int,
) -> None:
    """Join 48 kHz mono 16-bit phrase files with digital silence between them."""

    silence = b"\x00\x00" * int(SAMPLE_RATE_HZ * pause_ms / 1000)
    with wave.open(str(out), "wb") as output:
        output.setnchannels(CHANNELS)
        output.setsampwidth(SAMPLE_WIDTH_BYTES)
        output.setframerate(SAMPLE_RATE_HZ)
        for index, path in enumerate(phrase_paths):
            with wave.open(str(path), "rb") as source:
                if (
                    source.getnchannels() != CHANNELS
                    or source.getsampwidth() != SAMPLE_WIDTH_BYTES
                    or source.getframerate() != SAMPLE_RATE_HZ
                ):
                    raise PlaybackError(
                        f"{path}: expected 48 kHz mono 16-bit PCM, got "
                        f"{source.getframerate()} Hz / {source.getnchannels()} ch / "
                        f"{8 * source.getsampwidth()}-bit"
                    )
                output.writeframes(source.readframes(source.getnframes()))
            if index != len(phrase_paths) - 1:
                output.writeframes(silence)


def synthesise_phrases(
    phrases: list[dict[str, str]],
    *,
    out_dir: Path,
    pause_ms: int,
    voice: str | None = None,
) -> Path:
    """Synthesise the phrases with Windows SAPI and join them into one WAV."""

    if platform.system() != "Windows":
        raise PlaybackError(
            "phrase synthesis requires Windows SAPI. On this host use --dry-run to produce the "
            "playback plan and transcript template; run the synthesis on the reference runner."
        )
    powershell = shutil.which("powershell") or shutil.which("pwsh")
    if powershell is None:
        raise PlaybackError("PowerShell was not found; SAPI synthesis requires Windows PowerShell")

    phrase_dir = out_dir / "phrases"
    phrase_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="f0-audio-") as temporary:
        temporary_path = Path(temporary)
        script = temporary_path / "synth_phrases.ps1"
        script.write_text(SAPI_SCRIPT, encoding="utf-8")
        plan_path = temporary_path / "synth_plan.json"
        plan_path.write_text(
            json.dumps(
                {
                    "out_dir": str(phrase_dir),
                    "voice": voice or "",
                    "phrases": [
                        {"id": phrase["id"], "reference": phrase["reference"]} for phrase in phrases
                    ],
                },
                indent=2,
            ),
            encoding="utf-8",
        )
        completed = subprocess.run(  # noqa: S603 - fixed argument vector
            [
                powershell,
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(script),
                str(plan_path),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            raise PlaybackError(f"SAPI synthesis failed: {completed.stderr.strip()}")

    paths = [phrase_dir / f"{phrase['id']}.wav" for phrase in phrases]
    missing = [str(path) for path in paths if not path.exists()]
    if missing:
        raise PlaybackError(f"SAPI did not produce: {missing}")

    out = out_dir / "phrase_audio.wav"
    concatenate_phrase_wavs(paths, out=out, pause_ms=pause_ms)
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Render the scripted phrase manifest")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--run-id", default="aud-01")
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path("spikes/evidence/audio/aud-01"),
    )
    parser.add_argument("--pause-ms", type=int, default=DEFAULT_PAUSE_MS)
    parser.add_argument("--voice", default=None)
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="write the plan and transcript template without producing audio",
    )
    args = parser.parse_args(argv)

    if args.pause_ms < 0:
        print("error: --pause-ms must be non-negative", file=sys.stderr)
        return 2

    try:
        phrases = load_phrases(args.manifest)
        args.out_dir.mkdir(parents=True, exist_ok=True)
        plan = build_playback_plan(
            phrases,
            manifest=args.manifest,
            out_dir=args.out_dir,
            pause_ms=args.pause_ms,
        )
        (args.out_dir / "phrase_playback_manifest.json").write_text(
            json.dumps(plan, indent=2) + "\n", encoding="utf-8"
        )
        (args.out_dir / "transcriptions.json").write_text(
            json.dumps(build_transcript_template(phrases, run_id=args.run_id), indent=2) + "\n",
            encoding="utf-8",
        )
        if args.dry_run:
            print(
                f"wrote {args.out_dir / 'phrase_playback_manifest.json'} and "
                f"{args.out_dir / 'transcriptions.json'} (dry run; no audio produced)"
            )
            return 0
        audio = synthesise_phrases(
            phrases,
            out_dir=args.out_dir,
            pause_ms=args.pause_ms,
            voice=args.voice,
        )
    except (PlaybackError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    print(f"wrote {audio} and the transcript template in {args.out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


__all__ = [
    "DEFAULT_MANIFEST",
    "DEFAULT_PAUSE_MS",
    "PlaybackError",
    "SAMPLE_RATE_HZ",
    "build_playback_plan",
    "build_transcript_template",
    "concatenate_phrase_wavs",
    "load_phrases",
    "main",
    "synthesise_phrases",
]
