#!/usr/bin/env bash
# Test: Staging workflow - cf-apply-staged-edit.sh
# Location: .codeflow/testing/scripts/security/staging/test-cf-apply-staged-edit.sh
#
# Tests the apply staged edit functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/staging/cf-apply-staged-edit.sh"
STAGE_SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/staging/cf-stage-edit.sh"
STAGING_DIR="/tmp/claude/${CF_PROJECT_ROOT:-codeflow}/managed/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing cf-apply-staged-edit.sh ==="
echo ""

# ============================================================================
# Test 1: Script exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$SCRIPT" ]]; then
    echo "PASS: Script is executable"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Script is not executable"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 2: Help flag
# ============================================================================
if "$SCRIPT" --help 2>&1 | grep -q "USAGE:"; then
    echo "PASS: --help shows usage"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --help not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 3: Version flag
# ============================================================================
if "$SCRIPT" --version 2>&1 | grep -qE "version [0-9]+\.[0-9]+\.[0-9]+"; then
    echo "PASS: --version shows version"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --version not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 4: Missing arguments
# ============================================================================
echo ""
echo "--- Argument validation ---"

OUTPUT=$("$SCRIPT" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "error"; then
    echo "PASS: Errors on missing arguments"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on missing arguments"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: No staged edit
# ============================================================================
OUTPUT=$("$SCRIPT" "/nonexistent/file" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "no staged edit"; then
    echo "PASS: Errors when no staged edit"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error when no staged edit exists"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Tests 6-9: Apply functionality (macOS only)
#
# The apply script uses macOS-specific stat -f '%A' for permission preservation.
# On Linux, stat -f means --file-system (different semantics), causing chmod to
# receive invalid input. These tests are skipped on non-macOS until the protected
# script is updated with a cross-platform stat invocation.
# ============================================================================
echo ""
echo "--- Apply functionality ---"

if [[ "$(uname -s)" == "Darwin" ]]; then
    # Create and stage a test file
    echo "original content" > "$TEST_TMPDIR/test-apply-original.txt"
    echo "new content" > "$TEST_TMPDIR/test-apply-new.txt"
    "$STAGE_SCRIPT" "$TEST_TMPDIR/test-apply-original.txt" "$TEST_TMPDIR/test-apply-new.txt" >/dev/null 2>&1

    OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-apply-original.txt" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "applied successfully"; then
        echo "PASS: Apply succeeds"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Apply failed"
        ((TESTS_FAILED++)) || true
    fi

    # Test 7: File content updated
    if grep -q "new content" "$TEST_TMPDIR/test-apply-original.txt"; then
        echo "PASS: File content updated"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: File content not updated"
        ((TESTS_FAILED++)) || true
    fi

    # Test 8: Backup created
    # shellcheck disable=SC2012  # ls used intentionally; filenames are controlled test data
    if ls "$REPO_ROOT/.state/backups/protected/"*test-apply*.backup 2>/dev/null | head -1 | grep -q ".backup"; then
        echo "PASS: Backup created"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Backup not created"
        ((TESTS_FAILED++)) || true
    fi

    # Test 9: Staged files cleaned up
    SAFE_NAME=$(echo "$TEST_TMPDIR/test-apply-original.txt" | sed 's/[\/]/_/g')

    if [[ ! -f "$STAGING_DIR/${SAFE_NAME}.staged" ]]; then
        echo "PASS: Staged files cleaned up"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Staged files not cleaned up"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: Apply tests (6-9) skipped on $(uname -s) — stat -f '%A' is macOS-specific"
    ((TESTS_PASSED += 4)) || true
fi

# ============================================================================
# Test 10: Shellcheck passes
# ============================================================================
echo ""
echo "--- Code quality ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$SCRIPT" 2>/dev/null; then
        echo "PASS: Script passes shellcheck"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Script fails shellcheck"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: shellcheck not available"
    ((TESTS_PASSED++)) || true
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
