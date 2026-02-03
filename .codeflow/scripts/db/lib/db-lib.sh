#!/usr/bin/env bash
# db-lib.sh - Unified database operations library for shell scripts.
#
# Location: .codeflow/scripts/db/lib/db-lib.sh
#
# Provides:
#   - db_query()           Execute SELECT with retry logic
#   - db_execute()         Execute INSERT/UPDATE/DELETE
#   - db_transaction()     Transaction wrapper
#   - db_check_integrity() Database health check
#   - db_get_value()       Single value extraction
#   - db_log_operation()   Operation audit logging
#   - db_exists()          Check if database exists and is valid

set -euo pipefail

# Source guard to prevent multiple loads
[[ -n "${_CODEFLOW_DBLIB_LOADED:-}" ]] && return 0
_CODEFLOW_DBLIB_LOADED=1

# Configuration
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
DB_FILE="${CODEFLOW_DB_FILE:-$REPO_ROOT/.state/db/codeflow.db}"
DB_TIMEOUT="${CODEFLOW_DB_TIMEOUT:-5000}"
DB_RETRIES="${CODEFLOW_DB_RETRIES:-3}"
DB_RETRY_DELAY="${CODEFLOW_DB_RETRY_DELAY:-0.5}"
DB_OP_LOG="${CODEFLOW_DB_OP_LOG:-$REPO_ROOT/.state/logs/db/operations-$(date +%Y-%m-%d).jsonl}"

# Ensure log directory exists
mkdir -p "$(dirname "$DB_OP_LOG")"

# ============================================================================
# OPERATION LOGGING
# ============================================================================

# Log a database operation (JSONL format)
db_log_operation() {
    local operation="$1"
    local status="$2"
    local details="${3:-}"
    local timestamp
    timestamp=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
    local session_id="${CLAUDE_SESSION_ID:-unknown}"

    # Escape details for JSON
    details=$(echo "$details" | sed 's/"/\\"/g' | tr '\n' ' ')

    printf '{"ts":"%s","level":"INFO","session_id":"%s","event":"db_operation","operation":"%s","status":"%s","details":"%s"}\n' \
        "$timestamp" "$session_id" "$operation" "$status" "$details" >> "$DB_OP_LOG" 2>/dev/null || true
}

# ============================================================================
# CORE DATABASE OPERATIONS
# ============================================================================

# Execute a query with retry logic
# Args: $1 = query
# Returns: query result on stdout, exit 0 on success, exit 1 on failure
db_query() {
    local query="$1"
    local retries=$DB_RETRIES
    local result=""

    while [[ $retries -gt 0 ]]; do
        # Execute query with timeout
        if result=$(sqlite3 -init /dev/null "$DB_FILE" "$query" 2>&1); then
            echo "$result"
            return 0
        fi

        # Check if error is retryable (database locked)
        if [[ "$result" == *"database is locked"* ]]; then
            ((retries--))
            sleep "$DB_RETRY_DELAY"
        else
            # Non-retryable error
            db_log_operation "QUERY" "FAILED" "$query: $result"
            echo "$result" >&2
            return 1
        fi
    done

    # All retries exhausted
    db_log_operation "QUERY" "FAILED" "$query: timeout after $DB_RETRIES retries"
    echo "Error: database locked after $DB_RETRIES retries" >&2
    return 1
}

# Execute a write operation (INSERT/UPDATE/DELETE)
# Args: $1 = SQL statement
# Returns: exit 0 on success, exit 1 on failure
db_execute() {
    local statement="$1"

    if db_query "$statement" >/dev/null; then
        db_log_operation "EXECUTE" "SUCCESS" "${statement:0:100}..."
        return 0
    else
        return 1
    fi
}

