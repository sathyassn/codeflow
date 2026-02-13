#!/usr/bin/env bash
# test-work-state.sh - Tests for state/cf-work-state.sh
# Location: .codeflow/testing/scripts/state/test-work-state.sh
#
# Usage:
#   ./test-work-state.sh       Run all tests
#   ./test-work-state.sh -h    Show help

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="1.0.0"
readonly SCRIPT_VERSION
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for state/cf-work-state.sh library.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Source test isolation
# shellcheck disable=SC2034  # TEST_DIR used by test-isolation.sh
TEST_DIR="$SCRIPT_DIR"
source "$TESTING_DIR/lib/test-isolation.sh"

LIBRARY_UNDER_TEST="$REAL_REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh"

# Source the library once (uses readonly vars, cannot re-source)
source "$LIBRARY_UNDER_TEST"

# ============================================================================
# SETUP
# ============================================================================

setup_work_state_env() {
    mkdir -p "$REPO_ROOT/.state/runtime"
    rm -f "$REPO_ROOT/.state/runtime/active-task.json" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/runtime/active-task.json.lock" 2>/dev/null || true
}

require_jq() {
    local test_name="$1"
    if ! command -v jq &>/dev/null; then
        test_skip "$test_name" "jq not installed"
        return 1
    fi
    return 0
}

# ============================================================================
# TESTS
# ============================================================================

test_library_exists() {
    test_section "Library exists"
    if [[ -f "$LIBRARY_UNDER_TEST" ]]; then
        test_pass "cf-work-state.sh exists"
    else
        test_fail "cf-work-state.sh not found"
    fi
}

test_source_guard() {
    test_section "Source guard"
    # Library was sourced at top level - just check the guard variable
    if [[ "${_CF_WORK_STATE_LIB_SOURCED:-}" == "1" ]]; then
        test_pass "Source guard variable set"
    else
        test_fail "Source guard variable not set"
    fi
}

test_get_active_task_file() {
    test_section "get_active_task_file"
    setup_work_state_env
    local path
    path=$(get_active_task_file)
    if [[ "$path" == *"active-task.json" ]]; then
        test_pass "Returns active-task.json path"
    else
        test_fail "Should return active-task.json path: $path"
    fi
    if [[ "$path" == *".state/runtime/"* ]]; then
        test_pass "Path includes .state/runtime/"
    else
        test_fail "Path should include .state/runtime/"
    fi
}

test_set_active_task() {
    test_section "set_active_task"
    setup_work_state_env
    require_jq "set_active_task" || return
    set_active_task "INF-TSK-TEST-001" "INF-EPC-TEST-001" "Test Task" "in_progress" "fix/test"
    local task_file
    task_file=$(get_active_task_file)
    if [[ -f "$task_file" ]]; then
        test_pass "Active task file created"
    else
        test_fail "Active task file not created"
        return
    fi
    local task_id
    task_id=$(jq -r ".task_id" "$task_file")
    if [[ "$task_id" == "INF-TSK-TEST-001" ]]; then
        test_pass "task_id matches"
    else
        test_fail "task_id should be INF-TSK-TEST-001, got: $task_id"
    fi
}

test_set_has_required_fields() {
    test_section "set_active_task has fields"
    setup_work_state_env
    require_jq "set_fields" || return
    set_active_task "TSK-001" "EPC-001" "My Task" "in_progress" "fix/test"
    local task_file
    task_file=$(get_active_task_file)
    local fields="task_id epic_id title status branch session_id created_at updated_at"
    for field in $fields; do
        if jq -e ".$field" "$task_file" >/dev/null 2>&1; then
            test_pass "Has field: $field"
        else
            test_fail "Missing field: $field"
        fi
    done
}

test_set_pathflow_fields() {
    test_section "set_active_task with PathFlow fields"
    setup_work_state_env
    require_jq "set_pathflow" || return
    set_active_task "TSK-PF" "EPC-PF" "PathFlow Test" "in_progress" "fix/pf" "review" "review-team"
    local task_file
    task_file=$(get_active_task_file)
    local stage
    stage=$(jq -r ".current_stage // empty" "$task_file")
    if [[ "$stage" == "review" ]]; then
        test_pass "current_stage field set"
    else
        test_fail "Expected current_stage=review, got: $stage"
    fi
    local team
    team=$(jq -r ".team_name // empty" "$task_file")
    if [[ "$team" == "review-team" ]]; then
        test_pass "team_name field set"
    else
        test_fail "Expected team_name=review-team, got: $team"
    fi
}

test_get_active_task() {
    test_section "get_active_task returns JSON"
    setup_work_state_env
    require_jq "get_active_task" || return
    set_active_task "TSK-003" "" "Read Test" "in_progress" ""
    local result
    result=$(get_active_task)
    if echo "$result" | jq -e "." >/dev/null 2>&1; then
        test_pass "Returns valid JSON"
    else
        test_fail "Should return valid JSON"
    fi
}

