#!/usr/bin/env bash
# Purpose:   PreToolUse hook for team guard enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh
# Hook Type: PreToolUse
# Matcher:   Teammate
#
# This hook:
#   - Blocks Teammate tool cleanup operation while PathFlow is active
#   - Only applies to Teammate tool with operation="cleanup"
#   - If not Teammate tool or not cleanup operation, allows through
#   - If cleanup but PathFlow not active, allows cleanup
#   - If cleanup AND PathFlow active, blocks with exit 2
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (not Teammate, not cleanup, or PathFlow not active)
#   - 2: Operation blocked (cleanup while PathFlow active)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="1.0.0"


# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

# Read hook data from stdin (Claude Code protocol) or env vars (test fallback)
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null)
        [[ -n "$_tn" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null)
        [[ -n "$_ti" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# EARLY EXIT: CHECK TOOL NAME
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Teammate" ]]; then
    exit 0
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
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

# Only gate cleanup operations
if [[ "$OPERATION" != "cleanup" ]]; then
    exit 0
fi

# =============================================================================
# PATHFLOW FLAG CHECK
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

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
# BLOCK: CLEANUP WHILE PATHFLOW ACTIVE
# =============================================================================

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "team_guard_cleanup" "Teammate" "cleanup" "PathFlow active - team cleanup blocked"
fi

cat >&2 <<EOF
BLOCKED: Team cleanup not allowed during active PathFlow
Reason: PathFlow is active - team resources are still in use
Operation: cleanup

Complete the PathFlow workflow before cleaning up team resources.
EOF
exit 2
