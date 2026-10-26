#!/usr/bin/env python3
"""Generate the pinned cross-language conformance vectors.

The generated files are the contract artefacts. This script only exists so that
the exact bytes of the QR vectors and the full scenario matrix are reproducible
and reviewable in one place; it is not used at test time.

Usage: python3 protocol/vectors/tools/generate_vectors.py
"""

from __future__ import annotations

import ipaddress
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
VECTORS = ROOT / "protocol" / "vectors"

MAGIC = b"RCM1"
NOW = 1_893_400_000  # 2030-01-01T00:00:00Z, far enough that the examples never rot.
EXPIRES = NOW + 56_000
SESSION_ID = bytes.fromhex("00112233445566778899aabbccddeeff")
TOKEN = bytes.fromhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
FINGERPRINT = bytes.fromhex(
    "9f2c1e4b6a8d0f3c5e7a9b1d2f4c6e8a0b2d4f6a8c0e2b4d6f8a0c1e3b5d7f9a"
)


def address_bytes(address: str) -> bytes:
    parsed = ipaddress.ip_address(address)
    if parsed.version == 4:
        return b"\x00" * 12 + parsed.packed
    return parsed.packed


def endpoint(family: int, address: str, port: int) -> bytes:
    return bytes([family]) + address_bytes(address) + port.to_bytes(2, "big")


def encode_payload(
    *,
    version: int = 1,
    flags: int = 0,
    session_id: bytes = SESSION_ID,
    epoch: int = 7,
    expires_at: int = EXPIRES,
    pairing_token: bytes = TOKEN,
    tls_fingerprint: bytes = FINGERPRINT,
    capabilities: int = 0x0011,
    endpoints: list[tuple[int, str, int]],
    magic: bytes = MAGIC,
    trailing: bytes = b"",
) -> bytes:
    body = b"".join(endpoint(*item) for item in endpoints)
    if len(session_id) != 16 or len(pairing_token) != 32 or len(tls_fingerprint) != 32:
        raise ValueError("fixed-length field has the wrong size")
    return (
        magic
        + bytes([version, flags])
        + len(body).to_bytes(2, "big")
        + session_id
        + epoch.to_bytes(4, "big")
        + expires_at.to_bytes(8, "big")
        + pairing_token
        + tls_fingerprint
        + capabilities.to_bytes(2, "big")
        + body
        + trailing
    )


def endpoint_sort_key(item: tuple[int, str, int]) -> tuple[int, bytes]:
    return (item[0], address_bytes(item[1]))


def qr_vector(
    vector_id: str,
    description: str,
    *,
    accepted: bool,
    reason_code: str | None,
    payload: bytes,
    encoder_input: dict | None = None,
    now_unix: int = NOW,
) -> dict:
    vector = {
        "id": vector_id,
        "description": description,
        "now_unix": now_unix,
        "expected": {
            "accepted": accepted,
            "reason_code": reason_code,
            "payload_hex": payload.hex(),
        },
    }
    if encoder_input is not None:
        vector["encoder_input"] = encoder_input
    return vector


def encoder_input(
    *,
    endpoints: list[tuple[int, str, int]],
    epoch: int = 7,
    expires_at: int = EXPIRES,
    capabilities: int = 0x0011,
    session_id: bytes = SESSION_ID,
    pairing_token: bytes = TOKEN,
    tls_fingerprint: bytes = FINGERPRINT,
) -> dict:
    return {
        "session_id_hex": session_id.hex(),
        "epoch": epoch,
        "expires_at": expires_at,
        "pairing_token_hex": pairing_token.hex(),
        "tls_fingerprint_hex": tls_fingerprint.hex(),
        "capabilities": capabilities,
        "endpoints": [
            {"family": family, "address": address, "port": port}
            for family, address, port in endpoints
        ],
    }


