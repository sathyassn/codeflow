#!/usr/bin/env bash
# Test: cf-hook-bypass.sh
# Location: .codeflow/testing/scripts/security/test-cf-hook-bypass.sh
#
# Tests the hook bypass detection module

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

# Helper function to test command blocking
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected block"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper function to test command allowing
test_allows_command() {
    local command="$1"
    local description="$2"

    if COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-hook-bypass.sh ==="
echo ""

# Test 1: Block --no-verify on commit
echo "Test 1: Block --no-verify on commit"
test_blocks_command "git commit --no-verify -m 'message'" "Should block --no-verify"

# Test 2: Block -n short flag on commit
echo "Test 2: Block -n short flag on commit"
test_blocks_command "git commit -n -m 'message'" "Should block -n flag"

# Test 3: Block force push
echo "Test 3: Block force push"
test_blocks_command "git push --force origin main" "Should block --force"

# Test 4: Block force push short flag
echo "Test 4: Block force push short flag"
test_blocks_command "git push -f origin main" "Should block -f"

# Test 5: Block --force-with-lease
echo "Test 5: Block --force-with-lease"
test_blocks_command "git push --force-with-lease origin main" "Should block --force-with-lease"

# Test 6: Block HUSKY=0
echo "Test 6: Block HUSKY=0"
test_blocks_command "HUSKY=0 git commit -m 'msg'" "Should block HUSKY=0"

# Test 7: Block core.hooksPath override
echo "Test 7: Block core.hooksPath override"
test_blocks_command "git -c core.hooksPath=/tmp commit -m 'msg'" "Should block core.hooksPath"

# Test 8: Allow normal commit
echo "Test 8: Allow normal commit"
test_allows_command "git commit -m 'fix: proper commit'" "Should allow normal commit"

# Test 9: Allow normal push
echo "Test 9: Allow normal push"
test_allows_command "git push origin feature/branch" "Should allow normal push"

# Test 10: Allow -n in commit message (not flag)
echo "Test 10: Allow -n in commit message"
test_allows_command "git commit -m 'fix: handle -n flag properly'" "Should allow -n in message"

# Test 11: Block git --no-verify at git level (before subcommand)
echo "Test 11: Block git --no-verify at git level"
test_blocks_command "git --no-verify status" "Should block git --no-verify"

# Test 12: Block combined flags with -n (e.g., -anm)
echo "Test 12: Block combined flags with -n"
test_blocks_command "git commit -anm 'message'" "Should block -anm combined flag"

# Test 13: Block git config core.hooksPath
echo "Test 13: Block git config core.hooksPath"
test_blocks_command "git config core.hooksPath /tmp/hooks" "Should block core.hooksPath config"

# Test 14: Block git config --unset core.hooksPath
echo "Test 14: Block git config --unset core.hooksPath"
test_blocks_command "git config --unset core.hooksPath" "Should block --unset core.hooksPath"

# Test 15: Block GIT_HOOKS_PATH env var
echo "Test 15: Block GIT_HOOKS_PATH env var"
test_blocks_command "GIT_HOOKS_PATH=/tmp git commit -m 'msg'" "Should block GIT_HOOKS_PATH"

# Test 16: Block SKIP_HOOKS env var
echo "Test 16: Block SKIP_HOOKS env var"
test_blocks_command "SKIP_HOOKS=1 git commit -m 'msg'" "Should block SKIP_HOOKS"

# Test 17: Block GIT_SKIP_HOOKS env var
echo "Test 17: Block GIT_SKIP_HOOKS env var"
test_blocks_command "GIT_SKIP_HOOKS=1 git commit -m 'msg'" "Should block GIT_SKIP_HOOKS"

# Test 18: Block PRE_COMMIT_ALLOW_NO_CONFIG
echo "Test 18: Block PRE_COMMIT_ALLOW_NO_CONFIG"
test_blocks_command "PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit -m 'msg'" "Should block PRE_COMMIT_ALLOW_NO_CONFIG"

# Test 19: Block rm on .git/hooks
echo "Test 19: Block rm on .git/hooks"
test_blocks_command "rm -rf .git/hooks/pre-commit" "Should block rm on .git/hooks"

# Test 20: Block mv on .git/hooks
echo "Test 20: Block mv on .git/hooks"
test_blocks_command "mv .git/hooks/pre-commit /tmp/" "Should block mv on .git/hooks"

# Test 21: Block redirect to .git/hooks
echo "Test 21: Block redirect to .git/hooks"
test_blocks_command "echo 'exit 0' > .git/hooks/pre-commit" "Should block redirect to .git/hooks"

# Test 22: Block chmod on .git/hooks
echo "Test 22: Block chmod on .git/hooks"
test_blocks_command "chmod 644 .git/hooks/pre-commit" "Should block chmod on .git/hooks"

# Test 23: Block --no-verify on push
echo "Test 23: Block --no-verify on push"
test_blocks_command "git push --no-verify origin main" "Should block --no-verify on push"

# Test 24: Block --no-verify on rebase
echo "Test 24: Block --no-verify on rebase"
test_blocks_command "git rebase --no-verify main" "Should block --no-verify on rebase"

# Test 25: Block --no-verify on cherry-pick
echo "Test 25: Block --no-verify on cherry-pick"
test_blocks_command "git cherry-pick --no-verify abc123" "Should block --no-verify on cherry-pick"

# Test 26: Block --no-verify on merge
echo "Test 26: Block --no-verify on merge"
test_blocks_command "git merge --no-verify feature" "Should block --no-verify on merge"

# Test 27: Allow git push -n (dry-run, not no-verify)
echo "Test 27: Allow git push -n (dry-run)"
test_allows_command "git push -n origin main" "Should allow git push -n (dry-run)"

# Test 28: Block combined flags -nam on commit
echo "Test 28: Block combined flags -nam on commit"
test_blocks_command "git commit -nam 'message'" "Should block -nam combined flag"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
