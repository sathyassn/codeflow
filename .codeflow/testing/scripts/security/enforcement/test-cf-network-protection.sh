#!/usr/bin/env bash
# Test: cf-network-protection.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-network-protection.sh
#
# Tests the network protection enforcement module

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$ENFORCEMENT_DIR/cf-network-protection.sh"

export REPO_ROOT LIB_DIR

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing cf-network-protection.sh ==="
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

# Test 7: Checks sandbox bypass status
if grep -q 'dangerouslyDisableSandbox' "$MODULE" || grep -q 'SANDBOX_BYPASS' "$MODULE"; then
    pass "Checks sandbox bypass status"
else
    fail "Should check sandbox bypass status"
fi

# Test 8: Handles git network operations
if grep -q 'git' "$MODULE" && grep -q 'network' "$MODULE"; then
    pass "Handles git network operations"
else
    fail "Should handle git network operations"
fi

# Test 9: Handles GitHub CLI operations
if grep -q 'github' "$MODULE" || grep -q 'gh' "$MODULE"; then
    pass "Handles GitHub CLI operations"
else
    fail "Should handle GitHub CLI operations"
fi

# Test 10: Has check_network_pattern function
if grep -q 'check_network_pattern' "$MODULE"; then
    pass "Has check_network_pattern function"
else
    fail "Should have check_network_pattern function"
fi

# Test 11: Has output_network_block function
if grep -q 'output_network_block' "$MODULE"; then
    pass "Has output_network_block function"
else
    fail "Should have output_network_block function"
fi

# Test 12: Uses CONFIG variable for patterns
if grep -q 'CONFIG' "$MODULE"; then
    pass "Uses CONFIG variable"
else
    fail "Should use CONFIG variable"
fi

# Test 13: Uses jq for JSON parsing
if grep -q 'jq' "$MODULE"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 14: Reads from instructions directory
if grep -q 'INSTRUCTIONS_DIR' "$MODULE" || grep -q 'instructions' "$MODULE"; then
    pass "Reads from instructions directory"
else
    fail "Should read from instructions directory"
fi

# Test 15: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 16: Exits with code 2 on block
if grep -q 'exit 2' "$MODULE"; then
    pass "Exits with code 2 on block"
else
    fail "Should exit with code 2 on block"
fi

# Test 17: References .network_operations config path
if grep -q 'network_operations' "$MODULE"; then
    pass "References network_operations config path"
else
    fail "Should reference network_operations config path"
fi

# Test 18: Uses grep for pattern matching
if grep -q 'grep' "$MODULE"; then
    pass "Uses grep for pattern matching"
else
    fail "Should use grep for pattern matching"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
