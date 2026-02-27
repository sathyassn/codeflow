#!/usr/bin/env bash
# Test: cf-git-protection.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-git-protection.sh
#
# Tests the git protection enforcement module (Sections 1-4)

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

# Functional test helpers - actually source the module and check behavior
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED"; then
        pass "$description"
    else
        fail "$description - Expected block"
    fi
}

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

echo "=== Testing cf-git-protection.sh ==="
echo ""

# =========================================================================
# PART A: Structural Tests
# =========================================================================
echo "--- Structural Tests ---"

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

# Test 10: Has Section 4 - Git Hooks Directory Protection
if grep -q "SECTION 4" "$MODULE" && grep -q "Hooks Directory" "$MODULE"; then
    pass "Has Section 4 - Git Hooks Directory Protection"
else
    fail "Missing Section 4 - Git Hooks Directory Protection"
fi

# Test 11: Uses block_command function
if grep -q 'block_command' "$MODULE"; then
    pass "Uses block_command function"
else
    fail "Should use block_command function"
fi

# Test 12: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 13: Uses get_flags_portion function
if grep -q 'get_flags_portion' "$MODULE"; then
    pass "Uses get_flags_portion function"
else
    fail "Should use get_flags_portion function"
fi

# Test 14: Handles combined flags with n
if grep -q 'combined.*flag' "$MODULE" || grep -q '\[a-mo-z\]\*n' "$MODULE"; then
    pass "Handles combined flags with n"
else
    fail "Should handle combined flags with n"
fi

# Test 15: Has section ownership comment documenting duplication with cf-hook-bypass.sh
if grep -q 'SECTION OWNERSHIP' "$MODULE" && grep -q 'AUTHORITATIVE' "$MODULE"; then
    pass "Has section ownership documentation"
else
    fail "Missing section ownership documentation"
fi

# Test 16: Handles --unset core.hooksPath
if grep -q 'unset.*core\.hooksPath' "$MODULE"; then
    pass "Handles --unset core.hooksPath"
else
    fail "Should handle --unset core.hooksPath"
fi

# =========================================================================
# PART B: Functional Tests - Section 1: Hook Bypass Prevention
# =========================================================================
echo ""
echo "--- Functional Tests: Section 1 (Hook Bypass) ---"

test_blocks_command "git commit --no-verify -m 'message'" \
    "Blocks --no-verify on commit"

test_blocks_command "git push --no-verify origin main" \
    "Blocks --no-verify on push"

test_blocks_command "git rebase --no-verify main" \
    "Blocks --no-verify on rebase"

test_blocks_command "git cherry-pick --no-verify abc123" \
    "Blocks --no-verify on cherry-pick"

test_blocks_command "git merge --no-verify feature" \
    "Blocks --no-verify on merge"

test_blocks_command "git --no-verify status" \
    "Blocks git --no-verify at git level"

test_blocks_command "git commit -n -m 'message'" \
    "Blocks -n short flag on commit"

test_blocks_command "git commit -anm 'message'" \
    "Blocks -anm combined flag on commit"

test_blocks_command "git commit -nam 'message'" \
    "Blocks -nam combined flag on commit"

test_allows_command "git push -n origin main" \
    "Allows git push -n (dry-run, not no-verify)"

test_allows_command "git commit -m 'fix: handle -n flag properly'" \
    "Allows -n in commit message (not flag)"

# =========================================================================
# PART C: Functional Tests - Section 2: Force Push Prevention
# =========================================================================
echo ""
echo "--- Functional Tests: Section 2 (Force Push - Protected Branches) ---"

# Helper: test force push with a specific branch context
# Uses a temp git repo to control the branch name seen by git branch --show-current
test_force_push_on_branch() {
    local branch="$1"
    local command="$2"
    local should_block="$3"  # "block" or "allow"
    local description="$4"

    local tmpdir
    tmpdir=$(mktemp -d)

    # Run in captured subshell to isolate cd and guarantee cleanup
    local result
    result=$( (
        git init -q "$tmpdir" 2>/dev/null || true
        cd "$tmpdir"
        git checkout -q -b "$branch" 2>/dev/null || true

        local output
        output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
            bash -c "source '$MODULE'" 2>&1 || true)

        if [[ "$should_block" == "block" ]]; then
            if echo "$output" | grep -q "BLOCKED"; then
                echo "PASS"
            else
                echo "FAIL_BLOCK"
            fi
        else
            if echo "$output" | grep -q "BLOCKED"; then
                echo "FAIL_ALLOW"
            else
                echo "PASS"
            fi
        fi
    ) 2>/dev/null )

    rm -rf "$tmpdir"

    if [[ "$result" == "PASS" ]]; then
        pass "$description"
    elif [[ "$result" == "FAIL_BLOCK" ]]; then
        fail "$description - Expected block"
    else
        fail "$description - Expected allow but got blocked"
    fi
}

