#!/usr/bin/env bash
# Test: cf-post-tool-use-skill.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-skill.sh
#
# Tests PostToolUse skill hook for sentinel creation and logging
# Verifies V3 sentinel enforcement after skill invocation

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-skill.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-skill.sh ==="
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

# Test 8: Has Matcher header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Matcher:" "$HOOK"; then
    pass "Has Matcher header"
else
    fail "Should have Matcher header"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 9: Exits 0 for non-Skill tools
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-Skill tools"
else
    fail "Should exit 0 for non-Skill tools"
fi

# Test 10: Exits 0 for Skill tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{"skill":"cf-git-workflow"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Skill tool"
else
    fail "Should exit 0 for Skill tool"
fi

# Test 11: Has TOOL_NAME check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TOOL_NAME" "$HOOK"; then
    pass "Has TOOL_NAME check"
else
    fail "Should check TOOL_NAME"
fi

# Test 12: Exits 0 for Edit tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 13: Exits 0 for Write tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Write tool"
else
    fail "Should exit 0 for Write tool"
fi

# Test 14: Exits 0 for Read tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

echo ""
echo "--- Skill Parsing ---"

# Test 15: Has Skill tool check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"Skill"' "$HOOK" || grep -q "'Skill'" "$HOOK"; then
    pass "Has Skill tool check"
else
    fail "Should check for Skill tool"
fi

# Test 16: Processes skill name
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SKILL_NAME" "$HOOK"; then
    pass "Processes skill name"
else
    fail "Should process skill name"
fi

# Test 17: Processes skill args
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SKILL_ARGS" "$HOOK"; then
    pass "Processes skill args"
else
    fail "Should process skill args"
fi

# Test 18: Normalizes skill name (removes cf- prefix)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SKILL_BASE" "$HOOK" && grep -q "#cf-" "$HOOK"; then
    pass "Normalizes skill name"
else
    fail "Should normalize skill name"
fi

# Test 19: Extracts skill from JSON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq.*skill" "$HOOK"; then
    pass "Extracts skill from JSON"
else
    fail "Should extract skill from JSON"
fi

# Test 20: Extracts args from JSON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq.*args" "$HOOK"; then
    pass "Extracts args from JSON"
else
    fail "Should extract args from JSON"
fi

echo ""
echo "--- Sentinel Integration ---"

# Test 21: Has sentinel library sourcing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_LIB" "$HOOK" && grep -q "source.*SENTINEL_LIB" "$HOOK"; then
    pass "Has sentinel library sourcing"
else
    fail "Should source sentinel library"
fi

# Test 22: Has extract_operation function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "extract_operation" "$HOOK"; then
    pass "Has extract_operation function"
else
    fail "Should have extract_operation function"
fi

# Test 23: Has create_skill_sentinel function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_skill_sentinel" "$HOOK"; then
    pass "Has create_skill_sentinel function"
else
    fail "Should have create_skill_sentinel function"
fi

# Test 24: Checks for sentinel_create function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel_create" "$HOOK"; then
    pass "Calls sentinel_create function"
else
    fail "Should call sentinel_create function"
fi

# Test 25: Gets operation pattern from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel_get_operation_pattern" "$HOOK"; then
    pass "Gets operation pattern from config"
else
    fail "Should get operation pattern from config"
fi

# Test 26: Gets operation TTL from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel_get_operation_ttl" "$HOOK"; then
    pass "Gets operation TTL from config"
else
    fail "Should get operation TTL from config"
fi

# Test 27: Handles missing sentinel library gracefully
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "declare -f sentinel_create" "$HOOK"; then
    pass "Checks if sentinel library loaded"
else
    fail "Should check if sentinel library loaded"
fi

echo ""
echo "--- Skill Success Detection ---"

# Test 28: Has skill success detection
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SKILL_SUCCESS" "$HOOK"; then
    pass "Has skill success detection"
else
    fail "Should detect skill success/failure"
fi

# Test 29: Detects error in result
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"error"' "$HOOK" || grep -q "'error'" "$HOOK"; then
    pass "Detects error in result"