test_get_active_task_no_file() {
    test_section "get_active_task with no file"
    setup_work_state_env
    local result
    result=$(get_active_task)
    if [[ -z "$result" ]]; then
        test_pass "Returns empty when no file"
    else
        test_fail "Should return empty"
    fi
}

test_get_active_task_id() {
    test_section "get_active_task_id"
    setup_work_state_env
    require_jq "get_active_task_id" || return
    set_active_task "INF-TSK-ID-001" "" "ID Test" "in_progress" ""
    local result
    result=$(get_active_task_id)
    if [[ "$result" == "INF-TSK-ID-001" ]]; then
        test_pass "Returns correct ID"
    else
        test_fail "Expected INF-TSK-ID-001, got: $result"
    fi
}

test_update_status() {
    test_section "update_active_task_status"
    setup_work_state_env
    require_jq "update_status" || return
    set_active_task "TSK-UPD" "" "Update Test" "in_progress" ""
    update_active_task_status "complete"
    local task_file status
    task_file=$(get_active_task_file)
    status=$(jq -r ".status" "$task_file")
    if [[ "$status" == "complete" ]]; then
        test_pass "Status updated to complete"
    else
        test_fail "Expected complete, got: $status"
    fi
}

test_update_status_no_file() {
    test_section "update_active_task_status no file"
    setup_work_state_env
    if ! update_active_task_status "complete" 2>/dev/null; then
        test_pass "Returns error when no file"
    else
        test_fail "Should return error"
    fi
}

test_clear_active_task() {
    test_section "clear_active_task"
    setup_work_state_env
    require_jq "clear_active_task" || return
    set_active_task "TSK-CLR" "" "Clear Test" "in_progress" ""
    local task_file
    task_file=$(get_active_task_file)
    clear_active_task
    if [[ ! -f "$task_file" ]]; then
        test_pass "File removed"
    else
        test_fail "File should be removed"
    fi
}

test_is_task_active_true() {
    test_section "is_task_active when active"
    setup_work_state_env
    require_jq "is_task_active_true" || return
    set_active_task "TSK-ACT" "" "Active Test" "in_progress" ""
    if is_task_active; then
        test_pass "Returns true for in_progress"
    else
        test_fail "Should return true"
    fi
}

test_is_task_active_false() {
    test_section "is_task_active when no file"
    setup_work_state_env
    if ! is_task_active; then
        test_pass "Returns false when no file"
    else
        test_fail "Should return false"
    fi
}

test_is_task_active_complete() {
    test_section "is_task_active when complete"
    setup_work_state_env
    require_jq "is_task_active_complete" || return
    set_active_task "TSK-CMP" "" "Complete Test" "in_progress" ""
    update_active_task_status "complete"
    if ! is_task_active; then
        test_pass "Returns false for complete"
    else
        test_fail "Should return false for complete"
    fi
}

test_is_current_session() {
    test_section "is_current_session_task"
    setup_work_state_env
    require_jq "is_current_session" || return
    export CODEFLOW_SESSION_ID="test-session-123"
    set_active_task "TSK-SESS" "" "Session Test" "in_progress" ""
    if is_current_session_task "test-session-123"; then
        test_pass "Matches current session"
    else
        test_fail "Should match session"
    fi
    unset CODEFLOW_SESSION_ID
}

test_functions_available() {
    test_section "All functions available"
    setup_work_state_env
    local functions="get_active_task_file set_active_task get_active_task get_active_task_id update_active_task_status clear_active_task is_task_active is_current_session_task is_active_task_stale"
    for func in $functions; do
        if type "$func" >/dev/null 2>&1; then
            test_pass "Function: $func"
        else
            test_fail "Missing: $func"
        fi
    done
}

test_exported_variables() {
    test_section "Exported variables"
    setup_work_state_env
    if [[ -n "${ACTIVE_TASK_DIR:-}" ]]; then
        test_pass "ACTIVE_TASK_DIR set"
    else
        test_fail "ACTIVE_TASK_DIR not set"
    fi
    if [[ -n "${ACTIVE_TASK_FILE:-}" ]]; then
        test_pass "ACTIVE_TASK_FILE set"
    else
        test_fail "ACTIVE_TASK_FILE not set"
    fi
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) usage; exit 0 ;;
            -V|--version) echo "$SCRIPT_NAME version $SCRIPT_VERSION"; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: state/cf-work-state.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    test_library_exists
    test_source_guard
    test_get_active_task_file
    test_set_active_task
    test_set_has_required_fields
    test_set_pathflow_fields
    test_get_active_task
    test_get_active_task_no_file
    test_get_active_task_id
    test_update_status
    test_update_status_no_file
    test_clear_active_task
    test_is_task_active_true
    test_is_task_active_false
    test_is_task_active_complete
    test_is_current_session
    test_functions_available
    test_exported_variables

    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
