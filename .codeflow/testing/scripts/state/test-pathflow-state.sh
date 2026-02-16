#!/usr/bin/env bash
# test-pathflow-state.sh - Tests for state/cf-pathflow-state.sh
# Location: .codeflow/testing/scripts/state/test-pathflow-state.sh
#
# Tests the pathflow state library:
#   - Library exists, shellcheck, source guard
#   - create_pathflow_flag: JSON content, required fields, default values
#   - remove_pathflow_flag: removes file, idempotent on missing
#   - create_sentinel: creates file, empty name error, idempotent
#   - has_sentinel: exists/missing/empty-name checks
#   - list_sentinels: multiple sentinels, empty directory, no directory
#
# Usage:
#   ./test-pathflow-state.sh       Run all tests
#   ./test-pathflow-state.sh -h    Show help

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
Tests for state/cf-pathflow-state.sh library.
Options:
    -h, --help      Show this help message
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Source test isolation (provides REPO_ROOT in /tmp, REAL_REPO_ROOT for source files)
# shellcheck disable=SC2034
TEST_DIR="$SCRIPT_DIR"
source "$TESTING_DIR/lib/test-isolation.sh"

LIBRARY_UNDER_TEST="$REAL_REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"

# ============================================================================
# SETUP
# ============================================================================

TEST_SESSION_ID="ses-test-pfs-$$"
export CODEFLOW_SESSION_ID="$TEST_SESSION_ID"

setup_pfs_env() {
    # Reset source guard so library can be re-sourced in subshells
    unset _CF_PATHFLOW_STATE_LIB_SOURCED 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/session/$TEST_SESSION_ID"
    mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"
    rm -f "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"/pathflow-* 2>/dev/null || true
}

# ============================================================================
# TESTS: LIBRARY BASICS
# ============================================================================

test_library_exists() {
    test_section "Library exists"
    if [[ -f "$LIBRARY_UNDER_TEST" ]]; then
        test_pass "cf-pathflow-state.sh exists"
    else
        test_fail "cf-pathflow-state.sh not found at $LIBRARY_UNDER_TEST"
    fi
}

test_shellcheck() {
    test_section "ShellCheck compliance"
    if command -v shellcheck &>/dev/null; then
        if shellcheck -x -s bash "$LIBRARY_UNDER_TEST" 2>/dev/null; then
            test_pass "Passes shellcheck"
        else
            test_fail "Fails shellcheck"
        fi
    else
        test_skip "shellcheck" "shellcheck not installed"
    fi
}

test_has_source_guard() {
    test_section "Source guard"
    if grep -q "_CF_PATHFLOW_STATE_LIB_SOURCED" "$LIBRARY_UNDER_TEST"; then
        test_pass "Has source guard variable"
    else
        test_fail "Missing source guard"
    fi
}

test_has_set_euo() {
    test_section "Strict mode"
    if grep -q "set -euo pipefail" "$LIBRARY_UNDER_TEST"; then
        test_pass "Uses set -euo pipefail"
    else
        test_fail "Should use set -euo pipefail"
    fi
}

test_has_library_guard() {
    test_section "Library guard"
    if grep -q 'BASH_SOURCE\[0\]' "$LIBRARY_UNDER_TEST"; then
        test_pass "Has direct execution guard"
    else
        test_fail "Missing direct execution guard"
    fi
}

test_functions_available() {
    test_section "All functions available"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        for fn in create_pathflow_flag remove_pathflow_flag create_sentinel has_sentinel list_sentinels; do
            if type "$fn" >/dev/null 2>&1; then
                echo "OK:$fn"
            else
                echo "MISSING:$fn"
            fi
        done
    )
    for fn in create_pathflow_flag remove_pathflow_flag create_sentinel has_sentinel list_sentinels; do
        if echo "$result" | grep -q "OK:$fn"; then
            test_pass "Function: $fn"
        else
            test_fail "Missing: $fn"
        fi
    done
}

# ============================================================================
# TESTS: create_pathflow_flag
# ============================================================================

test_create_flag_file_exists() {
    test_section "create_pathflow_flag creates file"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" ""
        [[ -f "$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active" ]] && echo "EXISTS" || echo "MISSING"
    )
    if [[ "$result" == "EXISTS" ]]; then
        test_pass "Flag file created"
    else
        test_fail "Flag file not created"
    fi
}

test_create_flag_valid_json() {
    test_section "create_pathflow_flag writes valid JSON"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "flag_json" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" "my-team"
    )
    if jq -e '.' "$flag_file" >/dev/null 2>&1; then
        test_pass "Flag content is valid JSON"
    else
        test_fail "Flag content is not valid JSON"
    fi
}

