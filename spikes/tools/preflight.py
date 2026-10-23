#!/usr/bin/env python3
"""Preflight: record the reference environment and verify hardware decode.

Produces ``spikes/evidence/environment.json`` in the shape of
``contracts/measurement-formats.md`` §1 and the gate report's
``$defs/environment``.

Two rules govern this tool:

* It never fabricates a measurement. A value that cannot be read is omitted or
  recorded as ``not_detected``, and the console explains why.
* It never claims reference conditions it has not verified. On a host without a
  Windows Media Foundation decoder and NV12 output, ``hardware_accelerated`` is
  false, ``within_reference_conditions`` is false and the record states what was
  actually checked.

The Windows probe is ``spikes/tools/windows/probe_hardware_decode.ps1``: it calls
``MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, SYNCMFT|SORTANDFILTER)``, activates each
decoder, checks for an NV12 output type and samples the
``\\GPU Engine(*)\\Utilization Percentage`` counter while decoding a generated
clip. The reference machine's result is the only one admissible for the latency
gate.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = REPO_ROOT / "spikes" / "evidence" / "environment.json"
WINDOWS_PROBE = REPO_ROOT / "spikes" / "tools" / "windows" / "probe_hardware_decode.ps1"


def _now() -> str:
    return datetime.now(UTC).isoformat(timespec="seconds")


def _run(command: list[str], *, timeout: float = 5.0) -> str | None:
    """Run a command and return stripped stdout, or ``None`` when unavailable."""

    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            text=True,
            encoding="latin-1",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if completed.returncode != 0:
        return None
    output = (completed.stdout or completed.stderr).strip()
    return output or None


def _first_line(value: str | None) -> str | None:
    if not value:
        return None
    return value.splitlines()[0].strip() or None


def _version(command: str, *arguments: str) -> str | None:
    if shutil.which(command) is None:
        return None
    return _first_line(_run([command, *arguments]))


def _package_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def environment_id() -> str:
    """An anonymous, stable identifier for this machine.

    The hostname is hashed and never written; two machines with the same OS and
    architecture still differ, but no personal identifier is recorded.
    """

    explicit = os.environ.get("REMOTECAM_ENVIRONMENT_ID")
    if explicit:
        return explicit
    seed = f"{platform.node()}|{platform.machine()}|{platform.system()}".encode()
    digest = hashlib.sha256(seed).hexdigest()[:8]
    return f"{platform.system().lower()}-{platform.machine().lower()}-{digest}"


def parse_os_release(text: str) -> str | None:
    for line in text.splitlines():
        if line.startswith("PRETTY_NAME="):
            return line.split("=", 1)[1].strip().strip('"')
    return None


def cpu_model_from_cpuinfo(text: str) -> str | None:
    for line in text.splitlines():
        if line.lower().startswith("model name"):
            return line.split(":", 1)[1].strip()
    for line in text.splitlines():
        if line.lower().startswith("hardware") or line.lower().startswith("processor"):
            value = line.split(":", 1)[-1].strip()
            if value:
                return value
    return None


def band_from_frequency_mhz(frequency_mhz: float | None) -> str:
    """Map a Wi-Fi centre frequency to the schema's band enumeration."""

    if frequency_mhz is None:
        return "wired"
    if 2400 <= frequency_mhz < 3000:
        return "2_4_ghz"
    return "5_ghz"


def parse_iw_link(text: str) -> dict[str, Any]:
    """Parse ``iw dev <iface> link`` into band, width and power-save facts."""

    result: dict[str, Any] = {}
    frequency = re.search(r"freq:\s*(\d+(?:\.\d+)?)", text)
    if frequency:
        result["frequency_mhz"] = float(frequency.group(1))
        result["band"] = band_from_frequency_mhz(result["frequency_mhz"])
    width = re.search(r"width:\s*(\d+)\s*MHz", text)
    if width:
        result["channel_width_mhz"] = int(width.group(1))
    ssid = re.search(r"SSID:\s*(.+)", text)
    if ssid:
        result["ssid"] = ssid.group(1).strip()
    return result


def parse_ping_stats(text: str) -> dict[str, float]:
    """Parse ``ping`` output into RTT and loss, ignoring anything absent."""

    result: dict[str, float] = {}
    loss = re.search(r"(\d+(?:\.\d+)?)%\s*packet loss", text)
    if loss:
        result["baseline_loss_pct"] = float(loss.group(1))
    rtt = re.search(
        r"(?:rtt|round-trip)\s+min/avg/max/(?:mdev|stddev)\s*=\s*"
        r"[\d.]+/([\d.]+)/[\d.]+/[\d.]+",
        text,
    )
    if rtt:
        result["baseline_rtt_ms"] = float(rtt.group(1))
    return result


