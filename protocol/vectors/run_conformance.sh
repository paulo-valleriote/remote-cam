#!/usr/bin/env bash
# Executes the pinned protocol vectors against both independent implementations
# and fails on any divergence or on any accepted payload that should be rejected.
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

if ! command -v "$gradle_bin" >/dev/null 2>&1; then
  echo "conformance: gradle not found; set GRADLE to the Gradle 9.6.x executable" >&2
  exit 1
fi

echo "conformance: running the Kotlin implementation"
CONFORMANCE_OUTPUT="$work/kotlin.json" "$gradle_bin" \
  -p "$root/mobile" \
  :protocol:testDebugUnitTest \
  --tests "remotecam.protocol.ContractTest" \
  --rerun-tasks \
  --console=plain \
  --quiet

echo "conformance: comparing both result documents"
python3 "$root/protocol/vectors/compare_conformance.py" "$work/rust.json" "$work/kotlin.json"
