#!/usr/bin/env bash
# Purpose:   PreToolUse hook for PathFlow gate enforcement (JSONL-backed)
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write|Bash
#
# This hook:
#   - Blocks Edit/Write when PathFlow is active but phase < PF4-EXECUTE
#   - Blocks Bash(git commit) when phase < PF4-EXECUTE
#   - Blocks Bash(git push/gh pr) when phase < PF6-COMPLETE
#   - Reads phase from JSONL ledger (interim) or SQLite (future)
#   - Graceful degradation: ALLOW on any enforcement system failure
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed
#   - 2: Operation blocked (phase requirement not met)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="2.0.0"


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
# SETUP
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
# BASH TOOL: CLASSIFY GATED OPERATION
# =============================================================================

# Determine what type of gated operation this is:
#   edit_write  - Edit/Write tool (requires PF4)
#   git_commit  - Bash with git commit (requires PF4)
#   git_push_pr - Bash with git push or gh pr (requires PF6)
#   ungated     - Bash with other commands (always allowed)

GATE_TYPE="edit_write"
COMMAND=""

if [[ "$TOOL_NAME" == "Bash" ]]; then
    TOOL_INPUT="${TOOL_INPUT:-}"
    if [[ -z "$TOOL_INPUT" ]]; then
        exit 0
    fi

    if command -v jq &>/dev/null; then
        COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
    else
        COMMAND=$(echo "$TOOL_INPUT" | grep -oE '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
    fi

    if [[ -z "$COMMAND" ]]; then
        exit 0
    fi

    # Classify the bash command
    if echo "$COMMAND" | grep -qE '(^|\s|&&|\|)(git\s+push|gh\s+pr)(\s|$)'; then
        GATE_TYPE="git_push_pr"
    elif echo "$COMMAND" | grep -qE '(^|\s|&&|\|)git\s+commit(\s|$)'; then
        GATE_TYPE="git_commit"
    else
        # Non-gated bash command
        exit 0
    fi
fi

# =============================================================================
# PHASE LOOKUP (JSONL-backed)
# =============================================================================

# Map phase name to numeric level for comparison
phase_to_num() {
    case "$1" in
        PF1*) echo 1 ;;
        PF2*) echo 2 ;;
        PF3*) echo 3 ;;
        PF4*) echo 4 ;;
        PF5*) echo 5 ;;
        PF6*) echo 6 ;;
        PF7*) echo 7 ;;
        *) echo 0 ;;
    esac
}

# Read session ID for JSONL filtering
SESSION_ID_FILE="${PATHFLOW_SESSION_ID_FILE:-$REPO_ROOT/.state/runtime/current-session-id}"
if [[ ! -f "$SESSION_ID_FILE" ]]; then
    # No tracked session - allow (graceful degradation)
    exit 0
fi

CURRENT_SESSION_ID=$(cat "$SESSION_ID_FILE" 2>/dev/null) || true
if [[ -z "$CURRENT_SESSION_ID" ]]; then
    # Empty session ID - allow (graceful degradation)
    exit 0
fi

# Read latest phase_transition event from JSONL
JSONL_FILE="${PATHFLOW_JSONL_FILE:-$REPO_ROOT/.state/ledger/pathflow-events.jsonl}"
if [[ ! -f "$JSONL_FILE" ]]; then
    # No JSONL file - allow (graceful degradation)
    exit 0
fi

if ! command -v jq &>/dev/null; then
    # No jq available - allow (graceful degradation)
    exit 0
fi

# Get the latest phase_transition event for this session
# Performance: tail -100 is sufficient (~100 events max per session)
CURRENT_PHASE=$(tail -100 "$JSONL_FILE" 2>/dev/null | \
    jq -r "select(.type==\"phase_transition\" and .session_id==\"$CURRENT_SESSION_ID\") | .phase" 2>/dev/null | \
    tail -1) || true

if [[ -z "$CURRENT_PHASE" ]]; then
    # No phase info found - allow (graceful degradation)
    exit 0
fi

PHASE_NUM=$(phase_to_num "$CURRENT_PHASE")

# =============================================================================
# PHASE GATE ENFORCEMENT
# =============================================================================

# Edit/Write and git commit require PF4-EXECUTE (phase_num >= 4)
# git push/gh pr require PF6-COMPLETE (phase_num >= 6)

REQUIRED_PHASE=""
REQUIRED_NUM=0

case "$GATE_TYPE" in
    edit_write|git_commit)
        REQUIRED_PHASE="PF4-EXECUTE"
        REQUIRED_NUM=4
        ;;
    git_push_pr)
        REQUIRED_PHASE="PF6-COMPLETE"
        REQUIRED_NUM=6
        ;;
esac

if [[ $PHASE_NUM -ge $REQUIRED_NUM ]]; then
    # Phase requirement met - allow
    exit 0
fi

# =============================================================================
# BLOCK: PHASE REQUIREMENT NOT MET
# =============================================================================

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "pathflow_gate_phase" "$TOOL_NAME" "${COMMAND:-$TOOL_NAME}" "Phase $CURRENT_PHASE < $REQUIRED_PHASE"
fi

cat >&2 <<EOF
BLOCKED: PathFlow gate - phase requirement not met
Reason: Current phase is $CURRENT_PHASE but $REQUIRED_PHASE is required
Tool: $TOOL_NAME
Gate: $GATE_TYPE

Complete earlier phases before this operation.
EOF
exit 2
