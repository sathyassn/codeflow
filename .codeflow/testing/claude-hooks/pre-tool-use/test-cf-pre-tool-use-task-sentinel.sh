#!/usr/bin/env bash
# Test: cf-pre-tool-use-task-sentinel.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-task-sentinel.sh
#
# Comprehensive tests for task registration enforcement:
#   - Basic setup and shellcheck
#   - Tool filtering (Edit/Write only)
#   - Exempt paths (memory, state, tmp)
#   - Active task checking
#   - Scope validation (soft/strict)
#   - Block messages and guidance
#   - Security logging
#   - Config-driven features
#   - Code quality

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task-sentinel.sh"
ACTIVE_TASK_FILE="/tmp/claude/managed/state/active-task.json"

export REPO_ROOT

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

# Setup/teardown for active task tests
setup_active_task() {
    mkdir -p "$(dirname "$ACTIVE_TASK_FILE")"
    cat > "$ACTIVE_TASK_FILE" << 'EOF'
{
  "task_id": "test-task-001",
  "file_scope": ["src/**", "tests/**"],
  "scope_policy": "soft"
}
EOF
}

setup_strict_task() {
    mkdir -p "$(dirname "$ACTIVE_TASK_FILE")"
    cat > "$ACTIVE_TASK_FILE" << 'EOF'
{
  "task_id": "test-task-002",
  "file_scope": ["src/**"],
  "scope_policy": "strict"
}
EOF
}

setup_no_scope_task() {
    mkdir -p "$(dirname "$ACTIVE_TASK_FILE")"
    cat > "$ACTIVE_TASK_FILE" << 'EOF'
{
  "task_id": "test-task-003"
}
EOF
}

teardown_active_task() {
    rm -f "$ACTIVE_TASK_FILE" 2>/dev/null || true
}

echo "=== Testing cf-pre-tool-use-task-sentinel.sh ==="
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
if grep -q "Matcher:" "$HOOK" && grep -q "Edit" "$HOOK" && grep -q "Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write in header"
else
    fail "Should have Matcher for Edit|Write"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Edit/Write tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 8: Exits 0 for non-Edit/Write tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 9: Exits 0 for non-Edit/Write tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 10: Exits 0 for non-Edit/Write tools (Glob)
result=$(TOOL_NAME="Glob" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Glob tool"
else
    fail "Should exit 0 for Glob tool"
fi

# Test 11: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Edit" TOOL_INPUT="" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 12: Exits 0 when empty file_path
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty file_path"
else
    fail "Should exit 0 when empty file_path"
fi

echo ""
echo "--- Exempt Paths ---"

# Test 13: Exits 0 for memory paths
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/memory/test.md"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for memory paths"
else
    fail "Should exit 0 for memory paths"
fi

# Test 14: Exits 0 for tmp paths (absolute)
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"/tmp/claude/test.txt"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for /tmp/claude/ paths"
else
    fail "Should exit 0 for /tmp/claude/ paths"
fi

# Test 15: Exits 0 for /tmp paths
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"/tmp/test.txt"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for /tmp paths"
else
    fail "Should exit 0 for /tmp paths"
fi

# Test 16: Exits 0 for .state paths
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".state/logs/test.log"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for .state paths"
else
    fail "Should exit 0 for .state paths"
fi

# Test 17: Exits 0 for .codeflow/state paths
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/state/test.json"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for .codeflow/state paths"
else
    fail "Should exit 0 for .codeflow/state paths"
fi

echo ""
echo "--- Active Task Checking (No Task) ---"

# Clean up any existing task file
teardown_active_task

# Test 18: Blocks Edit when no active task
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/main.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit when no active task"
else
    fail "Should block Edit when no active task"
fi

# Test 19: Blocks Write when no active task
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"src/new.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Write when no active task"
else
    fail "Should block Write when no active task"
fi

# Test 20: Block message includes skill direction
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/main.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"cf-task-management"* ]]; then
    pass "Block message includes skill direction"
else
    fail "Should include cf-task-management in block message"
fi

# Test 21: Block message includes MUST directive
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/main.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"MUST:"* ]]; then
    pass "Block message includes MUST directive"
else
    fail "Should include MUST directive"
fi

# Test 22: Block message shows file path
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/main.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"Path:"* ]]; then
    pass "Block message shows file path"
else
    fail "Should show file path in block message"
fi

echo ""
echo "--- Active Task Checking (With Task) ---"

# Setup task with scope
setup_active_task

# Test 23: Allows Edit when task exists and file in scope
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/main.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit when task exists and file in scope"
else
    fail "Should allow Edit when task exists and in scope"
fi

# Test 24: Allows Write when task exists and file in scope
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"tests/test.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Write when task exists and file in scope"
else
    fail "Should allow Write when task exists and in scope"
fi

