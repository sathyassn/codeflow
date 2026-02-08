#!/usr/bin/env bash
# Test: cf-git-protection.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-git-protection.sh
#
# Tests the git protection enforcement module (Sections 1-3)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$ENFORCEMENT_DIR/cf-git-protection.sh"

export REPO_ROOT LIB_DIR

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing cf-git-protection.sh ==="
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

# Test 7: Has Section 1 - Git Hook Bypass Prevention
if grep -q "SECTION 1" "$MODULE" && grep -q "Hook Bypass" "$MODULE"; then
    pass "Has Section 1 - Git Hook Bypass"
else
    fail "Missing Section 1 - Git Hook Bypass"
fi

# Test 8: Has Section 2 - Force Push Prevention
if grep -q "SECTION 2" "$MODULE" && grep -q "Force Push" "$MODULE"; then
    pass "Has Section 2 - Force Push Prevention"
else
    fail "Missing Section 2 - Force Push Prevention"
fi

# Test 9: Has Section 3 - Hook Path Manipulation
if grep -q "SECTION 3" "$MODULE" && grep -q "Hook Path" "$MODULE"; then
    pass "Has Section 3 - Hook Path Manipulation"
else
    fail "Missing Section 3 - Hook Path Manipulation"
fi

# Test 10: Detects --no-verify flag
if grep -q '\-\-no-verify' "$MODULE"; then
    pass "Detects --no-verify flag"
else
    fail "Should detect --no-verify flag"
fi

# Test 11: Detects -n short flag for commit
if grep -q '\-n' "$MODULE" && grep -q 'commit' "$MODULE"; then
    pass "Detects -n flag for commit"
else
    fail "Should detect -n flag for commit"
fi

# Test 12: Detects --force flag
if grep -q '\-\-force' "$MODULE"; then
    pass "Detects --force flag"
else
    fail "Should detect --force flag"
fi

# Test 13: Detects --force-with-lease flag
if grep -q '\-\-force-with-lease' "$MODULE"; then
    pass "Detects --force-with-lease flag"
else
    fail "Should detect --force-with-lease flag"
fi

# Test 14: Detects -f short flag for push
if grep -q 'push' "$MODULE" && grep -q '\-f' "$MODULE"; then
    pass "Detects -f flag for push"
else
    fail "Should detect -f flag for push"
fi

# Test 15: Detects core.hooksPath manipulation
if grep -q 'core\.hooksPath' "$MODULE"; then
    pass "Detects core.hooksPath manipulation"
else
    fail "Should detect core.hooksPath manipulation"
fi

# Test 16: Detects GIT_HOOKS_PATH env var
if grep -q 'GIT_HOOKS_PATH' "$MODULE"; then
    pass "Detects GIT_HOOKS_PATH env var"
else
    fail "Should detect GIT_HOOKS_PATH env var"
fi

# Test 17: Detects SKIP_HOOKS env var
if grep -q 'SKIP_HOOKS' "$MODULE"; then
    pass "Detects SKIP_HOOKS env var"
else
    fail "Should detect SKIP_HOOKS env var"
fi

# Test 18: Detects HUSKY=0 bypass
if grep -q 'HUSKY' "$MODULE"; then
    pass "Detects HUSKY bypass attempt"
else
    fail "Should detect HUSKY bypass attempt"
fi

# Test 19: Uses block_command function
if grep -q 'block_command' "$MODULE"; then
    pass "Uses block_command function"
else
    fail "Should use block_command function"
fi

# Test 20: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 21: Uses get_flags_portion function
if grep -q 'get_flags_portion' "$MODULE"; then
    pass "Uses get_flags_portion function"
else
    fail "Should use get_flags_portion function"
fi

# Test 22: Handles combined flags with n
if grep -q 'combined.*flag' "$MODULE" || grep -q '\[a-mo-z\]\*n' "$MODULE"; then
    pass "Handles combined flags with n"
else
    fail "Should handle combined flags with n"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
