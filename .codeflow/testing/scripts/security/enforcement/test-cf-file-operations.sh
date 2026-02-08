#!/usr/bin/env bash
# Test: cf-file-operations.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-file-operations.sh
#
# Tests the file operations enforcement module (Sections 8-10)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$ENFORCEMENT_DIR/cf-file-operations.sh"

export REPO_ROOT LIB_DIR

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing cf-file-operations.sh ==="
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

# Test 7: Has Section 8 - Script Execution from Hook Directories
if grep -q "SECTION 8" "$MODULE" && grep -q "Script Execution" "$MODULE"; then
    pass "Has Section 8 - Script Execution"
else
    fail "Missing Section 8 - Script Execution"
fi

# Test 8: Has Section 9 - Indirect File Operations
if grep -q "SECTION 9" "$MODULE" && grep -q "Indirect File Operations" "$MODULE"; then
    pass "Has Section 9 - Indirect File Operations"
else
    fail "Missing Section 9 - Indirect File Operations"
fi

# Test 9: Has Section 10 - Glob Pattern Bypass Prevention
if grep -q "SECTION 10" "$MODULE" && grep -q "Glob Pattern Bypass" "$MODULE"; then
    pass "Has Section 10 - Glob Pattern Bypass"
else
    fail "Missing Section 10 - Glob Pattern Bypass"
fi

# Test 10: Checks EXECUTION_BLOCKED_PATHS array
if grep -q 'EXECUTION_BLOCKED_PATHS' "$MODULE"; then
    pass "Uses EXECUTION_BLOCKED_PATHS array"
else
    fail "Should use EXECUTION_BLOCKED_PATHS array"
fi

# Test 11: Checks PROTECTED_PATHS array
if grep -q 'PROTECTED_PATHS' "$MODULE"; then
    pass "Uses PROTECTED_PATHS array"
else
    fail "Should use PROTECTED_PATHS array"
fi

# Test 12: Uses block_command function
if grep -q 'block_command' "$MODULE"; then
    pass "Uses block_command function"
else
    fail "Should use block_command function"
fi

# Test 13: Checks INDIRECT_WRITE_CMDS
if grep -q 'INDIRECT_WRITE_CMDS' "$MODULE"; then
    pass "Uses INDIRECT_WRITE_CMDS pattern"
else
    fail "Should use INDIRECT_WRITE_CMDS pattern"
fi

# Test 14: Handles dd command specially
if grep -q 'dd[[:space:]]' "$MODULE" && grep -q 'of=' "$MODULE"; then
    pass "Handles dd command with of= parameter"
else
    fail "Should handle dd command with of= parameter"
fi

# Test 15: Handles piped tee
if grep -q 'tee' "$MODULE" && grep -q '\|' "$MODULE"; then
    pass "Handles piped tee command"
else
    fail "Should handle piped tee command"
fi

# Test 16: Handles cat append redirection
if grep -q 'cat' "$MODULE" && grep -q 'append' "$MODULE"; then
    pass "Handles cat append redirection"
else
    fail "Should handle cat append redirection"
fi

# Test 17: Handles glob patterns (* and ?)
if grep -q '\*' "$MODULE" && grep -q '?' "$MODULE"; then
    pass "Handles glob patterns (* and ?)"
else
    fail "Should handle glob patterns"
fi

# Test 18: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 19: Skips /tmp/claude destination
if grep -q '/tmp/claude' "$MODULE"; then
    pass "Skips /tmp/claude destination"
else
    fail "Should skip /tmp/claude destination"
fi

# Test 20: Uses is_path_or_glob_targeted function
if grep -q 'is_path_or_glob_targeted' "$MODULE"; then
    pass "Uses is_path_or_glob_targeted function"
else
    fail "Should use is_path_or_glob_targeted function"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
