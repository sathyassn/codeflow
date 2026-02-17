#!/usr/bin/env bash
# test-generate-format-id.sh - Tests for generate-format-id.sh
# Location: .codeflow/testing/scripts/db/test-generate-format-id.sh
#
# Tests .codeflow/scripts/db/generate-format-id.sh which generates
# human-readable format IDs for epics and tasks.
#
# Usage:
#   ./test-generate-format-id.sh           Run all tests
#   ./test-generate-format-id.sh -h        Show help
#   ./test-generate-format-id.sh -V        Show version

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
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
readonly REPO_ROOT

usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for .codeflow/scripts/db/generate-format-id.sh

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

SOURCE_SCRIPT="$REPO_ROOT/.codeflow/scripts/db/generate-format-id.sh"

# ============================================================================
# TEST: Script file exists
# ============================================================================

test_script_exists() {
    test_section "generate-format-id: script exists"
    assert_file_exists "$SOURCE_SCRIPT" "generate-format-id.sh exists"
}

# ============================================================================
# TEST: Help flag
# ============================================================================

test_help_flag() {
    test_section "generate-format-id: --help and -h flags"

    assert_success "bash \"$SOURCE_SCRIPT\" --help" "--help exits 0"
    assert_success "bash \"$SOURCE_SCRIPT\" -h" "-h exits 0"

    local output
    output=$(bash "$SOURCE_SCRIPT" --help 2>&1)
    assert_contains "$output" "Usage:" "--help shows Usage"
    assert_contains "$output" "kind" "--help describes kind argument"
    assert_contains "$output" "AREA" "--help describes AREA argument"
}

# ============================================================================
# TEST: Argument count validation
# ============================================================================

test_arg_count_validation() {
    test_section "generate-format-id: argument count validation"

    assert_fails "bash \"$SOURCE_SCRIPT\"" "no args fails"
    assert_fails "bash \"$SOURCE_SCRIPT\" epic" "1 arg fails"
    assert_fails "bash \"$SOURCE_SCRIPT\" epic INF" "2 args fails"
    assert_fails "bash \"$SOURCE_SCRIPT\" epic INF FEAT" "3 args fails"
    assert_fails "bash \"$SOURCE_SCRIPT\" epic INF FEAT GENL extra" "5 args fails"

    # Verify error message content for no-args case
    local err_output
    err_output=$(bash "$SOURCE_SCRIPT" 2>&1 || true)
    assert_contains "$err_output" "Error" "no-args produces error message"
}

# ============================================================================
# TEST: Invalid kind
# ============================================================================

test_invalid_kind() {
    test_section "generate-format-id: invalid kind rejected"

    local exit_code=0
    local err_output

    err_output=$(bash "$SOURCE_SCRIPT" widget INF FEAT GENL 2>&1) || exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        test_pass "invalid kind 'widget' exits non-zero"
    else
        test_fail "invalid kind 'widget' should exit non-zero"
    fi
    assert_contains "$err_output" "Error" "invalid kind produces error message"

    exit_code=0
    err_output=$(bash "$SOURCE_SCRIPT" Epic INF FEAT GENL 2>&1) || exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        test_pass "mixed-case kind 'Epic' exits non-zero"
    else
        test_fail "mixed-case kind 'Epic' should exit non-zero"
    fi
}

# ============================================================================
# TEST: Invalid code formats
# ============================================================================

