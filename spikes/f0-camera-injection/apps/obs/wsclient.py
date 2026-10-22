"""Minimal obs-websocket 5.x client on the standard library (T036).

**NON-PRODUCTION** — F0 risk-reduction spike.

The gate harness must not add a dependency to run, so this speaks the protocol
directly: a handshake request, an optional SHA-256 authentication, and a request
/ response / event loop. It supports exactly what the harness needs and refuses
anything else loudly rather than silently degrading.

obs-websocket handshake (protocol 5.x):

* every message is a JSON object with an op code;
* ``op 0`` Hello (server → client) carries ``authentication`` when a password is
  set: ``base64(sha256(base64(sha256(password + salt)) + challenge))``;
* ``op 1`` Identify (client → server) carries the negotiated RPC version;
* ``op 2`` Identified confirms the session;
* ``op 6`` Request (client → server), ``op 7`` RequestResponse (server → client);
* ``op 5`` Event (server → client).
"""

from __future__ import annotations

import base64
import hashlib
import json
import secrets
import socket
import struct
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, Final
from urllib.parse import urlparse

DEFAULT_URL: Final = "ws://127.0.0.1:4455"
RPC_VERSION: Final = 1
DEFAULT_TIMEOUT_S: Final = 30.0


class ObsWebSocketError(RuntimeError):
    """The obs-websocket session could not be established or used."""


