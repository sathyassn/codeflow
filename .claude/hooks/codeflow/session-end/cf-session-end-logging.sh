#!/usr/bin/env bash
# Purpose:   SessionEnd hook for session logging finalization
# Location:  .claude/hooks/codeflow/session-end/cf-session-end-logging.sh
# Hook Type: SessionEnd
# Usage:     Called by Claude Code at session end
# Platform:  macOS/Linux
# Version:   2.1.0
#
# This hook:
#   - Reads session_id and transcript_path from stdin JSON
#   - Sources codeflow-env.sh for canonical CODEFLOW_SESSION_ID
#   - Checks logging.session_end.enabled config
#   - Writes session end event with summary statistics to JSONL log
#   - Calculates session duration from metadata
#   - Updates session metadata with end time and duration
#   - Triggers log rotation if file size exceeds threshold
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging finalized successfully (always exits 0)

set -euo pipefail

# Read hook data from stdin (Claude Code protocol)
TRANSCRIPT_PATH=""
_stdin_sid=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && _stdin_sid="$_sid"
        _tp=$(echo "$_HOOK_STDIN" | jq -r '.transcript_path // empty' 2>/dev/null)
        [[ -n "$_tp" ]] && TRANSCRIPT_PATH="$_tp"
    fi
fi

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.1.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# Source env file for canonical CODEFLOW_SESSION_ID
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi

# Session ID priority: env file > stdin JSON > fallback
SESSION_ID="${CODEFLOW_SESSION_ID:-${_stdin_sid:-unknown}}"

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# =============================================================================
# CONFIGURATION
# =============================================================================

