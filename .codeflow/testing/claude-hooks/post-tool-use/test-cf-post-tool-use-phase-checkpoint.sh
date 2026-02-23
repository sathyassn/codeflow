#!/usr/bin/env bash
# Test: cf-post-tool-use-phase-checkpoint.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-phase-checkpoint.sh
#
# Tests PostToolUse hook for phase checkpoint registration (Layer 1):
#   - File exists, executable, shellcheck, headers, strict mode
#   - Only processes TaskCreate tool events
#   - Parses PF{N}-TSK-{NN} from tool_input.subject
#   - Registers matching tasks in checkpoint file
#   - Ignores non-PF subjects silently
#   - Handles missing session ID, missing pathflow flag
#   - Env file session ID takes priority over stdin
#
# Exit codes:
#   0 - All tests passed
#   1 - One or more tests failed

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-phase-checkpoint.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-phase-checkpoint.sh ==="
echo ""

# =============================================================================
# HELPER: Create pathflow-active flag and env file
# =============================================================================

create_test_flag() {
    local session_id="$1"
    local session_dir="$REPO_ROOT/.state/session/$session_id"
    mkdir -p "$session_dir/pathflow"
    echo "{\"session_id\":\"$session_id\"}" > "$session_dir/pathflow/is-pathflow-active"
}

create_test_env_file() {
    local session_id="$1"
    local env_file="$REPO_ROOT/.state/runtime/codeflow-env.sh"
    mkdir -p "$(dirname "$env_file")"
    echo "export CODEFLOW_SESSION_ID='$session_id'" > "$env_file"
}

remove_test_env_file() {
    rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
}

has_checkpoint_file() {
    local session_id="$1"
    [[ -f "$REPO_ROOT/.state/session/$session_id/pathflow/pathflow-phase-tasks.json" ]]
}

read_checkpoint() {
    local session_id="$1"
    local ckpt="$REPO_ROOT/.state/session/$session_id/pathflow/pathflow-phase-tasks.json"
    if [[ -f "$ckpt" ]]; then
        cat "$ckpt"
    else
        echo "{}"
    fi
}

