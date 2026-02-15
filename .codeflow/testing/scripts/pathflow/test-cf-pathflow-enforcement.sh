#!/usr/bin/env bash
# test-cf-pathflow-enforcement.sh - Tests for PathFlow enforcement hooks
# Location: .codeflow/testing/scripts/pathflow/test-cf-pathflow-enforcement.sh
#
# Tests 3 hooks:
#   - cf-session-start-init.sh (flag creation)
#   - cf-post-tool-use-pathflow-sentinel.sh (sentinel detection)
#   - cf-pre-tool-use-pathflow-gate.sh (gate enforcement)
#
# Usage:
#   ./test-cf-pathflow-enforcement.sh       Run all tests
#   ./test-cf-pathflow-enforcement.sh -h    Show help

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR

usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]
Tests for PathFlow enforcement hooks.
Options:
    -h, --help      Show this help message
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Source test isolation
# shellcheck disable=SC2034
TEST_DIR="$SCRIPT_DIR"
source "$TESTING_DIR/lib/test-isolation.sh"

# Hook paths: prefer staging (new hooks), fall back to REAL_REPO_ROOT (installed)
STAGING_DIR="/tmp/claude/staging/hooks"

# Session start init hook (new, may be in staging)
if [[ -f "$STAGING_DIR/session-start/cf-session-start-init.sh" ]]; then
    INIT_HOOK="$STAGING_DIR/session-start/cf-session-start-init.sh"
else
    INIT_HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-init.sh"
fi

# Sentinel hook (new, may be in staging)
if [[ -f "$STAGING_DIR/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh" ]]; then
    SENTINEL_HOOK="$STAGING_DIR/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh"
else
    SENTINEL_HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh"
fi

# Gate hook (modified, may be in staging)
if [[ -f "$STAGING_DIR/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh" ]]; then
    GATE_HOOK="$STAGING_DIR/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh"
else
    GATE_HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh"
fi

# ============================================================================
# SETUP
# ============================================================================

TEST_SESSION_ID="ses-test-enf-$$"
export CODEFLOW_SESSION_ID="$TEST_SESSION_ID"

setup_env() {
    mkdir -p "$REPO_ROOT/.state/session/$TEST_SESSION_ID"
    mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"
    rm -f "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"/pathflow-* 2>/dev/null || true
}

# Create pathflow-active flag for tests that need it
create_flag() {
    touch "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
}

# Create sentinel for tests
create_test_sentinel() {
    touch "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID/pathflow-$1"
}

SDIR="$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"

# ============================================================================
# TESTS: SESSION START HOOK (flag creation)
# ============================================================================

test_init_hook_exists() {
    test_section "Init hook exists"
    if [[ -f "$INIT_HOOK" ]]; then
        test_pass "cf-session-start-init.sh exists"
    else
        test_skip "init_hook" "Hook not yet installed (in staging)"
    fi
}

test_init_hook_creates_flag() {
    test_section "Init hook creates flag"
    setup_env
    if [[ ! -f "$INIT_HOOK" ]]; then
        test_skip "init_creates_flag" "Hook not yet installed"
        return
    fi
    echo "{\"session_id\":\"$TEST_SESSION_ID\"}" | bash "$INIT_HOOK" 2>/dev/null
    if [[ -f "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active" ]]; then
        test_pass "Flag file created by init hook"
    else
        test_fail "Flag file not created by init hook"
    fi
}

test_init_hook_flag_json() {
    test_section "Init hook flag has valid JSON"
    setup_env
    if [[ ! -f "$INIT_HOOK" ]]; then
        test_skip "init_flag_json" "Hook not yet installed"
        return
    fi
    if ! command -v jq &>/dev/null; then
        test_skip "init_flag_json" "jq not installed"
        return
    fi
    echo "{\"session_id\":\"$TEST_SESSION_ID\"}" | bash "$INIT_HOOK" 2>/dev/null
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    if jq -e '.' "$flag_file" >/dev/null 2>&1; then
        test_pass "Flag contains valid JSON"
    else
        test_fail "Flag does not contain valid JSON"
    fi
    local tl
    tl=$(jq -r '.tracking_level' "$flag_file" 2>/dev/null)
    if [[ "$tl" == "pending" ]]; then
        test_pass "tracking_level is pending"
    else
        test_fail "tracking_level expected pending, got: $tl"
    fi
}

