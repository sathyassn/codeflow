#!/usr/bin/env bash
# Purpose:   PreToolUse hook for team guard enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh
# Hook Type: PreToolUse
# Matcher:   Teammate
#
# This hook:
#   - Blocks Teammate tool cleanup operation while PathFlow is active
#   - Blocks TeamDelete tool while PathFlow is active
#   - PF7-END gate: When TeamDelete requested AND pf-6 sentinel exists,
#     allows TeamDelete (exit 0). Flag removal deferred to PostToolUse sentinel hook.
#   - If not Teammate/TeamDelete tool, allows through
#   - If Teammate but not cleanup operation, allows through
#   - If cleanup/TeamDelete but PathFlow not active, allows cleanup
#   - If (cleanup OR TeamDelete) AND PathFlow active, blocks with exit 2
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (not Teammate/TeamDelete, not cleanup, PathFlow
#         not active, or PF7-END gate passed)
#   - 2: Operation blocked (cleanup/TeamDelete while PathFlow active)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="1.0.0"


# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

# Read hook data from stdin (Claude Code protocol) or env vars (test fallback)
_stdin_sid=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null)
        [[ -n "$_tn" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null)
        [[ -n "$_ti" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && _stdin_sid="$_sid"
    fi
fi

# =============================================================================
# EARLY EXIT: CHECK TOOL NAME
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"

# TeamDelete is always blocked during PathFlow (no operation check needed)
IS_TEAM_DELETE=false
if [[ "$TOOL_NAME" == "TeamDelete" ]]; then
    IS_TEAM_DELETE=true
fi

# For non-Teammate and non-TeamDelete tools, allow through
if [[ "$TOOL_NAME" != "Teammate" ]] && [[ "$IS_TEAM_DELETE" != "true" ]]; then
    exit 0
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"

# TeamDelete skips operation check - always blocked during PathFlow
if [[ "$IS_TEAM_DELETE" != "true" ]]; then
    if [[ -z "$TOOL_INPUT" ]]; then
        exit 0
    fi

    # Extract operation from tool input
    OPERATION=""
    if command -v jq &>/dev/null; then
        OPERATION=$(echo "$TOOL_INPUT" | jq -r '.operation // empty')
    else
        OPERATION=$(echo "$TOOL_INPUT" | grep -oE '"operation"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"operation"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
    fi

    # Only gate cleanup operations for Teammate tool
    if [[ "$OPERATION" != "cleanup" ]]; then
        exit 0
    fi
fi
# =============================================================================
# PATHFLOW FLAG CHECK
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

# Session ID priority: env file > stdin JSON > fallback
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-${_stdin_sid:-unknown}}"
export CODEFLOW_SESSION_ID

# Source security library (provides is_pathflow_active via context-lib.sh)
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Allow overriding for testing via PATHFLOW_FLAG_FILE env var
if [[ -n "${PATHFLOW_FLAG_FILE:-}" ]]; then
    if [[ ! -f "$PATHFLOW_FLAG_FILE" ]]; then
        exit 0
    fi
elif ! is_pathflow_active 2>/dev/null; then
    # Not in PathFlow mode - allow cleanup
    exit 0
fi

# =============================================================================
# PF7-END GATE: Allow TeamDelete when PF6-COMPLETE is done
# =============================================================================
# If pf-6 sentinel exists, we're legitimately at PF7-END.
# Gate only — no cleanup side effects.
# PostToolUse on TeamDelete handles flag removal; SessionEnd handles remaining cleanup.

if [[ "$IS_TEAM_DELETE" == "true" ]]; then
    _sentinel_dir="$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID"
    if [[ -f "$_sentinel_dir/pathflow-pf-6" ]]; then
        # PF7-END: pf-6 found, allow TeamDelete through
        # Flag removal handled by PostToolUse on TeamDelete (not here)
        if declare -f log_security_event &>/dev/null; then
            log_security_event "allowed" "team_guard_pf7_gate" "$TOOL_NAME" "TeamDelete" "PF7-END: pf-6 sentinel found, TeamDelete allowed (flag removal deferred to PostToolUse)"
        fi
        exit 0
    fi
fi

# =============================================================================
# BLOCK: CLEANUP/DELETION WHILE PATHFLOW ACTIVE
# =============================================================================

# Determine what we're blocking for the message
if [[ "$IS_TEAM_DELETE" == "true" ]]; then
    BLOCKED_OP="TeamDelete"
else
    BLOCKED_OP="cleanup"
fi

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "team_guard_${BLOCKED_OP}" "$TOOL_NAME" "$BLOCKED_OP" "PathFlow active - team $BLOCKED_OP blocked"
fi

cat >&2 <<EOF
BLOCKED: Team cleanup/deletion not allowed during active PathFlow
Reason: PathFlow is active - team resources are still in use
Operation: $BLOCKED_OP

Complete the PathFlow workflow (PF7-END) before cleaning up team resources.

Do NOT bypass by directly modifying team config files, removing the pathflow-active flag, or killing tmux panes manually.
Follow the proper PF7-END shutdown sequence.
EOF
exit 2
