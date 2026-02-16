#!/usr/bin/env bash
# Test: cf-pre-tool-use-pathflow-gate.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-pathflow-gate.sh
#
# Tests PathFlow gate hook (v4.1.0 - sentinel-backed enforcement):
#   - File exists, executable, shellcheck, headers, strict mode, VERSION
#   - Exits 0 when no pathflow-active flag (standalone mode)
#   - Exits 0 when TOOL_NAME is not Edit/Write/Bash
#   - Sentinel-based gating: Edit/Write blocked without pf-3 sentinel
#   - Sentinel-based gating: git commit blocked without pf-3 sentinel
#   - Sentinel-based gating: git push/gh pr blocked without ws-rev sentinel
#   - Graceful degradation: non-critical gates ALLOW, critical gates BLOCK
#   - Session-unknown early check: critical gates BLOCK immediately
#   - Exits 0 for non-gated Bash commands

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

echo "=== Testing cf-pre-tool-use-pathflow-gate.sh (v4.1 sentinel) ==="
echo ""

# =============================================================================
# HELPER: Create sentinel files for testing
# =============================================================================

# Create a PathFlow sentinel in the isolated REPO_ROOT
create_test_sentinel() {
    local session_id="$1"
    local sentinel_name="$2"
    local sentinel_dir="$REPO_ROOT/.state/sentinels/pathflow/$session_id"
    mkdir -p "$sentinel_dir"
    touch "$sentinel_dir/pathflow-$sentinel_name"
}

# Create a degraded REPO_ROOT without sentinel library (for graceful degradation tests)
create_degraded_root() {
    local degraded_root
    degraded_root=$(mktemp -d "$TEST_TMPDIR/degraded-XXXXXX")
    mkdir -p "$degraded_root/.state/sentinels/pathflow"
    echo "$degraded_root"
}

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

# Test 6: Has VERSION constant (v4.1.0)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'VERSION="4.1.0"' "$HOOK"; then
    pass "Has VERSION 4.1.0"
else
    fail "Should have VERSION 4.1.0"
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

# Test 17: Uses has_sentinel for enforcement
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "has_sentinel" "$HOOK"; then
    pass "Uses has_sentinel for enforcement"
else
    fail "Should use has_sentinel for sentinel checks"
fi

# Test 18: Sources pathflow state library
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-pathflow-state" "$HOOK"; then
    pass "Sources pathflow state library"
else
    fail "Should source cf-pathflow-state.sh"
fi

echo ""
echo "--- Execution Tests: Standalone Mode ---"

# Test 19: Exits 0 when no pathflow-active flag (standalone mode)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(PATHFLOW_FLAG_FILE="$TEST_TMPDIR/nonexistent-flag" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no pathflow-active flag (standalone mode)"
else
    fail "Should exit 0 when pathflow-active flag missing"
fi

# Test 20: Exits 0 when TOOL_NAME is not Edit/Write/Bash
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 21: Exits 0 for Grep tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 22: Exits 0 for Glob tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(TOOL_NAME="Glob" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Glob tool"
else
    fail "Should exit 0 for Glob tool"
fi

echo ""
echo "--- Execution Tests: Graceful Degradation ---"

# Test 23: Exits 0 when sentinel library not available (graceful degradation)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-23"
touch "$flag_file"
degraded_root=$(create_degraded_root)
result=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-23" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: sentinel library missing"
else
    fail "Should exit 0 when sentinel library not available"
fi

# Test 24: Graceful degradation for git commit too
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-24"
touch "$flag_file"
degraded_root=$(create_degraded_root)
result=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-24" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: git commit when sentinel library missing"
else
    fail "Should exit 0 for git commit when sentinel library not available"
fi

# Test 25: Graceful degradation BLOCKS git push (critical gate)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-25"
touch "$flag_file"
degraded_root=$(create_degraded_root)
output=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-25" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push origin main"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Graceful degradation: git push BLOCKED (critical gate)"
else
    fail "Should block git push when sentinel library not available (got exit=$exit_code)"
fi

# Test 26: Graceful degradation preserves non-gated command behavior
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-26"
touch "$flag_file"
degraded_root=$(create_degraded_root)
result=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-26" TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: non-gated commands still allowed"
else
    fail "Should exit 0 for non-gated commands in degraded mode"
fi

echo ""
echo "--- Execution Tests: Sentinel Gating - Edit/Write ---"

# Test 27: BLOCKS Edit when no pf-3 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-27"
touch "$flag_file"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-27" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Edit without pf-3 sentinel"
else
    fail "Should block Edit without pf-3 sentinel (got exit=$exit_code)"
fi

# Test 28: BLOCKS Write when no pf-3 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-28"
touch "$flag_file"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-28" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Write without pf-3 sentinel"
else
    fail "Should block Write without pf-3 sentinel (got exit=$exit_code)"
fi

# Test 29: BLOCKS Edit when pf-1 sentinel exists but no pf-3
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-29"
touch "$flag_file"
create_test_sentinel "sess-29" "pf-1"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-29" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Edit with pf-1 but without pf-3 sentinel"
else
    fail "Should block Edit without pf-3 sentinel even with pf-1 (got exit=$exit_code)"
fi

# Test 30: ALLOWS Edit when pf-3 sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-30"
touch "$flag_file"
create_test_sentinel "sess-30" "pf-3"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-30" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit with pf-3 sentinel"
else
    fail "Should allow Edit with pf-3 sentinel"
fi

# Test 31: ALLOWS Write when pf-3 sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-31"
touch "$flag_file"
create_test_sentinel "sess-31" "pf-3"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-31" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Write with pf-3 sentinel"
else
    fail "Should allow Write with pf-3 sentinel"
fi

echo ""
echo "--- Execution Tests: Sentinel Gating - git commit ---"

# Test 32: BLOCKS git commit when no pf-3 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-32"
touch "$flag_file"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-32" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"fix: something\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks git commit without pf-3 sentinel"
else
    fail "Should block git commit without pf-3 sentinel (got exit=$exit_code)"
fi

# Test 33: ALLOWS git commit when pf-3 sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-33"
touch "$flag_file"
create_test_sentinel "sess-33" "pf-3"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-33" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"feat: new feature\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git commit with pf-3 sentinel"
else
    fail "Should allow git commit with pf-3 sentinel"
fi

echo ""
echo "--- Execution Tests: Sentinel Gating - git push / gh pr ---"

# Test 34: BLOCKS git push when no ws-rev sentinel
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-34"
touch "$flag_file"
create_test_sentinel "sess-34" "pf-3"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-34" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push -u origin feat/test"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]] && [[ "$output" == *"ws-rev"* ]]; then
    pass "Blocks git push without ws-rev sentinel"
