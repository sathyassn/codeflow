#!/usr/bin/env bash
# Purpose:   PostToolUse hook for universal operation logging
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-logging.sh
# Hook Type: PostToolUse
# Matcher:   * (all tools)
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Logs all tool operations to audit trail
#   - Captures tool name, input, results, and duration
#   - Implements dual-write pattern (JSONL + SQLite)
#   - Redacts sensitive data (passwords, tokens, emails)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Always succeeds (logging should not block)

set -euo pipefail

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LOG_DIR="$REPO_ROOT/.state/logs/sessions"

# Get session ID from environment
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# Read log config
MAX_RESULT_SIZE=2000
REDACT_SENSITIVE=true
TOOLS_TO_LOG=""

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    MAX_RESULT_SIZE=$(jq -r '.logging.post_tool_use.max_result_size // 2000' "$CONFIG" 2>/dev/null || echo "2000")
    REDACT_SENSITIVE=$(jq -r '.logging.post_tool_use.redact_sensitive // true' "$CONFIG" 2>/dev/null || echo "true")
    TOOLS_TO_LOG=$(jq -r '.logging.post_tool_use.tools_to_log // [] | join(",")' "$CONFIG" 2>/dev/null || echo "")
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
TOOL_INPUT="${TOOL_INPUT:-}"
TOOL_RESULT="${TOOL_RESULT:-}"
TOOL_START_TIME="${TOOL_START_TIME:-}"
TOOL_END_TIME="${TOOL_END_TIME:-}"

# Early exit if no tool name
if [[ -z "$TOOL_NAME" ]]; then
    exit 0
fi

# Check if this tool should be logged
should_log() {
    local tool="$1"

    # If no specific tools configured, log all
    if [[ -z "$TOOLS_TO_LOG" ]]; then
        return 0
    fi

    # Check if tool is in the list
    if [[ ",$TOOLS_TO_LOG," == *",$tool,"* ]]; then
        return 0
    fi

    return 1
}

if ! should_log "$TOOL_NAME"; then
    exit 0
fi

# =============================================================================
# SENSITIVE DATA REDACTION
# =============================================================================

redact_sensitive_data() {
    local text="$1"

    if [[ "$REDACT_SENSITIVE" != "true" ]]; then
        echo "$text"
        return
    fi

    # Redact common sensitive patterns
    text=$(echo "$text" | sed -E 's/([Pp]assword[[:space:]]*[:=][[:space:]]*)[^[:space:]"]+/\1[REDACTED]/g')
    text=$(echo "$text" | sed -E 's/([Tt]oken[[:space:]]*[:=][[:space:]]*)[^[:space:]"]+/\1[REDACTED]/g')
    text=$(echo "$text" | sed -E 's/([Kk]ey[[:space:]]*[:=][[:space:]]*)[^[:space:]"]+/\1[REDACTED]/g')
    text=$(echo "$text" | sed -E 's/([Ss]ecret[[:space:]]*[:=][[:space:]]*)[^[:space:]"]+/\1[REDACTED]/g')
    text=$(echo "$text" | sed -E 's/Bearer [A-Za-z0-9_-]+/Bearer [REDACTED]/g')
    text=$(echo "$text" | sed -E 's/[A-Za-z0-9]{40,}/[LONG_TOKEN_REDACTED]/g')
    # Redact email addresses
    text=$(echo "$text" | sed -E 's/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/[EMAIL_REDACTED]/g')

    echo "$text"
}

