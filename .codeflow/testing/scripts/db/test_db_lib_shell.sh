#!/usr/bin/env bash
# test_db_lib_shell.sh - Tests for db-lib.sh shell module
# Location: .codeflow/testing/scripts/db/test_db_lib_shell.sh
#
# Usage:
#   ./test_db_lib_shell.sh      Run all tests
#   ./test_db_lib_shell.sh -h   Show help
#   ./test_db_lib_shell.sh -V   Show version

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

Tests for db-lib.sh shell module.

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

# Override DB_FILE for testing
export TEST_DB_DIR=""
export CODEFLOW_DB_FILE=""

# Setup test database
setup_test_database() {
    TEST_DB_DIR=$(mktemp -d "/tmp/codeflow-db-test-XXXXXX")
    CODEFLOW_DB_FILE="$TEST_DB_DIR/test.db"
    export CODEFLOW_DB_FILE
    export DB_FILE="$CODEFLOW_DB_FILE"
    export CODEFLOW_DB_OP_LOG="$TEST_DB_DIR/ops.jsonl"
    export DB_OP_LOG="$CODEFLOW_DB_OP_LOG"

    # Source the db-lib after setting variables
    source "$REPO_ROOT/.codeflow/scripts/db/lib/db-lib.sh"
}

teardown_test_database() {
    if [[ -n "$TEST_DB_DIR" && -d "$TEST_DB_DIR" ]]; then
        rm -rf "$TEST_DB_DIR"
    fi
}

# ============================================================================
# TEST: Basic Query Operations
# ============================================================================

test_db_query_creates_database() {
    test_section "db_query: creates database"

    setup_test_database

    # Execute a simple query that creates the database
    local result
    result=$(db_query "SELECT 1 as test")

    if [[ -f "$CODEFLOW_DB_FILE" ]]; then
        test_pass "Database file created"
    else
        test_fail "Database file should be created"
    fi

    teardown_test_database
}

test_db_query_returns_result() {
    test_section "db_query: returns result"

    setup_test_database

    local result
    result=$(db_query "SELECT 42 as value")

    assert_equals "42" "$result" "Query returns expected value"

    teardown_test_database
}

test_db_query_with_table() {
    test_section "db_query: with table"

    setup_test_database

    # Create table and insert data
    db_query "CREATE TABLE items (id INTEGER, name TEXT)"
    db_query "INSERT INTO items VALUES (1, 'test')"

    local result
    result=$(db_query "SELECT name FROM items WHERE id = 1")

    assert_equals "test" "$result" "Query returns table data"

    teardown_test_database
}

# ============================================================================
# TEST: Write Operations
# ============================================================================

test_db_execute_insert() {
    test_section "db_execute: insert"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"

    if db_execute "INSERT INTO items VALUES (1)"; then
        test_pass "Insert executed successfully"
    else
        test_fail "Insert should succeed"
    fi

    local count
    count=$(db_get_value "SELECT COUNT(*) FROM items")
    assert_equals "1" "$count" "Row inserted"

    teardown_test_database
}

test_db_execute_update() {
    test_section "db_execute: update"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER, value INTEGER)"
    db_query "INSERT INTO items VALUES (1, 10)"

    if db_execute "UPDATE items SET value = 20 WHERE id = 1"; then
        test_pass "Update executed successfully"
    else
        test_fail "Update should succeed"
    fi

    local value
    value=$(db_get_value "SELECT value FROM items WHERE id = 1")
    assert_equals "20" "$value" "Value updated"

    teardown_test_database
}

# ============================================================================
# TEST: Transaction Operations
# ============================================================================

test_db_transaction_success() {
    test_section "db_transaction: success"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"

    # Note: statements must end with semicolons for proper transaction wrapping
    if db_transaction "INSERT INTO items VALUES (1); INSERT INTO items VALUES (2);"; then
        test_pass "Transaction committed"
    else
        test_fail "Transaction should succeed"
    fi

    local count
    count=$(db_get_value "SELECT COUNT(*) FROM items")
    assert_equals "2" "$count" "Both rows inserted"

    teardown_test_database
}

# ============================================================================
# TEST: Utility Functions
# ============================================================================

test_db_get_value() {
    test_section "db_get_value"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER, name TEXT)"
    db_query "INSERT INTO items VALUES (1, 'hello')"

    local value
    value=$(db_get_value "SELECT name FROM items WHERE id = 1")

    assert_equals "hello" "$value" "Returns single value"

    teardown_test_database
}

test_db_count_table() {
    test_section "db_count: table"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"
    db_query "INSERT INTO items VALUES (1), (2), (3)"

    local count
    count=$(db_count "items")

    assert_equals "3" "$count" "Returns table count"

    teardown_test_database
}

test_db_table_exists_true() {
    test_section "db_table_exists: true"

    setup_test_database

    db_query "CREATE TABLE existing_table (id INTEGER)"

    if db_table_exists "existing_table"; then
        test_pass "Existing table detected"
    else
        test_fail "Should detect existing table"
    fi

    teardown_test_database
}

