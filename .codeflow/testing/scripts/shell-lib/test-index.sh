#!/usr/bin/env bash
# test-index.sh - Tests for shell-lib/index.sh
# Location: .codeflow/testing/scripts/shell-lib/test-index.sh
#
# Usage:
#   ./test-index.sh        Run all tests
#   ./test-index.sh -h     Show help
#   ./test-index.sh -V     Show version

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

Tests for shell-lib/index.sh module.

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
# TEST: Library Loading
# ============================================================================

test_library_loaded_flag() {
    test_section "Library loaded flag"

    # _CODEFLOW_SHELL_LIB_LOADED should be set
    if [[ "${_CODEFLOW_SHELL_LIB_LOADED:-}" == "1" ]]; then
        test_pass "_CODEFLOW_SHELL_LIB_LOADED is set"
    else
        test_fail "_CODEFLOW_SHELL_LIB_LOADED should be set"
    fi
}

test_lib_version_defined() {
    test_section "Library version"

    # CODEFLOW_LIB_VERSION should be defined
    if [[ -n "${CODEFLOW_LIB_VERSION:-}" ]]; then
        test_pass "CODEFLOW_LIB_VERSION is defined: $CODEFLOW_LIB_VERSION"
    else
        test_fail "CODEFLOW_LIB_VERSION should be defined"
    fi
}

# ============================================================================
# TEST: Common Module Functions
# ============================================================================

test_common_functions_loaded() {
    test_section "Common module functions"

    # Check key functions from common.sh
    if type find_repo_root &>/dev/null; then
        test_pass "find_repo_root is available"
    else
        test_fail "find_repo_root should be available"
    fi

    if type trim &>/dev/null; then
        test_pass "trim is available"
    else
        test_fail "trim should be available"
    fi

    if type contains &>/dev/null; then
        test_pass "contains is available"
    else
        test_fail "contains should be available"
    fi

    if type ensure_dir &>/dev/null; then
        test_pass "ensure_dir is available"
    else
        test_fail "ensure_dir should be available"
    fi

    if type array_contains &>/dev/null; then
        test_pass "array_contains is available"
    else
        test_fail "array_contains should be available"
    fi
}

# ============================================================================
# TEST: Logging Module Functions
# ============================================================================

test_logging_functions_loaded() {
    test_section "Logging module functions"

    if type log_debug &>/dev/null; then
        test_pass "log_debug is available"
    else
        test_fail "log_debug should be available"
    fi

    if type log_info &>/dev/null; then
        test_pass "log_info is available"
    else
        test_fail "log_info should be available"
    fi

    if type log_warn &>/dev/null; then
        test_pass "log_warn is available"
    else
        test_fail "log_warn should be available"
    fi

    if type log_error &>/dev/null; then
        test_pass "log_error is available"
    else
        test_fail "log_error should be available"
    fi

    if type get_timestamp_iso &>/dev/null; then
        test_pass "get_timestamp_iso is available"
    else
        test_fail "get_timestamp_iso should be available"
    fi
}

# ============================================================================
# TEST: Errors Module Functions
# ============================================================================

test_errors_functions_loaded() {
    test_section "Errors module functions"

    if type die &>/dev/null; then
        test_pass "die is available"
    else
        test_fail "die should be available"
    fi

    if type assert_file_exists &>/dev/null; then
        test_pass "assert_file_exists is available"
    else
        test_fail "assert_file_exists should be available"
    fi

    if type try_or_default &>/dev/null; then
        test_pass "try_or_default is available"
    else
        test_fail "try_or_default should be available"
    fi

    if type retry &>/dev/null; then
        test_pass "retry is available"
    else
        test_fail "retry should be available"
    fi

    # Check exit code constants
    if [[ -n "${EXIT_SUCCESS:-}" ]]; then
        test_pass "EXIT_SUCCESS constant defined"
    else
        test_fail "EXIT_SUCCESS should be defined"
    fi
}

# ============================================================================
# TEST: Config Module Functions
# ============================================================================

test_config_functions_loaded() {
    test_section "Config module functions"

    if type get_config_dir &>/dev/null; then
        test_pass "get_config_dir is available"
    else
        test_fail "get_config_dir should be available"
    fi

    if type config_get_json &>/dev/null; then
        test_pass "config_get_json is available"
    else
        test_fail "config_get_json should be available"
    fi

    if type load_env_file &>/dev/null; then
        test_pass "load_env_file is available"
    else
        test_fail "load_env_file should be available"
    fi

    if type is_feature_enabled &>/dev/null; then
        test_pass "is_feature_enabled is available"
    else
        test_fail "is_feature_enabled should be available"
    fi
}

# ============================================================================
# TEST: Validation Module Functions
# ============================================================================

