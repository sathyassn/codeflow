#!/usr/bin/env bash
# test-migrate-to-dual-id.sh - Tests for migrate-to-dual-id.sh
# Location: .codeflow/testing/scripts/db/test-migrate-to-dual-id.sh
#
# Tests .codeflow/scripts/db/migrate-to-dual-id.sh which migrates
# project management markdown files from single-ID to dual-ID format.
#
# NOTE: Only --dry-run and CLI behavior tests are included here.
# The actual migration modifies real project-management files and is a
# one-time operation; it is not safe to run without --dry-run in
# automated tests.
#
# Usage:
#   ./test-migrate-to-dual-id.sh           Run all tests
#   ./test-migrate-to-dual-id.sh -h        Show help
#   ./test-migrate-to-dual-id.sh -V        Show version

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

Tests for .codeflow/scripts/db/migrate-to-dual-id.sh

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

SOURCE_SCRIPT="$REPO_ROOT/.codeflow/scripts/db/migrate-to-dual-id.sh"

# ============================================================================
# TEST: Script file exists
# ============================================================================

test_script_exists() {
    test_section "migrate-to-dual-id: script exists"
    assert_file_exists "$SOURCE_SCRIPT" "migrate-to-dual-id.sh exists"
}

# ============================================================================
# TEST: Help flag
# ============================================================================

test_help_flag() {
    test_section "migrate-to-dual-id: --help and -h flags"

    assert_success "bash \"$SOURCE_SCRIPT\" --help" "--help exits 0"
    assert_success "bash \"$SOURCE_SCRIPT\" -h" "-h exits 0"

    local output
    output=$(bash "$SOURCE_SCRIPT" --help 2>&1)
    assert_contains "$output" "Usage:" "--help shows Usage"
    assert_contains "$output" "--dry-run" "--help describes --dry-run option"
    assert_contains "$output" "format_id" "--help describes dual-ID format"
}

# ============================================================================
# TEST: Invalid argument
# ============================================================================

test_invalid_arg() {
    test_section "migrate-to-dual-id: invalid argument handling"

    local exit_code=0
    local err_output
    err_output=$(bash "$SOURCE_SCRIPT" --unknown 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "unknown argument exits non-zero"
    else
        test_fail "unknown argument should exit non-zero"
    fi
    assert_contains "$err_output" "Error" "unknown argument produces error message"
}

# ============================================================================
# TEST: Dry-run mode executes cleanly
# ============================================================================

test_dry_run_mode() {
    test_section "migrate-to-dual-id: --dry-run mode (no file modifications)"

    local exit_code=0
    local output
    output=$(bash "$SOURCE_SCRIPT" --dry-run 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "--dry-run exits 0"
    else
        test_fail "--dry-run should exit 0 (got $exit_code)"
    fi

    assert_contains "$output" "DRY RUN MODE" "--dry-run shows DRY RUN MODE header"
    assert_contains "$output" "Migrating Epics" "--dry-run shows epic migration section"
    assert_contains "$output" "Migrating Tasks" "--dry-run shows task migration section"
    assert_contains "$output" "Migration Summary" "--dry-run shows summary"
    assert_contains "$output" "dry-run mode" "--dry-run confirms no files modified"
}

# ============================================================================
# TEST: Dry-run does not modify files
# ============================================================================

test_dry_run_no_modification() {
    test_section "migrate-to-dual-id: --dry-run makes no changes to disk"

    local pm_dir="$REPO_ROOT/project-management"

    # Create a reference timestamp file just before running dry-run
    local ref_file
    ref_file=$(mktemp "${TMPDIR:-/tmp}/codeflow-migrate-test-XXXXXX")

    # Run dry-run
    bash "$SOURCE_SCRIPT" --dry-run >/dev/null 2>&1

    # Check for any markdown files modified after ref_file was created
    local modified_files
    modified_files=$(find "$pm_dir" -type f -name "*.md" -newer "$ref_file" 2>/dev/null || true)
    rm -f "$ref_file"

    if [[ -z "$modified_files" ]]; then
        test_pass "--dry-run modifies no .md files"
    else
        test_fail "--dry-run unexpectedly modified files: $modified_files"
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
    echo -e "${BOLD}Testing: migrate-to-dual-id.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    test_script_exists
    test_help_flag
    test_invalid_arg
    test_dry_run_mode
    test_dry_run_no_modification

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
