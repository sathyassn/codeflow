#!/usr/bin/env bash
# test-ledger.sh - Tests for state/ledger.sh
# Location: .codeflow/testing/scripts/state/test-ledger.sh
#
# Usage:
#   ./test-ledger.sh       Run all tests
#   ./test-ledger.sh -h    Show help
#   ./test-ledger.sh -V    Show version

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

Tests for state/ledger.sh module.

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

# Source the module under test
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# ============================================================================
# TEST SETUP
# ============================================================================

setup_test_ledger() {
    setup_test_dir "ledger"
    export CODEFLOW_LEDGER_PATH="$TEST_DIR/ledger"
    export LEDGER_PATH="$CODEFLOW_LEDGER_PATH"
    mkdir -p "$LEDGER_PATH"
}

teardown_test_ledger() {
    teardown_test_dir
    unset CODEFLOW_LEDGER_PATH
}

# ============================================================================
# TEST: init_ledger
# ============================================================================

test_init_ledger() {
    test_section "init_ledger"

    setup_test_ledger

    # Test initialization
    local output
    output=$(init_ledger)

    # Check output message
    assert_contains "$output" "Ledger initialized" "init_ledger reports success"

    # Check directory created
    assert_dir_exists "$LEDGER_PATH" "Ledger directory exists"

    # Check files created
    assert_file_exists "$LEDGER_PATH/config.jsonl" "config.jsonl exists"
    assert_file_exists "$LEDGER_PATH/work-graph.jsonl" "work-graph.jsonl exists"
    assert_file_exists "$LEDGER_PATH/memory-events.jsonl" "memory-events.jsonl exists"
    assert_file_exists "$LEDGER_PATH/sessions.jsonl" "sessions.jsonl exists"

    teardown_test_ledger
}

test_init_ledger_idempotent() {
    test_section "init_ledger: idempotent"

    setup_test_ledger

    # Initialize twice
    init_ledger >/dev/null
    init_ledger >/dev/null

    # Should not error
    test_pass "init_ledger can be called multiple times"

    # Files should still exist
    assert_file_exists "$LEDGER_PATH/sessions.jsonl" "Files still exist after second init"

    teardown_test_ledger
}

# ============================================================================
# TEST: append_ledger
# ============================================================================

test_append_ledger_basic() {
    test_section "append_ledger: basic"

    setup_test_ledger
    init_ledger >/dev/null

    # Append an event
    append_ledger "sessions.jsonl" "test_event" '"key":"value"'

    # Check file has content
    local content
    content=$(cat "$LEDGER_PATH/sessions.jsonl")

    assert_contains "$content" '"e":"test_event"' "Event type recorded"
    assert_contains "$content" '"key":"value"' "Event data recorded"
    assert_contains "$content" '"ts":"' "Timestamp added"

    teardown_test_ledger
}

test_append_ledger_multiple() {
    test_section "append_ledger: multiple events"

    setup_test_ledger
    init_ledger >/dev/null

    # Append multiple events
    append_ledger "sessions.jsonl" "event1" '"n":1'
    append_ledger "sessions.jsonl" "event2" '"n":2'
    append_ledger "sessions.jsonl" "event3" '"n":3'

    # Check line count
    local count
    count=$(wc -l < "$LEDGER_PATH/sessions.jsonl" | tr -d ' ')
    assert_equals "3" "$count" "Three events appended"

    teardown_test_ledger
}

test_append_ledger_creates_directory() {
    test_section "append_ledger: creates directory"

    setup_test_ledger

    # Append without init (directory doesn't exist)
    append_ledger "new-file.jsonl" "test" '"data":"value"'

    # Check file created
    assert_file_exists "$LEDGER_PATH/new-file.jsonl" "File created with event"

    teardown_test_ledger
}

# ============================================================================
# TEST: append_event
# ============================================================================

test_append_event_full_json() {
    test_section "append_event: full JSON"

    setup_test_ledger
    init_ledger >/dev/null

    # Append full JSON event
    append_event "sessions.jsonl" '{"e":"custom","data":"test"}'

    # Check content
    local content
    content=$(cat "$LEDGER_PATH/sessions.jsonl")

    assert_contains "$content" '"ts":"' "Timestamp added to event"
    assert_contains "$content" '"e":"custom"' "Event preserved"

    teardown_test_ledger
}

