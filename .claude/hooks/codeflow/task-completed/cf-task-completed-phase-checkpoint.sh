#!/usr/bin/env bash
# Purpose:   TaskCompleted hook for phase checkpoint completion (Layer 2)
# Hook Type: TaskCompleted (lifecycle event, fires OUTSIDE agentic loop)
# Usage:     Called by Claude Code TaskCompleted lifecycle hook system
# Platform:  macOS/Linux
#
# This hook:
#   - Fires when Claude Code marks a task as completed (CLI lifecycle event)
#   - Parses task_subject for PF{N}-TSK-{NN} pattern
#   - Marks matching tasks completed in session-scoped checkpoint file
#   - Creates phase sentinel when ALL required tasks are completed or skipped
#   - Supports exit 2 to block completion on dependency constraint violations
#   - Ignores non-PathFlow task completions silently
#
# Checkpoint file:
#   .state/session/{SID}/pathflow/pathflow-phase-tasks.json
#
# TaskCompleted stdin JSON fields:
#   task_id:          Internal Claude Code task tracker ID
#   task_subject:     Task subject text (contains PF{N}-TSK-{NN} pattern)
#   task_description: Full task description
#   session_id:       Current session ID
#   cwd:              Working directory
#   transcript_path:  Path to conversation transcript
#
# Exit codes:
#   0 - Task completion allowed
#   2 - Task completion BLOCKED (dependency constraint violated)
#
# Compatibility: bash 3.2+ (macOS compatible)

set -euo pipefail

# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

_HOOK_STDIN=""
_TASK_SUBJECT=""
_TASK_ID=""

if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _TASK_SUBJECT=$(echo "$_HOOK_STDIN" | jq -r '.task_subject // empty' 2>/dev/null) || true
        _TASK_ID=$(echo "$_HOOK_STDIN" | jq -r '.task_id // empty' 2>/dev/null) || true
    fi
fi

# =============================================================================
# MATCH PF{N}-TSK-{NN} PATTERN IN SUBJECT — early exit if no match
# =============================================================================

if [[ -z "$_TASK_SUBJECT" ]]; then
    exit 0
fi

if ! [[ "$_TASK_SUBJECT" =~ (PF[0-9]+-TSK-[0-9]+) ]]; then
    # Not a PathFlow task — exit silently
    exit 0
fi

_pf_task_id="${BASH_REMATCH[1]}"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# SESSION ID: Source from env file (shared across all teammates)
# =============================================================================

