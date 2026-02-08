#!/usr/bin/env bash
# Test: Staging workflow - cf-rollback-edit.sh
# Location: .codeflow/testing/scripts/security/test-cf-rollback-edit.sh
#
# Tests the rollback edit functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-rollback-edit.sh"
STAGE_SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-stage-edit.sh"
APPLY_SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-apply-staged-edit.sh"
STAGING_DIR="/tmp/claude/managed/protected-edits"
BACKUP_DIR="$REPO_ROOT/.state/backups/protected"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -rf "$STAGING_DIR" 2>/dev/null || true
    rm -rf "$BACKUP_DIR" 2>/dev/null || true
    rm -f /tmp/test-rollback-*.txt 2>/dev/null || true
}
trap cleanup EXIT

# Pre-clean
cleanup

echo "=== Testing cf-rollback-edit.sh ==="
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
# Test 5: No backup to rollback
# ============================================================================
OUTPUT=$("$SCRIPT" "/nonexistent/file" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "no backup"; then
    echo "PASS: Errors when no backup exists"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error when no backup exists"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: List backups option
# ============================================================================
echo ""
echo "--- Rollback functionality ---"

# Create, stage, and apply to create a backup
echo "original content" > /tmp/test-rollback-original.txt
echo "new content" > /tmp/test-rollback-new.txt
"$STAGE_SCRIPT" "/tmp/test-rollback-original.txt" "/tmp/test-rollback-new.txt" >/dev/null 2>&1
"$APPLY_SCRIPT" "/tmp/test-rollback-original.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" --list "/tmp/test-rollback-original.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "backup"; then
    echo "PASS: List option shows backups"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: List option should show backups"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: Successful rollback
# ============================================================================
OUTPUT=$("$SCRIPT" "/tmp/test-rollback-original.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "rollback.*success\|restored"; then
    echo "PASS: Rollback succeeds"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rollback failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Content restored
# ============================================================================
if grep -q "original content" /tmp/test-rollback-original.txt; then
    echo "PASS: Original content restored"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Content not restored properly"
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
