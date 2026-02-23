#!/usr/bin/env bash
# Purpose:   PostToolUse hook for phase checkpoint registration (Layer 1)
# Hook Type: PostToolUse
# Matcher:   TaskCreate (configured in settings.json)
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
#
# This hook:
#   - Fires after TaskCreate executes (inside agentic loop)
#   - Parses tool_input.subject for PF{N}-TSK-{NN} pattern
#   - Registers matching tasks in session-scoped checkpoint file
#   - Ignores non-PathFlow TaskCreate calls silently
#
# Checkpoint file:
#   .state/session/{SID}/pathflow/pathflow-phase-tasks.json
#
# Exit codes:
#   0 - Always succeeds (PostToolUse hooks should not block)
#
# Compatibility: bash 3.2+ (macOS compatible)

set -euo pipefail

# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
TOOL_INPUT="${TOOL_INPUT:-}"

if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null) || true
        [[ -n "${_tn:-}" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null) || true
        [[ -n "${_ti:-}" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
    fi
fi

# =============================================================================
# EARLY EXIT — only process TaskCreate
# =============================================================================

if [[ "$TOOL_NAME" != "TaskCreate" ]]; then
    exit 0
fi

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
    # Fallback: read from hook input (degraded mode — per-agent UUID)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null) || true
        [[ -n "${_sid:-}" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# PATHFLOW CHECK — early exit if not active
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    if ! source "$LIB_DIR/security-lib.sh" 2>/dev/null; then
        echo "PostToolUse[checkpoint]: failed to source security-lib.sh, skipping" >&2
        exit 0
    fi
else
    echo "PostToolUse[checkpoint]: security-lib.sh not found, skipping" >&2
    exit 0
fi

if ! is_pathflow_active 2>/dev/null; then
    exit 0
fi

# =============================================================================
# JQ REQUIRED
# =============================================================================

if ! command -v jq &>/dev/null; then
    echo "PostToolUse[checkpoint]: jq not available, skipping" >&2
    exit 0
fi

# =============================================================================
# EXTRACT TASK SUBJECT FROM TOOL INPUT
# =============================================================================

_subject=""
if [[ -n "$TOOL_INPUT" ]]; then
    _subject=$(echo "$TOOL_INPUT" | jq -r '.subject // empty' 2>/dev/null) || true
fi

if [[ -z "$_subject" ]]; then
    exit 0
fi

# =============================================================================
# MATCH PF{N}-TSK-{NN} PATTERN IN SUBJECT
# =============================================================================

if ! [[ "$_subject" =~ (PF[0-9]+-TSK-[0-9]+) ]]; then
    # Not a PathFlow task — exit silently
    exit 0
fi

_task_id="${BASH_REMATCH[1]}"

echo "PostToolUse[checkpoint]: detected PF task: $_task_id" >&2

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    echo "PostToolUse[checkpoint]: pathflow-state lib not found at $_PFS_LIB" >&2
    exit 0
fi
# shellcheck source=/dev/null
if ! source "$_PFS_LIB"; then
    echo "PostToolUse[checkpoint]: failed to source pathflow-state lib" >&2
    exit 0
fi

# =============================================================================
# REGISTER TASK IN CHECKPOINT
# =============================================================================

if checkpoint_register_task "$_task_id"; then
    echo "PostToolUse[checkpoint]: registered $_task_id in checkpoint" >&2
else
    echo "PostToolUse[checkpoint]: FAILED to register $_task_id" >&2
fi

exit 0
