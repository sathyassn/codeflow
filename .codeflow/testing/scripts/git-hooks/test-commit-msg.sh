#!/usr/bin/env bash
# Test: Git commit-msg hook
# Location: .codeflow/testing/scripts/git-hooks/test-commit-msg.sh
#
# Tests the commit-msg hook functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/commit-msg"
TEMP_MSG="/tmp/test-commit-msg-$$"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup (called via trap)
# shellcheck disable=SC2329  # Invoked indirectly via trap
cleanup() {
    rm -f "$TEMP_MSG"
}
trap cleanup EXIT

# Test helper - run hook with message
test_message() {
    local message="$1"
    local expected_exit="$2"
    local description="$3"

    echo "$message" > "$TEMP_MSG"

    # Capture exit code without -e interfering
    set +e
    bash "$HOOK" "$TEMP_MSG" >/dev/null 2>&1
    local actual_exit=$?
    set -e

    if [[ "$actual_exit" -eq "$expected_exit" ]]; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description (expected exit $expected_exit, got $actual_exit)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing Git Commit-Msg Hook ==="
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
# Test 3: Valid conventional commits
# ============================================================================
echo ""
echo "--- Valid commit messages ---"

test_message "feat: add new authentication feature" 0 "Should accept feat type"
test_message "fix: resolve null pointer exception" 0 "Should accept fix type"
test_message "docs: update README with examples" 0 "Should accept docs type"
test_message "refactor: simplify user validation" 0 "Should accept refactor type"
test_message "feat(api): add new endpoint for users" 0 "Should accept type with scope"
test_message "fix(db): correct connection handling" 0 "Should accept scoped fix"

# ============================================================================
# Test 4: Invalid commit messages
# ============================================================================
echo ""
echo "--- Invalid commit messages ---"

test_message "Updated the code" 1 "Should reject non-conventional format"
test_message "feat:missing space" 1 "Should reject missing space after colon"
test_message "feat: short" 1 "Should reject too short description"

# ============================================================================
# Test 5: Skip conditions
# ============================================================================
echo ""
echo "--- Skip conditions ---"

test_message "Merge branch 'feature' into main" 0 "Should skip merge commits"
test_message "Revert \"previous commit\"" 0 "Should skip revert commits"
test_message "fixup! feat: original commit" 0 "Should skip fixup commits"
test_message "squash! feat: original commit" 0 "Should skip squash commits"

# ============================================================================
# Test 6: All commit types are valid
# ============================================================================
echo ""
echo "--- All commit types ---"

VALID_TYPES=("feat" "fix" "docs" "style" "refactor" "test" "chore" "perf" "ci" "build" "revert")
for type in "${VALID_TYPES[@]}"; do
    test_message "$type: test message for validation" 0 "Should accept $type type"
done

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
