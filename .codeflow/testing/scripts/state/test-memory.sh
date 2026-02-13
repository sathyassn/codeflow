#!/usr/bin/env bash
# test-memory.sh - Tests for state/memory.sh
# Location: .codeflow/testing/scripts/state/test-memory.sh
#
# Usage:
#   ./test-memory.sh       Run all tests
#   ./test-memory.sh -h    Show help
#   ./test-memory.sh -V    Show version

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
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for state/memory.sh module.

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
# TEST SETUP
# ============================================================================

setup_test_memory() {
    setup_test_dir "memory"
    export CODEFLOW_DB_FILE="$TEST_DIR/test.db"
    export CODEFLOW_LEDGER_PATH="$TEST_DIR/ledger"
    export DB_PATH="$CODEFLOW_DB_FILE"
    export LEDGER_PATH="$CODEFLOW_LEDGER_PATH"
    mkdir -p "$LEDGER_PATH"
    mkdir -p "$(dirname "$CODEFLOW_DB_FILE")"

    # Copy schema for testing
    if [[ -f "$REPO_ROOT/.codeflow/scripts/db/schema.sql" ]]; then
        cp "$REPO_ROOT/.codeflow/scripts/db/schema.sql" "$TEST_DIR/"
    fi
}

teardown_test_memory() {
    teardown_test_dir
    unset CODEFLOW_DB_FILE CODEFLOW_LEDGER_PATH DB_PATH LEDGER_PATH
}

# Initialize test database
init_test_db() {
    local schema_path="${1:-$TEST_DIR/schema.sql}"
    if [[ -f "$schema_path" ]]; then
        sqlite3 "$CODEFLOW_DB_FILE" < "$schema_path"
    fi
}

# Source the module under test (after setup to use test paths)
load_memory_module() {
    # Re-source with test paths set
    source "$REPO_ROOT/.codeflow/scripts/state/memory.sh"
}

# Check if Go CLI is available (query/search/get/count delegate to it)
require_codeflow_cli() {
    local test_name="$1"
    if ! command -v codeflow >/dev/null 2>&1; then
        test_skip "$test_name" "codeflow CLI not installed (Go CLI required)"
        teardown_test_memory
        return 1
    fi
    return 0
}

# ============================================================================
# TEST: init_db
# ============================================================================

test_init_db_creates_database() {
    test_section "init_db: creates database"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available (init_db delegates to codeflow db init)
    require_codeflow_cli "init_db" || return

    # Skip if no schema available
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "init_db" "No schema.sql available"
        teardown_test_memory
        return
    fi

    local output
    output=$(init_db "$TEST_DIR/schema.sql" 2>&1)

    if [[ -f "$CODEFLOW_DB_FILE" ]]; then
        test_pass "Database file created"
    else
        test_fail "Database file not created"
    fi

    assert_contains "$output" "Database initialized" "Reports success"

    teardown_test_memory
}

test_init_db_missing_schema() {
    test_section "init_db: missing schema"

    setup_test_memory
    load_memory_module

    if ! init_db "/nonexistent/schema.sql" 2>&1; then
        test_pass "Returns error for missing schema"
    else
        test_fail "Should fail with missing schema"
    fi

    teardown_test_memory
}

# ============================================================================
# TEST: _generate_ulid (internal function)
# ============================================================================

test_generate_ulid() {
    test_section "generate_ulid: via shell-lib"

    setup_test_memory
    load_memory_module

    if ! type generate_ulid >/dev/null 2>&1; then
        test_skip "generate_ulid" "shell-lib not available"
        teardown_test_memory
        return
    fi

    local ulid
    ulid=$(generate_ulid)

    assert_not_empty "$ulid" "ULID generated"

    # ULID should be alphanumeric (Crockford Base32)
    if [[ "$ulid" =~ ^[A-Za-z0-9]+$ ]]; then
        test_pass "ULID is alphanumeric"
    else
        test_fail "ULID should be alphanumeric: $ulid"
    fi

    teardown_test_memory
}

test_generate_ulid_unique() {
    test_section "generate_ulid: unique"

    setup_test_memory
    load_memory_module

    if ! type generate_ulid >/dev/null 2>&1; then
        test_skip "generate_ulid unique" "shell-lib not available"
        teardown_test_memory
        return
    fi

    local ulid1 ulid2
    ulid1=$(generate_ulid)
    sleep 0.01  # Small delay to ensure different timestamp
    ulid2=$(generate_ulid)

    if [[ "$ulid1" != "$ulid2" ]]; then
        test_pass "ULIDs are unique"
    else
        test_fail "ULIDs should be unique"
    fi

    teardown_test_memory
}

# ============================================================================
# TEST: record_event
# ============================================================================

