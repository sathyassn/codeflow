#!/usr/bin/env bash
# Test: cf-pre-tool-use-read-delegation.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-read-delegation.sh
#
# Comprehensive tests for read delegation hook:
#   - Tool filtering (Read only)
#   - Allowed paths (bypass checks)
#   - Always-block patterns (require delegation)
#   - Threshold rules (line counting)
#   - Settings config
#   - V3 spec compliance

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

echo "=== Testing cf-pre-tool-use-read-delegation.sh ==="
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
    if shellcheck -e SC1091,SC2034 "$HOOK" 2>/dev/null; then
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

# Test 6: Has Matcher for Read in header
if grep -q "Matcher:" "$HOOK" && grep -q "Read" "$HOOK"; then
    pass "Has Matcher for Read in header"
else
    fail "Should have Matcher for Read"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Read tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 8: Exits 0 for non-Read tools (Edit)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 9: Exits 0 for non-Read tools (Write)
result=$(TOOL_NAME="Write" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Write tool"
else
    fail "Should exit 0 for Write tool"
fi

# Test 10: Exits 0 for non-Read tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 11: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Read" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 12: Exits 0 when empty file_path
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty file_path"
else
    fail "Should exit 0 when empty file_path"
fi

echo ""
echo "--- Allowed Paths (bypass checks) ---"

# Test 13: Allows reading .claude/CLAUDE.md
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":".claude/CLAUDE.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows reading .claude/CLAUDE.md"
else
    fail "Should allow reading .claude/CLAUDE.md"
fi

# Test 14: Allows reading .claude/settings.json
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":".claude/settings.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows reading .claude/settings.json"
else
    fail "Should allow reading .claude/settings.json"
fi

# Test 15: Allows reading README.md
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"README.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows reading README.md"
else
    fail "Should allow reading README.md"
fi

# Test 16: Allows reading package.json
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"package.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows reading package.json"
else
    fail "Should allow reading package.json"
fi

# Test 17: Allows reading regular files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"/tmp/test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows reading regular files"
else
    fail "Should allow reading regular files"
fi

echo ""
echo "--- Always-Block Patterns ---"

# Test 18: Blocks .jsonl files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"data/log.jsonl"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .jsonl files"
else
    fail "Should block .jsonl files"
fi

# Test 19: Blocks .db files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"cache/data.db"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .db files"
else
    fail "Should block .db files"
fi

# Test 20: Blocks .surrealdb files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"data/app.surrealdb"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .surrealdb files"
else
    fail "Should block .surrealdb files"
fi

# Test 21: Blocks .log files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"logs/app.log"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .log files"
else
    fail "Should block .log files"
fi

# Test 22: Blocks .claude/memory/** files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":".claude/memory/work/session.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .claude/memory/** files"
else
    fail "Should block .claude/memory/** files"
fi

# Test 23: Blocks .codeflow/state/** files
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":".codeflow/state/sessions.db"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks .codeflow/state/** files"
else
    fail "Should block .codeflow/state/** files"
fi

echo ""
echo "--- Block Messages ---"

# Test 24: Block message includes permissionDecision: deny
HOOK_OUTPUT=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"data/log.jsonl"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"permissionDecision: deny"* ]]; then
    pass "Block message includes permissionDecision: deny"
else
    fail "Should include permissionDecision: deny"
fi

# Test 25: Block message includes delegation target
if [[ "$HOOK_OUTPUT" == *"Task"* ]] && [[ "$HOOK_OUTPUT" == *"subagent_type"* ]]; then
    pass "Block message includes delegation target"
else
    fail "Should include delegation target"
fi

# Test 26: Block message includes MUST: directive
if [[ "$HOOK_OUTPUT" == *"MUST:"* ]]; then
    pass "Block message includes MUST: directive"
else
    fail "Should include MUST: directive"
fi

# Test 27: Block message shows file path
if [[ "$HOOK_OUTPUT" == *"Path:"* ]]; then
    pass "Block message shows file path"
else
    fail "Should show file path in block message"
fi

echo ""
echo "--- V3 Spec Compliance ---"

# Test 28: Has ALLOWED_PATHS array
if grep -q "ALLOWED_PATHS" "$HOOK"; then
    pass "Has ALLOWED_PATHS array (V3 spec)"
else
    fail "Should have ALLOWED_PATHS array"
fi

# Test 29: Has ALWAYS_BLOCK_PATTERNS array
if grep -q "ALWAYS_BLOCK_PATTERNS" "$HOOK"; then
    pass "Has ALWAYS_BLOCK_PATTERNS array (V3 spec)"
else
    fail "Should have ALWAYS_BLOCK_PATTERNS array"
fi

# Test 30: Has THRESHOLD_RULES associative array
if grep -q "THRESHOLD_RULES" "$HOOK"; then
    pass "Has THRESHOLD_RULES array (V3 spec)"
