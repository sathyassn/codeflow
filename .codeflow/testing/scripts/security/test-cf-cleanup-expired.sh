#!/usr/bin/env bash
# Test: Staging workflow - cf-cleanup-expired.sh
# Location: .codeflow/testing/scripts/security/test-cf-cleanup-expired.sh
#
# Tests the cleanup functionality for expired staged edits

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-cleanup-expired.sh"
STAGING_DIR="/tmp/claude/managed/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -rf "$STAGING_DIR" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Testing cf-cleanup-expired.sh ==="
echo ""

# ============================================================================
# Test 1: Script exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$SCRIPT" ]]; then
    echo "PASS: Script is executable"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Script is not executable"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 2: Help flag
# ============================================================================
if "$SCRIPT" --help 2>&1 | grep -q "USAGE:"; then
    echo "PASS: --help shows usage"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: --help not working"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 3: Version flag
# ============================================================================
if "$SCRIPT" --version 2>&1 | grep -qE "version [0-9]+\.[0-9]+\.[0-9]+"; then
    echo "PASS: --version shows version"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: --version not working"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 4: No staging directory
# ============================================================================
echo ""
echo "--- Cleanup functionality ---"

rm -rf "$STAGING_DIR"
if "$SCRIPT" 2>&1; then
    echo "PASS: Handles missing staging directory"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Should handle missing staging directory"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 5: Dry run mode
# ============================================================================
mkdir -p "$STAGING_DIR"

# Create an expired staged edit (use 2020 which is definitely in the past)
PAST_TIME="2020-01-01T00:00:00Z"
cat > "$STAGING_DIR/test_expired.metadata.json" <<EOF
{
    "file_path": "test/expired.txt",
    "expires_at": "$PAST_TIME"
}
EOF
touch "$STAGING_DIR/test_expired.staged"
touch "$STAGING_DIR/test_expired.original"

OUTPUT=$("$SCRIPT" --dry-run 2>&1 || true)
if echo "$OUTPUT" | grep -qi "dry run"; then
    echo "PASS: Dry run mode works"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Dry run mode should report"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Verify files still exist after dry run
if [[ -f "$STAGING_DIR/test_expired.staged" ]]; then
    echo "PASS: Dry run doesn't delete files"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Dry run should not delete files"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 6: Verbose mode
# ============================================================================
# Recreate expired file for verbose test
cat > "$STAGING_DIR/test_verbose.metadata.json" <<EOF
{
    "file_path": "test/verbose.txt",
    "expires_at": "2020-01-01T00:00:00Z"
}
EOF
touch "$STAGING_DIR/test_verbose.staged"
touch "$STAGING_DIR/test_verbose.original"

OUTPUT=$("$SCRIPT" --verbose 2>&1 || true)
if echo "$OUTPUT" | grep -qi "complete\|expired\|cleaned"; then
    echo "PASS: Verbose mode provides output"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Verbose mode should provide detailed output"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 7: Shellcheck passes
# ============================================================================
echo ""
echo "--- Code quality ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$SCRIPT" 2>/dev/null; then
        echo "PASS: Script passes shellcheck"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Script fails shellcheck"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
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