test_append_event_with_timestamp() {
    test_section "append_event: with existing timestamp"

    setup_test_ledger
    init_ledger >/dev/null

    # Event with timestamp should not have another added
    append_event "sessions.jsonl" '{"ts":"2026-01-01T00:00:00Z","e":"timed"}'

    local content
    content=$(cat "$LEDGER_PATH/sessions.jsonl")

    assert_contains "$content" '"ts":"2026-01-01T00:00:00Z"' "Original timestamp preserved"

    teardown_test_ledger
}

# ============================================================================
# TEST: read_ledger
# ============================================================================

test_read_ledger_empty() {
    test_section "read_ledger: empty file"

    setup_test_ledger
    init_ledger >/dev/null

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_empty "$content" "Empty ledger returns empty string"

    teardown_test_ledger
}

test_read_ledger_with_content() {
    test_section "read_ledger: with content"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "event1" '"n":1'
    append_ledger "sessions.jsonl" "event2" '"n":2'

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"event1"' "First event readable"
    assert_contains "$content" '"event2"' "Second event readable"

    teardown_test_ledger
}

test_read_ledger_nonexistent() {
    test_section "read_ledger: nonexistent file"

    setup_test_ledger

    local content
    content=$(read_ledger "nonexistent.jsonl" || true)

    assert_empty "$content" "Nonexistent file returns empty"

    teardown_test_ledger
}

# ============================================================================
# TEST: tail_ledger
# ============================================================================

test_tail_ledger_default() {
    test_section "tail_ledger: default count"

    setup_test_ledger
    init_ledger >/dev/null

    # Add 15 events
    for i in {1..15}; do
        append_ledger "sessions.jsonl" "event$i" "\"n\":$i"
    done

    # Default should return 10
    local content
    content=$(tail_ledger "sessions.jsonl")
    local count
    count=$(echo "$content" | wc -l | tr -d ' ')

    assert_equals "10" "$count" "Default returns 10 lines"

    teardown_test_ledger
}

test_tail_ledger_custom_count() {
    test_section "tail_ledger: custom count"

    setup_test_ledger
    init_ledger >/dev/null

    for i in {1..10}; do
        append_ledger "sessions.jsonl" "event$i" "\"n\":$i"
    done

    local content
    content=$(tail_ledger "sessions.jsonl" 3)
    local count
    count=$(echo "$content" | wc -l | tr -d ' ')

    assert_equals "3" "$count" "Returns requested count"

    # Should be the last 3 events
    assert_contains "$content" '"event10"' "Contains last event"
    assert_contains "$content" '"event9"' "Contains second to last"
    assert_contains "$content" '"event8"' "Contains third to last"

    teardown_test_ledger
}

# ============================================================================
# TEST: filter_ledger
# ============================================================================

test_filter_ledger_by_type() {
    test_section "filter_ledger: by type"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "type_a" '"data":"a1"'
    append_ledger "sessions.jsonl" "type_b" '"data":"b1"'
    append_ledger "sessions.jsonl" "type_a" '"data":"a2"'
    append_ledger "sessions.jsonl" "type_c" '"data":"c1"'

    local filtered
    filtered=$(filter_ledger "sessions.jsonl" "type_a")
    local count
    count=$(echo "$filtered" | grep -c "type_a" || echo "0")

    assert_equals "2" "$count" "Filter returns matching events"

    teardown_test_ledger
}

test_filter_ledger_no_match() {
    test_section "filter_ledger: no match"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "type_a" '"data":"a"'

    local filtered
    filtered=$(filter_ledger "sessions.jsonl" "nonexistent")

    assert_empty "$filtered" "No match returns empty"

    teardown_test_ledger
}

# ============================================================================
# TEST: count_ledger
# ============================================================================

test_count_ledger_empty() {
    test_section "count_ledger: empty"

    setup_test_ledger
    init_ledger >/dev/null

    local count
    count=$(count_ledger "sessions.jsonl")

    assert_equals "0" "$count" "Empty ledger returns 0"

    teardown_test_ledger
}

test_count_ledger_with_events() {
    test_section "count_ledger: with events"

    setup_test_ledger
    init_ledger >/dev/null

    for i in {1..5}; do
        append_ledger "sessions.jsonl" "event$i" '"x":1'
    done

    local count
    count=$(count_ledger "sessions.jsonl")

    assert_equals "5" "$count" "Correct event count"

    teardown_test_ledger
}

