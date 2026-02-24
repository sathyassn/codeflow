#!/usr/bin/env bash
# CodeFlow CI: Test Coverage Pairing Check
# Location: .codeflow/testing/ci/check-test-coverage-pairing.sh
#
# Verifies that modified hook source files have corresponding test file updates.
# Advisory only -- always exits 0 (warnings, not blockers).
#
# Usage:
#   bash .codeflow/testing/ci/check-test-coverage-pairing.sh [base-branch]
#
# Arguments:
#   base-branch  Branch to diff against (default: main)
#
# Mapping convention:
#   Source: .claude/hooks/codeflow/{event}/{name}.sh
#   Test:   .codeflow/testing/claude-hooks/{event}/test-{name}.sh

set -euo pipefail

BASE_BRANCH="${1:-main}"

# Find repo root
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"

HOOKS_DIR=".claude/hooks/codeflow"
TESTS_DIR=".codeflow/testing/claude-hooks"

# Get modified .sh files relative to repo root
MODIFIED_FILES=$(cd "$REPO_ROOT" && git diff --name-only "$BASE_BRANCH"...HEAD -- '*.sh' 2>/dev/null || true)

if [ -z "$MODIFIED_FILES" ]; then
    echo "[test-pairing] No .sh files modified in this branch."
    exit 0
fi

WARNINGS=0
PAIRED=0
SKIPPED=0

while IFS= read -r file; do
    # Only check files under the hooks directory
    if [[ "$file" != ${HOOKS_DIR}/* ]]; then
        SKIPPED=$((SKIPPED + 1))
        continue
    fi

    # Extract event and filename from path
    # e.g., .claude/hooks/codeflow/session-start/cf-session-start-init.sh
    #   -> event=session-start, name=cf-session-start-init.sh
    relative="${file#${HOOKS_DIR}/}"
    event="${relative%%/*}"
    name="${relative#*/}"

    # Build expected test path
    test_file="${TESTS_DIR}/${event}/test-${name}"

    # Check if the test file was also modified
    if echo "$MODIFIED_FILES" | grep -qF "$test_file"; then
        PAIRED=$((PAIRED + 1))
    else
        # Check if the test file exists at all
        if [ -f "$REPO_ROOT/$test_file" ]; then
            echo "[test-pairing] WARNING: $file was modified but $test_file was NOT updated."
            WARNINGS=$((WARNINGS + 1))
        else
            echo "[test-pairing] WARNING: $file has no test file at $test_file"
            WARNINGS=$((WARNINGS + 1))
        fi
    fi
done <<< "$MODIFIED_FILES"

echo ""
echo "[test-pairing] Summary: $PAIRED paired, $WARNINGS warnings, $SKIPPED skipped (non-hook)"

if [ "$WARNINGS" -gt 0 ]; then
    echo "[test-pairing] Consider updating test files for the warned sources above."
fi

# Advisory only -- always succeed
exit 0
