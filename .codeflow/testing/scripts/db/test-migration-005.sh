#!/usr/bin/env bash
# test-migration-005.sh - Tests for migration 005 (fix schema drift)
# Location: .codeflow/testing/scripts/db/test-migration-005.sh
#
# Tests the migration at .codeflow/scripts/db/migrations/005_fix_schema_drift.sql.
# Validates: format_id backfill, active_work columns, user_version, idempotency.
#
# Usage:
#   ./test-migration-005.sh           Run all tests
#   ./test-migration-005.sh -h        Show help
#   ./test-migration-005.sh -V        Show version

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

Tests for migration 005 (fix schema drift).

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

# Path under test
MIGRATION_FILE="$REPO_ROOT/.codeflow/scripts/db/migrations/005_fix_schema_drift.sql"

# Test database variables
TEST_DB_DIR=""
TEST_DB=""

# Setup: create temp directory and DB
setup_test_db() {
    TEST_DB_DIR=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-mig005-test-XXXXXX")
    TEST_DB="$TEST_DB_DIR/test.db"
}

# Teardown: remove temp directory
teardown_test_db() {
    if [[ -n "$TEST_DB_DIR" && -d "$TEST_DB_DIR" ]]; then
        rm -rf "$TEST_DB_DIR"
    fi
}

# Create a version-4 database (pre-migration 005 state)
load_v4_schema() {
    sqlite3 "$TEST_DB" <<'SCHEMA_SQL'
PRAGMA foreign_keys = OFF;

-- Reference tables (minimum seed data)
CREATE TABLE area_types (
    code TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
    scope_patterns TEXT, is_active BOOLEAN DEFAULT TRUE
);
INSERT INTO area_types (code, name) VALUES
    ('INF', 'Infrastructure'), ('FRT', 'Frontend'), ('BKD', 'Backend');

CREATE TABLE work_types (
    code TEXT PRIMARY KEY, name TEXT NOT NULL, branch_prefix TEXT NOT NULL,
    commit_type TEXT, urgency TEXT DEFAULT 'normal',
    default_scope_policy TEXT DEFAULT 'soft', is_active BOOLEAN DEFAULT TRUE
);
INSERT INTO work_types (code, name, branch_prefix, commit_type) VALUES
    ('FEAT', 'Feature', 'feat/', 'feat'), ('FIX', 'Bug Fix', 'fix/', 'fix');

CREATE TABLE domains (
    code TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
    is_reserved BOOLEAN DEFAULT FALSE, is_active BOOLEAN DEFAULT TRUE
);
INSERT INTO domains (code, name, is_reserved) VALUES ('GENL', 'General', 1);

CREATE TABLE estimate_types (
    code TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
    sort_order INTEGER DEFAULT 0, is_active BOOLEAN DEFAULT TRUE
);
INSERT INTO estimate_types (code, name, sort_order) VALUES ('S', 'Small', 2), ('M', 'Medium', 3);

-- Epics WITHOUT format_id (pre-migration 005 state)
CREATE TABLE epics (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    summary TEXT,
    status TEXT DEFAULT 'draft'
        CHECK(status IN ('draft', 'planning', 'in_progress', 'blocked', 'complete', 'archived')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    is_ongoing BOOLEAN DEFAULT FALSE,
    file_scope TEXT,
    priority TEXT DEFAULT 'normal'
        CHECK(priority IN ('low', 'normal', 'high', 'critical')),
    pr_number INTEGER,
    external_id TEXT,
    external_url TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (area_type) REFERENCES area_types(code),
    FOREIGN KEY (work_type) REFERENCES work_types(code),
    FOREIGN KEY (domain) REFERENCES domains(code)
);

-- Tasks WITH stage CHECK (post-migration 004 state)
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    format_id TEXT UNIQUE NOT NULL,
    epic_id TEXT NOT NULL REFERENCES epics(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'blocked', 'in_progress', 'complete', 'cancelled')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    origin TEXT DEFAULT 'planned',
    file_scope TEXT,
    scope_policy TEXT DEFAULT 'soft',
    scope_root TEXT,
    estimate TEXT,
    priority TEXT DEFAULT 'normal',
    assignee_id TEXT,
    autorun_eligible BOOLEAN DEFAULT FALSE,
    auto_commit BOOLEAN DEFAULT TRUE,
    raise_pr BOOLEAN DEFAULT TRUE,
    auto_merge BOOLEAN DEFAULT FALSE,
    target_branch TEXT,
    acceptance TEXT,
    tests TEXT,
    branch TEXT,
    pr_number INTEGER,
    external_id TEXT,
    external_url TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT,
    stage TEXT DEFAULT NULL
        CHECK(stage IS NULL OR stage IN ('dev', 'work', 'review', 'qa', 'done')),
    stage_status TEXT DEFAULT NULL
        CHECK(stage_status IS NULL OR stage_status IN ('pending', 'in_progress', 'complete', 'failed')),
    stage_history TEXT DEFAULT '[]'
);

-- Active work WITHOUT current_stage and team_name (pre-migration 005)
CREATE TABLE active_work (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id),
    topic TEXT NOT NULL,
    status TEXT DEFAULT 'in_progress'
        CHECK(status IN ('in_progress', 'complete', 'blocked')),
    branch TEXT,
    scope TEXT,
    deliverables TEXT,
    agent TEXT,
    session_id TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO schema_version (version) VALUES (4);

PRAGMA user_version = 4;
SCHEMA_SQL
}

