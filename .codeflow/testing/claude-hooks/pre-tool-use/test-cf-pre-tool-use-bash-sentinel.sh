#!/usr/bin/env bash
# Test: cf-pre-tool-use-bash-sentinel.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-bash-sentinel.sh
#
# Tests for Bash sentinel hook:
#   - Sandbox bypass validation
#   - Config-driven pattern matching
#   - Sentinel validation
#   - Cross-skill prerequisite checks
#   - Non-protected commands pass-through
#
# Based on workflow repo test structure.

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-bash-sentinel.sh"
SENTINEL_DIR="$REPO_ROOT/.state/sentinels/skill"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

# Helper: Run hook with command and optional sandbox bypass
run_bash_sentinel() {
    local command="$1"
    local sandbox_bypass="${2:-false}"

    local escaped_command
    escaped_command=$(printf '%s' "$command" | sed 's/\\/\\\\/g; s/"/\\"/g')

    local json_input
    if [[ "$sandbox_bypass" == "true" ]]; then
        json_input="{\"command\": \"$escaped_command\", \"dangerouslyDisableSandbox\": true}"
    else
        json_input="{\"command\": \"$escaped_command\"}"
    fi

    local output exit_code
    output=$(TOOL_NAME="Bash" TOOL_INPUT="$json_input" bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?

    HOOK_OUTPUT="$output"
    HOOK_EXIT_CODE=$exit_code
}

# Helper: Create a sentinel file
create_test_sentinel() {
    local skill="$1"
    local operation="$2"
    local ttl="${3:-300}"

    mkdir -p "$SENTINEL_DIR"

    local id
    id="test-$$-$(date +%s)"
    local filename="${skill}:${operation}-${id}.json"
    local created expires
    created=$(date +%s)
    expires=$((created + ttl))

    cat > "$SENTINEL_DIR/$filename" << EOF
{
  "sentinel": "${skill}:${operation}",
  "skill": "$skill",
  "operation": "$operation",
  "created": $created,
  "expires": $expires
}
EOF
    echo "$SENTINEL_DIR/$filename"
}

# Helper: Cleanup test sentinels
cleanup_test_sentinels() {
    rm -f "$SENTINEL_DIR"/*test-*.json 2>/dev/null || true
}

echo "=== Testing cf-pre-tool-use-bash-sentinel.sh ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

# Test 1: File exists
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    skip "Shellcheck not available"
fi

# Test 4: Uses strict mode
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 5: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

echo ""
echo "--- Config-Driven Architecture ---"

# Test 6: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json config"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 7: Has sentinel library loading
if grep -q "SENTINEL_LIB\|cf-sentinel.sh" "$HOOK"; then
    pass "Has sentinel library loading"
else
    fail "Should load sentinel library"
fi

# Test 8: Has fallback when sentinel library not available
if grep -q "SENTINEL_LIB_LOADED" "$HOOK" && grep -q "Fallback" "$HOOK"; then
    pass "Has fallback for missing sentinel library"
else
    fail "Should have fallback logic"
fi

# Test 9: No hardcoded git patterns (config-driven)
# The hook should NOT have hardcoded patterns like "^git commit" outside fallback
if grep -q "sentinel_find_skill_for_command" "$HOOK"; then
    pass "Uses config-driven pattern matching"
else
    fail "Should use sentinel_find_skill_for_command for pattern matching"
fi

# Test 10: Reads skills from config
if grep -q '\.skills\[' "$HOOK" || grep -q 'sentinel_find_skill_for_command' "$HOOK"; then
    pass "Reads skills configuration from config"
else
    fail "Should read skills from config"
fi

echo ""
echo "--- Sandbox Bypass ---"

# Test 11: Has sandbox bypass check
if grep -q "dangerouslyDisableSandbox" "$HOOK" && grep -q "SANDBOX_BYPASS" "$HOOK"; then
    pass "Has sandbox bypass check"
else
    fail "Should check dangerouslyDisableSandbox parameter"
fi

# Test 12: Blocks sandbox bypass without sentinel
cleanup_test_sentinels
run_bash_sentinel "git push origin main" "true"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks sandbox bypass without sentinel"
else
    fail "Should block sandbox bypass without sentinel (got exit $HOOK_EXIT_CODE)"
fi

# Test 13: Normal command without sandbox bypass passes
run_bash_sentinel "ls -la" "false"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Normal command without sandbox bypass passes"
else
    fail "Should allow normal command without sandbox bypass"
fi

echo ""
echo "--- Cross-Skill Prerequisites ---"

# Test 14: Has cross-skill prerequisite checking
if grep -q "cross_skill_prerequisite\|cross_prereq" "$HOOK"; then
    pass "Has cross-skill prerequisite checking"
else
    fail "Should check cross-skill prerequisites"
fi

# Test 15: Reads prerequisite config from JSON
if grep -q "cross_skill_prerequisite.skill\|cross_prereq_skill" "$HOOK"; then
    pass "Reads prerequisite skill from config"
else
    fail "Should read prerequisite skill from config"
fi

# Test 16: Reads prerequisite operation from config
if grep -q "cross_skill_prerequisite.operation\|cross_prereq_op" "$HOOK"; then
    pass "Reads prerequisite operation from config"
else
    fail "Should read prerequisite operation from config"
fi

echo ""
echo "--- Blocked Operations ---"

# Test 17: Has blocked operation check
if grep -q '\.block' "$HOOK" && grep -q "is_blocked" "$HOOK"; then
    pass "Has blocked operation check"
else
    fail "Should check for blocked operations"
fi

# Test 18: Block message mentions user confirmation
if grep -q "user confirmation\|Ask the user" "$HOOK"; then
    pass "Block message mentions user confirmation"
else
    fail "Should mention user confirmation for blocked ops"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 19: Exits 0 for non-Bash tools
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-Bash tools"
else
    fail "Should exit 0 for non-Bash tools"
fi

# Test 20: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Bash" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 21: Exits 0 when empty command
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty command"
else
    fail "Should exit 0 when empty command"
fi

echo ""
echo "--- Non-Protected Commands ---"

# Test 22: Allows ls command
run_bash_sentinel "ls -la"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows ls command"
else
    fail "Should allow ls command"
fi

# Test 23: Allows pwd command
run_bash_sentinel "pwd"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows pwd command"
else
    fail "Should allow pwd command"
fi

# Test 24: Allows cat command
run_bash_sentinel "cat file.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows cat command"
else
    fail "Should allow cat command"
fi

# Test 25: Allows git status
run_bash_sentinel "git status"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows git status"
else
    fail "Should allow git status"
fi

# Test 26: Allows git diff
run_bash_sentinel "git diff"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows git diff"
else
    fail "Should allow git diff"
fi

# Test 27: Allows git log
run_bash_sentinel "git log -n 5"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows git log"
else
    fail "Should allow git log"
fi

# Test 28: Allows git branch (listing)
run_bash_sentinel "git branch"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows git branch (listing)"
else
    fail "Should allow git branch listing"
fi

# Test 29: Allows npm commands
run_bash_sentinel "npm install"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows npm commands"
else
    fail "Should allow npm commands"
fi

# Test 30: Allows node commands
run_bash_sentinel "node --version"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows node commands"
else
    fail "Should allow node commands"
fi

echo ""
echo "--- Protected Commands (without sentinel) ---"

cleanup_test_sentinels

# Test 31: Blocks git commit without sentinel
run_bash_sentinel "git commit -m 'test'"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks git commit without sentinel"
else
    fail "Should block git commit without sentinel (exit=$HOOK_EXIT_CODE)"
fi

# Test 32: Blocks git push without sentinel
run_bash_sentinel "git push origin main"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks git push without sentinel"
else
    fail "Should block git push without sentinel (exit=$HOOK_EXIT_CODE)"
fi

# Test 33: Blocks gh pr create without sentinel
run_bash_sentinel "gh pr create --title 'test'"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks gh pr create without sentinel"
else
    fail "Should block gh pr create without sentinel (exit=$HOOK_EXIT_CODE)"
fi

# Test 34: Block message mentions required skill
run_bash_sentinel "git commit -m 'test'"
if [[ "$HOOK_OUTPUT" == *"MUST:"* ]] && [[ "$HOOK_OUTPUT" == *"Skill"* ]]; then
    pass "Block message mentions required skill"
else
    fail "Block message should mention required skill"
fi

echo ""
echo "--- Logging ---"

# Test 35: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should log security events"
fi

# Test 36: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

echo ""
echo "--- TTL/Expiry Handling ---"

# Test 37: Has expiry checking
if grep -q "expires" "$HOOK"; then
    pass "Has expiry checking"
else
    fail "Should check sentinel expiry"
fi

# Test 38: Compares with current time
if grep -q 'date +%s' "$HOOK" || grep -q "now.*expires\|expires.*now" "$HOOK"; then
    pass "Compares expiry with current time"
else
    fail "Should compare expiry with current time"
fi

cleanup_test_sentinels

echo ""
echo "--- V4: PathFlow Mode-Awareness ---"

# Test 39: Hook contains PATHFLOW_MODE reference
if grep -q "PATHFLOW_MODE" "$HOOK"; then
    pass "Hook contains PATHFLOW_MODE reference"
else
    fail "Should contain PATHFLOW_MODE reference"
fi

# Test 40: Hook contains V4 PathFlow comment section
if grep -q "V4: PATHFLOW MODE CHECK" "$HOOK"; then
    pass "Hook contains V4 PathFlow comment section"
else
    fail "Should contain V4 PathFlow comment section"
fi

# Test 41: Hook checks for PathFlow sentinels directory
if grep -q "PATHFLOW_SENTINEL_DIR" "$HOOK" && grep -q '\.state/sentinels' "$HOOK"; then
    pass "Hook checks for PathFlow sentinels directory"
else
    fail "Should check PathFlow sentinels directory"
fi

# Test 42: Hook checks is_pathflow_active
if grep -q "is_pathflow_active" "$HOOK"; then
    pass "Hook checks is_pathflow_active"
else
    fail "Should check is_pathflow_active"
fi

# Test 43: PathFlow mode checks for PF-3 sentinel
if grep -q "pathflow-pf-3" "$HOOK"; then
    pass "PathFlow mode checks for PF-3 sentinel"
else
    fail "Should check for PF-3 sentinel in PathFlow mode"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
