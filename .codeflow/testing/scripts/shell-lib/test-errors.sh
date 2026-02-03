#!/usr/bin/env bash
# test-errors.sh - Tests for shell-lib/errors.sh
# Location: .codeflow/testing/scripts/shell-lib/test-errors.sh
#
# Usage:
#   ./test-errors.sh       Run all tests
#   ./test-errors.sh -h    Show help
#   ./test-errors.sh -V    Show version

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

Tests for shell-lib/errors.sh module.

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

# ============================================================================
# TEST: Exit Code Constants
# ============================================================================

test_exit_code_constants() {
    test_section "Exit code constants"

    assert_equals "0" "$EXIT_SUCCESS" "EXIT_SUCCESS is 0"
    assert_equals "1" "$EXIT_GENERAL_ERROR" "EXIT_GENERAL_ERROR is 1"
    assert_equals "2" "$EXIT_BLOCKED" "EXIT_BLOCKED is 2"
    assert_equals "3" "$EXIT_INVALID_INPUT" "EXIT_INVALID_INPUT is 3"
    assert_equals "4" "$EXIT_CONFIG_ERROR" "EXIT_CONFIG_ERROR is 4"
    assert_equals "5" "$EXIT_DB_ERROR" "EXIT_DB_ERROR is 5"
    assert_equals "6" "$EXIT_PERMISSION_DENIED" "EXIT_PERMISSION_DENIED is 6"
    assert_equals "7" "$EXIT_NOT_FOUND" "EXIT_NOT_FOUND is 7"
    assert_equals "8" "$EXIT_TIMEOUT" "EXIT_TIMEOUT is 8"
    assert_equals "9" "$EXIT_DEPENDENCY_ERROR" "EXIT_DEPENDENCY_ERROR is 9"
}

# ============================================================================
# TEST: Die Functions
# ============================================================================

test_die() {
    test_section "die"

    # die should exit with given code
    local exit_code
    exit_code=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; die "test error" 42; echo "should not reach"' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:42"* ]]; then
        test_pass "die exits with specified code"
    else
        test_fail "die should exit with code 42: $exit_code"
    fi

    # Should output error message
    local output
    output=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; die "custom error message" 1' 2>&1 || true)

    if [[ "$output" == *"custom error message"* ]]; then
        test_pass "die outputs error message"
    else
        test_fail "die should output error message: $output"
    fi
}

test_die_default_exit_code() {
    test_section "die: default exit code"

    local exit_code
    exit_code=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; die "error"; echo "unreachable"' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:1"* ]]; then
        test_pass "die defaults to exit code 1"
    else
        test_fail "die should default to exit code 1: $exit_code"
    fi
}

# ============================================================================
# TEST: Assertions
# ============================================================================

test_assert_file_exists_pass() {
    test_section "assert_file_exists: pass"

    setup_test_dir "assert-file"

    create_test_file "exists.txt" "content"

    # Should not die for existing file
    local result
    result=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_file_exists "'"$TEST_DIR"'/exists.txt"; echo "OK"' 2>&1)

    if [[ "$result" == *"OK"* ]]; then
        test_pass "assert_file_exists passes for existing file"
    else
        test_fail "assert_file_exists should pass for existing file: $result"
    fi

    teardown_test_dir
}