# --- Protected branches: force push MUST be blocked ---
test_force_push_on_branch "main" "git push --force origin main" "block" \
    "Blocks --force push on main"

test_force_push_on_branch "main" "git push --force-with-lease origin main" "block" \
    "Blocks --force-with-lease push on main"

test_force_push_on_branch "main" "git push -f origin main" "block" \
    "Blocks -f push on main"

test_force_push_on_branch "master" "git push --force origin master" "block" \
    "Blocks --force push on master"

test_force_push_on_branch "production" "git push --force origin production" "block" \
    "Blocks --force push on production"

test_force_push_on_branch "release/v1.0" "git push --force origin release/v1.0" "block" \
    "Blocks --force push on release/* branch"

# --- Feature branches: force push MUST be allowed ---
test_force_push_on_branch "feat/update-command" "git push --force origin feat/update-command" "allow" \
    "Allows --force push on feature branch"

test_force_push_on_branch "feat/update-command" "git push --force-with-lease origin feat/update-command" "allow" \
    "Allows --force-with-lease push on feature branch"

test_force_push_on_branch "feat/update-command" "git push -f origin feat/update-command" "allow" \
    "Allows -f push on feature branch"

test_force_push_on_branch "fix/bug-fix" "git push --force origin fix/bug-fix" "allow" \
    "Allows --force push on fix branch"

test_force_push_on_branch "refactor/cleanup" "git push --force-with-lease origin refactor/cleanup" "allow" \
    "Allows --force-with-lease push on refactor branch"

# --- Normal push still allowed ---
test_allows_command "git push origin feature/branch" \
    "Allows normal push (no force flag)"

# =========================================================================
# PART D: Functional Tests - Section 3: Hook Path Manipulation
# =========================================================================
echo ""
echo "--- Functional Tests: Section 3 (Hook Manipulation) ---"

test_blocks_command "git config core.hooksPath /tmp/hooks" \
    "Blocks git config core.hooksPath"

test_blocks_command "git -c core.hooksPath=/tmp commit -m 'msg'" \
    "Blocks git -c core.hooksPath override"

test_blocks_command "GIT_HOOKS_PATH=/tmp git commit -m 'msg'" \
    "Blocks GIT_HOOKS_PATH env var"

test_blocks_command "SKIP_HOOKS=1 git commit -m 'msg'" \
    "Blocks SKIP_HOOKS env var"

test_blocks_command "GIT_SKIP_HOOKS=1 git commit -m 'msg'" \
    "Blocks GIT_SKIP_HOOKS env var"

test_blocks_command "HUSKY=0 git commit -m 'msg'" \
    "Blocks HUSKY=0 bypass"

test_blocks_command "PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit -m 'msg'" \
    "Blocks PRE_COMMIT_ALLOW_NO_CONFIG bypass"

test_blocks_command "git config --unset core.hooksPath" \
    "Blocks --unset core.hooksPath"

# =========================================================================
# PART E: Functional Tests - Section 4: Git Hooks Directory Protection
# =========================================================================
echo ""
echo "--- Functional Tests: Section 4 (Hooks Dir Protection) ---"

test_blocks_command "rm -rf .git/hooks/pre-commit" \
    "Blocks rm on .git/hooks"

test_blocks_command "mv .git/hooks/pre-commit /tmp/" \
    "Blocks mv on .git/hooks"

test_blocks_command "chmod 644 .git/hooks/pre-commit" \
    "Blocks chmod on .git/hooks"

test_blocks_command "echo 'exit 0' > .git/hooks/pre-commit" \
    "Blocks redirect to .git/hooks"

# =========================================================================
# PART F: Functional Tests - Allowed Commands
# =========================================================================
echo ""
echo "--- Functional Tests: Allowed Commands ---"

test_allows_command "git commit -m 'fix: proper commit'" \
    "Allows normal commit"

test_allows_command "git status" \
    "Allows git status"

test_allows_command "git diff" \
    "Allows git diff"

test_allows_command "git log --oneline -5" \
    "Allows git log"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
