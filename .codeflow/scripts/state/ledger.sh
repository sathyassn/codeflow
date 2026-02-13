#!/usr/bin/env bash
# ledger.sh - JSONL ledger operations for shell scripts.
#
# Location: .codeflow/scripts/state/ledger.sh
#
# Provides functions for JSONL ledger management:
#   - append_ledger()    Append event to ledger file
#   - read_ledger()      Read events from ledger
#   - tail_ledger()      Get recent events
#   - filter_ledger()    Get events by type
#   - count_ledger()     Count events in ledger
#   - validate_ledger()  Validate JSONL format

# Source guard to prevent multiple loads
[[ -n "${_CODEFLOW_LEDGER_LOADED:-}" ]] && return 0
_CODEFLOW_LEDGER_LOADED=1

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
    local ledger_file="${1:-}"
    local event_type="${2:-}"
    local data="${3:-}"

    if [[ -z "$ledger_file" || -z "$event_type" ]]; then
        echo "Error: append_ledger requires ledger_file and event_type" >&2
        return 1
    fi

    local timestamp
    timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    local full_path="$LEDGER_PATH/$ledger_file"

    # Ensure directory exists
    mkdir -p "$(dirname "$full_path")"

    # Build JSON line
    # Note: data should be pre-formatted as "key":"value","key2":"value2"
    local line
    if [[ -n "$data" ]]; then
        line="{\"ts\":\"$timestamp\",\"e\":\"$event_type\",$data}"
    else
        line="{\"ts\":\"$timestamp\",\"e\":\"$event_type\"}"
    fi

    # Use flock for parallel safety when available
    if command -v flock >/dev/null 2>&1; then
        (
            flock -x 200
            echo "$line" >> "$full_path"
        ) 200>"$full_path.lock"
    else
        echo "$line" >> "$full_path"
    fi
}

