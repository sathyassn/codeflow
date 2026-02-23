#!/usr/bin/env bash
# Purpose:   Config-driven SessionStart behavioral instructions
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-instructions.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   3.0.0
#
# Teammates: cf-knowledge-layer (memory/task ops)
# Operation: Session initialization and Section 2 enforcement
#
# Config:    .codeflow/config/instructions/instructions-config.json
#
# This hook:
#   - Reads enabled instructions from instructions-config.json
#   - Outputs behavioral instructions for session start
#   - Falls back to hardcoded instructions if config unavailable
#   - Shows PathFlow context when pathflow mode is active
#   - Shows active task context when tasks exist
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Instructions output successfully (always exits 0)

set -euo pipefail

# Consume stdin to prevent blocking on pipe (SessionStart provides JSON)
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null) || true
        [[ -n "${_sid:-}" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="3.0.0"

# =============================================================================
# CONFIGURATION
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

INSTRUCTIONS_DIR="$REPO_ROOT/.codeflow/config/instructions"
CONFIG_FILE="$INSTRUCTIONS_DIR/instructions-config.json"

# Source work-state library for active task functions
# shellcheck source=/dev/null
source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh" 2>/dev/null || true

# Security library for mode detection
SECURITY_LIB="$REPO_ROOT/.codeflow/scripts/security/lib/security-lib.sh"

# =============================================================================
# SECTION 1: CONFIG-DRIVEN SESSIONSTART INSTRUCTIONS
# =============================================================================
# Read enabled instructions from instructions-config.json .hooks.SessionStart and output content

if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
  # Get list of enabled SessionStart instructions
  ENABLED_INSTRUCTIONS=$(jq -r '.hooks.SessionStart | to_entries[] | select(.value.enabled == true) | .key' "$CONFIG_FILE" 2>/dev/null || echo "")

  for instruction in $ENABLED_INSTRUCTIONS; do
    # Get the file for this instruction
    INSTRUCTION_FILE=$(jq -r ".hooks.SessionStart[\"$instruction\"].file // empty" "$CONFIG_FILE" 2>/dev/null || echo "")

    if [[ -n "$INSTRUCTION_FILE" && -f "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE" ]]; then
      cat "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE" 2>/dev/null || true
      echo ""  # Blank line between instructions
    fi
  done
else
  # Fallback: hardcoded instruction if config not available
  cat <<'EOF'
SESSION START - EXECUTE CLAUDE.md SECTION 2
You MUST execute the Session Start procedure from CLAUDE.md Section 2.
Check for active work (grep Status: active), present options to user, wait for choice.
EOF
fi


# =============================================================================
# SECTION 2: ACTIVE TASK CONTEXT
# =============================================================================
# Show active task information if available

if is_task_active; then
    TASK_ID=$(get_active_task_id)
    TASK_STATUS="in_progress"
    _atf="$(get_active_task_file)"
    TASK_DESC=""
    if command -v jq &>/dev/null && [[ -f "$_atf" ]]; then
        TASK_DESC=$(jq -r '.description // empty' "$_atf" 2>/dev/null || echo "")
    fi

    if [[ -n "$TASK_ID" ]]; then
        echo ""
        echo "ACTIVE TASKS DETECTED"
        echo "====================="
        echo ""
        echo "Incomplete tasks found:"
        printf '  - %s (%s)\n' "$TASK_ID" "$TASK_STATUS"
        if [[ -n "$TASK_DESC" ]]; then
            printf '    "%s"\n' "$TASK_DESC"
        fi
        echo ""
        echo "Options:"
        echo "  1. Resume task"
        echo "  2. Start new work"
        echo "  3. Review tasks"
    fi
else
    echo ""
    echo "ACTIVE TASKS DETECTED: None"
    echo ""
    echo "IMPORTANT: Register work before making modifications."
    echo "Delegate to cf-knowledge-layer teammate: SendMessage(recipient=\"cf-knowledge-layer\", content=\"ensure-work-registered\")"
fi

# =============================================================================
# SECTION 3: PATHFLOW CONTEXT LOADING
# =============================================================================
# When PathFlow is active, output additional context about current phase and team

# Use security-lib.sh for mode detection if available, otherwise check flag directly
_is_pathflow_active() {
    # shellcheck disable=SC1090
    if [[ -f "$SECURITY_LIB" ]]; then
        source "$SECURITY_LIB"
        is_pathflow_active
    else
        # Direct flag check as fallback when security-lib.sh unavailable
        local flag_file="$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-unknown}/pathflow/is-pathflow-active"
        [[ -f "$flag_file" ]]
    fi
}

if _is_pathflow_active; then
    echo ""
    echo "PATHFLOW SESSION ACTIVE"
    echo "======================"

    # Check completed phases by looking at sentinels
    SENTINEL_DIR="$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID"
    if [[ -d "$SENTINEL_DIR" ]]; then
        COMPLETED_PHASES=""
        for phase_file in "$SENTINEL_DIR"/pathflow-pf-*; do
            [[ -f "$phase_file" ]] || continue
            phase_name=$(basename "$phase_file")
            COMPLETED_PHASES="${COMPLETED_PHASES}  - ${phase_name}
"
        done

        if [[ -n "$COMPLETED_PHASES" ]]; then
            echo "Completed phases:"
            printf '%s' "$COMPLETED_PHASES"
        fi
    fi

    echo "Mode: pathflow"
    echo "PCV: bypassed (WS-REV provides quality assurance)"
    echo ""
fi

exit 0
