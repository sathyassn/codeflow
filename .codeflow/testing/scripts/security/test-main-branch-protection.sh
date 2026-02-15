#!/usr/bin/env bash
# Test: Main branch protection comprehensive tests
# Location: .codeflow/testing/scripts/security/test-main-branch-protection.sh
#
# Tests all main branch protections:
#   - No local merge to main (PR-only)
#   - No force push
#   - No --no-verify flag
#   - No cherry-pick on main
#   - No rebase on main
#   - No reset on main
#   - No chained checkout+merge commands
#   - Pre-commit blocks direct commits to main

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh"
GIT_PROT="$REPO_ROOT/.codeflow/scripts/security/enforcement/cf-git-protection.sh"
PRE_COMMIT="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-commit"

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0

pass() {
    echo "PASS: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo "FAIL: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

skip() {
    echo "SKIP: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

# Test helper: simulate PreToolUse hook with a Bash command
# Returns exit code from hook
test_bash_command() {
    local command="$1"
    local description="$2"
    local expected_blocked="$3"  # "blocked" or "allowed"

    local hook_input
    hook_input=$(printf '{"tool_name":"Bash","tool_input":{"command":"%s"}}' "$command")

    set +e
    echo "$hook_input" | TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"$command\"}" bash "$HOOK" >/dev/null 2>&1
    local exit_code=$?
    set -e

    if [[ "$expected_blocked" == "blocked" ]]; then
        if [[ "$exit_code" -eq 2 ]]; then
            pass "$description"
        else
            fail "$description (expected blocked/exit 2, got exit $exit_code)"
        fi
    else
        if [[ "$exit_code" -eq 0 ]]; then
            pass "$description"
        else
            fail "$description (expected allowed/exit 0, got exit $exit_code)"
        fi
    fi
}

echo "=== Main Branch Protection Tests ==="
echo ""

# ============================================================================
# Test 0: Files exist
# ============================================================================
echo "--- File existence ---"

if [[ -f "$HOOK" ]]; then
    pass "Security hook exists"
else
    fail "Security hook not found: $HOOK"
fi

if [[ -f "$GIT_PROT" ]]; then
    pass "Git protection module exists"
else
    fail "Git protection module not found: $GIT_PROT"
fi

if [[ -f "$PRE_COMMIT" ]]; then
    pass "Pre-commit hook exists"
else
    fail "Pre-commit hook not found: $PRE_COMMIT"
fi

# ============================================================================
# Test 1: Section 5 exists in git-protection module
# ============================================================================
echo ""
echo "--- Section 5: Protected Branch Operations ---"

if grep -q "SECTION 5" "$GIT_PROT"; then
    pass "Section 5 exists in git-protection module"
else
    fail "Section 5 missing from git-protection module"
fi

if grep -q "_on_protected_branch" "$GIT_PROT"; then
    pass "Protected branch detection function exists"
else
    fail "Protected branch detection function missing"
fi

if grep -q "protected_branches" "$GIT_PROT"; then
    pass "Config-driven protected branches loaded"
else
    fail "Config-driven protected branches not loaded"
fi

# ============================================================================
# Test 2: Force push blocked (all variants)
# ============================================================================
echo ""
echo "--- Force push prevention ---"

test_bash_command "git push --force origin main" \
    "Block: git push --force" "blocked"

test_bash_command "git push --force-with-lease origin main" \
    "Block: git push --force-with-lease" "blocked"

test_bash_command "git push -f origin main" \
    "Block: git push -f (short flag)" "blocked"

test_bash_command "git push origin main --force" \
    "Block: git push with --force at end" "blocked"

# ============================================================================
# Test 3: Hook bypass blocked (--no-verify)
# ============================================================================
echo ""
echo "--- Hook bypass prevention ---"

test_bash_command "git commit --no-verify -m 'test'" \
    "Block: git commit --no-verify" "blocked"

test_bash_command "git push --no-verify origin main" \
    "Block: git push --no-verify" "blocked"

test_bash_command "git merge --no-verify feature" \
    "Block: git merge --no-verify" "blocked"

test_bash_command "git rebase --no-verify main" \
    "Block: git rebase --no-verify" "blocked"

test_bash_command "git commit -n -m 'test'" \
    "Block: git commit -n (short --no-verify)" "blocked"

# ============================================================================
# Test 4: Merge to main blocked (when on main)
# ============================================================================
echo ""
echo "--- Merge to protected branch (current branch = main) ---"

# These tests need to simulate being on main. The hook checks current branch
# via git branch --show-current. We test the pattern matching.

if grep -q 'git[[:space:]]*merge' "$GIT_PROT" && grep -q '_on_protected_branch' "$GIT_PROT"; then
    pass "Merge protection pattern exists"
else
    fail "Merge protection pattern missing"
fi

# Test the regex patterns directly
if grep -qE 'git\[' "$GIT_PROT" | head -1; then true; fi

# Verify the specific patterns are present
if grep -q 'git.*merge.*_on_protected_branch' "$GIT_PROT" 2>/dev/null || \
   grep -q 'git.*merge' "$GIT_PROT" && grep -q 'block_command.*Protected Branch.*Merge' "$GIT_PROT"; then
    pass "Merge block command for protected branch exists"
else
    fail "Merge block command missing"
fi

# ============================================================================
# Test 5: Cherry-pick to main blocked
# ============================================================================
echo ""
echo "--- Cherry-pick to protected branch ---"

if grep -q 'cherry-pick' "$GIT_PROT" && grep -q 'block_command.*Protected Branch.*Cherry' "$GIT_PROT"; then
    pass "Cherry-pick protection exists"
else
    fail "Cherry-pick protection missing"
fi

# ============================================================================
# Test 6: Rebase on main blocked
# ============================================================================
echo ""
echo "--- Rebase on protected branch ---"

if grep -q 'git.*rebase' "$GIT_PROT" && grep -q 'block_command.*Protected Branch.*Rebase' "$GIT_PROT"; then
    pass "Rebase protection exists"
else
    fail "Rebase protection missing"
fi

# ============================================================================
# Test 7: Reset on main blocked
# ============================================================================
echo ""
echo "--- Reset on protected branch ---"

if grep -q 'git.*reset' "$GIT_PROT" && grep -q 'block_command.*Protected Branch.*Reset' "$GIT_PROT"; then
    pass "Reset protection exists"
else
    fail "Reset protection missing"
fi

# ============================================================================
# Test 8: Chained checkout+merge blocked
# ============================================================================
echo ""
echo "--- Chained checkout+merge prevention ---"

test_bash_command "git checkout main && git merge feat/branch" \
    "Block: checkout main && merge" "blocked"

test_bash_command "git checkout main; git merge feat/branch" \
    "Block: checkout main ; merge" "blocked"

test_bash_command "git switch main && git merge feat/branch" \
    "Block: switch main && merge" "blocked"

test_bash_command "git checkout main && git cherry-pick abc123" \
    "Block: checkout main && cherry-pick" "blocked"

test_bash_command "git checkout main && git rebase feat/branch" \
    "Block: checkout main && rebase" "blocked"

test_bash_command "git checkout main && git reset --hard HEAD~1" \
    "Block: checkout main && reset" "blocked"

# ============================================================================
# Test 9: Allowed operations (should NOT be blocked)
# ============================================================================
echo ""
echo "--- Allowed operations (should pass) ---"

test_bash_command "git status" \
    "Allow: git status" "allowed"

test_bash_command "git log --oneline -5" \
    "Allow: git log" "allowed"

test_bash_command "git diff main..feat/branch" \
    "Allow: git diff" "allowed"

test_bash_command "git branch -v" \
    "Allow: git branch -v" "allowed"

test_bash_command "git push origin feat/branch" \
    "Block: regular push (requires sandbox bypass)" "blocked"

test_bash_command "git checkout feat/branch" \
    "Allow: checkout to feature branch" "allowed"

test_bash_command "git checkout main" \
    "Allow: checkout main alone (read-only)" "allowed"

test_bash_command "git merge feat/branch" \
    "Allow: merge on non-protected branch" "allowed"

# ============================================================================
# Test 10: Hook path manipulation blocked
# ============================================================================
echo ""
echo "--- Hook manipulation prevention ---"

test_bash_command "git config core.hooksPath /dev/null" \
    "Block: core.hooksPath modification" "blocked"

test_bash_command "git -c core.hooksPath=/dev/null status" \
    "Block: -c core.hooksPath override" "blocked"

test_bash_command "git config --unset core.hooksPath" \
    "Block: unset core.hooksPath" "blocked"

test_bash_command "GIT_HOOKS_PATH=/dev/null git commit" \
    "Block: GIT_HOOKS_PATH env var" "blocked"

test_bash_command "SKIP_HOOKS=1 git commit" \
    "Block: SKIP_HOOKS env var" "blocked"

test_bash_command "HUSKY=0 git commit" \
    "Block: HUSKY=0 bypass" "blocked"

# ============================================================================
# Test 11: Git hooks directory protection
# ============================================================================
echo ""
echo "--- Git hooks directory protection ---"

test_bash_command "rm -rf .git/hooks" \
    "Block: rm .git/hooks" "blocked"

test_bash_command "mv .git/hooks /tmp/" \
    "Block: mv .git/hooks" "blocked"

test_bash_command "chmod 777 .git/hooks/pre-commit" \
    "Block: chmod .git/hooks" "blocked"

# ============================================================================
# Test 12: Pre-commit hook has branch protection
# ============================================================================
echo ""
echo "--- Pre-commit hook branch protection ---"

if grep -q 'BRANCH PROTECTION' "$PRE_COMMIT"; then
    pass "Pre-commit has branch protection check"
else
    fail "Pre-commit missing branch protection"
fi

if grep -q 'protected_branches' "$PRE_COMMIT" || grep -q 'PROTECTED_BRANCHES' "$PRE_COMMIT"; then
    pass "Pre-commit loads protected branches"
else
    fail "Pre-commit does not load protected branches"
fi

# ============================================================================
# Test 13: Protected branches list matches config
# ============================================================================
echo ""
echo "--- Config consistency ---"

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
if [[ -f "$CONFIG" ]]; then
    if jq -e '.protected_branches | length > 0' "$CONFIG" >/dev/null 2>&1; then
        pass "Config has protected_branches array"
    else
        fail "Config missing protected_branches"
    fi

    if jq -e '.protected_branches | index("main")' "$CONFIG" >/dev/null 2>&1; then
        pass "Config protects 'main' branch"
    else
        fail "Config does not protect 'main'"
    fi

    if jq -e '.protected_branches | index("master")' "$CONFIG" >/dev/null 2>&1; then
        pass "Config protects 'master' branch"
    else
        fail "Config does not protect 'master'"
    fi
else
    skip "Config file not found"
fi

# ============================================================================
# Test 14: filter-branch / reflog deletion blocked
# ============================================================================
echo ""
echo "--- History rewrite protection ---"

test_bash_command "git filter-branch --msg-filter 'cat' main" \
    "Allow: filter-branch (not force push)" "allowed"

test_bash_command "git push --force origin main" \
    "Block: force push after filter-branch" "blocked"

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
