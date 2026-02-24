#!/usr/bin/env bash
# Test: cf-post-tool-use-pathflow-sentinel.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-sentinel.sh
#
# Tests PathFlow sentinel creation hook:
#   - File exists, executable, shellcheck, headers, strict mode
#   - References env file (codeflow-env.sh) for session ID
#   - Env-file-first session ID resolution (env file takes priority over stdin)
#   - Fallback to stdin session_id when env file missing
#   - Sentinel creation for stage triggers (STAGE-COMPLETE via SendMessage)
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
    mkdir -p "$session_dir/pathflow"
    echo "{\"session_id\":\"$session_id\"}" > "$session_dir/pathflow/is-pathflow-active"
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

get_team_file() {
    local session_id="$1"
    echo "$REPO_ROOT/.state/session/$session_id/pathflow/pathflow-team.json"
}

# Create a mock team config at ~/.claude/teams/{name}/config.json
create_mock_team_config() {
    local team_name="$1"
    local lead_uuid="${2:-test-lead-uuid}"
    local team_dir="${HOME}/.claude/teams/${team_name}"
    mkdir -p "$team_dir"
    echo "{\"leadSessionId\":\"$lead_uuid\",\"members\":[]}" > "$team_dir/config.json"
    echo "$team_dir"
}

cleanup_mock_team() {
    local team_name="$1"
    rm -rf "${HOME}/.claude/teams/${team_name}" 2>/dev/null || true
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
# Run hook with SendMessage STAGE-COMPLETE (triggers ws-dev sentinel) — stdin has DIFFERENT session_id
stdin_json="{\"tool_name\":\"SendMessage\",\"tool_input\":{\"content\":\"STAGE-COMPLETE: WS-DEV\",\"type\":\"message\"},\"session_id\":\"$stdin_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$env_session" "ws-dev"; then
    pass "Sentinel created under env file session ID (not stdin UUID)"
else
    fail "Sentinel should be created under env file session ID '$env_session'"
fi
# Verify it was NOT created under the stdin session
if has_test_sentinel "$stdin_session" "ws-dev"; then
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
stdin_json="{\"tool_name\":\"SendMessage\",\"tool_input\":{\"content\":\"STAGE-COMPLETE: WS-DEV\",\"type\":\"message\"},\"session_id\":\"$fallback_session\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$fallback_session" "ws-dev"; then
    pass "Falls back to stdin session_id when env file missing"
else
    fail "Should fall back to stdin session_id for sentinel creation"
fi

echo ""
echo "--- Execution Tests: Stage Sentinel Triggers ---"

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

# Test 22: (removed — phase trigger pf-6 via gh pr no longer exists)

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

# Test 24: SendMessage STAGE-COMPLETE: WS-REV triggers ws-rev sentinel (with primary stage)
TESTS_RUN=$((TESTS_RUN + 1))
rev_session="ses-wsrev-24"
create_test_flag "$rev_session"
create_test_env_file "$rev_session"
# Create a primary stage sentinel first (required by stage ordering validation)
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$rev_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$rev_session/pathflow-ws-dev"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV -- review approved","type":"message"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$rev_session" "ws-rev"; then
    pass "STAGE-COMPLETE: WS-REV triggers ws-rev sentinel"
else
    fail "STAGE-COMPLETE: WS-REV should create ws-rev sentinel"
fi
remove_test_env_file

# Test 25: (removed — phase trigger pf-3 via git switch no longer exists)

echo ""
echo "--- Static Analysis: pathflow-team.json Handlers ---"

# Test 26: Hook has TeamCreate handler
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'TOOL_NAME.*==.*"TeamCreate"' "$HOOK"; then
    pass "Hook has TeamCreate handler"
else
    fail "Hook should have TeamCreate handler"
fi

# Test 27: Hook has Task handler
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'TOOL_NAME.*==.*"Task"' "$HOOK"; then
    pass "Hook has Task handler"
else
    fail "Hook should have Task handler"
fi

# Test 28: TeamCreate handler writes pathflow-team.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'pathflow-team.json' "$HOOK"; then
    pass "Hook references pathflow-team.json"
else
    fail "Hook should reference pathflow-team.json"
fi

# Test 29: Uses PPID for lead PID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'PPID' "$HOOK"; then
    pass "Hook uses PPID for lead PID"
else
    fail "Hook should use PPID for lead PID"
fi

# Test 30: Uses atomic write (mktemp + mv)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mktemp.*team_file' "$HOOK" && grep -q 'mv.*_tmp_team' "$HOOK"; then
    pass "Uses atomic write (tmp + mv) for pathflow-team.json"
else
    fail "Should use atomic write for pathflow-team.json"
fi

# Test 31: Task handler checks for team_name in tool_input
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'team_name' "$HOOK"; then
    pass "Task handler checks team_name in tool_input"
else
    fail "Task handler should check team_name"
fi

echo ""
echo "--- Execution Tests: TeamCreate Handler ---"

# Test 32: TeamCreate creates pathflow-team.json with correct schema
TESTS_RUN=$((TESTS_RUN + 1))
tc_session="ses-teamcreate-32"
tc_team="test-team-32"
create_test_flag "$tc_session"
create_test_env_file "$tc_session"
create_mock_team_config "$tc_team" "uuid-lead-32"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$tc_session"

stdin_json="{\"tool_name\":\"TeamCreate\",\"tool_input\":{\"team_name\":\"$tc_team\"},\"session_id\":\"ignored\"}"
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true

team_file=$(get_team_file "$tc_session")
if [[ -f "$team_file" ]]; then
    pass "TeamCreate creates pathflow-team.json"
else
    fail "TeamCreate should create pathflow-team.json"
fi
cleanup_mock_team "$tc_team"
remove_test_env_file

# Test 33: pathflow-team.json has team_name field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    _tn=$(jq -r '.team_name // empty' "$team_file" 2>/dev/null) || true
    if [[ "$_tn" == "$tc_team" ]]; then
        pass "pathflow-team.json has correct team_name"
    else
        fail "pathflow-team.json team_name should be '$tc_team', got '$_tn'"
    fi
else
    fail "Cannot validate team_name (file missing or jq unavailable)"
fi

# Test 34: pathflow-team.json has lead_pid field (integer > 0)
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    _pid=$(jq -r '.lead_pid // 0' "$team_file" 2>/dev/null) || true
    if [[ "$_pid" -gt 0 ]]; then
        pass "pathflow-team.json has valid lead_pid ($_pid)"
    else
        fail "pathflow-team.json lead_pid should be > 0, got '$_pid'"
    fi
else
    fail "Cannot validate lead_pid"
fi

# Test 35: pathflow-team.json has codeflow_session_id field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    _sid=$(jq -r '.codeflow_session_id // empty' "$team_file" 2>/dev/null) || true
    if [[ "$_sid" == "$tc_session" ]]; then
        pass "pathflow-team.json has correct codeflow_session_id"
    else
        fail "pathflow-team.json codeflow_session_id should be '$tc_session', got '$_sid'"
    fi
else
    fail "Cannot validate codeflow_session_id"
fi

# Test 36: pathflow-team.json has teammate_spawned=false initially
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    _ts=$(jq -r '.teammate_spawned' "$team_file" 2>/dev/null) || true
    if [[ "$_ts" == "false" ]]; then
        pass "pathflow-team.json has teammate_spawned=false initially"
    else
        fail "pathflow-team.json teammate_spawned should be false, got '$_ts'"
    fi
else
    fail "Cannot validate teammate_spawned"
fi

# Test 37: pathflow-team.json has created_at timestamp
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    _ca=$(jq -r '.created_at // empty' "$team_file" 2>/dev/null) || true
    if [[ -n "$_ca" ]] && [[ "$_ca" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T ]]; then
        pass "pathflow-team.json has valid created_at timestamp"
    else
        fail "pathflow-team.json should have ISO 8601 created_at, got '$_ca'"
    fi
else
    fail "Cannot validate created_at"
fi

# Test 38: pathflow-team.json is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$team_file" ]] && command -v jq &>/dev/null; then
    if jq . "$team_file" &>/dev/null; then
        pass "pathflow-team.json is valid JSON"
    else
        fail "pathflow-team.json should be valid JSON"
    fi
else
    fail "Cannot validate JSON"
fi

echo ""
echo "--- Execution Tests: Task Handler ---"

# Test 39: Task with team_name sets teammate_spawned=true
TESTS_RUN=$((TESTS_RUN + 1))
task_session="ses-taskspawn-39"
task_team="test-team-39"
create_test_flag "$task_session"
create_test_env_file "$task_session"
create_mock_team_config "$task_team" "uuid-lead-39"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$task_session"

# First create the team file via TeamCreate
tc_json="{\"tool_name\":\"TeamCreate\",\"tool_input\":{\"team_name\":\"$task_team\"},\"session_id\":\"ignored\"}"
bash "$HOOK" <<< "$tc_json" 2>/dev/null || true

# Then trigger Task with team_name
task_json="{\"tool_name\":\"Task\",\"tool_input\":{\"team_name\":\"$task_team\",\"name\":\"cf-development\",\"prompt\":\"test\"},\"session_id\":\"ignored\"}"
bash "$HOOK" <<< "$task_json" 2>/dev/null || true

task_team_file=$(get_team_file "$task_session")
if [[ -f "$task_team_file" ]] && command -v jq &>/dev/null; then
    _ts=$(jq -r '.teammate_spawned' "$task_team_file" 2>/dev/null) || true
    if [[ "$_ts" == "true" ]]; then
        pass "Task with team_name sets teammate_spawned=true"
    else
        fail "Task handler should set teammate_spawned=true, got '$_ts'"
    fi
else
    fail "Cannot validate teammate_spawned after Task"
fi

# Test 40: Task with team_name records last_spawn_name
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$task_team_file" ]] && command -v jq &>/dev/null; then
    _name=$(jq -r '.last_spawn_name // empty' "$task_team_file" 2>/dev/null) || true
    if [[ "$_name" == "cf-development" ]]; then
        pass "Task handler records last_spawn_name=cf-development"
    else
        fail "Task handler should record last_spawn_name=cf-development, got '$_name'"
    fi
else
    fail "Cannot validate last_spawn_name"
fi
cleanup_mock_team "$task_team"
remove_test_env_file

# Test 41: Task without team_name does NOT modify pathflow-team.json
TESTS_RUN=$((TESTS_RUN + 1))
notask_session="ses-notask-41"
create_test_flag "$notask_session"
create_test_env_file "$notask_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$notask_session"

# No pathflow-team.json exists — Task without team_name should be a no-op
task_json='{"tool_name":"Task","tool_input":{"prompt":"test","subagent_type":"Explore"},"session_id":"ignored"}'
bash "$HOOK" <<< "$task_json" 2>/dev/null || true

notask_file=$(get_team_file "$notask_session")
if [[ ! -f "$notask_file" ]]; then
    pass "Task without team_name does not create pathflow-team.json"
else
    fail "Task without team_name should not create pathflow-team.json"
fi
remove_test_env_file

echo ""
echo "--- Edge Cases: pathflow-team.json ---"

# Test 42: TeamCreate with missing pathflow directory is a no-op
TESTS_RUN=$((TESTS_RUN + 1))
edge_session="ses-nodir-42"
# Do NOT create pathflow directory
create_test_env_file "$edge_session"
# But still create sentinel dir for pathflow check
mkdir -p "$REPO_ROOT/.state/session/$edge_session"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$edge_session"
echo "{\"session_id\":\"$edge_session\"}" > "$REPO_ROOT/.state/session/$edge_session/pathflow-active-mock"
# Actually create the flag so pathflow is active
mkdir -p "$REPO_ROOT/.state/session/$edge_session/pathflow"
echo "{\"session_id\":\"$edge_session\"}" > "$REPO_ROOT/.state/session/$edge_session/pathflow/is-pathflow-active"

edge_team="test-team-42"
create_mock_team_config "$edge_team"
stdin_json="{\"tool_name\":\"TeamCreate\",\"tool_input\":{\"team_name\":\"$edge_team\"},\"session_id\":\"ignored\"}"
result=$(bash "$HOOK" <<< "$stdin_json" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "TeamCreate exits 0 even when setup is incomplete"
else
    fail "TeamCreate should exit 0 gracefully"
fi
cleanup_mock_team "$edge_team"
remove_test_env_file

# Test 43: Hook exits 0 when pathflow not active (no pathflow-team.json changes)
TESTS_RUN=$((TESTS_RUN + 1))
remove_test_env_file
result=$(CODEFLOW_SESSION_ID="ses-inactive-43" TOOL_NAME="TeamCreate" TOOL_INPUT='{"team_name":"test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when pathflow not active (team handler)"
else
    fail "Should exit 0 when pathflow not active (team handler)"
fi

echo ""
echo "--- Execution Tests: Stage Ordering Validation ---"

# Test 44: ws-rev BLOCKED when no primary stage exists (hard gate)
TESTS_RUN=$((TESTS_RUN + 1))
rev_no_primary_session="ses-revnoprim-44"
create_test_flag "$rev_no_primary_session"
create_test_env_file "$rev_no_primary_session"
# Do NOT create any primary stage sentinel (ws-dev, ws-plan, ws-docs, ws-test)
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV -- review approved","type":"message"},"session_id":"ignored"}'
output=$(bash "$HOOK" <<< "$stdin_json" 2>&1) && exit_code=0 || exit_code=$?
# Sentinel should NOT be created (hard gate blocks before creation)
if ! has_test_sentinel "$rev_no_primary_session" "ws-rev"; then
    pass "ws-rev sentinel NOT created when no primary stage (hard block)"
else
    fail "ws-rev sentinel should NOT be created without primary stage (hard block)"
fi
# BLOCKED message should be logged and exit 2
TESTS_RUN=$((TESTS_RUN + 1))
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"*"ws-rev requires prior primary stage"* ]]; then
    pass "ws-rev blocked with exit 2 when no primary stage exists"
else
    fail "Should block ws-rev with exit 2 when no primary stage (exit=$exit_code, output: $output)"
fi
remove_test_env_file

# Test 46: ws-rev sentinel ALLOWED when ws-plan exists (ordering satisfied)
TESTS_RUN=$((TESTS_RUN + 1))
rev_with_plan_session="ses-revplan-46"
create_test_flag "$rev_with_plan_session"
create_test_env_file "$rev_with_plan_session"
# Create ws-plan sentinel first (the primary stage)
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$rev_with_plan_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$rev_with_plan_session/pathflow-ws-plan"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV -- review approved","type":"message"},"session_id":"ignored"}'
output=$(bash "$HOOK" <<< "$stdin_json" 2>&1) && exit_code=0 || exit_code=$?
if has_test_sentinel "$rev_with_plan_session" "ws-rev" && [[ $exit_code -eq 0 ]] && [[ "$output" != *"BLOCKED"* ]]; then
    pass "ws-rev created when ws-plan exists (ordering satisfied)"
else
    fail "Should create ws-rev when ws-plan exists (exit=$exit_code, output: $output)"
fi
remove_test_env_file

# Test 47: ws-qa BLOCKED when BOTH ws-dev AND ws-test missing (hard gate)
TESTS_RUN=$((TESTS_RUN + 1))
qa_no_dev_session="ses-qanodev-47"
create_test_flag "$qa_no_dev_session"
create_test_env_file "$qa_no_dev_session"
# Create ws-rev but NOT ws-dev or ws-test
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$qa_no_dev_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$qa_no_dev_session/pathflow-ws-rev"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA -- tests passed","type":"message"},"session_id":"ignored"}'
output=$(bash "$HOOK" <<< "$stdin_json" 2>&1) && exit_code=0 || exit_code=$?
# Sentinel should NOT be created (hard gate blocks before creation)
if ! has_test_sentinel "$qa_no_dev_session" "ws-qa"; then
    pass "ws-qa sentinel NOT created when ws-dev and ws-test missing (hard block)"
