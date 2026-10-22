"""Redaction filter for every artefact the F0 spike tooling produces.

The rule is absolute: no secrets, no personal data and no real user media may
appear in a committed artefact (``spikes/README.md`` rule 2). This module is
deliberately conservative -- when a value looks like a credential, a full
fingerprint, an absolute user path or inline media, it is removed or truncated
instead of being passed through.

Two entry points:

``redact(value, ...)``
    Recursively redact a parsed JSON-like structure. Dictionary keys that name a
    credential are dropped regardless of their value; hash fields named
    ``sha256``/``hash``/``digest`` are preserved because they identify synthetic
    payloads rather than authenticate anything.

``redact_text(text, ...)``
    Redact a string. Used for logs and for free-form fields.
"""

from __future__ import annotations

import re
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

REDACTED = "<redacted>"

#: Dictionary keys whose values are always replaced. A key is considered
#: sensitive when it contains one of these fragments.
_SECRET_KEY_FRAGMENTS = (
    "token",
    "secret",
    "password",
    "passphrase",
    "private_key",
    "privatekey",
    "api_key",
    "apikey",
    "access_key",
    "client_secret",
    "authorization",
    "cookie",
    "session_key",
    "credential",
)

#: Keys whose values are content identifiers and must survive redaction.
_HASH_KEY_FRAGMENTS = ("sha256", "sha-256", "hash", "digest", "checksum", "etag")

_MEDIA_SUFFIXES = (
    ".wav",
    ".mp3",
    ".flac",
    ".m4a",
    ".ogg",
    ".opus",
    ".mp4",
    ".mkv",
    ".mov",
    ".avi",
    ".webm",
    ".png",
    ".jpg",
    ".jpeg",
    ".etl",
)

_PEM_BLOCK = re.compile(
    r"-----BEGIN [A-Z0-9 ]+-----.*?-----END [A-Z0-9 ]+-----",
    re.DOTALL,
)
_AUTHORIZATION = re.compile(r"(?i)\b(authorization\s*[:=]\s*)(?:bearer\s+)?[A-Za-z0-9._~+/=-]{8,}")
_BEARER = re.compile(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]{8,}")
_SECRET_ASSIGNMENT = re.compile(
    r"(?i)\b("
    r"pairing[_-]?token|session[_-]?key|api[_-]?key|access[_-]?key|client[_-]?secret|"
    r"token|secret|password|passwd|passphrase"
    r")\b(\s*[:=]\s*)(\"[^\"]*\"|'[^']*'|[^\s,;\]}]+)"
)
_KNOWN_KEY = re.compile(
    r"\b(?:sk-[A-Za-z0-9_-]{16,}|ghp_[A-Za-z0-9]{20,}|"
    r"github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}|"
    r"AKIA[0-9A-Z]{16})\b"
)
_EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")
_FULL_HEX_64 = re.compile(r"\b[0-9a-fA-F]{64}\b")
_DATA_URI = re.compile(
    r"data:(?:audio|video|image)/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]+",
    re.IGNORECASE,
)
_LONG_BASE64 = re.compile(r"\b[A-Za-z0-9+/]{200,}={0,2}\b")
_POSIX_HOME = re.compile(r"/(?:home|Users)/[^/\s\"']+")
_ROOT_HOME = re.compile(r"/root(?=[/\s\"']|$)")
_WINDOWS_HOME = re.compile(r"(?i)\b[A-Z]:\\Users\\[^\\\s\"']+")


def _truncate_hex(match: re.Match[str]) -> str:
    return f"{match.group(0)[:8]}\u2026<redacted:fingerprint>"


def redact_text(
    text: str,
    *,
    user_names: Iterable[str] = (),
    media_paths: Iterable[str] = (),
) -> str:
    """Return ``text`` with secrets, personal data and inline media removed."""

    result = text

    for path in sorted(media_paths, key=len, reverse=True):
        if path:
            result = result.replace(path, "<redacted:media>")

    result = _PEM_BLOCK.sub("<redacted:pem>", result)
    result = _DATA_URI.sub("data:<redacted:media>", result)
    result = _AUTHORIZATION.sub(r"\1<redacted:authorization>", result)
    result = _BEARER.sub(r"\1 <redacted:token>", result)
    result = _SECRET_ASSIGNMENT.sub(r"\1\2<redacted:secret>", result)
    result = _KNOWN_KEY.sub("<redacted:key>", result)
    result = _EMAIL.sub("<redacted:email>", result)
    result = _LONG_BASE64.sub("<redacted:base64>", result)
    result = _FULL_HEX_64.sub(_truncate_hex, result)

    for name in sorted({n for n in user_names if n}, key=len, reverse=True):
        result = re.sub(
            rf"(?<![A-Za-z0-9]){re.escape(name)}(?![A-Za-z0-9])", "<redacted:user>", result
        )

    result = _WINDOWS_HOME.sub("%USERPROFILE%", result)
    result = _POSIX_HOME.sub("~", result)
    result = _ROOT_HOME.sub("~", result)

    return result


def _key_is_secret(key: str) -> bool:
    lowered = key.lower()
    if any(fragment in lowered for fragment in _HASH_KEY_FRAGMENTS):
        return False
    return any(fragment in lowered for fragment in _SECRET_KEY_FRAGMENTS)


def _key_is_hash(key: str) -> bool:
    lowered = key.lower()
    return any(fragment in lowered for fragment in _HASH_KEY_FRAGMENTS)


def _key_is_media(key: str) -> bool:
    lowered = key.lower()
    return lowered.endswith(_MEDIA_SUFFIXES) or lowered in {"media", "media_data", "capture_bytes"}


def redact(
    value: Any,
    *,
    user_names: Iterable[str] = (),
    media_paths: Iterable[str] = (),
    _parent_key: str | None = None,
) -> Any:
    """Recursively redact a JSON-like value.

    ``user_names`` are the account names to remove from free text. ``media_paths``
    are exact paths of raw captures that must not be referenced.
    """

    names = tuple(user_names)
    paths = tuple(media_paths)

    if isinstance(value, Mapping):
        redacted: dict[str, Any] = {}
        for raw_key, raw_value in value.items():
            key = str(raw_key)
            if _key_is_secret(key):
                redacted[key] = "<redacted:secret>"
            elif _key_is_media(key):
                redacted[key] = "<redacted:media>"
            elif _key_is_hash(key):
                redacted[key] = raw_value
            else:
                redacted[key] = redact(
                    raw_value,
                    user_names=names,
                    media_paths=paths,
                    _parent_key=key,
                )
        return redacted

    if isinstance(value, str):
        redacted_value = redact_text(value, user_names=names, media_paths=paths)
        if _parent_key and _parent_key.lower().endswith(_MEDIA_SUFFIXES):
            return "<redacted:media>"
        return redacted_value

    if isinstance(value, Sequence) and not isinstance(value, (bytes, bytearray)):
        return [
            redact(item, user_names=names, media_paths=paths, _parent_key=_parent_key)
            for item in value
        ]

    return value


__all__ = ["redact", "redact_text", "REDACTED"]
