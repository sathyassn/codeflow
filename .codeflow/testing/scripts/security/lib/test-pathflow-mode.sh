#!/usr/bin/env bash
# test-pathflow-mode.sh - Tests for PathFlow mode detection functions
# Location: .codeflow/testing/scripts/security/lib/test-pathflow-mode.sh
#
# Tests is_pathflow_active() and get_pathflow_setting() functions
# from context-lib.sh (re-exported via security-lib.sh).
#
# Usage:
#   ./test-pathflow-mode.sh      Run all tests
#   ./test-pathflow-mode.sh -h   Show help
#   ./test-pathflow-mode.sh -V   Show version

# shellcheck disable=SC2030,SC2031  # Intentional: REPO_ROOT overridden in test subshells
set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="1.0.0"
readonly SCRIPT_VERSION
# From .codeflow/testing/scripts/security/lib/ -> 5 levels up to repo root
# Note: REPO_ROOT is NOT readonly here because tests override it in subshells
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
TESTING_DIR="$REPO_ROOT/.codeflow/testing"
readonly TESTING_DIR

export REPO_ROOT

# Library under test — context-lib.sh is the canonical source for these functions
LIB_FILE="$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh"

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for is_pathflow_active() and get_pathflow_setting() from context-lib.sh.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information

Examples:
    $SCRIPT_NAME              Run all tests
    $SCRIPT_NAME --help       Show this help
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Test temp directory
TEST_TMPDIR=""

# Flag file path used by is_pathflow_active()
FLAG_FILE="$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}/is-pathflow-active"

# Setup: create temp directory structure
setup_test_env() {
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-pathflow-test-XXXXXX")
    # Create the .claude directory for settings
    mkdir -p "$TEST_TMPDIR/repo/.claude"
    # Ensure flag file does not exist at start
    rm -f "$FLAG_FILE" 2>/dev/null || true
}

# Teardown: remove temp directory and flag file
teardown_test_env() {
    rm -f "$FLAG_FILE" 2>/dev/null || true
    if [[ -n "$TEST_TMPDIR" && -d "$TEST_TMPDIR" ]]; then
        rm -rf "$TEST_TMPDIR"
    fi
}

# Cleanup on exit
trap teardown_test_env EXIT

# ============================================================================
# TEST: is_pathflow_active() - flag file absent
# ============================================================================

test_is_pathflow_active_no_flag() {
    test_section "is_pathflow_active: returns 1 when flag file absent"

    setup_test_env

    # Ensure flag file does not exist
    rm -f "$FLAG_FILE" 2>/dev/null || true

    local exit_code=0
    (
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        is_pathflow_active
    ) 2>/dev/null || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "is_pathflow_active returns non-zero when flag absent"
    else
        test_fail "is_pathflow_active should return non-zero when flag absent"
    fi

    teardown_test_env
}

# ============================================================================
# TEST: is_pathflow_active() - flag file present
# ============================================================================

test_is_pathflow_active_with_flag() {
    test_section "is_pathflow_active: returns 0 when flag file exists"

    setup_test_env

    # Create the flag file where the function will look (under TEST_TMPDIR)
    local test_flag="$TEST_TMPDIR/repo/.state/session/${CODEFLOW_SESSION_ID:-unknown}/is-pathflow-active"
    mkdir -p "$(dirname "$test_flag")"
    touch "$test_flag"

    local exit_code=0
    (
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        is_pathflow_active
    ) 2>/dev/null || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "is_pathflow_active returns 0 when flag exists"
    else
        test_fail "is_pathflow_active should return 0 when flag exists"
    fi

    teardown_test_env
}

# ============================================================================
# TEST: is_pathflow_active() - env file sourcing
# ============================================================================

test_is_pathflow_active_env_file() {
    test_section "is_pathflow_active: sources env file for session ID"

    setup_test_env

    # Create env file with a specific session ID
    local test_session="ses-envtest-$$"
    local env_dir="$TEST_TMPDIR/repo/.state/runtime"
    mkdir -p "$env_dir"
    echo "export CODEFLOW_SESSION_ID='$test_session'" > "$env_dir/codeflow-env.sh"

    # Create the flag file at the path the function will look for (using env file session ID)
    local test_flag="$TEST_TMPDIR/repo/.state/session/$test_session/is-pathflow-active"
    mkdir -p "$(dirname "$test_flag")"
    touch "$test_flag"

    local exit_code=0
    (
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset CODEFLOW_SESSION_ID 2>/dev/null || true
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        is_pathflow_active
    ) 2>/dev/null || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "is_pathflow_active finds flag via env file session ID"
    else
        test_fail "is_pathflow_active should find flag using session ID from env file"
    fi

    teardown_test_env
}