test_create_flag_fields() {
    test_section "create_pathflow_flag has required fields"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "flag_fields" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" "test-team"
    )
    for field in session_id team_name created_at tracking_level; do
        if jq -e ".$field" "$flag_file" >/dev/null 2>&1; then
            test_pass "Has field: $field"
        else
            test_fail "Missing field: $field"
        fi
    done
}

test_create_flag_values() {
    test_section "create_pathflow_flag correct values"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "flag_values" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" "my-team"
    )
    local sid
    sid=$(jq -r '.session_id' "$flag_file")
    if [[ "$sid" == "$TEST_SESSION_ID" ]]; then
        test_pass "session_id matches"
    else
        test_fail "session_id: expected $TEST_SESSION_ID, got $sid"
    fi
    local tn
    tn=$(jq -r '.team_name' "$flag_file")
    if [[ "$tn" == "my-team" ]]; then
        test_pass "team_name matches"
    else
        test_fail "team_name: expected my-team, got $tn"
    fi
    local tl
    tl=$(jq -r '.tracking_level' "$flag_file")
    if [[ "$tl" == "pending" ]]; then
        test_pass "tracking_level is pending"
    else
        test_fail "tracking_level: expected pending, got $tl"
    fi
}

test_create_flag_empty_team() {
    test_section "create_pathflow_flag with empty team_name"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "empty_team" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" ""
    )
    local tn
    tn=$(jq -r '.team_name' "$flag_file")
    if [[ "$tn" == "" ]]; then
        test_pass "team_name is empty string"
    else
        test_fail "team_name should be empty, got: $tn"
    fi
}

test_create_flag_defaults() {
    test_section "create_pathflow_flag default session_id"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "defaults" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag  # No args — should use CODEFLOW_SESSION_ID
    )
    local sid
    sid=$(jq -r '.session_id' "$flag_file")
    if [[ "$sid" == "$TEST_SESSION_ID" ]]; then
        test_pass "Defaults to CODEFLOW_SESSION_ID"
    else
        test_fail "Expected $TEST_SESSION_ID, got $sid"
    fi
}

# ============================================================================
# TESTS: remove_pathflow_flag
# ============================================================================

test_remove_flag() {
    test_section "remove_pathflow_flag removes file"
    setup_pfs_env
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    (
        source "$LIBRARY_UNDER_TEST"
        create_pathflow_flag "$TEST_SESSION_ID" ""
    )
    if [[ ! -f "$flag_file" ]]; then
        test_fail "Flag was not created for removal test"
        return
    fi
    (
        source "$LIBRARY_UNDER_TEST"
        remove_pathflow_flag
    )
    if [[ ! -f "$flag_file" ]]; then
        test_pass "Flag file removed"
    else
        test_fail "Flag file still exists"
    fi
}

test_remove_flag_idempotent() {
    test_section "remove_pathflow_flag idempotent"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        remove_pathflow_flag
        echo "OK"
    )
    if [[ "$result" == "OK" ]]; then
        test_pass "No error on removing non-existent flag"
    else
        test_fail "Error removing non-existent flag"
    fi
}

# ============================================================================
# TESTS: create_sentinel
# ============================================================================

test_create_sentinel() {
    test_section "create_sentinel creates file"
    setup_pfs_env
    local sdir="$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"
    (
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-3"
    )
    if [[ -f "$sdir/pathflow-pf-3" ]]; then
        test_pass "Sentinel pathflow-pf-3 created"
    else
        test_fail "Sentinel pathflow-pf-3 not found"
    fi
}

test_create_sentinel_idempotent() {
    test_section "create_sentinel idempotent"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-1"
        create_sentinel "pf-1"
        echo "OK"
    )
    if [[ "$result" == "OK" ]]; then
        test_pass "Double creation is idempotent"
    else
        test_fail "Error on double creation"
    fi
}

test_create_sentinel_empty_name() {
    test_section "create_sentinel rejects empty name"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "" && echo "ALLOWED" || echo "REJECTED"
    )
    if [[ "$result" == "REJECTED" ]]; then
        test_pass "Empty name rejected (returns 1)"
    else
        test_fail "Empty name should be rejected"
    fi
}

