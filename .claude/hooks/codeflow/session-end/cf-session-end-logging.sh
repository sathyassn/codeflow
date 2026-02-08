#!/usr/bin/env bash
# Purpose:   SessionEnd hook for session logging finalization
# Location:  .claude/hooks/codeflow/session-end/cf-session-end-logging.sh
# Hook Type: SessionEnd
# Usage:     Called by Claude Code at session end
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Writes session end event to JSONL log
#   - Calculates session duration from metadata
#   - Updates session metadata with end time and duration
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging finalized successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Read log directory from config or use default
LOG_DIR="$REPO_ROOT/.state/logs/sessions"
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    CONFIG_LOG_DIR=$(jq -r '.logging.session_start.log_directory // ".state/logs/sessions"' "$CONFIG" 2>/dev/null || echo ".state/logs/sessions")
    LOG_DIR="$REPO_ROOT/$CONFIG_LOG_DIR"
fi

# Get session ID from environment
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

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

# =============================================================================
# SESSION END EVENT
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/session-$LOG_DATE.jsonl"

# Calculate duration if session metadata exists
SESSION_META_FILE="$LOG_DIR/session-${SESSION_ID}.meta"
DURATION_SECONDS="unknown"

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

# Write session end event
if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg event "session_ended" \
        --arg duration "$DURATION_SECONDS" \
        '{ts: $ts, session_id: $session_id, event: $event, duration_seconds: $duration}')

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
        --arg duration "$DURATION_SECONDS" \
        --argjson ended_epoch "$(date +%s)" \
        '. + {ended_at: $ended_at, duration_seconds: $duration, ended_epoch: $ended_epoch}' \
        "$SESSION_META_FILE" > "$TMP_FILE" 2>/dev/null; then
        mv "$TMP_FILE" "$SESSION_META_FILE" 2>/dev/null || rm -f "$TMP_FILE"
    else
        rm -f "$TMP_FILE"
    fi
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
