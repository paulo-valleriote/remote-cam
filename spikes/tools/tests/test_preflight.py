from __future__ import annotations

from pathlib import Path

import pytest

import preflight
from preflight import (
    band_from_frequency_mhz,
    build_environment,
    cpu_model_from_cpuinfo,
    detect_tool_versions,
    environment_id,
    is_reference_condition,
    parse_iw_link,
    parse_os_release,
    parse_ping_stats,
)
from validation.validate_evidence import ENVIRONMENT, validate_document


def test_parse_iw_link_reads_band_and_width() -> None:
    text = """Connected to aa:bb:cc:dd:ee:ff (on wlp3s0)
\tSSID: TestNet
\tfreq: 5180.0
\trx bitrate: 866.7 MBit/s
\tchannel width: 80 MHz
"""

    parsed = parse_iw_link(text)

    assert parsed["frequency_mhz"] == 5180.0
    assert parsed["band"] == "5_ghz"
    assert parsed["channel_width_mhz"] == 80
    assert parsed["ssid"] == "TestNet"


@pytest.mark.parametrize(
    "frequency, band",
    [(2412.0, "2_4_ghz"), (2437.0, "2_4_ghz"), (5180.0, "5_ghz"), (5745.0, "5_ghz")],
)
def test_band_from_frequency(frequency: float, band: str) -> None:
    assert band_from_frequency_mhz(frequency) == band


def test_band_from_frequency_without_a_wireless_reading() -> None:
    assert band_from_frequency_mhz(None) == "wired"


def test_parse_ping_stats() -> None:
    text = """PING 192.168.1.1 (192.168.1.1) 56(84) bytes of data.

--- 192.168.1.1 ping statistics ---
5 packets transmitted, 5 received, 0% packet loss, time 4005ms
rtt min/avg/max/mdev = 2.914/3.123/3.401/0.180 ms
"""

    assert parse_ping_stats(text) == {
        "baseline_loss_pct": 0.0,
        "baseline_rtt_ms": 3.123,
    }


def test_parse_ping_stats_with_partial_loss() -> None:
    text = "5 packets transmitted, 4 received, 20% packet loss, time 4005ms"

    assert parse_ping_stats(text) == {"baseline_loss_pct": 20.0}


def test_parse_os_release() -> None:
    text = 'NAME="Ubuntu"\nPRETTY_NAME="Ubuntu 24.04.3 LTS"\nVERSION_ID="24.04"\n'

    assert parse_os_release(text) == "Ubuntu 24.04.3 LTS"


def test_cpu_model_from_cpuinfo() -> None:
    text = "processor\t: 0\nmodel name\t: AMD Ryzen 7 5800X 8-Core Processor\n"

    assert cpu_model_from_cpuinfo(text) == "AMD Ryzen 7 5800X 8-Core Processor"


def test_reference_condition_requires_every_fact() -> None:
    base = {
        "hardware_accelerated": True,
        "output_format_supported": "nv12",
        "wifi_power_save": "disabled",
        "internet_enabled": False,
        "band": "5_ghz",
    }

    assert is_reference_condition(**base) is True
    assert is_reference_condition(**{**base, "hardware_accelerated": False}) is False
    assert is_reference_condition(**{**base, "output_format_supported": "i420"}) is False
    assert is_reference_condition(**{**base, "wifi_power_save": "enabled"}) is False
    assert is_reference_condition(**{**base, "internet_enabled": True}) is False
    assert is_reference_condition(**{**base, "band": "2_4_ghz"}) is False


def test_environment_id_does_not_leak_the_hostname(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("REMOTECAM_ENVIRONMENT_ID", raising=False)
    monkeypatch.setattr(preflight.platform, "node", lambda: "secret-workstation")

    identifier = environment_id()

    assert "secret-workstation" not in identifier
    assert identifier.startswith("linux-")


def test_environment_id_honours_the_explicit_override(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("REMOTECAM_ENVIRONMENT_ID", "ref-win11-01")

    assert environment_id() == "ref-win11-01"


def test_tool_versions_include_the_measurement_stack() -> None:
    versions = detect_tool_versions()

    assert versions["python"]
    for key in ("ffmpeg", "zxing_cpp", "numpy", "jsonschema", "pytest"):
        assert key in versions


def test_software_hardware_verification_never_claims_acceleration() -> None:
    verification = preflight.detect_hardware_decode()

    assert verification["codec"] == "H.264"
    assert verification["hardware_accelerated"] is False
    assert (
        verification["output_format_supported"] != "nv12" or verification["decoder_found"] is False
    )
    assert verification["evidence"]


def test_preflight_record_validates_against_the_environment_schema() -> None:
    document = build_environment(probe_internet=False)

    findings = validate_document(document, ENVIRONMENT)

    errors = [finding for finding in findings if finding.level == "error"]
    assert errors == []
    assert document["network"]["within_reference_conditions"] is False
    assert document["cpu_logical_cores"] >= 1
    assert Path(document["environment_id"]).name == document["environment_id"]