else
    fail "ws-qa sentinel should NOT be created without ws-dev or ws-test (hard block)"
fi
# BLOCKED message should be logged and exit 2
TESTS_RUN=$((TESTS_RUN + 1))
if [[ $exit_code -eq 2 ]] && [[ "$output" == *"BLOCKED"*"ws-qa requires prior ws-dev or ws-test"* ]]; then
    pass "ws-qa blocked with exit 2 when ws-dev and ws-test missing"
else
    fail "Should block ws-qa with exit 2 when ws-dev and ws-test missing (exit=$exit_code, output: $output)"
fi
remove_test_env_file

# Test 55: ws-dev sentinel created with no ordering check (primary stages are unchecked)
TESTS_RUN=$((TESTS_RUN + 1))
dev_no_prereq_session="ses-devnoprereq-55"
create_test_flag "$dev_no_prereq_session"
create_test_env_file "$dev_no_prereq_session"
# No prior sentinels — ws-dev should still be created (no ordering requirement)
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV -- implementation done","type":"message"},"session_id":"ignored"}'
output=$(bash "$HOOK" <<< "$stdin_json" 2>&1) && exit_code=0 || exit_code=$?
if has_test_sentinel "$dev_no_prereq_session" "ws-dev" && [[ $exit_code -eq 0 ]] && [[ "$output" != *"BLOCKED"* ]]; then
    pass "ws-dev created with no ordering check (primary stages unchecked)"
