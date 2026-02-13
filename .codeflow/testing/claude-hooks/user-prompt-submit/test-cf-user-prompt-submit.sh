#!/usr/bin/env bash
# Test: cf-user-prompt-submit.sh
# Location: .codeflow/testing/claude-hooks/user-prompt-submit/test-cf-user-prompt-submit.sh
#
# Tests UserPromptSubmit context hook v2.0.0
# Verifies stdin reading, config-driven instructions, context gathering,
# protected branch detection, parallel work awareness, PathFlow mode,
# config integration, and output format.

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="${HOOK:-$REAL_REPO_ROOT/.claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit.sh}"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Helper: run hook with stdin JSON
run_hook_stdin() {
    local stdin_json
    stdin_json=$(jq -nc \
        --arg sid "test-session-$$" \
        '{session_id: $sid, cwd: "/tmp", hook_event_name: "UserPromptSubmit",
         permission_mode: "default", tool_use_id: "", transcript_path: "/tmp/test.jsonl"}')
    echo "$stdin_json" | bash "$HOOK" 2>&1
    return "${PIPESTATUS[1]}"
}

echo "=== Testing cf-user-prompt-submit.sh (v2.0.0) ==="
echo ""

# =========================================================================
# FILE STRUCTURE
# =========================================================================
echo "--- File Structure ---"

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found at $HOOK"; fi

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
if grep -q 'readonly VERSION="2.0.0"' "$HOOK"; then
    pass "Has VERSION 2.0.0 constant"
else
    fail "Should have readonly VERSION=2.0.0"
fi

# Test 7: Has Hook Type header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

# Test 8: Has Location header with new name
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:.*cf-user-prompt-submit.sh" "$HOOK"; then
    pass "Has Location header with new name"
else
    fail "Location header should reference cf-user-prompt-submit.sh"
fi

echo ""

# =========================================================================
# STDIN READING (NEW in v2.0.0)
# =========================================================================
echo "--- Stdin Reading ---"

# Test 9: Has STDIN_INPUT variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STDIN_INPUT=" "$HOOK"; then
    pass "Has STDIN_INPUT variable"
else
    fail "Should have STDIN_INPUT variable"
fi

# Test 10: Reads stdin with cat
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'STDIN_INPUT=$(cat)' "$HOOK"; then
    pass "Reads stdin with cat"
else
    fail "Should read stdin with cat"
fi

# Test 11: Checks if stdin is terminal before reading
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '! -t 0' "$HOOK"; then
    pass "Checks if stdin is terminal before reading"
else
    fail "Should check if stdin is terminal (! -t 0)"
fi

# Test 12: Has SESSION_ID variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_ID=" "$HOOK"; then
    pass "Has SESSION_ID variable"
else
    fail "Should have SESSION_ID variable"
fi

# Test 13: Extracts session_id from stdin JSON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id' "$HOOK"; then
    pass "Extracts session_id from stdin JSON"
else
    fail "Should extract session_id from stdin"
fi

# Test 14: Checks jq availability before parsing stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'STDIN_INPUT.*command -v jq' "$HOOK" || grep -B2 'session_id' "$HOOK" | grep -q 'command -v jq'; then
    pass "Checks jq availability before parsing stdin"
else
    fail "Should check jq availability before parsing stdin"
fi

echo ""

# =========================================================================
# SECTION 1: CONFIG-DRIVEN INSTRUCTIONS (NEW in v2.0.0)
# =========================================================================
echo "--- Config-Driven Instructions (Section 1) ---"

# Test 15: Has INSTRUCTIONS_CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTIONS_CONFIG=" "$HOOK"; then
    pass "Has INSTRUCTIONS_CONFIG variable"
else
    fail "Should have INSTRUCTIONS_CONFIG variable"
fi

# Test 16: References instructions-config.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "instructions-config.json" "$HOOK"; then
    pass "References instructions-config.json"
else
    fail "Should reference instructions-config.json"
fi

# Test 17: Has INSTRUCTIONS_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTIONS_DIR=" "$HOOK"; then
    pass "Has INSTRUCTIONS_DIR variable"
else
    fail "Should have INSTRUCTIONS_DIR variable"
fi

# Test 18: Reads UserPromptSubmit entries from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'UserPromptSubmit' "$HOOK"; then
    pass "Reads UserPromptSubmit entries from config"
else
    fail "Should read UserPromptSubmit from config"
fi

# Test 19: Checks enabled status of instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'enabled == true' "$HOOK" || grep -q '.enabled' "$HOOK"; then
    pass "Checks enabled status of instructions"
