#!/usr/bin/env bash
# Test: cf-pre-tool-use-pathflow-gate.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-pathflow-gate.sh
#
# Tests PathFlow gate hook (v2.0.0 - JSONL-backed enforcement):
#   - File exists, executable, shellcheck, headers, strict mode, VERSION
#   - Exits 0 when no pathflow-active flag (standalone mode)
#   - Exits 0 when TOOL_NAME is not Edit/Write/Bash
#   - JSONL-based phase gating: Edit/Write blocked before PF4-EXECUTE
#   - JSONL-based phase gating: git commit blocked before PF4-EXECUTE
#   - JSONL-based phase gating: git push/gh pr blocked before PF6-COMPLETE
#   - Graceful degradation: ALLOW when JSONL/session-id missing
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

echo "=== Testing cf-pre-tool-use-pathflow-gate.sh (v2 JSONL) ==="
echo ""

# =============================================================================
# HELPER: Create JSONL test data
# =============================================================================

# Write a phase_transition event to a JSONL file
write_phase_event() {
    local jsonl_file="$1"
    local session_id="$2"
    local phase="$3"
    local status="${4:-entered}"
    echo "{\"type\":\"phase_transition\",\"session_id\":\"$session_id\",\"phase\":\"$phase\",\"status\":\"$status\",\"ts\":\"2026-01-01T00:00:00Z\"}" >> "$jsonl_file"
}

