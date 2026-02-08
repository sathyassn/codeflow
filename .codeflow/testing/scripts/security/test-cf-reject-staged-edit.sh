#!/usr/bin/env bash
# Test: Staging workflow - cf-reject-staged-edit.sh
# Location: .codeflow/testing/scripts/security/test-cf-reject-staged-edit.sh
#
# Tests the reject staged edit functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-reject-staged-edit.sh"
STAGE_SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-stage-edit.sh"
STAGING_DIR="/tmp/claude/managed/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -rf "$STAGING_DIR" 2>/dev/null || true
    rm -f /tmp/test-reject-*.txt 2>/dev/null || true
}
trap cleanup EXIT

# Pre-clean
cleanup

echo "=== Testing cf-reject-staged-edit.sh ==="
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
# Test 5: No staged edit to reject
# ============================================================================
OUTPUT=$("$SCRIPT" "/nonexistent/file" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "no staged edit"; then
    echo "PASS: Errors when no staged edit exists"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error when no staged edit exists"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Successful rejection
# ============================================================================
echo ""
echo "--- Rejection functionality ---"

# Create and stage a test file
echo "original content" > /tmp/test-reject-original.txt
echo "new content" > /tmp/test-reject-new.txt
"$STAGE_SCRIPT" "/tmp/test-reject-original.txt" "/tmp/test-reject-new.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" "/tmp/test-reject-original.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "rejected\|cleaned"; then
    echo "PASS: Rejection succeeds"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rejection failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: Staged files cleaned up
# ============================================================================
SAFE_NAME=$(echo "/tmp/test-reject-original.txt" | sed 's/[\/]/_/g')

if [[ ! -f "$STAGING_DIR/${SAFE_NAME}.staged" ]]; then
    echo "PASS: Staged files cleaned up after rejection"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Staged files not cleaned up"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Original file unchanged
# ============================================================================
if grep -q "original content" /tmp/test-reject-original.txt; then
    echo "PASS: Original file unchanged"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Original file was modified"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Rejection with reason
# ============================================================================
echo "original2" > /tmp/test-reject-original2.txt
echo "new2" > /tmp/test-reject-new2.txt
"$STAGE_SCRIPT" "/tmp/test-reject-original2.txt" "/tmp/test-reject-new2.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" -r "Test rejection reason" "/tmp/test-reject-original2.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "rejected\|reason"; then
    echo "PASS: Rejection with reason works"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rejection with reason failed"
    ((TESTS_FAILED++)) || true
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