# Check if session-end logging is enabled (default: true)
SESSION_END_ENABLED="true"
GENERATE_SUMMARY="true"
TRIGGER_ROTATION="true"
LOG_DIR="$REPO_ROOT/.state/logs/sessions"
MAX_LOG_SIZE_MB=100
MAX_AGE_DAYS=30
MAX_LOGS=100

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Check enabled flag - try session_end first, then fall back
    _enabled=$(jq -r '.logging.session_end.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_enabled" == "false" ]] && SESSION_END_ENABLED="false"

    # Check generate_summary flag
    _summary=$(jq -r '.logging.session_end.generate_summary // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_summary" == "false" ]] && GENERATE_SUMMARY="false"

    # Check trigger_rotation flag
    _rotation=$(jq -r '.logging.session_end.trigger_rotation // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_rotation" == "false" ]] && TRIGGER_ROTATION="false"

    # Read log directory: try session_end-specific, then shared logging.log_directory, then session_start
    CONFIG_LOG_DIR=$(jq -r '
        .logging.session_end.log_directory //
        .logging.log_directory //
        .logging.session_start.log_directory //
        ".state/logs/sessions"
    ' "$CONFIG" 2>/dev/null || echo ".state/logs/sessions")
    LOG_DIR="$REPO_ROOT/$CONFIG_LOG_DIR"

    # Rotation settings: try session_end first, then fall back to session_start
    MAX_LOG_SIZE_MB=$(jq -r '.logging.session_end.rotation.max_size_mb // .logging.session_start.rotation.max_size_mb // 100' "$CONFIG" 2>/dev/null || echo "100")
    MAX_AGE_DAYS=$(jq -r '.logging.session_end.rotation.max_age_days // .logging.session_start.rotation.max_age_days // 30' "$CONFIG" 2>/dev/null || echo "30")
    MAX_LOGS=$(jq -r '.logging.session_end.rotation.max_logs // .logging.session_start.rotation.max_logs // 100' "$CONFIG" 2>/dev/null || echo "100")
fi

# Exit early if logging is disabled
if [[ "$SESSION_END_ENABLED" == "false" ]]; then
    exit 0
fi

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

# Cross-platform epoch time parsing
# Tries to parse ISO timestamp to epoch, returns 0 on failure
parse_iso_to_epoch() {
    local iso_ts="$1"
    local epoch=0

    # Try GNU date first (Linux)
    if command -v gdate &>/dev/null; then
        epoch=$(gdate -d "$iso_ts" +%s 2>/dev/null || echo "0")
    elif date --version 2>/dev/null | grep -q "GNU"; then
        epoch=$(date -d "$iso_ts" +%s 2>/dev/null || echo "0")
    else
        # macOS/BSD date - try parsing ISO format
        # Strip timezone and milliseconds for BSD date compatibility
        local clean_ts
        clean_ts=$(echo "$iso_ts" | sed 's/\.[0-9]*Z$//' | sed 's/Z$//' | sed 's/T/ /')
        epoch=$(date -j -f "%Y-%m-%d %H:%M:%S" "$clean_ts" +%s 2>/dev/null || echo "0")
    fi

    echo "$epoch"
}

# Gather summary statistics from transcript file
# Outputs JSON object with tool_calls, files_modified, tasks_completed, memory_entries
gather_summary_stats() {
    local transcript="$1"
    local tool_calls_json="{}"
    local files_json="[]"
    local tasks_completed=0
    local memory_entries=0

    if [[ -f "$transcript" ]] && command -v jq &>/dev/null; then
        # Count tool calls by type from tool_use entries
        tool_calls_json=$(jq -s '
            [.[] | select(.type == "tool_use" or .type == "assistant" and .content) |
             if .type == "tool_use" then .name
             elif .content then [.content[] | select(.type == "tool_use") | .name] | .[]
             else empty end
            ] | group_by(.) | map({(.[0]): length}) | add // {}
        ' "$transcript" 2>/dev/null || echo "{}")

        # Extract unique file paths from Edit/Write tool calls
        files_json=$(jq -s '
            [.[] |
             if .type == "tool_use" and (.name == "Edit" or .name == "Write") then
                .input.file_path // empty
             elif .type == "assistant" and .content then
                [.content[] | select(.type == "tool_use" and (.name == "Edit" or .name == "Write")) |
                 .input.file_path // empty] | .[]
             else empty end
            ] | unique
        ' "$transcript" 2>/dev/null || echo "[]")

        # Count tasks completed (TaskUpdate with status=completed)
        tasks_completed=$(jq -s '
            [.[] |
             if .type == "tool_use" and .name == "TaskUpdate" and .input.status == "completed" then 1
             elif .type == "assistant" and .content then
                [.content[] | select(.type == "tool_use" and .name == "TaskUpdate" and .input.status == "completed")] | length
             else 0 end
            ] | add // 0
        ' "$transcript" 2>/dev/null || echo "0")

        # Count memory entries (Write to .claude/memory/)
        memory_entries=$(jq -s '
            [.[] |
             if .type == "tool_use" and .name == "Write" and (.input.file_path // "" | test("\\.claude/memory/")) then 1
             elif .type == "assistant" and .content then
                [.content[] | select(.type == "tool_use" and .name == "Write" and (.input.file_path // "" | test("\\.claude/memory/")))] | length
             else 0 end
            ] | add // 0
        ' "$transcript" 2>/dev/null || echo "0")
    fi

    # Build summary JSON
    jq -nc \
        --argjson tool_calls "$tool_calls_json" \
        --argjson files_modified "$files_json" \
        --argjson tasks_completed "$tasks_completed" \
        --argjson memory_entries "$memory_entries" \
        '{tool_calls: $tool_calls, files_modified: $files_modified, tasks_completed: $tasks_completed, memory_entries: $memory_entries}'
}

# =============================================================================
# SESSION END EVENT
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/session-$LOG_DATE.jsonl"

# Calculate duration if session metadata exists
# Meta file uses CODEFLOW_SESSION_ID (written by init hook as session-{ses-...}.meta)
SESSION_META_FILE="$LOG_DIR/session-${SESSION_ID}.meta"
DURATION_SECONDS=0

if [[ -f "$SESSION_META_FILE" ]] && command -v jq &>/dev/null; then
    STARTED_AT=$(jq -r '.started_at // empty' "$SESSION_META_FILE" 2>/dev/null || echo "")
    # Also try started_epoch if stored directly
    STARTED_EPOCH=$(jq -r '.started_epoch // empty' "$SESSION_META_FILE" 2>/dev/null || echo "")

    if [[ -n "$STARTED_EPOCH" ]] && [[ "$STARTED_EPOCH" != "null" ]]; then
        # Use stored epoch directly (most reliable)
        NOW_EPOCH=$(date +%s)
        DURATION_SECONDS=$((NOW_EPOCH - STARTED_EPOCH))
    elif [[ -n "$STARTED_AT" ]] && [[ "$STARTED_AT" != "null" ]]; then
        # Parse ISO timestamp
        START_EPOCH=$(parse_iso_to_epoch "$STARTED_AT")
        NOW_EPOCH=$(date +%s)
        if [[ "$START_EPOCH" != "0" ]]; then
            DURATION_SECONDS=$((NOW_EPOCH - START_EPOCH))
        fi
    fi
fi

# Calculate duration in minutes for summary
DURATION_MINUTES=0
if [[ "$DURATION_SECONDS" -gt 0 ]]; then
    DURATION_MINUTES=$(( (DURATION_SECONDS + 30) / 60 ))  # round to nearest minute
    [[ "$DURATION_MINUTES" -lt 1 ]] && DURATION_MINUTES=1
fi

# Gather summary statistics if enabled
SUMMARY_JSON="{}"
if [[ "$GENERATE_SUMMARY" == "true" ]] && [[ -n "$TRANSCRIPT_PATH" ]] && [[ -f "$TRANSCRIPT_PATH" ]]; then
    SUMMARY_JSON=$(gather_summary_stats "$TRANSCRIPT_PATH")
fi

# Add duration_minutes to summary
if command -v jq &>/dev/null; then
    SUMMARY_JSON=$(echo "$SUMMARY_JSON" | jq --argjson dm "$DURATION_MINUTES" '. + {duration_minutes: $dm}' 2>/dev/null || echo "$SUMMARY_JSON")
fi

# Write session end event
if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg event "session_end" \
        --argjson duration_seconds "$DURATION_SECONDS" \
        --argjson summary "$SUMMARY_JSON" \
        '{event: $event, session_id: $session_id, timestamp: $timestamp, duration_seconds: $duration_seconds, summary: $summary}')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# UPDATE SESSION METADATA
# =============================================================================

if [[ -f "$SESSION_META_FILE" ]] && command -v jq &>/dev/null; then
    # Update metadata with end time and duration
    TMP_FILE=$(mktemp)
    if jq \
        --arg ended_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --argjson duration_seconds "$DURATION_SECONDS" \
        --argjson ended_epoch "$(date +%s)" \
        '. + {ended_at: $ended_at, duration_seconds: $duration_seconds, ended_epoch: $ended_epoch}' \
        "$SESSION_META_FILE" > "$TMP_FILE" 2>/dev/null; then
        mv "$TMP_FILE" "$SESSION_META_FILE" 2>/dev/null || rm -f "$TMP_FILE"
    else
        rm -f "$TMP_FILE"
    fi
fi

# =============================================================================
# LOG ROTATION
# =============================================================================

if [[ "$TRIGGER_ROTATION" == "true" ]] && [[ -d "$LOG_DIR" ]]; then
    # Check if current log file exceeds size threshold
    if [[ -f "$LOG_FILE" ]]; then
        FILE_SIZE_KB=$(du -k "$LOG_FILE" 2>/dev/null | cut -f1 || echo "0")
        MAX_SIZE_KB=$((MAX_LOG_SIZE_MB * 1024))
        if [[ "$FILE_SIZE_KB" -gt "$MAX_SIZE_KB" ]]; then
            # Rotate: rename current file with timestamp suffix
            ROTATE_TS=$(date +%Y%m%d%H%M%S)
            mv "$LOG_FILE" "${LOG_FILE%.jsonl}-rotated-${ROTATE_TS}.jsonl" 2>/dev/null || true
        fi
    fi

    # Remove logs older than max age
    find "$LOG_DIR" -name "*.jsonl" -type f -mtime "+$MAX_AGE_DAYS" -delete 2>/dev/null || true

    # Keep only max_logs files (portable approach)
    LOG_COUNT=$(find "$LOG_DIR" -name "*.jsonl" -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$LOG_COUNT" -gt "$MAX_LOGS" ]]; then
        EXCESS=$((LOG_COUNT - MAX_LOGS))
        # shellcheck disable=SC2012  # ls is fine here for simple file listing
        ls -1t "$LOG_DIR"/*.jsonl 2>/dev/null | tail -n "$EXCESS" | xargs rm -f 2>/dev/null || true
    fi
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