_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
else
    # Fallback: read from hook input
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null) || true
        [[ -n "${_sid:-}" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

echo "TaskCompleted[checkpoint]: hook start, pf_task=$_pf_task_id, session=$CODEFLOW_SESSION_ID" >&2

# =============================================================================
# PATHFLOW CHECK — early exit if not active
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
else
    echo "TaskCompleted[checkpoint]: security-lib.sh not found, skipping" >&2
    exit 0
fi

if ! is_pathflow_active 2>/dev/null; then
    echo "TaskCompleted[checkpoint]: pathflow not active, skipping" >&2
    exit 0
fi

# =============================================================================
# JQ REQUIRED
# =============================================================================

if ! command -v jq &>/dev/null; then
    echo "TaskCompleted[checkpoint]: jq not available, skipping" >&2
    exit 0
fi

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    echo "TaskCompleted[checkpoint]: pathflow-state lib not found at $_PFS_LIB" >&2
    exit 0
fi
# shellcheck source=/dev/null
if ! source "$_PFS_LIB"; then
    echo "TaskCompleted[checkpoint]: failed to source pathflow-state lib" >&2
    exit 0
fi

# =============================================================================
# DEPENDENCY CHECK — block if prerequisites not met
# =============================================================================

# Extract phase from task_id (PF1-TSK-01 -> PF1)
_phase_id=""
if [[ "$_pf_task_id" =~ ^(PF[0-9]+)-TSK-[0-9]+$ ]]; then
    _phase_id="${BASH_REMATCH[1]}"
fi

if [[ -z "$_phase_id" ]]; then
    echo "TaskCompleted[checkpoint]: cannot extract phase from $_pf_task_id" >&2
    exit 0
fi

# Check if the task has blockedBy dependencies in pathflow-config.json
_config_file="$REPO_ROOT/.codeflow/config/pathflow/pathflow-config.json"
if [[ -f "$_config_file" ]]; then
    # Map phase_id to config key
    _config_key=$(jq -r --arg pfx "$_phase_id" \
        '.phases | keys[] | select(startswith($pfx + "-"))' \
        "$_config_file" 2>/dev/null) || true

    if [[ -n "${_config_key:-}" ]]; then
        # Get this task's task_order
        _task_order=$(jq -r --arg key "$_config_key" --arg tid "$_pf_task_id" \
            '.phases[$key].tasks[] | select(.id == $tid) | .task_order' \
            "$_config_file" 2>/dev/null) || true

        if [[ -n "${_task_order:-}" ]] && [[ "$_task_order" -gt 1 ]]; then
            # Check previous tasks (lower task_order) that have blockedBy relationship
            # Read checkpoint to verify predecessors are complete
            _checkpoint=$(checkpoint_read)

            # Get all task IDs with lower task_order in this phase
            _prev_tasks=$(jq -r --arg key "$_config_key" --argjson order "$_task_order" \
                '[.phases[$key].tasks[] | select(.task_order < $order) | .id] | .[]' \
                "$_config_file" 2>/dev/null) || true

            # Check each predecessor — only enforce if this task explicitly depends on them
            # For now we do NOT enforce strict sequential ordering (see design doc: "Why strict sequential completion is NOT enforced")
            # We only check cross-phase dependencies: previous phase sentinel must exist
        fi
    fi
fi

# Cross-phase dependency: check that previous phase sentinel exists
# PF2 requires pf-1, PF3 requires pf-2, etc.
_phase_num=""
if [[ "$_phase_id" =~ ^PF([0-9]+)$ ]]; then
    _phase_num="${BASH_REMATCH[1]}"
fi

if [[ -n "${_phase_num:-}" ]] && [[ "$_phase_num" -gt 1 ]]; then
    _prev_phase_num=$((_phase_num - 1))
    _prev_sentinel="pf-${_prev_phase_num}"

    if ! has_sentinel "$_prev_sentinel" 2>/dev/null; then
        echo "TaskCompleted[checkpoint]: BLOCKED — completing $_pf_task_id requires sentinel $_prev_sentinel (PF${_prev_phase_num} must complete first)" >&2
        exit 2
    fi
fi

# =============================================================================
# MARK TASK COMPLETED IN CHECKPOINT
# =============================================================================

_result=""
_result=$(checkpoint_complete_task "$_pf_task_id") || {
    echo "TaskCompleted[checkpoint]: FAILED to complete $_pf_task_id in checkpoint" >&2
    exit 0
}

echo "TaskCompleted[checkpoint]: marked $_pf_task_id completed" >&2

# =============================================================================
# CREATE PHASE SENTINEL IF PHASE IS COMPLETE
# =============================================================================

if [[ "$_result" == "phase_complete" ]]; then
    # Map phase number to sentinel name
    _sentinel_name="pf-${_phase_num}"

    if create_sentinel "$_sentinel_name"; then
        echo "TaskCompleted[checkpoint]: ALL tasks in $_phase_id done — created sentinel $_sentinel_name" >&2

        # Mark sentinel_created in checkpoint
        _checkpoint=$(checkpoint_read)
        _updated=$(echo "$_checkpoint" | jq -c --arg pf "$_phase_id" \
            '.[$pf].sentinel_created = true' 2>/dev/null) || true
        if [[ -n "${_updated:-}" ]]; then
            checkpoint_write "$_updated" || true
        fi
    else
        echo "TaskCompleted[checkpoint]: FAILED to create sentinel $_sentinel_name" >&2
    fi
fi

exit 0
