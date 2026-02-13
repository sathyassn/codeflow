#!/usr/bin/env bash
# Test: cf-pre-tool-use-pathflow-gate.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-pathflow-gate.sh
#
# Tests PathFlow gate hook:
#   - File exists, executable, shellcheck, headers, strict mode, VERSION
#   - Exits 0 when no pathflow-active flag (standalone mode)
#   - Exits 0 when TOOL_NAME is not Edit/Write/Bash
#   - Exits 0 when PF-3 sentinel exists
#   - Exits 2 when pathflow-active exists but no PF-3 sentinel (Edit)
#   - Exits 2 when pathflow-active exists but no PF-3 sentinel (Write)
#   - Exits 2 for Bash with git commit when gated
#   - Exits 0 for Bash with non-commit commands when gated

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-pre-tool-use-pathflow-gate.sh ==="
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
echo "--- Execution Tests: Standalone Mode ---"

# Test 17: Exits 0 when no pathflow-active flag (standalone mode)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/nonexistent-flag" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no pathflow-active flag (standalone mode)"
else
    fail "Should exit 0 when pathflow-active flag missing"
fi

# Test 18: Exits 0 when TOOL_NAME is not Edit/Write/Bash
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 19: Exits 0 for Grep tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 20: Exits 0 for Glob tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Glob" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Glob tool"
else
    fail "Should exit 0 for Glob tool"
fi

echo ""
echo "--- Execution Tests: PathFlow Active, PF-3 Complete ---"

# Test 21: Exits 0 when PF-3 sentinel exists (Edit)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
touch "$TEMP_DIR/sentinels/pathflow-pf-3-classify"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when PF-3 sentinel exists (Edit)"
else
    fail "Should exit 0 when PF-3 sentinel exists"
fi

# Test 22: Exits 0 when PF-3 sentinel exists (Write)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
touch "$TEMP_DIR/sentinels/pathflow-pf-3-work-classification"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when PF-3 sentinel exists (Write)"
else
    fail "Should exit 0 when PF-3 sentinel exists for Write"
fi

# Test 23: Exits 0 when PF-3 sentinel exists (Bash git commit)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
touch "$TEMP_DIR/sentinels/pathflow-pf-3-done"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when PF-3 sentinel exists (Bash git commit)"
else
    fail "Should exit 0 when PF-3 sentinel exists for git commit"
fi

echo ""
echo "--- Execution Tests: PathFlow Active, PF-3 Missing (BLOCKED) ---"

# Test 24: Exits 2 when pathflow-active exists but no PF-3 sentinel (Edit)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
# No PF-3 sentinel
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Exits 2 when pathflow-active but no PF-3 (Edit)"
else
    fail "Should exit 2 when pathflow-active but no PF-3 for Edit (got exit=$exit_code)"
fi

# Test 25: Exits 2 when pathflow-active exists but no PF-3 sentinel (Write)
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Exits 2 when pathflow-active but no PF-3 (Write)"
else
    fail "Should exit 2 when pathflow-active but no PF-3 for Write (got exit=$exit_code)"
fi

# Test 26: Exits 2 for Bash with git commit when gated
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"fix: something\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
rm -rf "$TEMP_DIR"
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Exits 2 for Bash git commit when gated"
else
    fail "Should exit 2 for Bash git commit when gated (got exit=$exit_code)"
fi

# Test 27: Block message mentions PathFlow
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
output=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) || true
rm -rf "$TEMP_DIR"
if [[ "$output" == *"PathFlow"* ]] && [[ "$output" == *"PF-3"* ]]; then
    pass "Block message mentions PathFlow and PF-3"
else
    fail "Block message should mention PathFlow and PF-3"
fi

echo ""
echo "--- Execution Tests: Bash Non-Commit Commands ---"

# Test 28: Exits 0 for Bash with non-commit commands when PathFlow active
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash ls command (not gated)"
else
    fail "Should exit 0 for non-commit Bash commands"
fi

# Test 29: Exits 0 for Bash with git status when PathFlow active
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git status"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash git status (not gated)"
else
    fail "Should exit 0 for git status"
fi

# Test 30: Exits 0 for Bash with git diff when PathFlow active
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state" "$TEMP_DIR/sentinels"
touch "$TEMP_DIR/state/pathflow-active"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" PATHFLOW_SENTINEL_DIR="$TEMP_DIR/sentinels" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git diff HEAD"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash git diff (not gated)"
else
    fail "Should exit 0 for git diff"
fi

# Test 31: Exits 0 for Bash with empty input when PathFlow active
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/state"
touch "$TEMP_DIR/state/pathflow-active"
result=$(PATHFLOW_FLAG_FILE="$TEMP_DIR/state/pathflow-active" TOOL_NAME="Bash" TOOL_INPUT='' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash with empty input"
else
    fail "Should exit 0 for Bash with empty input"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