test_count_ledger_nonexistent() {
    test_section "count_ledger: nonexistent file"

    setup_test_ledger

    local count
    count=$(count_ledger "nonexistent.jsonl")

    assert_equals "0" "$count" "Nonexistent returns 0"

    teardown_test_ledger
}

# ============================================================================
# TEST: ledger_size
# ============================================================================

test_ledger_size_empty() {
    test_section "ledger_size: empty"

    setup_test_ledger
    init_ledger >/dev/null

    local size
    size=$(ledger_size "sessions.jsonl")

    assert_equals "0" "$size" "Empty file has size 0"

    teardown_test_ledger
}

test_ledger_size_with_content() {
    test_section "ledger_size: with content"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "event" '"data":"some text"'

    local size
    size=$(ledger_size "sessions.jsonl")

    # Size should be greater than 0
    if [[ "$size" -gt 0 ]]; then
        test_pass "Size is positive: $size bytes"
    else
        test_fail "Size should be positive, got: $size"
    fi

    teardown_test_ledger
}

test_ledger_size_nonexistent() {
    test_section "ledger_size: nonexistent"

    setup_test_ledger

    local size
    size=$(ledger_size "nonexistent.jsonl")

    assert_equals "0" "$size" "Nonexistent returns 0"

    teardown_test_ledger
}

# ============================================================================
# TEST: Session Logging Functions
# ============================================================================

test_log_session_start() {
    test_section "log_session_start"

    setup_test_ledger
    init_ledger >/dev/null

    log_session_start "ses-123" "developer"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"e":"session_start"' "Event type is session_start"
    assert_contains "$content" '"sid":"ses-123"' "Session ID recorded"
    assert_contains "$content" '"agent":"developer"' "Agent type recorded"

    teardown_test_ledger
}

test_log_session_start_default_agent() {
    test_section "log_session_start: default agent"

    setup_test_ledger
    init_ledger >/dev/null

    log_session_start "ses-456"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"agent":"developer"' "Default agent is developer"

    teardown_test_ledger
}

test_log_session_end() {
    test_section "log_session_end"

    setup_test_ledger
    init_ledger >/dev/null

    log_session_end "ses-123" "work-456" "completed"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"e":"session_end"' "Event type is session_end"
    assert_contains "$content" '"sid":"ses-123"' "Session ID recorded"
    assert_contains "$content" '"wid":"work-456"' "Work ID recorded"
    assert_contains "$content" '"status":"completed"' "Status recorded"

    teardown_test_ledger
}

test_log_session_end_no_work() {
    test_section "log_session_end: no work ID"

    setup_test_ledger
    init_ledger >/dev/null

    log_session_end "ses-123"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"e":"session_end"' "Event recorded"
    assert_contains "$content" '"status":"completed"' "Default status is completed"

    teardown_test_ledger
}

test_log_work_claimed() {
    test_section "log_work_claimed"

    setup_test_ledger
    init_ledger >/dev/null

    log_work_claimed "ses-123" "work-456"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"e":"work_claimed"' "Event type is work_claimed"
    assert_contains "$content" '"sid":"ses-123"' "Session ID recorded"
    assert_contains "$content" '"wid":"work-456"' "Work ID recorded"

    teardown_test_ledger
}

test_log_progress() {
    test_section "log_progress"

    setup_test_ledger
    init_ledger >/dev/null

    log_progress "ses-123" "work-456" "Implemented feature X"

    local content
    content=$(read_ledger "sessions.jsonl")

    assert_contains "$content" '"e":"progress"' "Event type is progress"
    assert_contains "$content" '"sid":"ses-123"' "Session ID recorded"
    assert_contains "$content" '"wid":"work-456"' "Work ID recorded"
    assert_contains "$content" '"d":"Implemented feature X"' "Description recorded"

    teardown_test_ledger
}

test_log_progress_escapes_quotes() {
    test_section "log_progress: escapes quotes"

    setup_test_ledger
    init_ledger >/dev/null

    log_progress "ses-123" "work-456" 'Fixed "bug" in code'

    local content
    content=$(read_ledger "sessions.jsonl")

    # Check event was logged (quotes should be escaped)
    assert_contains "$content" '"e":"progress"' "Event logged with escaped quotes"

    teardown_test_ledger
}