test_init_hook_graceful_no_lib() {
    test_section "Init hook graceful without library"
    setup_env
    # Remove the state library temporarily
    local lib_path="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
    local backup_path="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh.bak"
    if [[ -f "$lib_path" ]]; then
        mv "$lib_path" "$backup_path"
    fi
    if [[ ! -f "$INIT_HOOK" ]]; then
        test_skip "init_no_lib" "Hook not yet installed"
        [[ -f "$backup_path" ]] && mv "$backup_path" "$lib_path"
        return
    fi
    local exit_code=0
    echo "{\"session_id\":\"$TEST_SESSION_ID\"}" | bash "$INIT_HOOK" 2>/dev/null || exit_code=$?
    if [[ $exit_code -eq 0 ]]; then
        test_pass "Exits 0 without library (graceful)"
    else
        test_fail "Should exit 0 without library, got $exit_code"
    fi
    [[ -f "$backup_path" ]] && mv "$backup_path" "$lib_path"
}

# ============================================================================
# TESTS: SENTINEL HOOK (PostToolUse detection)
# ============================================================================

test_sentinel_hook_exists() {
    test_section "Sentinel hook exists"
    if [[ -f "$SENTINEL_HOOK" ]]; then
        test_pass "cf-post-tool-use-pathflow-sentinel.sh exists"
    else
        test_skip "sentinel_hook" "Hook not yet installed (in staging)"
    fi
}

run_sentinel_hook() {
    local stdin_json="$1"
    echo "$stdin_json" | bash "$SENTINEL_HOOK" 2>/dev/null
}

test_sentinel_teamcreate() {
    test_section "Sentinel: TeamCreate → pf-1"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_tc" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"TeamCreate","tool_input":{"team_name":"test"}}'
    if [[ -f "$SDIR/pathflow-pf-1" ]]; then
        test_pass "Created pathflow-pf-1"
    else
        test_fail "pathflow-pf-1 not created"
    fi
}

test_sentinel_knowledge_layer() {
    test_section "Sentinel: Task(cf-knowledge-layer) → pf-2"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_kl" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Task","tool_input":{"name":"cf-knowledge-layer","prompt":"test"}}'
    if [[ -f "$SDIR/pathflow-pf-2" ]]; then
        test_pass "Created pathflow-pf-2"
    else
        test_fail "pathflow-pf-2 not created"
    fi
}

test_sentinel_task_other() {
    test_section "Sentinel: Task(cf-security) → no pf-2"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_other" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Task","tool_input":{"name":"cf-security","prompt":"test"}}'
    if [[ ! -f "$SDIR/pathflow-pf-2" ]]; then
        test_pass "No pf-2 for non-knowledge-layer task"
    else
        test_fail "Should not create pf-2 for cf-security"
    fi
}

test_sentinel_git_checkout() {
    test_section "Sentinel: git checkout -b → pf-3"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_checkout" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Bash","tool_input":{"command":"git checkout -b feat/test-branch"}}'
    if [[ -f "$SDIR/pathflow-pf-3" ]]; then
        test_pass "Created pathflow-pf-3"
    else
        test_fail "pathflow-pf-3 not created"
    fi
}

test_sentinel_git_switch() {
    test_section "Sentinel: git switch -c → pf-3"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_switch" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Bash","tool_input":{"command":"git switch -c feat/test2"}}'
    if [[ -f "$SDIR/pathflow-pf-3" ]]; then
        test_pass "Created pathflow-pf-3 (switch -c)"
    else
        test_fail "pathflow-pf-3 not created for switch -c"
    fi
}

test_sentinel_stage_complete_dev() {
    test_section "Sentinel: STAGE-COMPLETE WS-DEV → ws-dev"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_dev" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}'
    if [[ -f "$SDIR/pathflow-ws-dev" ]]; then
        test_pass "Created pathflow-ws-dev"
    else
        test_fail "pathflow-ws-dev not created"
    fi
}

test_sentinel_stage_complete_rev() {
    test_section "Sentinel: STAGE-COMPLETE WS-REV → ws-rev"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_rev" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"All reviewed. STAGE-COMPLETE: WS-REV"}}'
    if [[ -f "$SDIR/pathflow-ws-rev" ]]; then
        test_pass "Created pathflow-ws-rev"
    else
        test_fail "pathflow-ws-rev not created"
    fi
}

