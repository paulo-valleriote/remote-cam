"""obs-websocket client tests for the OBS harness (T036).

The client is the one part of the OBS harness that cannot be exercised without a
running OBS, and the authentication hash is the part most likely to be wrong in
a way that looks like "the password is wrong". These tests pin the hash to the
protocol definition instead.
"""

from __future__ import annotations

import base64
import hashlib
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[3]
APPS_DIR = REPO_ROOT / "spikes" / "f0-camera-injection" / "apps"
if str(APPS_DIR) not in sys.path:
    sys.path.insert(0, str(APPS_DIR))

from obs.wsclient import ObsClient, ObsWebSocketError, _auth_string  # noqa: E402


def reference_auth_string(password: str, salt: str, challenge: str) -> str:
    """The obs-websocket 5.x formula, spelled out independently."""

    secret = base64.b64encode(hashlib.sha256(f"{password}{salt}".encode()).digest()).decode()
    return base64.b64encode(hashlib.sha256(f"{secret}{challenge}".encode()).digest()).decode()


def test_the_authentication_hash_matches_the_protocol() -> None:
    authentication = {"salt": "c2FsdA==", "challenge": "Y2hhbGxlbmdl"}

    assert _auth_string("hunter2", authentication) == reference_auth_string(
        "hunter2", "c2FsdA==", "Y2hhbGxlbmdl"
    )


def test_the_authentication_hash_is_base64_of_a_sha256() -> None:
    digest = base64.b64decode(_auth_string("pw", {"salt": "s", "challenge": "c"}))

    assert len(digest) == 32


def test_a_non_websocket_url_is_refused_before_connecting() -> None:
    client = ObsClient(url="http://127.0.0.1:4455")

    with pytest.raises(ObsWebSocketError, match="only ws://"):
        client.connect()


def test_an_unreachable_port_reports_a_connection_error() -> None:
    # Port 1 is reserved and never has a listener, so this exercises the failure
    # path without depending on OBS being absent.
    client = ObsClient(url="ws://127.0.0.1:1", timeout_s=1.0)

    with pytest.raises(OSError):
        client.connect()


def test_using_a_client_before_connecting_is_refused() -> None:
    client = ObsClient()

    with pytest.raises(ObsWebSocketError, match="not connected"):
        client.request("GetVersion")