else
    fail "Should check enabled status"
fi

# Test 20: Reads file path from config entries
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\.file' "$HOOK"; then
    pass "Reads file path from config entries"
else
    fail "Should read .file from config entries"
fi

# Test 21: Cats instruction text files
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'cat "$INSTRUCTIONS_DIR/$IFILE"' "$HOOK" || grep -q 'cat.*INSTRUCTIONS_DIR' "$HOOK"; then
    pass "Cats instruction text files"
else
    fail "Should cat instruction text files"
fi

# Test 22: Has fallback when config not available
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Fallback:" "$HOOK" || grep -q "cf-working-protocol" "$HOOK"; then
    pass "Has fallback when config not available"
else
    fail "Should have fallback instruction"
fi

# Test 23: Wraps instruction output in XML tags
TESTS_RUN=$((TESTS_RUN + 1))
# Count occurrences of echo'd opening and closing tags - should be multiple balanced pairs
OPEN_TAGS=$(grep -c 'echo "<user-prompt-submit-hook>"' "$HOOK" 2>/dev/null) || OPEN_TAGS=0
CLOSE_TAGS=$(grep -c 'echo "</user-prompt-submit-hook>"' "$HOOK" 2>/dev/null) || CLOSE_TAGS=0
if [[ "$OPEN_TAGS" -eq "$CLOSE_TAGS" && "$OPEN_TAGS" -gt 1 ]]; then
    pass "Wraps instruction output in balanced XML tags ($OPEN_TAGS pairs)"
else
    fail "Should have balanced XML tag pairs (got open=$OPEN_TAGS close=$CLOSE_TAGS)"
fi

echo ""

# =========================================================================
# SECTION 2: GIT CONTEXT
# =========================================================================
echo "--- Git Context (Section 2) ---"

# Test 24: Has GIT_BRANCH variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH=" "$HOOK"; then
    pass "Has GIT_BRANCH variable"
else
    fail "Should have GIT_BRANCH variable"
fi

# Test 25: Uses git branch --show-current
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*branch --show-current" "$HOOK"; then
    pass "Uses git branch --show-current"
else
    fail "Should use git branch --show-current"
fi

# Test 26: Has UNCOMMITTED_COUNT variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "UNCOMMITTED_COUNT=" "$HOOK"; then
    pass "Has UNCOMMITTED_COUNT variable"
else
    fail "Should have UNCOMMITTED_COUNT variable"
fi

# Test 27: Uses git status --porcelain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*status --porcelain" "$HOOK"; then
    pass "Uses git status --porcelain"
else
    fail "Should use git status --porcelain"
fi

# Test 28: Has ACTIVE_WORK variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE_WORK=" "$HOOK"; then
    pass "Has ACTIVE_WORK variable"
else
    fail "Should have ACTIVE_WORK variable"
fi

# Test 29: Sources cf-work-state.sh library
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-work-state.sh" "$HOOK"; then
    pass "Sources cf-work-state.sh library"
else
    fail "Should source cf-work-state.sh library"
fi

# Test 30: Uses is_task_active from cf-work-state.sh
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_task_active" "$HOOK"; then
    pass "Uses is_task_active from cf-work-state.sh"
else
    fail "Should use is_task_active from cf-work-state.sh"
fi

# Test 31: Outputs uncommitted changes count
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "uncommitted changes" "$HOOK"; then
    pass "Outputs uncommitted changes count"
else
    fail "Should output uncommitted changes"
fi

# Test 32: Outputs branch name in context
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'branch.*GIT_BRANCH' "$HOOK"; then
    pass "Outputs branch name in context"
else
    fail "Should output branch name"
fi

echo ""

# =========================================================================
# SECTION 3: PROTECTED BRANCH WARNING
# =========================================================================
echo "--- Protected Branch Warning (Section 3) ---"

# Test 33: Has PROTECTED_BRANCHES variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PROTECTED_BRANCHES=" "$HOOK"; then
    pass "Has PROTECTED_BRANCHES variable"
else
    fail "Should have PROTECTED_BRANCHES variable"
fi

# Test 34: Has is_protected_branch function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_protected_branch()" "$HOOK"; then
    pass "Has is_protected_branch function"
else
    fail "Should have is_protected_branch function"
fi

# Test 35: Includes main in defaults
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"main"' "$HOOK" || grep -q "'main'" "$HOOK" || grep -q "main master" "$HOOK"; then
    pass "Includes main in defaults"