else
    fail "Should block git push without ws-rev sentinel (got exit=$exit_code)"
fi

# Test 35: BLOCKS gh pr create when no ws-rev sentinel
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-35"
touch "$flag_file"
create_test_sentinel "sess-35" "pf-3"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-35" TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks gh pr without ws-rev sentinel"
else
    fail "Should block gh pr without ws-rev sentinel (got exit=$exit_code)"
fi

# Test 36: ALLOWS git push when ws-rev sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-36"
touch "$flag_file"
create_test_sentinel "sess-36" "pf-3"
create_test_sentinel "sess-36" "ws-rev"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-36" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push -u origin feat/test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git push with ws-rev sentinel"
else
    fail "Should allow git push with ws-rev sentinel"
fi

# Test 37: ALLOWS gh pr create when ws-rev sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-37"
touch "$flag_file"
create_test_sentinel "sess-37" "pf-3"
create_test_sentinel "sess-37" "ws-rev"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-37" TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh pr with ws-rev sentinel"
else
    fail "Should allow gh pr with ws-rev sentinel"
fi

echo ""
echo "--- Execution Tests: Bash Non-Gated Commands ---"

# Test 38: Exits 0 for ls command
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-38"
touch "$flag_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-38" TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for ls command (not gated)"
else
    fail "Should exit 0 for non-gated Bash commands"
fi

# Test 39: Exits 0 for git status
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-39"
touch "$flag_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-39" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git status"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for git status (not gated)"
else
    fail "Should exit 0 for git status"
fi

# Test 40: Exits 0 for git diff
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-40"
touch "$flag_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-40" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git diff HEAD"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for git diff (not gated)"
else
    fail "Should exit 0 for git diff"
fi

# Test 41: Exits 0 for Bash with empty input
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-41"
touch "$flag_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" TOOL_NAME="Bash" TOOL_INPUT='' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash with empty input"
else
    fail "Should exit 0 for Bash with empty input"
fi

echo ""
echo "--- Execution Tests: Block Message Quality ---"

# Test 42: Block message mentions sentinel name and required description
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-42"
touch "$flag_file"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-42" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"pf-3"* ]] && [[ "$output" == *"PF3-CLASSIFY"* ]]; then
    pass "Block message shows sentinel name and required phase"
else
    fail "Block message should show sentinel name (pf-3) and required phase (PF3-CLASSIFY)"
fi

# Test 43: Block message includes gate type for git push
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-43"
touch "$flag_file"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-43" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push origin main"}' bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"git_push_pr"* ]]; then
    pass "Block message includes gate type for git push"
else
    fail "Block message should include gate type"
fi

echo ""
echo "--- Execution Tests: Sentinel Isolation ---"

# Test 44: pf-3 sentinel allows Edit but git push still blocked (no ws-rev)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-44"
touch "$flag_file"
create_test_sentinel "sess-44" "pf-3"
# Edit should be allowed (pf-3 exists)
result_edit=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-44" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
# git push should be blocked (no ws-rev)
_output_push=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-44" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push origin main"}' bash "$HOOK" </dev/null 2>&1) && push_exit=0 || push_exit=$?
if [[ "$result_edit" == *"EXIT:0"* ]] && [[ $push_exit -eq 2 ]]; then
    pass "pf-3 allows Edit but git push still blocked (no ws-rev)"
else
    fail "Should allow Edit (pf-3) but block git push (no ws-rev)"
fi