# Set up JSONL test environment in isolated temp dir
setup_jsonl_env() {
    local session_id="${1:-test-session-001}"
    local flag_file="$TEST_TMPDIR/pathflow-active"
    local session_id_file="$TEST_TMPDIR/current-session-id"
    local jsonl_file="$TEST_TMPDIR/pathflow-events.jsonl"

    touch "$flag_file"
    echo "$session_id" > "$session_id_file"
    : > "$jsonl_file"

    echo "$flag_file|$session_id_file|$jsonl_file|$session_id"
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

# Test 6: Has VERSION constant (v2.0.0)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'VERSION="2.0.0"' "$HOOK"; then
    pass "Has VERSION 2.0.0"
else
    fail "Should have VERSION 2.0.0"
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

# Test 17: Has phase_to_num function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "phase_to_num" "$HOOK"; then
    pass "Has phase_to_num function"
else
    fail "Should have phase_to_num function"
fi

# Test 18: References JSONL file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "pathflow-events.jsonl" "$HOOK"; then
    pass "References pathflow-events.jsonl"
else
    fail "Should reference pathflow-events.jsonl"
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

# Test 23: Exits 0 when PathFlow active but no session-id file
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-23"
touch "$flag_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$TEST_TMPDIR/nonexistent-session-id" PATHFLOW_JSONL_FILE="$TEST_TMPDIR/nonexistent.jsonl" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: no session-id file"
else
    fail "Should exit 0 when session-id file missing"
fi

# Test 24: Exits 0 when PathFlow active but empty session-id
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-24"
sid_file="$TEST_TMPDIR/sid-empty-24"
touch "$flag_file"
: > "$sid_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$TEST_TMPDIR/nonexistent.jsonl" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: empty session-id"
else
    fail "Should exit 0 when session-id is empty"
fi

# Test 25: Exits 0 when PathFlow active but no JSONL file
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-25"
sid_file="$TEST_TMPDIR/sid-25"
touch "$flag_file"
echo "test-session" > "$sid_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$TEST_TMPDIR/nonexistent-25.jsonl" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: no JSONL file"
else
    fail "Should exit 0 when JSONL file missing"
fi

# Test 26: Exits 0 when JSONL has no matching session events
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-26"
sid_file="$TEST_TMPDIR/sid-26"
jsonl_file="$TEST_TMPDIR/events-26.jsonl"
touch "$flag_file"
echo "my-session" > "$sid_file"
echo '{"type":"phase_transition","session_id":"other-session","phase":"PF1-INIT","status":"entered"}' > "$jsonl_file"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful degradation: no matching session events"
else
    fail "Should exit 0 when no events match current session"
fi

echo ""
echo "--- Execution Tests: Phase Gating - Edit/Write ---"

# Test 27: BLOCKS Edit when phase is PF1-INIT (< PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-27"
sid_file="$TEST_TMPDIR/sid-27"
jsonl_file="$TEST_TMPDIR/events-27.jsonl"
touch "$flag_file"
echo "sess-27" > "$sid_file"
write_phase_event "$jsonl_file" "sess-27" "PF1-INIT" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Edit at PF1-INIT"
else
    fail "Should block Edit at PF1-INIT (got exit=$exit_code)"
fi

# Test 28: BLOCKS Write when phase is PF2-CONTEXT (< PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-28"
sid_file="$TEST_TMPDIR/sid-28"
jsonl_file="$TEST_TMPDIR/events-28.jsonl"
touch "$flag_file"
echo "sess-28" > "$sid_file"
write_phase_event "$jsonl_file" "sess-28" "PF2-CONTEXT" "completed"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Write at PF2-CONTEXT"
else
    fail "Should block Write at PF2-CONTEXT (got exit=$exit_code)"
fi

# Test 29: BLOCKS Edit when phase is PF3-CLASSIFY (< PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-29"
sid_file="$TEST_TMPDIR/sid-29"
jsonl_file="$TEST_TMPDIR/events-29.jsonl"
touch "$flag_file"
echo "sess-29" > "$sid_file"
write_phase_event "$jsonl_file" "sess-29" "PF1-INIT" "entered"
write_phase_event "$jsonl_file" "sess-29" "PF2-CONTEXT" "completed"
write_phase_event "$jsonl_file" "sess-29" "PF3-CLASSIFY" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks Edit at PF3-CLASSIFY"
else
    fail "Should block Edit at PF3-CLASSIFY (got exit=$exit_code)"
fi

# Test 30: ALLOWS Edit when phase is PF4-EXECUTE (>= PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-30"
sid_file="$TEST_TMPDIR/sid-30"
jsonl_file="$TEST_TMPDIR/events-30.jsonl"
touch "$flag_file"
echo "sess-30" > "$sid_file"
write_phase_event "$jsonl_file" "sess-30" "PF4-EXECUTE" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit at PF4-EXECUTE"
else
    fail "Should allow Edit at PF4-EXECUTE"
fi

# Test 31: ALLOWS Write when phase is PF5-VERIFY (>= PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-31"
sid_file="$TEST_TMPDIR/sid-31"
jsonl_file="$TEST_TMPDIR/events-31.jsonl"
touch "$flag_file"
echo "sess-31" > "$sid_file"
write_phase_event "$jsonl_file" "sess-31" "PF5-VERIFY" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Write" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Write at PF5-VERIFY"
else
    fail "Should allow Write at PF5-VERIFY"
fi

echo ""
echo "--- Execution Tests: Phase Gating - git commit ---"

# Test 32: BLOCKS git commit when phase is PF3-CLASSIFY (< PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-32"
sid_file="$TEST_TMPDIR/sid-32"
jsonl_file="$TEST_TMPDIR/events-32.jsonl"
touch "$flag_file"
echo "sess-32" > "$sid_file"
write_phase_event "$jsonl_file" "sess-32" "PF3-CLASSIFY" "completed"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"fix: something\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks git commit at PF3-CLASSIFY"
else
    fail "Should block git commit at PF3-CLASSIFY (got exit=$exit_code)"
fi

# Test 33: ALLOWS git commit when phase is PF4-EXECUTE (>= PF4)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-33"
sid_file="$TEST_TMPDIR/sid-33"
jsonl_file="$TEST_TMPDIR/events-33.jsonl"
touch "$flag_file"
echo "sess-33" > "$sid_file"
write_phase_event "$jsonl_file" "sess-33" "PF4-EXECUTE" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"feat: new feature\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git commit at PF4-EXECUTE"
else
    fail "Should allow git commit at PF4-EXECUTE"
fi

echo ""
echo "--- Execution Tests: Phase Gating - git push / gh pr ---"

# Test 34: BLOCKS git push when phase is PF4-EXECUTE (< PF6)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-34"
sid_file="$TEST_TMPDIR/sid-34"
jsonl_file="$TEST_TMPDIR/events-34.jsonl"
touch "$flag_file"
echo "sess-34" > "$sid_file"
write_phase_event "$jsonl_file" "sess-34" "PF4-EXECUTE" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push -u origin feat/test"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]] && [[ "$output" == *"PF6-COMPLETE"* ]]; then
    pass "Blocks git push at PF4-EXECUTE (requires PF6)"
else
    fail "Should block git push at PF4-EXECUTE (got exit=$exit_code)"
fi

# Test 35: BLOCKS gh pr create when phase is PF5-VERIFY (< PF6)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-35"
sid_file="$TEST_TMPDIR/sid-35"
jsonl_file="$TEST_TMPDIR/events-35.jsonl"
touch "$flag_file"
echo "sess-35" > "$sid_file"
write_phase_event "$jsonl_file" "sess-35" "PF5-VERIFY" "completed"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test\""}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"* ]]; then
    pass "Blocks gh pr at PF5-VERIFY (requires PF6)"
else
    fail "Should block gh pr at PF5-VERIFY (got exit=$exit_code)"
fi

# Test 36: ALLOWS git push when phase is PF6-COMPLETE (>= PF6)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-36"
sid_file="$TEST_TMPDIR/sid-36"
jsonl_file="$TEST_TMPDIR/events-36.jsonl"
touch "$flag_file"
echo "sess-36" > "$sid_file"
write_phase_event "$jsonl_file" "sess-36" "PF6-COMPLETE" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push -u origin feat/test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git push at PF6-COMPLETE"
else
    fail "Should allow git push at PF6-COMPLETE"
fi

