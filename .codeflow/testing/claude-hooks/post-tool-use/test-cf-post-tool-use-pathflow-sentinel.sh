#!/usr/bin/env bash
# Test: cf-post-tool-use-pathflow-sentinel.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-sentinel.sh
#
# Tests PathFlow sentinel creation hook:
#   - File exists, executable, shellcheck, headers, strict mode
#   - References env file (codeflow-env.sh) for session ID
#   - Env-file-first session ID resolution (env file takes priority over stdin)
#   - Fallback to stdin session_id when env file missing
#   - Sentinel creation for each phase/stage trigger
#
# Exit codes:
#   0 - All tests passed
#   1 - One or more tests failed

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-pathflow-sentinel.sh ==="
echo ""

# =============================================================================
# HELPER: Create pathflow-active flag and env file
# =============================================================================

# Create a pathflow-active flag for the given session ID
create_test_flag() {
    local session_id="$1"
    local session_dir="$REPO_ROOT/.state/session/$session_id"
    mkdir -p "$session_dir"
    echo "{\"session_id\":\"$session_id\"}" > "$session_dir/is-pathflow-active"
}

# Create a codeflow-env.sh with the given session ID
create_test_env_file() {
    local session_id="$1"
    local env_file="$REPO_ROOT/.state/runtime/codeflow-env.sh"
    mkdir -p "$(dirname "$env_file")"
    echo "export CODEFLOW_SESSION_ID='$session_id'" > "$env_file"
}

# Remove the env file
remove_test_env_file() {
    rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
}

