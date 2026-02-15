#!/usr/bin/env bash
# Purpose:   PostToolUse hook for automatic PathFlow sentinel creation
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh
# Hook Type: PostToolUse
# Matcher:   TeamCreate|Task|SendMessage|Bash (configured in settings.json)
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
#
# This hook:
#   - Detects phase-transition events from tool calls
#   - Creates sentinel files to track PathFlow phase progression
#   - Enables pathflow-gate enforcement without requiring JSONL writes
#
# Sentinel detection:
#   tool_name=TeamCreate                        → pathflow-pf-1
#   tool_name=Task, name has cf-knowledge-layer → pathflow-pf-2
#   tool_name=Bash, git checkout -b/switch -c   → pathflow-pf-3
#   tool_name=SendMessage, STAGE-COMPLETE: WS-* → pathflow-ws-{stage}
#   tool_name=Bash, gh pr create                → pathflow-pf-6
#
# Idempotent: touch on existing file is a no-op.
# Early exit: If is_pathflow_active() returns false, exit 0.
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always succeeds (PostToolUse hooks should not block)

set -euo pipefail

# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
TOOL_INPUT="${TOOL_INPUT:-}"

if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null)
        [[ -n "$_tn" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null)
        [[ -n "$_ti" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "${_sid:-}" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi

CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# PATHFLOW CHECK — early exit if not active
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

if ! is_pathflow_active 2>/dev/null; then
    exit 0
fi

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    exit 0
fi
# shellcheck source=/dev/null
source "$_PFS_LIB"

# =============================================================================
# SENTINEL DETECTION
# =============================================================================

case "$TOOL_NAME" in

    TeamCreate)
        # PF1-INIT: Team infrastructure created
        create_sentinel "pf-1"
        ;;

    Task)
        # PF2-CONTEXT: cf-knowledge-layer teammate spawned
        if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
            _task_name=$(echo "$TOOL_INPUT" | jq -r '.name // empty' 2>/dev/null) || true
            if [[ "${_task_name:-}" == *"cf-knowledge-layer"* ]]; then
                create_sentinel "pf-2"
            fi
        fi
        ;;

    Bash)
        # PF3-CLASSIFY: git branch creation
        # PF6-COMPLETE: gh pr create
        if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
            _command=$(echo "$TOOL_INPUT" | jq -r '.command // empty' 2>/dev/null) || true
            if [[ -n "${_command:-}" ]]; then
                # Check for git branch creation
                if echo "$_command" | grep -qE 'git\s+(checkout\s+-b|switch\s+-c)\s'; then
                    create_sentinel "pf-3"
                fi
                # Check for PR creation
                if echo "$_command" | grep -qE 'gh\s+pr\s+create'; then
                    create_sentinel "pf-6"
                fi
            fi
        fi
        ;;

    SendMessage)
        # Stage completion: STAGE-COMPLETE: WS-{STAGE}
        if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
            _content=$(echo "$TOOL_INPUT" | jq -r '.content // empty' 2>/dev/null) || true
            if [[ -n "${_content:-}" ]]; then
                # Match STAGE-COMPLETE: WS-{STAGE} pattern
                if [[ "$_content" =~ STAGE-COMPLETE:\ WS-(DEV|REV|QA|TEST|PLAN|DOCS) ]]; then
                    _stage="${BASH_REMATCH[1]}"
                    # Convert to lowercase for sentinel name
                    _stage_lower=$(echo "$_stage" | tr '[:upper:]' '[:lower:]')
                    create_sentinel "ws-${_stage_lower}"
                fi
            fi
        fi
        ;;

esac

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
