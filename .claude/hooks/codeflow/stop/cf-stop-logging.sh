#!/usr/bin/env bash
# Purpose:   Stop hook for stop event logging
# Location:  .claude/hooks/codeflow/stop/cf-stop-logging.sh
# Hook Type: Stop
# Usage:     Called by Claude Code at stop event
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Reads session_id and transcript_path from stdin JSON
#   - Captures stop reason, PCV status, and task context
#   - Writes stop event to session JSONL log
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Logging completed successfully (always exits 0)

set -euo pipefail

# =============================================================================
# STDIN READING (Claude Code protocol)
# =============================================================================

TRANSCRIPT_PATH=""
_stdin_sid=""
_STDIN_STOP_REASON=""
_STDIN_STOP_HOOK_ACTIVE=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && _stdin_sid="$_sid"
        _tp=$(echo "$_HOOK_STDIN" | jq -r '.transcript_path // empty' 2>/dev/null)
        [[ -n "$_tp" ]] && TRANSCRIPT_PATH="$_tp"
        _sr=$(echo "$_HOOK_STDIN" | jq -r '.stop_reason // empty' 2>/dev/null)
        [[ -n "$_sr" ]] && _STDIN_STOP_REASON="$_sr"
        _sha=$(echo "$_HOOK_STDIN" | jq -r '.stop_hook_active // empty' 2>/dev/null)
        [[ -n "$_sha" ]] && _STDIN_STOP_HOOK_ACTIVE="$_sha"
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

# Source env file for canonical CODEFLOW_SESSION_ID
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi

# Priority: env file > stdin > fallback
SESSION_ID="${CODEFLOW_SESSION_ID:-${_stdin_sid:-unknown}}"

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# =============================================================================
# CONFIGURATION
# =============================================================================

