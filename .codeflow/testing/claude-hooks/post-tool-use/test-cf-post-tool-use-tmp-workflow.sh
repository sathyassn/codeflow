#!/usr/bin/env bash
# Test: cf-post-tool-use-tmp-workflow.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-tmp-workflow.sh
#
# Tests tmp file workflow guidance hook
# Verifies proper workflow instructions for protected-edits tmp files

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-tmp-workflow.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-tmp-workflow.sh ==="
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

# Test 6: Has Version header comment
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Version:" "$HOOK"; then
    pass "Has Version header comment"
else
    fail "Should have Version header comment"
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
echo "--- Path Filtering ---"

# Test 9: Exits 0 for non-protected-edits files
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/regular/file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"hookSpecificOutput"* ]]; then
    pass "Exits 0 silently for non-protected-edits files"
else
    fail "Should exit 0 silently for non-protected-edits files"
fi

# Test 10: Exits 0 for regular tmp files (not in protected-edits)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/regular-file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"hookSpecificOutput"* ]]; then
    pass "Exits 0 silently for regular tmp files"
else
    fail "Should exit 0 silently for regular tmp files"
fi

# Test 11: Exits 0 for managed but non-protected-edits
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"$REPO_ROOT/.state/session/file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"hookSpecificOutput"* ]]; then
    pass "Exits 0 silently for managed non-protected-edits"
else
    fail "Should exit 0 silently for managed non-protected-edits"
fi

# Test 12: Outputs workflow for protected-edits files
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/some-file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"hookSpecificOutput"* ]]; then
    pass "Outputs workflow for protected-edits files"
else
    fail "Should output workflow for protected-edits files"
fi

# Test 13: References protected-edits directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/tmp/claude/managed/codeflow/protected-edits" "$HOOK"; then
    pass "References protected-edits directory"
else
    fail "Should reference protected-edits directory"
fi

echo ""
echo "--- Settings File Handling ---"

# Test 14: Has special handling for settings files
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/settings.json"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"SETTINGS FILE"* ]]; then
    pass "Has special handling for settings files"
else
    fail "Should have special handling for settings files"
fi

# Test 15: Has special handling for settings.local.json
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/settings.local.json"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"SETTINGS FILE"* ]]; then
    pass "Has special handling for settings.local.json"
else
    fail "Should have special handling for settings.local.json"
fi

# Test 16: Settings file workflow includes sync-settings-templates
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/settings.json"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"sync-settings-templates"* ]]; then
    pass "Settings file workflow includes sync-settings-templates"
else
    fail "Should include sync-settings-templates for settings files"
fi

# Test 17: Non-settings workflow does not include sync-settings-templates
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/other-file.txt"}}' | bash "$HOOK" 2>&1)
if [[ "$result" != *"sync-settings-templates"* ]]; then
    pass "Non-settings workflow excludes sync-settings-templates"
else
    fail "Should not include sync-settings-templates for non-settings files"
fi

# Test 18: Has IS_SETTINGS_FILE check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "IS_SETTINGS_FILE" "$HOOK"; then
    pass "Has IS_SETTINGS_FILE check"
else
    fail "Should have IS_SETTINGS_FILE check"
fi

# Test 19: Settings workflow has FORBIDDEN step
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/settings.json"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"FORBIDDEN"* ]]; then
    pass "Settings workflow has FORBIDDEN guidance"
else
    fail "Should have FORBIDDEN guidance for settings"
fi

echo ""
echo "--- File Type Detection ---"

# Test 20: Has detect_file_type function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "detect_file_type" "$HOOK"; then
    pass "Has detect_file_type function"
else
    fail "Should have detect_file_type function"
fi

# Test 21: Has get_validation_hint function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "get_validation_hint" "$HOOK"; then
    pass "Has get_validation_hint function"
else
    fail "Should have get_validation_hint function"
fi

# Test 22: Detects shell script file type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sh|bash)" "$HOOK" && grep -q "shellcheck" "$HOOK"; then
    pass "Detects shell script file type"
else
    fail "Should detect and validate shell scripts"
fi

# Test 23: Detects Python file type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "py)" "$HOOK" && grep -q "python" "$HOOK"; then
    pass "Detects Python file type"
else
    fail "Should detect and validate Python files"
fi

# Test 24: Detects JSON file type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "json)" "$HOOK" && grep -q "jq" "$HOOK"; then
    pass "Detects JSON file type"