# Insert test epics for data preservation and backfill tests
insert_test_epics() {
    sqlite3 "$TEST_DB" <<'TEST_DATA'
INSERT INTO epics (id, title, area_type, work_type, domain, external_id, created_at)
VALUES
    ('epic-001', 'Epic with matching external_id', 'INF', 'FEAT', 'GENL',
     'INF-EPC-001', '2026-01-01T00:00:00'),
    ('epic-002', 'Epic with null external_id', 'INF', 'FIX', 'GENL',
     NULL, '2026-01-02T00:00:00'),
    ('epic-003', 'Epic with non-matching external_id', 'FRT', 'FEAT', 'GENL',
     'custom-ref-123', '2026-01-03T00:00:00');
TEST_DATA
}

# Insert test active_work entries
insert_test_active_work() {
    sqlite3 "$TEST_DB" <<'TEST_DATA'
INSERT INTO active_work (id, topic, status, branch, created_at)
VALUES
    ('work-001', 'Test topic 1', 'in_progress', 'feat/test-1', '2026-01-01T00:00:00'),
    ('work-002', 'Test topic 2', 'complete', 'fix/test-2', '2026-01-02T00:00:00');
TEST_DATA
}

# Apply migration 005
apply_migration() {
    sqlite3 "$TEST_DB" < "$MIGRATION_FILE" 2>&1
}

# ============================================================================
# TEST: Migration file exists
# ============================================================================

test_migration_file_exists() {
    test_section "migration-005: migration file exists"

    assert_file_exists "$MIGRATION_FILE" "005_fix_schema_drift.sql exists"
}

# ============================================================================
# TEST: Migration applies cleanly to a version-4 DB
# ============================================================================

test_migration_applies_cleanly() {
    test_section "migration-005: applies cleanly to version-4 DB"

    setup_test_db
    load_v4_schema
    insert_test_epics
    insert_test_active_work

    local output
    output=$(apply_migration)
    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "Migration 005 applies cleanly (exit 0)"
    else
        test_fail "Migration 005 failed: $output"
    fi

    teardown_test_db
}

# ============================================================================
# TEST: format_id column exists with NOT NULL
# ============================================================================

test_epics_format_id_column() {
    test_section "migration-005: epics.format_id column with NOT NULL"

    setup_test_db
    load_v4_schema
    insert_test_epics
    apply_migration >/dev/null 2>&1

    # Check format_id column exists
    local col_name
    col_name=$(sqlite3 "$TEST_DB" "SELECT name FROM pragma_table_info('epics') WHERE name='format_id';")
    assert_equals "format_id" "$col_name" "format_id column exists in epics table"

    # Check NOT NULL constraint
    local notnull
    notnull=$(sqlite3 "$TEST_DB" "SELECT \"notnull\" FROM pragma_table_info('epics') WHERE name='format_id';")
    assert_equals "1" "$notnull" "format_id has NOT NULL constraint"

    # Check type
    local col_type
    col_type=$(sqlite3 "$TEST_DB" "SELECT type FROM pragma_table_info('epics') WHERE name='format_id';")
    assert_equals "TEXT" "$col_type" "format_id type is TEXT"

    teardown_test_db
}

# ============================================================================
# TEST: active_work has current_stage and team_name columns
# ============================================================================

