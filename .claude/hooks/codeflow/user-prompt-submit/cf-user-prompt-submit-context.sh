#!/usr/bin/env bash
# Purpose:   UserPromptSubmit hook for context reminders
# Location:  .claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit-context.sh
# Hook Type: UserPromptSubmit
# Usage:     Called by Claude Code when user submits a prompt
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Provides context reminders to the agent
#   - Injects relevant information based on project state
#   - Outputs reminders as <user-prompt-submit-hook> content
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (UserPromptSubmit hooks never block)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# =============================================================================
# CONFIG LOADING
# =============================================================================

# Read protected branches from config with defaults
PROTECTED_BRANCHES="main master"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Read from git_format.protected_branches (primary for branch warnings)
    PB=$(jq -r '.git_format.protected_branches // ["main", "master"] | join(" ")' "$CONFIG" 2>/dev/null || echo "main master")
    if [[ -n "$PB" ]]; then
        PROTECTED_BRANCHES="$PB"
    fi
fi

# =============================================================================
# CONTEXT GATHERING
# =============================================================================

# Get git status
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
UNCOMMITTED_COUNT=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ')

# Check for active work items
ACTIVE_WORK=""
ACTIVE_WORK_FILE="$REPO_ROOT/.state/active-work.json"
if [[ -f "$ACTIVE_WORK_FILE" ]] && command -v jq &>/dev/null; then
    ACTIVE_WORK=$(jq -r '.current_task // empty' "$ACTIVE_WORK_FILE" 2>/dev/null || echo "")
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
# CONTEXT OUTPUT
# =============================================================================

# Only output reminders if there's relevant context to share

# Reminder about uncommitted changes
if [[ "$UNCOMMITTED_COUNT" -gt 0 ]]; then
    echo "<user-prompt-submit-hook>"
    echo "Note: $UNCOMMITTED_COUNT uncommitted changes on branch '$GIT_BRANCH'"
    echo "</user-prompt-submit-hook>"
fi

# Reminder about protected branch (config-driven)
if is_protected_branch "$GIT_BRANCH"; then
    echo "<user-prompt-submit-hook>"
    echo "Warning: On protected branch '$GIT_BRANCH'. Create a feature branch for changes."
    echo "</user-prompt-submit-hook>"
fi

# Reminder about active work
if [[ -n "$ACTIVE_WORK" ]]; then
    echo "<user-prompt-submit-hook>"
    echo "Active work: $ACTIVE_WORK"
    echo "</user-prompt-submit-hook>"
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
