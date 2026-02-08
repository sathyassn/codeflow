#!/usr/bin/env bash
# Purpose:   Test cf-worktree-setup.sh functionality
# Usage:     bash test-cf-worktree-setup.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-setup.sh"

# Source test helpers
source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"

TEST_COUNT=0
PASS_COUNT=0

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

setup_test() {
    TEST_DIR=$(mktemp -d)
    cd "$TEST_DIR"
    git init --quiet
    git config user.email "test@test.com"
    git config user.name "Test"
    echo "test" > file.txt
    git add file.txt
    git commit -m "Initial commit" --quiet
}

cleanup_test() {
    cd "$REPO_ROOT"
    [[ -n "${TEST_DIR:-}" ]] && rm -rf "$TEST_DIR"
}

# =============================================================================
# TESTS
# =============================================================================

test_help_option() {
    ((TEST_COUNT++)) || true
    local output
    output=$("$SCRIPT_UNDER_TEST" --help 2>&1)

    if echo "$output" | grep -q "cf-worktree-setup.sh"; then
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

test_missing_arguments() {
    ((TEST_COUNT++)) || true

    if ! "$SCRIPT_UNDER_TEST" 2>/dev/null; then
        echo "PASS: Fails with missing arguments"
        ((PASS_COUNT++)) || true
    else
        echo "FAIL: Should fail with missing arguments"
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

# =============================================================================
# MAIN
# =============================================================================

echo "=== Testing cf-worktree-setup.sh ==="
echo ""

test_script_exists_and_executable
test_script_syntax
test_help_option
test_version_option
test_missing_arguments

echo ""
echo "=== Results: $PASS_COUNT/$TEST_COUNT passed ==="

[[ "$PASS_COUNT" -eq "$TEST_COUNT" ]] && exit 0 || exit 1
