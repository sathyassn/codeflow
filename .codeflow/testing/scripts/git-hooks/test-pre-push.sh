#!/usr/bin/env bash
# Test: Git pre-push hook
# Location: .codeflow/testing/scripts/git-hooks/test-pre-push.sh
#
# Tests the pre-push hook functionality:
#   - Pattern checks (grep-based, existing)
#   - Behavioral tests (actually runs the hook)

set -euo pipefail

# Setup
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
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

# ============================================================================
# Test 8: Config-driven pattern checks
# ============================================================================
echo ""
echo "--- Config-driven checks ---"

check_pattern "CONFIG_FILE" "Should reference config file"
check_pattern "enforcement-policy.json" "Should use enforcement-policy.json"
check_pattern "jq" "Should use jq for config parsing"
check_pattern "DEFAULT_PROTECTED_BRANCHES" "Should have fallback protected branches"
check_pattern "DEFAULT_VALID_PREFIXES" "Should have fallback valid prefixes"

# ############################################################################
#
# BEHAVIORAL TESTS - Actually run the hook and verify exit codes/output
#
# ############################################################################

echo ""
echo "========================================="
echo "=== Behavioral Tests (hook execution) ==="
echo "========================================="

# Setup for behavioral tests
BEHAV_TEST_DIR="/tmp/claude/test-prepush-$$"
mkdir -p "$BEHAV_TEST_DIR/mock-bin"

# Cleanup on exit
cleanup_behavioral() {
    rm -rf "$BEHAV_TEST_DIR"
}
trap cleanup_behavioral EXIT

# --------------------------------------------------------------------------
# Helper: create a mock git script with specified behavior
#
# Arguments:
#   $1 - mock git behavior: "ancestor", "not-ancestor", "passthrough"
#   $2 - mock branch name for `git branch --show-current`
# --------------------------------------------------------------------------
create_mock_git() {
    local mock_mode="$1"
    local mock_branch="$2"

    cat > "$BEHAV_TEST_DIR/mock-bin/git" << MOCKEOF
#!/bin/bash
if [[ "\$1" == "rev-parse" && "\$2" == "--show-toplevel" ]]; then
    echo "$REPO_ROOT"
    exit 0
fi
if [[ "\$1" == "branch" && "\$2" == "--show-current" ]]; then
    echo "$mock_branch"
    exit 0
fi
if [[ "\$1" == "merge-base" && "\$2" == "--is-ancestor" ]]; then
    if [[ "$mock_mode" == "ancestor" ]]; then
        exit 0
    elif [[ "$mock_mode" == "not-ancestor" ]]; then
        exit 1
    fi
fi
/usr/bin/git "\$@"
MOCKEOF
    chmod +x "$BEHAV_TEST_DIR/mock-bin/git"
}