test_db_table_exists_false() {
    test_section "db_table_exists: false"

    setup_test_database

    # Don't create the table

    if db_table_exists "nonexistent_table"; then
        test_fail "Should not detect nonexistent table"
    else
        test_pass "Nonexistent table not detected"
    fi

    teardown_test_database
}

test_db_list_tables() {
    test_section "db_list_tables"

    setup_test_database

    db_query "CREATE TABLE table_a (id INTEGER)"
    db_query "CREATE TABLE table_b (id INTEGER)"

    local tables
    tables=$(db_list_tables)

    if [[ "$tables" == *"table_a"* ]] && [[ "$tables" == *"table_b"* ]]; then
        test_pass "Lists all tables"
    else
        test_fail "Should list all tables: $tables"
    fi

    teardown_test_database
}

# ============================================================================
# TEST: Health Operations
# ============================================================================

test_db_check_integrity() {
    test_section "db_check_integrity"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"

    if db_check_integrity; then
        test_pass "Integrity check passes"
    else
        test_fail "Integrity check should pass"
    fi

    teardown_test_database
}

test_db_exists_true() {
    test_section "db_exists: true"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"

    if db_exists; then
        test_pass "Database exists"
    else
        test_fail "Database should exist"
    fi

    teardown_test_database
}

# ============================================================================
# TEST: Maintenance Operations
# ============================================================================

test_db_vacuum() {
    test_section "db_vacuum"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER, data TEXT)"
    db_query "INSERT INTO items VALUES (1, 'test data')"
    db_query "DELETE FROM items WHERE id = 1"

    if db_vacuum; then
        test_pass "Vacuum succeeds"
    else
        test_fail "Vacuum should succeed"
    fi

    teardown_test_database
}

test_db_analyze() {
    test_section "db_analyze"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"
    db_query "INSERT INTO items VALUES (1), (2), (3)"

    if db_analyze; then
        test_pass "Analyze succeeds"
    else
        test_fail "Analyze should succeed"
    fi

    teardown_test_database
}

test_db_size() {
    test_section "db_size"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER)"

    local size
    size=$(db_size)

    if [[ "$size" -gt 0 ]]; then
        test_pass "Database size returned: $size bytes"
    else
        test_fail "Database size should be > 0"
    fi

    teardown_test_database
}

# ============================================================================
# TEST: WAL Operations
# ============================================================================

test_db_enable_wal() {
    test_section "db_enable_wal"

    setup_test_database

    if db_enable_wal; then
        test_pass "WAL mode enabled"
    else
        test_fail "WAL mode should be enabled"
    fi

    # Verify WAL mode
    local mode
    mode=$(db_get_value "PRAGMA journal_mode")
    assert_equals "wal" "$mode" "Journal mode is WAL"

    teardown_test_database
}

# ============================================================================
# TEST: Backup Operations
# ============================================================================

test_db_backup() {
    test_section "db_backup"

    setup_test_database

    db_query "CREATE TABLE items (id INTEGER, data TEXT)"
    db_query "INSERT INTO items VALUES (1, 'important data')"

    local backup_path="$TEST_DB_DIR/backup.db"
    local result
    result=$(db_backup "$backup_path")

    if [[ -f "$backup_path" ]]; then
        test_pass "Backup file created"
    else
        test_fail "Backup file should be created"
    fi

    # Verify backup contents
    local data
    data=$(sqlite3 "$backup_path" "SELECT data FROM items WHERE id = 1")
    assert_equals "important data" "$data" "Backup contains data"

    teardown_test_database
}

# ============================================================================
# TEST: Operation Logging
# ============================================================================

test_db_log_operation() {
    test_section "db_log_operation"

    setup_test_database

    db_log_operation "TEST" "SUCCESS" "test details"

    if [[ -f "$CODEFLOW_DB_OP_LOG" ]]; then
        test_pass "Operation log file created"
    else
        test_fail "Operation log file should be created"
    fi

    local content
    content=$(cat "$CODEFLOW_DB_OP_LOG")

    if [[ "$content" == *"TEST"* ]] && [[ "$content" == *"SUCCESS"* ]]; then
        test_pass "Log contains operation details"
    else
        test_fail "Log should contain operation details"
    fi

    teardown_test_database
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
    echo -e "${BOLD}Testing: db/lib/db-lib.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Basic query operations
    test_db_query_creates_database
    test_db_query_returns_result
    test_db_query_with_table

    # Write operations
    test_db_execute_insert
    test_db_execute_update

    # Transaction operations
    test_db_transaction_success

    # Utility functions
    test_db_get_value
    test_db_count_table
    test_db_table_exists_true
    test_db_table_exists_false
    test_db_list_tables

    # Health operations
    test_db_check_integrity
    test_db_exists_true

    # Maintenance operations
    test_db_vacuum
    test_db_analyze
    test_db_size

    # WAL operations
    test_db_enable_wal

    # Backup operations
    test_db_backup

    # Operation logging
    test_db_log_operation

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
