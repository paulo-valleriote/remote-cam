"""Window-capture harness for the injection gate (T037).

**NON-PRODUCTION** — F0 risk-reduction spike.

The gate needs three applications that have no scripting interface the spike can
use: the team's conferencing client, the inbox Windows Camera app and the team's
screen recorder. This harness observes them from the outside:

* ``gdigrab`` — FFmpeg captures the application window for the requested
  duration while its preview shows the virtual camera (conferencing client,
  Windows Camera);
* ``app_native_recording`` — the application records itself and the operator
  points the harness at the file it produced (screen recorder), so the evidence
  is the application's own output rather than a re-encoding of its window.

The exact product and version are read from the running executable or supplied
on the command line, and are recorded verbatim: "the conferencing client" is not
gate evidence, "Contoso Meet 7.0.0 (contoso-meet.exe)" is.

Usage::

    python apps/window_capture.py --backend mf_virtual_camera \\
        --application-id conferencing --duration-s 1800 --output-dir <dir> \\
        --product-name "Contoso Meet" --version 7.0.0 \\
        --window-title "Contoso Meet"

    python apps/window_capture.py --backend mf_virtual_camera \\
        --application-id screen_recorder --duration-s 1800 --output-dir <dir> \\
        --capture-method app_native_recording --capture-file "%USERPROFILE%\\Videos\\rec.mp4"

Exit codes: 0 when a capture (or a recorded failure) exists, 1 when nothing was
produced, so `run_matrix.py` never treats silence as evidence.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from harness_common import (  # noqa: E402
    HarnessError,
    HarnessOptions,
    harness_parser,
    options_from,
    require_identity,
    write_capture_json,
)

FAILURE_JSON = "failure.json"
CAPTURE_METHODS = ("gdigrab_window_capture", "app_native_recording")


def parser() -> argparse.ArgumentParser:
    harness = harness_parser(__doc__)
    harness.add_argument("--capture-method", default=None, choices=CAPTURE_METHODS)
    harness.add_argument("--window-title", default="", help="the window FFmpeg captures")
    harness.add_argument(
        "--capture-file",
        default="",
        help="an existing recording, for --capture-method app_native_recording",
    )
    harness.add_argument("--ffmpeg", default="ffmpeg")
    harness.add_argument("--fps", type=int, default=30)
    harness.add_argument(
        "--exe",
        default="",
        help="the application executable, so its file version can be read",
    )
    return harness


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    args.product_name, args.version = detect_identity(args)
    options = options_from(args)
    options.output_dir.mkdir(parents=True, exist_ok=True)

    try:
        require_identity(options)
        return run(args, options)
    except HarnessError as error:
        if args.product_name and args.version:
            record_failure(args, options, str(error))
            print(f"camera not visible: {error}")
            return 0
        print(f"error: {error}", file=sys.stderr)
        return 1


def run(args: argparse.Namespace, options: HarnessOptions) -> int:
    method = args.capture_method or (
        "app_native_recording" if args.capture_file else "gdigrab_window_capture"
    )

    if method == "app_native_recording":
        source = Path(args.capture_file).expanduser()
        if not source.is_file():
            raise HarnessError(
                f"{source} does not exist: the application's own recording is the evidence, "
                "and a missing file means the cell has no capture"
            )
        suffix = source.suffix or ".mp4"
        target = options.output_dir / f"capture{suffix}"
        if source.resolve() != target.resolve():
            shutil.copyfile(source, target)
        recorded_s = float(args.duration_s)
    else:
        if not args.window_title:
            raise HarnessError(
                "--window-title is required: the window whose preview shows the camera is the "
                "consumer being measured"
            )
        if shutil.which(args.ffmpeg) is None:
            raise HarnessError(
                f"{args.ffmpeg} is required for window capture; install FFmpeg or record with "
                "the application itself and pass --capture-file"
            )
        target = options.output_dir / "capture.mp4"
        recorded_s = gdigrab(args, target)

    write_capture_json(
        options,
        capture_file=target.name,
        capture_method=method,
        recorded_duration_s=recorded_s,
        bitness="64_bit",
        observed={
            "harness": "gdigrab_window_capture"
            if method == "gdigrab_window_capture"
            else "app_native_recording",
            "window_title": args.window_title or None,
            "executable": args.exe or None,
            "detected_version": args.version,
        },
    )
    print(f"capture written to {target} ({target.stat().st_size / 1e6:.1f} MB)")
    return 0


def gdigrab(args: argparse.Namespace, target: Path) -> float:
    """Capture one window with FFmpeg for the requested duration."""

    started = time.monotonic()
    completed = subprocess.run(
        [
            args.ffmpeg,
            "-hide_banner",
            "-v",
            "warning",
            "-f",
            "gdigrab",
            "-framerate",
            str(args.fps),
            "-i",
            f"title={args.window_title}",
            "-t",
            str(args.duration_s),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-y",
            str(target),
        ],
        capture_output=True,
        text=True,
    )
    elapsed = time.monotonic() - started
    if completed.returncode != 0:
        message = (completed.stderr or "").strip().splitlines()
        detail = message[-1] if message else f"ffmpeg exited with {completed.returncode}"
        raise HarnessError(
            f"gdigrab could not capture the window {args.window_title!r} ({detail}); "
            "a window that cannot be captured is a failed cell, not a passed one"
        )
    if not target.is_file() or target.stat().st_size == 0:
        raise HarnessError(f"ffmpeg produced no capture for {args.window_title!r}")
    return elapsed


def detect_identity(args: argparse.Namespace) -> tuple[str, str]:
    """Fill in the product name and version from the executable when possible."""

    product, version = args.product_name.strip(), args.version.strip()
    if product and version:
        return product, version
    if not args.exe:
        return product, version

    executable = Path(args.exe)
    info = executable_version(str(executable))
    return (
        product or str(info.get("ProductName") or info.get("FileDescription") or executable.name),
        version or str(info.get("FileVersion") or info.get("ProductVersion") or ""),
    )


def executable_version(path: str) -> dict[str, str]:
    """Read the Windows file-version resource with PowerShell."""

    if shutil.which("powershell") is None:
        return {}
    script = (
        f"$i = (Get-Item -LiteralPath '{path}').VersionInfo; "
        "@{ProductName=$i.ProductName; FileVersion=$i.FileVersion; "
        "ProductVersion=$i.ProductVersion; FileDescription=$i.FileDescription} | ConvertTo-Json"
    )
    completed = subprocess.run(
        ["powershell", "-NoProfile", "-Command", script],
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        return {}
    return {key: str(value) for key, value in json.loads(completed.stdout).items()}


def record_failure(args: argparse.Namespace, options: HarnessOptions, reason: str) -> None:
    failure = {
        "application_id": options.application_id,
        "product_name": args.product_name,
        "version": args.version,
        "reason": reason,
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%S+00:00", time.gmtime()),
    }
    (options.output_dir / FAILURE_JSON).write_text(
        json.dumps(failure, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    raise SystemExit(main())
