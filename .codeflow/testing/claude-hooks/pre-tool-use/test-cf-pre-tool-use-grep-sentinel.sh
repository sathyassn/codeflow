#!/usr/bin/env bash
# Test: cf-pre-tool-use-grep-sentinel.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-grep-sentinel.sh
#
# Comprehensive tests for grep sentinel hook:
#   - Tool filtering (Grep only)
#   - Input parsing (pattern, path, glob, type)
#   - File context building
#   - Sentinel library integration
#   - Graceful degradation
#   - Block messages and guidance
#   - Security logging

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-grep-sentinel.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

echo "=== Testing cf-pre-tool-use-grep-sentinel.sh ==="
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

# Test 6: Has Matcher for Grep in header
if grep -q "Matcher:" "$HOOK" && grep -q "Grep" "$HOOK"; then
    pass "Has Matcher for Grep in header"
else
    fail "Should have Matcher for Grep"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Grep tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 8: Exits 0 for non-Grep tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 9: Exits 0 for non-Grep tools (Edit)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 10: Exits 0 for non-Grep tools (Write)
result=$(TOOL_NAME="Write" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Write tool"
else
    fail "Should exit 0 for Write tool"
fi

# Test 11: Exits 0 for non-Grep tools (Glob)
result=$(TOOL_NAME="Glob" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Glob tool"
else
    fail "Should exit 0 for Glob tool"
fi

# Test 12: Exits 0 when no TOOL_NAME
result=$(TOOL_NAME="" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_NAME"
else
    fail "Should exit 0 when no TOOL_NAME"
fi

echo ""
echo "--- Input Validation ---"

# Test 13: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Grep" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 14: Exits 0 when empty JSON
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty JSON"
else
    fail "Should exit 0 when empty JSON"
fi

# Test 15: Exits 0 when no file context (pattern only)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"pattern":"test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no file context"
else
    fail "Should exit 0 when no file context"
fi

# Test 16: Exits 0 when no pattern (path only)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"path":"/some/path"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no pattern"
else
    fail "Should exit 0 when no pattern"
fi

# Test 17: Exits 0 when no pattern (type only)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"type":"py"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when type but no pattern"
else
    fail "Should exit 0 when type but no pattern"
fi

# Test 18: Exits 0 when no pattern (glob only)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"glob":"*.py"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when glob but no pattern"
else
    fail "Should exit 0 when glob but no pattern"
fi

echo ""
echo "--- File Context Building ---"

# Test 19: Has type filter extraction
if grep -q "TYPE_FILTER" "$HOOK"; then
    pass "Has type filter extraction"
else
    fail "Should extract type filter"
fi

# Test 20: Has path filter extraction
if grep -q "PATH_FILTER" "$HOOK"; then
    pass "Has path filter extraction"
else
    fail "Should extract path filter"
fi

# Test 21: Has glob filter extraction
if grep -q "GLOB_FILTER" "$HOOK"; then
    pass "Has glob filter extraction"
else
    fail "Should extract glob filter"
fi

# Test 22: Type filter takes priority for file context
if grep -q 'if \[\[ -n "\$TYPE_FILTER" \]\]' "$HOOK"; then
    pass "Type filter takes priority"
else
    fail "Type filter should take priority"
fi

# Test 23: Builds file context from type (adds dot prefix)
if grep -q 'FILE_CONTEXT=".\$TYPE_FILTER"' "$HOOK"; then
    pass "Adds dot prefix to type filter"
else
    fail "Should add dot prefix to type filter"
fi

echo ""
echo "--- Graceful Degradation ---"

# Test 24: Exits 0 when sentinel library unavailable
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"pattern":"function.*test","type":"py"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when sentinel library unavailable"
else
    fail "Should exit 0 when sentinel library unavailable"
fi

# Test 25: Exits 0 for regular grep with path
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"pattern":"test","path":"src/"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for regular grep with path"
else
    fail "Should exit 0 for regular grep with path"
fi

# Test 26: Exits 0 for regular grep with glob
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"pattern":"import","glob":"*.ts"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for regular grep with glob"
else
    fail "Should exit 0 for regular grep with glob"
fi

# Test 27: Exits 0 for regular grep with type
result=$(TOOL_NAME="Grep" TOOL_INPUT='{"pattern":"class","type":"js"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for regular grep with type"
else
    fail "Should exit 0 for regular grep with type"
fi

echo ""
echo "--- Sentinel Library Integration ---"

# Test 28: References sentinel library
if grep -q "SENTINEL_LIB" "$HOOK" && grep -q "cf-sentinel.sh" "$HOOK"; then
    pass "References sentinel library"
else
    fail "Should reference sentinel library"
fi

# Test 29: Has sentinel_find_skill_for_grep call
if grep -q "sentinel_find_skill_for_grep" "$HOOK"; then
    pass "Has sentinel_find_skill_for_grep call"
else
    fail "Should call sentinel_find_skill_for_grep"
fi

# Test 30: Has sentinel_validate call
if grep -q "sentinel_validate" "$HOOK"; then
    pass "Has sentinel_validate call"
else
    fail "Should call sentinel_validate"
fi

# Test 31: Has sentinel_get_operation_guidance call
if grep -q "sentinel_get_operation_guidance" "$HOOK"; then
    pass "Has sentinel_get_operation_guidance call"
else
    fail "Should call sentinel_get_operation_guidance"
fi

# Test 32: Checks if functions exist before calling
if grep -q "declare -f sentinel_find_skill_for_grep" "$HOOK"; then
    pass "Checks sentinel functions exist before calling"
else
    fail "Should check functions exist"
fi

echo ""
echo "--- Block Messages ---"

# Test 33: Has BLOCKED output message
if grep -q "BLOCKED" "$HOOK"; then
    pass "Has BLOCKED output message"
else
    fail "Should have BLOCKED output message"
fi

# Test 34: Has MUST: Skill direction
if grep -q "MUST:" "$HOOK" && grep -q "Skill" "$HOOK"; then
    pass "Has MUST: Skill direction"
else
    fail "Should have MUST: Skill direction"
fi

# Test 35: Shows pattern in block message
if grep -q 'Pattern: \$PATTERN' "$HOOK"; then
    pass "Shows pattern in block message"
else
    fail "Should show pattern in block message"
fi

# Test 36: Shows required skill in block message
if grep -q 'Skill: \$REQUIRED_SKILL' "$HOOK"; then
    pass "Shows required skill in block message"
else
    fail "Should show required skill"
fi

# Test 37: Mentions LSP-first approach
if grep -q "LSP" "$HOOK"; then
    pass "Mentions LSP-first approach"
else
    fail "Should mention LSP-first approach"
fi

# Test 38: Shows guidance when available
if grep -q 'GUIDANCE' "$HOOK" && grep -q 'echo "\$GUIDANCE"' "$HOOK"; then
    pass "Shows guidance when available"
else
    fail "Should show guidance when available"
fi

echo ""
echo "--- Security Logging ---"

# Test 39: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should have security event logging"
fi

# Test 40: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

# Test 41: Logs allowed operations (audit)
if grep -q 'log_security_event.*audit' "$HOOK"; then
    pass "Logs allowed operations (audit)"
else
    fail "Should log allowed operations"
fi

# Test 42: Sources security library
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security library"
else
    fail "Should source security library"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 43: References enforcement-policy.json (via sentinel lib)
if grep -q "enforcement-policy" "$HOOK" || grep -q "sentinel" "$HOOK"; then
    pass "Uses config-driven approach via sentinel"
else
    fail "Should use config-driven approach"
fi

# Test 44: Documents config-driven approach in header
if grep -q "Configuration:" "$HOOK" && grep -q "enforcement-policy.json" "$HOOK"; then
    pass "Documents config-driven approach"
else
    fail "Should document config-driven approach"
fi

echo ""
echo "--- Code Quality ---"

# Test 45: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 46: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 47: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 48: Uses jq for JSON parsing
if grep -q "command -v jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 49: Has jq fallback for input parsing
if grep -q "grep -o" "$HOOK" && grep -q "pattern" "$HOOK"; then
    pass "Has jq fallback for input parsing"
else
    fail "Should have jq fallback"
fi

# Test 50: Documents bash 3.2+ compatibility
if grep -q "bash 3.2" "$HOOK" || grep -q "Compatibility" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash compatibility"
fi

# Test 51: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK"; then
    pass "Has exit code 2 for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

# Test 52: Parses skill:operation result correctly
if grep -q 'REQUIRED_SKILL=.*RESULT%%' "$HOOK" && grep -q 'OPERATION=.*RESULT#' "$HOOK"; then
    pass "Parses skill:operation result correctly"
else
    fail "Should parse skill:operation correctly"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