else
    fail "Should have THRESHOLD_RULES array"
fi

# Test 31: Has permissionDecision output format
if grep -q "permissionDecision" "$HOOK"; then
    pass "Has permissionDecision output format"
else
    fail "Should output permissionDecision format"
fi

# Test 32: Has delegation target configuration
if grep -q "DELEGATION_TARGET" "$HOOK"; then
    pass "Has delegation target configuration"
else
    fail "Should have delegation target configuration"
fi

echo ""
echo "--- Settings Config ---"

# Test 33: Reads from settings.local.json
if grep -q "settings.local.json" "$HOOK"; then
    pass "Reads from settings.local.json"
else
    fail "Should read from settings.local.json"
fi

# Test 34: Has get_settings_config function
if grep -q "get_settings_config" "$HOOK"; then
    pass "Has get_settings_config function"
else
    fail "Should have get_settings_config function"
fi

# Test 35: Has DELEGATION_ENABLED setting
if grep -q "DELEGATION_ENABLED" "$HOOK"; then
    pass "Has DELEGATION_ENABLED setting"
else
    fail "Should have DELEGATION_ENABLED setting"
fi

# Test 36: Has ALWAYS_BLOCK_ENABLED setting
if grep -q "ALWAYS_BLOCK_ENABLED" "$HOOK"; then
    pass "Has ALWAYS_BLOCK_ENABLED setting"
else
    fail "Should have ALWAYS_BLOCK_ENABLED setting"
fi

echo ""
echo "--- Config Loading ---"

# Test 37: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 38: Reads read_delegation section from config
if grep -q "read_delegation" "$HOOK"; then
    pass "Reads read_delegation section from config"
else
    fail "Should read read_delegation from config"
fi

# Test 39: Loads allowed_paths from config
if grep -q "allowed_paths" "$HOOK"; then
    pass "Loads allowed_paths from config"
else
    fail "Should load allowed_paths from config"
fi

# Test 40: Loads always_block_patterns from config
if grep -q "always_block_patterns" "$HOOK"; then
    pass "Loads always_block_patterns from config"
else
    fail "Should load always_block_patterns from config"
fi

# Test 41: Loads threshold_rules from config
if grep -q "threshold_rules" "$HOOK"; then
    pass "Loads threshold_rules from config"
else
    fail "Should load threshold_rules from config"
fi

echo ""
echo "--- Pattern Matching ---"

# Test 42: Has glob_to_regex function
if grep -q "glob_to_regex()" "$HOOK"; then
    pass "Has glob_to_regex function"
else
    fail "Should have glob_to_regex function"
fi

# Test 43: Has matches_pattern_list function
if grep -q "matches_pattern_list()" "$HOOK"; then
    pass "Has matches_pattern_list function"
else
    fail "Should have matches_pattern_list function"
fi

# Test 44: Has matches_extension_pattern function
if grep -q "matches_extension_pattern()" "$HOOK"; then
    pass "Has matches_extension_pattern function"
else
    fail "Should have matches_extension_pattern function"
fi

echo ""
echo "--- Threshold Rules ---"

# Test 45: Has line counting with wc -l
if grep -q "wc -l" "$HOOK"; then
    pass "Has line counting with wc -l"
else
    fail "Should count lines with wc -l"
fi

# Test 46: Checks file extension for threshold
if grep -q "EXTENSION" "$HOOK"; then
    pass "Checks file extension for threshold"
else
    fail "Should check file extension"
fi

# Test 47: Has default threshold values
if grep -q '["default"]' "$HOOK" || grep -q 'default.*400' "$HOOK"; then
    pass "Has default threshold values"
else
    fail "Should have default threshold values"
fi

# Test 48: Threshold warning includes line count
if grep -q 'Lines:' "$HOOK"; then
    pass "Threshold warning includes line count"
else
    fail "Should include line count in warning"
fi

echo ""
echo "--- Security Logging ---"

# Test 49: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should have security event logging"
fi

# Test 50: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

# Test 51: Logs allowed operations (audit)
if grep -q 'log_security_event.*audit' "$HOOK"; then
    pass "Logs allowed operations (audit)"
else
    fail "Should log allowed operations"
fi

# Test 52: Sources security library
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security library"
else
    fail "Should source security library"
fi

echo ""
echo "--- Code Quality ---"

# Test 53: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 54: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 55: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 56: Documents bash 3.2+ compatibility
if grep -q "bash 3.2" "$HOOK" || grep -q "Compatibility" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash compatibility"
fi

# Test 57: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK"; then
    pass "Has exit code 2 for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

# Test 58: Normalizes paths to relative
if grep -q 'REL_PATH=.*REPO_ROOT' "$HOOK"; then
    pass "Normalizes paths to relative"
else
    fail "Should normalize paths"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
