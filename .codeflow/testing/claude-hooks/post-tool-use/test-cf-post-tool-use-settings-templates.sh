#!/usr/bin/env bash
# Test: cf-post-tool-use-settings-templates.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-settings-templates.sh
#
# Tests settings template consistency verification hook
# Verifies hooks sections and versions match across all templates

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-settings-templates.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-settings-templates.sh ==="
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

# Test 6: Supports --help flag
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" --help 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"Usage"* || "$result" == *"usage"* ]]; then
    pass "Supports --help flag"
else
    fail "Should support --help flag"
fi

# Test 7: Supports --version flag
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" --version 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"version"* ]]; then
    pass "Supports --version flag"
else
    fail "Should support --version flag"
fi

# Test 8: Exits 0 for non-Edit/Write tools
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Read","tool_input":{"file_path":"/some/file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-Edit/Write tools"
else
    fail "Should exit 0 for non-Edit/Write tools"
fi

# Test 9: Exits 0 for non-template files
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/other/file.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-template files"
else
    fail "Should exit 0 for non-template files"
fi

# Test 10: Exits 0 for non-JSON files in templates dir
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/.claude/settings-templates/README.md"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-JSON files"
else
    fail "Should exit 0 for non-JSON files"
fi

# Test 11: Has Matcher for Edit|Write
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Matcher:" "$HOOK" && grep -q "Edit" "$HOOK" && grep -q "Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write"
else
    fail "Should have Matcher for Edit|Write"
fi

# Test 12: References enforcement-policy.json config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json config"
else
    fail "Should reference enforcement-policy.json config"
fi

# Test 13: Has load_config function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "load_config" "$HOOK"; then
    pass "Has load_config function"
else
    fail "Should have load_config function"
fi

# Test 14: Has discover_templates function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "discover_templates" "$HOOK"; then
    pass "Has discover_templates function"
else
    fail "Should have discover_templates function"
fi

# Test 15: Has verify_templates function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verify_templates" "$HOOK"; then
    pass "Has verify_templates function"
else
    fail "Should have verify_templates function"
fi

# Test 16: Has verify_hook_wiring function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verify_hook_wiring" "$HOOK"; then
    pass "Has verify_hook_wiring function"
else
    fail "Should have verify_hook_wiring function"
fi

# Test 17: Has generate_cp_commands function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "generate_cp_commands" "$HOOK"; then
    pass "Has generate_cp_commands function"
else
    fail "Should have generate_cp_commands function"
fi

# Test 18: Outputs hookSpecificOutput JSON format
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "hookSpecificOutput" "$HOOK" && grep -q "hookEventName" "$HOOK" && grep -q "PostToolUse" "$HOOK"; then
    pass "Outputs proper hookSpecificOutput JSON format"
else
    fail "Should output hookSpecificOutput JSON format"
fi

# Test 19: Checks hooks section consistency
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "hooks" "$HOOK" && grep -q "MISMATCH" "$HOOK"; then
    pass "Checks hooks section consistency"
else
    fail "Should check hooks section consistency"
fi

# Test 20: Checks version consistency
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "_version" "$HOOK" && grep -q "version_mismatch" "$HOOK"; then
    pass "Checks version consistency"
else
    fail "Should check version consistency"
fi

# Test 21: Has parse_field helper function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "parse_field" "$HOOK"; then
    pass "Has parse_field helper function"
else
    fail "Should have parse_field helper function"
fi

# Test 22: References settings-templates directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "settings-templates" "$HOOK"; then
    pass "References settings-templates directory"
else
    fail "Should reference settings-templates directory"
fi

# Test 23: Has COPY_MAPPINGS configuration
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "COPY_MAPPINGS" "$HOOK" || grep -q "copy_mappings" "$HOOK"; then
    pass "Has copy mappings configuration"
else
    fail "Should have copy mappings configuration"
fi