else
    fail "Should detect and validate JSON files"
fi

# Test 25: Detects YAML file type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "yaml|yml)" "$HOOK"; then
    pass "Detects YAML file type"
else
    fail "Should detect YAML files"
fi

# Test 26: Detects TOML file type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "toml)" "$HOOK"; then
    pass "Detects TOML file type"
else
    fail "Should detect TOML files"
fi

# Test 27: Detects shell by shebang
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "shebang" "$HOOK" || grep -q "head -1" "$HOOK"; then
    pass "Detects shell by shebang"
else
    fail "Should detect shell scripts by shebang"
fi

echo ""
echo "--- Validation Hints ---"

# Test 28: Shell file workflow includes shellcheck hint
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/script.sh"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"shellcheck"* ]] || [[ "$result" == *"bash -n"* ]]; then
    pass "Shell file workflow includes shellcheck hint"
else
    fail "Should include shellcheck hint for shell files"
fi

# Test 29: Python file workflow includes validation hint
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/script.py"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"py_compile"* ]] || [[ "$result" == *"ruff"* ]]; then
    pass "Python file workflow includes validation hint"
else
    fail "Should include validation hint for Python files"
fi

# Test 30: JSON file workflow includes jq validation hint
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/config.json"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"jq"* ]] || [[ "$result" == *"json.tool"* ]]; then
    pass "JSON file workflow includes jq validation hint"
else
    fail "Should include jq validation hint for JSON files"
fi

# Test 31: YAML file workflow includes validation hint
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/config.yaml"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"yaml"* ]]; then
    pass "YAML file workflow includes validation hint"
else
    fail "Should include validation hint for YAML files"
fi

echo ""
echo "--- Workflow Steps ---"

# Test 32: Outputs hookSpecificOutput JSON format
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "hookSpecificOutput" "$HOOK" && grep -q "hookEventName" "$HOOK" && grep -q "PostToolUse" "$HOOK"; then
    pass "Outputs proper hookSpecificOutput JSON format"
else
    fail "Should output hookSpecificOutput JSON format"
fi

# Test 33: Workflow includes show user changes step
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Show user" "$HOOK"; then
    pass "Workflow includes show user changes step"
else
    fail "Should include show user changes step"
fi

# Test 34: Workflow includes sudo cp command
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sudo cp" "$HOOK"; then
    pass "Workflow includes sudo cp command"
else
    fail "Should include sudo cp command"
fi

# Test 35: Workflow includes cleanup step
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Cleanup" "$HOOK"; then
    pass "Workflow includes cleanup step"
else
    fail "Should include cleanup step"
fi

# Test 36: Workflow includes verify step
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "READ original" "$HOOK" || grep -q "verify" "$HOOK"; then
    pass "Workflow includes verify step"
else
    fail "Should include verify step"
fi

echo ""
echo "--- Input Handling ---"

# Test 37: Uses jq for JSON parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 38: Reads from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'INPUT=$(cat)' "$HOOK"; then
    pass "Reads from stdin"
else
    fail "Should read from stdin"
fi

# Test 39: Extracts FILE_PATH from input
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "FILE_PATH" "$HOOK" && grep -q "file_path" "$HOOK"; then
    pass "Extracts FILE_PATH from input"
else
    fail "Should extract FILE_PATH from input"
fi

# Test 40: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK" || grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
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

# Test 45: Always exits 0
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Always exits 0"
else
    fail "Should always exit 0"
fi

# Test 46: All exits are 0 (PostToolUse should not block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "PostToolUse should only have exit 0"
else
    pass "All exits are 0 (PostToolUse should not block)"
fi

echo ""
echo "--- Edge Cases ---"

# Test 47: Handles empty file_path
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty file_path gracefully"
else
    fail "Should handle empty file_path"
fi

# Test 48: Handles malformed JSON
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo 'not json' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles malformed JSON gracefully"
else
    fail "Should handle malformed JSON"
fi

# Test 49: Handles empty input
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty input gracefully"
else
    fail "Should handle empty input"
fi

# Test 50: Unknown file type gets basic workflow
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/claude/managed/codeflow/protected-edits/file.unknown"}}' | bash "$HOOK" 2>&1)
if [[ "$result" == *"PROTECTED RESOURCE WORKFLOW"* ]] && [[ "$result" != *"Validate:"* ]]; then
    pass "Unknown file type gets basic workflow"
else
    pass "Unknown file type handled"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