else
    fail "ws-dev should always be created (no ordering requirement, exit=$exit_code, output: $output)"
fi
remove_test_env_file

echo ""
echo "--- TeamDelete Flag Removal (Three-Layer Cleanup) ---"

# Test 49: PostToolUse TeamDelete handler removes pathflow-active flag
TESTS_RUN=$((TESTS_RUN + 1))
td_session="ses-teamdelete-49"
create_test_flag "$td_session"
create_test_env_file "$td_session"
flag_path="$REPO_ROOT/.state/session/$td_session/pathflow/is-pathflow-active"
# Verify flag exists before
if [[ ! -f "$flag_path" ]]; then
    fail "Flag should exist before PostToolUse TeamDelete"
else
    stdin_json='{"tool_name":"TeamDelete","tool_input":{},"session_id":"ignored"}'
    bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
    if [[ ! -f "$flag_path" ]]; then
        pass "PostToolUse TeamDelete removes pathflow-active flag"
    else
        fail "PostToolUse TeamDelete should remove pathflow-active flag"
    fi
fi
remove_test_env_file

# Test 50: PostToolUse TeamDelete graceful when flag already absent
TESTS_RUN=$((TESTS_RUN + 1))
td_absent_session="ses-tdabsent-50"
create_test_flag "$td_absent_session"
create_test_env_file "$td_absent_session"
rm -f "$REPO_ROOT/.state/session/$td_absent_session/pathflow/is-pathflow-active" 2>/dev/null || true
stdin_json='{"tool_name":"TeamDelete","tool_input":{},"session_id":"ignored"}'
result=$(bash "$HOOK" <<< "$stdin_json" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "PostToolUse TeamDelete exits 0 when flag already absent"
else
    fail "PostToolUse TeamDelete should exit 0 when flag already absent"
fi
remove_test_env_file

# Test 51: Hook has TeamDelete handler section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'TOOL_NAME.*==.*"TeamDelete"' "$HOOK"; then
    pass "Hook has TeamDelete handler"
else
    fail "Hook should have TeamDelete handler for flag removal"
fi

echo ""
echo "--- WS-QA Ordering with WS-TEST ---"

# Test 52: ws-qa ALLOWED when ws-test exists (TEST pipeline, ordering satisfied)
TESTS_RUN=$((TESTS_RUN + 1))
qa_test_session="ses-qatest-52"
create_test_flag "$qa_test_session"
create_test_env_file "$qa_test_session"
# Create ws-test sentinel (but NOT ws-dev) — simulates TEST pipeline
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$qa_test_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$qa_test_session/pathflow-ws-test"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA -- tests passed","type":"message"},"session_id":"ignored"}'
output=$(bash "$HOOK" <<< "$stdin_json" 2>&1) && exit_code=0 || exit_code=$?
if has_test_sentinel "$qa_test_session" "ws-qa" && [[ $exit_code -eq 0 ]] && [[ "$output" != *"BLOCKED"* ]]; then
    pass "ws-qa created when ws-test exists (TEST pipeline, ordering satisfied)"
