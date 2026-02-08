#!/usr/bin/env bash
# Test: Staging workflow - cf-stage-edit.sh
# Location: .codeflow/testing/scripts/security/test-cf-stage-edit.sh
#
# Tests the staging edit functionality for protected files

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/staging/cf-stage-edit.sh"
STAGING_DIR="/tmp/claude/managed/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -rf "$STAGING_DIR" 2>/dev/null || true
    rm -f /tmp/claude/test-stage-*.txt 2>/dev/null || true
}
trap cleanup EXIT

# Pre-clean to ensure fresh state
cleanup

echo "=== Testing cf-stage-edit.sh ==="
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
# Test 5: File not found
# ============================================================================
echo "content" > /tmp/claude/test-stage-content.txt
OUTPUT=$("$SCRIPT" "/nonexistent/file" "/tmp/claude/test-stage-content.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found"; then
    echo "PASS: Errors on nonexistent original file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Successful staging
# ============================================================================
echo ""
echo "--- Staging functionality ---"

# Create test files
echo "original content" > /tmp/claude/test-stage-original.txt
echo "new content" > /tmp/claude/test-stage-new.txt

OUTPUT=$("$SCRIPT" "/tmp/claude/test-stage-original.txt" "/tmp/claude/test-stage-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "staged successfully"; then
    echo "PASS: Staging succeeds"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Staging failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: Staged files created
# ============================================================================
SAFE_NAME=$(echo "/tmp/claude/test-stage-original.txt" | sed 's/[\/]/_/g')

if [[ -f "$STAGING_DIR/${SAFE_NAME}.staged" ]]; then
    echo "PASS: Staged file created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Staged file not created"
    ((TESTS_FAILED++)) || true
fi

if [[ -f "$STAGING_DIR/${SAFE_NAME}.original" ]]; then
    echo "PASS: Original backup created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Original backup not created"
    ((TESTS_FAILED++)) || true
fi

if [[ -f "$STAGING_DIR/${SAFE_NAME}.metadata.json" ]]; then
    echo "PASS: Metadata file created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Metadata file not created"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Duplicate staging blocked
# ============================================================================
OUTPUT=$("$SCRIPT" "/tmp/claude/test-stage-original.txt" "/tmp/claude/test-stage-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "already exists"; then
    echo "PASS: Blocks duplicate staging"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should block duplicate staging"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Shellcheck passes
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
