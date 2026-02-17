#!/usr/bin/env bash
# Purpose:   PreToolUse hook for PathFlow gate enforcement (sentinel-backed)
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write|Bash|Task
#
# This hook:
#   - Blocks Edit/Write when PathFlow is active but PF3-CLASSIFY not reached
#   - Blocks Bash(git commit) when PF3-CLASSIFY not reached
#   - Blocks Bash(git push/gh pr) when WS-REV not completed
#   - Blocks Task(role teammate spawn) when PF3-CLASSIFY not reached
#   - Uses file sentinels (primary) for fast, reliable enforcement
#   - Graceful degradation: critical gates BLOCK, non-critical gates ALLOW
#     when enforcement state is unknown
#
# Sentinel check (primary enforcement):
#   Edit/Write, git commit → has_sentinel("pf-3")? Allow/Block
#   git push, gh pr        → has_sentinel("ws-rev")? Allow/Block
#   Task(role teammate)    → has_sentinel("pf-3")? Allow/Block
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed
#   - 2: Operation blocked (prerequisite sentinel not met)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="4.1.0"


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
    fi
fi

# =============================================================================
# EARLY EXIT: CHECK TOOL NAME
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]] && [[ "$TOOL_NAME" != "Bash" ]] && [[ "$TOOL_NAME" != "Task" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# SESSION ID: Source from env file (shared across all teammates)
# =============================================================================

# TODO(go-cli): Session ID sourcing unchanged when CLI arrives
# The env file path and variable name remain stable
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
# TASK TOOL: CHECK FOR ROLE TEAMMATE SPAWN
# =============================================================================

# Role teammates that require pf-3 sentinel before spawning.
# Function teammates (cf-security, cf-knowledge-layer, cf-git-operations) are
# excluded because they spawn at PF1/PF2/PF3 before pf-3 exists.
ROLE_TEAMMATES="cf-development cf-planning cf-documentation cf-review cf-quality-assurance"

if [[ "$TOOL_NAME" == "Task" ]]; then
    TOOL_INPUT="${TOOL_INPUT:-}"
    if [[ -z "$TOOL_INPUT" ]]; then
        exit 0
    fi

    # Extract the prompt/description from the Task tool input to check for role teammates
    _task_text=""
    if command -v jq &>/dev/null; then
        _task_prompt=$(echo "$TOOL_INPUT" | jq -r '.prompt // empty' 2>/dev/null) || true
        _task_name=$(echo "$TOOL_INPUT" | jq -r '.name // empty' 2>/dev/null) || true
        _task_desc=$(echo "$TOOL_INPUT" | jq -r '.description // empty' 2>/dev/null) || true
        _task_text="${_task_prompt} ${_task_name} ${_task_desc}"
    fi

    if [[ -z "$_task_text" ]]; then
        exit 0
    fi

    # Check if the Task spawn references a role teammate
    _is_role_spawn=""
    _matched_role=""
    for _role in $ROLE_TEAMMATES; do
        if echo "$_task_text" | grep -q "$_role"; then
            _is_role_spawn="true"
            _matched_role="$_role"
            break
        fi
    done

    if [[ -z "$_is_role_spawn" ]]; then
        # Not a role teammate spawn -- allow (could be function teammate or explore sub-agent)
        exit 0
    fi

    # It IS a role teammate spawn -- check pf-3 sentinel
    GATE_TYPE="role_teammate_spawn"
    COMMAND="Task(spawn ${_matched_role})"
    # Fall through to sentinel check below
fi

# =============================================================================
# BASH TOOL: CLASSIFY GATED OPERATION
# =============================================================================

# Determine what type of gated operation this is:
#   edit_write  - Edit/Write tool (requires pf-3 sentinel)
#   git_commit  - Bash with git commit (requires pf-3 sentinel)
#   git_push_pr - Bash with git push or gh pr (requires ws-rev sentinel)
#   role_teammate_spawn - Task tool spawning role teammate (requires pf-3 sentinel)
#   ungated     - Bash with other commands (always allowed)

if [[ "$TOOL_NAME" != "Task" ]]; then
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
fi

# =============================================================================
# EARLY CHECK: SESSION STATE UNKNOWN + CRITICAL GATE
# =============================================================================

# If session ID is "unknown", sentinel checks will fail (sentinel directory
# won't exist). For critical gates, block immediately with a clear message
# rather than falling through to graceful degradation.
if [[ "$CODEFLOW_SESSION_ID" == "unknown" ]]; then
    case "$GATE_TYPE" in
        git_push_pr|role_teammate_spawn)
            echo "BLOCKED: PathFlow gate - session state unknown" >&2
            echo "Reason: Cannot verify prerequisites (session ID not registered)" >&2
            echo "Tool: $TOOL_NAME" >&2
            echo "Gate: $GATE_TYPE" >&2
            echo "" >&2
            echo "Ensure PathFlow session is properly initialized (PF1-INIT) before push/PR/teammate spawn." >&2
            echo "" >&2
            echo "⛔ Do NOT bypass by manually creating sentinel files or session state." >&2
            echo "Initialize properly through PF1-INIT." >&2
            exit 2
            ;;
    esac
    # Non-critical gates with unknown session: fall through to sentinel check
    # (which will reach graceful degradation and allow)
fi

# =============================================================================
# SENTINEL-BASED ENFORCEMENT (primary)
# =============================================================================

# Source pathflow state library for sentinel functions
_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ -f "$_PFS_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$_PFS_LIB"
fi

# Determine required sentinel based on gate type
REQUIRED_SENTINEL=""
REQUIRED_DESC=""

case "$GATE_TYPE" in
    edit_write|git_commit|role_teammate_spawn)
        REQUIRED_SENTINEL="pf-3"
        REQUIRED_DESC="PF3-CLASSIFY (branch creation)"
        ;;
    git_push_pr)
        REQUIRED_SENTINEL="ws-rev"
        REQUIRED_DESC="WS-REV (review completed)"
        ;;
