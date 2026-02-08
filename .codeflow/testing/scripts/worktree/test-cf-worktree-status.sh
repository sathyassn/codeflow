#!/usr/bin/env bash
# Purpose:   Test cf-worktree-status.sh functionality
# Usage:     bash test-cf-worktree-status.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-status.sh"

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

    if echo "$output" | grep -q "cf-worktree-status.sh"; then
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

test_runs_in_git_repo() {
    ((TEST_COUNT++)) || true
    local output

    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" 2>&1) || true

    if echo "$output" | grep -qi "worktree\|status\|healthy"; then
        echo "PASS: Runs in git repo"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: Should run in git repo"
    fi
}

test_verbose_option() {
    ((TEST_COUNT++)) || true

    # Just verify option is accepted
    if "$SCRIPT_UNDER_TEST" --verbose --help &>/dev/null || "$SCRIPT_UNDER_TEST" --help 2>&1 | grep -q "verbose"; then
        echo "PASS: --verbose option recognized"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --verbose option not recognized"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

echo "=== Testing cf-worktree-status.sh ==="
echo ""

test_script_exists_and_executable
test_script_syntax
test_help_option
test_version_option
test_runs_in_git_repo
test_verbose_option

echo ""
echo "=== Results: $PASS_COUNT/$TEST_COUNT passed ==="

[[ "$PASS_COUNT" -eq "$TEST_COUNT" ]] && exit 0 || exit 1
