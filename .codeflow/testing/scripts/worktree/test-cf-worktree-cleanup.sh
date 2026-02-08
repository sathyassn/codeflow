#!/usr/bin/env bash
# Purpose:   Test cf-worktree-cleanup.sh functionality
# Usage:     bash test-cf-worktree-cleanup.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-cleanup.sh"

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

    if echo "$output" | grep -q "cf-worktree-cleanup.sh"; then
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

test_dry_run_option() {
    ((TEST_COUNT++)) || true
    local output

    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run 2>&1) || true

    if echo "$output" | grep -qi "dry.run\|no worktrees\|clean"; then
        echo "PASS: --dry-run option works"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --dry-run should show dry run output"
    fi
}

test_days_option() {
    ((TEST_COUNT++)) || true

    # Just verify option is accepted
    if "$SCRIPT_UNDER_TEST" --days 7 --help &>/dev/null || "$SCRIPT_UNDER_TEST" --help 2>&1 | grep -q "days"; then
        echo "PASS: --days option recognized"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --days option not recognized"
    fi
}

test_status_option() {
    ((TEST_COUNT++)) || true

    # Just verify option is accepted
    if "$SCRIPT_UNDER_TEST" --status merged --help &>/dev/null || "$SCRIPT_UNDER_TEST" --help 2>&1 | grep -q "status"; then
        echo "PASS: --status option recognized"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: --status option not recognized"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

echo "=== Testing cf-worktree-cleanup.sh ==="
echo ""

test_script_exists_and_executable
test_script_syntax
test_help_option
test_version_option
test_dry_run_option
test_days_option
test_status_option

echo ""
echo "=== Results: $PASS_COUNT/$TEST_COUNT passed ==="

[[ "$PASS_COUNT" -eq "$TEST_COUNT" ]] && exit 0 || exit 1
