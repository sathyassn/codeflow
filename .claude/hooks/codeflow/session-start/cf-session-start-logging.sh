#!/usr/bin/env bash
# Purpose:   SessionStart hook for session logging initialization
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-logging.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Creates session log files
#   - Writes session start event to JSONL log
#   - Initializes log rotation (removes old logs)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging initialized successfully (always exits 0)

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
LOG_DIR="$REPO_ROOT/.state/logs/sessions"

# Get session ID from environment (set by cleanup/init hook)
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# Read logging config with fallbacks
MAX_LOGS=100
MAX_AGE_DAYS=30

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    MAX_LOGS=$(jq -r '.logging.session_start.rotation.max_logs // 100' "$CONFIG" 2>/dev/null || echo "100")
    MAX_AGE_DAYS=$(jq -r '.logging.session_start.rotation.max_age_days // 30' "$CONFIG" 2>/dev/null || echo "30")
fi

# =============================================================================
# LOG DIRECTORY SETUP
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

# =============================================================================
# LOG ROTATION
# =============================================================================

# Remove old log files (cross-platform compatible)
if [[ -d "$LOG_DIR" ]]; then
    # Remove logs older than max age
    find "$LOG_DIR" -name "*.jsonl" -type f -mtime "+$MAX_AGE_DAYS" -delete 2>/dev/null || true

    # Keep only max_logs files (portable approach - no GNU -printf)
    LOG_COUNT=$(find "$LOG_DIR" -name "*.jsonl" -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$LOG_COUNT" -gt "$MAX_LOGS" ]]; then
        EXCESS=$((LOG_COUNT - MAX_LOGS))
        # Use ls -t for time-sorted listing (oldest last), portable across macOS/Linux
        # shellcheck disable=SC2012  # ls is fine here for simple file listing
        ls -1t "$LOG_DIR"/*.jsonl 2>/dev/null | tail -n "$EXCESS" | xargs rm -f 2>/dev/null || true
    fi
fi

# =============================================================================
# SESSION START EVENT
# =============================================================================

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/session-$LOG_DATE.jsonl"

# Get system info
HOSTNAME=$(hostname 2>/dev/null || echo "unknown")
PLATFORM=$(uname -s 2>/dev/null || echo "unknown")

if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg event "session_started" \
        --arg hostname "$HOSTNAME" \
        --arg platform "$PLATFORM" \
        --arg cwd "$REPO_ROOT" \
        '{ts: $ts, session_id: $session_id, event: $event, hostname: $hostname, platform: $platform, cwd: $cwd}')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