@dataclass
class ObsClient:
    """A request/response client for obs-websocket 5.x."""

    url: str = DEFAULT_URL
    password: str | None = None
    timeout_s: float = DEFAULT_TIMEOUT_S
    _socket: socket.socket | None = field(default=None, init=False, repr=False)
    _buffer: bytearray = field(default_factory=bytearray, init=False, repr=False)
    _request_id: int = field(default=0, init=False, repr=False)
    events: list[dict[str, Any]] = field(default_factory=list, init=False)
    server_version: str = field(default="", init=False)

    def __enter__(self) -> ObsClient:
        return self.connect()

    def __exit__(self, *_error: object) -> None:
        self.close()

    def connect(self) -> ObsClient:
        parsed = urlparse(self.url)
        if parsed.scheme != "ws":
            raise ObsWebSocketError(f"only ws:// is supported, not {self.url!r}")
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 4455
        path = parsed.path or "/"

        connection = socket.create_connection((host, port), timeout=self.timeout_s)
        connection.settimeout(self.timeout_s)
        key = base64.b64encode(secrets.token_bytes(16)).decode()
        handshake = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            "\r\n"
        )
        connection.sendall(handshake.encode())
        response = self._read_handshake(connection)
        if "101" not in response.splitlines()[0]:
            connection.close()
            raise ObsWebSocketError(
                f"the WebSocket handshake was refused: {response.splitlines()[0]}"
            )
        expected = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        if expected.lower() not in response.lower():
            connection.close()
            raise ObsWebSocketError("the WebSocket handshake returned the wrong accept key")

        self._socket = connection
        hello = self._next_json()
        if hello.get("op") != 0:
            self.close()
            raise ObsWebSocketError(f"expected a Hello, received op {hello.get('op')}")
        self.server_version = hello["d"].get("obsWebSocketVersion", "")
        authentication = hello["d"].get("authentication")
        identify: dict[str, Any] = {"rpcVersion": RPC_VERSION, "eventSubscriptions": 0}
        if authentication:
            if not self.password:
                self.close()
                raise ObsWebSocketError(
                    "obs-websocket requires a password; pass --obs-password "
                    "or set OBS_WEBSOCKET_PASSWORD"
                )
            identify["authentication"] = _auth_string(self.password, authentication)
        self._send_json({"op": 1, "d": identify})
        identified = self._next_json()
        if identified.get("op") != 2:
            self.close()
            raise ObsWebSocketError(f"identify was refused: {identified}")
        return self

    def close(self) -> None:
        if self._socket is not None:
            try:
                self._send_frame(0x8, b"")
            except OSError:
                pass
            self._socket.close()
            self._socket = None

    def request(self, request_type: str, data: dict[str, Any] | None = None) -> dict[str, Any]:
        """Send one Request and return its response data."""

        self._request_id += 1
        request_id = str(self._request_id)
        self._send_json(
            {
                "op": 6,
                "d": {
                    "requestType": request_type,
                    "requestId": request_id,
                    "requestData": data or {},
                },
            }
        )
        while True:
            message = self._next_json()
            op = message.get("op")
            if op == 5:
                self.events.append(message)
                continue
            if op == 7:
                payload = message["d"]
                if payload.get("requestId") != request_id:
                    raise ObsWebSocketError(
                        f"response for {payload.get('requestId')} arrived while "
                        f"waiting for {request_id}"
                    )
                status = payload.get("requestStatus", {})
                if not status.get("result"):
                    raise ObsWebSocketError(
                        f"{request_type} failed with code {status.get('code')}: "
                        f"{status.get('comment', 'no comment')}"
                    )
                return payload.get("responseData", {})
            if op == 9:
                raise ObsWebSocketError(f"obs-websocket reported a protocol error: {message}")

    def wait_for(
        self, event, predicate: Callable[[dict[str, Any]], bool], *, timeout_s: float = 30.0
    ) -> dict[str, Any]:
        """Block until a matching event arrives, reusing events already seen."""

        for event_message in self.events:
            if event_message.get("d", {}).get("eventType") == event and predicate(
                event_message["d"]
            ):
                return event_message["d"]
        deadline = self.timeout_s if timeout_s is None else timeout_s
        previous = self._socket.gettimeout() if self._socket else None
        if self._socket is not None:
            self._socket.settimeout(deadline)
        try:
            while True:
                message = self._next_json()
                if message.get("op") == 5:
                    self.events.append(message)
                    payload = message["d"]
                    if payload.get("eventType") == event and predicate(payload):
                        return payload
        except TimeoutError as error:
            raise ObsWebSocketError(f"no {event} event arrived within {deadline:.0f} s") from error
        finally:
            if self._socket is not None and previous is not None:
                self._socket.settimeout(previous)

    # -- framing ---------------------------------------------------------

    def _read_handshake(self, connection: socket.socket) -> str:
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = connection.recv(4096)
            if not chunk:
                break
            data += chunk
        return data.decode("latin-1", "replace")

    def _send_json(self, message: dict[str, Any]) -> None:
        self._send_frame(0x1, json.dumps(message).encode("utf-8"))

    def _send_frame(self, opcode: int, payload: bytes) -> None:
        if self._socket is None:
            raise ObsWebSocketError("the client is not connected")
        mask = secrets.token_bytes(4)
        header = bytearray([0x80 | opcode])
        length = len(payload)
        if length < 126:
            header.append(0x80 | length)
        elif length < 1 << 16:
            header.append(0x80 | 126)
            header += struct.pack(">H", length)
        else:
            header.append(0x80 | 127)
            header += struct.pack(">Q", length)
        masked = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        self._socket.sendall(bytes(header) + mask + masked)

    def _next_json(self) -> dict[str, Any]:
        opcode, payload = self._next_frame()
        while opcode == 0x9:  # ping
            self._send_frame(0xA, payload)
            opcode, payload = self._next_frame()
        if opcode == 0x8:
            raise ObsWebSocketError("obs-websocket closed the connection")
        if opcode not in (0x1, 0x2):
            raise ObsWebSocketError(f"unexpected WebSocket opcode {opcode:#x}")
        return json.loads(payload.decode("utf-8"))

    def _next_frame(self) -> tuple[int, bytes]:
        first = self._read_exact(2)
        opcode = first[0] & 0x0F
        masked = bool(first[1] & 0x80)
        length = first[1] & 0x7F
        if length == 126:
            length = struct.unpack(">H", self._read_exact(2))[0]
        elif length == 127:
            length = struct.unpack(">Q", self._read_exact(8))[0]
        mask = self._read_exact(4) if masked else b""
        payload = self._read_exact(length)
        if masked:
            payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        return opcode, payload

    def _read_exact(self, count: int) -> bytes:
        if self._socket is None:
            raise ObsWebSocketError("the client is not connected")
        while len(self._buffer) < count:
            chunk = self._socket.recv(max(4096, count - len(self._buffer)))
            if not chunk:
                raise ObsWebSocketError("obs-websocket closed the connection")
            self._buffer += chunk
        result = bytes(self._buffer[:count])
        del self._buffer[:count]
        return result


def _auth_string(password: str, authentication: dict[str, str]) -> str:
    salt = authentication["salt"]
    challenge = authentication["challenge"]
    secret = base64.b64encode(hashlib.sha256((password + salt).encode()).digest()).decode()
    return base64.b64encode(hashlib.sha256((secret + challenge).encode()).digest()).decode()