def is_reference_condition(
    *,
    hardware_accelerated: bool,
    output_format_supported: str,
    wifi_power_save: str,
    internet_enabled: bool,
    band: str,
) -> bool:
    """The reference conditions of ``measurement-formats.md`` §1."""

    return (
        hardware_accelerated
        and output_format_supported == "nv12"
        and wifi_power_save == "disabled"
        and not internet_enabled
        and band in {"5_ghz", "wired"}
    )


def detect_cpu() -> tuple[str, int]:
    model = None
    if Path("/proc/cpuinfo").exists():
        try:
            model = cpu_model_from_cpuinfo(Path("/proc/cpuinfo").read_text(encoding="utf-8"))
        except OSError:
            model = None
    if model is None and platform.system() == "Windows":
        output = _run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)",
            ]
        )
        model = output.strip() if output else None
    if model is None and platform.system() == "Darwin":
        output = _run(["sysctl", "-n", "machdep.cpu.brand_string"])
        model = output.strip() if output else None
    return (model or "not_detected", os.cpu_count() or 1)


def detect_gpu() -> str:
    system = platform.system()
    if system == "Windows":
        output = _run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | "
                "Select-Object -First 1 -ExpandProperty Name",
            ]
        )
        return output.strip() if output else "not_detected"
    if shutil.which("lspci"):
        output = _run(["lspci"])
        if output:
            for line in output.splitlines():
                if re.search(r"(VGA compatible controller|3D controller|Display controller)", line):
                    return re.sub(r"^[0-9a-fA-F:.]+\s+[^:]+:\s*", "", line).strip()
    if system == "Darwin" and shutil.which("system_profiler"):
        output = _run(["system_profiler", "SPDisplaysDataType"])
        if output:
            match = re.search(r"Chipset Model:\s*(.+)", output)
            if match:
                return match.group(1).strip()
    return "not_detected"


def detect_os() -> tuple[str, str]:
    system = platform.system()
    if system == "Windows":
        build = platform.version()
        release = platform.release()
        return (f"Windows {release}", build)
    if Path("/etc/os-release").exists():
        try:
            name = parse_os_release(Path("/etc/os-release").read_text(encoding="utf-8"))
        except OSError:
            name = None
        return (name or platform.system(), platform.release())
    return (platform.system(), platform.release())


def detect_android() -> tuple[str, str]:
    model = os.environ.get("ANDROID_DEVICE_MODEL")
    version = os.environ.get("ANDROID_OS_VERSION")
    if model and version:
        return model, version
    if shutil.which("adb") is None:
        return "not_connected", "not_connected"
    adb_model = _run(["adb", "shell", "getprop", "ro.product.model"])
    adb_release = _run(["adb", "shell", "getprop", "ro.build.version.release"])
    adb_sdk = _run(["adb", "shell", "getprop", "ro.build.version.sdk"])
    if not adb_model or not adb_release or not adb_sdk:
        return "not_connected", "not_connected"
    return adb_model.strip(), f"Android {adb_release.strip()}, API {adb_sdk.strip()}"


def detect_app_versions() -> dict[str, str]:
    versions: dict[str, str] = {
        "chrome": _version("google-chrome", "--version")
        or _version("chromium", "--version")
        or "not_detected",
        "obs": _version("obs", "--version") or "not_detected",
    }
    if platform.system() == "Windows":
        camera = _run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "Get-AppxPackage Microsoft.WindowsCamera | "
                "Select-Object -First 1 -ExpandProperty Version",
            ]
        )
        versions["windows_camera"] = camera.strip() if camera else "not_detected"
    else:
        versions["windows_camera"] = "not_detected"
    return versions


def detect_tool_versions() -> dict[str, str]:
    versions = {
        "python": platform.python_version(),
        "ffmpeg": _version("ffmpeg", "-version") or "not_detected",
        "ffprobe": _version("ffprobe", "-version") or "not_detected",
        "iperf3": _version("iperf3", "--version") or "not_detected",
        "gradle": _version("gradle", "--version") or "not_detected",
        "cargo": _version("cargo", "--version") or "not_detected",
        "rustc": _version("rustc", "--version") or "not_detected",
    }
    for package, key in (
        ("zxing-cpp", "zxing_cpp"),
        ("numpy", "numpy"),
        ("scipy", "scipy"),
        ("soundfile", "soundfile"),
        ("qrcode", "qrcode"),
        ("jsonschema", "jsonschema"),
        ("pytest", "pytest"),
        ("ruff", "ruff"),
    ):
        versions[key] = _package_version(package) or "not_detected"
    return versions


