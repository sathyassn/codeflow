#!/usr/bin/env bash
# test-logging.sh - Tests for shell-lib/logging.sh
# Location: .codeflow/testing/scripts/shell-lib/test-logging.sh
#
# Usage:
#   ./test-logging.sh      Run all tests
#   ./test-logging.sh -h   Show help
#   ./test-logging.sh -V   Show version

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

Tests for shell-lib/logging.sh module.

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
# TEST: Log Levels
# ============================================================================

test_log_level_constants() {
    test_section "Log level constants"

    # Check constants are defined
    assert_equals "0" "$LOG_LEVEL_DEBUG" "LOG_LEVEL_DEBUG is 0"
    assert_equals "1" "$LOG_LEVEL_INFO" "LOG_LEVEL_INFO is 1"
    assert_equals "2" "$LOG_LEVEL_WARN" "LOG_LEVEL_WARN is 2"
    assert_equals "3" "$LOG_LEVEL_ERROR" "LOG_LEVEL_ERROR is 3"
}

test_default_log_level() {
    test_section "Default log level"

    # Default should be INFO (1)
    assert_equals "$LOG_LEVEL_INFO" "${CODEFLOW_LOG_LEVEL:-$LOG_LEVEL_INFO}" "Default log level is INFO"
}

# ============================================================================
# TEST: Timestamp Utilities
# ============================================================================