# Test 25: Allows edit for nested path in scope
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/components/Button.tsx"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows edit for nested path in scope"
else
    fail "Should allow edit for nested path"
fi

teardown_active_task

echo ""
echo "--- Scope Validation (Soft Policy) ---"

setup_active_task  # soft policy with src/** and tests/**

# Test 26: Warns but allows out-of-scope file (soft policy)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"docs/README.md"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"WARNING"* ]]; then
    pass "Warns but allows out-of-scope file (soft)"
else
    fail "Should warn but allow with soft policy"
fi

# Test 27: Warning includes scope info
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"docs/README.md"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"Scope:"* ]]; then
    pass "Warning includes scope info"
else
    fail "Should include scope in warning"
fi

# Test 28: Warning suggests expand-scope
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"docs/README.md"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"expand-scope"* ]]; then
    pass "Warning suggests expand-scope"
else
    fail "Should suggest expand-scope"
fi

teardown_active_task

echo ""
echo "--- Scope Validation (Strict Policy) ---"

setup_strict_task  # strict policy with src/** only

# Test 29: Blocks out-of-scope file (strict policy)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"tests/test.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks out-of-scope file (strict)"
else
    fail "Should block with strict policy"
fi

# Test 30: Allows in-scope file (strict policy)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/index.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows in-scope file (strict)"
else
    fail "Should allow in-scope with strict policy"
fi

# Test 31: Block message mentions strict policy
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"tests/test.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"strict"* ]]; then
    pass "Block message mentions strict policy"
else
    fail "Should mention strict policy"
fi

teardown_active_task

echo ""
echo "--- No Scope Defined ---"

setup_no_scope_task  # task with no file_scope

# Test 32: Allows any file when no scope defined
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"anywhere/file.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows any file when no scope defined"
else
    fail "Should allow any file without scope"
fi

# Test 33: No warning when no scope defined
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"random/path.ts"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" != *"WARNING"* ]]; then
    pass "No warning when no scope defined"
else
    fail "Should not warn without scope"
fi

teardown_active_task

echo ""
echo "--- Config-Driven Features ---"

# Test 34: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 35: References active-task.json
if grep -q "active-task.json" "$HOOK"; then
    pass "References active-task.json"
else
    fail "Should reference active-task.json"
fi

# Test 36: Has config-driven sentinel enabled check
if grep -q "L1_SENTINEL" "$HOOK" && grep -q "enabled" "$HOOK"; then
    pass "Has config-driven sentinel enabled check"
else
    fail "Should check L1_SENTINEL.enabled"
fi

# Test 37: Has is_exempt_path function
if grep -q "is_exempt_path()" "$HOOK"; then
    pass "Has is_exempt_path function"
else
    fail "Should have is_exempt_path function"
fi

# Test 38: Has matches_scope function
if grep -q "matches_scope()" "$HOOK"; then
    pass "Has matches_scope function"
else
    fail "Should have matches_scope function"
fi

echo ""
echo "--- Security Logging ---"

# Test 39: Sources security library
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security library"
else
    fail "Should source security library"
fi

# Test 40: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

# Test 41: Logs warning operations
if grep -q 'log_security_event.*warn' "$HOOK"; then
    pass "Logs warning operations"
else
    fail "Should log warning operations"
fi

echo ""
echo "--- Code Quality ---"

# Test 42: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 43: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 44: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 45: Documents bash 3.2+ compatibility
if grep -q "bash 3.2" "$HOOK" || grep -q "Compatibility" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash compatibility"
fi

# Test 46: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK"; then
    pass "Has exit code 2 for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

# Test 47: Uses jq for JSON parsing
if grep -q "command -v jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 48: Has jq fallback for input parsing
if grep -q "grep -o" "$HOOK"; then
    pass "Has jq fallback for input parsing"
else
    fail "Should have jq fallback"
fi

# Test 49: Has TOOL_NAME early exit check
if grep -q 'TOOL_NAME.*Edit.*Write' "$HOOK"; then
    pass "Has TOOL_NAME early exit check"
else
    fail "Should have TOOL_NAME early exit"
fi

# Test 50: Has TOOL_INPUT empty check
if grep -q -- '-z.*TOOL_INPUT' "$HOOK" || grep -q 'TOOL_INPUT.*\-z' "$HOOK"; then
    pass "Has TOOL_INPUT empty check"
else
    fail "Should check for empty TOOL_INPUT"
fi

# Test 51: Normalizes paths to relative
if grep -q 'FILE_PATH=.*REPO_ROOT' "$HOOK"; then
    pass "Normalizes paths to relative"
else
    fail "Should normalize paths"
fi

# Test 52: Has glob pattern conversion for scope matching
if grep -q 'regex=' "$HOOK" && grep -q '\.\*' "$HOOK"; then
    pass "Has glob pattern conversion for scope"
else
    fail "Should convert glob patterns for scope"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

# Cleanup
teardown_active_task

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