esac

# Check sentinel
if [[ -n "$REQUIRED_SENTINEL" ]] && declare -f has_sentinel &>/dev/null; then
    if has_sentinel "$REQUIRED_SENTINEL"; then
        # Sentinel exists — prerequisite met, allow
        exit 0
    fi

    # Sentinel missing — prerequisite not met, block
    # Collect current sentinels for diagnostic message
    CURRENT_SENTINELS=""
    if declare -f list_sentinels &>/dev/null; then
        CURRENT_SENTINELS=$(list_sentinels | tr '\n' ', ' | sed 's/,$//')
    fi

    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "pathflow_gate_sentinel" "$TOOL_NAME" "${COMMAND:-$TOOL_NAME}" "Missing sentinel: pathflow-$REQUIRED_SENTINEL"
    fi

    # Customize block message for role teammate spawns
    _block_reason="$GATE_TYPE requires $REQUIRED_DESC. No pathflow-$REQUIRED_SENTINEL sentinel found."
    if [[ "$GATE_TYPE" == "role_teammate_spawn" ]]; then
        _block_reason="Cannot spawn role teammates before PF3-CLASSIFY. No pathflow-$REQUIRED_SENTINEL sentinel found.
⛔ Do NOT bypass by creating pf-3 sentinel directly. Delegate to cf-git-operations to create a feature branch, which properly advances to PF3."
    fi

    cat >&2 <<EOF
BLOCKED: PathFlow gate - prerequisite not met
Reason: $_block_reason
Tool: $TOOL_NAME
Gate: $GATE_TYPE

Complete earlier phases before this operation.
Current sentinels: ${CURRENT_SENTINELS:-none}

⛔ Do NOT bypass PathFlow by creating sentinels directly, using workarounds, or skipping phases.
Progress through phases sequentially by delegating to the appropriate teammate.
EOF
    exit 2
fi

# =============================================================================
# FALLBACK: GRACEFUL DEGRADATION
# =============================================================================

# Sentinel library not available -- enforcement state is unknown.
# Critical gates (git push/PR, role teammate spawn) default to BLOCK to prevent
# bypassing prerequisites when infrastructure fails.
# Non-critical gates (edit/write, git commit) default to ALLOW to avoid halting
# development work.
case "$GATE_TYPE" in
    git_push_pr|role_teammate_spawn)
        echo "BLOCKED: Cannot verify prerequisites (sentinel library unavailable)" >&2
        echo "Ensure PathFlow session is properly initialized before push/PR/teammate spawn." >&2
        echo "Tool: $TOOL_NAME" >&2
        echo "Gate: $GATE_TYPE" >&2
        exit 2
        ;;
    *)
        # Non-critical: allow with degraded enforcement
        exit 0
        ;;
esac

# =============================================================================
# JSONL PHASE LOOKUP (secondary, commented fallback)
# =============================================================================
# When cf-knowledge-layer writes phase_transition events to JSONL, this section
# can be uncommented to provide a secondary enforcement mechanism.
#
# # Map phase name to numeric level for comparison
# phase_to_num() {
#     case "$1" in
#         PF1*) echo 1 ;; PF2*) echo 2 ;; PF3*) echo 3 ;; PF4*) echo 4 ;;
#         PF5*) echo 5 ;; PF6*) echo 6 ;; PF7*) echo 7 ;; *) echo 0 ;;
#     esac
# }
#
# SESSION_ID_FILE="${PATHFLOW_SESSION_ID_FILE:-$REPO_ROOT/.state/runtime/current-session-id}"
# [[ ! -f "$SESSION_ID_FILE" ]] && exit 0
# CURRENT_SESSION_ID=$(cat "$SESSION_ID_FILE" 2>/dev/null) || true
# [[ -z "$CURRENT_SESSION_ID" ]] && exit 0
#
# JSONL_FILE="${PATHFLOW_JSONL_FILE:-$REPO_ROOT/.state/logs/pathflow-events.jsonl}"
# [[ ! -f "$JSONL_FILE" ]] && exit 0
# command -v jq &>/dev/null || exit 0
#
# CURRENT_PHASE=$(tail -100 "$JSONL_FILE" 2>/dev/null | \
#     jq -r "select(.type==\"phase_transition\" and .session_id==\"$CURRENT_SESSION_ID\") | .phase" 2>/dev/null | \
#     tail -1) || true
# [[ -z "$CURRENT_PHASE" ]] && exit 0
#
# PHASE_NUM=$(phase_to_num "$CURRENT_PHASE")
# REQUIRED_PHASE="" REQUIRED_NUM=0
# case "$GATE_TYPE" in
#     edit_write|git_commit) REQUIRED_PHASE="PF4-EXECUTE"; REQUIRED_NUM=4 ;;
#     git_push_pr) REQUIRED_PHASE="PF6-COMPLETE"; REQUIRED_NUM=6 ;;
# esac
# [[ $PHASE_NUM -ge $REQUIRED_NUM ]] && exit 0
#
# cat >&2 <<EOF
# BLOCKED: PathFlow gate - phase requirement not met
# Reason: Current phase is $CURRENT_PHASE but $REQUIRED_PHASE is required
# Tool: $TOOL_NAME | Gate: $GATE_TYPE
# Complete earlier phases before this operation.
# EOF
# exit 2
