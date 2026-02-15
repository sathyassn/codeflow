#!/usr/bin/env bash
# Test: cf-pre-tool-use-team-guard.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-team-guard.sh
#
# Tests team guard hook:
#   - File exists, executable, shellcheck, headers, strict mode, VERSION
#   - Exits 0 for non-Teammate tool
#   - Exits 0 for Teammate with operation=spawnTeam
#   - Exits 0 for cleanup when pathflow-active missing
#   - Exits 2 for cleanup when pathflow-active exists
#   - Exits 0 for TeamDelete when pathflow-active missing
#   - Exits 2 for TeamDelete when pathflow-active exists

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-pre-tool-use-team-guard.sh ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

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

# Test 8: Has Location header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:" "$HOOK"; then
    pass "Has Location header"
else
    fail "Should have Location header"
fi

# Test 9: Has Matcher header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Matcher:" "$HOOK"; then
    pass "Has Matcher header"
else
    fail "Should have Matcher header"
fi

# Test 10: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Code Quality ---"

# Test 11: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 12: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 13: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 14: Sources security-lib.sh
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security-lib.sh"
else
    fail "Should source security-lib.sh"
fi

# Test 15: Uses log_security_event for blocking
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "log_security_event" "$HOOK"; then
    pass "Uses log_security_event for blocking"
else
    fail "Should use log_security_event"
fi

# Test 16: Uses heredoc for block message
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cat.*>&2.*<<" "$HOOK"; then
    pass "Uses heredoc for block message to stderr"
else
    fail "Should use heredoc for block message to stderr"
fi

echo ""
echo "--- Execution Tests: Non-Teammate Tools ---"

# Test 17: Exits 0 for non-Teammate tool (Edit)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 18: Exits 0 for non-Teammate tool (Bash)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 19: Exits 0 for non-Teammate tool (Read)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 20: Exits 0 when no TOOL_NAME
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_NAME"
else
    fail "Should exit 0 when no TOOL_NAME"
fi

echo ""
echo "--- Execution Tests: Teammate Non-Cleanup Operations ---"

# Test 21: Exits 0 for Teammate with operation=spawnTeam
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Teammate" TOOL_INPUT='{"operation":"spawnTeam","team_name":"test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Teammate spawnTeam"
else
    fail "Should exit 0 for Teammate spawnTeam"
fi

# Test 22: Exits 0 for Teammate with empty input
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Teammate" TOOL_INPUT='' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Teammate with empty input"
else
    fail "Should exit 0 for Teammate with empty input"
fi

# Test 23: Exits 0 for Teammate with no operation field
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Teammate" TOOL_INPUT='{"team_name":"test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Teammate with no operation field"
else
    fail "Should exit 0 for Teammate with no operation field"
fi

echo ""
echo "--- Execution Tests: Cleanup Without PathFlow ---"

# Test 24: Exits 0 for cleanup when pathflow-active missing
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/nonexistent-flag" TOOL_NAME="Teammate" TOOL_INPUT='{"operation":"cleanup"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for cleanup when pathflow-active missing"
else
    fail "Should exit 0 for cleanup when pathflow-active missing"
fi

echo ""
echo "--- Execution Tests: Cleanup With PathFlow (BLOCKED) ---"

# Test 25: Exits 2 for cleanup when pathflow-active exists
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="Teammate" TOOL_INPUT='{"operation":"cleanup"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Exits 2 for cleanup when pathflow-active exists"
else
    fail "Should exit 2 for cleanup when pathflow-active exists (got exit=$exit_code)"
fi

# Test 26: Block message mentions PathFlow
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="Teammate" TOOL_INPUT='{"operation":"cleanup"}' bash "$HOOK" </dev/null 2>&1) || true
rm -rf "$TEMP_DIR"
if [[ "$output" == *"PathFlow"* ]]; then
    pass "Block message mentions PathFlow"
else
    fail "Block message should mention PathFlow"
fi

# Test 27: Block message mentions cleanup
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="Teammate" TOOL_INPUT='{"operation":"cleanup"}' bash "$HOOK" </dev/null 2>&1) || true
rm -rf "$TEMP_DIR"
if [[ "$output" == *"cleanup"* ]]; then
    pass "Block message mentions cleanup"
else
    fail "Block message should mention cleanup"
fi

echo ""
echo "--- Execution Tests: TeamDelete Without PathFlow ---"

# Test 28: Exits 0 for TeamDelete when pathflow-active missing
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/nonexistent-flag" TOOL_NAME="TeamDelete" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for TeamDelete when pathflow-active missing"
else
    fail "Should exit 0 for TeamDelete when pathflow-active missing"
fi

echo ""
echo "--- Execution Tests: TeamDelete With PathFlow (BLOCKED) ---"

# Test 29: Exits 2 for TeamDelete when pathflow-active exists
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="TeamDelete" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Exits 2 for TeamDelete when pathflow-active exists"
else
    fail "Should exit 2 for TeamDelete when pathflow-active exists (got exit=$exit_code)"
fi

# Test 30: TeamDelete block message mentions TeamDelete
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="TeamDelete" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1) || true
rm -rf "$TEMP_DIR"
if [[ "$output" == *"TeamDelete"* ]]; then
    pass "TeamDelete block message mentions TeamDelete"
else
    fail "TeamDelete block message should mention TeamDelete"
fi

# Test 31: TeamDelete block message mentions PF7-END
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="TeamDelete" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1) || true
rm -rf "$TEMP_DIR"
if [[ "$output" == *"PF7-END"* ]]; then
    pass "TeamDelete block message mentions PF7-END"
else
    fail "TeamDelete block message should mention PF7-END"
fi

# Test 32: TeamDelete does not need operation field (always blocked during PathFlow)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="TeamDelete" TOOL_INPUT='' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]]; then
    pass "TeamDelete blocked even with empty input"
else
    fail "TeamDelete should be blocked even with empty input (got exit=$exit_code)"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