test_sentinel_stage_complete_qa() {
    test_section "Sentinel: STAGE-COMPLETE WS-QA → ws-qa"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_qa" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-QA"}}'
    if [[ -f "$SDIR/pathflow-ws-qa" ]]; then
        test_pass "Created pathflow-ws-qa"
    else
        test_fail "pathflow-ws-qa not created"
    fi
}

test_sentinel_stage_complete_plan() {
    test_section "Sentinel: STAGE-COMPLETE WS-PLAN → ws-plan"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_plan" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-PLAN"}}'
    if [[ -f "$SDIR/pathflow-ws-plan" ]]; then
        test_pass "Created pathflow-ws-plan"
    else
        test_fail "pathflow-ws-plan not created"
    fi
}

test_sentinel_stage_complete_docs() {
    test_section "Sentinel: STAGE-COMPLETE WS-DOCS → ws-docs"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_docs" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DOCS"}}'
    if [[ -f "$SDIR/pathflow-ws-docs" ]]; then
        test_pass "Created pathflow-ws-docs"
    else
        test_fail "pathflow-ws-docs not created"
    fi
}

test_sentinel_stage_complete_test() {
    test_section "Sentinel: STAGE-COMPLETE WS-TEST → ws-test"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_test" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-TEST"}}'
    if [[ -f "$SDIR/pathflow-ws-test" ]]; then
        test_pass "Created pathflow-ws-test"
    else
        test_fail "pathflow-ws-test not created"
    fi
}

test_sentinel_gh_pr() {
    test_section "Sentinel: gh pr create → pf-6"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_pr" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Bash","tool_input":{"command":"gh pr create --title test --body test"}}'
    if [[ -f "$SDIR/pathflow-pf-6" ]]; then
        test_pass "Created pathflow-pf-6"
    else
        test_fail "pathflow-pf-6 not created"
    fi
}

