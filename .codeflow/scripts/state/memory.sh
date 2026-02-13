#!/usr/bin/env bash
# memory.sh - Memory management library for shell scripts.
#
# Location: .codeflow/scripts/state/memory.sh
#
# Provides functions for memory event management:
#   - init_db()        Initialize database via Go CLI
#   - record_event()   Record a memory event (JSONL ledger)
#   - query_events()   Query events via Go CLI
#   - search_memory()  Full-text search via Go CLI
#
# DB access policy: All SQLite operations go through the Go CLI (`codeflow db`).
# Shell scripts write JSONL ledger files only. Direct sqlite3 calls are prohibited.

set -euo pipefail

# Load shell library
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

# Source dependencies if available
if [[ -f "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh" ]]; then
    source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
fi


# Configuration
LEDGER_PATH="${CODEFLOW_LEDGER_PATH:-$REPO_ROOT/.state/ledger}"

# ============================================================================
# DATABASE INITIALIZATION
# ============================================================================

# Initialize database with schema (delegates to Go CLI)
init_db() {
    local schema_path="${1:-$REPO_ROOT/.codeflow/scripts/db/schema.sql}"

    if [[ ! -f "$schema_path" ]]; then
        echo "Error: Schema file not found: $schema_path" >&2
        return 1
    fi

    if command -v codeflow >/dev/null 2>&1; then
        codeflow db init --schema "$schema_path"
    else
        echo "Error: codeflow CLI not found. DB operations require the Go CLI." >&2
        return 1
    fi
}

# ============================================================================
# MEMORY EVENT OPERATIONS
# ============================================================================

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
        progress|decision|milestone|blocker|stage_transition|stage_complete|rework_limit) ;;
        *)
            echo "Error: record_event: Invalid event_type: $event_type" >&2
            return 1
            ;;
    esac

    # Validate domain
    case "$domain" in
        planning|development|review|qa|ops|documentation) ;;
        *)
            echo "Error: record_event: Invalid domain: $domain" >&2
            return 1
            ;;
    esac

    # Validate data is valid JSON
    if command -v jq >/dev/null 2>&1; then
        if ! echo "$data" | jq empty 2>/dev/null; then
            echo "Error: record_event: data must be valid JSON" >&2
            return 1
        fi
    fi

    # Generate event ID using shell-lib ULID (falls back to timestamp+random)
    local event_id
    if type generate_ulid >/dev/null 2>&1; then
        event_id="memory-$(generate_ulid)"
    else
        event_id="memory-$(date +%s)$(printf '%06d' $RANDOM)"
    fi
    local timestamp
    timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    # Build JSON line, omitting empty optional fields (matches cf-memory-store.py)
    local json_line="{\"ts\":\"$timestamp\",\"type\":\"memory_stored\",\"id\":\"$event_id\",\"event_type\":\"$event_type\",\"domain\":\"$domain\""
    [[ -n "$work_id" ]] && json_line="$json_line,\"work_id\":\"$work_id\""
    [[ -n "$memory_type" ]] && json_line="$json_line,\"memory_type\":\"$memory_type\""
    json_line="$json_line,\"data\":$data}"

    # Append to JSONL ledger with flock for parallel safety
    local ledger_file="$LEDGER_PATH/memory-events.jsonl"
    mkdir -p "$(dirname "$ledger_file")"

    if command -v flock >/dev/null 2>&1; then
        (
            flock -x 200
            echo "$json_line" >> "$ledger_file"
        ) 200>"$ledger_file.lock"
    else
        echo "$json_line" >> "$ledger_file"
    fi

    echo "$event_id"
}

# Query events by domain and/or type (delegates to Go CLI)
# Args: $1=domain (optional), $2=event_type (optional), $3=limit (optional, default 100)
query_events() {
    local domain="${1:-}"
    local event_type="${2:-}"
    local limit="${3:-100}"

    if ! command -v codeflow >/dev/null 2>&1; then
        echo "Error: codeflow CLI not found. DB queries require the Go CLI." >&2
        return 1
    fi

    local args=("db" "query" "memory_events" "--limit" "$limit" "--format" "json")
    [[ -n "$domain" ]] && args+=("--where" "domain=$domain")
    [[ -n "$event_type" ]] && args+=("--where" "event_type=$event_type")

    codeflow "${args[@]}"
}

# Full-text search memory events (delegates to Go CLI)
# Args: $1=query, $2=limit (optional, default 50)
search_memory() {
    local query="$1"
    local limit="${2:-50}"

    if ! command -v codeflow >/dev/null 2>&1; then
        echo "Error: codeflow CLI not found. DB queries require the Go CLI." >&2
        return 1
    fi

    codeflow db search memory_events --query "$query" --limit "$limit" --format json
}

# Get memory event by ID (delegates to Go CLI)
# Args: $1=event_id
get_event() {
    local event_id="$1"

    if ! command -v codeflow >/dev/null 2>&1; then
        echo "Error: codeflow CLI not found. DB queries require the Go CLI." >&2
        return 1
    fi

    codeflow db get memory_events --id "$event_id" --format json
}

# Count memory events (delegates to Go CLI)
# Args: $1=domain (optional), $2=event_type (optional)
count_events() {
    local domain="${1:-}"
    local event_type="${2:-}"

    if ! command -v codeflow >/dev/null 2>&1; then
        echo "Error: codeflow CLI not found. DB queries require the Go CLI." >&2
        return 1
    fi

    local args=("db" "count" "memory_events")
    [[ -n "$domain" ]] && args+=("--where" "domain=$domain")
    [[ -n "$event_type" ]] && args+=("--where" "event_type=$event_type")

    codeflow "${args[@]}"
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
