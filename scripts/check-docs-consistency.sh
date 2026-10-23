#!/usr/bin/env bash
#
# check-docs-consistency.sh
#
# Verifies that every `make <target>` command documented in a README exists as
# a Makefile target, and that every Makefile target is documented in a README.
#
# This enforces the project rule: "Every command documented in a README.md must
# exist as a Makefile target." (See docs/tdd-remotecam-v1.md and the root
# README.md.)
#
# Exit status is 1 when any inconsistency is found.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAKEFILES=(
    "$REPO_ROOT/Makefile"
    "$REPO_ROOT/scripts/make/common.mk"
    "$REPO_ROOT/scripts/make/desktop.mk"
    "$REPO_ROOT/scripts/make/mobile.mk"
    "$REPO_ROOT/scripts/make/quality.mk"
    "$REPO_ROOT/scripts/make/spikes.mk"
)
READMES=(
    "$REPO_ROOT/README.md"
    "$REPO_ROOT/desktop/README.md"
    "$REPO_ROOT/mobile/README.md"
)

FAILURES=0

# --- Collect Makefile targets ------------------------------------------------
#
# Targets are lines of the form:
#   target: ## description
#   target1 target2: ## description
#
declare -A MAKE_TARGETS=()
for mk in "${MAKEFILES[@]}"; do
    if [[ ! -f "$mk" ]]; then
        continue
    fi
    while IFS= read -r line; do
        # Match `target: ## ...`, `target1 target2: ## ...` and
        # `target: prerequisite ## ...` (the description follows the prerequisites).
        if [[ "$line" =~ ^([A-Za-z0-9_-]+( [A-Za-z0-9_-]+)*):[^#]*## ]]; then
            # Split multi-target lines on whitespace, take each token before ':'
            targets_part="${line%%:*}"
            for t in $targets_part; do
                MAKE_TARGETS["$t"]=1
            done
        fi
    done < "$mk"
done

# --- Collect make commands from READMEs --------------------------------------
#
# Match `make <target>` or `` `make <target>` `` inline code spans.
declare -A README_COMMANDS=()
for readme in "${READMES[@]}"; do
    if [[ ! -f "$readme" ]]; then
        continue
    fi
    while IFS= read -r line; do
        # Extract `make <word>` occurrences (including inside backticks)
        while [[ "$line" =~ make[[:space:]]+([A-Za-z0-9_-]+) ]]; do
            cmd="${BASH_REMATCH[1]}"
            README_COMMANDS["$cmd"]=1
            # Remove the matched portion to continue searching
            line="${line/*make ${BASH_REMATCH[1]}/}"
        done
    done < "$readme"
done

# --- Check 1: every documented make command exists as a target ---------------
echo "==> Checking README-documented commands exist as Makefile targets"
for cmd in "${!README_COMMANDS[@]}"; do
    if [[ -z "${MAKE_TARGETS[$cmd]:-}" ]]; then
        echo "  FAIL: '$cmd' is documented in a README but is not a Makefile target"
        FAILURES=$((FAILURES + 1))
    else
        echo "  OK:   make $cmd"
    fi
done

# --- Check 2: every Makefile target is documented in a README ----------------
echo ""
echo "==> Checking Makefile targets are documented in a README"
for target in "${!MAKE_TARGETS[@]}"; do
    if [[ -z "${README_COMMANDS[$target]:-}" ]]; then
        echo "  FAIL: Makefile target '$target' is not documented in any README"
        FAILURES=$((FAILURES + 1))
    else
        echo "  OK:   $target documented"
    fi
done

# --- Summary -----------------------------------------------------------------
echo ""
if [[ "$FAILURES" -gt 0 ]]; then
    echo "DOCS CONSISTENCY FAILED: $FAILURES inconsistency(ies) found"
    exit 1
fi
echo "DOCS CONSISTENCY PASSED: all README commands are Makefile targets and vice versa"
exit 0