test_sentinel_non_matching_bash() {
    test_section "Sentinel: non-matching bash → no sentinel"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_nomatch" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}'
    local count
    count=$(find "$SDIR" -maxdepth 1 -name 'pathflow-*' -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$count" -eq 0 ]]; then
        test_pass "No sentinels for ls -la"
    else
        test_fail "Should create no sentinels for ls -la, got $count"
    fi
}

test_sentinel_early_exit_no_flag() {
    test_section "Sentinel: early exit when not active"
    setup_env
    # Do NOT create flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_early" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"TeamCreate","tool_input":{"team_name":"test"}}'
    if [[ ! -f "$SDIR/pathflow-pf-1" ]]; then
        test_pass "No sentinel when pathflow not active"
    else
        test_fail "Should not create sentinel when flag absent"
    fi
}

test_sentinel_idempotent() {
    test_section "Sentinel: idempotent creation"
    setup_env
    create_flag
    if [[ ! -f "$SENTINEL_HOOK" ]]; then
        test_skip "sentinel_idem" "Hook not yet installed"
        return
    fi
    run_sentinel_hook '{"tool_name":"TeamCreate","tool_input":{"team_name":"test"}}'
    run_sentinel_hook '{"tool_name":"TeamCreate","tool_input":{"team_name":"test2"}}'
    local count
    count=$(find "$SDIR" -maxdepth 1 -name 'pathflow-pf-1*' -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$count" -eq 1 ]]; then
        test_pass "Single sentinel after double TeamCreate"
    else
        test_fail "Expected 1 sentinel, got $count"
    fi
}

# ============================================================================
# TESTS: PATHFLOW GATE (PreToolUse enforcement)
# ============================================================================

test_gate_hook_exists() {
    test_section "Gate hook exists"
    if [[ -f "$GATE_HOOK" ]]; then
        test_pass "cf-pre-tool-use-pathflow-gate.sh exists"
    else
        test_skip "gate_hook" "Hook not yet installed (in staging)"
    fi
}

run_gate() {
    local stdin_json="$1"
    local exit_code=0
    echo "$stdin_json" | bash "$GATE_HOOK" 2>/dev/null || exit_code=$?
    echo "$exit_code"
}

test_gate_blocks_edit_no_pf3() {
    test_section "Gate: Edit blocked without pf-3"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_edit_block" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "Edit blocked (exit 2)"
    else
        test_fail "Edit should be blocked, got exit $code"
    fi
}

test_gate_blocks_write_no_pf3() {
    test_section "Gate: Write blocked without pf-3"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_write_block" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Write","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "Write blocked (exit 2)"
    else
        test_fail "Write should be blocked, got exit $code"
    fi
}

test_gate_blocks_commit_no_pf3() {
    test_section "Gate: git commit blocked without pf-3"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_commit_block" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git commit -m test"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "git commit blocked (exit 2)"
    else
        test_fail "git commit should be blocked, got exit $code"
    fi
}

test_gate_blocks_push_no_wsrev() {
    test_section "Gate: git push blocked without ws-rev"
    setup_env
    create_flag
    create_test_sentinel "pf-3"  # pf-3 exists but ws-rev doesn't
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_push_block" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git push origin feat/x"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "git push blocked (exit 2)"
    else
        test_fail "git push should be blocked, got exit $code"
    fi
}

test_gate_blocks_pr_no_wsrev() {
    test_section "Gate: gh pr blocked without ws-rev"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_pr_block" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"gh pr create --title test"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "gh pr blocked (exit 2)"
    else
        test_fail "gh pr should be blocked, got exit $code"
    fi
}

test_gate_allows_edit_with_pf3() {
    test_section "Gate: Edit allowed with pf-3"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_edit_allow" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Edit allowed (exit 0)"
    else
        test_fail "Edit should be allowed, got exit $code"
    fi
}

test_gate_allows_write_with_pf3() {
    test_section "Gate: Write allowed with pf-3"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_write_allow" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Write","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Write allowed (exit 0)"
    else
        test_fail "Write should be allowed, got exit $code"
    fi
}

test_gate_allows_commit_with_pf3() {
    test_section "Gate: git commit allowed with pf-3"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_commit_allow" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git commit -m test"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "git commit allowed (exit 0)"
    else
        test_fail "git commit should be allowed, got exit $code"
    fi
}

test_gate_allows_push_with_wsrev() {
    test_section "Gate: git push allowed with ws-rev"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    create_test_sentinel "ws-rev"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_push_allow" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git push origin feat/x"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "git push allowed (exit 0)"
    else
        test_fail "git push should be allowed, got exit $code"
    fi
}

test_gate_allows_pr_with_wsrev() {
    test_section "Gate: gh pr allowed with ws-rev"
    setup_env
    create_flag
    create_test_sentinel "pf-3"
    create_test_sentinel "ws-rev"
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_pr_allow" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"gh pr create --title test"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "gh pr allowed (exit 0)"
    else
        test_fail "gh pr should be allowed, got exit $code"
    fi
}

test_gate_allows_ls() {
    test_section "Gate: non-gated bash allowed"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_ls" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "ls -la allowed (exit 0)"
    else
        test_fail "ls should be allowed, got exit $code"
    fi
}

test_gate_allows_read() {
    test_section "Gate: Read tool always allowed"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_read" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Read","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Read allowed (exit 0)"
    else
        test_fail "Read should be allowed, got exit $code"
    fi
}

test_gate_allows_no_flag() {
    test_section "Gate: everything allowed when no flag"
    setup_env
    # No flag, no sentinels
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_noflag" "Hook not yet installed"
        return
    fi
    local code
    code=$(run_gate '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Edit allowed without flag (exit 0)"
    else
        test_fail "Edit should be allowed without flag, got exit $code"
    fi
}

test_gate_block_message_has_reason() {
    test_section "Gate: block message includes reason"
    setup_env
    create_flag
    if [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "gate_msg" "Hook not yet installed"
        return
    fi
    local stderr_output
    stderr_output=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}' | bash "$GATE_HOOK" 2>&1 >/dev/null) || true
    if echo "$stderr_output" | grep -q "BLOCKED"; then
        test_pass "Block message includes BLOCKED"
    else
        test_fail "Block message should include BLOCKED"
    fi
    if echo "$stderr_output" | grep -q "pf-3"; then
        test_pass "Block message mentions pf-3"
    else
        test_fail "Block message should mention pf-3"
    fi
}

# ============================================================================
# TESTS: Integration (full session lifecycle)
# ============================================================================

test_full_lifecycle() {
    test_section "Full lifecycle: init → sentinels → gate"
    setup_env

    # Check all hooks exist
    if [[ ! -f "$INIT_HOOK" ]] || [[ ! -f "$SENTINEL_HOOK" ]] || [[ ! -f "$GATE_HOOK" ]]; then
        test_skip "lifecycle" "One or more hooks not yet installed"
        return
    fi

    # Step 1: Session start creates flag
    echo "{\"session_id\":\"$TEST_SESSION_ID\"}" | bash "$INIT_HOOK" 2>/dev/null
    if [[ -f "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active" ]]; then
        test_pass "Lifecycle 1: Flag created"
    else
        test_fail "Lifecycle 1: Flag not created"
        return
    fi

    # Step 2: Edit blocked before pf-3
    local code
    code=$(run_gate '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "Lifecycle 2: Edit blocked (no pf-3)"
    else
        test_fail "Lifecycle 2: Edit should be blocked"
    fi

    # Step 3: Sentinel hook creates pf-3 on branch creation
    run_sentinel_hook '{"tool_name":"Bash","tool_input":{"command":"git checkout -b feat/test"}}'
    if [[ -f "$SDIR/pathflow-pf-3" ]]; then
        test_pass "Lifecycle 3: pf-3 sentinel created"
    else
        test_fail "Lifecycle 3: pf-3 not created"
    fi

    # Step 4: Edit now allowed
    code=$(run_gate '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Lifecycle 4: Edit allowed (pf-3 exists)"
    else
        test_fail "Lifecycle 4: Edit should be allowed"
    fi

    # Step 5: Push still blocked
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git push origin feat/x"}}')
    if [[ "$code" -eq 2 ]]; then
        test_pass "Lifecycle 5: Push blocked (no ws-rev)"
    else
        test_fail "Lifecycle 5: Push should be blocked"
    fi

    # Step 6: Stage completion creates ws-rev
    run_sentinel_hook '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-REV"}}'
    if [[ -f "$SDIR/pathflow-ws-rev" ]]; then
        test_pass "Lifecycle 6: ws-rev sentinel created"
    else
        test_fail "Lifecycle 6: ws-rev not created"
    fi

    # Step 7: Push now allowed
    code=$(run_gate '{"tool_name":"Bash","tool_input":{"command":"git push origin feat/x"}}')
    if [[ "$code" -eq 0 ]]; then
        test_pass "Lifecycle 7: Push allowed (ws-rev exists)"
    else
        test_fail "Lifecycle 7: Push should be allowed"
    fi
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: PathFlow Enforcement Hooks${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Session start hook
    test_init_hook_exists
    test_init_hook_creates_flag
    test_init_hook_flag_json
    test_init_hook_graceful_no_lib

    # Sentinel hook (PostToolUse)
    test_sentinel_hook_exists
    test_sentinel_teamcreate
    test_sentinel_knowledge_layer
    test_sentinel_task_other
    test_sentinel_git_checkout
    test_sentinel_git_switch
    test_sentinel_stage_complete_dev
    test_sentinel_stage_complete_rev
    test_sentinel_stage_complete_qa
    test_sentinel_stage_complete_plan
    test_sentinel_stage_complete_docs
    test_sentinel_stage_complete_test
    test_sentinel_gh_pr
    test_sentinel_non_matching_bash
    test_sentinel_early_exit_no_flag
    test_sentinel_idempotent

    # Gate hook (PreToolUse)
    test_gate_hook_exists
    test_gate_blocks_edit_no_pf3
    test_gate_blocks_write_no_pf3
    test_gate_blocks_commit_no_pf3
    test_gate_blocks_push_no_wsrev
    test_gate_blocks_pr_no_wsrev
    test_gate_allows_edit_with_pf3
    test_gate_allows_write_with_pf3
    test_gate_allows_commit_with_pf3
    test_gate_allows_push_with_wsrev
    test_gate_allows_pr_with_wsrev
    test_gate_allows_ls
    test_gate_allows_read
    test_gate_allows_no_flag
    test_gate_block_message_has_reason

    # Integration
    test_full_lifecycle

    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