# Append with full JSON event
# Args: $1=ledger_file, $2=full JSON event (must include "ts" if not auto-generated)
append_event() {
    local ledger_file="${1:-}"
    local event="${2:-}"

    if [[ -z "$ledger_file" || -z "$event" ]]; then
        echo "Error: append_event requires ledger_file and event" >&2
        return 1
    fi

    local full_path="$LEDGER_PATH/$ledger_file"
    mkdir -p "$(dirname "$full_path")"

    # If event doesn't have timestamp, add one
    if [[ "$event" != *'"ts"'* ]]; then
        local timestamp
        timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
        # Prepend timestamp to JSON object
        event="{\"ts\":\"$timestamp\",${event#\{}"
    fi

    # Use flock for parallel safety when available
    if command -v flock >/dev/null 2>&1; then
        (
            flock -x 200
            echo "$event" >> "$full_path"
        ) 200>"$full_path.lock"
    else
        echo "$event" >> "$full_path"
    fi
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
    local ledger_file="${1:-}"
    local full_path="$LEDGER_PATH/$ledger_file"

    if [[ -z "$ledger_file" ]]; then
        echo "Error: validate_ledger requires ledger_file" >&2
        return 1
    fi

    if [[ ! -f "$full_path" ]]; then
        echo "File not found: $full_path"
        return 1
    fi

    local line_count
    line_count=$(wc -l < "$full_path" | tr -d ' ')

    if [[ "$line_count" -eq 0 ]]; then
        echo "Ledger valid: $ledger_file (0 lines)"
        return 0
    fi

    # Single-process validation (much faster than per-line python3)
    local output
    if output=$(python3 -c "
import sys, json
errors = 0
line_count = 0
with open(sys.argv[1]) as f:
    for i, line in enumerate(f, 1):
        line = line.strip()
        if line:
            line_count += 1
            try:
                json.loads(line)
            except json.JSONDecodeError:
                print(f'Invalid JSON at line {i}: {line}')
                errors += 1
if errors:
    print(f'Ledger has {errors} errors')
    sys.exit(1)
print(f'Ledger valid: {sys.argv[2]} ({line_count} lines)')
" "$full_path" "$ledger_file" 2>&1); then
        echo "$output"
        return 0
    else
        echo "$output"
        return 1
    fi
}

# ============================================================================
# WORK GRAPH EVENT HELPERS
# ============================================================================

# Record epic created event
# Args: epic_id title area_type work_type domain [is_ongoing] [priority] [file_scope] [format_id]
#   epic_id:   ULID PK (epic-{ulid}) — used for DB FK references
#   format_id: Human-readable ID ({AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}) — used for display/filenames
record_epic_created() {
    local epic_id="$1" title="$2" area_type="$3" work_type="$4" domain="$5"
    local is_ongoing="${6:-false}" priority="${7:-normal}" file_scope="${8:-}" format_id="${9:-}"

    # Escape quotes in title
    title=$(echo "$title" | sed 's/"/\\"/g')

    local ongoing_bool="false"
    [[ "$is_ongoing" == "true" ]] && ongoing_bool="true"

    local data="\"id\":\"$epic_id\",\"title\":\"$title\",\"area_type\":\"$area_type\""
    data="$data,\"work_type\":\"$work_type\",\"domain\":\"$domain\""
    data="$data,\"is_ongoing\":$ongoing_bool,\"priority\":\"$priority\""
    [[ -n "$file_scope" ]] && data="$data,\"file_scope\":\"$file_scope\""
    [[ -n "$format_id" ]] && data="$data,\"format_id\":\"$format_id\""

    append_ledger "$LEDGER_WORK_GRAPH" "epic_created" "$data"
}

# Record task created event
# Args: task_id epic_id title area_type work_type domain [status] [priority] [format_id]
#   task_id:   ULID PK (task-{ulid}) — used for DB FK references
#   epic_id:   ULID PK (epic-{ulid}) — parent epic reference
#   format_id: Human-readable ID ({AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}) — used for display/filenames
record_task_created() {
    local task_id="$1" epic_id="$2" title="$3" area_type="$4" work_type="$5" domain="$6"
    local status="${7:-todo}" priority="${8:-normal}" format_id="${9:-}"

    # Escape quotes in title
    title=$(echo "$title" | sed 's/"/\\"/g')

    local data="\"id\":\"$task_id\",\"epic_id\":\"$epic_id\",\"title\":\"$title\""
    data="$data,\"area_type\":\"$area_type\",\"work_type\":\"$work_type\""
    data="$data,\"domain\":\"$domain\",\"status\":\"$status\",\"priority\":\"$priority\""
    [[ -n "$format_id" ]] && data="$data,\"format_id\":\"$format_id\""

    append_ledger "$LEDGER_WORK_GRAPH" "task_created" "$data"
}

# Record task status change event
# Args: task_id new_status [old_status] [format_id]
#   task_id:   ULID PK (task-{ulid})
#   format_id: Human-readable ID ({AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}) — optional, for display
record_task_status_changed() {
    local task_id="$1" new_status="$2" old_status="${3:-}" format_id="${4:-}"

    local data="\"task_id\":\"$task_id\",\"new_status\":\"$new_status\""
    [[ -n "$old_status" ]] && data="$data,\"old_status\":\"$old_status\""
    [[ -n "$format_id" ]] && data="$data,\"format_id\":\"$format_id\""

    append_ledger "$LEDGER_WORK_GRAPH" "task_status_changed" "$data"
}

# Record epic status change event
# Args: epic_id new_status [old_status] [format_id]
#   epic_id:   ULID PK (epic-{ulid})
#   format_id: Human-readable ID ({AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}) — optional, for display
record_epic_status_changed() {
    local epic_id="$1" new_status="$2" old_status="${3:-}" format_id="${4:-}"

    local data="\"epic_id\":\"$epic_id\",\"new_status\":\"$new_status\""
    [[ -n "$old_status" ]] && data="$data,\"old_status\":\"$old_status\""
    [[ -n "$format_id" ]] && data="$data,\"format_id\":\"$format_id\""

    append_ledger "$LEDGER_WORK_GRAPH" "epic_status_changed" "$data"
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
export -f record_epic_created
export -f record_task_created
export -f record_task_status_changed
export -f record_epic_status_changed

# Export ledger file names
export LEDGER_CONFIG
export LEDGER_WORK_GRAPH
export LEDGER_MEMORY
export LEDGER_SESSIONS
export LEDGER_PATH
