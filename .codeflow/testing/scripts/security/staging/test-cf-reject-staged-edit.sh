#!/usr/bin/env bash
# Test: Staging workflow - cf-reject-staged-edit.sh
# Location: .codeflow/testing/scripts/security/staging/test-cf-reject-staged-edit.sh
#
# Tests the reject staged edit functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/staging/cf-reject-staged-edit.sh"
STAGE_SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/staging/cf-stage-edit.sh"
STAGING_DIR="/tmp/claude/managed/codeflow/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

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
# Test 6: Unknown option handling
# ============================================================================
OUTPUT=$("$SCRIPT" --invalid-flag 2>&1 || true)
if echo "$OUTPUT" | grep -qi "unknown option"; then
    echo "PASS: Unknown option returns error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Unknown option not handled"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: --reason without value
# ============================================================================
OUTPUT=$("$SCRIPT" -r 2>&1 || true)
if echo "$OUTPUT" | grep -qi "requires a value"; then
    echo "PASS: --reason without value returns error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --reason without value not handled"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Successful rejection
# ============================================================================
echo ""
echo "--- Rejection functionality ---"

# Create and stage a test file
echo "original content" > "$TEST_TMPDIR/test-reject-original.txt"
echo "new content" > "$TEST_TMPDIR/test-reject-new.txt"
"$STAGE_SCRIPT" "$TEST_TMPDIR/test-reject-original.txt" "$TEST_TMPDIR/test-reject-new.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-reject-original.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "rejected\|cleaned"; then
    echo "PASS: Rejection succeeds"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rejection failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: All 3 staging artifacts cleaned up
# ============================================================================
SAFE_NAME=$(echo "$TEST_TMPDIR/test-reject-original.txt" | sed 's/[\/]/_/g')

ARTIFACTS_CLEAN=true
for ext in staged original metadata.json; do
    if [[ -f "$STAGING_DIR/${SAFE_NAME}.${ext}" ]]; then
        ARTIFACTS_CLEAN=false
        echo "FAIL: Artifact not cleaned: ${SAFE_NAME}.${ext}"
    fi
done
if [[ "$ARTIFACTS_CLEAN" == "true" ]]; then
    echo "PASS: All 3 staging artifacts cleaned up"
    ((TESTS_PASSED++)) || true
else
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 10: Original file unchanged
# ============================================================================
if grep -q "original content" "$TEST_TMPDIR/test-reject-original.txt"; then
    echo "PASS: Original file unchanged"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Original file was modified"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Rejection with short reason flag (-r)
# ============================================================================
echo "original3" > "$TEST_TMPDIR/test-reject-original3.txt"
echo "new3" > "$TEST_TMPDIR/test-reject-new3.txt"
"$STAGE_SCRIPT" "$TEST_TMPDIR/test-reject-original3.txt" "$TEST_TMPDIR/test-reject-new3.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" -r "Test rejection reason" "$TEST_TMPDIR/test-reject-original3.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -q "Test rejection reason"; then
    echo "PASS: Rejection with -r flag shows reason"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rejection with -r flag failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Rejection with long reason flag (--reason)
# ============================================================================
echo "original4" > "$TEST_TMPDIR/test-reject-original4.txt"
echo "new4" > "$TEST_TMPDIR/test-reject-new4.txt"
"$STAGE_SCRIPT" "$TEST_TMPDIR/test-reject-original4.txt" "$TEST_TMPDIR/test-reject-new4.txt" >/dev/null 2>&1

OUTPUT=$("$SCRIPT" --reason "Long flag reason" "$TEST_TMPDIR/test-reject-original4.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -q "Long flag reason"; then
    echo "PASS: Rejection with --reason flag shows reason"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Rejection with --reason flag failed"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 13: Exit code 0 on success
# ============================================================================
echo "original5" > "$TEST_TMPDIR/test-reject-original5.txt"
echo "new5" > "$TEST_TMPDIR/test-reject-new5.txt"
"$STAGE_SCRIPT" "$TEST_TMPDIR/test-reject-original5.txt" "$TEST_TMPDIR/test-reject-new5.txt" >/dev/null 2>&1

"$SCRIPT" "$TEST_TMPDIR/test-reject-original5.txt" >/dev/null 2>&1
EXIT_CODE=$?
if [[ $EXIT_CODE -eq 0 ]]; then
    echo "PASS: Exit code 0 on successful rejection"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Exit code was $EXIT_CODE, expected 0"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 14: Exit code 1 on missing staged edit
# ============================================================================
EXIT_CODE=0
"$SCRIPT" "/nonexistent/file" >/dev/null 2>&1 || EXIT_CODE=$?
if [[ $EXIT_CODE -eq 1 ]]; then
    echo "PASS: Exit code 1 on missing staged edit"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Exit code was $EXIT_CODE, expected 1"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 15: Shellcheck passes
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