def build_qr_vectors() -> list[dict]:
    minimal_endpoints = [(4, "192.168.1.42", 51820)]
    multi_endpoints = [
        (6, "fe80::1", 51822),
        (4, "10.0.0.7", 51821),
        (4, "192.168.1.42", 51820),
    ]
    ipv6_endpoints = [(6, "fe80::1c9b:2f4d:8a11:9c02", 51823)]

    valid_minimal = encode_payload(endpoints=minimal_endpoints)
    valid_multi = encode_payload(
        endpoints=sorted(multi_endpoints, key=endpoint_sort_key), capabilities=0x001B
    )
    valid_ipv6 = encode_payload(endpoints=ipv6_endpoints, capabilities=0x0013)

    return [
        qr_vector(
            "minimal-v1",
            "Single IPv4 endpoint, H.264 and 720p30 capabilities.",
            accepted=True,
            reason_code=None,
            payload=valid_minimal,
            encoder_input=encoder_input(endpoints=minimal_endpoints),
        ),
        qr_vector(
            "multi-endpoint-v1",
            "Three endpoints; the encoder MUST emit them in stable family/address order.",
            accepted=True,
            reason_code=None,
            payload=valid_multi,
            encoder_input=encoder_input(endpoints=multi_endpoints, capabilities=0x001B),
        ),
        qr_vector(
            "ipv6-v1",
            "IPv6 link-local endpoint with H.264, VP8 and 720p30 capabilities.",
            accepted=True,
            reason_code=None,
            payload=valid_ipv6,
            encoder_input=encoder_input(endpoints=ipv6_endpoints, capabilities=0x0013),
        ),
        qr_vector(
            "unknown-version",
            "Version 2 must be rejected outright; no implicit compatibility.",
            accepted=False,
            reason_code="unsupported_qr_version",
            payload=encode_payload(endpoints=minimal_endpoints, version=2),
        ),
        qr_vector(
            "bad-magic",
            "Magic other than RCM1 is rejected.",
            accepted=False,
            reason_code="invalid_qr_payload",
            payload=encode_payload(endpoints=minimal_endpoints, magic=b"RCM0"),
        ),
        qr_vector(
            "reserved-bit-set",
            "A reserved flag bit is set; version 1 forbids every flag bit.",
            accepted=False,
            reason_code="invalid_qr_payload",
            payload=encode_payload(endpoints=minimal_endpoints, flags=0x01),
        ),
        qr_vector(
            "truncated",
            "The payload ends before the declared endpoints block is complete.",
            accepted=False,
            reason_code="invalid_qr_payload",
            payload=encode_payload(endpoints=minimal_endpoints)[:-2],
        ),
        qr_vector(
            "trailing-byte",
            "One extra byte after the declared payload is an error, not padding.",
            accepted=False,
            reason_code="invalid_qr_payload",
            payload=encode_payload(endpoints=minimal_endpoints, trailing=b"\x00"),
        ),
        qr_vector(
            "public-endpoint",
            "A public IPv4 endpoint is rejected: only RFC 1918 addresses are permitted.",
            accepted=False,
            reason_code="no_private_endpoint",
            payload=encode_payload(endpoints=[(4, "8.8.8.8", 51820)]),
        ),
        qr_vector(
            "expired",
            "The expiry is strictly in the past relative to the pinned clock.",
            accepted=False,
            reason_code="qr_expired",
            payload=encode_payload(endpoints=minimal_endpoints, expires_at=NOW - 1),
        ),
        qr_vector(
            "empty-endpoints",
            "No endpoint record is present; endpoints_len must be a non-zero multiple of 19.",
            accepted=False,
            reason_code="invalid_qr_payload",
            payload=encode_payload(endpoints=[]),
        ),
    ]


def message(
    scenario_id: str,
    description: str,
    payload: dict | str,
    *,
    accepted: bool,
    reason_code: str | None = None,
) -> dict:
    return {
        "id": scenario_id,
        "kind": "message",
        "description": description,
        "message": payload,
        "expected_outcome": "accept" if accepted else "reject",
        "expected_reason_code": reason_code,
    }


