#!/usr/bin/env bash
# Purpose:   Test cf-worktree-list.sh functionality
# Usage:     bash test-cf-worktree-list.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-list.sh"

# Source test helpers
source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"

TEST_COUNT=0
PASS_COUNT=0

# =============================================================================
# TESTS
# =============================================================================

test_help_option() {
    ((TEST_COUNT++)) || true
    local output
    output=$("$SCRIPT_UNDER_TEST" --help 2>&1)

    if echo "$output" | grep -q "cf-worktree-list.sh"; then
        echo "PASS: --help shows usage"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --help doesn't show expected output"
    fi
}

test_version_option() {
    ((TEST_COUNT++)) || true
    local output
    output=$("$SCRIPT_UNDER_TEST" --version 2>&1)

    if echo "$output" | grep -q "version"; then
        echo "PASS: --version shows version"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --version doesn't show expected output"
    fi
}

test_script_exists_and_executable() {
    ((TEST_COUNT++)) || true

    if [[ -x "$SCRIPT_UNDER_TEST" ]]; then
        echo "PASS: Script exists and is executable"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: Script not found or not executable"
    fi
}

test_script_syntax() {
    ((TEST_COUNT++)) || true

    if bash -n "$SCRIPT_UNDER_TEST" 2>/dev/null; then
        echo "PASS: Script has valid syntax"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: Script has syntax errors"
    fi
}

test_runs_without_state_file() {
    ((TEST_COUNT++)) || true
    local output

    # Run in temp dir without worktrees.yaml
    local tmpdir
    tmpdir=$(mktemp -d)
    cd "$tmpdir"

    output=$("$SCRIPT_UNDER_TEST" 2>&1) || true

    cd "$REPO_ROOT"
    rm -rf "$tmpdir"

    if echo "$output" | grep -qi "no tracked worktrees\|git worktrees"; then
        echo "PASS: Handles missing state file gracefully"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: Should handle missing state file"
    fi
}

test_format_option() {
    ((TEST_COUNT++)) || true

    # Just verify option is accepted
    if "$SCRIPT_UNDER_TEST" --format table --help &>/dev/null || "$SCRIPT_UNDER_TEST" --help 2>&1 | grep -q "format"; then
        echo "PASS: --format option recognized"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --format option not recognized"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

echo "=== Testing cf-worktree-list.sh ==="
echo ""

test_script_exists_and_executable
test_script_syntax
test_help_option
test_version_option
test_runs_without_state_file
test_format_option

echo ""
echo "=== Results: $PASS_COUNT/$TEST_COUNT passed ==="

[[ "$PASS_COUNT" -eq "$TEST_COUNT" ]] && exit 0 || exit 1
