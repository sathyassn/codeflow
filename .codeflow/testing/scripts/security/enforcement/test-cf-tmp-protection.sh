#!/usr/bin/env bash
# Test: cf-tmp-protection.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-tmp-protection.sh
#
# Tests the managed tmp protection enforcement module (Section 11)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$ENFORCEMENT_DIR/cf-tmp-protection.sh"

export REPO_ROOT LIB_DIR

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing cf-tmp-protection.sh ==="
echo ""

# Test 1: File exists
if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

# Test 2: File is executable
if [[ -x "$MODULE" ]]; then pass "Module is executable"; else fail "Module not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Has proper header comments
if grep -q "Purpose:" "$MODULE" && grep -q "Exit codes:" "$MODULE"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Uses set -euo pipefail
if grep -q "set -euo pipefail" "$MODULE"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Sources security-lib.sh
if grep -q 'source.*security-lib.sh' "$MODULE"; then
    pass "Sources security-lib.sh"
else
    fail "Should source security-lib.sh"
fi

# Test 7: Has Section 11 - Managed Tmp Protection
if grep -q "SECTION 11" "$MODULE" || grep -q "Managed Tmp Protection" "$MODULE"; then
    pass "Has Section 11 - Managed Tmp Protection"
else
    fail "Missing Section 11 - Managed Tmp Protection"
fi

# Test 8: Uses MANAGED_TMP_FOLDERS array
if grep -q 'MANAGED_TMP_FOLDERS' "$MODULE"; then
    pass "Uses MANAGED_TMP_FOLDERS array"
else
    fail "Should use MANAGED_TMP_FOLDERS array"
fi

# Test 9: Uses STATE_FOLDER variable
if grep -q 'STATE_FOLDER' "$MODULE"; then
    pass "Uses STATE_FOLDER variable"
else
    fail "Should use STATE_FOLDER variable"
fi

# Test 10: Detects rm command
if grep -q 'rm' "$MODULE"; then
    pass "Detects rm command"
else
    fail "Should detect rm command"
fi

# Test 11: Detects rmdir command
if grep -q 'rmdir' "$MODULE"; then
    pass "Detects rmdir command"
else
    fail "Should detect rmdir command"
fi

# Test 12: Detects mv command
if grep -q 'mv' "$MODULE"; then
    pass "Detects mv command"
else
    fail "Should detect mv command"
fi

# Test 13: Handles rm -rf pattern
if grep -q 'rm.*-.*r.*f' "$MODULE" || grep -q 'rm.*-.*f.*r' "$MODULE"; then
    pass "Handles rm -rf pattern"
else
    fail "Should handle rm -rf pattern"
fi

# Test 14: Uses block_with_skill function
if grep -q 'block_with_skill' "$MODULE"; then
    pass "Uses block_with_skill function"
else
    fail "Should use block_with_skill function"
fi

# Test 15: References security-management skill (without cf- prefix)
if grep -q '"security-management"' "$MODULE"; then
    pass "References security-management skill"
else
    fail "Should reference security-management skill (without cf- prefix)"
fi

# Test 16: References diagnose-permission-error operation
if grep -q 'diagnose-permission-error' "$MODULE"; then
    pass "References diagnose-permission-error operation"
else
    fail "Should reference diagnose-permission-error operation"
fi

# Test 17: Protects state files from deletion
if grep -q 'State.*Protection' "$MODULE" || grep -q 'state.*file' "$MODULE"; then
    pass "Protects state files from deletion"
else
    fail "Should protect state files from deletion"
fi

# Test 18: Detects unlink command
if grep -q 'unlink' "$MODULE"; then
    pass "Detects unlink command"
else
    fail "Should detect unlink command"
fi

# Test 19: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 20: Documents managed tmp structure
if grep -q '/tmp/claude/managed' "$MODULE"; then
    pass "Documents managed tmp structure"
else
    fail "Should document managed tmp structure"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