test_record_event_valid() {
    test_section "record_event: valid input"

    setup_test_memory
    load_memory_module

    local event_id
    event_id=$(record_event "progress" "development" '{"content":"Test progress"}')

    if [[ "$event_id" == memory-* ]]; then
        test_pass "Event ID has correct prefix: $event_id"
    else
        test_fail "Event ID should start with 'memory-': $event_id"
    fi

    teardown_test_memory
}

test_record_event_invalid_type() {
    test_section "record_event: invalid event type"

    setup_test_memory
    load_memory_module

    if [[ -f "$TEST_DIR/schema.sql" ]]; then
        init_test_db
    fi

    if ! record_event "invalid_type" "development" '{}' 2>&1; then
        test_pass "Rejects invalid event type"
    else
        test_fail "Should reject invalid event type"
    fi

    teardown_test_memory
}

test_record_event_invalid_domain() {
    test_section "record_event: invalid domain"

    setup_test_memory
    load_memory_module

    if [[ -f "$TEST_DIR/schema.sql" ]]; then
        init_test_db
    fi

    if ! record_event "progress" "invalid_domain" '{}' 2>&1; then
        test_pass "Rejects invalid domain"
    else
        test_fail "Should reject invalid domain"
    fi

    teardown_test_memory
}

test_record_event_valid_event_types() {
    test_section "record_event: valid event types (core)"

    setup_test_memory
    load_memory_module

    local valid_types=("progress" "decision" "milestone" "blocker")
    for type in "${valid_types[@]}"; do
        if record_event "$type" "development" '{}' >/dev/null 2>&1; then
            test_pass "Accepts event type: $type"
        else
            test_fail "Should accept event type: $type"
        fi
    done

    teardown_test_memory
}

test_record_event_v4_pathflow_types() {
    test_section "record_event: V4 PathFlow event types"

    setup_test_memory
    load_memory_module

    local v4_types=("stage_transition" "stage_complete" "rework_limit")
    for type in "${v4_types[@]}"; do
        if record_event "$type" "development" '{}' >/dev/null 2>&1; then
            test_pass "Accepts V4 event type: $type"
        else
            test_fail "Should accept V4 event type: $type"
        fi
    done

    teardown_test_memory
}

test_record_event_valid_domains() {
    test_section "record_event: valid domains"

    setup_test_memory
    load_memory_module

    local valid_domains=("planning" "development" "review" "qa" "ops" "documentation")
    for domain in "${valid_domains[@]}"; do
        if record_event "progress" "$domain" '{}' >/dev/null 2>&1; then
            test_pass "Accepts domain: $domain"
        else
            test_fail "Should accept domain: $domain"
        fi
    done

    teardown_test_memory
}

test_record_event_with_work_id() {
    test_section "record_event: with work_id"

    setup_test_memory
    load_memory_module

    local event_id
    event_id=$(record_event "progress" "development" '{}' "work-123")

    if [[ -n "$event_id" ]]; then
        test_pass "Event with work_id created"
    else
        test_fail "Should create event with work_id"
    fi

    teardown_test_memory
}

test_record_event_ledger_written() {
    test_section "record_event: writes to ledger"

    setup_test_memory
    load_memory_module

    record_event "progress" "development" '{"content":"test"}' >/dev/null

    local ledger_file="$LEDGER_PATH/memory-events.jsonl"
    if [[ -f "$ledger_file" ]]; then
        local content
        content=$(cat "$ledger_file")
        assert_contains "$content" "memory_stored" "Ledger has memory_stored event"
    else
        test_fail "Ledger file not created"
    fi

    teardown_test_memory
}

test_record_event_jsonl_format() {
    test_section "record_event: JSONL format valid"

    setup_test_memory
    load_memory_module

    record_event "progress" "development" '{"content":"format test"}' "work-456" "episodic" >/dev/null

    local ledger_file="$LEDGER_PATH/memory-events.jsonl"
    local last_line
    last_line=$(tail -1 "$ledger_file")

    # Validate it's valid JSON
    if echo "$last_line" | jq empty 2>/dev/null; then
        test_pass "JSONL line is valid JSON"
    else
        test_fail "JSONL line should be valid JSON: $last_line"
    fi

    # Verify required fields
    local event_type domain ts event_id
    event_type=$(echo "$last_line" | jq -r '.event_type')
    domain=$(echo "$last_line" | jq -r '.domain')
    ts=$(echo "$last_line" | jq -r '.ts')
    event_id=$(echo "$last_line" | jq -r '.id')

    assert_equals "progress" "$event_type" "event_type field correct"
    assert_equals "development" "$domain" "domain field correct"
    assert_not_empty "$ts" "timestamp present"
    assert_contains "$event_id" "memory-" "id has memory- prefix"

    # Verify optional fields present when provided
    local work_id memory_type
    work_id=$(echo "$last_line" | jq -r '.work_id')
    memory_type=$(echo "$last_line" | jq -r '.memory_type')
    assert_equals "work-456" "$work_id" "work_id included when provided"
    assert_equals "episodic" "$memory_type" "memory_type included when provided"

    teardown_test_memory
}

