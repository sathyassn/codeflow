#!/usr/bin/env bash
# ledger.sh - JSONL ledger operations for shell scripts.
#
# Location: .codeflow/scripts/state/ledger.sh
#
# Provides functions for JSONL ledger management:
#   - append_ledger()    Append event to ledger file
#   - read_ledger()      Read events from ledger
#   - tail_ledger()      Get recent events
#   - replay_ledger()    Replay events to rebuild state

set -euo pipefail

# Load shell library
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

# Source dependencies if available
if [[ -f "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh" ]]; then
    source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
fi

# Configuration
LEDGER_PATH="${CODEFLOW_LEDGER_PATH:-$REPO_ROOT/.state/ledger}"

# Ledger files
readonly LEDGER_CONFIG="config.jsonl"
readonly LEDGER_WORK_GRAPH="work-graph.jsonl"
readonly LEDGER_MEMORY="memory-events.jsonl"
readonly LEDGER_SESSIONS="sessions.jsonl"

# ============================================================================
# LEDGER OPERATIONS
# ============================================================================

# Ensure ledger directory and files exist
init_ledger() {
    mkdir -p "$LEDGER_PATH"

    # Create empty ledger files if they don't exist
    for file in "$LEDGER_CONFIG" "$LEDGER_WORK_GRAPH" "$LEDGER_MEMORY" "$LEDGER_SESSIONS"; do
        local path="$LEDGER_PATH/$file"
        [[ -f "$path" ]] || touch "$path"
    done

    echo "Ledger initialized: $LEDGER_PATH"
}

# Append event to ledger file
# Args: $1=ledger_file (e.g., "sessions.jsonl"), $2=event_type, $3=data (JSON key-value pairs)
append_ledger() {
    local ledger_file="$1"
    local event_type="$2"
    local data="$3"

    local timestamp
    timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    local full_path="$LEDGER_PATH/$ledger_file"

    # Ensure directory exists
    mkdir -p "$(dirname "$full_path")"

    # Build JSON line
    # Note: data should be pre-formatted as "key":"value","key2":"value2"
    echo "{\"ts\":\"$timestamp\",\"e\":\"$event_type\",$data}" >> "$full_path"
}

# Append with full JSON event
# Args: $1=ledger_file, $2=full JSON event (must include "ts" if not auto-generated)
append_event() {
    local ledger_file="$1"
    local event="$2"

    local full_path="$LEDGER_PATH/$ledger_file"
    mkdir -p "$(dirname "$full_path")"

    # If event doesn't have timestamp, add one
    if ! echo "$event" | grep -q '"ts"'; then
        local timestamp
        timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
        event=$(echo "$event" | sed "s/^{/{\"ts\":\"$timestamp\",/")
    fi

    echo "$event" >> "$full_path"
}

# Read all events from ledger
# Args: $1=ledger_file
read_ledger() {
    local ledger_file="$1"
    local full_path="$LEDGER_PATH/$ledger_file"

    [[ -f "$full_path" ]] && cat "$full_path"
}

# Get last N events from ledger
# Args: $1=ledger_file, $2=count (default 10)
tail_ledger() {
    local ledger_file="$1"
    local count="${2:-10}"
    local full_path="$LEDGER_PATH/$ledger_file"

    [[ -f "$full_path" ]] && tail -n "$count" "$full_path"
}

# Get events of a specific type
# Args: $1=ledger_file, $2=event_type
filter_ledger() {
    local ledger_file="$1"
    local event_type="$2"
    local full_path="$LEDGER_PATH/$ledger_file"

    [[ -f "$full_path" ]] && grep "\"e\":\"$event_type\"" "$full_path" || true
}

# Count events in ledger
# Args: $1=ledger_file
count_ledger() {
    local ledger_file="$1"
    local full_path="$LEDGER_PATH/$ledger_file"

    [[ -f "$full_path" ]] && wc -l < "$full_path" | tr -d ' ' || echo "0"
}

