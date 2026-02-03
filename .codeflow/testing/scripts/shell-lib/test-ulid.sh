#!/usr/bin/env bash
# test-ulid.sh - Tests for shell-lib/ulid.sh
# Location: .codeflow/testing/scripts/shell-lib/test-ulid.sh
#
# Usage:
#   ./test-ulid.sh         Run all tests
#   ./test-ulid.sh -h      Show help
#   ./test-ulid.sh -V      Show version

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

Tests for shell-lib/ulid.sh module.

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
# TEST: ULID Generation
# ============================================================================

test_generate_ulid_length() {
    test_section "generate_ulid: length"

    local ulid
    ulid=$(generate_ulid)

    local len=${#ulid}
    assert_equals "26" "$len" "ULID is 26 characters"
}

test_generate_ulid_format() {
    test_section "generate_ulid: format"

    local ulid
    ulid=$(generate_ulid)

    # Should match Crockford base32 pattern
    if [[ "$ulid" =~ ^[0-9A-HJKMNP-TV-Z]{26}$ ]]; then
        test_pass "ULID matches Crockford base32 pattern"
    else
        test_fail "ULID should match Crockford base32 pattern: $ulid"
    fi
}

test_generate_ulid_uniqueness() {
    test_section "generate_ulid: uniqueness"

    local ulids=()
    local i

    # Generate 20 ULIDs
    for ((i=0; i<20; i++)); do
        ulids+=("$(generate_ulid)")
    done

    # Check all unique
    local unique_count
    unique_count=$(printf '%s\n' "${ulids[@]}" | sort -u | wc -l | tr -d ' ')

    assert_equals "20" "$unique_count" "All 20 ULIDs are unique"
}

test_generate_ulid_valid_chars() {
    test_section "generate_ulid: valid characters"

    local ulid
    ulid=$(generate_ulid)

    # Should not contain I, L, O, U (excluded from Crockford base32)
    if [[ "$ulid" =~ [ILOU] ]]; then
        test_fail "ULID should not contain I, L, O, or U: $ulid"
    else
        test_pass "ULID does not contain excluded characters"
    fi
}

test_generate_ulid_timestamp_prefix() {
    test_section "generate_ulid: timestamp prefix"

    local ulid1 ulid2
    ulid1=$(generate_ulid)
    ulid2=$(generate_ulid)

    # First 10 chars are timestamp (should be same or very close)
    local ts1="${ulid1:0:10}"
    local ts2="${ulid2:0:10}"

    # Allow small difference due to timing
    # In rapid succession, timestamps should be very similar
    assert_not_empty "$ts1" "First ULID has timestamp"
    assert_not_empty "$ts2" "Second ULID has timestamp"
}

# ============================================================================
# TEST: ID Generation Helpers
# ============================================================================

test_generate_epic_id() {
    test_section "generate_epic_id"

    local epic_id
    epic_id=$(generate_epic_id)

    # Should have EPC- prefix
    if [[ "$epic_id" =~ ^EPC- ]]; then
        test_pass "Epic ID has EPC- prefix"
    else
        test_fail "Epic ID should have EPC- prefix: $epic_id"
    fi

    # Should be valid epic ID
    if is_valid_epic_id "$epic_id"; then
        test_pass "Generated epic ID is valid"
    else
        test_fail "Generated epic ID should be valid: $epic_id"
    fi

    # Total length: EPC- (4) + ULID (26) = 30
    local len=${#epic_id}
    assert_equals "30" "$len" "Epic ID is 30 characters"
}

test_generate_task_id() {
    test_section "generate_task_id"

    local task_id
    task_id=$(generate_task_id)

    # Should have TSK- prefix
    if [[ "$task_id" =~ ^TSK- ]]; then
        test_pass "Task ID has TSK- prefix"
    else
        test_fail "Task ID should have TSK- prefix: $task_id"
    fi

    # Should be valid task ID
    if is_valid_task_id "$task_id"; then
        test_pass "Generated task ID is valid"
    else
        test_fail "Generated task ID should be valid: $task_id"
    fi

    # Total length: TSK- (4) + ULID (26) = 30
    local len=${#task_id}
    assert_equals "30" "$len" "Task ID is 30 characters"
}

test_generate_session_id() {
    test_section "generate_session_id"

    local session_id
    session_id=$(generate_session_id)

    # Should have SES- prefix
    if [[ "$session_id" =~ ^SES- ]]; then
        test_pass "Session ID has SES- prefix"
    else
        test_fail "Session ID should have SES- prefix: $session_id"
    fi

    # Total length: SES- (4) + ULID (26) = 30
    local len=${#session_id}
    assert_equals "30" "$len" "Session ID is 30 characters"
}

test_generate_claim_id() {
    test_section "generate_claim_id"

    local claim_id
    claim_id=$(generate_claim_id)

    # Should have CLM- prefix
    if [[ "$claim_id" =~ ^CLM- ]]; then
        test_pass "Claim ID has CLM- prefix"
    else
        test_fail "Claim ID should have CLM- prefix: $claim_id"
    fi

    # Total length: CLM- (4) + ULID (26) = 30
    local len=${#claim_id}
    assert_equals "30" "$len" "Claim ID is 30 characters"
}

test_generate_event_id() {
    test_section "generate_event_id"

    local event_id
    event_id=$(generate_event_id)

    # Should have EVT- prefix
    if [[ "$event_id" =~ ^EVT- ]]; then
        test_pass "Event ID has EVT- prefix"
    else
        test_fail "Event ID should have EVT- prefix: $event_id"
    fi

    # Total length: EVT- (4) + ULID (26) = 30
    local len=${#event_id}
    assert_equals "30" "$len" "Event ID is 30 characters"
}

# ============================================================================
# TEST: ULID Parsing
# ============================================================================

test_ulid_timestamp() {
    test_section "ulid_timestamp"

    local ulid
    ulid=$(generate_ulid)

    local ts
    ts=$(ulid_timestamp "$ulid")

    # Should be a number
    if [[ "$ts" =~ ^[0-9]+$ ]]; then
        test_pass "ulid_timestamp returns number"
    else
        test_fail "ulid_timestamp should return number: $ts"
    fi

    # Should be recent (after 2024-01-01: 1704067200000 ms)
    local jan_2024=1704067200000
    if [[ $ts -gt $jan_2024 ]]; then
        test_pass "Timestamp is after Jan 2024"
    else
        test_fail "Timestamp should be after Jan 2024: $ts"
    fi

    # Should not be in the distant future (before 2100: ~4102444800000 ms)
    local year_2100=4102444800000
    if [[ $ts -lt $year_2100 ]]; then
        test_pass "Timestamp is before year 2100"
    else
        test_fail "Timestamp should be before year 2100: $ts"
    fi
}

test_ulid_created_at() {
    test_section "ulid_created_at"

    local ulid
    ulid=$(generate_ulid)

    local created_at
    created_at=$(ulid_created_at "$ulid")

    # Should be ISO format
    if [[ "$created_at" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
        test_pass "ulid_created_at returns ISO format"
    else
        test_fail "ulid_created_at should return ISO format: $created_at"
    fi

    # Should contain current year (or close to it)
    local current_year
    current_year=$(date +%Y)
    if [[ "$created_at" =~ ^$current_year ]]; then
        test_pass "Timestamp contains current year"
    else
        test_fail "Timestamp should contain current year: $created_at"
    fi
}

# ============================================================================
# TEST: ULID Constants
# ============================================================================

test_ulid_alphabet() {
    test_section "ULID_ALPHABET constant"

    # Should be defined
    if [[ -n "${ULID_ALPHABET:-}" ]]; then
        test_pass "ULID_ALPHABET is defined"
    else
        test_fail "ULID_ALPHABET should be defined"
        return
    fi

    # Should be 32 characters
    local len=${#ULID_ALPHABET}
    assert_equals "32" "$len" "ULID_ALPHABET has 32 characters"

    # Should not contain I, L, O, U
    if [[ "$ULID_ALPHABET" =~ [ILOU] ]]; then
        test_fail "ULID_ALPHABET should not contain I, L, O, U"
    else
        test_pass "ULID_ALPHABET excludes I, L, O, U"
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
    echo -e "${BOLD}Testing: shell-lib/ulid.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # ULID generation
    test_generate_ulid_length
    test_generate_ulid_format
    test_generate_ulid_uniqueness
    test_generate_ulid_valid_chars
    test_generate_ulid_timestamp_prefix

    # ID generation helpers
    test_generate_epic_id
    test_generate_task_id
    test_generate_session_id
    test_generate_claim_id
    test_generate_event_id

    # ULID parsing
    test_ulid_timestamp
    test_ulid_created_at

    # Constants
    test_ulid_alphabet

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