test_create_sentinel_multiple() {
    test_section "create_sentinel multiple names"
    setup_pfs_env
    local sdir="$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"
    (
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-1"
        create_sentinel "pf-2"
        create_sentinel "pf-3"
        create_sentinel "ws-dev"
    )
    local count=0
    for name in pf-1 pf-2 pf-3 ws-dev; do
        if [[ -f "$sdir/pathflow-$name" ]]; then
            test_pass "Sentinel: $name"
            count=$((count + 1))
        else
            test_fail "Missing sentinel: $name"
        fi
    done
}

# ============================================================================
# TESTS: has_sentinel
# ============================================================================

test_has_sentinel_exists() {
    test_section "has_sentinel returns true when exists"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-3"
        has_sentinel "pf-3" && echo "YES" || echo "NO"
    )
    if [[ "$result" == "YES" ]]; then
        test_pass "Returns 0 for existing sentinel"
    else
        test_fail "Should return 0 for existing sentinel"
    fi
}

test_has_sentinel_missing() {
    test_section "has_sentinel returns false when missing"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        has_sentinel "pf-99" && echo "YES" || echo "NO"
    )
    if [[ "$result" == "NO" ]]; then
        test_pass "Returns 1 for missing sentinel"
    else
        test_fail "Should return 1 for missing sentinel"
    fi
}

test_has_sentinel_empty_name() {
    test_section "has_sentinel rejects empty name"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        has_sentinel "" && echo "YES" || echo "NO"
    )
    if [[ "$result" == "NO" ]]; then
        test_pass "Returns 1 for empty name"
    else
        test_fail "Should return 1 for empty name"
    fi
}

# ============================================================================
# TESTS: list_sentinels
# ============================================================================

test_list_sentinels_multiple() {
    test_section "list_sentinels lists all"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-1"
        create_sentinel "pf-3"
        create_sentinel "ws-dev"
        list_sentinels | sort
    )
    local expected
    expected=$(printf 'pf-1\npf-3\nws-dev')
    if [[ "$result" == "$expected" ]]; then
        test_pass "Lists all 3 sentinels"
    else
        test_fail "Expected 3 sentinels, got: $result"
    fi
}

test_list_sentinels_empty() {
    test_section "list_sentinels empty directory"
    setup_pfs_env
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        list_sentinels
    )
    if [[ -z "$result" ]]; then
        test_pass "Returns empty for no sentinels"
    else
        test_fail "Should return empty, got: $result"
    fi
}

test_list_sentinels_no_directory() {
    test_section "list_sentinels no directory"
    setup_pfs_env
    rm -rf "$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"
    local result
    result=$(
        source "$LIBRARY_UNDER_TEST"
        list_sentinels
        echo "OK"
    )
    if echo "$result" | grep -q "OK"; then
        test_pass "Handles missing directory gracefully"
    else
        test_fail "Should handle missing directory"
    fi
}

# ============================================================================
# TESTS: Env File Session ID
# ============================================================================

test_pfs_reads_from_env_file() {
    test_section "Library reads session ID from env file"
    setup_pfs_env
    # Verify the library sources codeflow-env.sh
    if grep -q "codeflow-env.sh" "$LIBRARY_UNDER_TEST"; then
        test_pass "Library references codeflow-env.sh"
    else
        test_fail "Library should reference codeflow-env.sh for session ID"
    fi
}

test_pfs_has_todo_go_cli() {
    test_section "Library has TODO(go-cli) comment"
    if grep -q "TODO(go-cli)" "$LIBRARY_UNDER_TEST"; then
        test_pass "Has TODO(go-cli) comment near env file sourcing"
    else
        test_fail "Should have TODO(go-cli) comment"
    fi
}

test_sentinel_uses_env_file_id() {
    test_section "Sentinel uses session ID from env file"
    setup_pfs_env
    # Create env file with a specific session ID
    local env_file="$REPO_ROOT/.state/runtime/codeflow-env.sh"
    mkdir -p "$(dirname "$env_file")"
    local env_session_id="ses-envtest-$$"
    echo "export CODEFLOW_SESSION_ID='$env_session_id'" > "$env_file"
    # Create sentinel directory for the env file session ID
    mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$env_session_id"

    # Source the library in a subshell with the env file present
    # (must unset source guard so it re-sources)
    local result
    result=$(
        unset _CF_PATHFLOW_STATE_LIB_SOURCED 2>/dev/null || true
        unset CODEFLOW_SESSION_ID 2>/dev/null || true
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-3"
        [[ -f "$REPO_ROOT/.state/sentinels/pathflow/$env_session_id/pathflow-pf-3" ]] && echo "FOUND" || echo "MISSING"
    )
    if [[ "$result" == "FOUND" ]]; then
        test_pass "Sentinel created under env file session ID"
    else
        test_fail "Sentinel should be created under env file session ID ($env_session_id)"
    fi
    rm -f "$env_file"
}

