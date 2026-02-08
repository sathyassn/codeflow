#!/usr/bin/env bash
# Purpose:   Config-driven SessionStart behavioral instructions
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-instructions.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   2.1.0
#
# Skills:    working-protocol, memory-management
# Operation: Session initialization and Section 2 enforcement
#
# Config:    .codeflow/config/instructions/instructions-config.json
#
# This hook:
#   - Reads enabled instructions from instructions-config.json
#   - Outputs behavioral instructions for session start
#   - Falls back to hardcoded instructions if config unavailable
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Instructions output successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.1.0"

# =============================================================================
# CONFIGURATION
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

INSTRUCTIONS_DIR="$REPO_ROOT/.codeflow/config/instructions"
CONFIG_FILE="$INSTRUCTIONS_DIR/instructions-config.json"

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
# V4: PATHFLOW CONTEXT LOADING
# =============================================================================
# When PathFlow is active, output additional context about current phase and team

PATHFLOW_ACTIVE="/tmp/claude/managed/state/pathflow-active"
if [[ -f "$PATHFLOW_ACTIVE" ]]; then
    echo ""
    echo "PATHFLOW SESSION ACTIVE"
    echo "======================"

    # Check completed phases by looking at sentinels
    SENTINEL_DIR="$REPO_ROOT/.state/sentinels"
    if [[ -d "$SENTINEL_DIR" ]]; then
        COMPLETED_PHASES=""
        for phase_file in "$SENTINEL_DIR"/pathflow:pf-*; do
            [[ -f "$phase_file" ]] || continue
            phase_name=$(basename "$phase_file")
            COMPLETED_PHASES="${COMPLETED_PHASES}  - ${phase_name}\n"
        done

        if [[ -n "$COMPLETED_PHASES" ]]; then
            echo "Completed phases:"
            echo -e "$COMPLETED_PHASES"
        fi
    fi

    echo "Mode: agent-teams"
    echo "PCV: bypassed (WS-REV provides quality assurance)"
    echo ""
fi

exit 0