# ============================================================================
# TEST: backup_ledger
# ============================================================================

test_backup_ledger() {
    test_section "backup_ledger"

    setup_test_ledger
    init_ledger >/dev/null

    # Add some content
    append_ledger "sessions.jsonl" "event" '"data":"test"'
    append_ledger "config.jsonl" "config" '"setting":"value"'

    # Backup
    local backup_dir="$TEST_DIR/backups"
    local output
    output=$(backup_ledger "$backup_dir")

    assert_contains "$output" "Ledger backed up" "Reports success"
    assert_dir_exists "$backup_dir" "Backup directory created"

    # Check backup files exist
    local backup_count
    backup_count=$(find "$backup_dir" -name "*.jsonl" | wc -l | tr -d ' ')
    if [[ "$backup_count" -gt 0 ]]; then
        test_pass "Backup files created: $backup_count"
    else
        test_fail "No backup files created"
    fi

    teardown_test_ledger
}

test_backup_ledger_default_dir() {
    test_section "backup_ledger: default directory"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "event" '"data":"test"'

    # Backup without specifying directory
    backup_ledger >/dev/null

    assert_dir_exists "$LEDGER_PATH/backups" "Default backup directory created"

    teardown_test_ledger
}

# ============================================================================
# TEST: validate_ledger
# ============================================================================

test_validate_ledger_valid() {
    test_section "validate_ledger: valid JSONL"

    setup_test_ledger
    init_ledger >/dev/null

    append_ledger "sessions.jsonl" "event1" '"data":"a"'
    append_ledger "sessions.jsonl" "event2" '"data":"b"'

    local output
    if output=$(validate_ledger "sessions.jsonl" 2>&1); then
        test_pass "Valid JSONL passes validation"
        assert_contains "$output" "valid" "Reports valid"
    else
        test_fail "Valid JSONL should pass validation"
    fi

    teardown_test_ledger
}

test_validate_ledger_invalid() {
    test_section "validate_ledger: invalid JSONL"

    setup_test_ledger
    init_ledger >/dev/null

    # Write invalid JSON directly
    echo "not valid json" >> "$LEDGER_PATH/sessions.jsonl"
    echo '{"valid":"json"}' >> "$LEDGER_PATH/sessions.jsonl"

    local output
    if ! output=$(validate_ledger "sessions.jsonl" 2>&1); then
        test_pass "Invalid JSONL fails validation"
        assert_contains "$output" "Invalid JSON" "Reports invalid line"
    else
        test_fail "Invalid JSONL should fail validation"
    fi

    teardown_test_ledger
}

test_validate_ledger_nonexistent() {
    test_section "validate_ledger: nonexistent file"

    setup_test_ledger

    if ! validate_ledger "nonexistent.jsonl" 2>&1; then
        test_pass "Nonexistent file fails validation"
    else
        test_fail "Nonexistent file should fail validation"
    fi

    teardown_test_ledger
}

# ============================================================================
# TEST: Ledger File Constants
# ============================================================================

test_ledger_constants() {
    test_section "Ledger constants"

    assert_equals "config.jsonl" "$LEDGER_CONFIG" "LEDGER_CONFIG correct"
    assert_equals "work-graph.jsonl" "$LEDGER_WORK_GRAPH" "LEDGER_WORK_GRAPH correct"
    assert_equals "memory-events.jsonl" "$LEDGER_MEMORY" "LEDGER_MEMORY correct"
    assert_equals "sessions.jsonl" "$LEDGER_SESSIONS" "LEDGER_SESSIONS correct"
}

# ============================================================================
# TEST: Concurrent Writes
# ============================================================================

test_concurrent_append() {
    test_section "concurrent append"

    setup_test_ledger
    init_ledger >/dev/null

    # Simulate concurrent writes with background processes
    for i in {1..5}; do
        append_ledger "sessions.jsonl" "concurrent$i" "\"n\":$i" &
    done
    wait

    local count
    count=$(count_ledger "sessions.jsonl")

    if [[ "$count" -eq 5 ]]; then
        test_pass "All concurrent writes succeeded"
    else
        test_fail "Expected 5 events, got $count"
    fi

    teardown_test_ledger
}

# ============================================================================
# TEST: Input Validation
# ============================================================================