test_assert_file_exists_fail() {
    test_section "assert_file_exists: fail"

    # Should die for non-existent file
    # Note: We need to source errors.sh directly to test its assert functions
    # because test-helpers.sh overrides them with test versions
    local exit_code
    exit_code=$(bash -c '
        REPO_ROOT="'"$(find_repo_root)"'"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/common.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/logging.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/errors.sh"
        assert_file_exists "/nonexistent/file.txt"
        echo "unreachable"
    ' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:7"* ]]; then
        test_pass "assert_file_exists dies with EXIT_NOT_FOUND"
    else
        test_fail "assert_file_exists should die with code 7: $exit_code"
    fi
}

test_assert_dir_exists_pass() {
    test_section "assert_dir_exists: pass"

    setup_test_dir "assert-dir"

    # Should not die for existing directory
    local result
    result=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_dir_exists "'"$TEST_DIR"'"; echo "OK"' 2>&1)

    if [[ "$result" == *"OK"* ]]; then
        test_pass "assert_dir_exists passes for existing directory"
    else
        test_fail "assert_dir_exists should pass for existing directory: $result"
    fi

    teardown_test_dir
}

test_assert_dir_exists_fail() {
    test_section "assert_dir_exists: fail"

    # Should die for non-existent directory
    local exit_code
    exit_code=$(bash -c '
        REPO_ROOT="'"$(find_repo_root)"'"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/common.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/logging.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/errors.sh"
        assert_dir_exists "/nonexistent/dir"
        echo "unreachable"
    ' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:7"* ]]; then
        test_pass "assert_dir_exists dies with EXIT_NOT_FOUND"
    else
        test_fail "assert_dir_exists should die with code 7: $exit_code"
    fi
}

test_assert_command_exists_pass() {
    test_section "assert_command_exists: pass"

    # Should not die for existing command
    local result
    result=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_command_exists "bash"; echo "OK"' 2>&1)

    if [[ "$result" == *"OK"* ]]; then
        test_pass "assert_command_exists passes for bash"
    else
        test_fail "assert_command_exists should pass for bash: $result"
    fi
}

test_assert_command_exists_fail() {
    test_section "assert_command_exists: fail"

    # Should die for non-existent command
    local exit_code
    exit_code=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_command_exists "nonexistent_cmd_xyz"; echo "unreachable"' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:9"* ]]; then
        test_pass "assert_command_exists dies with EXIT_DEPENDENCY_ERROR"
    else
        test_fail "assert_command_exists should die with code 9: $exit_code"
    fi
}

test_assert_var_set_pass() {
    test_section "assert_var_set: pass"

    # Should not die for set variable
    local result
    result=$(bash -c 'export TEST_VAR="value"; source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_var_set "TEST_VAR"; echo "OK"' 2>&1)

    if [[ "$result" == *"OK"* ]]; then
        test_pass "assert_var_set passes for set variable"
    else
        test_fail "assert_var_set should pass for set variable: $result"
    fi
}

test_assert_var_set_fail() {
    test_section "assert_var_set: fail"

    # Should die for unset variable
    local exit_code
    exit_code=$(bash -c 'unset UNSET_VAR; source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_var_set "UNSET_VAR"; echo "unreachable"' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:3"* ]]; then
        test_pass "assert_var_set dies with EXIT_INVALID_INPUT"
    else
        test_fail "assert_var_set should die with code 3: $exit_code"
    fi
}

test_assert_not_empty_pass() {
    test_section "assert_not_empty: pass"

    local result
    result=$(bash -c 'source "'"$TESTING_DIR"'/lib/test-helpers.sh"; assert_not_empty "content"; echo "OK"' 2>&1)

    if [[ "$result" == *"OK"* ]]; then
        test_pass "assert_not_empty passes for non-empty string"
    else
        test_fail "assert_not_empty should pass for non-empty string: $result"
    fi
}

test_assert_not_empty_fail() {
    test_section "assert_not_empty: fail"

    local exit_code
    exit_code=$(bash -c '
        REPO_ROOT="'"$(find_repo_root)"'"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/common.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/logging.sh"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/errors.sh"
        assert_not_empty ""
        echo "unreachable"
    ' 2>&1 || echo "EXIT:$?")

    if [[ "$exit_code" == *"EXIT:3"* ]]; then
        test_pass "assert_not_empty dies with EXIT_INVALID_INPUT"
    else
        test_fail "assert_not_empty should die with code 3: $exit_code"
    fi
}

# ============================================================================
# TEST: Graceful Degradation
# ============================================================================

test_try_or_default() {
    test_section "try_or_default"

    # Command succeeds
    local result
    result=$(try_or_default "fallback" echo "success")
    assert_equals "success" "$result" "try_or_default returns command output on success"

    # Command fails
    result=$(try_or_default "fallback" false)
    assert_equals "fallback" "$result" "try_or_default returns default on failure"

    # Non-existent command
    result=$(try_or_default "fallback" nonexistent_cmd_xyz)
    assert_equals "fallback" "$result" "try_or_default returns default for missing command"
}

test_try_or_warn() {
    test_section "try_or_warn"

    # Command succeeds
    if try_or_warn "warning" true; then
        test_pass "try_or_warn returns 0 on success"
    else
        test_fail "try_or_warn should return 0 on success"
    fi

    # Command fails
    if ! try_or_warn "warning" false 2>/dev/null; then
        test_pass "try_or_warn returns 1 on failure"
    else
        test_fail "try_or_warn should return 1 on failure"
    fi
}

test_retry() {
    test_section "retry"

    # Command that always succeeds
    if retry 3 0.1 true; then
        test_pass "retry succeeds when command succeeds"
    else
        test_fail "retry should succeed when command succeeds"
    fi

    # Command that always fails
    if retry 2 0.1 false 2>/dev/null; then
        test_fail "retry should fail when command always fails"
    else
        test_pass "retry fails after max attempts"
    fi
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
    echo -e "${BOLD}Testing: shell-lib/errors.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Exit code constants
    test_exit_code_constants

    # Die functions
    test_die
    test_die_default_exit_code

    # Assertions
    test_assert_file_exists_pass
    test_assert_file_exists_fail
    test_assert_dir_exists_pass
    test_assert_dir_exists_fail
    test_assert_command_exists_pass
    test_assert_command_exists_fail
    test_assert_var_set_pass
    test_assert_var_set_fail
    test_assert_not_empty_pass
    test_assert_not_empty_fail

    # Graceful degradation
    test_try_or_default
    test_try_or_warn
    test_retry

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