test_active_work_new_columns() {
    test_section "migration-005: active_work.current_stage and team_name columns"

    setup_test_db
    load_v4_schema
    insert_test_active_work
    apply_migration >/dev/null 2>&1

    # Check current_stage column exists
    local stage_col
    stage_col=$(sqlite3 "$TEST_DB" "SELECT name FROM pragma_table_info('active_work') WHERE name='current_stage';")
    assert_equals "current_stage" "$stage_col" "current_stage column exists"

    # Check current_stage type
    local stage_type
    stage_type=$(sqlite3 "$TEST_DB" "SELECT type FROM pragma_table_info('active_work') WHERE name='current_stage';")
    assert_equals "TEXT" "$stage_type" "current_stage type is TEXT"

    # Check team_name column exists
    local team_col
    team_col=$(sqlite3 "$TEST_DB" "SELECT name FROM pragma_table_info('active_work') WHERE name='team_name';")
    assert_equals "team_name" "$team_col" "team_name column exists"

    # Check team_name type
    local team_type
    team_type=$(sqlite3 "$TEST_DB" "SELECT type FROM pragma_table_info('active_work') WHERE name='team_name';")
    assert_equals "TEXT" "$team_type" "team_name type is TEXT"

    teardown_test_db
}

# ============================================================================
# TEST: PRAGMA user_version = 5
# ============================================================================

test_user_version() {
    test_section "migration-005: PRAGMA user_version is 5"

    setup_test_db
    load_v4_schema
    apply_migration >/dev/null 2>&1

    local version
    version=$(sqlite3 "$TEST_DB" "PRAGMA user_version;")
    assert_equals "5" "$version" "user_version is 5 after migration"

    teardown_test_db
}

# ============================================================================
# TEST: Existing epic data preserved after migration
# ============================================================================

test_data_preservation() {
    test_section "migration-005: existing data preserved after migration"

    setup_test_db
    load_v4_schema
    insert_test_epics
    insert_test_active_work
    apply_migration >/dev/null 2>&1

    # Verify epic count
    local epic_count
    epic_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM epics;")
    assert_equals "3" "$epic_count" "Epic count preserved (3 rows)"

    # Verify specific epic data
    local epic_title
    epic_title=$(sqlite3 "$TEST_DB" "SELECT title FROM epics WHERE id='epic-001';")
    assert_equals "Epic with matching external_id" "$epic_title" "Epic title preserved"

    local epic_status
    epic_status=$(sqlite3 "$TEST_DB" "SELECT status FROM epics WHERE id='epic-001';")
    assert_equals "draft" "$epic_status" "Epic default status preserved"

    local epic_area
    epic_area=$(sqlite3 "$TEST_DB" "SELECT area_type FROM epics WHERE id='epic-002';")
    assert_equals "INF" "$epic_area" "Epic area_type preserved"

    # Verify active_work count
    local aw_count
    aw_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM active_work;")
    assert_equals "2" "$aw_count" "Active work count preserved (2 rows)"

    # Verify active_work data
    local aw_topic
    aw_topic=$(sqlite3 "$TEST_DB" "SELECT topic FROM active_work WHERE id='work-001';")
    assert_equals "Test topic 1" "$aw_topic" "Active work topic preserved"

    local aw_status
    aw_status=$(sqlite3 "$TEST_DB" "SELECT status FROM active_work WHERE id='work-002';")
    assert_equals "complete" "$aw_status" "Active work status preserved"

    teardown_test_db
}

# ============================================================================
# TEST: format_id backfill correctness
# ============================================================================

test_format_id_backfill() {
    test_section "migration-005: format_id backfill correctness"

    setup_test_db
    load_v4_schema
    insert_test_epics
    apply_migration >/dev/null 2>&1

    # Epic with matching external_id -> format_id = external_id
    local fid_matched
    fid_matched=$(sqlite3 "$TEST_DB" "SELECT format_id FROM epics WHERE id='epic-001';")
    assert_equals "INF-EPC-001" "$fid_matched" "Matching external_id used as format_id"

    # Epic with null external_id -> format_id generated with area_type prefix
    local fid_null
    fid_null=$(sqlite3 "$TEST_DB" "SELECT format_id FROM epics WHERE id='epic-002';")
    assert_contains "$fid_null" "INF-EPC-" "Generated format_id has INF-EPC- prefix (null external_id)"

    # Epic with non-matching external_id -> format_id generated with area_type prefix
    local fid_custom
    fid_custom=$(sqlite3 "$TEST_DB" "SELECT format_id FROM epics WHERE id='epic-003';")
    assert_contains "$fid_custom" "FRT-EPC-" "Generated format_id has FRT-EPC- prefix (non-matching external_id)"

    # Verify all format_ids are unique
    local unique_count
    unique_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(DISTINCT format_id) FROM epics;")
    assert_equals "3" "$unique_count" "All format_ids are unique"

    # Verify generated format_ids are not the same as the matched one
    assert_not_equals "$fid_matched" "$fid_null" "Generated format_id differs from matched"

    teardown_test_db
}