test_invalid_codes() {
    test_section "generate-format-id: invalid AREA/TYPE/DOMAIN codes rejected"

    local err_output

    # Lowercase AREA (must be uppercase)
    err_output=$(bash "$SOURCE_SCRIPT" epic inf FEAT GENL 2>&1 || true)
    assert_contains "$err_output" "Error" "lowercase AREA produces error"

    # Single-char TYPE (minimum 2 chars)
    err_output=$(bash "$SOURCE_SCRIPT" epic INF F GENL 2>&1 || true)
    assert_contains "$err_output" "Error" "1-char TYPE produces error"

    # 5-char DOMAIN (maximum 4 chars)
    err_output=$(bash "$SOURCE_SCRIPT" epic INF FEAT TOOLNG 2>&1 || true)
    assert_contains "$err_output" "Error" "5-char DOMAIN produces error"

    # Digits in code (letters only)
    err_output=$(bash "$SOURCE_SCRIPT" epic IN1 FEAT GENL 2>&1 || true)
    assert_contains "$err_output" "Error" "AREA with digit produces error"
}

# ============================================================================
# TEST: Epic ID generation
# ============================================================================

test_epic_id_generation() {
    test_section "generate-format-id: epic ID generation"

    local id
    id=$(bash "$SOURCE_SCRIPT" epic TST UNIT TEST 2>/dev/null)

    assert_not_empty "$id" "epic ID output is not empty"
    assert_matches "$id" "^TST-EPC-UNIT-TEST-[0-9]{3}$" "epic ID matches format TST-EPC-UNIT-TEST-NNN"
    assert_contains "$id" "EPC" "epic ID contains 'EPC' kind code"
    assert_contains "$id" "TST-EPC-UNIT-TEST-" "epic ID contains correct prefix"

    # Verify sequence is zero-padded 3 digits
    local seq="${id##*-}"
    assert_matches "$seq" "^[0-9]{3}$" "sequence is exactly 3 digits"
    assert_not_equals "000" "$seq" "sequence starts from 001 (non-zero)"
}

# ============================================================================
# TEST: Task ID generation
# ============================================================================

test_task_id_generation() {
    test_section "generate-format-id: task ID generation"

    local id
    id=$(bash "$SOURCE_SCRIPT" task TST UNIT TEST 2>/dev/null)

    assert_not_empty "$id" "task ID output is not empty"
    assert_matches "$id" "^TST-TSK-UNIT-TEST-[0-9]{3}$" "task ID matches format TST-TSK-UNIT-TEST-NNN"
    assert_contains "$id" "TSK" "task ID contains 'TSK' kind code"
    assert_contains "$id" "TST-TSK-UNIT-TEST-" "task ID contains correct prefix"
}

# ============================================================================
# TEST: Kind code distinction (epic=EPC, task=TSK)
# ============================================================================

test_kind_code_distinction() {
    test_section "generate-format-id: epic uses EPC, task uses TSK"

    local epic_id task_id
    epic_id=$(bash "$SOURCE_SCRIPT" epic XYZ FEAT GENL 2>/dev/null)
    task_id=$(bash "$SOURCE_SCRIPT" task XYZ FEAT GENL 2>/dev/null)

    assert_contains "$epic_id" "-EPC-" "epic ID contains -EPC-"
    assert_contains "$task_id" "-TSK-" "task ID contains -TSK-"
    assert_not_contains "$epic_id" "-TSK-" "epic ID does not contain -TSK-"
    assert_not_contains "$task_id" "-EPC-" "task ID does not contain -EPC-"
}

# ============================================================================
# TEST: Output goes to stdout (not stderr)
# ============================================================================

test_output_to_stdout() {
    test_section "generate-format-id: ID written to stdout only"

    local stdout_only
    stdout_only=$(bash "$SOURCE_SCRIPT" epic TST FEAT CHCK 2>/dev/null)

    assert_not_empty "$stdout_only" "stdout is non-empty for valid input"
    assert_matches "$stdout_only" "^TST-EPC-FEAT-CHCK-[0-9]{3}$" "stdout matches ID format"
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
    echo -e "${BOLD}Testing: generate-format-id.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    test_script_exists
    test_help_flag
    test_arg_count_validation
    test_invalid_kind
    test_invalid_codes
    test_epic_id_generation
    test_task_id_generation
    test_kind_code_distinction
    test_output_to_stdout

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