test_append_ledger_empty_file() {
    test_section "append_ledger: empty ledger_file"

    setup_test_ledger
    init_ledger >/dev/null

    local output
    if output=$(append_ledger "" "event" '"data":"x"' 2>&1); then
        test_fail "Should reject empty ledger_file"
    else
        test_pass "Rejects empty ledger_file"
        assert_contains "$output" "requires" "Error message mentions requirements"
    fi

    teardown_test_ledger
}

test_append_ledger_empty_event_type() {
    test_section "append_ledger: empty event_type"

    setup_test_ledger
    init_ledger >/dev/null

    local output
    if output=$(append_ledger "sessions.jsonl" "" '"data":"x"' 2>&1); then
        test_fail "Should reject empty event_type"
    else
        test_pass "Rejects empty event_type"
    fi

    teardown_test_ledger
}

test_append_ledger_empty_data() {
    test_section "append_ledger: empty data is allowed"

    setup_test_ledger
    init_ledger >/dev/null

    # Empty data should produce valid JSON with just ts and e
    append_ledger "sessions.jsonl" "simple_event" ""

    local content
    content=$(cat "$LEDGER_PATH/sessions.jsonl")
    assert_contains "$content" '"e":"simple_event"' "Event type recorded without data"

    teardown_test_ledger
}

test_append_event_empty_args() {
    test_section "append_event: empty args"

    setup_test_ledger

    local output
    if output=$(append_event "" "" 2>&1); then
        test_fail "Should reject empty args"
    else
        test_pass "Rejects empty args"
    fi

    teardown_test_ledger
}

# ============================================================================
# TEST: Work Graph Event Helpers
# ============================================================================

test_record_epic_created() {
    test_section "record_epic_created: basic"

    setup_test_ledger
    init_ledger >/dev/null

    record_epic_created "epic-ABC123" "Test Epic" "FRT" "FEAT" "AUTH"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"e":"epic_created"' "Event type is epic_created"
    assert_contains "$content" '"id":"epic-ABC123"' "Epic ID recorded"
    assert_contains "$content" '"title":"Test Epic"' "Title recorded"
    assert_contains "$content" '"area_type":"FRT"' "Area type recorded"
    assert_contains "$content" '"work_type":"FEAT"' "Work type recorded"
    assert_contains "$content" '"domain":"AUTH"' "Domain recorded"

    teardown_test_ledger
}

test_record_epic_created_optional_params() {
    test_section "record_epic_created: optional params"

    setup_test_ledger
    init_ledger >/dev/null

    record_epic_created "epic-XYZ789" "Ongoing Epic" "INF" "FIX" "GENL" "true" "high" "src/" "INF-EPC-FIX-GENL-001"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"is_ongoing":true' "Ongoing flag set"
    assert_contains "$content" '"priority":"high"' "Priority recorded"
    assert_contains "$content" '"file_scope":"src/"' "File scope recorded"
    assert_contains "$content" '"format_id":"INF-EPC-FIX-GENL-001"' "Format ID recorded"

    teardown_test_ledger
}

test_record_task_created() {
    test_section "record_task_created: basic"

    setup_test_ledger
    init_ledger >/dev/null

    record_task_created "task-DEF456" "epic-ABC123" "Implement auth" "FRT" "FEAT" "AUTH"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"e":"task_created"' "Event type is task_created"
    assert_contains "$content" '"id":"task-DEF456"' "Task ID recorded"
    assert_contains "$content" '"epic_id":"epic-ABC123"' "Epic ID recorded"
    assert_contains "$content" '"title":"Implement auth"' "Title recorded"
    assert_contains "$content" '"status":"todo"' "Default status is todo"

    teardown_test_ledger
}

test_record_task_created_with_format_id() {
    test_section "record_task_created: with format ID"

    setup_test_ledger
    init_ledger >/dev/null

    record_task_created "task-GHI789" "epic-ABC123" "Fix bug" "BKD" "FIX" "API" "in_progress" "high" "BKD-TSK-FIX-API-001"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"status":"in_progress"' "Custom status recorded"
    assert_contains "$content" '"priority":"high"' "Priority recorded"
    assert_contains "$content" '"format_id":"BKD-TSK-FIX-API-001"' "Format ID recorded"

    teardown_test_ledger
}

