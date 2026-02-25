#!/usr/bin/env bash
# Purpose:   SessionEnd hook for session cleanup
# Location:  .claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh
# Hook Type: SessionEnd
# Usage:     Called by Claude Code at session end
# Platform:  macOS/Linux
# Version:   2.4.0
#
# This hook:
#   - Guards against premature cleanup using pathflow-active flag
#     (if flag exists AND lead PID is alive, this is a teammate shutdown — skip cleanup)
#   - Removes all PathFlow sentinels (session-scoped)
#   - Cleans up EXPIRED skill sentinels (preserves valid ones)
#   - Cleans up memory-progress state files
#   - Preserves active task context if in_progress
#   - Reads pathflow-team.json to clean up team config/task list
#   - Removes env file (session ID shared state)
#   - Removes session-specific temp files
#   - Logs session end event (if configured)
#
# Cleanup order (per V4 spec):
#   1. Remove all PathFlow sentinels
#   2. Remove expired skill sentinels
#   3. Flag removed by PostToolUse on TeamDelete; SessionEnd removes implicitly via session dir cleanup (backstop)
#   4. Clean memory progress files
#   5. Preserve active task context if in_progress
#   6. Read pathflow-team.json, clean team config/task list, remove session dir
#   7. Log session end event (optional)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Cleanup completed successfully (always exits 0)

set -euo pipefail

# Read hook data from stdin (Claude Code protocol)
TRANSCRIPT_PATH=""
_stdin_sid=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _stdin_sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null || true)
        _tp=$(echo "$_HOOK_STDIN" | jq -r '.transcript_path // empty' 2>/dev/null)
        # shellcheck disable=SC2034  # TRANSCRIPT_PATH available for future use
        [[ -n "$_tp" ]] && TRANSCRIPT_PATH="$_tp"
    fi
fi

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.4.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# TODO(go-cli): Session ID sourcing unchanged when CLI arrives
# The env file path and variable name remain stable
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi

# Session ID priority: env file > stdin JSON > fallback
SESSION_ID="${CODEFLOW_SESSION_ID:-${_stdin_sid:-unknown}}"

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

# =============================================================================
# PATHFLOW GUARD
# =============================================================================
# If pathflow-active flag exists, check if the lead is still alive.
# Three-way check:
#   1. $PPID == lead_pid → This IS the lead's own SessionEnd. Proceed with cleanup.
#   2. kill -0 lead_pid succeeds → Lead is alive, this is a teammate shutdown. Skip.
#   3. kill -0 lead_pid fails → Lead is dead, orphaned session. Proceed with cleanup.
# The flag is removed by team-guard hook during PF7-END (before TeamDelete),
# so when the lead's session actually ends, _PATHFLOW_ACTIVE is already false.

if [[ "$_PATHFLOW_ACTIVE" == "true" ]]; then
    _pf_team_file_guard="$SESSION_STATE_DIR/pathflow/pathflow-team.json"
    _skip_cleanup="true"

    if [[ -f "$_pf_team_file_guard" ]] && command -v jq &>/dev/null; then
        _guard_lead_pid=$(jq -r '.lead_pid // 0' "$_pf_team_file_guard" 2>/dev/null) || _guard_lead_pid=0

        if [[ "$_guard_lead_pid" -gt 0 ]]; then
            if [[ "$_guard_lead_pid" == "${PPID:-0}" ]]; then
                # This IS the lead's own SessionEnd (PPID matches lead_pid).
                # In-process teammates share the lead's PID, so kill -0 would
                # always succeed for the lead itself. Use PPID to distinguish.
                echo "SessionEnd: PathFlow active, PPID matches lead PID $_guard_lead_pid — proceeding with cleanup (lead's own SessionEnd)" >&2
                _skip_cleanup="false"
            elif kill -0 "$_guard_lead_pid" 2>/dev/null; then
                # Lead is alive and this is NOT the lead — teammate shutdown
                echo "SessionEnd: PathFlow active, lead PID $_guard_lead_pid alive — skipping cleanup (teammate shutdown)" >&2
                exit 0
            else
                # Lead is dead — orphaned session, proceed with cleanup
                echo "SessionEnd: PathFlow active but lead PID $_guard_lead_pid dead — proceeding with cleanup (orphaned session)" >&2
                _skip_cleanup="false"
            fi
        else
            # lead_pid is 0 or missing — cannot verify, proceed with cleanup
            echo "SessionEnd: PathFlow active but lead PID unknown — proceeding with cleanup" >&2
            _skip_cleanup="false"
        fi
    else
        # No team file — cannot verify lead, proceed with cleanup
        echo "SessionEnd: PathFlow active but no team file — proceeding with cleanup (no lead to protect)" >&2
        _skip_cleanup="false"
    fi

    if [[ "$_skip_cleanup" == "true" ]]; then
        echo "SessionEnd: PathFlow active, skipping cleanup (teammate shutdown)" >&2
        exit 0
    fi
fi

# Track cleanup stats for output
_sentinels_cleaned=0
_task_preserved="false"

# =============================================================================
# PF7 DIAGNOSTIC — Check for clean shutdown signal
# =============================================================================

_pf_sentinel_dir="$REPO_ROOT/.state/sentinels/pathflow/$SESSION_ID"
if [[ -f "$_pf_sentinel_dir/pathflow-pf-7" ]]; then
    echo "SessionEnd: Clean PF7 shutdown (all phases completed)" >&2
else
    echo "SessionEnd: Incomplete PF7 shutdown (pf-7 sentinel absent — possible crash or skip)" >&2
fi

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

# Read pathflow-team.json before removing session directory (need team_name for cleanup)
_pf_team_name=""
_pf_team_file="$SESSION_STATE_DIR/pathflow/pathflow-team.json"
if [[ -f "$_pf_team_file" ]] && command -v jq &>/dev/null; then
    _pf_team_name=$(jq -r '.team_name // empty' "$_pf_team_file" 2>/dev/null) || true
fi

# Backstop: Clean team config/task list if PostToolUse on TeamDelete missed it
# Normal path: TeamDelete removes these; this catches crash/skip scenarios
# Clean up team config and task list if team_name is known
if [[ -n "$_pf_team_name" ]]; then
    if [[ -d "${HOME}/.claude/teams/${_pf_team_name}" ]]; then
        rm -rf "${HOME}/.claude/teams/${_pf_team_name}" 2>/dev/null || true
        echo "SessionEnd: Removed team config: ${_pf_team_name}" >&2
    fi
    if [[ -d "${HOME}/.claude/tasks/${_pf_team_name}" ]]; then
        rm -rf "${HOME}/.claude/tasks/${_pf_team_name}" 2>/dev/null || true
        echo "SessionEnd: Removed task list: ${_pf_team_name}" >&2
    fi
fi

# Clean up session state directory
if [[ -d "$SESSION_STATE_DIR" ]] && [[ "$SESSION_ID" != "unknown" ]]; then
    rm -rf "$SESSION_STATE_DIR" 2>/dev/null || true
fi

# Clean up env file (session ID shared state)
rm -f "${REPO_ROOT}/.state/runtime/codeflow-env.sh" 2>/dev/null || true

# Clean up project temp directory (staging area, test artifacts, etc.)
PROJECT_TEMP_DIR="/tmp/claude/${CF_PROJECT_ROOT:-codeflow}"
if [[ -d "$PROJECT_TEMP_DIR" ]]; then
    rm -rf "$PROJECT_TEMP_DIR" 2>/dev/null || true
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
