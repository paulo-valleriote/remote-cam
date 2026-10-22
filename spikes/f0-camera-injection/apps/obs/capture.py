"""OBS Studio capture harness for the injection gate (T036).

**NON-PRODUCTION** — F0 risk-reduction spike.

Drives OBS Studio through obs-websocket 5.7.x: selects the virtual camera as a
DirectShow device input, confirms OBS can actually see it and is receiving a
changing picture, records for the requested duration, stops, and records what
OBS itself reports about the output. Nothing about the camera is invented here:
the recording is the capture, decoded later by `finalise_run`, and OBS's own
frame counters are recorded because a screen recorder cannot expose per-frame
delivery any other way.

Usage::

    python apps/obs/capture.py --backend mf_virtual_camera --application-id obs \\
        --duration-s 1800 --output-dir <dir> \\
        --product-name "OBS Studio" --version 32.0.0 \\
        --obs-url ws://127.0.0.1:4455 --obs-password "$OBS_WEBSOCKET_PASSWORD" \\
        --device "RemoteCam Virtual Camera (Spike)"

Exit codes: 0 when a recording (or a recorded failure) was produced, 1 when OBS
could not be driven at all, so `run_matrix.py` never treats silence as evidence.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent))

from harness_common import (  # noqa: E402
    HarnessError,
    HarnessOptions,
    harness_parser,
    options_from,
    require_identity,
    write_capture_json,
)

from obs.wsclient import ObsClient, ObsWebSocketError  # noqa: E402

SOURCE_NAME = "RemoteCam virtual camera"
DEVICE_PROPERTY = "video_device_id"
DEFAULT_DEVICE = "RemoteCam Virtual Camera (Spike)"
FAILURE_JSON = "failure.json"


def parser() -> argparse.ArgumentParser:
    harness = harness_parser(__doc__, default_duration_s=1800)
    harness.add_argument(
        "--obs-url", default=os.environ.get("OBS_WEBSOCKET_URL", "ws://127.0.0.1:4455")
    )
    harness.add_argument(
        "--obs-password",
        default=os.environ.get("OBS_WEBSOCKET_PASSWORD", ""),
        help="required when OBS has websocket authentication enabled",
    )
    harness.add_argument("--device", default=DEFAULT_DEVICE, help="the camera name as OBS lists it")
    harness.add_argument(
        "--scene", default="", help="scene to add the camera to; defaults to the current one"
    )
    harness.add_argument(
        "--frame-check-s",
        type=int,
        default=10,
        help="how long to watch the source before recording starts",
    )
    harness.add_argument(
        "--keep-camera",
        action="store_true",
        help="leave the input in the scene instead of removing it afterwards",
    )
    return harness


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    options = options_from(args)
    require_identity(options)
    options.output_dir.mkdir(parents=True, exist_ok=True)

    try:
        return run(args, options)
    except (HarnessError, ObsWebSocketError) as error:
        # A camera OBS cannot see is a matrix failure with its own outcome, so it
        # is recorded rather than raised: the matrix must show the failing cell.
        message = str(error)
        if "not visible" in message or "no frame" in message or "empty capture" in message:
            record_failure(options, message)
            print(f"camera not visible to OBS: {message}")
            return 0
        print(f"error: {message}", file=sys.stderr)
        return 1


def run(args: argparse.Namespace, options: HarnessOptions) -> int:
    password = args.obs_password or None
    with ObsClient(url=args.obs_url, password=password) as client:
        version = client.request("GetVersion")
        print(
            f"OBS {version.get('obsVersion')} via "
            f"obs-websocket {version.get('obsWebSocketVersion')}"
        )

        scene = args.scene or client.request("GetCurrentProgramScene")["currentProgramSceneName"]
        source = ensure_camera_source(client, scene, args.device)
        print(f"scene {scene!r} uses {source!r}")

        # A recording that never receives a picture proves nothing, so the
        # picture is confirmed to be changing before the measurement starts.
        fingerprint = watch_for_frames(client, args.frame_check_s)

        stats_before = client.request("GetStats")
        client.request("StartRecord")
        started = time.monotonic()
        print(f"recording for {options.duration_s} s")

        remaining = float(options.duration_s)
        while remaining > 0:
            time.sleep(min(30.0, remaining))
            remaining -= 30.0
            status = client.request("GetRecordStatus")
            elapsed = time.monotonic() - started
            print(
                f"  {elapsed:.0f}s  bytes={status.get('outputBytes')}  "
                f"paused={status.get('outputPaused')}",
                flush=True,
            )

        stopped = client.request("StopRecord")
        recording_path = Path(str(stopped.get("outputPath", "")))
        elapsed = time.monotonic() - started
        stats_after = client.request("GetStats")

        if not args.keep_camera:
            # Removing the input keeps the next cell's scene clean; a stale
            # camera input is itself a way to accidentally measure nothing.
            client.request("RemoveInput", {"inputName": SOURCE_NAME})

    if not recording_path or not recording_path.is_file():
        raise HarnessError(f"OBS reported {recording_path!r} but no file exists there")

    delivered: dict[str, Any] = {
        "os_byte_fingerprint_at_start": fingerprint,
        "output_total_frames_delta": _delta(stats_before, stats_after, "outputTotalFrames"),
        "output_skipped_frames_delta": _delta(stats_before, stats_after, "outputSkippedFrames"),
        "render_total_frames_delta": _delta(stats_before, stats_after, "renderTotalFrames"),
        "render_skipped_frames_delta": _delta(stats_before, stats_after, "renderSkippedFrames"),
        "obs_version": version.get("obsVersion"),
        "obs_websocket_version": version.get("obsWebSocketVersion"),
        "record_ms_reported": stopped.get("outputDuration"),
        "record_seconds_measured": round(elapsed, 3),
        "source_name": source,
        "source_removed_after_run": not args.keep_camera,
    }
    reported_ms = int(stopped.get("outputDuration") or 0)
    if reported_ms and abs(reported_ms - options.duration_s * 1000) > 60_000:
        # A recording that stopped early cannot support the 1800 s criterion, so
        # the difference is recorded instead of reporting the requested length.
        delivered["duration_mismatch_ms"] = reported_ms - options.duration_s * 1000

    capture_file = f"capture{recording_path.suffix or '.mkv'}"
    target = options.output_dir / capture_file
    if recording_path.resolve() != target.resolve():
        target.write_bytes(recording_path.read_bytes())

    notes = options.extra.get("notes", "")
    skipped = delivered["output_skipped_frames_delta"]
    if skipped:
        notes = "; ".join(filter(None, [notes, f"OBS skipped {skipped} output frames"]))

    write_capture_json(
        options,
        capture_file=capture_file,
        capture_method="obs_websocket",
        recorded_duration_s=max(elapsed, reported_ms / 1000.0),
        bitness="64_bit",
        discarded_frames=int(skipped or 0),
        observed=delivered,
    )
    print(f"recording copied to {target} ({target.stat().st_size / 1e6:.1f} MB)")
    return 0


def ensure_camera_source(client: ObsClient, scene: str, device: str) -> str:
    """Add or update the camera input, refusing when OBS does not list the device.

    `GetInputPropertiesListPropertyItems` is the authoritative answer to "does
    this application see the camera?": it is the same list OBS shows in its own
    properties dialog.
    """

    devices = client.request(
        "GetInputPropertiesListPropertyItems",
        {"inputName": SOURCE_NAME, "propertyName": DEVICE_PROPERTY},
    ).get("propertyItems", [])

    if not devices:
        devices = _devices_from_a_temporary_input(client, scene, device)

    names = [str(item.get("itemName", "")) for item in devices]
    match = next((item for item in devices if str(item.get("itemValue", "")) == device), None)
    match = match or next(
        (item for item in devices if device.lower() in str(item.get("itemName", "")).lower()), None
    )
    if match is None:
        raise HarnessError(
            f"the camera {device!r} is not visible to OBS; it lists: "
            + ", ".join(sorted(names) or ["(no camera at all)"])
        )

    inputs = client.request("GetInputList", {"inputKind": "dshow_input"}).get("inputs", [])
    if not any(item["inputName"] == SOURCE_NAME for item in inputs):
        client.request(
            "CreateInput",
            {
                "sceneName": scene,
                "inputName": SOURCE_NAME,
                "inputKind": "dshow_input",
                "inputSettings": {DEVICE_PROPERTY: match["itemValue"]},
                "sceneItemEnabled": True,
            },
        )
    else:
        client.request(
            "SetInputSettings",
            {"inputName": SOURCE_NAME, "inputSettings": {DEVICE_PROPERTY: match["itemValue"]}},
        )
    return str(match.get("itemName", match["itemValue"]))


def _devices_from_a_temporary_input(
    client: ObsClient, scene: str, device: str
) -> list[dict[str, Any]]:
    """Create a throwaway input to read the device list when none exists yet."""

    temporary = f"{SOURCE_NAME} (probe)"
    client.request(
        "CreateInput",
        {
            "sceneName": scene,
            "inputName": temporary,
            "inputKind": "dshow_input",
            "inputSettings": {},
            "sceneItemEnabled": False,
        },
    )
    try:
        return client.request(
            "GetInputPropertiesListPropertyItems",
            {"inputName": temporary, "propertyName": DEVICE_PROPERTY},
        ).get("propertyItems", [])
    finally:
        client.request("RemoveInput", {"inputName": temporary})


def watch_for_frames(client: ObsClient, seconds: int) -> str:
    """Confirm the source picture changes, then return a byte fingerprint.

    Two screenshots taken `seconds` apart must differ: a source that OBS can
    list but that never delivers a frame would otherwise be recorded as a
    30-minute run with an empty picture, which is exactly the failure the gate
    must not hide.
    """
    first = client.request("GetSourceScreenshot", _screenshot_request(SOURCE_NAME))["imageData"]
    time.sleep(max(1, seconds))
    second = client.request("GetSourceScreenshot", _screenshot_request(SOURCE_NAME))["imageData"]
    if first == second:
        raise HarnessError(
            f"no frame reached OBS from {SOURCE_NAME!r}: two screenshots {seconds} s apart are "
            "identical, so recording the source would produce an empty capture"
        )
    return hashlib.sha256(base64.b64decode(second)).hexdigest()


def _screenshot_request(source: str) -> dict[str, Any]:
    return {"sourceName": source, "imageFormat": "png", "imageWidth": 320, "imageHeight": 180}


def _delta(before: dict[str, Any], after: dict[str, Any], key: str) -> int | None:
    if key not in before or key not in after:
        return None
    return int(after[key]) - int(before[key])


def record_failure(options: HarnessOptions, reason: str) -> None:
    failure = {
        "application_id": options.application_id,
        "product_name": options.product_name,
        "version": options.version,
        "reason": reason,
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%S+00:00", time.gmtime()),
    }
    (options.output_dir / FAILURE_JSON).write_text(
        json.dumps(failure, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    raise SystemExit(main())
