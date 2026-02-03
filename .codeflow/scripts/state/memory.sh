#!/usr/bin/env bash
# memory.sh - Memory management library for shell scripts.
#
# Location: .codeflow/scripts/state/memory.sh
#
# Provides functions for memory event management:
#   - init_db()        Initialize database with schema
#   - record_event()   Record a memory event
#   - query_events()   Query events by domain/type
#   - search_memory()  Full-text search

set -euo pipefail

# Load shell library
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

# Source dependencies if available
if [[ -f "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh" ]]; then
    source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
fi

if [[ -f "$REPO_ROOT/.codeflow/scripts/db/lib/db-lib.sh" ]]; then
    source "$REPO_ROOT/.codeflow/scripts/db/lib/db-lib.sh"
fi

# Configuration
DB_PATH="${CODEFLOW_DB_FILE:-$REPO_ROOT/.state/db/codeflow.db}"
LEDGER_PATH="${CODEFLOW_LEDGER_PATH:-$REPO_ROOT/.state/ledger}"

# ============================================================================
# DATABASE INITIALIZATION
# ============================================================================

# Initialize database with schema
init_db() {
    local schema_path="${1:-$REPO_ROOT/.state/db/schema.sql}"

    if [[ ! -f "$schema_path" ]]; then
        echo "Error: Schema file not found: $schema_path" >&2
        return 1
    fi

    # Ensure directory exists
    mkdir -p "$(dirname "$DB_PATH")"

    sqlite3 "$DB_PATH" < "$schema_path"
    echo "Database initialized: $DB_PATH"
}

# ============================================================================
# MEMORY EVENT OPERATIONS
# ============================================================================

# Generate ULID (requires Python)
_generate_ulid() {
    python3 -c "
import sys
sys.path.insert(0, '$REPO_ROOT/.codeflow/scripts/codeflow_py_lib')
from codeflow_py_lib import generate_ulid
print(generate_ulid())
" 2>/dev/null || echo "$(date +%s)$(printf '%06d' $RANDOM)"
}

# Record a memory event
# Args: $1=event_type, $2=domain, $3=data (JSON), $4=work_id (optional), $5=memory_type (optional)
# Returns: event ID
record_event() {
    local event_type="$1"
    local domain="$2"
    local data="$3"
    local work_id="${4:-}"
    local memory_type="${5:-}"

    # Validate event_type
    case "$event_type" in
        progress|decision|milestone|blocker) ;;
        *)
            echo "Error: Invalid event_type: $event_type" >&2
            return 1
            ;;
    esac

    # Validate domain
    case "$domain" in
        planning|development|review|qa|ops|documentation) ;;
        *)
            echo "Error: Invalid domain: $domain" >&2
            return 1
            ;;
    esac

    # Generate event ID
    local event_id
    event_id="memory-$(_generate_ulid)"
    local timestamp
    timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    # Insert into database
    sqlite3 "$DB_PATH" <<EOF
INSERT INTO memory_events (id, event_type, domain, work_id, data, memory_type, created_at)
VALUES ('$event_id', '$event_type', '$domain', $([ -n "$work_id" ] && echo "'$work_id'" || echo "NULL"), '$data', $([ -n "$memory_type" ] && echo "'$memory_type'" || echo "NULL"), '$timestamp');
EOF

    # Append to JSONL ledger
    local ledger_file="$LEDGER_PATH/memory-events.jsonl"
    mkdir -p "$(dirname "$ledger_file")"
    echo "{\"ts\":\"$timestamp\",\"type\":\"memory_stored\",\"id\":\"$event_id\",\"event_type\":\"$event_type\",\"domain\":\"$domain\",\"work_id\":\"$work_id\",\"data\":$data}" >> "$ledger_file"

    echo "$event_id"
}

# Query events by domain and/or type
# Args: $1=domain (optional), $2=event_type (optional), $3=limit (optional, default 100)
query_events() {
    local domain="${1:-}"
    local event_type="${2:-}"
    local limit="${3:-100}"

    local where_clause="1=1"
    [[ -n "$domain" ]] && where_clause="$where_clause AND domain='$domain'"
    [[ -n "$event_type" ]] && where_clause="$where_clause AND event_type='$event_type'"

    sqlite3 -json "$DB_PATH" <<EOF
SELECT id, event_type, domain, work_id, data, memory_type, created_at
FROM memory_events
WHERE $where_clause
ORDER BY created_at DESC
LIMIT $limit;
EOF
}

# Full-text search memory events
# Args: $1=query, $2=limit (optional, default 50)
search_memory() {
    local query="$1"
    local limit="${2:-50}"

    sqlite3 -json "$DB_PATH" <<EOF
SELECT m.id, m.event_type, m.domain, m.work_id, m.data, m.memory_type, m.created_at,
       bm25(memory_fts) as score
FROM memory_fts f
JOIN memory_events m ON f.id = m.id
WHERE memory_fts MATCH '$query'
ORDER BY score
LIMIT $limit;
EOF
}

# Get memory event by ID
# Args: $1=event_id
get_event() {
    local event_id="$1"

    sqlite3 -json "$DB_PATH" <<EOF
SELECT id, event_type, domain, work_id, data, memory_type, created_at
FROM memory_events
WHERE id = '$event_id';
EOF
}

# Count memory events
# Args: $1=domain (optional), $2=event_type (optional)
count_events() {
    local domain="${1:-}"
    local event_type="${2:-}"

    local where_clause="1=1"
    [[ -n "$domain" ]] && where_clause="$where_clause AND domain='$domain'"
    [[ -n "$event_type" ]] && where_clause="$where_clause AND event_type='$event_type'"

    sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM memory_events WHERE $where_clause;"
}

# ============================================================================
# EXPORT FUNCTIONS
# ============================================================================

export -f init_db
export -f record_event
export -f query_events
export -f search_memory
export -f get_event
export -f count_events
