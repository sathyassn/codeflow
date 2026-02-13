#!/usr/bin/env bash
# Purpose:   UserPromptSubmit hook for context reminders and config-driven instructions
# Location:  .claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit.sh
# Hook Type: UserPromptSubmit
# Usage:     Called by Claude Code when user submits a prompt
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Reads stdin JSON from Claude Code hook protocol
#   - Outputs config-driven instructions from instructions-config.json
#   - Provides context reminders (uncommitted changes, protected branch, active work)
#   - Alerts about parallel work (worktrees, model sessions)
#   - Detects PathFlow mode for team coordination
#   - Outputs reminders as <user-prompt-submit-hook> content
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (UserPromptSubmit hooks never block)

set -euo pipefail

# =============================================================================
# STDIN READING (Claude Code hook protocol)
# =============================================================================
STDIN_INPUT=""
if [[ ! -t 0 ]]; then
    STDIN_INPUT=$(cat)
fi

# Extract session_id from stdin (for logging/state)
SESSION_ID=""
if [[ -n "$STDIN_INPUT" ]] && command -v jq &>/dev/null; then
    # shellcheck disable=SC2034  # SESSION_ID available for future sections
    SESSION_ID=$(echo "$STDIN_INPUT" | jq -r '.session_id // empty' 2>/dev/null || echo "")
fi

CODEFLOW_SESSION_ID="${SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Source work-state library for active task functions
# shellcheck source=/dev/null
source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh" 2>/dev/null || true

# Source security library (provides is_pathflow_active via context-lib.sh)
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# CONFIG LOADING
# =============================================================================

# Read protected branches from config with defaults
PROTECTED_BRANCHES="main master"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Read from protected_branches (root-level key)
    PB=$(jq -r '.protected_branches // ["main", "master"] | join(" ")' "$CONFIG" 2>/dev/null || echo "main master")
    if [[ -n "$PB" ]]; then
        PROTECTED_BRANCHES="$PB"
    fi
fi

# =============================================================================
# CONTEXT GATHERING
# =============================================================================

# Get git status
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
UNCOMMITTED_COUNT=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ') || UNCOMMITTED_COUNT=0

# Check for active work items (using work-state.sh canonical path)
ACTIVE_WORK=""
if is_task_active; then
    ACTIVE_WORK=$(get_active_task_id)
fi

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

# Check if current branch is protected (config-driven)
is_protected_branch() {
    local branch="$1"
    local protected

    for protected in $PROTECTED_BRANCHES; do
        # Handle glob patterns (e.g., release/*)
        # shellcheck disable=SC2053  # Pattern matching intentional
        if [[ "$branch" == $protected ]]; then
            return 0
        fi
    done

    return 1
}

# =============================================================================
# SECTION 1: CONFIG-DRIVEN INSTRUCTIONS
# =============================================================================
INSTRUCTIONS_CONFIG="$REPO_ROOT/.codeflow/config/instructions/instructions-config.json"
INSTRUCTIONS_DIR="$REPO_ROOT/.codeflow/config/instructions"

if [[ -f "$INSTRUCTIONS_CONFIG" ]] && command -v jq &>/dev/null; then
    ENABLED=$(jq -r '.hooks.UserPromptSubmit | to_entries[] | select(.value.enabled == true) | .key' "$INSTRUCTIONS_CONFIG" 2>/dev/null)
    for instruction in $ENABLED; do
        IFILE=$(jq -r ".hooks.UserPromptSubmit[\"$instruction\"].file // empty" "$INSTRUCTIONS_CONFIG" 2>/dev/null)
        if [[ -n "$IFILE" && -f "$INSTRUCTIONS_DIR/$IFILE" ]]; then
            echo "<user-prompt-submit-hook>"
            cat "$INSTRUCTIONS_DIR/$IFILE"
            echo "</user-prompt-submit-hook>"
        fi
    done
else
    # Fallback: hardcoded instruction if config not available
    echo "<user-prompt-submit-hook>"
    echo "MUST INVOKE Skill('cf-working-protocol') for all workflows"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SECTION 2: GIT CONTEXT (uncommitted changes reminder)
# =============================================================================

# Only output reminders if there's relevant context to share

# Reminder about uncommitted changes
if [[ "$UNCOMMITTED_COUNT" -gt 0 ]]; then
    echo "<user-prompt-submit-hook>"
    echo "Note: $UNCOMMITTED_COUNT uncommitted changes on branch '$GIT_BRANCH'"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SECTION 3: PROTECTED BRANCH WARNING
# =============================================================================

# Reminder about protected branch (config-driven)
if is_protected_branch "$GIT_BRANCH"; then
    echo "<user-prompt-submit-hook>"
    echo "Warning: On protected branch '$GIT_BRANCH'. Create a feature branch before changes."
    echo "  Skill('cf-git-workflow', args='create-feature-branch name=feat/...')"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SECTION 4: ACTIVE WORK
# =============================================================================

# Reminder about active work
if [[ -n "$ACTIVE_WORK" ]]; then
    echo "<user-prompt-submit-hook>"
    echo "Active work: $ACTIVE_WORK"
    echo "</user-prompt-submit-hook>"
else
    echo "<user-prompt-submit-hook>"
    echo "Note: No active task registered. Consider: Skill('cf-task-management', args='ensure-work-registered')"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SECTION 5: PARALLEL WORK AWARENESS
# =============================================================================
STATE_DIR="$REPO_ROOT/.state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"
SESSIONS_FILE="$STATE_DIR/model-sessions.yaml"
ACTIVE_WORKTREES=0
ACTIVE_SESSIONS=0

if [[ -f "$WORKTREES_FILE" ]]; then
    ACTIVE_WORKTREES=$(grep -c "status: active" "$WORKTREES_FILE" 2>/dev/null) || ACTIVE_WORKTREES=0
fi

if [[ -f "$SESSIONS_FILE" ]]; then
    ACTIVE_SESSIONS=$(grep -c "status: working" "$SESSIONS_FILE" 2>/dev/null) || ACTIVE_SESSIONS=0
fi

if [[ "$ACTIVE_WORKTREES" -gt 0 || "$ACTIVE_SESSIONS" -gt 0 ]]; then
    echo "<user-prompt-submit-hook>"
    echo "Parallel work active:"
    [[ "$ACTIVE_WORKTREES" -gt 0 ]] && echo "  Worktrees: $ACTIVE_WORKTREES - Skill('cf-git-workflow', args='list-worktrees')"
    [[ "$ACTIVE_SESSIONS" -gt 0 ]] && echo "  Model sessions: $ACTIVE_SESSIONS - Skill('cf-model-orchestrator', args='list-sessions')"
    echo "  Check conflicts: Skill('cf-model-orchestrator', args='check-scope-conflict')"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SECTION 6: PATHFLOW MODE AWARENESS
# =============================================================================
# Check PathFlow mode using canonical function from context-lib.sh
if is_pathflow_active 2>/dev/null; then
    echo "<user-prompt-submit-hook>"
    echo "PathFlow mode active: Coordinate with teammates for specialized tasks."
    echo "  Git operations -> cf-gitops | Code review -> cf-reviewer"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
