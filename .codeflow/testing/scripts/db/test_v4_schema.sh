#!/usr/bin/env bash
# test_v4_schema.sh - Tests for V4 PathFlow schema additions
# Location: .codeflow/testing/scripts/db/test_v4_schema.sh
#
# Tests V4 columns, CHECK constraints, and indexes added to
# the tasks, active_work, and memory_events tables.
#
# Usage:
#   ./test_v4_schema.sh      Run all tests
#   ./test_v4_schema.sh -h   Show help
#   ./test_v4_schema.sh -V   Show version

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

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for V4 PathFlow schema additions (stage columns, indexes, constraints).

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

# Test database variables
TEST_DB_DIR=""
TEST_DB=""
SCHEMA_FILE="$REPO_ROOT/.codeflow/scripts/db/schema.sql"

# Setup: create temp DB from schema.sql
setup_test_db() {
    TEST_DB_DIR=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-v4-schema-test-XXXXXX")
    TEST_DB="$TEST_DB_DIR/test.db"
    sqlite3 "$TEST_DB" < "$SCHEMA_FILE"
}

# Teardown: remove temp directory
teardown_test_db() {
    if [[ -n "$TEST_DB_DIR" && -d "$TEST_DB_DIR" ]]; then
        rm -rf "$TEST_DB_DIR"
    fi
}

# Cleanup on exit
trap teardown_test_db EXIT

# Helper: run sqlite3 query and return result
db_query() {
    sqlite3 "$TEST_DB" "$1" 2>&1
}

# Helper: check if a column exists in a table via PRAGMA table_info
column_exists() {
    local table="$1"
    local column="$2"
    local result
    result=$(sqlite3 "$TEST_DB" "PRAGMA table_info($table);" | grep -c "|${column}|" || true)
    [[ "$result" -gt 0 ]]
}

# Helper: check if an index exists
index_exists() {
    local index_name="$1"
    local result
    result=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='$index_name';")
    [[ "$result" -gt 0 ]]
}

# ============================================================================
# TEST: tasks table V4 columns
# ============================================================================

test_tasks_stage_column_exists() {
    test_section "tasks.stage column exists"

    setup_test_db

    if column_exists "tasks" "stage"; then
        test_pass "tasks.stage column exists"
    else
        test_fail "tasks.stage column should exist"
    fi

    teardown_test_db
}

test_tasks_stage_status_column_exists() {
    test_section "tasks.stage_status column exists"

    setup_test_db

    if column_exists "tasks" "stage_status"; then
        test_pass "tasks.stage_status column exists"
    else
        test_fail "tasks.stage_status column should exist"
    fi

    teardown_test_db
}

test_tasks_stage_history_column_exists() {
    test_section "tasks.stage_history column exists with DEFAULT '[]'"

    setup_test_db

    if column_exists "tasks" "stage_history"; then
        test_pass "tasks.stage_history column exists"
    else
        test_fail "tasks.stage_history column should exist"
    fi

    # Verify default value by inserting a minimal row and checking
    # We need to satisfy foreign keys, so insert seed data first
    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"
    db_query "INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain) VALUES ('test-task-1', 'TST-TSK-001', 'test-epic-1', 'Test Task', 'TST', 'TST', 'TST');"

    local default_value
    default_value=$(db_query "SELECT stage_history FROM tasks WHERE id = 'test-task-1';")
    assert_equals "[]" "$default_value" "tasks.stage_history defaults to '[]'"

    teardown_test_db
}

# ============================================================================
# TEST: active_work table V4 columns
# ============================================================================

test_active_work_current_stage_column_exists() {
    test_section "active_work.current_stage column exists"

    setup_test_db

    if column_exists "active_work" "current_stage"; then
        test_pass "active_work.current_stage column exists"
    else
        test_fail "active_work.current_stage column should exist"
    fi

    teardown_test_db
}

test_active_work_team_name_column_exists() {
    test_section "active_work.team_name column exists"

    setup_test_db

    if column_exists "active_work" "team_name"; then
        test_pass "active_work.team_name column exists"
    else
        test_fail "active_work.team_name column should exist"
    fi

    teardown_test_db
}

# ============================================================================
# TEST: Indexes
# ============================================================================

test_idx_tasks_stage_exists() {
    test_section "idx_tasks_stage index exists"

    setup_test_db

    if index_exists "idx_tasks_stage"; then
        test_pass "idx_tasks_stage index exists"
    else
        test_fail "idx_tasks_stage index should exist"
    fi

    teardown_test_db
}