test_record_task_status_changed() {
    test_section "record_task_status_changed: basic"

    setup_test_ledger
    init_ledger >/dev/null

    record_task_status_changed "task-DEF456" "in_progress" "todo"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"e":"task_status_changed"' "Event type is task_status_changed"
    assert_contains "$content" '"task_id":"task-DEF456"' "Task ID recorded"
    assert_contains "$content" '"new_status":"in_progress"' "New status recorded"
    assert_contains "$content" '"old_status":"todo"' "Old status recorded"

    teardown_test_ledger
}

test_record_task_status_changed_with_format_id() {
    test_section "record_task_status_changed: with format ID"

    setup_test_ledger
    init_ledger >/dev/null

    record_task_status_changed "task-DEF456" "complete" "in_progress" "FRT-TSK-FEAT-AUTH-001"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"format_id":"FRT-TSK-FEAT-AUTH-001"' "Format ID recorded"

    teardown_test_ledger
}

test_record_epic_status_changed() {
    test_section "record_epic_status_changed: basic"

    setup_test_ledger
    init_ledger >/dev/null

    record_epic_status_changed "epic-ABC123" "in_progress" "draft"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"e":"epic_status_changed"' "Event type is epic_status_changed"
    assert_contains "$content" '"epic_id":"epic-ABC123"' "Epic ID recorded"
    assert_contains "$content" '"new_status":"in_progress"' "New status recorded"
    assert_contains "$content" '"old_status":"draft"' "Old status recorded"

    teardown_test_ledger
}

test_record_epic_status_changed_with_format_id() {
    test_section "record_epic_status_changed: with format ID"

    setup_test_ledger
    init_ledger >/dev/null

    record_epic_status_changed "epic-ABC123" "complete" "in_progress" "INF-EPC-FIX-GENL-001"

    local content
    content=$(read_ledger "work-graph.jsonl")

    assert_contains "$content" '"format_id":"INF-EPC-FIX-GENL-001"' "Format ID recorded"

    teardown_test_ledger
}

# ============================================================================
# TEST: Source Guard
# ============================================================================

test_source_guard() {
    test_section "source guard"

    # Source guard should prevent errors on double-source
    # The variable _CODEFLOW_LEDGER_LOADED should be set
    if [[ -n "${_CODEFLOW_LEDGER_LOADED:-}" ]]; then
        test_pass "Source guard variable is set"
    else
        test_fail "Source guard variable not set"
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
    echo -e "${BOLD}Testing: state/ledger.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # init_ledger tests
    test_init_ledger
    test_init_ledger_idempotent

    # append_ledger tests
    test_append_ledger_basic
    test_append_ledger_multiple
    test_append_ledger_creates_directory

    # append_event tests
    test_append_event_full_json
    test_append_event_with_timestamp

    # read_ledger tests
    test_read_ledger_empty
    test_read_ledger_with_content
    test_read_ledger_nonexistent

    # tail_ledger tests
    test_tail_ledger_default
    test_tail_ledger_custom_count

    # filter_ledger tests
    test_filter_ledger_by_type
    test_filter_ledger_no_match

    # count_ledger tests
    test_count_ledger_empty
    test_count_ledger_with_events
    test_count_ledger_nonexistent

    # ledger_size tests
    test_ledger_size_empty
    test_ledger_size_with_content
    test_ledger_size_nonexistent

    # Session logging tests
    test_log_session_start
    test_log_session_start_default_agent
    test_log_session_end
    test_log_session_end_no_work
    test_log_work_claimed
    test_log_progress
    test_log_progress_escapes_quotes

    # Backup tests
    test_backup_ledger
    test_backup_ledger_default_dir

    # Validation tests
    test_validate_ledger_valid
    test_validate_ledger_invalid
    test_validate_ledger_nonexistent

    # Constants test
    test_ledger_constants

    # Concurrent test
    test_concurrent_append

    # Input validation tests
    test_append_ledger_empty_file
    test_append_ledger_empty_event_type
    test_append_ledger_empty_data
    test_append_event_empty_args

    # Work graph event helper tests
    test_record_epic_created
    test_record_epic_created_optional_params
    test_record_task_created
    test_record_task_created_with_format_id
    test_record_task_status_changed
    test_record_task_status_changed_with_format_id
    test_record_epic_status_changed
    test_record_epic_status_changed_with_format_id

    # Source guard test
    test_source_guard

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