def handshake(
    scenario_id: str,
    description: str,
    *,
    accepted: bool,
    reason_code: str | None = None,
    requires_network: bool = True,
) -> dict:
    return {
        "id": scenario_id,
        "kind": "handshake",
        "description": description,
        "requires_network": requires_network,
        "expected_outcome": "accept" if accepted else "reject",
        "expected_reason_code": reason_code,
    }


def base_messages() -> dict[str, dict]:
    return {
        "pair": {
            "version": 1,
            "type": "pair",
            "token": TOKEN.hex(),
            "dtls_fingerprint": FINGERPRINT.hex(),
            "limits": {
                "max_width": 1280,
                "max_height": 720,
                "max_frame_rate": 30,
                "max_video_bitrate_kbps": 2500,
                "max_audio_bitrate_kbps": 48,
            },
        },
        "paired": {
            "version": 1,
            "type": "paired",
            "session_id": SESSION_ID.hex(),
            "epoch": 7,
            "dtls_fingerprint": FINGERPRINT.hex(),
            "accepted_limits": {"max_width": 1280, "max_height": 720, "max_frame_rate": 30},
        },
        "offer": {
            "version": 1,
            "type": "offer",
            "epoch": 7,
            "sdp": "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=sendonly\r\n",
        },
        "answer": {
            "version": 1,
            "type": "answer",
            "epoch": 7,
            "sdp": "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\n",
        },
        "ice": {
            "version": 1,
            "type": "ice",
            "epoch": 7,
            "candidates": [
                {"address": "192.168.1.42", "port": 51234, "component": 1, "protocol": "udp"},
                {"address": "192.168.1.42", "port": 51235, "component": 2, "protocol": "udp"},
            ],
        },
        "stats": {
            "version": 1,
            "type": "stats",
            "fps": 30,
            "video_bitrate_kbps": 2500,
            "rtt_ms": 12.4,
            "loss_pct": 0.0,
            "thermal_status": "none",
        },
        "close": {"version": 1, "type": "close", "code": "ended_by_mobile"},
        "error": {"version": 1, "type": "error", "code": "invalid_message"},
        "ping": {"version": 1, "type": "ping"},
        "pong": {"version": 1, "type": "pong"},
    }


def mutated(base: dict, **changes: object) -> dict:
    result = dict(base)
    for key, value in changes.items():
        if value is None:
            result.pop(key, None)
        else:
            result[key] = value
    return result


