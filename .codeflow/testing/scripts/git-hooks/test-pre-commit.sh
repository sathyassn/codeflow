#!/usr/bin/env bash
# Test: Git pre-commit hook
# Location: .codeflow/testing/scripts/git-hooks/test-pre-commit.sh
#
# Tests the pre-commit hook functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-commit"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing Git Pre-Commit Hook ==="
echo ""

# ============================================================================
# Test 1: Hook exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$HOOK" ]]; then
    echo "PASS: Hook is executable"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Hook is not executable"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 2: Shellcheck passes
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        echo "PASS: Hook passes shellcheck"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Hook fails shellcheck"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# ============================================================================
# Test 3: Sensitive file patterns are defined
# ============================================================================
echo ""
echo "--- Sensitive file detection ---"

if grep -q "\.env" "$HOOK"; then
    echo "PASS: .env pattern defined"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: .env pattern not found"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

if grep -q "\.pem" "$HOOK"; then
    echo "PASS: .pem pattern defined"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: .pem pattern not found"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

if grep -q "id_rsa" "$HOOK"; then
    echo "PASS: id_rsa pattern defined"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: id_rsa pattern not found"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 4: Shell script linting check present
# ============================================================================
echo ""
echo "--- Linting integration ---"

if grep -q "shellcheck" "$HOOK"; then
    echo "PASS: shellcheck integration present"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: shellcheck integration missing"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 5: Python linting check present
# ============================================================================
if grep -q "ruff\|flake8" "$HOOK"; then
    echo "PASS: Python linting integration present"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Python linting integration missing"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 6: JSON validation check present
# ============================================================================
if grep -q "jq empty" "$HOOK"; then
    echo "PASS: JSON validation present"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: JSON validation missing"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 7: Proper exit codes
# ============================================================================
echo ""
echo "--- Exit code handling ---"

if grep -q "exit 0" "$HOOK" && grep -q "exit 1" "$HOOK"; then
    echo "PASS: Proper exit codes used"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Missing proper exit codes"
    TESTS_FAILED=$((TESTS_FAILED + 1))
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
