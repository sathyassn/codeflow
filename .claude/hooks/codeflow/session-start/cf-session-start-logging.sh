#!/usr/bin/env bash
# Purpose:   SessionStart hook for session logging initialization
# Location:  see settings.json SessionStart hooks
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Reads session_id and permission_mode from stdin JSON (Claude Code protocol)
#   - Checks logging.session_start.enabled config
#   - Creates session log files
#   - Writes session start event with metadata to JSONL log
#   - Writes session ID to current-session.txt for other hooks
#   - Initializes log rotation (removes old logs)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging initialized successfully (always exits 0)

set -euo pipefail

# Read hook data from stdin (Claude Code protocol)
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
PERMISSION_MODE=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && SESSION_ID="$_sid"
        _pm=$(echo "$_HOOK_STDIN" | jq -r '.permission_mode // empty' 2>/dev/null)
        [[ -n "$_pm" ]] && PERMISSION_MODE="$_pm"
    fi
fi

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Source work-state library for active task functions
# shellcheck source=/dev/null
source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh" 2>/dev/null || true

# =============================================================================
# CONFIGURATION
# =============================================================================

# Defaults (V4 spec values)
LOGGING_ENABLED="true"
CAPTURE_METADATA="true"
LOG_DIR="$REPO_ROOT/.state/logs/sessions"
MAX_LOGS=50
MAX_AGE_DAYS=7

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Check enabled flag
    _enabled=$(jq -r '.logging.session_start.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_enabled" == "false" ]] && LOGGING_ENABLED="false"

    # Check capture_metadata flag
    _cm=$(jq -r '.logging.session_start.capture_metadata // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_cm" == "false" ]] && CAPTURE_METADATA="false"

    # Read log directory from config
    _log_dir=$(jq -r '.logging.session_start.log_directory // empty' "$CONFIG" 2>/dev/null)
    if [[ -n "$_log_dir" ]]; then
        # Absolute path: use as-is; relative path: prepend REPO_ROOT
        if [[ "$_log_dir" == /* ]]; then
            LOG_DIR="$_log_dir"
        else
            LOG_DIR="$REPO_ROOT/$_log_dir"
        fi
    fi

    # Rotation settings
    MAX_LOGS=$(jq -r '.logging.session_start.rotation.max_logs // 50' "$CONFIG" 2>/dev/null || echo "50")
    MAX_AGE_DAYS=$(jq -r '.logging.session_start.rotation.max_age_days // 7' "$CONFIG" 2>/dev/null || echo "7")
fi

# Exit early if logging is disabled
if [[ "$LOGGING_ENABLED" == "false" ]]; then
    exit 0
fi

# SESSION_ID is already set from stdin reading above (prefers stdin > env > "unknown")

# =============================================================================
# WRITE SESSION ID STATE FILE
# =============================================================================

# Write session ID to current-session.txt for other hooks to read
# Use session-scoped directory under .state
SESSION_STATE_DIR="$REPO_ROOT/.state/session/$SESSION_ID"
mkdir -p "$SESSION_STATE_DIR" 2>/dev/null || true
echo "$SESSION_ID" > "$SESSION_STATE_DIR/current-session.txt" 2>/dev/null || true

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

if command -v jq &>/dev/null; then
    if [[ "$CAPTURE_METADATA" == "true" ]]; then
        # Get git info
        GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
        GIT_COMMIT=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "unknown")
        APPROVAL_MODE="${PERMISSION_MODE:-standard}"

        # Check for active task (using work-state.sh canonical path)
        ACTIVE_TASK="null"
        _task_id=$(get_active_task_id)
        [[ -n "$_task_id" ]] && ACTIVE_TASK="\"$_task_id\""

        LOG_ENTRY=$(jq -nc \
            --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg session_id "$SESSION_ID" \
            --arg event "session_start" \
            --arg cwd "$REPO_ROOT" \
            --arg git_branch "$GIT_BRANCH" \
            --arg git_commit "$GIT_COMMIT" \
            --arg approval_mode "$APPROVAL_MODE" \
            --argjson active_task "$ACTIVE_TASK" \
            '{timestamp: $timestamp, session_id: $session_id, event: $event, metadata: {cwd: $cwd, git_branch: $git_branch, git_commit: $git_commit, approval_mode: $approval_mode, active_task: $active_task}}')
    else
        LOG_ENTRY=$(jq -nc \
            --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg session_id "$SESSION_ID" \
            --arg event "session_start" \
            '{timestamp: $timestamp, session_id: $session_id, event: $event}')
    fi

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