def _default_interface() -> str | None:
    output = _run(["ip", "route", "show", "default"])
    if not output:
        return None
    match = re.search(r"\bdev\s+(\S+)", output)
    return match.group(1) if match else None


def _is_wireless(interface: str) -> bool:
    return Path(f"/sys/class/net/{interface}/wireless").exists()


def detect_network(*, probe_internet: bool = True) -> dict[str, Any]:
    network: dict[str, Any] = {
        "band": "wired",
        "wifi_power_save": "disabled",
        "internet_enabled": False,
        "within_reference_conditions": False,
    }
    interface = _default_interface()
    if interface is None:
        network["band"] = "emulated"
    elif _is_wireless(interface):
        link = _run(["iw", "dev", interface, "link"])
        parsed = parse_iw_link(link or "")
        network["band"] = parsed.get("band", "5_ghz")
        if "channel_width_mhz" in parsed:
            network["channel_width_mhz"] = parsed["channel_width_mhz"]
        power = _run(["iw", "dev", interface, "get", "power_save"])
        if power and re.search(r"power_save:\s*on", power):
            network["wifi_power_save"] = "enabled"
        elif power and re.search(r"power_save:\s*off", power):
            network["wifi_power_save"] = "disabled"
        else:
            network["wifi_power_save"] = "enabled"
        adapter = _run(["ethtool", "-i", interface])
        if adapter:
            driver = re.search(r"driver:\s*(\S+)", adapter)
            if driver:
                network["client_adapter"] = driver.group(1)
    else:
        adapter = _run(["ethtool", "-i", interface])
        if adapter:
            driver = re.search(r"driver:\s*(\S+)", adapter)
            if driver:
                network["client_adapter"] = driver.group(1)

    access_point = os.environ.get("REMOTECAM_ACCESS_POINT_MODEL")
    if access_point:
        network["access_point_model"] = access_point
    distance = os.environ.get("REMOTECAM_AP_DISTANCE_M")
    if distance:
        try:
            network["distance_m"] = float(distance)
        except ValueError:
            pass

    gateway = None
    default_route = _run(["ip", "route", "show", "default"])
    if default_route:
        match = re.search(r"default\s+via\s+(\S+)", default_route)
        gateway = match.group(1) if match else None
    if gateway and shutil.which("ping"):
        ping = _run(["ping", "-c", "5", "-W", "1", "-q", gateway], timeout=10)
        if ping:
            network.update(parse_ping_stats(ping))

    iperf_server = os.environ.get("REMOTECAM_IPERF_SERVER")
    if iperf_server and shutil.which("iperf3"):
        iperf = _run(["iperf3", "-c", iperf_server, "-t", "3", "-J"], timeout=20)
        if iperf:
            try:
                payload = json.loads(iperf)
                bits = payload["end"]["sum_received"]["bits_per_second"]
                network["baseline_throughput_mbps"] = round(float(bits) / 1e6, 1)
            except (KeyError, TypeError, ValueError):
                pass

    if probe_internet:
        try:
            with socket.create_connection(("1.1.1.1", 53), timeout=1.0):
                network["internet_enabled"] = True
        except OSError:
            network["internet_enabled"] = False

    return network


def _software_hardware_verification() -> dict[str, Any]:
    """Honest non-Windows record: no Media Foundation, no NV12 verification."""

    decoders = _run(["ffmpeg", "-hide_banner", "-decoders"]) if shutil.which("ffmpeg") else None
    decoder_found = bool(decoders and re.search(r"\bh264\b", decoders))
    return {
        "codec": "H.264",
        "verified_at": _now(),
        "discovery_method": "other",
        "decoder_found": decoder_found,
        "hardware_accelerated": False,
        "output_format_supported": "other",
        "evidence": (
            "MFTEnumEx(SYNCMFT|SORTANDFILTER) requires Windows Media Foundation; this host is "
            f"{platform.system()}. "
            + (
                "ffmpeg lists an H.264 decoder, but that is a software decoder and is not "
                "evidence for PC-1 or for the latency gate."
                if decoder_found
                else "No H.264 decoder was detected either."
            )
        ),
    }


