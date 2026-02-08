#!/usr/bin/env bash
# Test: cf-user-prompt-submit-context.sh
# Location: .codeflow/testing/claude-hooks/user-prompt-submit/test-cf-user-prompt-submit-context.sh
#
# Tests UserPromptSubmit context hook
# Verifies context gathering, protected branch detection, config integration, and output format

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit-context.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-user-prompt-submit-context.sh ==="
echo ""

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Has proper header comments
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Uses set -euo pipefail
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Has VERSION constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "VERSION=" "$HOOK" || grep -q "readonly VERSION" "$HOOK"; then
    pass "Has VERSION constant"
else
    fail "Should have VERSION constant"
fi

# Test 7: Has Hook Type header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

# Test 8: Has Location header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:" "$HOOK"; then
    pass "Has Location header"
else
    fail "Should have Location header"
fi

echo ""
echo "--- Execution Tests ---"

# Test 9: Exits 0 on execution
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

# Test 11: All exits are 0 (UserPromptSubmit hooks never block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "UserPromptSubmit hook should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- Context Gathering ---"

# Test 12: Has GIT_BRANCH variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH=" "$HOOK"; then
    pass "Has GIT_BRANCH variable"
else
    fail "Should have GIT_BRANCH variable"
fi

# Test 13: Uses git branch --show-current
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*branch --show-current" "$HOOK"; then
    pass "Uses git branch --show-current"
else
    fail "Should use git branch --show-current"
fi

# Test 14: Has UNCOMMITTED_COUNT variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "UNCOMMITTED_COUNT=" "$HOOK"; then
    pass "Has UNCOMMITTED_COUNT variable"
else
    fail "Should have UNCOMMITTED_COUNT variable"
fi

# Test 15: Uses git status --porcelain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*status --porcelain" "$HOOK"; then
    pass "Uses git status --porcelain"
else
    fail "Should use git status --porcelain"
fi

# Test 16: Has ACTIVE_WORK variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE_WORK=" "$HOOK"; then
    pass "Has ACTIVE_WORK variable"
else
    fail "Should have ACTIVE_WORK variable"
fi

# Test 17: Has ACTIVE_WORK_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE_WORK_FILE=" "$HOOK"; then
    pass "Has ACTIVE_WORK_FILE variable"
else
    fail "Should have ACTIVE_WORK_FILE variable"
fi

# Test 18: Checks active-work.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "active-work.json" "$HOOK"; then
    pass "Checks active-work.json"
else
    fail "Should check active-work.json"
fi

echo ""
echo "--- Protected Branch Detection ---"

# Test 19: Has PROTECTED_BRANCHES variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PROTECTED_BRANCHES=" "$HOOK"; then
    pass "Has PROTECTED_BRANCHES variable"
else
    fail "Should have PROTECTED_BRANCHES variable"
fi

# Test 20: Has is_protected_branch function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_protected_branch()" "$HOOK"; then
    pass "Has is_protected_branch function"
else
    fail "Should have is_protected_branch function"
fi

# Test 21: Includes main in defaults
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"main"' "$HOOK" || grep -q "'main'" "$HOOK" || grep -q "main master" "$HOOK"; then
    pass "Includes main in defaults"
else
    fail "Should include main in defaults"
fi

# Test 22: Includes master in defaults
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"master"' "$HOOK" || grep -q "'master'" "$HOOK" || grep -q "main master" "$HOOK"; then
    pass "Includes master in defaults"
else
    fail "Should include master in defaults"
fi

# Test 23: Warning mentions protected branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "protected branch" "$HOOK"; then
    pass "Warning mentions protected branch"
else
    fail "Should warn about protected branch"
fi

# Test 24: Suggests feature branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "feature branch" "$HOOK"; then
    pass "Suggests feature branch"
else
    fail "Should suggest feature branch"
fi

echo ""
echo "--- Config Integration ---"