# Test 37: ALLOWS gh pr create when phase is PF7-END (>= PF6)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-37"
sid_file="$TEST_TMPDIR/sid-37"
jsonl_file="$TEST_TMPDIR/events-37.jsonl"
touch "$flag_file"
echo "sess-37" > "$sid_file"
write_phase_event "$jsonl_file" "sess-37" "PF7-END" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh pr at PF7-END"
else
    fail "Should allow gh pr at PF7-END"
fi

echo ""
echo "--- Execution Tests: Bash Non-Gated Commands ---"

# Test 38: Exits 0 for ls command
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-38"
sid_file="$TEST_TMPDIR/sid-38"
jsonl_file="$TEST_TMPDIR/events-38.jsonl"
touch "$flag_file"
echo "sess-38" > "$sid_file"
write_phase_event "$jsonl_file" "sess-38" "PF1-INIT" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for ls command (not gated)"
else
    fail "Should exit 0 for non-gated Bash commands"
fi

# Test 39: Exits 0 for git status
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-39"
sid_file="$TEST_TMPDIR/sid-39"
jsonl_file="$TEST_TMPDIR/events-39.jsonl"
touch "$flag_file"
echo "sess-39" > "$sid_file"
write_phase_event "$jsonl_file" "sess-39" "PF1-INIT" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git status"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for git status (not gated)"
else
    fail "Should exit 0 for git status"
fi

# Test 40: Exits 0 for git diff
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-40"
sid_file="$TEST_TMPDIR/sid-40"
jsonl_file="$TEST_TMPDIR/events-40.jsonl"
touch "$flag_file"
echo "sess-40" > "$sid_file"
write_phase_event "$jsonl_file" "sess-40" "PF1-INIT" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git diff HEAD"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
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

# Test 42: Block message mentions current phase and required phase
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-42"
sid_file="$TEST_TMPDIR/sid-42"
jsonl_file="$TEST_TMPDIR/events-42.jsonl"
touch "$flag_file"
echo "sess-42" > "$sid_file"
write_phase_event "$jsonl_file" "sess-42" "PF2-CONTEXT" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"PF2-CONTEXT"* ]] && [[ "$output" == *"PF4-EXECUTE"* ]]; then
    pass "Block message shows current and required phase"
else
    fail "Block message should show current and required phase"
fi

# Test 43: Block message mentions gate type
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-43"
sid_file="$TEST_TMPDIR/sid-43"
jsonl_file="$TEST_TMPDIR/events-43.jsonl"
touch "$flag_file"
echo "sess-43" > "$sid_file"
write_phase_event "$jsonl_file" "sess-43" "PF4-EXECUTE" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Bash" TOOL_INPUT='{"command":"git push origin main"}' bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"git_push_pr"* ]]; then
    pass "Block message includes gate type for git push"
else
    fail "Block message should include gate type"
fi

echo ""
echo "--- Execution Tests: Multiple Phase Events ---"

# Test 44: Uses latest phase event (not first)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-44"
sid_file="$TEST_TMPDIR/sid-44"
jsonl_file="$TEST_TMPDIR/events-44.jsonl"
touch "$flag_file"
echo "sess-44" > "$sid_file"
# Write progression through phases - latest is PF4
write_phase_event "$jsonl_file" "sess-44" "PF1-INIT" "entered"
write_phase_event "$jsonl_file" "sess-44" "PF1-INIT" "completed"
write_phase_event "$jsonl_file" "sess-44" "PF2-CONTEXT" "entered"
write_phase_event "$jsonl_file" "sess-44" "PF2-CONTEXT" "completed"
write_phase_event "$jsonl_file" "sess-44" "PF3-CLASSIFY" "entered"
write_phase_event "$jsonl_file" "sess-44" "PF3-CLASSIFY" "completed"
write_phase_event "$jsonl_file" "sess-44" "PF4-EXECUTE" "entered"
result=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Uses latest phase event (PF4 allows Edit)"
else
    fail "Should use latest phase event, not first"
fi

# Test 45: Filters by session_id (other session events ignored)
TESTS_RUN=$((TESTS_RUN + 1))
flag_file="$TEST_TMPDIR/pf-active-45"
sid_file="$TEST_TMPDIR/sid-45"
jsonl_file="$TEST_TMPDIR/events-45.jsonl"
touch "$flag_file"
echo "my-session" > "$sid_file"
# Another session is at PF4, but our session is at PF1
write_phase_event "$jsonl_file" "other-session" "PF4-EXECUTE" "entered"
write_phase_event "$jsonl_file" "my-session" "PF1-INIT" "entered"
output=$(PATHFLOW_FLAG_FILE="$flag_file" PATHFLOW_SESSION_ID_FILE="$sid_file" PATHFLOW_JSONL_FILE="$jsonl_file" TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1) && exit_code=0 || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "Filters by session_id (blocks despite other session at PF4)"
else
    fail "Should filter by session_id (got exit=$exit_code)"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