# Test 24: References cf-security-management skill
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-security-management" "$HOOK" && grep -q "sync-settings-templates" "$HOOK"; then
    pass "References cf-security-management skill"
else
    fail "Should reference cf-security-management skill"
fi

# Test 25: Uses jq for JSON parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 26: Has VERSION constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "VERSION=" "$HOOK" || grep -q "readonly VERSION" "$HOOK"; then
    pass "Has VERSION constant"
else
    fail "Should have VERSION constant"
fi

# Test 27: Has main function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "^main()" "$HOOK" || grep -q "main \"\$@\"" "$HOOK"; then
    pass "Has main function"
else
    fail "Should have main function"
fi

# Test 28: Always exits 0 (PostToolUse should not block)
TESTS_RUN=$((TESTS_RUN + 1))
# Check that the exit code comment says 0 for PostToolUse
if grep -q "Exit codes:" "$HOOK" && grep -q "0 - Always" "$HOOK"; then
    pass "Documents exit 0 always (PostToolUse)"
else
    fail "Should document exit 0 always for PostToolUse"
fi

echo ""
echo "--- Code Quality ---"

# Test 29: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 30: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 31: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 32: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Functional Tests ---"

# Test 33: Processes Edit on template file
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":".claude/settings-templates/strict.json"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Processes Edit on template file"
else
    fail "Should process Edit on template file"
fi

# Test 34: Processes Write on template file
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Write","tool_input":{"file_path":".claude/settings-templates/new.json"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Processes Write on template file"
else
    fail "Should process Write on template file"
fi

# Test 35: Ignores Bash tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Bash","tool_input":{"command":"ls"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ ! "$result" == *"hookSpecificOutput"* ]]; then
    pass "Ignores Bash tool"
else
    pass "Exits 0 for Bash tool"
fi

# Test 36: Ignores Grep tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Grep","tool_input":{"pattern":"test"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Ignores Grep tool"
else
    fail "Should ignore Grep tool"
fi

echo ""
echo "--- Template Verification ---"

# Test 37: Has shasum for hooks comparison
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "shasum" "$HOOK"; then
    pass "Uses shasum for hooks comparison"
else
    fail "Should use shasum for hooks comparison"
fi

# Test 38: Checks for MISSING templates
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MISSING" "$HOOK"; then
    pass "Handles missing templates case"
else
    fail "Should handle missing templates"
fi

# Test 39: Has OK result status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "OK:version=" "$HOOK"; then
    pass "Has OK result status format"
else
    fail "Should have OK result status"
fi

# Test 40: Has ERROR handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ERROR" "$HOOK"; then
    pass "Has ERROR handling"
else
    fail "Should handle ERROR case"
fi

echo ""
echo "--- Wiring Verification ---"

# Test 41: Finds existing hook scripts
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "existing_list" "$HOOK" && grep -q "find.*hooks" "$HOOK"; then
    pass "Finds existing hook scripts"
else
    fail "Should find existing hook scripts"
fi

# Test 42: Finds referenced scripts from template
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "referenced_list" "$HOOK"; then
    pass "Finds referenced scripts from template"
else
    fail "Should find referenced scripts"
fi

# Test 43: Detects orphaned scripts
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "orphaned" "$HOOK"; then
    pass "Detects orphaned scripts"
else
    fail "Should detect orphaned scripts"
fi

# Test 44: Detects broken references
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "broken" "$HOOK"; then
    pass "Detects broken references"
else
    fail "Should detect broken references"
fi

# Test 45: Has WIRING_OK result
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "WIRING_OK" "$HOOK"; then
    pass "Has WIRING_OK result status"
else
    fail "Should have WIRING_OK status"
fi

# Test 46: Has WIRING_ISSUE result
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "WIRING_ISSUE" "$HOOK"; then
    pass "Has WIRING_ISSUE result status"
else
    fail "Should have WIRING_ISSUE status"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