test_is_pathflow_active_env_file_ref() {
    test_section "is_pathflow_active: references codeflow-env.sh in source"

    if grep -q "codeflow-env.sh" "$LIB_FILE"; then
        test_pass "context-lib.sh references codeflow-env.sh"
    else
        test_fail "context-lib.sh should reference codeflow-env.sh for session ID"
    fi
}

# ============================================================================
# TEST: get_pathflow_setting() - no settings.json
# ============================================================================

test_get_setting_no_settings_file() {
    test_section "get_pathflow_setting: returns '' when settings.json missing"

    setup_test_env

    # Do not create settings.json - the directory exists but no file
    local result
    result=$(
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        get_pathflow_setting
    ) 2>/dev/null

    assert_empty "$result" "get_pathflow_setting returns empty when no settings.json"

    teardown_test_env
}

# ============================================================================
# TEST: get_pathflow_setting() - settings.json without _codeflow key
# ============================================================================

test_get_setting_no_codeflow_key() {
    test_section "get_pathflow_setting: returns '' when _codeflow key missing"

    setup_test_env

    # Create settings.json without _codeflow key
    cat > "$TEST_TMPDIR/repo/.claude/settings.json" <<'SETTINGS'
{
    "permissions": {
        "allow": []
    }
}
SETTINGS

    local result
    result=$(
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        get_pathflow_setting
    ) 2>/dev/null

    assert_empty "$result" "get_pathflow_setting returns empty when _codeflow key missing"

    teardown_test_env
}

# ============================================================================
# TEST: get_pathflow_setting() - returns "auto"
# ============================================================================

test_get_setting_returns_auto() {
    test_section "get_pathflow_setting: returns 'auto'"

    setup_test_env

    cat > "$TEST_TMPDIR/repo/.claude/settings.json" <<'SETTINGS'
{
    "_codeflow": {
        "agent_teams": "auto"
    }
}
SETTINGS

    local result
    result=$(
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        get_pathflow_setting
    ) 2>/dev/null

    assert_equals "auto" "$result" "get_pathflow_setting returns 'auto'"

    teardown_test_env
}

# ============================================================================
# TEST: get_pathflow_setting() - returns "always"
# ============================================================================

test_get_setting_returns_always() {
    test_section "get_pathflow_setting: returns 'always'"

    setup_test_env

    cat > "$TEST_TMPDIR/repo/.claude/settings.json" <<'SETTINGS'
{
    "_codeflow": {
        "agent_teams": "always"
    }
}
SETTINGS

    local result
    result=$(
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        get_pathflow_setting
    ) 2>/dev/null

    assert_equals "always" "$result" "get_pathflow_setting returns 'always'"

    teardown_test_env
}

# ============================================================================
# TEST: get_pathflow_setting() - returns "never"
# ============================================================================

test_get_setting_returns_never() {
    test_section "get_pathflow_setting: returns 'never'"

    setup_test_env

    cat > "$TEST_TMPDIR/repo/.claude/settings.json" <<'SETTINGS'
{
    "_codeflow": {
        "agent_teams": "never"
    }
}
SETTINGS

    local result
    result=$(
        export REPO_ROOT="$TEST_TMPDIR/repo"
        unset _CONTEXT_LIB_LOADED 2>/dev/null || true
        source "$LIB_FILE"
        get_pathflow_setting
    ) 2>/dev/null

    assert_equals "never" "$result" "get_pathflow_setting returns 'never'"

    teardown_test_env
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                echo "$SCRIPT_NAME version $SCRIPT_VERSION"
                exit 0
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: PathFlow Mode Detection (context-lib.sh)${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # is_pathflow_active() tests
    test_is_pathflow_active_no_flag
    test_is_pathflow_active_with_flag
    test_is_pathflow_active_env_file
    test_is_pathflow_active_env_file_ref

    # get_pathflow_setting tests
    test_get_setting_no_settings_file
    test_get_setting_no_codeflow_key
    test_get_setting_returns_auto
    test_get_setting_returns_always
    test_get_setting_returns_never

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