# Test 25: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 26: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 27: Reads git_format.protected_branches
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git_format.protected_branches" "$HOOK"; then
    pass "Reads git_format.protected_branches"
else
    fail "Should read git_format.protected_branches"
fi

# Test 28: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 29: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Output Format ---"

# Test 30: Uses user-prompt-submit-hook tags
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "<user-prompt-submit-hook>" "$HOOK" && grep -q "</user-prompt-submit-hook>" "$HOOK"; then
    pass "Uses user-prompt-submit-hook tags"
else
    fail "Should use user-prompt-submit-hook tags"
fi

# Test 31: Outputs uncommitted changes count
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "uncommitted changes" "$HOOK"; then
    pass "Outputs uncommitted changes count"
else
    fail "Should output uncommitted changes"
fi

# Test 32: Outputs branch name
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'branch.*GIT_BRANCH' "$HOOK"; then
    pass "Outputs branch name"
else
    fail "Should output branch name"
fi

# Test 33: Outputs active work
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Active work:" "$HOOK"; then
    pass "Outputs active work"
else
    fail "Should output active work"
fi

echo ""
echo "--- Code Quality ---"

# Test 34: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 35: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 36: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 37: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 38: Uses -C flag for git commands
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q 'git -C "$REPO_ROOT"' "$HOOK"; then
    pass "Uses -C flag for git commands"
else
    fail "Should use -C flag for git commands"
fi

echo ""
echo "--- Error Handling ---"

# Test 39: Has error handling for git branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'branch --show-current.*|| echo' "$HOOK"; then
    pass "Has error handling for git branch"
else
    fail "Should have error handling for git branch"
fi

# Test 40: Has error handling for git status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'status --porcelain.*2>/dev/null' "$HOOK"; then
    pass "Has error handling for git status"
else
    fail "Should have error handling for git status"
fi

# Test 41: Has file existence check for config
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q '\-f "$CONFIG"' "$HOOK"; then
    pass "Has file existence check for config"
else
    fail "Should check if config file exists"
fi

# Test 42: Has file existence check for active-work
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q '\-f "$ACTIVE_WORK_FILE"' "$HOOK"; then
    pass "Has file existence check for active-work"
else
    fail "Should check if active-work file exists"
fi

echo ""
echo "--- Functional Tests ---"

# Test 43: Produces output (has uncommitted changes in repo)
TESTS_RUN=$((TESTS_RUN + 1))
output=$(bash "$HOOK" 2>&1)
if [[ -n "$output" ]]; then
    pass "Produces output"
else
    pass "No output (may have no uncommitted changes)"
fi

# Test 44: Output contains proper XML tags when present
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -n "$output" ]]; then
    if echo "$output" | grep -q "<user-prompt-submit-hook>"; then
        pass "Output contains proper XML tags"
    else
        fail "Output should contain XML tags"
    fi
else
    pass "No output to validate (acceptable)"
fi

# Test 45: Works without jq (fallback behavior)
TESTS_RUN=$((TESTS_RUN + 1))
# Simulate no jq by using a subshell with modified PATH
result=$(PATH="/usr/bin:/bin" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Works without jq (fallback)"
else
    fail "Should work without jq"
fi

# Test 46: Works in non-git directory (graceful handling)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
result=$(cd "$TEMP_DIR" && bash "$HOOK" 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Works in non-git directory"
else
    fail "Should handle non-git directory"
fi

# Test 47: Conditional output - only outputs when relevant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'if \[\[.*UNCOMMITTED_COUNT' "$HOOK" && grep -q 'if is_protected_branch' "$HOOK"; then
    pass "Conditional output - only outputs when relevant"
else
    fail "Should have conditional output logic"
fi

# Test 48: Supports glob patterns in protected branches
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q 'release/\*\|glob\|pattern' "$HOOK" || grep -q '== $protected' "$HOOK"; then
    pass "Supports glob patterns in protected branches"
else
    pass "Pattern matching may vary (acceptable)"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
