#!/usr/bin/env bash
# Purpose:   PreToolUse hook for PathFlow gate enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write|Bash
#
# This hook:
#   - Blocks Edit/Write/Bash(git commit) when PathFlow is active but PF-3 not complete
#   - Only checks when pathflow-active flag file exists (PathFlow mode)
#   - If flag doesn't exist, allows everything (standalone mode)
#   - Checks for PF-3 sentinel at REPO_ROOT/.state/sentinels/pathflow-pf-3-*
#   - For Bash: only gates "git commit" commands
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (not in PathFlow, PF-3 complete, or non-gated tool)
#   - 2: Operation blocked (PathFlow active, PF-3 not complete)

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
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]] && [[ "$TOOL_NAME" != "Bash" ]]; then
    exit 0
fi

# =============================================================================
# SETUP (before flag check - REPO_ROOT needed for session-scoped paths)
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# PATHFLOW FLAG CHECK
# =============================================================================

# Source security library (provides is_pathflow_active via context-lib.sh)
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

# Allow overriding the flag path for testing via PATHFLOW_FLAG_FILE env var
if [[ -n "${PATHFLOW_FLAG_FILE:-}" ]]; then
    # Test mode: use explicit flag file path
    if [[ ! -f "$PATHFLOW_FLAG_FILE" ]]; then
        exit 0
    fi
else
    # Production mode: use is_pathflow_active() from context-lib.sh
    if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
        # shellcheck source=/dev/null
        source "$LIB_DIR/security-lib.sh"
    fi
    if ! is_pathflow_active 2>/dev/null; then
        # Not in PathFlow mode - allow everything
        exit 0
    fi
fi

# Source security library for logging (if not already sourced above)
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# BASH TOOL: ONLY GATE GIT COMMIT
# =============================================================================

if [[ "$TOOL_NAME" == "Bash" ]]; then
    TOOL_INPUT="${TOOL_INPUT:-}"
    if [[ -z "$TOOL_INPUT" ]]; then
        exit 0
    fi

    COMMAND=""
    if command -v jq &>/dev/null; then
        COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
    else
        COMMAND=$(echo "$TOOL_INPUT" | grep -oE '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
    fi

    if [[ -z "$COMMAND" ]]; then
        exit 0
    fi

    # Only gate git commit commands
    if ! echo "$COMMAND" | grep -qE '(^|\s|&&|\|)git\s+commit(\s|$)'; then
        exit 0
    fi
fi

# =============================================================================
# PF-3 SENTINEL CHECK
# =============================================================================

# Allow overriding sentinel dir for testing
SENTINEL_DIR="${PATHFLOW_SENTINEL_DIR:-$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID}"

# Check for PF-3 sentinel (work classification complete)
pf3_found=false
for file in "$SENTINEL_DIR"/pathflow-pf-3-*; do
    if [[ -f "$file" ]]; then
        pf3_found=true
        break
    fi
done

if [[ "$pf3_found" == "true" ]]; then
    # PF-3 complete - allow operation
    exit 0
fi

# =============================================================================
# BLOCK: PATHFLOW ACTIVE BUT PF-3 NOT COMPLETE
# =============================================================================

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "pathflow_gate_pf3" "$TOOL_NAME" "${COMMAND:-$TOOL_NAME}" "PathFlow active but PF-3 not complete"
fi

cat >&2 <<EOF
BLOCKED: PathFlow gate - work classification required
Reason: PathFlow is active but PF-3 (work classification) has not been completed
Tool: $TOOL_NAME

Complete PF-3 before making changes or committing.
EOF
exit 2
