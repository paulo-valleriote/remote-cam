from __future__ import annotations

import pytest

from redaction import REDACTED, redact, redact_text

FULL_HEX = "9f2c1e4b6a8d0f3c5e7a9b1d2f4c6e8a0b2d4f6a8c0e2b4d6f8a0c1e3b5d7f9a"


def test_full_fingerprint_is_truncated() -> None:
    result = redact_text(f"tls_fingerprint={FULL_HEX}")

    assert FULL_HEX not in result
    assert result.startswith("tls_fingerprint=9f2c1e4b")
    assert "<redacted:fingerprint>" in result


def test_bare_full_hex_is_truncated_but_prefix_is_kept() -> None:
    result = redact_text(f"observed {FULL_HEX} in the trace")

    assert FULL_HEX not in result
    assert FULL_HEX[:8] in result
    assert "<redacted:fingerprint>" in result


def test_pairing_token_field_is_removed_regardless_of_value() -> None:
    value = {"encoder_input": {"pairing_token_hex": "00" * 32, "epoch": 7}}

    result = redact(value)

    assert result["encoder_input"]["pairing_token_hex"] == "<redacted:secret>"
    assert result["encoder_input"]["epoch"] == 7


def test_sha256_field_is_preserved() -> None:
    value = {"decoded_payload_sha256": FULL_HEX, "note": "synthetic payload"}

    result = redact(value)

    assert result["decoded_payload_sha256"] == FULL_HEX


@pytest.mark.parametrize(
    "text, secret",
    [
        ("token=abc123def456ghi789", "abc123def456ghi789"),
        ("password: hunter2hunter2", "hunter2hunter2"),
        ("api_key='sk-live-1234567890abcdef'", "sk-live-1234567890abcdef"),
        ("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig", "eyJhbGciOiJIUzI1NiJ9"),
    ],
)
def test_assignments_are_redacted(text: str, secret: str) -> None:
    result = redact_text(text)

    assert secret not in result
    assert "<redacted" in result


@pytest.mark.parametrize(
    "text",
    [
        "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sk-proj-abcdefghijklmnopqrstuvwx",
        "AKIAIOSFODNN7EXAMPLE",
        "xoxb-123456789012-abcdefghij",
    ],
)
def test_known_key_formats_are_redacted(text: str) -> None:
    assert "<redacted:key>" in redact_text(f"key {text} leaked")


def test_posix_home_path_is_collapsed() -> None:
    result = redact_text("/home/alice/programming/remotecam/spikes/evidence/run.json")

    assert "alice" not in result
    assert result == "~/programming/remotecam/spikes/evidence/run.json"


def test_root_home_path_is_collapsed() -> None:
    assert redact_text("/root/.config/app") == "~/.config/app"


def test_windows_user_path_is_collapsed() -> None:
    result = redact_text(r"C:\Users\Alice\AppData\Local\Remotecam")

    assert "Alice" not in result
    assert result == r"%USERPROFILE%\AppData\Local\Remotecam"


def test_explicit_user_name_is_removed() -> None:
    result = redact_text("Engineer pmv ran the capture.", user_names=["pmv"])

    assert "pmv" not in result
    assert result == "Engineer <redacted:user> ran the capture."


def test_user_name_does_not_match_inside_a_word() -> None:
    assert redact_text("pmvalue", user_names=["pmv"]) == "pmvalue"


def test_email_is_redacted() -> None:
    result = redact_text("contact alice@example.com for the run")

    assert "alice@example.com" not in result
    assert "<redacted:email>" in result


def test_pem_block_is_redacted() -> None:
    text = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg\n-----END PRIVATE KEY-----"

    assert redact_text(text) == "<redacted:pem>"


def test_inline_media_data_is_redacted() -> None:
    text = "capture=data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEA"

    assert "UklGRiQ" not in redact_text(text)
    assert "<redacted:media>" in redact_text(text)


def test_long_base64_blob_is_redacted() -> None:
    blob = "QUJD" * 100

    assert blob not in redact_text(f"payload {blob}")


def test_exact_media_path_is_redacted() -> None:
    path = "/mnt/captures/session.wav"

    assert redact_text(f"wrote {path}", media_paths=[path]) == "wrote <redacted:media>"


def test_media_extension_key_is_replaced() -> None:
    result = redact({"consumer_capture.wav": "/tmp/anything"})

    assert result["consumer_capture.wav"] == "<redacted:media>"


def test_nested_structures_are_redacted() -> None:
    value = {
        "scenarios": [
            {
                "scenario_id": "valid-01",
                "trace_ref": "/home/alice/traces/valid-01.trace",
                "pairing_token": "abc",
                "artefacts_found": [],
            }
        ]
    }

    result = redact(value)

    assert result["scenarios"][0]["scenario_id"] == "valid-01"
    assert result["scenarios"][0]["pairing_token"] == "<redacted:secret>"
    assert result["scenarios"][0]["trace_ref"] == "~/traces/valid-01.trace"
    assert result["scenarios"][0]["artefacts_found"] == []


def test_non_text_values_are_untouched() -> None:
    assert redact({"count": 3, "passed": True, "floor_ms": 33.4, "none": None}) == {
        "count": 3,
        "passed": True,
        "floor_ms": 33.4,
        "none": None,
    }


def test_redacted_constant_is_stable() -> None:
    assert REDACTED == "<redacted>"