test_idx_active_work_stage_exists() {
    test_section "idx_active_work_stage index exists"

    setup_test_db

    if index_exists "idx_active_work_stage"; then
        test_pass "idx_active_work_stage index exists"
    else
        test_fail "idx_active_work_stage index should exist"
    fi

    teardown_test_db
}

test_idx_active_work_team_exists() {
    test_section "idx_active_work_team index exists"

    setup_test_db

    if index_exists "idx_active_work_team"; then
        test_pass "idx_active_work_team index exists"
    else
        test_fail "idx_active_work_team index should exist"
    fi

    teardown_test_db
}

# ============================================================================
# TEST: tasks.stage CHECK constraint
# ============================================================================

test_tasks_stage_accepts_valid_values() {
    test_section "tasks.stage CHECK constraint accepts valid values"

    setup_test_db

    # Seed reference data
    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"

    local valid_stages=("dev" "work" "review" "qa" "done")
    local i=1
    for stage in "${valid_stages[@]}"; do
        assert_success \
            "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage) VALUES ('task-stage-$i', 'TST-TSK-STG-$i', 'test-epic-1', 'Test $stage', 'TST', 'TST', 'TST', '$stage');\"" \
            "tasks.stage accepts '$stage'"
        i=$((i + 1))
    done

    # NULL should also be accepted (it's the default)
    assert_success \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage) VALUES ('task-stage-null', 'TST-TSK-STG-N', 'test-epic-1', 'Test null', 'TST', 'TST', 'TST', NULL);\"" \
        "tasks.stage accepts NULL"

    teardown_test_db
}

test_tasks_stage_rejects_invalid_values() {
    test_section "tasks.stage CHECK constraint rejects invalid values"

    setup_test_db

    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage) VALUES ('task-bad-1', 'TST-TSK-BAD-1', 'test-epic-1', 'Bad stage', 'TST', 'TST', 'TST', 'invalid');\"" \
        "tasks.stage rejects 'invalid'"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage) VALUES ('task-bad-2', 'TST-TSK-BAD-2', 'test-epic-1', 'Bad stage', 'TST', 'TST', 'TST', 'DEV');\"" \
        "tasks.stage rejects uppercase 'DEV'"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage) VALUES ('task-bad-3', 'TST-TSK-BAD-3', 'test-epic-1', 'Bad stage', 'TST', 'TST', 'TST', 'testing');\"" \
        "tasks.stage rejects 'testing'"

    teardown_test_db
}

# ============================================================================
# TEST: tasks.stage_status CHECK constraint
# ============================================================================

test_tasks_stage_status_accepts_valid_values() {
    test_section "tasks.stage_status CHECK constraint accepts valid values"

    setup_test_db

    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"

    local valid_statuses=("pending" "in_progress" "complete" "failed")
    local i=1
    for status in "${valid_statuses[@]}"; do
        assert_success \
            "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage, stage_status) VALUES ('task-ss-$i', 'TST-TSK-SS-$i', 'test-epic-1', 'Test $status', 'TST', 'TST', 'TST', 'dev', '$status');\"" \
            "tasks.stage_status accepts '$status'"
        i=$((i + 1))
    done

    # NULL should also be accepted
    assert_success \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage_status) VALUES ('task-ss-null', 'TST-TSK-SS-N', 'test-epic-1', 'Test null', 'TST', 'TST', 'TST', NULL);\"" \
        "tasks.stage_status accepts NULL"

    teardown_test_db
}

test_tasks_stage_status_rejects_invalid_values() {
    test_section "tasks.stage_status CHECK constraint rejects invalid values"

    setup_test_db

    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage, stage_status) VALUES ('task-bss-1', 'TST-TSK-BSS-1', 'test-epic-1', 'Bad status', 'TST', 'TST', 'TST', 'dev', 'running');\"" \
        "tasks.stage_status rejects 'running'"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage, stage_status) VALUES ('task-bss-2', 'TST-TSK-BSS-2', 'test-epic-1', 'Bad status', 'TST', 'TST', 'TST', 'dev', 'PENDING');\"" \
        "tasks.stage_status rejects uppercase 'PENDING'"

    teardown_test_db
}

# ============================================================================
# TEST: active_work.current_stage CHECK constraint
# ============================================================================