STOP_LOGGING_ENABLED="true"
CAPTURE_PCV_DETAILS="true"
CAPTURE_TASK_CONTEXT="true"
LOG_DIR="$REPO_ROOT/.state/logs/sessions"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Check enabled flag from logging.stop section (default: true)
    _enabled=$(jq -r '.logging.stop.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_enabled" == "false" ]] && STOP_LOGGING_ENABLED="false"

    # Check capture flags
    _pcv=$(jq -r '.logging.stop.capture_pcv_details // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_pcv" == "false" ]] && CAPTURE_PCV_DETAILS="false"

    _task=$(jq -r '.logging.stop.capture_task_context // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_task" == "false" ]] && CAPTURE_TASK_CONTEXT="false"

    # Read log directory: try stop-specific, then shared, then session_start fallback
    CONFIG_LOG_DIR=$(jq -r '
        .logging.stop.log_directory //
        .logging.log_directory //
        .logging.session_start.log_directory //
        ".state/logs/sessions"
    ' "$CONFIG" 2>/dev/null || echo ".state/logs/sessions")
    LOG_DIR="$REPO_ROOT/$CONFIG_LOG_DIR"
fi

# Exit early if logging is disabled
if [[ "$STOP_LOGGING_ENABLED" == "false" ]]; then
    exit 0
fi

# Get stop reason: prefer stdin, then environment, then fallback
if [[ -n "$_STDIN_STOP_REASON" ]]; then
    STOP_REASON="$_STDIN_STOP_REASON"
else
    STOP_REASON="${STOP_REASON:-unknown}"
fi

# =============================================================================
# PCV STATUS CAPTURE
# =============================================================================

PCV_STATUS_JSON="null"

if [[ "$CAPTURE_PCV_DETAILS" == "true" ]] && [[ -n "$TRANSCRIPT_PATH" ]] && [[ -f "$TRANSCRIPT_PATH" ]] && command -v jq &>/dev/null; then
    # Extract the current turn text (after last user message) from transcript
    # We look for PCV markers in assistant content from the last turn
    LAST_TURN_TEXT=$(jq -s '
        [.[] | select(.type == "assistant" and .message.content)] | last |
        if . then
            [.message.content[] | select(.type == "text") | .text] | join("\n")
        else "" end
    ' "$TRANSCRIPT_PATH" 2>/dev/null || echo "")

    if [[ -n "$LAST_TURN_TEXT" ]]; then
        # Check for PCV markers
        HAS_VERIFY_WORK="false"
        HAS_TIER="false"
        HAS_ARTIFACTS="false"
        HAS_VERIFICATION="false"
        HAS_ADVERSARIAL="false"
        TIER_VALUE="null"

        # Check each marker
        if echo "$LAST_TURN_TEXT" | grep -q "verify-work" 2>/dev/null; then
            HAS_VERIFY_WORK="true"
        fi
        if echo "$LAST_TURN_TEXT" | grep -q "TIER" 2>/dev/null; then
            HAS_TIER="true"
            # Extract tier number
            TIER_NUM=$(echo "$LAST_TURN_TEXT" | grep -o 'TIER [0-9]' 2>/dev/null | head -1 | grep -o '[0-9]' || echo "")
            if [[ -n "$TIER_NUM" ]]; then
                TIER_VALUE="$TIER_NUM"
            fi
        fi
        if echo "$LAST_TURN_TEXT" | grep -q "ARTIFACTS" 2>/dev/null; then
            HAS_ARTIFACTS="true"
        fi
        if echo "$LAST_TURN_TEXT" | grep -q "VERIFICATION" 2>/dev/null; then
            HAS_VERIFICATION="true"
        fi
        if echo "$LAST_TURN_TEXT" | grep -q "ADVERSARIAL" 2>/dev/null; then
            HAS_ADVERSARIAL="true"
        fi

        # Build markers_present and markers_missing arrays
        _present_items=""
        _missing_items=""

        if [[ "$HAS_VERIFY_WORK" == "true" ]]; then
            _present_items="${_present_items:+${_present_items},}\"verify-work\""
        else
            _missing_items="${_missing_items:+${_missing_items},}\"verify-work\""
        fi
        if [[ "$HAS_TIER" == "true" ]]; then
            _present_items="${_present_items:+${_present_items},}\"TIER\""
        else
            _missing_items="${_missing_items:+${_missing_items},}\"TIER\""
        fi
        if [[ "$HAS_ARTIFACTS" == "true" ]]; then
            _present_items="${_present_items:+${_present_items},}\"ARTIFACTS\""
        fi
        if [[ "$HAS_VERIFICATION" == "true" ]]; then
            _present_items="${_present_items:+${_present_items},}\"VERIFICATION\""
        fi
        if [[ "$HAS_ADVERSARIAL" == "true" ]]; then
            _present_items="${_present_items:+${_present_items},}\"ADVERSARIAL\""
        fi

        MARKERS_PRESENT="[${_present_items}]"
        MARKERS_MISSING="[${_missing_items}]"

        # Determine verified status
        VERIFIED="false"
        if [[ "$HAS_VERIFY_WORK" == "true" ]] && [[ "$HAS_TIER" == "true" ]]; then
            VERIFIED="true"
        fi

        # Build PCV status object
        PCV_STATUS_JSON=$(jq -nc \
            --argjson tier "$TIER_VALUE" \
            --argjson markers_present "$MARKERS_PRESENT" \
            --argjson markers_missing "$MARKERS_MISSING" \
            --argjson verified "$VERIFIED" \
            '{tier: $tier, markers_present: $markers_present, markers_missing: $markers_missing, verified: $verified}' 2>/dev/null || echo "null")
    fi

    # If no markers found at all, use default unverified status
    if [[ "$PCV_STATUS_JSON" == "null" ]]; then
        PCV_STATUS_JSON='{"tier":null,"markers_present":[],"markers_missing":["verify-work","TIER"],"verified":false}'
    fi
fi

# =============================================================================
# TASK CONTEXT CAPTURE
# =============================================================================

TASK_CONTEXT_JSON="null"

if [[ "$CAPTURE_TASK_CONTEXT" == "true" ]]; then
    ACTIVE_TASK_FILE="$REPO_ROOT/.state/runtime/active-task.json"
    if [[ -f "$ACTIVE_TASK_FILE" ]] && command -v jq &>/dev/null; then
        _task_id=$(jq -r '.task_id // empty' "$ACTIVE_TASK_FILE" 2>/dev/null)
        _task_status=$(jq -r '.status // empty' "$ACTIVE_TASK_FILE" 2>/dev/null)
        if [[ -n "$_task_id" ]]; then
            TASK_CONTEXT_JSON=$(jq -nc \
                --arg task_id "$_task_id" \
                --arg status "${_task_status:-unknown}" \
                '{task_id: $task_id, status: $status}' 2>/dev/null || echo "null")
        fi
    fi
fi

# =============================================================================
# DECISION DETECTION
# =============================================================================

DECISION="allow"

# =============================================================================
# GIT STATE
# =============================================================================

GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
GIT_STATUS=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ') || GIT_STATUS="0"
HAS_CHANGES="false"
if [[ "$GIT_STATUS" -gt 0 ]]; then
    HAS_CHANGES="true"
fi

# =============================================================================
# WRITE LOG ENTRY
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/session-$LOG_DATE.jsonl"

if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg event "stop" \
        --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg stop_reason "$STOP_REASON" \
        --argjson pcv_status "$PCV_STATUS_JSON" \
        --argjson task_context "$TASK_CONTEXT_JSON" \
        --arg decision "$DECISION" \
        --arg git_branch "$GIT_BRANCH" \
        --argjson has_uncommitted_changes "$HAS_CHANGES" \
        '{
            event: $event,
            timestamp: $timestamp,
            session_id: $session_id,
            stop_reason: $stop_reason,
            pcv_status: $pcv_status,
            task_context: $task_context,
            decision: $decision,
            git_branch: $git_branch,
            has_uncommitted_changes: $has_uncommitted_changes
        }')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