# Test 45: Different session's sentinels don't affect current session
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-45"
touch "$flag_file"
# Create sentinel for a DIFFERENT session
create_test_sentinel "other-session" "pf-3"
output=$(PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-45" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "Different session's sentinels don't affect current session"
else
    fail "Should not use another session's sentinels (got exit=$exit_code)"
fi

echo ""
echo "--- Execution Tests: Session Unknown Early Check ---"

# Test 46: BLOCKS git push when session ID is "unknown" (early check)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-46"
touch "$flag_file"
degraded_root=$(create_degraded_root)
output=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="unknown" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push origin main"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"session state unknown"* ]]; then
    pass "Blocks git push when session ID is unknown"
else
    fail "Should block git push when session ID is unknown (got exit=$exit_code)"
fi

# Test 47: BLOCKS gh pr when session ID is "unknown" (early check)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-47"
touch "$flag_file"
degraded_root=$(create_degraded_root)
output=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="unknown" TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"session state unknown"* ]]; then
    pass "Blocks gh pr when session ID is unknown"
else
    fail "Should block gh pr when session ID is unknown (got exit=$exit_code)"
fi

# Test 48: ALLOWS Edit when session ID is "unknown" (non-critical, degraded root)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-48"
touch "$flag_file"
degraded_root=$(create_degraded_root)
result=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="unknown" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit when session ID is unknown (non-critical)"
else
    fail "Should allow Edit when session ID is unknown"
fi

# Test 49: ALLOWS git commit when session ID is "unknown" (non-critical, degraded root)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-49"
touch "$flag_file"
degraded_root=$(create_degraded_root)
result=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="unknown" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"fix: test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git commit when session ID is unknown (non-critical)"
else
    fail "Should allow git commit when session ID is unknown"
fi

echo ""
echo "--- Execution Tests: Graceful Degradation - Role Teammate Spawn ---"

# Test 50: Graceful degradation BLOCKS role teammate spawn (critical gate)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-50"
touch "$flag_file"
degraded_root=$(create_degraded_root)
output=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="sess-50" TOOL_NAME="Task" TOOL_INPUT='{"prompt":"Read .claude/agents/cf-development.md","name":"cf-development","description":"Spawn cf-development"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Graceful degradation: role teammate spawn BLOCKED (critical gate)"
else
    fail "Should block role teammate spawn when sentinel library not available (got exit=$exit_code)"
fi

# Test 51: BLOCKS role teammate spawn when session ID is "unknown" (early check)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-51"
touch "$flag_file"
degraded_root=$(create_degraded_root)
output=$(REPO_ROOT="$degraded_root" PATHFLOW_FLAG_FILE="$flag_file" CODEFLOW_SESSION_ID="unknown" TOOL_NAME="Task" TOOL_INPUT='{"prompt":"Read .claude/agents/cf-development.md","name":"cf-development","description":"Spawn cf-development"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"session state unknown"* ]]; then
    pass "Blocks role teammate spawn when session ID is unknown"
else
    fail "Should block role teammate spawn when session ID is unknown (got exit=$exit_code)"
fi

echo ""
echo "--- Execution Tests: Env File Session ID ---"

# Test 52: Hook references env file (codeflow-env.sh)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "codeflow-env.sh" "$HOOK"; then
    pass "Hook references env file (codeflow-env.sh)"
else
    fail "Hook should reference codeflow-env.sh for session ID"
fi

# Test 53: Hook has TODO(go-cli) comment near env file sourcing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TODO(go-cli)" "$HOOK"; then
    pass "Has TODO(go-cli) comment"
else
    fail "Should have TODO(go-cli) comment near session ID sourcing"
fi

# Test 54: Hook uses session ID from env file when present
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-54"
touch "$flag_file"
# Create env file in isolated REPO_ROOT
env_file="$REPO_ROOT/.state/runtime/codeflow-env.sh"
mkdir -p "$(dirname "$env_file")"
echo "export CODEFLOW_SESSION_ID='ses-envtest54'" > "$env_file"
# Create pf-3 sentinel for the env file session ID
create_test_sentinel "ses-envtest54" "pf-3"
# Run hook — it should source env file and use ses-envtest54, finding the pf-3 sentinel
result=$(PATHFLOW_FLAG_FILE="$flag_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Uses session ID from env file (sentinel found)"
else
    fail "Should use session ID from env file to find sentinel"
fi
rm -f "$env_file"

# Test 55: Hook falls back to hook input .session_id when env file missing
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-55"
touch "$flag_file"
# Ensure no env file exists
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
# Create pf-3 sentinel for the fallback session ID
create_test_sentinel "fallback-uuid-55" "pf-3"
# Send session_id via stdin (simulating Claude Code hook protocol)
stdin_json='{"tool_name":"Edit","tool_input":{"file_path":"test.txt"},"session_id":"fallback-uuid-55"}'
result=$(PATHFLOW_FLAG_FILE="$flag_file" bash "$HOOK" <<< "$stdin_json" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Falls back to hook input session_id when env file missing"
else
    fail "Should fall back to hook input session_id when env file missing"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
