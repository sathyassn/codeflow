#!/usr/bin/env bash
# Purpose:   Stop hook for stop event logging
# Location:  .claude/hooks/codeflow/stop/cf-stop-logging.sh
# Hook Type: Stop
# Usage:     Called by Claude Code at stop event
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Logs stop events with context
#   - Captures stop reason and git state
#   - Records session end state to JSONL log
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging completed successfully (always exits 0)

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

# Get stop context from environment
STOP_REASON="${STOP_REASON:-unknown}"

# =============================================================================
# STOP EVENT LOGGING
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/stop-events-$LOG_DATE.jsonl"

# Get git status
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
GIT_STATUS=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
HAS_CHANGES="false"
if [[ "$GIT_STATUS" -gt 0 ]]; then
    HAS_CHANGES="true"
fi

if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg event "stop" \
        --arg reason "$STOP_REASON" \
        --arg git_branch "$GIT_BRANCH" \
        --arg has_uncommitted "$HAS_CHANGES" \
        '{ts: $ts, session_id: $session_id, event: $event, reason: $reason, git_branch: $git_branch, has_uncommitted_changes: ($has_uncommitted == "true")}')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