# ============================================================================
# TEST: Idempotency (apply migration twice)
# ============================================================================

test_idempotency() {
    test_section "migration-005: idempotent (apply twice, no errors)"

    setup_test_db
    load_v4_schema
    insert_test_epics
    insert_test_active_work

    # First application
    local output1
    output1=$(apply_migration)
    local exit1=$?

    if [[ $exit1 -ne 0 ]]; then
        test_fail "First application failed: $output1"
        teardown_test_db
        return
    fi
    test_pass "First application succeeds"

    # Second application
    local output2
    output2=$(apply_migration)
    local exit2=$?

    if [[ $exit2 -eq 0 ]]; then
        test_pass "Second application succeeds (idempotent)"
    else
        test_fail "Second application failed: $output2"
    fi

    # Verify data integrity after double application
    local epic_count
    epic_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM epics;")
    assert_equals "3" "$epic_count" "Epic count correct after double application"

    local aw_count
    aw_count=$(sqlite3 "$TEST_DB" "SELECT COUNT(*) FROM active_work;")
    assert_equals "2" "$aw_count" "Active work count correct after double application"

    local version
    version=$(sqlite3 "$TEST_DB" "PRAGMA user_version;")
    assert_equals "5" "$version" "user_version still 5 after double application"

    teardown_test_db
}

# ============================================================================
# TEST: CHECK constraint on active_work.current_stage
# ============================================================================

test_check_constraint_current_stage() {
    test_section "migration-005: CHECK constraint on active_work.current_stage"

    setup_test_db
    load_v4_schema
    apply_migration >/dev/null 2>&1

    # Valid values should succeed
    local valid_values=("dev" "work" "review" "qa")
    for val in "${valid_values[@]}"; do
        local insert_result
        insert_result=$(sqlite3 "$TEST_DB" \
            "INSERT INTO active_work (id, topic, current_stage) VALUES ('test-$val', 'test', '$val');" 2>&1)
        local insert_exit=$?
        if [[ $insert_exit -eq 0 ]]; then
            test_pass "current_stage='$val' accepted"
        else
            test_fail "current_stage='$val' rejected: $insert_result"
        fi
    done

    # NULL should succeed (DEFAULT NULL)
    local null_result
    null_result=$(sqlite3 "$TEST_DB" \
        "INSERT INTO active_work (id, topic, current_stage) VALUES ('test-null', 'test', NULL);" 2>&1)
    local null_exit=$?
    if [[ $null_exit -eq 0 ]]; then
        test_pass "current_stage=NULL accepted"
    else
        test_fail "current_stage=NULL rejected: $null_result"
    fi

    # Invalid value should fail (use || to prevent set -e from aborting)
    local invalid_exit=0
    sqlite3 "$TEST_DB" \
        "INSERT INTO active_work (id, topic, current_stage) VALUES ('test-bad', 'test', 'invalid_stage');" \
        2>/dev/null || invalid_exit=$?
    if [[ $invalid_exit -ne 0 ]]; then
        test_pass "current_stage='invalid_stage' correctly rejected"
    else
        test_fail "current_stage='invalid_stage' should have been rejected"
    fi

    teardown_test_db
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
    echo -e "${BOLD}Testing: Migration 005 (fix schema drift)${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # File existence
    test_migration_file_exists

    # Migration applies cleanly
    test_migration_applies_cleanly

    # Column verification
    test_epics_format_id_column
    test_active_work_new_columns

    # Version check
    test_user_version

    # Data preservation
    test_data_preservation

    # Backfill correctness
    test_format_id_backfill

    # Idempotency
    test_idempotency

    # CHECK constraint
    test_check_constraint_current_stage

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