test_validation_functions_loaded() {
    test_section "Validation module functions"

    if type is_valid_ulid &>/dev/null; then
        test_pass "is_valid_ulid is available"
    else
        test_fail "is_valid_ulid should be available"
    fi

    if type is_safe_path &>/dev/null; then
        test_pass "is_safe_path is available"
    else
        test_fail "is_safe_path should be available"
    fi

    if type is_valid_branch_name &>/dev/null; then
        test_pass "is_valid_branch_name is available"
    else
        test_fail "is_valid_branch_name should be available"
    fi

    if type is_dangerous_command &>/dev/null; then
        test_pass "is_dangerous_command is available"
    else
        test_fail "is_dangerous_command should be available"
    fi
}

# ============================================================================
# TEST: ULID Module Functions
# ============================================================================

test_ulid_functions_loaded() {
    test_section "ULID module functions"

    if type generate_ulid &>/dev/null; then
        test_pass "generate_ulid is available"
    else
        test_fail "generate_ulid should be available"
    fi

    if type generate_epic_id &>/dev/null; then
        test_pass "generate_epic_id is available"
    else
        test_fail "generate_epic_id should be available"
    fi

    if type generate_task_id &>/dev/null; then
        test_pass "generate_task_id is available"
    else
        test_fail "generate_task_id should be available"
    fi

    if type ulid_timestamp &>/dev/null; then
        test_pass "ulid_timestamp is available"
    else
        test_fail "ulid_timestamp should be available"
    fi

    # Check ULID_ALPHABET constant
    if [[ -n "${ULID_ALPHABET:-}" ]]; then
        test_pass "ULID_ALPHABET constant defined"
    else
        test_fail "ULID_ALPHABET should be defined"
    fi
}

# ============================================================================
# TEST: Direct Loading
# ============================================================================

test_direct_index_load() {
    test_section "Direct index.sh loading"

    # Test loading index.sh directly in a subshell
    local result
    result=$(bash -c '
        REPO_ROOT="'"$(find_repo_root)"'"
        source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"

        # Check flag is set
        if [[ "${_CODEFLOW_SHELL_LIB_LOADED:-}" == "1" ]]; then
            echo "FLAG_OK"
        fi

        # Check a function from each module
        type find_repo_root &>/dev/null && echo "COMMON_OK"
        type log_info &>/dev/null && echo "LOGGING_OK"
        type die &>/dev/null && echo "ERRORS_OK"
        type get_config_dir &>/dev/null && echo "CONFIG_OK"
        type is_valid_ulid &>/dev/null && echo "VALIDATION_OK"
        type generate_ulid &>/dev/null && echo "ULID_OK"
    ' 2>/dev/null)

    if [[ "$result" == *"FLAG_OK"* ]]; then
        test_pass "Direct load sets _CODEFLOW_SHELL_LIB_LOADED"
    else
        test_fail "Direct load should set _CODEFLOW_SHELL_LIB_LOADED"
    fi

    if [[ "$result" == *"COMMON_OK"* ]]; then
        test_pass "Direct load includes common module"
    else
        test_fail "Direct load should include common module"
    fi

    if [[ "$result" == *"LOGGING_OK"* ]]; then
        test_pass "Direct load includes logging module"
    else
        test_fail "Direct load should include logging module"
    fi

    if [[ "$result" == *"ERRORS_OK"* ]]; then
        test_pass "Direct load includes errors module"
    else
        test_fail "Direct load should include errors module"
    fi

    if [[ "$result" == *"CONFIG_OK"* ]]; then
        test_pass "Direct load includes config module"
    else
        test_fail "Direct load should include config module"
    fi

    if [[ "$result" == *"VALIDATION_OK"* ]]; then
        test_pass "Direct load includes validation module"
    else
        test_fail "Direct load should include validation module"
    fi

    if [[ "$result" == *"ULID_OK"* ]]; then
        test_pass "Direct load includes ulid module"
    else
        test_fail "Direct load should include ulid module"
    fi
}

# ============================================================================
# TEST: Integration
# ============================================================================

test_modules_work_together() {
    test_section "Module integration"

    # Test that functions from different modules work together

    # Generate a ULID (ulid module)
    local ulid
    ulid=$(generate_ulid)

    # Validate it (validation module)
    if is_valid_ulid "$ulid"; then
        test_pass "Generated ULID passes validation"
    else
        test_fail "Generated ULID should pass validation: $ulid"
    fi

    # Test path functions with logging
    local repo_root
    repo_root=$(find_repo_root)

    if [[ -d "$repo_root" ]]; then
        test_pass "find_repo_root returns valid directory"
    else
        test_fail "find_repo_root should return valid directory"
    fi

    # Test config with validation
    local config_dir
    config_dir=$(get_config_dir)

    if [[ "$config_dir" == *"config"* ]]; then
        test_pass "Config and common modules integrate correctly"
    else
        test_fail "Config and common modules should integrate"
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
    echo -e "${BOLD}Testing: shell-lib/index.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Library loading
    test_library_loaded_flag
    test_lib_version_defined

    # Module functions
    test_common_functions_loaded
    test_logging_functions_loaded
    test_errors_functions_loaded
    test_config_functions_loaded
    test_validation_functions_loaded
    test_ulid_functions_loaded

    # Direct loading
    test_direct_index_load

    # Integration
    test_modules_work_together

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
