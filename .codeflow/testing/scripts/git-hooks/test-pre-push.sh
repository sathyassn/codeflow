#!/usr/bin/env bash
# Test: Git pre-push hook
# Location: .codeflow/testing/scripts/git-hooks/test-pre-push.sh
#
# Tests the pre-push hook functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-push"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test helper
check_pattern() {
    local pattern="$1"
    local description="$2"

    if grep -qE "$pattern" "$HOOK"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing Git Pre-Push Hook ==="
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
# Test 3: Protected branches defined
# ============================================================================
echo ""
echo "--- Protected branch checks ---"

check_pattern "main" "Should protect main branch"
check_pattern "master" "Should protect master branch"
check_pattern "production" "Should protect production branch"

# ============================================================================
# Test 4: Force push detection
# ============================================================================
echo ""
echo "--- Force push detection ---"

check_pattern "merge-base.*ancestor" "Should check ancestry for force push"
check_pattern "Force push" "Should have force push warning message"

# ============================================================================
# Test 5: Valid branch prefixes defined
# ============================================================================
echo ""
echo "--- Branch naming validation ---"

check_pattern "feat/" "Should recognize feat/ prefix"
check_pattern "fix/" "Should recognize fix/ prefix"
check_pattern "docs/" "Should recognize docs/ prefix"
check_pattern "refactor/" "Should recognize refactor/ prefix"
check_pattern "test/" "Should recognize test/ prefix"
check_pattern "chore/" "Should recognize chore/ prefix"

# ============================================================================
# Test 6: Proper argument handling
# ============================================================================
echo ""
echo "--- Argument handling ---"

# shellcheck disable=SC2016  # Single quotes intentional to match literal pattern
check_pattern 'REMOTE="\$1"' "Should capture remote argument"
# shellcheck disable=SC2016  # Single quotes intentional to match literal pattern
check_pattern 'URL="\$2"' "Should capture URL argument"

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