# Get ledger file size
# Args: $1=ledger_file
ledger_size() {
    local ledger_file="$1"
    local full_path="$LEDGER_PATH/$ledger_file"

    if [[ -f "$full_path" ]]; then
        stat -f%z "$full_path" 2>/dev/null || stat --printf="%s" "$full_path" 2>/dev/null || echo "0"
    else
        echo "0"
    fi
}

# ============================================================================
# SPECIALIZED LEDGER FUNCTIONS
# ============================================================================

# Log session start
# Args: $1=session_id, $2=agent_type
log_session_start() {
    local session_id="$1"
    local agent="${2:-developer}"

    append_ledger "$LEDGER_SESSIONS" "session_start" "\"sid\":\"$session_id\",\"agent\":\"$agent\""
}

# Log session end
# Args: $1=session_id, $2=work_id (optional), $3=status (optional)
log_session_end() {
    local session_id="$1"
    local work_id="${2:-}"
    local status="${3:-completed}"

    local data="\"sid\":\"$session_id\",\"status\":\"$status\""
    [[ -n "$work_id" ]] && data="$data,\"wid\":\"$work_id\""

    append_ledger "$LEDGER_SESSIONS" "session_end" "$data"
}

# Log work claimed
# Args: $1=session_id, $2=work_id
log_work_claimed() {
    local session_id="$1"
    local work_id="$2"

    append_ledger "$LEDGER_SESSIONS" "work_claimed" "\"sid\":\"$session_id\",\"wid\":\"$work_id\""
}

# Log progress event
# Args: $1=session_id, $2=work_id, $3=description
log_progress() {
    local session_id="$1"
    local work_id="$2"
    local description="$3"

    # Escape quotes in description
    description=$(echo "$description" | sed 's/"/\\"/g')

    append_ledger "$LEDGER_SESSIONS" "progress" "\"sid\":\"$session_id\",\"wid\":\"$work_id\",\"d\":\"$description\""
}

# ============================================================================
# BACKUP AND RECOVERY
# ============================================================================

# Backup ledger files
# Args: $1=backup_dir (optional)
backup_ledger() {
    local backup_dir="${1:-$LEDGER_PATH/backups}"
    local timestamp
    timestamp=$(date +%Y%m%d_%H%M%S)

    mkdir -p "$backup_dir"

    for file in "$LEDGER_CONFIG" "$LEDGER_WORK_GRAPH" "$LEDGER_MEMORY" "$LEDGER_SESSIONS"; do
        local src="$LEDGER_PATH/$file"
        if [[ -f "$src" && -s "$src" ]]; then
            cp "$src" "$backup_dir/${file%.jsonl}-$timestamp.jsonl"
        fi
    done

    echo "Ledger backed up to: $backup_dir"
}

# Validate JSONL file (check each line is valid JSON)
# Args: $1=ledger_file
validate_ledger() {
    local ledger_file="$1"
    local full_path="$LEDGER_PATH/$ledger_file"
    local errors=0
    local line_num=0

    if [[ ! -f "$full_path" ]]; then
        echo "File not found: $full_path"
        return 1
    fi

    while IFS= read -r line; do
        ((line_num++))
        if [[ -n "$line" ]]; then
            if ! echo "$line" | python3 -c "import sys, json; json.loads(sys.stdin.read())" 2>/dev/null; then
                echo "Invalid JSON at line $line_num: $line"
                ((errors++))
            fi
        fi
    done < "$full_path"

    if [[ $errors -eq 0 ]]; then
        echo "Ledger valid: $ledger_file ($line_num lines)"
        return 0
    else
        echo "Ledger has $errors errors"
        return 1
    fi
}

# ============================================================================
# EXPORT FUNCTIONS
# ============================================================================

export -f init_ledger
export -f append_ledger
export -f append_event
export -f read_ledger
export -f tail_ledger
export -f filter_ledger
export -f count_ledger
export -f ledger_size
export -f log_session_start
export -f log_session_end
export -f log_work_claimed
export -f log_progress
export -f backup_ledger
export -f validate_ledger

# Export ledger file names
export LEDGER_CONFIG
export LEDGER_WORK_GRAPH
export LEDGER_MEMORY
export LEDGER_SESSIONS
export LEDGER_PATH