# Check if sentinel exists for a session
has_test_sentinel() {
    local session_id="$1"
    local sentinel_name="$2"
    [[ -f "$REPO_ROOT/.state/sentinels/pathflow/$session_id/pathflow-$sentinel_name" ]]
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

# Test 10: Uses create_sentinel for sentinel creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_sentinel" "$HOOK"; then
    pass "Uses create_sentinel for sentinel creation"
else
    fail "Should use create_sentinel function"
fi

echo ""
echo "--- Session ID Resolution ---"

# Test 11: Hook references env file (codeflow-env.sh)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "codeflow-env.sh" "$HOOK"; then
    pass "Hook references env file (codeflow-env.sh)"
else
    fail "Hook should reference codeflow-env.sh for session ID"
fi

# Test 12: Hook has TODO(go-cli) comment near env file sourcing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TODO(go-cli)" "$HOOK"; then
    pass "Has TODO(go-cli) comment"
else
    fail "Should have TODO(go-cli) comment near session ID sourcing"
fi

# Test 13: Hook sources env file BEFORE using session ID
TESTS_RUN=$((TESTS_RUN + 1))
# Verify the env file sourcing appears before pathflow-state.sh sourcing
env_line=$(grep -n "codeflow-env.sh" "$HOOK" | head -1 | cut -d: -f1) || true
state_line=$(grep -n "cf-pathflow-state.sh" "$HOOK" | head -1 | cut -d: -f1) || true
if [[ -n "${env_line:-}" ]] && [[ -n "${state_line:-}" ]] && [[ "$env_line" -lt "$state_line" ]]; then
    pass "Env file sourced before pathflow state library"
else
    fail "Env file should be sourced before pathflow state library (env=${env_line:-missing}, state=${state_line:-missing})"
fi

# Test 14: Hook does NOT extract session_id from stdin in the input parsing block
TESTS_RUN=$((TESTS_RUN + 1))
# Check that the stdin parsing block (before SETUP) doesn't set CODEFLOW_SESSION_ID
# The pattern we DON'T want: session_id extraction mixed in with tool_name/tool_input parsing
# Get lines between "HOOK INPUT PARSING" and "SETUP" sections
input_block=$(sed -n '/HOOK INPUT PARSING/,/^# SETUP$/p' "$HOOK" 2>/dev/null) || true
if echo "$input_block" | grep -q "CODEFLOW_SESSION_ID"; then
    fail "Input parsing block should NOT set CODEFLOW_SESSION_ID (env file takes priority)"
else
    pass "Input parsing block does not set CODEFLOW_SESSION_ID directly"
fi

# Test 15: Hook explains why env file takes priority
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "per-agent\|wrong directory\|shared.*session\|env.*file.*prior" "$HOOK"; then
    pass "Documents why env file takes priority over stdin"
else
    fail "Should explain why env file takes priority"
fi

echo ""
echo "--- Execution Tests: Env File Session ID ---"

# Test 16: Uses env file session ID for sentinel creation (env file present)
TESTS_RUN=$((TESTS_RUN + 1))
env_session="ses-envtest16"
stdin_session="uuid-stdin-16"
create_test_flag "$env_session"
create_test_env_file "$env_session"
# Create sentinel dir for env session so we can verify sentinel lands there
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$env_session"
# Run hook with TeamCreate (triggers pf-1 sentinel) — stdin has DIFFERENT session_id
stdin_json="{\"tool_name\":\"TeamCreate\",\"tool_input\":{\"team_name\":\"test\"},\"session_id\":\"$stdin_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$env_session" "pf-1"; then
    pass "Sentinel created under env file session ID (not stdin UUID)"
else
    fail "Sentinel should be created under env file session ID '$env_session'"
fi
# Verify it was NOT created under the stdin session
if has_test_sentinel "$stdin_session" "pf-1"; then
    fail "Sentinel should NOT be under stdin session ID '$stdin_session'"
else
    pass "Sentinel correctly NOT under stdin session ID"
fi
TESTS_RUN=$((TESTS_RUN + 1))
remove_test_env_file

# Test 17: Falls back to stdin session_id when env file missing
TESTS_RUN=$((TESTS_RUN + 1))
fallback_session="uuid-fallback-17"
create_test_flag "$fallback_session"
remove_test_env_file
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$fallback_session"
stdin_json="{\"tool_name\":\"TeamCreate\",\"tool_input\":{\"team_name\":\"test\"},\"session_id\":\"$fallback_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$fallback_session" "pf-1"; then
    pass "Falls back to stdin session_id when env file missing"
else
    fail "Should fall back to stdin session_id for sentinel creation"
fi

echo ""
echo "--- Execution Tests: Sentinel Triggers ---"

# Test 18: TeamCreate triggers pf-1 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-teamcreate-18"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
stdin_json='{"tool_name":"TeamCreate","tool_input":{"team_name":"test"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$tc_session" "pf-1"; then
    pass "TeamCreate triggers pf-1 sentinel"
else
    fail "TeamCreate should create pf-1 sentinel"
fi
remove_test_env_file

# Test 19: Task with cf-knowledge-layer triggers pf-2 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
kl_session="ses-knowledgelayer-19"
create_test_flag "$kl_session"
create_test_env_file "$kl_session"
stdin_json='{"tool_name":"Task","tool_input":{"name":"cf-knowledge-layer","prompt":"test"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$kl_session" "pf-2"; then
    pass "Task(cf-knowledge-layer) triggers pf-2 sentinel"
else
    fail "Task with cf-knowledge-layer should create pf-2 sentinel"
fi
remove_test_env_file

# Test 20: Bash git checkout -b triggers pf-3 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
gc_session="ses-gitcheckout-20"
create_test_flag "$gc_session"
create_test_env_file "$gc_session"
stdin_json='{"tool_name":"Bash","tool_input":{"command":"git checkout -b feat/test-branch"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$gc_session" "pf-3"; then
    pass "git checkout -b triggers pf-3 sentinel"
else
    fail "git checkout -b should create pf-3 sentinel"
fi
remove_test_env_file

# Test 21: SendMessage STAGE-COMPLETE: WS-DEV triggers ws-dev sentinel
TESTS_RUN=$((TESTS_RUN + 1))
sm_session="ses-stagecomplete-21"
create_test_flag "$sm_session"
create_test_env_file "$sm_session"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV -- 3 files changed","type":"message"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$sm_session" "ws-dev"; then
    pass "STAGE-COMPLETE: WS-DEV triggers ws-dev sentinel"
else
    fail "STAGE-COMPLETE: WS-DEV should create ws-dev sentinel"
fi
remove_test_env_file

# Test 22: Bash gh pr create triggers pf-6 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
pr_session="ses-prcreate-22"
create_test_flag "$pr_session"
create_test_env_file "$pr_session"
stdin_json='{"tool_name":"Bash","tool_input":{"command":"gh pr create --title \"test\""},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$pr_session" "pf-6"; then
    pass "gh pr create triggers pf-6 sentinel"
else
    fail "gh pr create should create pf-6 sentinel"
fi
remove_test_env_file

# Test 23: Exits 0 when pathflow not active
TESTS_RUN=$((TESTS_RUN + 1))
# Don't create any pathflow flag — hook should early-exit
remove_test_env_file
result=$(CODEFLOW_SESSION_ID="ses-inactive-23" TOOL_NAME="TeamCreate" TOOL_INPUT='{"team_name":"test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when pathflow not active"
else
    fail "Should exit 0 when pathflow not active"
fi

# Test 24: SendMessage STAGE-COMPLETE: WS-REV triggers ws-rev sentinel
TESTS_RUN=$((TESTS_RUN + 1))
rev_session="ses-wsrev-24"
create_test_flag "$rev_session"
create_test_env_file "$rev_session"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV -- review approved","type":"message"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$rev_session" "ws-rev"; then
    pass "STAGE-COMPLETE: WS-REV triggers ws-rev sentinel"
else
    fail "STAGE-COMPLETE: WS-REV should create ws-rev sentinel"
fi
remove_test_env_file

# Test 25: git switch -c triggers pf-3 sentinel
TESTS_RUN=$((TESTS_RUN + 1))
sw_session="ses-gitswitch-25"
create_test_flag "$sw_session"
create_test_env_file "$sw_session"
stdin_json='{"tool_name":"Bash","tool_input":{"command":"git switch -c fix/my-bugfix"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$sw_session" "pf-3"; then
    pass "git switch -c triggers pf-3 sentinel"
else
    fail "git switch -c should create pf-3 sentinel"
fi
remove_test_env_file

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