cleanup_test() {
    local session_id="$1"
    rm -rf "$REPO_ROOT/.state/session/$session_id/pathflow" 2>/dev/null || true
    rm -rf "$REPO_ROOT/.state/session/$session_id" 2>/dev/null || true
    rm -rf "$REPO_ROOT/.state/sentinels/pathflow/$session_id" 2>/dev/null || true
    remove_test_env_file
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

# Test 6: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Code Quality ---"

# Test 7: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 8: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 9: Sources pathflow state library
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-pathflow-state" "$HOOK"; then
    pass "Sources pathflow state library"
else
    fail "Should source cf-pathflow-state.sh"
fi

# Test 10: Uses checkpoint_register_task function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "checkpoint_register_task" "$HOOK"; then
    pass "Uses checkpoint_register_task function"
else
    fail "Should use checkpoint_register_task function"
fi

# Test 11: Only matches TaskCreate tool
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"TaskCreate"' "$HOOK"; then
    pass "Filters for TaskCreate tool"
else
    fail "Should filter for TaskCreate tool"
fi

# Test 12: Matches PF task pattern
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'PF\[0-9\]' "$HOOK"; then
    pass "Has PF task pattern regex"
else
    fail "Should have PF task pattern regex"
fi

# Test 13: Has exit 2 for cross-phase blocking
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'exit 2' "$HOOK"; then
    pass "Has exit 2 for cross-phase blocking"
else
    fail "Should have exit 2 for cross-phase registration blocking"
fi

# Test 14: References env file (codeflow-env.sh)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "codeflow-env.sh" "$HOOK"; then
    pass "References env file for session ID"
else
    fail "Should reference codeflow-env.sh for session ID"
fi

echo ""
echo "--- Execution Tests: Early Exit ---"

# Test 15: Exits 0 for non-TaskCreate tools
TESTS_RUN=$((TESTS_RUN + 1))
stdin_json='{"tool_name":"Edit","tool_input":{"file_path":"test.sh"},"session_id":"ses-nontaskcreate-15"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 for non-TaskCreate tool (Edit)"
else
    fail "Should exit 0 for non-TaskCreate tools"
fi

# Test 16: Exits 0 when pathflow not active
TESTS_RUN=$((TESTS_RUN + 1))
remove_test_env_file
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01: Init"},"session_id":"ses-inactive-16"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 when pathflow not active"
else
    fail "Should exit 0 when pathflow not active"
fi

# Test 17: Exits 0 for TaskCreate without PF pattern in subject
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-nonpf-17"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"Fix the login bug"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 for non-PF subject"
else
    fail "Should exit 0 for non-PF subject"
fi
cleanup_test "$tc_session"

# Test 18: Exits 0 for TaskCreate with empty subject
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-empty-18"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"description":"some task"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 for missing subject"
else
    fail "Should exit 0 for missing subject"
fi
cleanup_test "$tc_session"

echo ""
echo "--- Execution Tests: Task Registration ---"

# Test 19: Registers PF1-TSK-01 in checkpoint
TESTS_RUN=$((TESTS_RUN + 1))
reg_session="ses-register-19"
create_test_flag "$reg_session"
create_test_env_file "$reg_session"
mkdir -p "$REPO_ROOT/.state/session/$reg_session/pathflow"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01: Initialize PathFlow"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_checkpoint_file "$reg_session"; then
    pass "Checkpoint file created after registration"
else
    fail "Checkpoint file should be created after task registration"
fi
TESTS_RUN=$((TESTS_RUN + 1))
ckpt=$(read_checkpoint "$reg_session")
if echo "$ckpt" | jq -e '.PF1.registered["PF1-TSK-01"]' >/dev/null 2>&1; then
    pass "PF1-TSK-01 registered in checkpoint"
else
    fail "PF1-TSK-01 should be registered in checkpoint"
fi
cleanup_test "$reg_session"

# Test 20: Registers PF3-TSK-04 (multi-digit phase, with required sentinel)
TESTS_RUN=$((TESTS_RUN + 1))
reg_session="ses-register-20"
create_test_flag "$reg_session"
create_test_env_file "$reg_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$reg_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$reg_session/pathflow-pf-2"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF3-TSK-04: Register task in WorkGraph"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
ckpt=$(read_checkpoint "$reg_session")
if echo "$ckpt" | jq -e '.PF3.registered["PF3-TSK-04"]' >/dev/null 2>&1; then
    pass "PF3-TSK-04 registered in checkpoint"
else
    fail "PF3-TSK-04 should be registered in checkpoint"
fi
cleanup_test "$reg_session"

# Test 21: PF pattern embedded in longer subject text (with required sentinel)
TESTS_RUN=$((TESTS_RUN + 1))
reg_session="ses-embedded-21"
create_test_flag "$reg_session"
create_test_env_file "$reg_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$reg_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$reg_session/pathflow-pf-1"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"Complete PF2-TSK-03 context loading step"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
ckpt=$(read_checkpoint "$reg_session")
if echo "$ckpt" | jq -e '.PF2.registered["PF2-TSK-03"]' >/dev/null 2>&1; then
    pass "PF2-TSK-03 extracted from embedded subject"
else
    fail "Should extract PF task ID from embedded subject text"
fi
cleanup_test "$reg_session"

# Test 22: Idempotent registration (register twice, no error)
TESTS_RUN=$((TESTS_RUN + 1))
reg_session="ses-idempotent-22"
create_test_flag "$reg_session"
create_test_env_file "$reg_session"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01: Init"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Idempotent registration (no error on duplicate)"
else
    fail "Should handle duplicate registration gracefully"
fi
cleanup_test "$reg_session"

echo ""
echo "--- Execution Tests: Session ID Resolution ---"

# Test 23: Uses env file session ID (not stdin)
TESTS_RUN=$((TESTS_RUN + 1))
env_session="ses-envpriority-23"
stdin_session="uuid-stdin-23"
create_test_flag "$env_session"
create_test_env_file "$env_session"
stdin_json="{\"tool_name\":\"TaskCreate\",\"tool_input\":{\"subject\":\"PF1-TSK-01: Init\"},\"session_id\":\"$stdin_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_checkpoint_file "$env_session"; then
    pass "Checkpoint created under env file session ID"
else
    fail "Checkpoint should use env file session ID, not stdin"
fi
TESTS_RUN=$((TESTS_RUN + 1))
if ! has_checkpoint_file "$stdin_session"; then
    pass "No checkpoint under stdin session ID"
else
    fail "Should NOT create checkpoint under stdin session ID"
fi
cleanup_test "$env_session"
cleanup_test "$stdin_session"

# Test 24: Falls back to stdin session_id when env file missing
TESTS_RUN=$((TESTS_RUN + 1))
fb_session="ses-fallback-24"
create_test_flag "$fb_session"
remove_test_env_file
stdin_json="{\"tool_name\":\"TaskCreate\",\"tool_input\":{\"subject\":\"PF1-TSK-01: Init\"},\"session_id\":\"$fb_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_checkpoint_file "$fb_session"; then
    pass "Falls back to stdin session_id"
else
    fail "Should fall back to stdin session_id when env file missing"
fi
cleanup_test "$fb_session"

echo ""
echo "--- Execution Tests: Cross-Phase Registration Blocking ---"

# Test 25: PF1 tasks always register (no previous phase check)
TESTS_RUN=$((TESTS_RUN + 1))
cpb_session="ses-pf1noreg-25"
create_test_flag "$cpb_session"
create_test_env_file "$cpb_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01: Init PathFlow"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "PF1 tasks register without previous phase check"
else
    fail "PF1 tasks should always register (no previous phase, got exit $exit_code)"
fi
cleanup_test "$cpb_session"

# Test 26: PF2 tasks register when pf-1 sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
cpb_session="ses-pf2allow-26"
create_test_flag "$cpb_session"
create_test_env_file "$cpb_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session/pathflow-pf-1"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF2-TSK-01: Load context"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "PF2 tasks register when pf-1 sentinel exists"
else
    fail "PF2 tasks should register when pf-1 exists (got exit $exit_code)"
fi
cleanup_test "$cpb_session"

# Test 27: PF2 tasks BLOCKED (exit 2) when pf-1 sentinel missing
TESTS_RUN=$((TESTS_RUN + 1))
cpb_session="ses-pf2block-27"
create_test_flag "$cpb_session"
create_test_env_file "$cpb_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session"
# NO pf-1 sentinel
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF2-TSK-01: Load context"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "PF2 task registration BLOCKED when pf-1 sentinel missing (exit 2)"
else
    fail "Should exit 2 when pf-1 sentinel missing (got exit $exit_code)"
fi
cleanup_test "$cpb_session"

# Test 28: PF3 tasks BLOCKED when pf-2 sentinel missing
TESTS_RUN=$((TESTS_RUN + 1))
cpb_session="ses-pf3block-28"
create_test_flag "$cpb_session"
create_test_env_file "$cpb_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$cpb_session/pathflow-pf-1"
# pf-1 exists but NOT pf-2
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"PF3-TSK-01: Classify work"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "PF3 task registration BLOCKED when pf-2 sentinel missing (exit 2)"
else
    fail "Should exit 2 when pf-2 sentinel missing (got exit $exit_code)"
fi
cleanup_test "$cpb_session"

# Test 29: Non-PF tasks pass through unblocked
TESTS_RUN=$((TESTS_RUN + 1))
cpb_session="ses-nonpf-29"
create_test_flag "$cpb_session"
create_test_env_file "$cpb_session"
stdin_json='{"tool_name":"TaskCreate","tool_input":{"subject":"Fix the authentication bug"},"session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Non-PF tasks pass through unblocked"
else
    fail "Non-PF tasks should pass through (got exit $exit_code)"
fi
cleanup_test "$cpb_session"

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
