#!/usr/bin/env bash
# Executes the pinned protocol vectors (message and handshake) against both
# independent implementations and fails on any divergence, on any accepted
# payload that should be rejected, and on any vector neither side executed.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/remotecam-conformance.XXXXXX")"
trap 'rm -rf "$work"' EXIT

cargo_bin="${CARGO:-cargo}"
gradle_bin="${GRADLE:-gradle}"

echo "conformance: running the Rust implementation"
"$cargo_bin" run --quiet \
  --manifest-path "$root/desktop/Cargo.toml" \
  -p remotecam-protocol \
  --bin conformance \
  -- --vectors "$root/protocol/vectors" --output "$work/rust.json"

echo "conformance: replaying the handshake scenarios on the Rust session core"
"$cargo_bin" run --quiet \
  --manifest-path "$root/desktop/Cargo.toml" \
  -p remotecam-session-core \
  --bin handshake_conformance \
  -- --vectors "$root/protocol/vectors" --output "$work/rust-handshake.json"

if ! command -v "$gradle_bin" >/dev/null 2>&1; then
  echo "conformance: gradle not found; set GRADLE to the Gradle 9.6.x executable" >&2
  exit 1
fi

echo "conformance: running the Kotlin implementation"
CONFORMANCE_OUTPUT="$work/kotlin.json" \
CONFORMANCE_HANDSHAKE_OUTPUT="$work/kotlin-handshake.json" "$gradle_bin" \
  -p "$root/mobile" \
  :protocol:test --tests "remotecam.protocol.ContractTest" \
  :session-core:test --tests "remotecam.session.HandshakeConformanceTest" \
  --rerun-tasks \
  --console=plain \
  --quiet

echo "conformance: comparing the result documents"
python3 "$root/protocol/vectors/compare_conformance.py" \
  "$work/rust.json" "$work/kotlin.json" \
  --rust-handshake "$work/rust-handshake.json" \
  --kotlin-handshake "$work/kotlin-handshake.json" \
  --vectors "$root/protocol/vectors"