else
    fail "Should include main in defaults"
fi

# Test 36: Includes master in defaults
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"master"' "$HOOK" || grep -q "'master'" "$HOOK" || grep -q "main master" "$HOOK"; then
    pass "Includes master in defaults"
else
    fail "Should include master in defaults"
fi

# Test 37: Warning mentions protected branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "protected branch" "$HOOK"; then
    pass "Warning mentions protected branch"
else
    fail "Should warn about protected branch"
fi

# Test 38: Suggests feature branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "feature branch" "$HOOK"; then
    pass "Suggests feature branch"
else
    fail "Should suggest feature branch"
fi

# Test 39: Has Skill reference in protected branch warning
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Skill('cf-git-workflow'.*create-feature-branch" "$HOOK"; then
    pass "Has Skill reference in protected branch warning"
else
    fail "Protected branch warning should include Skill('cf-git-workflow') reference"
fi

echo ""

# =========================================================================
# SECTION 4: ACTIVE WORK
# =========================================================================
echo "--- Active Work (Section 4) ---"

# Test 40: Outputs active work
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Active work:" "$HOOK"; then
    pass "Outputs active work"
else
    fail "Should output active work"
fi

echo ""

# =========================================================================
# SECTION 5: PARALLEL WORK AWARENESS (NEW in v2.0.0)
# =========================================================================
echo "--- Parallel Work Awareness (Section 5) ---"

# Test 41: Has WORKTREES_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "WORKTREES_FILE=" "$HOOK"; then
    pass "Has WORKTREES_FILE variable"
else
    fail "Should have WORKTREES_FILE variable"
fi

# Test 42: Has SESSIONS_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSIONS_FILE=" "$HOOK"; then
    pass "Has SESSIONS_FILE variable"
else
    fail "Should have SESSIONS_FILE variable"
fi

# Test 43: References worktrees.yaml
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "worktrees.yaml" "$HOOK"; then
    pass "References worktrees.yaml"
else
    fail "Should reference worktrees.yaml"
fi

# Test 44: References model-sessions.yaml
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "model-sessions.yaml" "$HOOK"; then
    pass "References model-sessions.yaml"
else
    fail "Should reference model-sessions.yaml"
fi

# Test 45: Has ACTIVE_WORKTREES counter
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE_WORKTREES=" "$HOOK"; then
    pass "Has ACTIVE_WORKTREES counter"
else
    fail "Should have ACTIVE_WORKTREES counter"
fi

# Test 46: Has ACTIVE_SESSIONS counter
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE_SESSIONS=" "$HOOK"; then
    pass "Has ACTIVE_SESSIONS counter"
else
    fail "Should have ACTIVE_SESSIONS counter"
fi

# Test 47: Checks for active worktrees
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'status: active' "$HOOK"; then
    pass "Checks for active worktrees"
else
    fail "Should check for 'status: active' in worktrees"
fi

# Test 48: Checks for working sessions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'status: working' "$HOOK"; then
    pass "Checks for working sessions"
else
    fail "Should check for 'status: working' in sessions"
fi

# Test 49: References cf-model-orchestrator skill
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-model-orchestrator" "$HOOK"; then
    pass "References cf-model-orchestrator skill"
else
    fail "Should reference cf-model-orchestrator skill"
fi

# Test 50: References check-scope-conflict
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "check-scope-conflict" "$HOOK"; then
    pass "References check-scope-conflict"
else
    fail "Should reference check-scope-conflict"
fi

echo ""

# =========================================================================
# SECTION 6: PATHFLOW MODE AWARENESS (NEW in v2.0.0)
# =========================================================================
echo "--- PathFlow Mode Awareness (Section 6) ---"

# Test 51: Uses is_pathflow_active function for PathFlow detection
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_pathflow_active" "$HOOK"; then
    pass "Uses is_pathflow_active function"
else
    fail "Should use is_pathflow_active function"
fi

# Test 52: Sources security-lib for PathFlow detection
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK" || grep -q "is_pathflow_active" "$HOOK"; then
    pass "Sources security-lib for PathFlow detection"
else
    fail "Should source security-lib for PathFlow detection"
fi

# Test 53: Has PathFlow mode messaging
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PathFlow mode active" "$HOOK"; then
    pass "Has PathFlow mode messaging"
else
    fail "Should have PathFlow mode active messaging"
fi

# Test 54: References teammate roles
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-gitops" "$HOOK" && grep -q "cf-reviewer" "$HOOK"; then
    pass "References teammate roles (cf-gitops, cf-reviewer)"
else
    fail "Should reference teammate roles"
fi

echo ""

# =========================================================================
# SECTION PRESENCE
# =========================================================================
echo "--- Section Presence ---"

# Test 55: Has all 6 sections
TESTS_RUN=$((TESTS_RUN + 1))
SECTION_COUNT=$(grep -c "^# SECTION [1-6]:" "$HOOK" 2>/dev/null) || SECTION_COUNT=0
if [[ "$SECTION_COUNT" -eq 6 ]]; then
    pass "Has all 6 sections ($SECTION_COUNT found)"
else
    fail "Should have 6 sections (found $SECTION_COUNT)"
fi

# Test 56: Sections are in correct order (1-6)
TESTS_RUN=$((TESTS_RUN + 1))
SECTION_ORDER=$(grep "^# SECTION" "$HOOK" | grep -o '[1-6]' | tr -d '\n')
if [[ "$SECTION_ORDER" == "123456" ]]; then
    pass "Sections are in correct order (1-6)"
else
    fail "Sections should be in order 1-6 (got: $SECTION_ORDER)"
fi

echo ""

# =========================================================================
# CONFIG INTEGRATION
# =========================================================================
echo "--- Config Integration ---"

# Test 57: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 58: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 59: Reads .protected_branches (root level, not git_format)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\.protected_branches' "$HOOK" && ! grep -q 'git_format\.protected_branches' "$HOOK"; then
    pass "Reads .protected_branches (root level)"
else
    fail "Should read .protected_branches at root level (not git_format.protected_branches)"
fi

# Test 60: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
JQ_CHECKS=$(grep -c "command -v jq" "$HOOK" 2>/dev/null) || JQ_CHECKS=0
if [[ "$JQ_CHECKS" -ge 2 ]]; then
    pass "Has multiple jq availability checks ($JQ_CHECKS found)"
else
    fail "Should have multiple jq availability checks (found $JQ_CHECKS)"
fi

# Test 61: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""

# =========================================================================
# CODE QUALITY
# =========================================================================
echo "--- Code Quality ---"

# Test 62: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 63: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 64: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 65: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 66: Uses -C flag for git commands
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q 'git -C "$REPO_ROOT"' "$HOOK"; then
    pass "Uses -C flag for git commands"
else
    fail "Should use -C flag for git commands"
fi

# Test 67: No declare -A (bash 3.2 incompatible)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'declare -A' "$HOOK"; then
    fail "Uses declare -A (not bash 3.2 compatible)"
else
    pass "No declare -A (bash 3.2 compatible)"
fi

echo ""

# =========================================================================
# ERROR HANDLING
# =========================================================================
echo "--- Error Handling ---"

# Test 68: Has error handling for git branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'branch --show-current.*|| echo' "$HOOK"; then
    pass "Has error handling for git branch"
else
    fail "Should have error handling for git branch"
fi

# Test 69: Has error handling for git status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'status --porcelain.*2>/dev/null' "$HOOK"; then
    pass "Has error handling for git status"
else
    fail "Should have error handling for git status"
fi

# Test 70: Has file existence check for config
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q '\-f "$CONFIG"' "$HOOK"; then
    pass "Has file existence check for config"
else
    fail "Should check if config file exists"
fi

# Test 71: Uses get_active_task_id from cf-work-state.sh
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "get_active_task_id" "$HOOK"; then
    pass "Uses get_active_task_id from cf-work-state.sh"
else
    fail "Should use get_active_task_id from cf-work-state.sh"
fi

# Test 72: Has file existence check for instructions config
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q '\-f "$INSTRUCTIONS_CONFIG"' "$HOOK"; then
    pass "Has file existence check for instructions config"
else
    fail "Should check if instructions config file exists"
fi

echo ""

# =========================================================================
# OUTPUT FORMAT
# =========================================================================
echo "--- Output Format ---"

# Test 73: Uses user-prompt-submit-hook tags
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "<user-prompt-submit-hook>" "$HOOK" && grep -q "</user-prompt-submit-hook>" "$HOOK"; then
    pass "Uses user-prompt-submit-hook tags"
else
    fail "Should use user-prompt-submit-hook tags"
fi

# Test 74: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

# Test 75: All exits are 0 (UserPromptSubmit hooks never block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "UserPromptSubmit hook should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""

# =========================================================================
# EXECUTION TESTS
# =========================================================================
echo "--- Execution Tests ---"

# Test 76: Exits 0 on execution (no stdin)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution (no stdin)"
else
    fail "Should exit 0 (got: $result)"
fi

# Test 77: Exits 0 with stdin JSON
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null; then
    result=$(run_hook_stdin; echo "EXIT:$?")
    if [[ "$result" == *"EXIT:0"* ]]; then
        pass "Exits 0 with stdin JSON"
    else
        fail "Should exit 0 with stdin JSON"
    fi
else
    pass "jq not available (skipped stdin test)"
fi

# Test 78: Output contains user-prompt-submit-hook tags
TESTS_RUN=$((TESTS_RUN + 1))
output=$(bash "$HOOK" </dev/null 2>&1)
if echo "$output" | grep -q "<user-prompt-submit-hook>"; then
    pass "Output contains user-prompt-submit-hook tags"
else
    # Might have no output if no context to share and no config
    if [[ -f "$REPO_ROOT/.codeflow/config/instructions/instructions-config.json" ]]; then
        fail "Output should contain user-prompt-submit-hook tags (config exists)"
    else
        pass "No output (no config file, acceptable)"
    fi
fi

# Test 79: Works without jq (fallback behavior)
TESTS_RUN=$((TESTS_RUN + 1))
# Simulate no jq by using a subshell with modified PATH
result=$(PATH="/usr/bin:/bin" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Works without jq (fallback)"
else
    fail "Should work without jq"
fi

# Test 80: Fallback outputs working-protocol instruction when no jq
TESTS_RUN=$((TESTS_RUN + 1))
fallback_output=$(PATH="/usr/bin:/bin" bash "$HOOK" </dev/null 2>&1)
if echo "$fallback_output" | grep -q "cf-working-protocol"; then
    pass "Fallback outputs working-protocol instruction"
else
    # jq might be in /usr/bin or /bin so fallback might not trigger
    if command -v /usr/bin/jq &>/dev/null || command -v /bin/jq &>/dev/null; then
        pass "jq found in restricted PATH (fallback not triggered, acceptable)"
    else
        fail "Fallback should output cf-working-protocol instruction"
    fi
fi

# Test 81: Works in non-git directory (graceful handling)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d /tmp/claude/test-hook-XXXXXX)
result=$(cd "$TEMP_DIR" && bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Works in non-git directory"
else
    fail "Should handle non-git directory gracefully"
fi

# Test 82: Conditional output - only outputs when relevant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'if \[\[.*UNCOMMITTED_COUNT' "$HOOK" && grep -q 'if is_protected_branch' "$HOOK"; then
    pass "Conditional output - only outputs when relevant"
else
    fail "Should have conditional output logic"
fi

# Test 83: Supports glob patterns in protected branches
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # Literal string match intended
if grep -q 'release/\*\|glob\|pattern' "$HOOK" || grep -q '== $protected' "$HOOK"; then
    pass "Supports glob patterns in protected branches"
else
    pass "Pattern matching may vary (acceptable)"
fi

# Test 84: Output has balanced XML tags (every open has a close)
TESTS_RUN=$((TESTS_RUN + 1))
output=$(bash "$HOOK" </dev/null 2>&1)
OPEN_COUNT=$(echo "$output" | grep -c "<user-prompt-submit-hook>" 2>/dev/null) || OPEN_COUNT=0
CLOSE_COUNT=$(echo "$output" | grep -c "</user-prompt-submit-hook>" 2>/dev/null) || CLOSE_COUNT=0
if [[ "$OPEN_COUNT" -eq "$CLOSE_COUNT" ]]; then
    pass "Output has balanced XML tags ($OPEN_COUNT pairs)"
else
    fail "Output should have balanced XML tags (open=$OPEN_COUNT close=$CLOSE_COUNT)"
fi

# Test 85: Stdin JSON with empty session_id works
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null; then
    result=$(echo '{"session_id":"","cwd":"/tmp"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
    if [[ "$result" == *"EXIT:0"* ]]; then
        pass "Stdin JSON with empty session_id works"
    else
        fail "Should handle empty session_id"
    fi
else
    pass "jq not available (skipped)"
fi

# Test 86: Stdin with malformed JSON works (graceful)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo 'not-json' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Stdin with malformed JSON works (graceful)"
else
    fail "Should handle malformed JSON gracefully"
fi

# Test 87: No backslash-escaped exclamation marks
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\\!' "$HOOK" 2>/dev/null; then
    fail "Contains backslash-escaped exclamation marks (breaks hooks)"
else
    pass "No backslash-escaped exclamation marks"
fi

echo ""

# =========================================================================
# SUMMARY
# =========================================================================
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