# Execute multiple statements in a transaction
# Args: $1 = statements (semicolon-separated)
# Returns: exit 0 on success, exit 1 on failure
db_transaction() {
    local statements="$1"
    local wrapped="BEGIN TRANSACTION;
$statements
COMMIT;"

    if db_query "$wrapped" >/dev/null; then
        db_log_operation "TRANSACTION" "SUCCESS" "${statements:0:100}..."
        return 0
    else
        db_log_operation "TRANSACTION" "FAILED" "${statements:0:100}..."
        return 1
    fi
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

# Get single value from query (first line of result)
# Args: $1 = query
# Returns: single value on stdout
db_get_value() {
    local query="$1"
    db_query "$query" | head -1
}

# Get row count from query
# Args: $1 = table name or query
db_count() {
    local target="$1"
    if [[ "$target" == *" "* ]]; then
        # It's a query
        db_get_value "SELECT COUNT(*) FROM ($target)"
    else
        # It's a table name
        db_get_value "SELECT COUNT(*) FROM $target"
    fi
}

# ============================================================================
# DATABASE HEALTH
# ============================================================================

# Check database integrity
# Returns: exit 0 if ok, exit 1 if failed
db_check_integrity() {
    local result
    result=$(db_query "PRAGMA integrity_check;")

    if [[ "$result" == "ok" ]]; then
        db_log_operation "INTEGRITY_CHECK" "PASSED" ""
        return 0
    else
        db_log_operation "INTEGRITY_CHECK" "FAILED" "$result"
        return 1
    fi
}

# Check if database exists and is valid
# Returns: exit 0 if exists and valid, exit 1 otherwise
db_exists() {
    [[ -f "$DB_FILE" ]] && db_check_integrity
}

# Get current schema version
# Returns: version number on stdout
db_get_schema_version() {
    db_get_value "SELECT COALESCE(MAX(version), 0) FROM schema_version"
}

# ============================================================================
# WAL MODE OPERATIONS
# ============================================================================

# Enable WAL mode for better concurrency
db_enable_wal() {
    db_query "PRAGMA journal_mode=WAL;" >/dev/null
    db_query "PRAGMA synchronous=NORMAL;" >/dev/null
}

# Checkpoint WAL file
db_checkpoint() {
    local mode="${1:-PASSIVE}"
    db_query "PRAGMA wal_checkpoint($mode);" >/dev/null
    db_log_operation "CHECKPOINT" "SUCCESS" "mode=$mode"
}

# ============================================================================
# MAINTENANCE OPERATIONS
# ============================================================================

# Run VACUUM to reclaim space
db_vacuum() {
    if db_query "VACUUM;" >/dev/null; then
        db_log_operation "VACUUM" "SUCCESS" ""
        return 0
    else
        db_log_operation "VACUUM" "FAILED" ""
        return 1
    fi
}

# Run ANALYZE to update statistics
db_analyze() {
    if db_query "ANALYZE;" >/dev/null; then
        db_log_operation "ANALYZE" "SUCCESS" ""
        return 0
    else
        db_log_operation "ANALYZE" "FAILED" ""
        return 1
    fi
}

# Get database size in bytes
db_size() {
    stat -f%z "$DB_FILE" 2>/dev/null || stat --printf="%s" "$DB_FILE" 2>/dev/null || echo "0"
}

# Get WAL file size
db_wal_size() {
    local wal_file="${DB_FILE}-wal"
    if [[ -f "$wal_file" ]]; then
        stat -f%z "$wal_file" 2>/dev/null || stat --printf="%s" "$wal_file" 2>/dev/null || echo "0"
    else
        echo "0"
    fi
}

# ============================================================================
# TABLE OPERATIONS
# ============================================================================

# Check if table exists
# Args: $1 = table name
db_table_exists() {
    local table="$1"
    local count
    count=$(db_get_value "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='$table'")
    [[ "$count" -gt 0 ]]
}

# Get list of tables
db_list_tables() {
    db_query "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
}

# Get table row count
# Args: $1 = table name
db_table_count() {
    local table="$1"
    db_get_value "SELECT COUNT(*) FROM $table"
}

# ============================================================================
# FTS5 OPERATIONS
# ============================================================================

# Rebuild FTS5 index
# Args: $1 = FTS table name
db_fts_rebuild() {
    local fts_table="$1"
    if db_query "INSERT INTO ${fts_table}(${fts_table}) VALUES('rebuild');" >/dev/null; then
        db_log_operation "FTS_REBUILD" "SUCCESS" "$fts_table"
        return 0
    else
        db_log_operation "FTS_REBUILD" "FAILED" "$fts_table"
        return 1
    fi
}

# Optimize FTS5 index
# Args: $1 = FTS table name
db_fts_optimize() {
    local fts_table="$1"
    if db_query "INSERT INTO ${fts_table}(${fts_table}) VALUES('optimize');" >/dev/null; then
        db_log_operation "FTS_OPTIMIZE" "SUCCESS" "$fts_table"
        return 0
    else
        db_log_operation "FTS_OPTIMIZE" "FAILED" "$fts_table"
        return 1
    fi
}

# ============================================================================
# BACKUP OPERATIONS
# ============================================================================

# Create database backup
# Args: $1 = backup path (optional, defaults to timestamped backup)
db_backup() {
    local backup_path="${1:-$REPO_ROOT/.state/db/backups/codeflow-$(date +%Y%m%d_%H%M%S).db}"

    # Ensure backup directory exists
    mkdir -p "$(dirname "$backup_path")"

    if sqlite3 "$DB_FILE" ".backup '$backup_path'" 2>/dev/null; then
        db_log_operation "BACKUP" "SUCCESS" "$backup_path"
        echo "$backup_path"
        return 0
    else
        db_log_operation "BACKUP" "FAILED" "$backup_path"
        return 1
    fi
}

# ============================================================================
# EXPORT FUNCTIONS
# ============================================================================

# List of exported functions
export -f db_log_operation
export -f db_query
export -f db_execute
export -f db_transaction
export -f db_get_value
export -f db_count
export -f db_check_integrity
export -f db_exists
export -f db_get_schema_version
export -f db_enable_wal
export -f db_checkpoint
export -f db_vacuum
export -f db_analyze
export -f db_size
export -f db_wal_size
export -f db_table_exists
export -f db_list_tables
export -f db_table_count
export -f db_fts_rebuild
export -f db_fts_optimize
export -f db_backup

# Export configuration
export DB_FILE
export DB_TIMEOUT
export DB_RETRIES
export DB_RETRY_DELAY
export DB_OP_LOG
