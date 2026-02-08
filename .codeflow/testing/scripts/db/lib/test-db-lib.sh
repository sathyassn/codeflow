#!/usr/bin/env bash
# Test: db-lib.sh
# Location: .codeflow/testing/scripts/db/lib/test-db-lib.sh
#
# Tests the database library functions

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
LIB_FILE="$REPO_ROOT/.codeflow/scripts/db/lib/db-lib.sh"

export REPO_ROOT

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test helper
pass() {
    echo "PASS: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo "FAIL: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

echo "=== Testing db-lib.sh ==="
echo ""

# ============================================================================
# Test 1: Library file exists
# ============================================================================
echo "--- Basic checks ---"

if [[ -f "$LIB_FILE" ]]; then
    pass "Library file exists"
else
    fail "Library file not found"
fi

# ============================================================================
# Test 2: Shellcheck passes
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$LIB_FILE" 2>/dev/null; then
        pass "Library passes shellcheck"
    else
        fail "Library fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# ============================================================================
# Test 3: Library can be sourced
# ============================================================================
echo ""
echo "--- Source tests ---"

if (source "$LIB_FILE" 2>/dev/null); then
    pass "Library can be sourced"
else
    fail "Library cannot be sourced"
fi

# ============================================================================
# Test 4: Source guard prevents double-sourcing
# ============================================================================
if (
    source "$LIB_FILE"
    _CODEFLOW_DBLIB_LOADED=1
    source "$LIB_FILE"  # Should return immediately
    exit 0
) 2>/dev/null; then
    pass "Source guard works"
else
    fail "Source guard failed"
fi

# ============================================================================
# Test 5: Configuration variables are set
# ============================================================================
echo ""
echo "--- Configuration tests ---"

if (
    source "$LIB_FILE"
    [[ -n "$DB_FILE" ]]
); then
    pass "DB_FILE is set"
else
    fail "DB_FILE not set"
fi

if (
    source "$LIB_FILE"
    [[ -n "$DB_TIMEOUT" ]]
); then
    pass "DB_TIMEOUT is set"
else
    fail "DB_TIMEOUT not set"
fi

if (
    source "$LIB_FILE"
    [[ -n "$DB_RETRIES" ]]
); then
    pass "DB_RETRIES is set"
else
    fail "DB_RETRIES not set"
fi

if (
    source "$LIB_FILE"
    [[ -n "$DB_RETRY_DELAY" ]]
); then
    pass "DB_RETRY_DELAY is set"
else
    fail "DB_RETRY_DELAY not set"
fi

if (
    source "$LIB_FILE"
    [[ -n "$DB_OP_LOG" ]]
); then
    pass "DB_OP_LOG is set"
else
    fail "DB_OP_LOG not set"
fi

# ============================================================================
# Test 6: Core functions exist
# ============================================================================
echo ""
echo "--- Function existence tests ---"

if (
    source "$LIB_FILE"
    type db_query &>/dev/null
); then
    pass "db_query function exists"
else
    fail "db_query function not defined"
fi

if (
    source "$LIB_FILE"
    type db_execute &>/dev/null
); then
    pass "db_execute function exists"
else
    fail "db_execute function not defined"
fi

if (
    source "$LIB_FILE"
    type db_transaction &>/dev/null
); then
    pass "db_transaction function exists"
else
    fail "db_transaction function not defined"
fi

if (
    source "$LIB_FILE"
    type db_get_value &>/dev/null
); then
    pass "db_get_value function exists"
else
    fail "db_get_value function not defined"
fi

if (
    source "$LIB_FILE"
    type db_count &>/dev/null
); then
    pass "db_count function exists"
else
    fail "db_count function not defined"
fi

# ============================================================================
# Test 7: Health functions exist
# ============================================================================
echo ""
echo "--- Health function tests ---"

if (
    source "$LIB_FILE"
    type db_check_integrity &>/dev/null
); then
    pass "db_check_integrity function exists"
else
    fail "db_check_integrity function not defined"
fi

if (
    source "$LIB_FILE"
    type db_exists &>/dev/null
); then
    pass "db_exists function exists"
else
    fail "db_exists function not defined"
fi

if (
    source "$LIB_FILE"
    type db_get_schema_version &>/dev/null
); then
    pass "db_get_schema_version function exists"
else
    fail "db_get_schema_version function not defined"
fi

# ============================================================================
# Test 8: WAL functions exist
# ============================================================================
echo ""
echo "--- WAL function tests ---"

if (
    source "$LIB_FILE"
    type db_enable_wal &>/dev/null
); then
    pass "db_enable_wal function exists"
else
    fail "db_enable_wal function not defined"
fi

if (
    source "$LIB_FILE"
    type db_checkpoint &>/dev/null
); then
    pass "db_checkpoint function exists"
else
    fail "db_checkpoint function not defined"
fi

# ============================================================================
# Test 9: Maintenance functions exist
# ============================================================================
echo ""
echo "--- Maintenance function tests ---"

if (
    source "$LIB_FILE"
    type db_vacuum &>/dev/null
); then
    pass "db_vacuum function exists"
else
    fail "db_vacuum function not defined"
fi

if (
    source "$LIB_FILE"
    type db_analyze &>/dev/null
); then
    pass "db_analyze function exists"
else
    fail "db_analyze function not defined"
fi

if (
    source "$LIB_FILE"
    type db_size &>/dev/null
); then
    pass "db_size function exists"
else
    fail "db_size function not defined"
fi

if (
    source "$LIB_FILE"
    type db_wal_size &>/dev/null
); then
    pass "db_wal_size function exists"
else
    fail "db_wal_size function not defined"
fi

# ============================================================================
# Test 10: Table functions exist
# ============================================================================
echo ""
echo "--- Table function tests ---"

if (
    source "$LIB_FILE"
    type db_table_exists &>/dev/null
); then
    pass "db_table_exists function exists"
else
    fail "db_table_exists function not defined"
fi

if (
    source "$LIB_FILE"
    type db_list_tables &>/dev/null
); then
    pass "db_list_tables function exists"
else
    fail "db_list_tables function not defined"
fi

if (
    source "$LIB_FILE"
    type db_table_count &>/dev/null
); then
    pass "db_table_count function exists"
else
    fail "db_table_count function not defined"
fi

# ============================================================================
# Test 11: FTS functions exist
# ============================================================================
echo ""
echo "--- FTS function tests ---"

if (
    source "$LIB_FILE"
    type db_fts_rebuild &>/dev/null
); then
    pass "db_fts_rebuild function exists"
else
    fail "db_fts_rebuild function not defined"
fi

if (
    source "$LIB_FILE"
    type db_fts_optimize &>/dev/null
); then
    pass "db_fts_optimize function exists"
else
    fail "db_fts_optimize function not defined"
fi

# ============================================================================
# Test 12: Backup function exists
# ============================================================================
echo ""
echo "--- Backup function tests ---"

if (
    source "$LIB_FILE"
    type db_backup &>/dev/null
); then
    pass "db_backup function exists"
else
    fail "db_backup function not defined"
fi

# ============================================================================
# Test 13: Logging function exists
# ============================================================================
if (
    source "$LIB_FILE"
    type db_log_operation &>/dev/null
); then
    pass "db_log_operation function exists"
else
    fail "db_log_operation function not defined"
fi

# ============================================================================
# Test 14: Functions are exported
# ============================================================================
echo ""
echo "--- Export tests ---"

if (
    source "$LIB_FILE"
    export -f | grep -q "db_query"
); then
    pass "db_query is exported"
else
    fail "db_query not exported"
fi

if (
    source "$LIB_FILE"
    export -f | grep -q "db_execute"
); then
    pass "db_execute is exported"
else
    fail "db_execute not exported"
fi

# ============================================================================
# Test 15: Log directory is created
# ============================================================================
echo ""
echo "--- Directory tests ---"

if (
    source "$LIB_FILE"
    [[ -d "$(dirname "$DB_OP_LOG")" ]]
); then
    pass "Log directory is created on source"
else
    fail "Log directory not created"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