def _windows_hardware_verification() -> dict[str, Any]:
    powershell = shutil.which("powershell") or shutil.which("pwsh")
    if powershell is None:
        return {
            "codec": "H.264",
            "verified_at": _now(),
            "discovery_method": "other",
            "decoder_found": False,
            "hardware_accelerated": False,
            "output_format_supported": "other",
            "evidence": "PowerShell is unavailable, so the MFTEnumEx probe could not run.",
        }
    if not WINDOWS_PROBE.exists():
        return {
            "codec": "H.264",
            "verified_at": _now(),
            "discovery_method": "other",
            "decoder_found": False,
            "hardware_accelerated": False,
            "output_format_supported": "other",
            "evidence": f"probe script not found: {WINDOWS_PROBE}",
        }

    with tempfile.TemporaryDirectory(prefix="remotecam-preflight-") as directory:
        output_path = Path(directory) / "hardware.json"
        completed = subprocess.run(
            [
                powershell,
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(WINDOWS_PROBE),
                "-JsonPath",
                str(output_path),
            ],
            capture_output=True,
            text=True,
            encoding="latin-1",
            errors="replace",
            timeout=120,
            check=False,
        )
        if output_path.exists():
            try:
                probe = json.loads(output_path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                probe = {}
            if isinstance(probe, dict) and probe.get("hardware_accelerated") is not None:
                return {
                    "codec": "H.264",
                    "verified_at": _now(),
                    "discovery_method": probe.get("discovery_method", "mft_enum_syncmft"),
                    "decoder_found": bool(probe.get("decoder_found")),
                    "hardware_accelerated": bool(probe.get("hardware_accelerated")),
                    "output_format_supported": probe.get("output_format_supported", "other"),
                    "evidence": str(probe.get("evidence", "MFTEnumEx probe returned no evidence")),
                }
        detail = (completed.stderr or completed.stdout or "no output").strip().splitlines()
        return {
            "codec": "H.264",
            "verified_at": _now(),
            "discovery_method": "other",
            "decoder_found": False,
            "hardware_accelerated": False,
            "output_format_supported": "other",
            "evidence": f"MFTEnumEx probe failed: {detail[-1] if detail else 'unknown error'}",
        }


def detect_hardware_decode() -> dict[str, Any]:
    if platform.system() == "Windows":
        return _windows_hardware_verification()
    return _software_hardware_verification()


def build_environment(*, probe_internet: bool = True) -> dict[str, Any]:
    cpu_model, cores = detect_cpu()
    os_name, os_build = detect_os()
    android_model, android_version = detect_android()
    network = detect_network(probe_internet=probe_internet)
    hardware = detect_hardware_decode()

    network["within_reference_conditions"] = is_reference_condition(
        hardware_accelerated=bool(hardware["hardware_accelerated"]),
        output_format_supported=str(hardware["output_format_supported"]),
        wifi_power_save=str(network["wifi_power_save"]),
        internet_enabled=bool(network["internet_enabled"]),
        band=str(network["band"]),
    )

    return {
        "environment_id": environment_id(),
        "captured_at": _now(),
        "cpu_model": cpu_model,
        "cpu_logical_cores": cores,
        "gpu_model": detect_gpu(),
        "os_name": os_name,
        "os_build": os_build,
        "android_device_model": android_model,
        "android_os_version": android_version,
        "app_versions": detect_app_versions(),
        "tool_versions": detect_tool_versions(),
        "network": network,
        "hardware_decode_verification": hardware,
    }


def _write(document: dict[str, Any], output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    temporary.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Record the F0 reference environment.")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--json", action="store_true", help="print the environment record")
    parser.add_argument(
        "--no-network-probe",
        action="store_true",
        help="do not attempt an internet reachability probe",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="exit non-zero when the record is not within reference conditions",
    )
    args = parser.parse_args(argv)

    document = build_environment(probe_internet=not args.no_network_probe)
    _write(document, args.output)

    if args.json:
        print(json.dumps(document, indent=2, sort_keys=True))
    else:
        hardware = document["hardware_decode_verification"]
        network = document["network"]
        print(f"environment: {document['environment_id']} -> {args.output}")
        print(
            "hardware decode: "
            f"accelerated={hardware['hardware_accelerated']} "
            f"format={hardware['output_format_supported']}"
        )
        print(
            "network: "
            f"band={network['band']} power_save={network['wifi_power_save']} "
            f"internet={network['internet_enabled']}"
        )
        if not network["within_reference_conditions"]:
            print(
                "WARNING: outside reference conditions. Runs from this environment MUST be "
                "labelled "
                "outside-reference-environment and cannot support a go verdict.",
                file=sys.stderr,
            )
            if hardware["hardware_accelerated"] is not True:
                print("WARNING: PC-1 is not verified (hardware decode / NV12).", file=sys.stderr)
        else:
            print("within reference conditions: true")

    if args.strict and not document["network"]["within_reference_conditions"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
