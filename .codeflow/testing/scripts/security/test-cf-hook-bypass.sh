#!/usr/bin/env bash
# Test: cf-hook-bypass.sh
# Location: .codeflow/testing/scripts/security/test-cf-hook-bypass.sh
#
# Tests the hook bypass module after consolidation into cf-git-protection.sh.
# cf-hook-bypass.sh is now a no-op — all checks are handled by
# cf-git-protection.sh (Sections 1-4). This test verifies:
#   1. Module exists and has valid syntax
#   2. Module is a documented no-op (passes everything through)
#   3. Consolidation note is present

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-hook-bypass.sh"

export REPO_ROOT LIB_DIR

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "  PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "  FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Helper function to test command allowing
test_allows_command() {
    local command="$1"
    local description="$2"

    if COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        pass "$description"
    else
        fail "$description - Expected allow"
    fi
}

echo "=== Testing cf-hook-bypass.sh (consolidated into cf-git-protection.sh) ==="
echo ""

# =============================================================================
# STRUCTURAL CHECKS
# =============================================================================
echo "--- Structural Checks ---"

if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

if bash -n "$MODULE" 2>/dev/null; then
    pass "Module has valid bash syntax"
else
    fail "Module has invalid bash syntax"
fi

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

if grep -q "CONSOLIDATION NOTE" "$MODULE"; then
    pass "Has consolidation documentation"
else
    fail "Missing consolidation documentation"
fi

if grep -q "cf-git-protection.sh" "$MODULE"; then
    pass "References cf-git-protection.sh as authority"
else
    fail "Should reference cf-git-protection.sh"
fi

if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 (no-op passthrough)"
else
    fail "Should return 0 as no-op"
fi

echo ""

# =============================================================================
# NO-OP VERIFICATION: All commands should pass through
# =============================================================================
echo "--- No-op Verification (all commands should pass through) ---"

test_allows_command "git -c core.hooksPath=/tmp commit -m 'msg'" \
    "Passes through -c core.hooksPath (handled by cf-git-protection.sh)"

test_allows_command "git config --unset core.hooksPath" \
    "Passes through --unset core.hooksPath (handled by cf-git-protection.sh)"

test_allows_command "PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit -m 'msg'" \
    "Passes through PRE_COMMIT_ALLOW_NO_CONFIG (handled by cf-git-protection.sh)"

test_allows_command "rm -rf .git/hooks/pre-commit" \
    "Passes through .git/hooks rm (handled by cf-git-protection.sh)"

test_allows_command "echo 'exit 0' > .git/hooks/pre-commit" \
    "Passes through .git/hooks redirect (handled by cf-git-protection.sh)"

test_allows_command "git commit -m 'fix: proper commit'" \
    "Passes through normal commit"

test_allows_command "git push origin feature/branch" \
    "Passes through normal push"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