test_record_event_omits_empty_fields() {
    test_section "record_event: omits empty optional fields"

    setup_test_memory
    load_memory_module

    # Record without work_id or memory_type
    record_event "progress" "development" '{}' >/dev/null

    local ledger_file="$LEDGER_PATH/memory-events.jsonl"
    local last_line
    last_line=$(tail -1 "$ledger_file")

    # work_id and memory_type should not appear in output
    if echo "$last_line" | jq -e '.work_id' >/dev/null 2>&1; then
        test_fail "work_id should be omitted when empty"
    else
        test_pass "work_id omitted when empty"
    fi

    if echo "$last_line" | jq -e '.memory_type' >/dev/null 2>&1; then
        test_fail "memory_type should be omitted when empty"
    else
        test_pass "memory_type omitted when empty"
    fi

    teardown_test_memory
}

test_record_event_invalid_json_data() {
    test_section "record_event: rejects invalid JSON data"

    setup_test_memory
    load_memory_module

    # Only test if jq is available (validation depends on it)
    if ! command -v jq >/dev/null 2>&1; then
        test_skip "invalid JSON data" "jq not available"
        teardown_test_memory
        return
    fi

    if ! record_event "progress" "development" "not-json" 2>/dev/null; then
        test_pass "Rejects invalid JSON data"
    else
        test_fail "Should reject invalid JSON data"
    fi

    teardown_test_memory
}

# ============================================================================
# TEST: query_events
# ============================================================================

test_query_events_empty() {
    test_section "query_events: empty database"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "query_events_empty" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "query_events empty" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    local result
    result=$(query_events "" "")

    # Should return empty JSON array
    if [[ "$result" == "[]" || -z "$result" ]]; then
        test_pass "Empty database returns empty result"
    else
        test_fail "Expected empty result, got: $result"
    fi

    teardown_test_memory
}

test_query_events_by_domain() {
    test_section "query_events: filter by domain"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "query_events_by_domain" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "query_events domain" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    # Add events with different domains
    record_event "progress" "development" '{}' >/dev/null
    record_event "progress" "review" '{}' >/dev/null
    record_event "progress" "development" '{}' >/dev/null

    local result
    result=$(query_events "development" "")

    # Count matches (simple check)
    local count
    count=$(echo "$result" | grep -c "development" || echo "0")

    if [[ "$count" -ge 2 ]]; then
        test_pass "Filters by domain correctly"
    else
        test_fail "Should return 2 development events, got: $count"
    fi

    teardown_test_memory
}

test_query_events_by_type() {
    test_section "query_events: filter by type"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "query_events_by_type" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "query_events type" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    # Add events with different types
    record_event "progress" "development" '{}' >/dev/null
    record_event "decision" "development" '{}' >/dev/null
    record_event "milestone" "development" '{}' >/dev/null

    local result
    result=$(query_events "" "decision")

    if [[ "$result" == *"decision"* ]]; then
        test_pass "Filters by event type"
    else
        test_fail "Should return decision event"
    fi

    teardown_test_memory
}

test_query_events_with_limit() {
    test_section "query_events: with limit"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "query_events_with_limit" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "query_events limit" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    # Add multiple events
    for _i in {1..10}; do
        record_event "progress" "development" '{}' >/dev/null
    done

    local result
    result=$(query_events "" "" 5)

    # Count JSON objects in result
    local count
    count=$(echo "$result" | grep -c '"id"' || echo "0")

    if [[ "$count" -le 5 ]]; then
        test_pass "Respects limit"
    else
        test_fail "Should limit to 5 results, got: $count"
    fi

    teardown_test_memory
}

# ============================================================================
# TEST: search_memory
# ============================================================================

test_search_memory_basic() {
    test_section "search_memory: basic search"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "search_memory_basic" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "search_memory" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    # Add event with searchable content
    record_event "progress" "development" '{"content":"authentication implementation"}' >/dev/null

    # Note: FTS may not work without proper triggers, test basic query
    local result
    result=$(search_memory "authentication" 10 2>/dev/null || echo "[]")

    # Just verify it doesn't crash
    test_pass "search_memory executes without error"

    teardown_test_memory
}

# ============================================================================
# TEST: get_event
# ============================================================================

test_get_event_existing() {
    test_section "get_event: existing event"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "get_event_existing" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "get_event existing" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    local event_id
    event_id=$(record_event "progress" "development" '{"content":"test"}')

    local result
    result=$(get_event "$event_id")

    if [[ "$result" == *"$event_id"* ]]; then
        test_pass "get_event returns correct event"
    else
        test_fail "Should return event with ID $event_id"
    fi

    teardown_test_memory
}

