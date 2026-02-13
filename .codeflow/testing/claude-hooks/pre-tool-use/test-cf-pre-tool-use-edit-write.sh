#!/usr/bin/env bash
# Test: cf-pre-tool-use-edit-write.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-edit-write.sh
#
# Comprehensive tests for Edit/Write pre-tool-use hook:
#   - Tool filtering (Edit|Write only)
#   - Branch protection
#   - Blocked directories (config-driven)
#   - Path validation
#   - Dangerous extension warnings
#   - Allowed tmp prefixes
#   - Config loading

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-edit-write.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

# Helper: Run hook with tool name and file path
run_edit_write() {
    local tool_name="$1"
    local file_path="$2"

    local json_input="{\"file_path\": \"$file_path\"}"

    local output exit_code
    output=$(TOOL_NAME="$tool_name" TOOL_INPUT="$json_input" bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?

    HOOK_OUTPUT="$output"
    HOOK_EXIT_CODE=$exit_code
}

echo "=== Testing cf-pre-tool-use-edit-write.sh ==="
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

# Test 6: Has Matcher for Edit|Write in header
if grep -q "Matcher:" "$HOOK" && grep -q "Edit|Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write in header"
else
    fail "Should have Matcher for Edit|Write"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Edit/Write tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 8: Exits 0 for non-Edit/Write tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 9: Exits 0 for non-Edit/Write tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 10: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Edit" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 11: Exits 0 when empty file_path
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty file_path"
else
    fail "Should exit 0 when empty file_path"
fi

echo ""
echo "--- Allowed Operations ---"

# Test 12: Allows editing files in /tmp/claude
run_edit_write "Edit" "/tmp/claude/test.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing files in /tmp/claude"
else
    fail "Should allow editing files in /tmp/claude"
fi

# Test 13: Allows Write to /tmp/claude
run_edit_write "Write" "/tmp/claude/test.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows Write to /tmp/claude"
else
    fail "Should allow Write to /tmp/claude"
fi

# Test 14: Allows editing files in /tmp
run_edit_write "Edit" "/tmp/test.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing files in /tmp"
else
    fail "Should allow editing files in /tmp"
fi

# Test 15: Allows editing regular project files
run_edit_write "Edit" "src/component.ts"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing regular project files"
else
    fail "Should allow editing regular project files"
fi

# Test 16: Allows writing to nested project directories
run_edit_write "Write" "src/components/Button.tsx"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows writing to nested project directories"
else
    fail "Should allow writing to nested project directories"
fi

echo ""
echo "--- Blocked Directories ---"

# Test 17: Blocks writes to .git directory
run_edit_write "Edit" ".git/config"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to .git directory"
else
    fail "Should block writes to .git directory (exit=$HOOK_EXIT_CODE)"
fi

# Test 18: Blocks writes to nested .git directory
run_edit_write "Write" "submodule/.git/config"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to nested .git directory"
else
    fail "Should block writes to nested .git directory"
fi

# Test 19: Blocks writes to node_modules
run_edit_write "Edit" "node_modules/package/index.js"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to node_modules"
else
    fail "Should block writes to node_modules"
fi

# Test 20: Blocks writes to nested node_modules
run_edit_write "Write" "packages/app/node_modules/lib.js"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to nested node_modules"
else
    fail "Should block writes to nested node_modules"
fi

# Test 21: Blocks writes to __pycache__
run_edit_write "Edit" "__pycache__/module.pyc"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to __pycache__"
else
    fail "Should block writes to __pycache__"
fi

# Test 22: Blocks writes to .venv
run_edit_write "Edit" ".venv/lib/python3.11/site-packages/pkg.py"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to .venv"
else
    fail "Should block writes to .venv"
fi

# Test 23: Blocks writes to venv
run_edit_write "Edit" "venv/bin/activate"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to venv"
else
    fail "Should block writes to venv"
fi

echo ""
echo "--- Path Validation ---"

# Test 24: Blocks writes outside repository
run_edit_write "Edit" "/etc/passwd"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes outside repository"
else
    fail "Should block writes outside repository"
fi

# Test 25: Blocks writes to home directory
run_edit_write "Write" "/Users/someone/.bashrc"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to home directory"
else
    fail "Should block writes to home directory"
fi

# Test 26: Blocks writes to system paths
run_edit_write "Edit" "/usr/local/bin/script.sh"
if [[ $HOOK_EXIT_CODE -eq 2 ]] && [[ "$HOOK_OUTPUT" == *"BLOCKED"* ]]; then
    pass "Blocks writes to system paths"
else
    fail "Should block writes to system paths"
fi

echo ""
echo "--- Dangerous Extension Warnings ---"

# Test 27: Warns on binary file extension (exe)
run_edit_write "Write" "program.exe"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"binary"* ]]; then
    pass "Warns on binary file extension (exe)"