def build_signal_scenarios() -> list[dict]:
    base = base_messages()
    scenarios = [
        message("msg-pair-valid", "A valid pair message.", base["pair"], accepted=True),
        message("msg-paired-valid", "A valid paired message.", base["paired"], accepted=True),
        message("msg-offer-valid", "A valid SDP offer.", base["offer"], accepted=True),
        message("msg-answer-valid", "A valid recvonly SDP answer.", base["answer"], accepted=True),
        message("msg-ice-valid", "A valid ICE candidate list.", base["ice"], accepted=True),
        message("msg-stats-valid", "A valid stats message.", base["stats"], accepted=True),
        message("msg-close-valid", "A valid session close.", base["close"], accepted=True),
        message("msg-error-valid", "A valid signaling error.", base["error"], accepted=True),
        message("msg-ping-valid", "A valid ping.", base["ping"], accepted=True),
        message("msg-pong-valid", "A valid pong.", base["pong"], accepted=True),
        message(
            "msg-missing-version",
            "version is required.",
            mutated(base["ping"], version=None),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-missing-type",
            "type is required.",
            mutated(base["ping"], type=None),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-version-zero",
            "Version 0 is below the supported minimum.",
            mutated(base["ping"], version=0),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-version-two",
            "A newer protocol version is rejected with its own code, not silently accepted.",
            mutated(base["ping"], version=2),
            accepted=False,
            reason_code="unsupported_version",
        ),
        message(
            "msg-version-wrong-type",
            "A string version does not satisfy the schema.",
            mutated(base["ping"], version="1"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-type-unknown",
            "The type enumeration is closed.",
            mutated(base["ping"], type="hello"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-type-wrong-type",
            "A non-string type does not satisfy the schema.",
            mutated(base["ping"], type=1),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-missing-token",
            "pair requires the single-use token.",
            mutated(base["pair"], token=None),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-short-token",
            "The pairing token must be exactly 256 bits.",
            mutated(base["pair"], token="00" * 31),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-non-hex-token",
            "The pairing token must be lowercase hexadecimal.",
            mutated(base["pair"], token="zz" * 32),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-unknown-field",
            "Unknown fields are rejected, never ignored.",
            mutated(base["pair"]) | {"token_hint": "leak"},
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-bad-fingerprint",
            "The DTLS fingerprint is exactly 64 lowercase hex characters.",
            mutated(base["pair"], dtls_fingerprint="a" * 63),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-pair-out-of-range-limit",
            "A capture limit outside the declared range is rejected.",
            mutated(
                base["pair"],
                limits={"max_width": 9000},
            ),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-paired-missing-session",
            "paired requires the session identifier.",
            mutated(base["paired"], session_id=None),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-paired-bad-session-length",
            "The session identifier is exactly 128 bits.",
            mutated(base["paired"], session_id="ab" * 15),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-paired-negative-epoch",
            "The epoch cannot be negative.",
            mutated(base["paired"], epoch=-1),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-paired-unknown-field",
            "paired rejects unknown fields.",
            mutated(base["paired"], role="host"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-offer-empty-sdp",
            "The SDP body must not be empty.",
            mutated(base["offer"], sdp=""),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-offer-oversized-sdp",
            "The SDP body is capped at 8192 characters.",
            mutated(base["offer"], sdp="a" * 8193),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-answer-missing-sdp",
            "answer requires an SDP body.",
            mutated(base["answer"], sdp=None),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ice-public-candidate",
            "A candidate with a public address is rejected.",
            mutated(
                base["ice"],
                candidates=[{"address": "203.0.113.9", "port": 5000, "component": 1}],
            ),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ice-tcp-candidate",
            "Only UDP ICE candidates are permitted.",
            mutated(
                base["ice"],
                candidates=[
                    {"address": "192.168.1.42", "port": 5000, "component": 1, "protocol": "tcp"}
                ],
            ),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ice-candidate-missing-port",
            "A candidate requires its port.",
            mutated(base["ice"], candidates=[{"address": "192.168.1.42", "component": 1}]),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ice-bad-component",
            "The ICE component is 1 or 2.",
            mutated(
                base["ice"],
                candidates=[{"address": "192.168.1.42", "port": 5000, "component": 3}],
            ),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ice-too-many-candidates",
            "At most 64 candidates are accepted.",
            mutated(
                base["ice"],
                candidates=[
                    {"address": "192.168.1.42", "port": 5000 + index, "component": 1}
                    for index in range(65)
                ],
            ),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-stats-fps-out-of-range",
            "fps is capped at 120.",
            mutated(base["stats"], fps=121),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-stats-unknown-field",
            "stats carries no identifiers and rejects unknown fields.",
            mutated(base["stats"], device_name="phone"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-close-unknown-code",
            "The close code enumeration is closed.",
            mutated(base["close"], code="bye"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-error-unknown-code",
            "The error code enumeration is closed.",
            mutated(base["error"], code="oops"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-error-message-too-long",
            "The human-readable error text is capped at 200 characters.",
            mutated(base["error"], message="x" * 201),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-ping-unknown-field",
            "ping carries no payload and rejects unknown fields.",
            mutated(base["ping"], nonce="123"),
            accepted=False,
            reason_code="invalid_message",
        ),
        message(
            "msg-not-an-object",
            "A non-object frame is not a signaling message.",
            "hello",
            accepted=False,
            reason_code="invalid_message",
        ),
        {
            "id": "msg-invalid-json",
            "kind": "message",
            "description": "Malformed JSON is rejected without detail.",
            "raw_frame": "{not json",
            "expected_outcome": "reject",
            "expected_reason_code": "invalid_message",
        },
    ]

    scenarios.extend(
        [
            handshake(
                "hs-valid-pairing",
                "The valid pairing scenario establishes the authenticated channel.",
                accepted=True,
            ),
            handshake(
                "hs-wrong-fingerprint",
                "One byte of the certificate fingerprint differs.",
                accepted=False,
                reason_code="certificate_pin_mismatch",
            ),
            handshake(
                "hs-chain-only-trust",
                "The peer presents a public-CA chain instead of the pinned certificate.",
                accepted=False,
                reason_code="certificate_chain_rejected",
            ),
            handshake(
                "hs-tls-downgrade",
                "The peer offers TLS 1.2 or attempts a downgrade.",
                accepted=False,
                reason_code="tls_version_unsupported",
            ),
            handshake(
                "hs-token-expired",
                "The pairing token outlived its 120 s window.",
                accepted=False,
                reason_code="token_expired",
            ),
            handshake(
                "hs-token-reused",
                "A consumed pairing token is presented again.",
                accepted=False,
                reason_code="token_already_used",
            ),
            handshake(
                "hs-token-wrong",
                "A forged pairing token is presented.",
                accepted=False,
                reason_code="token_invalid",
            ),
            handshake(
                "hs-source-not-private",
                "The connection originates outside the private ranges.",
                accepted=False,
                reason_code="source_address_not_private",
            ),
            handshake(
                "hs-interception-proxy",
                "An intercepting proxy terminates TLS with its own certificate.",
                accepted=False,
                reason_code="certificate_pin_mismatch",
            ),
            handshake(
                "hs-qr-expired",
                "The QR payload expired before it was scanned.",
                accepted=False,
                reason_code="qr_expired",
                requires_network=False,
            ),
            handshake(
                "hs-qr-malformed",
                "The QR payload fails structural validation.",
                accepted=False,
                reason_code="invalid_qr_payload",
                requires_network=False,
            ),
            handshake(
                "hs-qr-public-endpoint",
                "The QR payload advertises only a public endpoint.",
                accepted=False,
                reason_code="no_private_endpoint",
                requires_network=False,
            ),
            handshake(
                "hs-qr-unsupported-version",
                "The QR payload version is newer than this build supports.",
                accepted=False,
                reason_code="unsupported_qr_version",
                requires_network=False,
            ),
            handshake(
                "hs-desktop-unreachable",
                "The advertised endpoint is closed or blocked.",
                accepted=False,
                reason_code="desktop_unreachable",
            ),
            handshake(
                "hs-tls-handshake-failed",
                "The TLS handshake fails for a network or protocol reason.",
                accepted=False,
                reason_code="tls_handshake_failed",
            ),
            handshake(
                "hs-source-subnet-mismatch",
                "The source address is private but outside the announced subnet.",
                accepted=False,
                reason_code="source_subnet_mismatch",
            ),
            handshake(
                "hs-superseded",
                "A newer QR supersedes the session under test.",
                accepted=False,
                reason_code="superseded",
            ),
            handshake(
                "hs-protocol-violation",
                "A second pair message with a stale epoch is discarded and counted.",
                accepted=False,
                reason_code="protocol_violation",
            ),
            handshake(
                "hs-internal-error",
                "An unexpected internal fault is reported with a closed code.",
                accepted=False,
                reason_code="internal_error",
            ),
            handshake(
                "hs-rate-limited",
                "The sixth attempt from one origin within a minute is refused.",
                accepted=False,
                reason_code="rate_limited",
            ),
            handshake(
                "hs-listener-closed",
                "Ten consecutive failures close the pairing listener.",
                accepted=False,
                reason_code="listener_closed",
            ),
            handshake(
                "hs-flood-second-origin-succeeds",
                "A flood from one origin does not deny a legitimate second origin.",
                accepted=True,
            ),
        ]
    )

    ids = [item["id"] for item in scenarios]
    if len(ids) != len(set(ids)):
        raise SystemExit("duplicate scenario id")
    return scenarios


def canonical_frame(message: dict) -> str:
    return json.dumps(message, separators=(",", ":"), ensure_ascii=False)


def encoding_vector(vector_id: str, description: str, message: dict) -> dict:
    return {
        "id": vector_id,
        "description": description,
        "message": message,
        "expected_frame": canonical_frame(message),
    }


def build_encoding_vectors() -> list[dict]:
    base = base_messages()
    without_protocol = mutated(
        base["ice"],
        candidates=[
            {"address": "192.168.1.42", "port": 51234, "component": 1, "protocol": "udp"},
            {"address": "10.0.0.7", "port": 51235, "component": 2},
        ],
    )
    stats_with_floats = mutated(
        base["stats"],
        fps=30.0,
        video_bitrate_kbps=2500.0,
        rtt_ms=12.4,
        loss_pct=0.0,
    )

    return [
        encoding_vector(
            "enc-pair",
            "The canonical pair frame; every limit is present.",
            base["pair"],
        ),
        encoding_vector(
            "enc-pair-empty-limits",
            "An empty limits object is emitted as an empty object, not omitted.",
            mutated(base["pair"], limits={}),
        ),
        encoding_vector(
            "enc-paired",
            "The canonical paired frame.",
            base["paired"],
        ),
        encoding_vector(
            "enc-offer",
            "The canonical offer frame with its CRLF SDP body escaped.",
            base["offer"],
        ),
        encoding_vector(
            "enc-answer",
            "The canonical recvonly answer frame.",
            base["answer"],
        ),
        encoding_vector(
            "enc-ice",
            "Candidates keep their schema order and omit an absent protocol.",
            without_protocol,
        ),
        encoding_vector(
            "enc-stats",
            "Every optional metric is present and emitted in schema order.",
            stats_with_floats,
        ),
        encoding_vector(
            "enc-stats-empty",
            "A stats message without metrics emits only version and type.",
            {"version": 1, "type": "stats"},
        ),
        encoding_vector(
            "enc-close",
            "The canonical close frame.",
            base["close"],
        ),
        encoding_vector(
            "enc-error",
            "The canonical error frame with its diagnostic text.",
            mutated(base["error"], message="malformed frame"),
        ),
        encoding_vector(
            "enc-error-without-message",
            "An absent diagnostic text is omitted.",
            base["error"],
        ),
        encoding_vector(
            "enc-ping",
            "The canonical ping frame.",
            base["ping"],
        ),
        encoding_vector(
            "enc-pong",
            "The canonical pong frame.",
            base["pong"],
        ),
    ]


def build_reason_code_vector() -> dict:
    source = json.loads((ROOT / "protocol" / "reason-codes.json").read_text())
    return {
        "version": 1,
        "description": "Pinned closed enumeration. Both implementations MUST spell every code exactly as listed.",
        "codes": [
            {
                "code": entry["code"],
                "categories": entry["categories"],
                "security_event": entry["security_event"],
                "user_actionable": entry["user_actionable"],
            }
            for entry in source["codes"]
        ],
    }


def write_json(path: Path, data: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def main() -> None:
    for vector in build_qr_vectors():
        write_json(VECTORS / "qr" / f"{vector['id']}.json", vector)
    write_json(VECTORS / "reason-codes" / "all-codes.json", build_reason_code_vector())
    write_json(
        VECTORS / "signaling" / "scenarios.json",
        {
            "version": 1,
            "description": "Pinned signaling and pairing scenarios. kind=message cases are executed by both protocol consumers; kind=handshake cases are executed by the pairing harness.",
            "scenarios": build_signal_scenarios(),
        },
    )
    write_json(
        VECTORS / "signaling" / "encoding.json",
        {
            "version": 1,
            "description": "Pinned canonical frames for the emitting direction. Every vector must encode to exactly expected_frame and parse back to the same message. Executed by the Rust contract crate; the Kotlin emitter joins with REC-14.",
            "vectors": build_encoding_vectors(),
        },
    )
    print("vectors written under", VECTORS)


if __name__ == "__main__":
    main()
