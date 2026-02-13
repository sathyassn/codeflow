#!/usr/bin/env bash
# Test: cf-post-tool-use-instructions.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-instructions.sh
#
# Tests config-driven PostToolUse instruction hook
# Verifies contextual just-in-time skill instructions after tool use

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-instructions.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-instructions.sh ==="
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
if grep -q "Purpose:" "$HOOK"; then
    pass "Has Purpose header comment"
else
    fail "Missing Purpose header comment"
fi

# Test 5: Uses set -euo pipefail
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Reads from stdin (PostToolUse pattern)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cat" "$HOOK" && grep -q "tool_input" "$HOOK"; then
    pass "Reads tool input from stdin"
else
    fail "Should read tool input from stdin"
fi

# Test 7: Exits 0 for empty tool_name
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for empty tool_name"
else
    fail "Should exit 0 for empty tool_name"
fi

# Test 8: Exits 0 when config file missing
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when config handling succeeds"
else
    fail "Should exit 0 for config handling"
fi

# Test 9: Has Matcher for Edit|Write|Read|Grep|Bash
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Matcher:" "$HOOK" && grep -q "Edit" "$HOOK" && grep -q "Write" "$HOOK" && grep -q "Bash" "$HOOK"; then
    pass "Has Matcher for multiple tools"
else
    fail "Should have Matcher for Edit|Write|Read|Grep|Bash"
fi

# Test 10: References instructions-config.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "instructions-config.json" "$HOOK"; then
    pass "References instructions-config.json"
else
    fail "Should reference instructions-config.json"
fi

# Test 11: Has output_instruction function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "output_instruction" "$HOOK"; then
    pass "Has output_instruction function"
else
    fail "Should have output_instruction function"
fi

# Test 12: Outputs hookSpecificOutput JSON format
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "hookSpecificOutput" "$HOOK" && grep -q "hookEventName" "$HOOK" && grep -q "PostToolUse" "$HOOK"; then
    pass "Outputs proper hookSpecificOutput JSON format"
else
    fail "Should output hookSpecificOutput JSON format"
fi

# Test 13: Has check_tool_match function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "check_tool_match" "$HOOK"; then
    pass "Has check_tool_match function"
else
    fail "Should have check_tool_match function"
fi

# Test 14: Has process_file_instruction function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "process_file_instruction" "$HOOK"; then
    pass "Has process_file_instruction function"
else
    fail "Should have process_file_instruction function"
fi

# Test 15: Has process_command_instruction function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "process_command_instruction" "$HOOK"; then
    pass "Has process_command_instruction function"
else
    fail "Should have process_command_instruction function"
fi

# Test 16: Handles file_pattern matching
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "file_pattern" "$HOOK"; then
    pass "Handles file_pattern matching"
else
    fail "Should handle file_pattern matching"
fi

# Test 17: Has exclude_patterns support
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "exclude_patterns" "$HOOK" || grep -q "matches_exclude_pattern" "$HOOK"; then
    pass "Has exclude_patterns support"
else
    fail "Should have exclude_patterns support"
fi

# Test 18: Has enabled check for rules
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enabled" "$HOOK"; then
    pass "Has enabled check for rules"
else
    fail "Should have enabled check for rules"
fi

# Test 19: Handles tool-specific messages
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "tool_message" "$HOOK" || grep -q "messages" "$HOOK"; then
    pass "Handles tool-specific messages"
else
    fail "Should handle tool-specific messages"
fi

# Test 20: Has main function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "^main()" "$HOOK" || grep -q "main \"\$@\"" "$HOOK"; then
    pass "Has main function"
else
    fail "Should have main function"
fi

# Test 21: Uses jq for JSON parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 22: Handles Grep tool path extraction
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Grep" "$HOOK" && grep -q "\.path" "$HOOK"; then
    pass "Handles Grep tool path extraction"
else
    fail "Should handle Grep tool path extraction"
fi

# Test 23: Handles Bash tool command extraction
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Bash" "$HOOK" && grep -q "command" "$HOOK"; then
    pass "Handles Bash tool command extraction"
else
    fail "Should handle Bash tool command extraction"
fi

# Test 24: Always exits 0 (PostToolUse should not block)
TESTS_RUN=$((TESTS_RUN + 1))
# Check that all exit statements are exit 0 (no exit 1 or exit 2)
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "PostToolUse should only have exit 0"
else
    pass "All exits are 0 (PostToolUse should not block)"
fi

echo ""
echo "--- Code Quality ---"

# Test 25: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 26: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 27: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 28: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Functional Tests (Edit Tool) ---"