# Helper to test hook and check result
test_prepush() {
    local description="$1"
    local expected_exit="$2"
    local stdin_line="$3"
    local remote_name="${4:-origin}"
    local remote_url="${5:-https://github.com/test/repo.git}"
    local mock_mode="${6:-passthrough}"
    local mock_branch="${7:-feat/test-branch}"

    create_mock_git "$mock_mode" "$mock_branch"

    local actual_exit=0
    local output=""
    output=$(echo "$stdin_line" | PATH="$BEHAV_TEST_DIR/mock-bin:$PATH" bash "$HOOK" "$remote_name" "$remote_url" 2>&1) || actual_exit=$?

    if [[ "$actual_exit" -eq "$expected_exit" ]]; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description (expected exit $expected_exit, got $actual_exit)"
        echo "  Output: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper to test hook and check both result and output content
test_prepush_with_output() {
    local description="$1"
    local expected_exit="$2"
    local expected_output="$3"
    local stdin_line="$4"
    local remote_name="${5:-origin}"
    local remote_url="${6:-https://github.com/test/repo.git}"
    local mock_mode="${7:-passthrough}"
    local mock_branch="${8:-feat/test-branch}"

    create_mock_git "$mock_mode" "$mock_branch"

    local actual_exit=0
    local output=""
    output=$(echo "$stdin_line" | PATH="$BEHAV_TEST_DIR/mock-bin:$PATH" bash "$HOOK" "$remote_name" "$remote_url" 2>&1) || actual_exit=$?

    if [[ "$actual_exit" -eq "$expected_exit" ]] && echo "$output" | grep -qi "$expected_output"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    elif [[ "$actual_exit" -ne "$expected_exit" ]]; then
        echo "FAIL: $description (expected exit $expected_exit, got $actual_exit)"
        echo "  Output: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    else
        echo "FAIL: $description (output missing: $expected_output)"
        echo "  Output: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Common SHA values for tests
ZERO_SHA="0000000000000000000000000000000000000000"
FAKE_LOCAL="abc1234567890abc1234567890abc1234567890ab"
FAKE_REMOTE="def4567890abc1234567890abc1234567890abcde"

# ============================================================================
# Behavioral Test 1: Force push to protected branch "main" (should BLOCK)
# ============================================================================
echo ""
echo "--- Force push detection (behavioral) ---"

test_prepush_with_output \
    "Should block force push to main" \
    1 \
    "Force push" \
    "refs/heads/main $FAKE_LOCAL refs/heads/main $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "not-ancestor" \
    "main"

# ============================================================================
# Behavioral Test 2: Force push to "master" (should BLOCK)
# ============================================================================
test_prepush_with_output \
    "Should block force push to master" \
    1 \
    "Force push" \
    "refs/heads/master $FAKE_LOCAL refs/heads/master $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "not-ancestor" \
    "master"

# ============================================================================
# Behavioral Test 3: Force push to "production" (should BLOCK)
# ============================================================================
test_prepush_with_output \
    "Should block force push to production" \
    1 \
    "Force push" \
    "refs/heads/production $FAKE_LOCAL refs/heads/production $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "not-ancestor" \
    "production"

# ============================================================================
# Behavioral Test 4: Normal push to feature branch (should ALLOW)
# ============================================================================
echo ""
echo "--- Normal push (behavioral) ---"

test_prepush \
    "Should allow normal push to feat/my-feature" \
    0 \
    "refs/heads/feat/my-feature $FAKE_LOCAL refs/heads/feat/my-feature $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "ancestor" \
    "feat/my-feature"

# ============================================================================
# Behavioral Test 5: Normal push to fix branch (should ALLOW)
# ============================================================================
test_prepush \
    "Should allow normal push to fix/bug-123" \
    0 \
    "refs/heads/fix/bug-123 $FAKE_LOCAL refs/heads/fix/bug-123 $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "ancestor" \
    "fix/bug-123"

# ============================================================================
# Behavioral Test 6: Branch delete (all-zero local SHA) should skip checks
# ============================================================================
echo ""
echo "--- Branch delete (behavioral) ---"

test_prepush \
    "Should allow branch delete (zero SHA skips checks)" \
    0 \
    "refs/heads/feat/old-branch $ZERO_SHA refs/heads/feat/old-branch $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "passthrough" \
    "feat/test-branch"

# ============================================================================
# Behavioral Test 7: Non-standard branch name (should warn but ALLOW)
# ============================================================================
echo ""
echo "--- Non-standard branch name (behavioral) ---"

test_prepush_with_output \
    "Should warn on non-standard branch name but allow" \
    0 \
    "naming convention" \
    "refs/heads/my-random-branch $FAKE_LOCAL refs/heads/my-random-branch $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "ancestor" \
    "my-random-branch"

# ============================================================================
# Behavioral Test 8: Normal push to protected branch (not force, should BLOCK)
# ============================================================================
echo ""
echo "--- Direct push to protected branch (behavioral) ---"

test_prepush_with_output \
    "Should block direct push to main (non-force)" \
    1 \
    "Direct push" \
    "refs/heads/main $FAKE_LOCAL refs/heads/main $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "ancestor" \
    "main"

# ============================================================================
# Behavioral Test 9: New branch push (remote SHA is all zeros)
# ============================================================================
echo ""
echo "--- New branch push (behavioral) ---"

test_prepush \
    "Should allow push of new branch to remote" \
    0 \
    "refs/heads/feat/new-branch $FAKE_LOCAL refs/heads/feat/new-branch $ZERO_SHA" \
    "origin" \
    "https://github.com/test/repo.git" \
    "passthrough" \
    "feat/new-branch"

# ============================================================================
# Behavioral Test 10: Push from main to main (should BLOCK - protected branch)
# ============================================================================
echo ""
echo "--- Push from main to protected branch (behavioral) ---"

test_prepush_with_output \
    "Should block push from main to main (protected)" \
    1 \
    "Direct push" \
    "refs/heads/main $FAKE_LOCAL refs/heads/main $ZERO_SHA" \
    "origin" \
    "https://github.com/test/repo.git" \
    "passthrough" \
    "main"

# ============================================================================
# Behavioral Test 11: Push from master to master (should BLOCK - protected branch)
# ============================================================================
test_prepush_with_output \
    "Should block push from master to master (protected)" \
    1 \
    "Direct push" \
    "refs/heads/master $FAKE_LOCAL refs/heads/master $ZERO_SHA" \
    "origin" \
    "https://github.com/test/repo.git" \
    "passthrough" \
    "master"

# ============================================================================
# Behavioral Test 12: Valid prefix branches all pass
# ============================================================================
echo ""
echo "--- Valid prefix branches (behavioral) ---"

for prefix in feat fix docs refactor test chore plan experiment release hotfix bugfix feature perf style build ci revert merge wip refine; do
    test_prepush \
        "Should allow push to ${prefix}/something" \
        0 \
        "refs/heads/${prefix}/something $FAKE_LOCAL refs/heads/${prefix}/something $FAKE_REMOTE" \
        "origin" \
        "https://github.com/test/repo.git" \
        "ancestor" \
        "${prefix}/something"
done

# ============================================================================
# Behavioral Test 13: Empty stdin (no refs pushed) should succeed
# ============================================================================
echo ""
echo "--- Edge cases (behavioral) ---"

test_prepush \
    "Should allow when stdin is empty (no refs)" \
    0 \
    "" \
    "origin" \
    "https://github.com/test/repo.git" \
    "passthrough" \
    "feat/test-branch"

# ============================================================================
# Behavioral Test 14: Force push error message content
# ============================================================================
echo ""
echo "--- Error message content (behavioral) ---"

test_prepush_with_output \
    "Force push error should mention creating a PR" \
    1 \
    "create a PR" \
    "refs/heads/main $FAKE_LOCAL refs/heads/main $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "not-ancestor" \
    "main"

test_prepush_with_output \
    "Force push error should mention rewriting history" \
    1 \
    "rewrite history" \
    "refs/heads/main $FAKE_LOCAL refs/heads/main $FAKE_REMOTE" \
    "origin" \
    "https://github.com/test/repo.git" \
    "not-ancestor" \
    "main"

# ============================================================================
# Summary
# ============================================================================

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
