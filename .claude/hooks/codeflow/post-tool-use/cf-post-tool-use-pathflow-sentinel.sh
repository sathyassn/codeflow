#!/usr/bin/env bash
# Purpose:   PostToolUse hook for automatic PathFlow stage sentinel creation
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh
# Hook Type: PostToolUse
# Matcher:   SendMessage (configured in settings.json)
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
#
# This hook:
#   - Detects stage-completion events from SendMessage tool calls
#   - Creates stage sentinel files to track PathFlow work stage progression
#   - Enables pathflow-gate enforcement without requiring JSONL writes
#
# Note: Phase sentinels (pf-1, pf-2, pf-3, pf-6) are created by the
#       phase checkpoint hooks (session-start, post-tool-use checkpoint hooks).
#
# Sentinel detection:
#   tool_name=SendMessage, STAGE-COMPLETE: WS-* -> pathflow-ws-{stage}
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
    fi
fi

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# SESSION ID: Source from env file (shared across all teammates)
# =============================================================================
# The env file contains the real session ID written at session start.
# Stdin .session_id is a per-agent UUID from Claude Code, NOT the shared session.
# Using stdin would create sentinels under the wrong directory.
# TODO(go-cli): Session ID sourcing unchanged when CLI arrives

_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
else
    # Fallback: read from hook input (degraded mode — per-agent UUID)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

echo "PostToolUse[sentinel]: hook start, tool=$TOOL_NAME, session=$CODEFLOW_SESSION_ID" >&2

# =============================================================================
# PATHFLOW CHECK — early exit if not active
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
else
    echo "PostToolUse[sentinel]: security-lib.sh not found, skipping" >&2
    exit 0
fi

if ! is_pathflow_active 2>/dev/null; then
    echo "PostToolUse[sentinel]: pathflow not active, skipping" >&2
    exit 0
fi

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    echo "PostToolUse[sentinel]: pathflow-state lib not found at $_PFS_LIB, skipping" >&2
    exit 0
fi
# shellcheck source=/dev/null
if ! source "$_PFS_LIB"; then
    echo "PostToolUse[sentinel]: failed to source pathflow-state lib" >&2
    exit 0
fi

# =============================================================================
# SENTINEL DETECTION — Stage completion via SendMessage
# =============================================================================

if [[ "$TOOL_NAME" == "SendMessage" ]]; then
    if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
        _content=$(echo "$TOOL_INPUT" | jq -r '.content // empty' 2>/dev/null) || true
        if [[ -n "${_content:-}" ]]; then
            # Match STAGE-COMPLETE: WS-{STAGE} pattern
            if [[ "$_content" =~ STAGE-COMPLETE:\ WS-(DEV|REV|QA|TEST|PLAN|DOCS) ]]; then
                _stage="${BASH_REMATCH[1]}"
                # Convert to lowercase for sentinel name
                _stage_lower=$(echo "$_stage" | tr '[:upper:]' '[:lower:]')
                if create_sentinel "ws-${_stage_lower}"; then
                    echo "PostToolUse[sentinel]: created sentinel ws-${_stage_lower}" >&2
                else
                    echo "PostToolUse[sentinel]: FAILED to create sentinel ws-${_stage_lower}" >&2
                fi
            fi
        fi
    fi
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