test_get_timestamp_iso() {
    test_section "get_timestamp_iso"

    local ts
    ts=$(get_timestamp_iso)

    # Should match ISO 8601 format
    if [[ "$ts" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
        test_pass "Timestamp is ISO 8601 format"
    else
        test_fail "Timestamp should be ISO 8601 format: $ts"
    fi

    # Should end with Z (UTC)
    if [[ "$ts" == *Z ]]; then
        test_pass "Timestamp ends with Z (UTC)"
    else
        test_fail "Timestamp should end with Z: $ts"
    fi
}

test_get_timestamp_unix() {
    test_section "get_timestamp_unix"

    local ts
    ts=$(get_timestamp_unix)

    # Should be numeric
    if [[ "$ts" =~ ^[0-9]+$ ]]; then
        test_pass "Unix timestamp is numeric"
    else
        test_fail "Unix timestamp should be numeric: $ts"
    fi

    # Should be reasonable (after 2024: 1704067200)
    if [[ $ts -gt 1704067200 ]]; then
        test_pass "Unix timestamp is after 2024"
    else
        test_fail "Unix timestamp should be after 2024: $ts"
    fi
}

test_get_timestamp_human() {
    test_section "get_timestamp_human"

    local ts
    ts=$(get_timestamp_human)

    # Should match human readable format
    if [[ "$ts" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}\ [0-9]{2}:[0-9]{2}:[0-9]{2}$ ]]; then
        test_pass "Human timestamp is correct format"
    else
        test_fail "Human timestamp should be YYYY-MM-DD HH:MM:SS: $ts"
    fi
}

# ============================================================================
# TEST: Core Logging Functions
# ============================================================================

test_log_debug() {
    test_section "log_debug"

    # Save original log level
    local original_level="${CODEFLOW_LOG_LEVEL:-$LOG_LEVEL_INFO}"

    # Set to DEBUG to capture output
    CODEFLOW_LOG_LEVEL=$LOG_LEVEL_DEBUG

    local output
    output=$(log_debug "test debug message" 2>&1)

    # Should contain DEBUG label
    if [[ "$output" == *"DEBUG"* ]]; then
        test_pass "log_debug outputs DEBUG label"
    else
        test_fail "log_debug should output DEBUG label: $output"
    fi

    # Should contain message
    if [[ "$output" == *"test debug message"* ]]; then
        test_pass "log_debug outputs message"
    else
        test_fail "log_debug should output message: $output"
    fi

    # Restore log level
    CODEFLOW_LOG_LEVEL=$original_level
}

test_log_info() {
    test_section "log_info"

    local output
    output=$(log_info "test info message" 2>&1)

    # Should contain INFO label
    if [[ "$output" == *"INFO"* ]]; then
        test_pass "log_info outputs INFO label"
    else
        test_fail "log_info should output INFO label: $output"
    fi

    # Should contain message
    if [[ "$output" == *"test info message"* ]]; then
        test_pass "log_info outputs message"
    else
        test_fail "log_info should output message: $output"
    fi
}

test_log_warn() {
    test_section "log_warn"

    local output
    output=$(log_warn "test warning message" 2>&1)

    # Should contain WARN label
    if [[ "$output" == *"WARN"* ]]; then
        test_pass "log_warn outputs WARN label"
    else
        test_fail "log_warn should output WARN label: $output"
    fi

    # Should contain message
    if [[ "$output" == *"test warning message"* ]]; then
        test_pass "log_warn outputs message"
    else
        test_fail "log_warn should output message: $output"
    fi
}

test_log_error() {
    test_section "log_error"

    local output
    output=$(log_error "test error message" 2>&1)

    # Should contain ERROR label
    if [[ "$output" == *"ERROR"* ]]; then
        test_pass "log_error outputs ERROR label"
    else
        test_fail "log_error should output ERROR label: $output"
    fi

    # Should contain message
    if [[ "$output" == *"test error message"* ]]; then
        test_pass "log_error outputs message"
    else
        test_fail "log_error should output message: $output"
    fi
}

test_log_level_filtering() {
    test_section "Log level filtering"

    # Set to WARN level
    local original_level="${CODEFLOW_LOG_LEVEL:-$LOG_LEVEL_INFO}"
    CODEFLOW_LOG_LEVEL=$LOG_LEVEL_WARN

    # Debug should be filtered
    local debug_output
    debug_output=$(log_debug "debug" 2>&1)
    if [[ -z "$debug_output" ]]; then
        test_pass "DEBUG filtered when level is WARN"
    else
        test_fail "DEBUG should be filtered when level is WARN"
    fi

    # Info should be filtered
    local info_output
    info_output=$(log_info "info" 2>&1)
    if [[ -z "$info_output" ]]; then
        test_pass "INFO filtered when level is WARN"
    else
        test_fail "INFO should be filtered when level is WARN"
    fi

    # Warn should pass
    local warn_output
    warn_output=$(log_warn "warn" 2>&1)
    if [[ -n "$warn_output" ]]; then
        test_pass "WARN passes when level is WARN"
    else
        test_fail "WARN should pass when level is WARN"
    fi

    # Restore log level
    CODEFLOW_LOG_LEVEL=$original_level
}

# ============================================================================
# TEST: File Logging
# ============================================================================

test_get_log_dir() {
    test_section "get_log_dir"

    local log_dir
    log_dir=$(get_log_dir "test-category")

    # Should end with category
    if [[ "$log_dir" == *"logs/test-category" ]]; then
        test_pass "Log dir includes category"
    else
        test_fail "Log dir should include category: $log_dir"
    fi

    # Default category
    log_dir=$(get_log_dir)
    if [[ "$log_dir" == *"logs/general" ]]; then
        test_pass "Default log dir is general"
    else
        test_fail "Default log dir should be general: $log_dir"
    fi
}

test_get_log_file() {
    test_section "get_log_file"

    local log_file
    log_file=$(get_log_file "test-category" "app")

    # Should contain date
    local today
    today=$(date +%Y-%m-%d)
    if [[ "$log_file" == *"$today"* ]]; then
        test_pass "Log file contains today's date"
    else
        test_fail "Log file should contain today's date: $log_file"
    fi

    # Should have .log extension
    if [[ "$log_file" == *".log" ]]; then
        test_pass "Log file has .log extension"
    else
        test_fail "Log file should have .log extension: $log_file"
    fi
}

test_log_to_file() {
    test_section "log_to_file"

    setup_test_dir "log-file"

    local log_file="$TEST_DIR/test.log"
    log_to_file "$log_file" "Test log message"

    # File should exist
    assert_file_exists "$log_file" "Log file created"

    # Should contain timestamp and message
    local content
    content=$(cat "$log_file")

    if [[ "$content" == *"Test log message"* ]]; then
        test_pass "Log file contains message"
    else
        test_fail "Log file should contain message"
    fi

    if [[ "$content" =~ \[[0-9]{4}-[0-9]{2}-[0-9]{2}T ]]; then
        test_pass "Log file contains timestamp"
    else
        test_fail "Log file should contain timestamp"
    fi

    teardown_test_dir
}

# ============================================================================
# TEST: Structured Logging (JSON)
# ============================================================================

test_log_json() {
    test_section "log_json"

    setup_test_dir "log-json"

    local log_file="$TEST_DIR/events.jsonl"
    log_json "$log_file" "test_event" '"key":"value"'

    # File should exist
    assert_file_exists "$log_file" "JSON log file created"

    # Should be valid JSON-ish
    local content
    content=$(cat "$log_file")

    if [[ "$content" == *"test_event"* ]]; then
        test_pass "JSON log contains event type"
    else
        test_fail "JSON log should contain event type"
    fi

    if [[ "$content" == *"timestamp"* ]]; then
        test_pass "JSON log contains timestamp"
    else
        test_fail "JSON log should contain timestamp"
    fi

    teardown_test_dir
}

# ============================================================================
# TEST: Convenience Functions
# ============================================================================

test_log_success() {
    test_section "log_success"

    local output
    output=$(log_success "Success message")

    # Should contain checkmark (may be color-coded)
    if [[ "$output" == *"Success message"* ]]; then
        test_pass "log_success outputs message"
    else
        test_fail "log_success should output message: $output"
    fi
}

test_log_failure() {
    test_section "log_failure"

    local output
    output=$(log_failure "Failure message")

    if [[ "$output" == *"Failure message"* ]]; then
        test_pass "log_failure outputs message"
    else
        test_fail "log_failure should output message: $output"
    fi
}

test_log_warning() {
    test_section "log_warning"

    local output
    output=$(log_warning "Warning message")

    if [[ "$output" == *"Warning message"* ]]; then
        test_pass "log_warning outputs message"
    else
        test_fail "log_warning should output message: $output"
    fi
}

test_log_section() {
    test_section "log_section"

    local output
    output=$(log_section "Section Title")

    if [[ "$output" == *"Section Title"* ]]; then
        test_pass "log_section outputs title"
    else
        test_fail "log_section should output title: $output"
    fi

    if [[ "$output" == *"==="* ]]; then
        test_pass "log_section has section markers"
    else
        test_fail "log_section should have === markers: $output"
    fi
}

test_log_subsection() {
    test_section "log_subsection"

    local output
    output=$(log_subsection "Subsection Title")

    if [[ "$output" == *"Subsection Title"* ]]; then
        test_pass "log_subsection outputs title"
    else
        test_fail "log_subsection should output title: $output"
    fi

    if [[ "$output" == *"---"* ]]; then
        test_pass "log_subsection has subsection markers"
    else
        test_fail "log_subsection should have --- markers: $output"
    fi
}

test_no_color_support() {
    test_section "NO_COLOR support"

    # Test that NO_COLOR disables color escape codes
    # Use a fresh bash subprocess to avoid readonly variable conflicts
    local lib_dir
    lib_dir="$(cd "$TESTING_DIR/../scripts/shell-lib" && pwd)"
    local output
    output=$(NO_COLOR=1 bash -c 'source "'"$lib_dir"'/logging.sh" && echo "${COLOR_RED}${COLOR_GREEN}${COLOR_BLUE}"' 2>/dev/null)

    if [[ -z "$output" ]]; then
        test_pass "NO_COLOR disables color escape codes"
    else
        test_fail "NO_COLOR should disable color escape codes but got: $output"
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
    echo -e "${BOLD}Testing: shell-lib/logging.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Log levels
    test_log_level_constants
    test_default_log_level

    # Timestamp utilities
    test_get_timestamp_iso
    test_get_timestamp_unix
    test_get_timestamp_human

    # Core logging
    test_log_debug
    test_log_info
    test_log_warn
    test_log_error
    test_log_level_filtering

    # File logging
    test_get_log_dir
    test_get_log_file
    test_log_to_file

    # Structured logging
    test_log_json

    # Convenience functions
    test_log_success
    test_log_failure
    test_log_warning
    test_log_section
    test_log_subsection
    test_no_color_support

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