else
    fail "Should detect error in result"
fi

# Test 30: Detects BLOCKED in result
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "BLOCKED" "$HOOK"; then
    pass "Detects BLOCKED in result"
else
    fail "Should detect BLOCKED in result"
fi

# Test 31: Only creates sentinel on success
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'SKILL_SUCCESS.*true' "$HOOK" || grep -q 'if.*SKILL_SUCCESS' "$HOOK"; then
    pass "Only creates sentinel on success"
else
    fail "Should only create sentinel on success"
fi

echo ""
echo "--- Logging ---"

# Test 32: Has sentinel creation logging
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "log_sentinel_creation" "$HOOK"; then
    pass "Has sentinel creation logging function"
else
    fail "Should have sentinel creation logging"
fi

# Test 33: Has security log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SECURITY_LOG_DIR" "$HOOK"; then
    pass "Has security log directory"
else
    fail "Should have security log directory"
fi

# Test 34: Creates log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir -p.*LOG_DIR" "$HOOK" || grep -q "mkdir -p.*SECURITY_LOG_DIR" "$HOOK"; then
    pass "Creates log directory"
else
    fail "Should create log directory"
fi

# Test 35: Has session log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_DIR" "$HOOK"; then
    pass "Has session log directory"
else
    fail "Should have session log directory"
fi

# Test 36: Logs to JSONL format
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".jsonl" "$HOOK"; then
    pass "Logs to JSONL format"
else
    fail "Should log to JSONL format"
fi

# Test 37: Creates log entry with jq
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Creates log entry with jq"
else
    fail "Should create log entry with jq"
fi

# Test 38: Logs skill_completed event
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "skill_completed" "$HOOK"; then
    pass "Logs skill_completed event"
else
    fail "Should log skill_completed event"
fi

# Test 39: Logs sentinel_created event
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel_created" "$HOOK"; then
    pass "Logs sentinel_created event"
else
    fail "Should log sentinel_created event"
fi

# Test 40: Has session ID in logging
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_ID" "$HOOK" && grep -q "session_id" "$HOOK"; then
    pass "Has session ID in logging"
else
    fail "Should include session ID in logs"
fi

echo ""
echo "--- Code Quality ---"

# Test 41: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 42: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 43: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 44: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 45: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "exit 0" "$HOOK"; then
    pass "Has exit 0"
else
    fail "Should have exit 0"
fi

# Test 46: All exits are 0 (PostToolUse should not block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "PostToolUse should only have exit 0"
else
    pass "All exits are 0 (PostToolUse should not block)"
fi

echo ""
echo "--- Functional Tests ---"

# Test 47: Processes skill with args
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{"skill":"cf-git-workflow","args":"create-commit"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Processes skill with args"
else
    fail "Should process skill with args"
fi

# Test 48: Handles empty skill name
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{"skill":""}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty skill name gracefully"
else
    fail "Should handle empty skill name"
fi

# Test 49: Handles missing skill field
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing skill field gracefully"
else
    fail "Should handle missing skill field"
fi

# Test 50: Handles malformed JSON
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='not json' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles malformed JSON gracefully"
else
    fail "Should handle malformed JSON"
fi

# Test 51: Handles empty TOOL_INPUT
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty TOOL_INPUT gracefully"
else
    fail "Should handle empty TOOL_INPUT"
fi

# Test 52: Handles no TOOL_NAME
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles no TOOL_NAME gracefully"
else
    fail "Should handle no TOOL_NAME"
fi

# Test 53: Handles skill with TOOL_RESULT error
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{"skill":"cf-test"}' TOOL_RESULT='{"error":"test error"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles skill with error result"
else
    fail "Should handle error result"
fi

# Test 54: Handles skill with BLOCKED result
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Skill" TOOL_INPUT='{"skill":"cf-test"}' TOOL_RESULT='BLOCKED: missing sentinel' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles skill with BLOCKED result"
else
    fail "Should handle BLOCKED result"
fi

echo ""
echo "--- Config Integration ---"

# Test 55: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 56: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