# Calculate duration in milliseconds if timestamps provided
calculate_duration_ms() {
    local start_time="$1"
    local end_time="$2"

    # If no timestamps provided, return empty
    if [[ -z "$start_time" ]] || [[ -z "$end_time" ]]; then
        echo ""
        return
    fi

    # Try to calculate duration (timestamps expected in epoch milliseconds or ISO format)
    if [[ "$start_time" =~ ^[0-9]+$ ]] && [[ "$end_time" =~ ^[0-9]+$ ]]; then
        # Epoch milliseconds
        echo $((end_time - start_time))
    else
        # ISO format - try to convert using date command
        local start_epoch end_epoch
        start_epoch=$(date -j -f "%Y-%m-%dT%H:%M:%S" "${start_time%.*}" "+%s" 2>/dev/null || echo "")
        end_epoch=$(date -j -f "%Y-%m-%dT%H:%M:%S" "${end_time%.*}" "+%s" 2>/dev/null || echo "")

        if [[ -n "$start_epoch" ]] && [[ -n "$end_epoch" ]]; then
            echo $(( (end_epoch - start_epoch) * 1000 ))
        else
            echo ""
        fi
    fi
}

# =============================================================================
# LOG ENTRY CREATION
# =============================================================================

# Ensure log directory exists
mkdir -p "$LOG_DIR" 2>/dev/null || true

# Get current date for log file
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/tool-use-$LOG_DATE.jsonl"

# Truncate result if too long
RESULT_TRUNCATED="false"
LOG_RESULT="$TOOL_RESULT"
if [[ ${#LOG_RESULT} -gt $MAX_RESULT_SIZE ]]; then
    LOG_RESULT="${LOG_RESULT:0:$MAX_RESULT_SIZE}...[truncated]"
    RESULT_TRUNCATED="true"
fi

# Redact sensitive data
LOG_INPUT=$(redact_sensitive_data "$TOOL_INPUT")
LOG_RESULT=$(redact_sensitive_data "$LOG_RESULT")

# Calculate duration if timestamps available
DURATION_MS=$(calculate_duration_ms "$TOOL_START_TIME" "$TOOL_END_TIME")

# Create log entry
TS=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

if command -v jq &>/dev/null; then
    if [[ -n "$DURATION_MS" ]]; then
        LOG_ENTRY=$(jq -nc \
            --arg ts "$TS" \
            --arg session_id "$SESSION_ID" \
            --arg tool_name "$TOOL_NAME" \
            --arg tool_input "$LOG_INPUT" \
            --arg tool_result "$LOG_RESULT" \
            --arg truncated "$RESULT_TRUNCATED" \
            --argjson duration_ms "$DURATION_MS" \
            '{ts: $ts, session_id: $session_id, event: "tool_completed", tool_name: $tool_name, tool_input: $tool_input, tool_result: $tool_result, result_truncated: ($truncated == "true"), duration_ms: $duration_ms}')
    else
        LOG_ENTRY=$(jq -nc \
            --arg ts "$TS" \
            --arg session_id "$SESSION_ID" \
            --arg tool_name "$TOOL_NAME" \
            --arg tool_input "$LOG_INPUT" \
            --arg tool_result "$LOG_RESULT" \
            --arg truncated "$RESULT_TRUNCATED" \
            '{ts: $ts, session_id: $session_id, event: "tool_completed", tool_name: $tool_name, tool_input: $tool_input, tool_result: $tool_result, result_truncated: ($truncated == "true")}')
    fi
else
    # Fallback without jq - basic JSON
    LOG_ENTRY="{\"ts\":\"$TS\",\"session_id\":\"$SESSION_ID\",\"event\":\"tool_completed\",\"tool_name\":\"$TOOL_NAME\"}"
fi

# Write to JSONL file
echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true

# =============================================================================
# SQLITE LOGGING (BEST EFFORT)
# =============================================================================

DB_FILE="$REPO_ROOT/.state/db/codeflow.db"
if [[ -f "$DB_FILE" ]] && command -v sqlite3 &>/dev/null; then
    # Generate ID
    LOG_ID="toollog-$(date +%s%N | cut -c1-13)-$(head -c 4 /dev/urandom | od -An -tx1 | tr -d ' \n')"

    # Insert log entry
    sqlite3 "$DB_FILE" "
        INSERT OR IGNORE INTO tool_logs (id, session_id, tool_name, created_at)
        VALUES ('$LOG_ID', '$SESSION_ID', '$TOOL_NAME', '$TS');
    " 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
