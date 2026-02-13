#!/usr/bin/env bash
# Purpose:   SessionEnd hook for session cleanup
# Location:  .claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh
# Hook Type: SessionEnd
# Usage:     Called by Claude Code at session end
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Removes all PathFlow sentinels (session-scoped)
#   - Cleans up EXPIRED skill sentinels (preserves valid ones)
#   - Removes is-pathflow-active flag file
#   - Cleans up memory-progress state files
#   - Preserves active task context if in_progress
#   - Removes session-specific temp files
#   - Logs session end event (if configured)
#
# Cleanup order (per V4 spec):
#   1. Remove all PathFlow sentinels
#   2. Remove expired skill sentinels
#   3. Remove is-pathflow-active flag
#   4. Clean memory progress files
#   5. Preserve active task context if in_progress
#   6. Log session end event (optional)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Cleanup completed successfully (always exits 0)

set -euo pipefail

# Read hook data from stdin (Claude Code protocol)
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
TRANSCRIPT_PATH=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && SESSION_ID="$_sid"
        _tp=$(echo "$_HOOK_STDIN" | jq -r '.transcript_path // empty' 2>/dev/null)
        # shellcheck disable=SC2034  # TRANSCRIPT_PATH available for future use
        [[ -n "$_tp" ]] && TRANSCRIPT_PATH="$_tp"
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

# Read configuration
SENTINEL_DIR="$REPO_ROOT/.state/sentinels/skill/$SESSION_ID"
SESSION_STATE_DIR="$REPO_ROOT/.state/session/$SESSION_ID"
# shellcheck disable=SC2034  # SHARED_STATE_DIR used by child hooks/scripts
SHARED_STATE_DIR="$REPO_ROOT/.state/session"

CLEANUP_ENABLED="true"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    _sd=$(jq -r '.sentinel.directory // ".state/sentinels/skill"' "$CONFIG" 2>/dev/null || echo ".state/sentinels/skill")
    if [[ "$_sd" != /* ]]; then _sd="$REPO_ROOT/$_sd"; fi
    SENTINEL_DIR="${_sd}/$SESSION_ID"
    # Check if session_end cleanup is enabled in config
    _enabled=$(jq -r '.session_end.cleanup.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    [[ "$_enabled" == "false" ]] && CLEANUP_ENABLED="false"
fi

# If cleanup is disabled, exit early
if [[ "$CLEANUP_ENABLED" == "false" ]]; then
    echo "SessionEnd: Cleanup disabled by configuration"
    exit 0
fi

# Set CODEFLOW_SESSION_ID for security-lib
CODEFLOW_SESSION_ID="$SESSION_ID"
export CODEFLOW_SESSION_ID

# Source security-lib for is_pathflow_active()
SECURITY_LIB="$REPO_ROOT/.codeflow/scripts/security/lib/security-lib.sh"
_PATHFLOW_ACTIVE="false"
if [[ -f "$SECURITY_LIB" ]]; then
    # shellcheck source=../../../../.codeflow/scripts/security/lib/security-lib.sh
    source "$SECURITY_LIB" 2>/dev/null || true
    if type is_pathflow_active &>/dev/null && is_pathflow_active; then
        _PATHFLOW_ACTIVE="true"
    fi
fi

# Track cleanup stats for output
_sentinels_cleaned=0
_task_preserved="false"

# =============================================================================
# 1. PATHFLOW SENTINEL CLEANUP
# =============================================================================

# Remove ALL PathFlow sentinels for this session
PATHFLOW_SENTINEL_DIR="$REPO_ROOT/.state/sentinels/pathflow/$SESSION_ID"
if [[ -d "$PATHFLOW_SENTINEL_DIR" ]]; then
    rm -rf "$PATHFLOW_SENTINEL_DIR" 2>/dev/null || true
    _sentinels_cleaned=$((_sentinels_cleaned + 1))
fi

# =============================================================================
# 2. EXPIRED SKILL SENTINEL CLEANUP
# =============================================================================

# Remove ALL skill sentinels for this session (session is ending)
if [[ -d "$SENTINEL_DIR" ]]; then
    _count=$(find "$SENTINEL_DIR" -name "*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
    rm -rf "$SENTINEL_DIR" 2>/dev/null || true
    _sentinels_cleaned=$((_sentinels_cleaned + _count))
fi

# =============================================================================
# 3. PATHFLOW FLAG REMOVAL
# =============================================================================

# PathFlow flag is in session state dir - cleaned with session dir below

# =============================================================================
# 4. MEMORY PROGRESS CLEANUP
# =============================================================================

# Memory progress is in session state dir - cleaned with session dir below

# =============================================================================
# 5. ACTIVE TASK PRESERVATION
# =============================================================================

# Preserve active task context if status is in_progress
if [[ -f "$(get_active_task_file)" ]]; then
    _task_id=$(get_active_task_id)
    if is_task_active; then
        # Keep the file - preserve for next session
        _task_preserved="true"
    else
        # Task is not in_progress, safe to clean up
        clear_active_task
    fi
fi

# =============================================================================
# 6. SESSION STATE & TEMP CLEANUP
# =============================================================================

# Clean up session state directory
if [[ -d "$SESSION_STATE_DIR" ]] && [[ "$SESSION_ID" != "unknown" ]]; then
    rm -rf "$SESSION_STATE_DIR" 2>/dev/null || true
fi

# Clean up session-specific temp files
TEMP_DIR="/tmp/claude/sessions/$SESSION_ID"
if [[ -d "$TEMP_DIR" ]]; then
    rm -rf "$TEMP_DIR" 2>/dev/null || true
fi

# =============================================================================
# OUTPUT
# =============================================================================

if [[ "$_sentinels_cleaned" -gt 0 ]]; then
    echo "SessionEnd: Cleaned $_sentinels_cleaned sentinel(s)"
else
    echo "SessionEnd: No expired sentinels to clean"
fi

if [[ "$_task_preserved" == "true" ]] && [[ -n "${_task_id:-}" ]]; then
    echo "SessionEnd: Task $_task_id preserved for next session"
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