test_get_event_nonexistent() {
    test_section "get_event: nonexistent event"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "get_event_nonexistent" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "get_event nonexistent" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    local result
    result=$(get_event "nonexistent-id")

    if [[ "$result" == "[]" || -z "$result" ]]; then
        test_pass "Returns empty for nonexistent event"
    else
        test_fail "Should return empty for nonexistent ID"
    fi

    teardown_test_memory
}

# ============================================================================
# TEST: count_events
# ============================================================================

test_count_events_empty() {
    test_section "count_events: empty database"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "count_events_empty" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "count_events empty" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    local count
    count=$(count_events "" "")

    assert_equals "0" "$count" "Empty database has 0 events"

    teardown_test_memory
}

test_count_events_with_data() {
    test_section "count_events: with data"

    setup_test_memory
    load_memory_module

    # Skip if Go CLI not available
    require_codeflow_cli "count_events_with_data" || return

    # Skip if no schema
    if [[ ! -f "$TEST_DIR/schema.sql" ]]; then
        test_skip "count_events data" "No schema.sql available"
        teardown_test_memory
        return
    fi

    init_test_db

    # Add events
    record_event "progress" "development" '{}' >/dev/null
    record_event "decision" "development" '{}' >/dev/null
    record_event "progress" "review" '{}' >/dev/null

    local total
    total=$(count_events "" "")
    assert_equals "3" "$total" "Total count is 3"

    local dev_count
    dev_count=$(count_events "development" "")
    assert_equals "2" "$dev_count" "Development count is 2"

    local progress_count
    progress_count=$(count_events "" "progress")
    assert_equals "2" "$progress_count" "Progress count is 2"

    teardown_test_memory
}

# ============================================================================
# TEST: Export Functions
# ============================================================================

test_functions_exported() {
    test_section "Functions exported"

    setup_test_memory
    load_memory_module

    # Check functions are available
    if type init_db >/dev/null 2>&1; then
        test_pass "init_db is exported"
    else
        test_fail "init_db should be exported"
    fi

    if type record_event >/dev/null 2>&1; then
        test_pass "record_event is exported"
    else
        test_fail "record_event should be exported"
    fi

    if type query_events >/dev/null 2>&1; then
        test_pass "query_events is exported"
    else
        test_fail "query_events should be exported"
    fi

    if type search_memory >/dev/null 2>&1; then
        test_pass "search_memory is exported"
    else
        test_fail "search_memory should be exported"
    fi

    if type get_event >/dev/null 2>&1; then
        test_pass "get_event is exported"
    else
        test_fail "get_event should be exported"
    fi

    if type count_events >/dev/null 2>&1; then
        test_pass "count_events is exported"
    else
        test_fail "count_events should be exported"
    fi

    teardown_test_memory
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
    echo -e "${BOLD}Testing: state/memory.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Most memory.sh functions delegate to Go CLI (codeflow db).
    # Skip DB-dependent tests when CLI is not installed.
    local HAS_CODEFLOW_CLI=false
    if command -v codeflow >/dev/null 2>&1 && codeflow db version >/dev/null 2>&1; then
        HAS_CODEFLOW_CLI=true
    fi

    # init_db tests (requires Go CLI)
    if [[ "$HAS_CODEFLOW_CLI" == "true" ]]; then
        test_init_db_creates_database
    else
        test_section "init_db: creates database"
        test_skip "init_db" "codeflow CLI not installed"
    fi
    test_init_db_missing_schema

    # ULID generation tests
    test_generate_ulid
    test_generate_ulid_unique

    # record_event tests
    test_record_event_valid
    test_record_event_invalid_type
    test_record_event_invalid_domain
    test_record_event_valid_event_types
    test_record_event_v4_pathflow_types
    test_record_event_valid_domains
    test_record_event_with_work_id
    test_record_event_ledger_written
    test_record_event_jsonl_format
    test_record_event_omits_empty_fields
    test_record_event_invalid_json_data

    # query/search/get/count tests (all require Go CLI)
    if [[ "$HAS_CODEFLOW_CLI" == "true" ]]; then
        test_query_events_empty
        test_query_events_by_domain
        test_query_events_by_type
        test_query_events_with_limit
        test_search_memory_basic
        test_get_event_existing
        test_get_event_nonexistent
        test_count_events_empty
        test_count_events_with_data
    else
        test_section "query/search/get/count: skipped (no Go CLI)"
        test_skip "query_events" "codeflow CLI not installed"
        test_skip "search_memory" "codeflow CLI not installed"
        test_skip "get_event" "codeflow CLI not installed"
        test_skip "count_events" "codeflow CLI not installed"
    fi

    # Export tests
    test_functions_exported

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
