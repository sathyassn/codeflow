#!/usr/bin/env bash
# Test: cf-task-completed-phase-checkpoint.sh
# Location: .codeflow/testing/claude-hooks/task-completed/test-cf-task-completed-phase-checkpoint.sh
#
# Tests TaskCompleted hook for phase checkpoint completion (Layer 2):
#   - File exists, executable, shellcheck, headers, strict mode
#   - Parses task_subject for PF{N}-TSK-{NN} pattern
#   - Marks matching tasks completed in checkpoint
#   - Creates phase sentinel when ALL required tasks complete
#   - Cross-phase dependency check (blocks if prev phase sentinel missing)
#   - Ignores non-PF task completions silently
#   - Handles conditional/skipped tasks
#
# Exit codes:
#   0 - All tests passed
#   1 - One or more tests failed

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/task-completed/cf-task-completed-phase-checkpoint.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-task-completed-phase-checkpoint.sh ==="
echo ""

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

create_test_flag() {
    local session_id="$1"
    local session_dir="$REPO_ROOT/.state/session/$session_id"
    mkdir -p "$session_dir"
    echo "{\"session_id\":\"$session_id\"}" > "$session_dir/is-pathflow-active"
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

has_test_sentinel() {
    local session_id="$1"
    local sentinel_name="$2"
    [[ -f "$REPO_ROOT/.state/sentinels/pathflow/$session_id/pathflow-$sentinel_name" ]]
}

read_checkpoint() {
    local session_id="$1"
    local ckpt="$REPO_ROOT/.state/checkpoints/pathflow/$session_id/phase-tasks.json"
    if [[ -f "$ckpt" ]]; then
        cat "$ckpt"
    else
        echo "{}"
    fi
}

# Pre-populate a checkpoint with phase initialized + tasks registered
# Args: session_id phase_id task_ids...
setup_checkpoint() {
    local session_id="$1"
    local phase_id="$2"
    shift 2
    local task_ids=("$@")

    local ckpt_dir="$REPO_ROOT/.state/checkpoints/pathflow/$session_id"
    mkdir -p "$ckpt_dir"

    # Build expected array and registered map
    local expected_json registered_json
    expected_json=$(printf '%s\n' "${task_ids[@]}" | jq -R -s -c 'split("\n") | map(select(. != ""))')
    registered_json="{}"
    for tid in "${task_ids[@]}"; do
        registered_json=$(echo "$registered_json" | jq -c --arg tid "$tid" '.[$tid] = "2026-01-01T00:00:00Z"')
    done

    jq -n --arg pf "$phase_id" --argjson exp "$expected_json" --argjson reg "$registered_json" \
        '{($pf): {expected: $exp, registered: $reg, completed: {}, skipped: {}, sentinel_created: false}}' \
        > "$ckpt_dir/phase-tasks.json"
}

cleanup_test() {
    local session_id="$1"
    rm -rf "$REPO_ROOT/.state/checkpoints/pathflow/$session_id" 2>/dev/null || true
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

# Test 8: Sources pathflow state library
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-pathflow-state" "$HOOK"; then
    pass "Sources pathflow state library"
else
    fail "Should source cf-pathflow-state.sh"
fi

# Test 9: Uses checkpoint_complete_task function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "checkpoint_complete_task" "$HOOK"; then
    pass "Uses checkpoint_complete_task function"
else
    fail "Should use checkpoint_complete_task function"
fi

# Test 10: Uses create_sentinel function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_sentinel" "$HOOK"; then
    pass "Uses create_sentinel function"
else
    fail "Should use create_sentinel function"
fi

# Test 11: Parses task_subject from stdin JSON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "task_subject" "$HOOK"; then
    pass "Parses task_subject from stdin"
else
    fail "Should parse task_subject from stdin JSON"
fi

# Test 12: Has cross-phase dependency check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "has_sentinel" "$HOOK" && grep -q "exit 2" "$HOOK"; then
    pass "Has cross-phase dependency check with exit 2"
else
    fail "Should check cross-phase dependency and exit 2 on violation"
fi

# Test 13: References env file (codeflow-env.sh)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "codeflow-env.sh" "$HOOK"; then
    pass "References env file for session ID"
else
    fail "Should reference codeflow-env.sh for session ID"
fi

echo ""
echo "--- Execution Tests: Early Exit ---"

# Test 14: Exits 0 for non-PF task subject
TESTS_RUN=$((TESTS_RUN + 1))
stdin_json='{"task_subject":"Fix the login bug","task_id":"123","session_id":"ses-nonpf-14"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 for non-PF task subject"
else
    fail "Should exit 0 for non-PF task subject"
fi

# Test 15: Exits 0 for empty task subject
TESTS_RUN=$((TESTS_RUN + 1))
stdin_json='{"task_subject":"","task_id":"123","session_id":"ses-empty-15"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 for empty task subject"
else
    fail "Should exit 0 for empty task subject"
fi

# Test 16: Exits 0 when pathflow not active
TESTS_RUN=$((TESTS_RUN + 1))
remove_test_env_file
stdin_json='{"task_subject":"PF1-TSK-01: Init","task_id":"123","session_id":"ses-inactive-16"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Exits 0 when pathflow not active"
else
    fail "Should exit 0 when pathflow not active"
fi

echo ""
echo "--- Execution Tests: Task Completion ---"

# Test 17: Marks PF1-TSK-01 completed in checkpoint
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-complete-17"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# PF1 has no previous phase sentinel required
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01" "PF1-TSK-02"
stdin_json='{"task_subject":"PF1-TSK-01: Init session","task_id":"1","session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
ckpt=$(read_checkpoint "$tc_session")
if echo "$ckpt" | jq -e '.PF1.completed["PF1-TSK-01"]' >/dev/null 2>&1; then
    pass "PF1-TSK-01 marked completed in checkpoint"
else
    fail "PF1-TSK-01 should be marked completed"
fi
cleanup_test "$tc_session"

# Test 18: Creates sentinel when ALL phase tasks complete
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-sentinel-18"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# Set up PF1 with 2 tasks, complete the first manually
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01" "PF1-TSK-02"
ckpt_file="$REPO_ROOT/.state/checkpoints/pathflow/$tc_session/phase-tasks.json"
jq '.PF1.completed["PF1-TSK-01"] = "2026-01-01T00:00:00Z"' "$ckpt_file" > "${ckpt_file}.tmp" && mv "${ckpt_file}.tmp" "$ckpt_file"
# Now complete the second task via hook — should trigger sentinel
stdin_json='{"task_subject":"PF1-TSK-02: Spawn security","task_id":"2","session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$tc_session" "pf-1"; then
    pass "pf-1 sentinel created when all PF1 tasks complete"
else
    fail "Should create pf-1 sentinel when all PF1 tasks are done"
fi
cleanup_test "$tc_session"

# Test 19: Does NOT create sentinel when tasks remain
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-partial-19"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# PF1 with 2 tasks, complete only 1
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01" "PF1-TSK-02"
stdin_json='{"task_subject":"PF1-TSK-01: Init","task_id":"1","session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if ! has_test_sentinel "$tc_session" "pf-1"; then
    pass "No sentinel when tasks remain"
else
    fail "Should NOT create sentinel when tasks remain incomplete"
fi
cleanup_test "$tc_session"

# Test 20: Idempotent completion (complete same task twice)
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-idempotent-20"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01"
stdin_json='{"task_subject":"PF1-TSK-01: Init","task_id":"1","session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Idempotent completion (no error on duplicate)"
else
    fail "Should handle duplicate completion gracefully"
fi
cleanup_test "$tc_session"

echo ""
echo "--- Execution Tests: Cross-Phase Dependencies ---"

# Test 21: Blocks PF2 task when pf-1 sentinel missing (exit 2)
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-depblock-21"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# NO pf-1 sentinel created
setup_checkpoint "$tc_session" "PF2" "PF2-TSK-01"
stdin_json='{"task_subject":"PF2-TSK-01: Spawn knowledge layer","task_id":"1","session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "Blocks PF2 completion when pf-1 sentinel missing (exit 2)"
else
    fail "Should exit 2 when previous phase sentinel missing (got exit $exit_code)"
fi
cleanup_test "$tc_session"

# Test 22: Allows PF2 task when pf-1 sentinel exists
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-depallow-22"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# Create pf-1 sentinel
touch "$REPO_ROOT/.state/sentinels/pathflow/$tc_session/pathflow-pf-1"
setup_checkpoint "$tc_session" "PF2" "PF2-TSK-01"
stdin_json='{"task_subject":"PF2-TSK-01: Spawn knowledge layer","task_id":"1","session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "Allows PF2 task when pf-1 sentinel exists"
else
    fail "Should allow PF2 when pf-1 sentinel exists (got exit $exit_code)"
fi
cleanup_test "$tc_session"

# Test 23: PF1 tasks have no dependency check (first phase)
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-pf1nodep-23"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01"
stdin_json='{"task_subject":"PF1-TSK-01: Init","task_id":"1","session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 0 ]]; then
    pass "PF1 tasks have no dependency (no previous phase)"
else
    fail "PF1 should never be blocked (no previous phase)"
fi
cleanup_test "$tc_session"

# Test 24: Blocks PF3 when pf-2 sentinel missing
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-pf3block-24"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# Create pf-1 but NOT pf-2
touch "$REPO_ROOT/.state/sentinels/pathflow/$tc_session/pathflow-pf-1"
setup_checkpoint "$tc_session" "PF3" "PF3-TSK-01"
stdin_json='{"task_subject":"PF3-TSK-01: Classify work","task_id":"1","session_id":"ignored"}'
exit_code=0
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "Blocks PF3 when pf-2 sentinel missing"
else
    fail "Should block PF3 when pf-2 missing (got exit $exit_code)"
fi
cleanup_test "$tc_session"

echo ""
echo "--- Execution Tests: Session ID Resolution ---"

# Test 25: Uses env file session ID
TESTS_RUN=$((TESTS_RUN + 1))
env_session="ses-envid-25"
stdin_session="uuid-stdin-25"
create_test_flag "$env_session"
create_test_env_file "$env_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$env_session"
setup_checkpoint "$env_session" "PF1" "PF1-TSK-01"
stdin_json="{\"task_subject\":\"PF1-TSK-01: Init\",\"task_id\":\"1\",\"session_id\":\"$stdin_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
ckpt=$(read_checkpoint "$env_session")
if echo "$ckpt" | jq -e '.PF1.completed["PF1-TSK-01"]' >/dev/null 2>&1; then
    pass "Completion recorded under env file session ID"
else
    fail "Should use env file session ID for checkpoint"
fi
cleanup_test "$env_session"

# Test 26: Sentinel creation marks sentinel_created in checkpoint
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-sentflag-26"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"
# Single-task phase — completing it should create sentinel + mark flag
setup_checkpoint "$tc_session" "PF1" "PF1-TSK-01"
stdin_json='{"task_subject":"PF1-TSK-01: Init","task_id":"1","session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
ckpt=$(read_checkpoint "$tc_session")
sentinel_flag=$(echo "$ckpt" | jq -r '.PF1.sentinel_created' 2>/dev/null)
if [[ "$sentinel_flag" == "true" ]]; then
    pass "sentinel_created flag set to true in checkpoint"
else
    fail "Should set sentinel_created=true after sentinel creation (got: $sentinel_flag)"
fi
cleanup_test "$tc_session"

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