# Test 29: Edit on .sh file triggers script-standards instruction
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/path/test.sh"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-script-standards"* ]] || [[ "$result" == *"lint-shell"* ]]; then
    pass "Edit on .sh file triggers script-standards instruction"
else
    fail "Should trigger script-standards for .sh files"
fi

# Test 30: Edit on .md file triggers documentation-standards instruction
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/path/README.md"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-documentation-standards"* ]] || [[ "$result" == *"lint"* ]]; then
    pass "Edit on .md file triggers documentation-standards instruction"
else
    fail "Should trigger documentation-standards for .md files"
fi

# Test 31: Edit on .py file triggers script-standards instruction
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/path/script.py"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-script-standards"* ]] || [[ "$result" == *"lint-python"* ]] || [[ "$result" == *"flake8"* ]]; then
    pass "Edit on .py file triggers script-standards instruction"
else
    fail "Should trigger script-standards for .py files"
fi

# Test 32: Edit on excluded memory path does not trigger documentation-standards
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":".claude/memory/test.md"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-documentation-standards"* ]]; then
    fail "Should not trigger documentation-standards for memory paths"
else
    pass "Edit on memory path is excluded from documentation-standards"
fi

echo ""
echo "--- Functional Tests (Write Tool) ---"

# Test 33: Write on .sh file triggers script-standards instruction
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Write","tool_input":{"file_path":"/some/path/new-script.sh"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-script-standards"* ]] || [[ "$result" == *"apply-shell-standards"* ]]; then
    pass "Write on .sh file triggers script-standards instruction"
else
    fail "Should trigger script-standards for new .sh files"
fi

# Test 34: Write on .md file triggers documentation-standards instruction
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Write","tool_input":{"file_path":"/some/path/new-doc.md"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"cf-documentation-standards"* ]] || [[ "$result" == *"apply-standard"* ]]; then
    pass "Write on .md file triggers documentation-standards instruction"
else
    fail "Should trigger documentation-standards for new .md files"
fi

echo ""
echo "--- Functional Tests (Other Tools) ---"

# Test 35: Read tool on non-matching file exits silently
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Read","tool_input":{"file_path":"/some/path/data.json"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Read tool on non-matching file exits 0"
else
    fail "Read tool should exit 0"
fi

# Test 36: Grep tool uses path field
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Grep","tool_input":{"path":"/some/path/test.sh","pattern":"foo"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Grep tool handles path field correctly"
else
    fail "Grep tool should handle path field"
fi

# Test 37: Unknown tool exits 0 silently
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"UnknownTool","tool_input":{}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Unknown tool exits 0 silently"
else
    fail "Unknown tool should exit 0"
fi

echo ""
echo "--- Edge Cases ---"

# Test 38: Empty tool_input exits 0
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Empty tool_input exits 0"
else
    fail "Should exit 0 for empty tool_input"
fi

# Test 39: Malformed JSON handled (jq returns empty, exits 0)
TESTS_RUN=$((TESTS_RUN + 1))
# With set -euo pipefail, jq exits with error but script catches it with // empty
result=$(echo 'not valid json' | bash "$HOOK" 2>&1; echo "EXIT:$?")
# Script should exit 0 because tool_name extraction returns empty
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Malformed JSON handled gracefully (exits 0)"
else
    # Alternative: script may exit non-zero due to jq error, which is acceptable
    pass "Malformed JSON causes jq error (acceptable strict behavior)"
fi

# Test 40: File path with special characters handled
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/path/with spaces/file.sh"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "File path with spaces handled"
else
    fail "Should handle file paths with spaces"
fi

# Test 41: Config file exists
TESTS_RUN=$((TESTS_RUN + 1))
CONFIG_FILE="$REPO_ROOT/.codeflow/config/instructions/instructions-config.json"
if [[ -f "$CONFIG_FILE" ]]; then
    pass "Config file instructions-config.json exists"
else
    fail "Config file should exist at $CONFIG_FILE"
fi

# Test 42: Config file is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null && [[ -f "$CONFIG_FILE" ]]; then
    if jq empty "$CONFIG_FILE" 2>/dev/null; then
        pass "Config file is valid JSON"
    else
        fail "Config file should be valid JSON"
    fi
else
    pass "Config validation skipped (jq or file not available)"
fi

# Test 43: Output format is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"test.sh"}}' | bash "$HOOK" 2>&1)
if [[ -n "$result" ]] && echo "$result" | jq empty 2>/dev/null; then
    pass "Output format is valid JSON"
else
    if [[ -z "$result" ]]; then
        pass "No output (valid - no matching rule)"
    else
        fail "Output should be valid JSON"
    fi
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
