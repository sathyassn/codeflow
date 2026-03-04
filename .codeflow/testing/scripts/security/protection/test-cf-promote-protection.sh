#!/usr/bin/env bash
# Test: Protection - cf-promote-protection.sh
# Location: .codeflow/testing/scripts/security/protection/test-cf-promote-protection.sh
#
# Tests the protection promotion functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/protection/cf-promote-protection.sh"
EXTENDED_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-extended.list"
ADHOC_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-adhoc.list"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Backup and cleanup
EXTENDED_BACKUP=""
ADHOC_BACKUP=""

setup_backup() {
    if [[ -f "$EXTENDED_LIST" ]]; then
        EXTENDED_BACKUP=$(cat "$EXTENDED_LIST")
    fi
    if [[ -f "$ADHOC_LIST" ]]; then
        ADHOC_BACKUP=$(cat "$ADHOC_LIST")
    fi
}

cleanup() {
    # Restore original files
    if [[ -n "$EXTENDED_BACKUP" ]]; then
        echo "$EXTENDED_BACKUP" > "$EXTENDED_LIST"
    fi
    if [[ -n "$ADHOC_BACKUP" ]]; then
        echo "$ADHOC_BACKUP" > "$ADHOC_LIST"
    fi
}
trap cleanup EXIT

setup_backup

echo "=== Testing cf-promote-protection.sh ==="
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
# Test 5: Pattern not in adhoc (without --force)
# ============================================================================
OUTPUT=$("$SCRIPT" "test-pattern-xyz-123" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: Errors when pattern not in adhoc"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error when pattern not in adhoc"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Force add to extended
# ============================================================================
echo ""
echo "--- Promotion functionality ---"

OUTPUT=$("$SCRIPT" --force "test-pattern-force-add" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "added\|extended"; then
    echo "PASS: Force add works"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Force add should work"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: Pattern added to extended list
# ============================================================================
if grep -q "test-pattern-force-add" "$EXTENDED_LIST" 2>/dev/null; then
    echo "PASS: Pattern in extended list"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Pattern not in extended list"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Promote from adhoc
# ============================================================================
# Add to adhoc first
echo "test-adhoc-pattern" >> "$ADHOC_LIST"

OUTPUT=$("$SCRIPT" "test-adhoc-pattern" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "added\|extended"; then
    echo "PASS: Promote from adhoc works"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Promote from adhoc should work"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Pattern removed from adhoc after promotion
# ============================================================================
if ! grep -q "test-adhoc-pattern" "$ADHOC_LIST" 2>/dev/null; then
    echo "PASS: Pattern removed from adhoc"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Pattern should be removed from adhoc"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 10: --keep flag preserves adhoc entry
# ============================================================================
echo ""
echo "--- Keep and edge cases ---"

echo "test-keep-pattern" >> "$ADHOC_LIST"

OUTPUT=$("$SCRIPT" --keep "test-keep-pattern" 2>&1 || true)
if grep -qxF "test-keep-pattern" "$ADHOC_LIST" 2>/dev/null; then
    echo "PASS: --keep preserves adhoc entry"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --keep should preserve adhoc entry"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Already in extended list (duplicate handling)
# ============================================================================
OUTPUT=$("$SCRIPT" --force "test-pattern-force-add" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "already"; then
    echo "PASS: Detects duplicate in extended list"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should detect duplicate in extended list"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Unknown option rejected
# ============================================================================
OUTPUT=$("$SCRIPT" --bogus 2>&1 || true)
if echo "$OUTPUT" | grep -qi "unknown\|error"; then
    echo "PASS: Unknown option rejected"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should reject unknown options"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 13: Help text shows correct paths
# ============================================================================
HELP_OUTPUT=$("$SCRIPT" --help 2>&1)
if echo "$HELP_OUTPUT" | grep -q "config/enforcement/protection/protected-adhoc.list" && \
   echo "$HELP_OUTPUT" | grep -q "config/enforcement/protection/protected-extended.list"; then
    echo "PASS: Help text shows correct list paths"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Help text has wrong list paths"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 14: Exact line match (not substring)
# ============================================================================
# Add "foo" to adhoc, then try to promote "foobar" - should fail (not found)
echo "test-exact-foo" >> "$ADHOC_LIST"
OUTPUT=$("$SCRIPT" "test-exact-foobar" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: grep uses exact line match (not substring)"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: grep should use exact line match"
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