test_sentinel_env_file_cross_teammate() {
    test_section "Multiple sources share sentinel via env file"
    setup_pfs_env
    # Create env file with a specific session ID
    local env_file="$REPO_ROOT/.state/runtime/codeflow-env.sh"
    mkdir -p "$(dirname "$env_file")"
    local shared_id="ses-shared-$$"
    echo "export CODEFLOW_SESSION_ID='$shared_id'" > "$env_file"
    mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$shared_id"

    # "Teammate 1" creates sentinel
    (
        unset _CF_PATHFLOW_STATE_LIB_SOURCED 2>/dev/null || true
        unset CODEFLOW_SESSION_ID 2>/dev/null || true
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-3"
    )

    # "Teammate 2" checks sentinel (fresh source with same env file)
    local result
    result=$(
        unset _CF_PATHFLOW_STATE_LIB_SOURCED 2>/dev/null || true
        unset CODEFLOW_SESSION_ID 2>/dev/null || true
        source "$LIBRARY_UNDER_TEST"
        has_sentinel "pf-3" && echo "YES" || echo "NO"
    )
    if [[ "$result" == "YES" ]]; then
        test_pass "Second source finds sentinel created by first"
    else
        test_fail "Both sources should see same sentinel via env file"
    fi
    rm -f "$env_file"
}

# ============================================================================
# TESTS: Integration
# ============================================================================

test_flag_and_sentinels_lifecycle() {
    test_section "Full lifecycle: create flag → sentinels → remove"
    setup_pfs_env
    if ! command -v jq &>/dev/null; then
        test_skip "lifecycle" "jq not installed"
        return
    fi
    local flag_file="$REPO_ROOT/.state/session/$TEST_SESSION_ID/is-pathflow-active"
    local sdir="$REPO_ROOT/.state/sentinels/pathflow/$TEST_SESSION_ID"

    # Create flag
    (source "$LIBRARY_UNDER_TEST"; create_pathflow_flag "$TEST_SESSION_ID" "lifecycle-team")
    if [[ -f "$flag_file" ]]; then
        test_pass "Step 1: Flag created"
    else
        test_fail "Step 1: Flag not created"
        return
    fi

    # Create sentinels
    (
        source "$LIBRARY_UNDER_TEST"
        create_sentinel "pf-1"
        create_sentinel "pf-3"
        create_sentinel "ws-dev"
    )
    local count
    count=$(find "$sdir" -maxdepth 1 -name 'pathflow-*' -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$count" -eq 3 ]]; then
        test_pass "Step 2: 3 sentinels created"
    else
        test_fail "Step 2: Expected 3 sentinels, got $count"
    fi

    # Check sentinel
    local has_result
    has_result=$(source "$LIBRARY_UNDER_TEST"; has_sentinel "pf-3" && echo YES || echo NO)
    if [[ "$has_result" == "YES" ]]; then
        test_pass "Step 3: has_sentinel works"
    else
        test_fail "Step 3: has_sentinel failed"
    fi

    # Remove flag (simulates session end)
    (source "$LIBRARY_UNDER_TEST"; remove_pathflow_flag)
    if [[ ! -f "$flag_file" ]]; then
        test_pass "Step 4: Flag removed"
    else
        test_fail "Step 4: Flag still exists"
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
    echo -e "${BOLD}Testing: state/cf-pathflow-state.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Library basics
    test_library_exists
    test_shellcheck
    test_has_source_guard
    test_has_set_euo
    test_has_library_guard
    test_functions_available

    # create_pathflow_flag
    test_create_flag_file_exists
    test_create_flag_valid_json
    test_create_flag_fields
    test_create_flag_values
    test_create_flag_empty_team
    test_create_flag_defaults

    # remove_pathflow_flag
    test_remove_flag
    test_remove_flag_idempotent

    # create_sentinel
    test_create_sentinel
    test_create_sentinel_idempotent
    test_create_sentinel_empty_name
    test_create_sentinel_multiple

    # has_sentinel
    test_has_sentinel_exists
    test_has_sentinel_missing
    test_has_sentinel_empty_name

    # list_sentinels
    test_list_sentinels_multiple
    test_list_sentinels_empty
    test_list_sentinels_no_directory

    # Env file session ID
    test_pfs_reads_from_env_file
    test_pfs_has_todo_go_cli
    test_sentinel_uses_env_file_id
    test_sentinel_env_file_cross_teammate

    # Integration
    test_flag_and_sentinels_lifecycle

    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