else
    fail "Should warn on binary file extension"
fi

# Test 28: Warns on binary file extension (dll)
run_edit_write "Write" "library.dll"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"binary"* ]]; then
    pass "Warns on binary file extension (dll)"
else
    fail "Should warn on dll extension"
fi

# Test 29: Warns on binary file extension (so)
run_edit_write "Write" "libfoo.so"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"binary"* ]]; then
    pass "Warns on binary file extension (so)"
else
    fail "Should warn on so extension"
fi

# Test 30: Warns on credential file extension (pem)
run_edit_write "Write" "server.pem"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"credential"* ]]; then
    pass "Warns on credential file extension (pem)"
else
    fail "Should warn on pem extension"
fi

# Test 31: Warns on credential file extension (key)
run_edit_write "Write" "private.key"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"credential"* ]]; then
    pass "Warns on credential file extension (key)"
else
    fail "Should warn on key extension"
fi

# Test 32: Allows and warns (exit 0) for dangerous extensions
run_edit_write "Write" "test.exe"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows dangerous extensions with warning (exit 0)"
else
    fail "Should allow dangerous extensions (just warn)"
fi

# Test 33: No warning for safe extensions
run_edit_write "Write" "component.tsx"
if [[ "$HOOK_OUTPUT" != *"Warning"* ]]; then
    pass "No warning for safe extensions"
else
    fail "Should not warn for safe extensions"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 34: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 35: Reads blocked_directories from config
if grep -q "edit_write.blocked_directories" "$HOOK"; then
    pass "Reads blocked_directories from config"
else
    fail "Should read blocked_directories from config"
fi

# Test 36: Reads allowed_tmp_prefixes from config
if grep -q "edit_write.allowed_tmp_prefixes" "$HOOK"; then
    pass "Reads allowed_tmp_prefixes from config"
else
    fail "Should read allowed_tmp_prefixes from config"
fi

# Test 37: Reads dangerous_extensions from config
if grep -q "edit_write.dangerous_extensions" "$HOOK"; then
    pass "Reads dangerous_extensions from config"
else
    fail "Should read dangerous_extensions from config"
fi

# Test 38: Has default fallbacks for config values
if grep -q "DEFAULT_BLOCKED_DIRS" "$HOOK" && grep -q "DEFAULT_BINARY_EXT" "$HOOK"; then
    pass "Has default fallbacks for config values"
else
    fail "Should have default fallbacks"
fi

# Test 39: Has jq fallback for file_path extraction
if grep -q "grep -o" "$HOOK" && grep -q "file_path" "$HOOK"; then
    pass "Has jq fallback for file_path extraction"
else
    fail "Should have jq fallback"
fi

echo ""
echo "--- Branch Protection ---"

# Test 40: Reads protected_branches from config
if grep -q "protected_branches" "$HOOK"; then
    pass "Reads protected_branches from config"
else
    fail "Should read protected_branches from config"
fi

# Test 41: Has is_protected_branch function
if grep -q "is_protected_branch" "$HOOK"; then
    pass "Has is_protected_branch function"
else
    fail "Should have is_protected_branch function"
fi

# Test 42: Supports glob patterns for branches
if grep -q 'release/\*' "$HOOK" || grep -q '\*.*pattern' "$HOOK"; then
    pass "Supports glob patterns for branches"
else
    fail "Should support glob patterns for protected branches"
fi

echo ""
echo "--- Security Logging ---"

# Test 43: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should have security event logging"
fi

# Test 44: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

# Test 45: Logs warnings for dangerous files
if grep -q 'log_security_event.*warn' "$HOOK"; then
    pass "Logs warnings for dangerous files"
else
    fail "Should log warnings"
fi

# Test 46: Logs allowed operations (audit)
if grep -q 'log_security_event.*audit' "$HOOK"; then
    pass "Logs allowed operations (audit)"
else
    fail "Should log allowed operations"
fi

echo ""
echo "--- Code Quality ---"

# Test 47: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 48: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 49: Has bash 3.2+ compatibility note
if grep -q "bash 3.2" "$HOOK" || grep -q "macOS compatible" "$HOOK"; then
    pass "Has bash 3.2+ compatibility note"
else
    fail "Should note bash 3.2+ compatibility"
fi

# Test 50: Handles uppercase extensions
run_edit_write "Write" "program.EXE"
if [[ "$HOOK_OUTPUT" == *"Warning"* ]] && [[ "$HOOK_OUTPUT" == *"binary"* ]]; then
    pass "Handles uppercase extensions"
else
    fail "Should handle uppercase extensions"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
