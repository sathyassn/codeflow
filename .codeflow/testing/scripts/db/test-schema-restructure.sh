#!/usr/bin/env bash
# test-schema-restructure.sh - Tests for schema at scripts/db/schema.sql
# Location: .codeflow/testing/scripts/db/test-schema-restructure.sh
#
# Tests the consolidated schema file at .codeflow/scripts/db/schema.sql.
# The schema is the single source of truth for DB structure (Go CLI loads it).
#
# Usage:
#   ./test-schema-restructure.sh           Run all tests
#   ./test-schema-restructure.sh -h        Show help
#   ./test-schema-restructure.sh -V        Show version

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="2.0.0"
readonly SCRIPT_VERSION
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
readonly REPO_ROOT

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for the consolidated schema at .codeflow/scripts/db/schema.sql.

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

# Paths under test
SOURCE_SCHEMA="$REPO_ROOT/.codeflow/scripts/db/schema.sql"

# Test database variables
TEST_DB_DIR=""
TEST_DB=""

# Setup: create temp directory and DB
setup_test_db() {
    TEST_DB_DIR=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-schema-test-XXXXXX")
    TEST_DB="$TEST_DB_DIR/test.db"
}

# Teardown: remove temp directory
teardown_test_db() {
    if [[ -n "$TEST_DB_DIR" && -d "$TEST_DB_DIR" ]]; then
        rm -rf "$TEST_DB_DIR"
    fi
}

# ============================================================================
# TEST: Schema file exists
# ============================================================================

test_schema_file_exists() {
    test_section "schema: source schema file exists"

    assert_file_exists "$SOURCE_SCHEMA" "scripts/db/schema.sql exists"
}

# ============================================================================
# TEST: Schema loads cleanly
# ============================================================================

test_schema_loads_cleanly() {
    test_section "schema: schema.sql loads without errors"

    setup_test_db

    local output
    output=$(sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>&1)
    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "schema.sql loads cleanly (exit 0)"
    else
        test_fail "schema.sql failed to load: $output"
    fi

    teardown_test_db
}

test_schema_idempotent() {
    test_section "schema: schema.sql is idempotent (CREATE IF NOT EXISTS)"

    setup_test_db

    # Load schema twice
    sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>/dev/null
    local output
    output=$(sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>&1)
    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "schema.sql loads twice without errors (idempotent)"
    else
        test_fail "schema.sql not idempotent: $output"
    fi

    teardown_test_db
}

# ============================================================================
# TEST: Expected tables exist
# ============================================================================

test_expected_tables_exist() {
    test_section "schema: expected tables created"

    setup_test_db
    sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>/dev/null

    local expected_tables=("epics" "tasks" "memory_events" "sessions" "entities")
    for table in "${expected_tables[@]}"; do
        local count
        count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='$table';")
        if [[ "$count" -eq 1 ]]; then
            test_pass "Table '$table' exists"
        else
            test_fail "Table '$table' missing"
        fi
    done

    teardown_test_db
}

# ============================================================================
# TEST: Indexes exist
# ============================================================================

test_indexes_exist() {
    test_section "schema: indexes created"

    setup_test_db
    sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>/dev/null

    local index_count
    index_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%';")

    if [[ "$index_count" -gt 0 ]]; then
        test_pass "Created $index_count indexes"
    else
        test_fail "Expected indexes, got 0"
    fi

    teardown_test_db
}

# ============================================================================
# TEST: FTS triggers present
# ============================================================================

test_fts_triggers_present() {
    test_section "schema: FTS triggers present"

    setup_test_db
    sqlite3 "$TEST_DB" < "$SOURCE_SCHEMA" 2>/dev/null

    local fts_triggers=("entities_fts_insert" "entities_fts_delete" "entities_fts_update")
    for trigger in "${fts_triggers[@]}"; do
        local count
        count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='$trigger';")
        if [[ "$count" -eq 1 ]]; then
            test_pass "Trigger $trigger exists"
        else
            test_fail "Trigger $trigger missing"
        fi
    done

    teardown_test_db
}

# ============================================================================
# TEST: No stale references to old split schema paths
# ============================================================================

test_no_stale_schema_references() {
    test_section "schema: no stale references to .state/db/schema/ or .state/db/queries/"

    local stale_refs=""

    # Search for references to old split paths
    local old_refs
    old_refs=$(grep -rl '\.state/db/schema/' "$REPO_ROOT/.codeflow/scripts/" 2>/dev/null || true)
    if [[ -n "$old_refs" ]]; then
        stale_refs="$old_refs"
    fi

    local old_query_refs
    old_query_refs=$(grep -rl '\.state/db/queries/' "$REPO_ROOT/.codeflow/scripts/" 2>/dev/null || true)
    if [[ -n "$old_query_refs" ]]; then
        stale_refs="${stale_refs}${old_query_refs}"
    fi

    if [[ -z "$stale_refs" ]]; then
        test_pass "No stale references to .state/db/schema/ or .state/db/queries/"
    else
        test_fail "Stale references found"
        echo "$stale_refs" | while read -r ref; do
            echo "  -> $ref" >&2
        done
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
    echo -e "${BOLD}Testing: Schema (scripts/db/schema.sql)${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # File existence
    test_schema_file_exists

    # Schema loads
    test_schema_loads_cleanly
    test_schema_idempotent

    # Tables and indexes
    test_expected_tables_exist
    test_indexes_exist

    # FTS triggers
    test_fts_triggers_present

    # Stale references
    test_no_stale_schema_references

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
