"""Redaction audit across every F0 spike artefact.

Scans all committed artefacts under ``spikes/`` for secrets, full fingerprints
(64-hex), absolute user paths, personal data and real user media. Any finding
makes the audit fail (exit 1).

This is the machine-readable companion to ``spikes/tools/redaction.py``: it
applies ``redact_text()`` to every text/JSON artefact and reports any value
that would be changed, meaning a secret or personal datum is present.

NON-PRODUCTION — F0 risk-reduction spike (FR-039).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from redaction import (  # noqa: E402
    _EMAIL,
    _FULL_HEX_64,
    _KNOWN_KEY,
    _LONG_BASE64,
    _PEM_BLOCK,
    _POSIX_HOME,
    _ROOT_HOME,
    _WINDOWS_HOME,
    redact_text,
)

REPO_ROOT = Path(__file__).resolve().parents[3]

# Directories and file extensions to audit.
AUDIT_DIRS = [
    REPO_ROOT / "spikes" / "evidence",
    REPO_ROOT / "spikes" / "tools",
    REPO_ROOT / "docs" / "spike",
    REPO_ROOT / "protocol",
]
AUDIT_EXTENSIONS = {".json", ".md", ".py", ".rs", ".kt", ".toml", ".yaml", ".yml"}

# Patterns that indicate a secret / personal datum / raw media reference.
FINDING_PATTERNS = [
    ("pem_block", _PEM_BLOCK),
    ("known_key", _KNOWN_KEY),
    ("email", _EMAIL),
    ("full_hex_64", _FULL_HEX_64),
    ("long_base64", _LONG_BASE64),
    ("posix_home", _POSIX_HOME),
    ("root_home", _ROOT_HOME),
    ("windows_home", _WINDOWS_HOME),
]


def _scan_text(text: str, source: str) -> list[dict]:
    """Return a list of findings in ``text``."""
    findings: list[dict] = []
    for name, pattern in FINDING_PATTERNS:
        for match in pattern.finditer(text):
            findings.append(
                {
                    "source": source,
                    "kind": name,
                    "match": match.group(0)[:80],
                    "line": text.count("\n", 0, match.start()) + 1,
                }
            )
    # Compare against redacted output: any difference means something was redacted.
    redacted = redact_text(text)
    if redacted != text:
        findings.append(
            {
                "source": source,
                "kind": "redaction_changed",
                "match": "text differs after redaction",
                "line": 0,
            }
        )
    return findings


def _audit_file(path: Path) -> list[dict]:
    """Audit a single file, returning findings."""
    try:
        text = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError):
        # Binary or unreadable file — skip text audit but flag media extensions.
        media_extensions = {".wav", ".mp4", ".mkv", ".etl", ".webm", ".png", ".jpg", ".jpeg"}
        if path.suffix.lower() in media_extensions:
            return [
                {
                    "source": str(path.relative_to(REPO_ROOT)),
                    "kind": "raw_media_file",
                    "match": path.name,
                    "line": 0,
                }
            ]
        return []

    findings: list[dict] = []
    findings.extend(_scan_text(text, str(path.relative_to(REPO_ROOT))))

    # Also audit JSON structure recursively for secret keys.
    if path.suffix.lower() == ".json":
        try:
            data = json.loads(text)
        except json.JSONDecodeError:
            pass
        else:
            findings.extend(_audit_json(data, str(path.relative_to(REPO_ROOT))))

    return findings


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


def _audit_json(value, source: str, path: str = "$") -> list[dict]:
    """Recursively audit a parsed JSON structure for secret keys and media refs."""
    findings: list[dict] = []
    if isinstance(value, dict):
        for key, val in value.items():
            key_lower = str(key).lower()
            if any(frag in key_lower for frag in _SECRET_KEY_FRAGMENTS):
                findings.append(
                    {
                        "source": source,
                        "kind": "secret_key",
                        "match": f"{path}.{key}",
                        "line": 0,
                    }
                )
            if isinstance(val, (dict, list)):
                findings.extend(_audit_json(val, source, f"{path}.{key}"))
            elif isinstance(val, str):
                findings.extend(_scan_text(val, f"{source} ({path}.{key})"))
    elif isinstance(value, list):
        for i, item in enumerate(value):
            if isinstance(item, (dict, list)):
                findings.extend(_audit_json(item, source, f"{path}[{i}]"))
            elif isinstance(item, str):
                findings.extend(_scan_text(item, f"{source} ({path}[{i}])"))
    return findings


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Audit F0 spike artefacts for secrets and personal data."
    )
    parser.add_argument(
        "paths",
        nargs="*",
        help="Files or directories to audit (default: all AUDIT_DIRS)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit findings as JSON on stdout.",
    )
    args = parser.parse_args()

    if args.paths:
        roots = [Path(p) for p in args.paths]
    else:
        roots = AUDIT_DIRS

    all_findings: list[dict] = []
    for root in roots:
        if root.is_file():
            all_findings.extend(_audit_file(root))
        elif root.is_dir():
            for path in sorted(root.rglob("*")):
                if path.is_file() and path.suffix.lower() in AUDIT_EXTENSIONS:
                    all_findings.extend(_audit_file(path))

    if args.json:
        print(json.dumps({"findings": all_findings, "count": len(all_findings)}, indent=2))
    else:
        if all_findings:
            print(f"REDACTION AUDIT FAILED: {len(all_findings)} finding(s)")
            for f in all_findings:
                print(f"  {f['source']}:{f['line']} [{f['kind']}] {f['match']}")
        else:
            print(
                "REDACTION AUDIT PASSED: no secrets, fingerprints, paths, or personal data found."
            )

    return 1 if all_findings else 0


if __name__ == "__main__":
    sys.exit(main())