test_active_work_current_stage_accepts_valid_values() {
    test_section "active_work.current_stage CHECK constraint accepts valid values"

    setup_test_db

    # active_work does not have strict FKs that require seeding, just insert directly
    local valid_stages=("dev" "work" "review" "qa")
    local i=1
    for stage in "${valid_stages[@]}"; do
        assert_success \
            "sqlite3 '$TEST_DB' \"INSERT INTO active_work (id, topic, current_stage) VALUES ('work-cs-$i', 'Test topic', '$stage');\"" \
            "active_work.current_stage accepts '$stage'"
        i=$((i + 1))
    done

    # NULL should be accepted
    assert_success \
        "sqlite3 '$TEST_DB' \"INSERT INTO active_work (id, topic, current_stage) VALUES ('work-cs-null', 'Test topic', NULL);\"" \
        "active_work.current_stage accepts NULL"

    teardown_test_db
}

test_active_work_current_stage_rejects_done() {
    test_section "active_work.current_stage CHECK constraint rejects 'done'"

    setup_test_db

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO active_work (id, topic, current_stage) VALUES ('work-bad-1', 'Test topic', 'done');\"" \
        "active_work.current_stage rejects 'done'"

    assert_fails \
        "sqlite3 '$TEST_DB' \"INSERT INTO active_work (id, topic, current_stage) VALUES ('work-bad-2', 'Test topic', 'invalid');\"" \
        "active_work.current_stage rejects 'invalid'"

    teardown_test_db
}

# ============================================================================
# TEST: memory_events.event_type CHECK constraint (V4 additions)
# ============================================================================

test_memory_events_accepts_v4_event_types() {
    test_section "memory_events.event_type CHECK accepts V4 values"

    setup_test_db

    local v4_types=("stage_transition" "stage_complete" "rework_limit")
    local i=1
    for etype in "${v4_types[@]}"; do
        assert_success \
            "sqlite3 '$TEST_DB' \"INSERT INTO memory_events (id, event_type, domain, data) VALUES ('mem-v4-$i', '$etype', 'development', '{\\\"test\\\": true}');\"" \
            "memory_events.event_type accepts '$etype'"
        i=$((i + 1))
    done

    teardown_test_db
}

# ============================================================================
# TEST: stage_history JSON storage
# ============================================================================

test_stage_history_stores_json() {
    test_section "tasks.stage_history stores and retrieves JSON correctly"

    setup_test_db

    db_query "INSERT OR IGNORE INTO area_types (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT OR IGNORE INTO work_types (code, name, branch_prefix) VALUES ('TST', 'Test', 'test/');"
    db_query "INSERT OR IGNORE INTO domains (code, name) VALUES ('TST', 'Test');"
    db_query "INSERT INTO epics (id, format_id, title, area_type, work_type, domain) VALUES ('test-epic-1', 'TST-EPC-001', 'Test Epic', 'TST', 'TST', 'TST');"

    local json_value='[{"stage":"dev","status":"complete","at":"2025-01-01T00:00:00Z"}]'
    db_query "INSERT INTO tasks (id, format_id, epic_id, title, area_type, work_type, domain, stage, stage_history) VALUES ('task-json-1', 'TST-TSK-JSON-1', 'test-epic-1', 'JSON Test', 'TST', 'TST', 'TST', 'work', '$json_value');"

    local retrieved
    retrieved=$(db_query "SELECT stage_history FROM tasks WHERE id = 'task-json-1';")
    assert_equals "$json_value" "$retrieved" "stage_history stores and retrieves JSON"

    # Verify json_extract works on the stored value
    local stage_val
    stage_val=$(db_query "SELECT json_extract(stage_history, '\$[0].stage') FROM tasks WHERE id = 'task-json-1';")
    assert_equals "dev" "$stage_val" "json_extract reads stage from stage_history"

    teardown_test_db
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
    echo -e "${BOLD}Testing: V4 PathFlow Schema Additions${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Column existence tests
    test_tasks_stage_column_exists
    test_tasks_stage_status_column_exists
    test_tasks_stage_history_column_exists
    test_active_work_current_stage_column_exists
    test_active_work_team_name_column_exists

    # Index tests
    test_idx_tasks_stage_exists
    test_idx_active_work_stage_exists
    test_idx_active_work_team_exists

    # CHECK constraint tests - tasks.stage
    test_tasks_stage_accepts_valid_values
    test_tasks_stage_rejects_invalid_values

    # CHECK constraint tests - tasks.stage_status
    test_tasks_stage_status_accepts_valid_values
    test_tasks_stage_status_rejects_invalid_values

    # CHECK constraint tests - active_work.current_stage
    test_active_work_current_stage_accepts_valid_values
    test_active_work_current_stage_rejects_done

    # memory_events V4 event types
    test_memory_events_accepts_v4_event_types

    # JSON storage
    test_stage_history_stores_json

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