else
    fail "ws-qa should be created when ws-test exists (exit=$exit_code, output: $output)"
fi
remove_test_env_file

echo ""
echo "--- Stage Pattern Normalization ---"

# Test 53: Lowercase stage-complete message creates sentinel via normalization
TESTS_RUN=$((TESTS_RUN + 1))
lower_session="ses-lower-53"
create_test_flag "$lower_session"
create_test_env_file "$lower_session"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"stage-complete: ws-dev -- done","type":"message"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$lower_session" "ws-dev"; then
    pass "Lowercase stage-complete creates sentinel via normalization"
else
    fail "Lowercase stage-complete should create sentinel via normalization"
fi
remove_test_env_file

# Test 54: Mixed case stage-complete message creates sentinel (with primary stage)
TESTS_RUN=$((TESTS_RUN + 1))
mixed_session="ses-mixed-54"
create_test_flag "$mixed_session"
create_test_env_file "$mixed_session"
# Create a primary stage sentinel first (required by stage ordering validation for ws-rev)
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$mixed_session"
touch "$REPO_ROOT/.state/sentinels/pathflow/$mixed_session/pathflow-ws-dev"
stdin_json='{"tool_name":"SendMessage","tool_input":{"content":"Stage-Complete: WS-Rev -- approved","type":"message"},"session_id":"ignored"}'
bash "$HOOK" <<< "$stdin_json" 2>/dev/null || true
if has_test_sentinel "$mixed_session" "ws-rev"; then
    pass "Mixed case Stage-Complete creates sentinel"
else
    fail "Mixed case Stage-Complete should create sentinel"
fi
remove_test_env_file

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
