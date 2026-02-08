#!/usr/bin/env bash
# Purpose:   UserPromptSubmit hook for prompt logging
# Location:  .claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit-logging.sh
# Hook Type: UserPromptSubmit
# Usage:     Called by Claude Code when user submits a prompt
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Logs user prompts (redacted) for audit
#   - Tracks prompt count for session
#   - Records prompt metadata (length, type classification)
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

# Read log directory from config with default
LOG_DIR="$REPO_ROOT/.state/logs/sessions"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    CONFIG_LOG_DIR=$(jq -r '.logging.session_start.log_directory // ".state/logs/sessions"' "$CONFIG" 2>/dev/null || echo ".state/logs/sessions")
    if [[ -n "$CONFIG_LOG_DIR" ]]; then
        LOG_DIR="$REPO_ROOT/$CONFIG_LOG_DIR"
    fi
fi

# Get session ID from environment
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# Get prompt from environment (if provided)
USER_PROMPT="${USER_PROMPT:-}"

# =============================================================================
# PROMPT LOGGING
# =============================================================================

mkdir -p "$LOG_DIR" 2>/dev/null || true

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/prompts-$LOG_DATE.jsonl"

# Increment prompt counter for this session
PROMPT_COUNTER_FILE="$LOG_DIR/.prompt-counter-${SESSION_ID}"
PROMPT_COUNT=1
if [[ -f "$PROMPT_COUNTER_FILE" ]]; then
    PROMPT_COUNT=$(<"$PROMPT_COUNTER_FILE") 2>/dev/null || PROMPT_COUNT=0
    PROMPT_COUNT=$((PROMPT_COUNT + 1))
fi
echo "$PROMPT_COUNT" > "$PROMPT_COUNTER_FILE" 2>/dev/null || true

# Calculate prompt length (don't log full content for privacy)
PROMPT_LENGTH=${#USER_PROMPT}

# Detect prompt type (basic classification)
PROMPT_TYPE="general"
if [[ "$USER_PROMPT" == "/"* ]]; then
    PROMPT_TYPE="command"
elif [[ "$USER_PROMPT" == *"?"* ]]; then
    PROMPT_TYPE="question"
elif [[ "$USER_PROMPT" =~ (fix|bug|error|issue) ]]; then
    PROMPT_TYPE="debugging"
elif [[ "$USER_PROMPT" =~ (create|add|implement|build) ]]; then
    PROMPT_TYPE="creation"
elif [[ "$USER_PROMPT" =~ (update|change|modify|edit) ]]; then
    PROMPT_TYPE="modification"
fi

# Get git context for logging
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")

if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --arg session_id "$SESSION_ID" \
        --arg event "prompt_submitted" \
        --argjson prompt_count "$PROMPT_COUNT" \
        --argjson prompt_length "$PROMPT_LENGTH" \
        --arg prompt_type "$PROMPT_TYPE" \
        --arg git_branch "$GIT_BRANCH" \
        '{ts: $ts, session_id: $session_id, event: $event, prompt_count: $prompt_count, prompt_length: $prompt_length, prompt_type: $prompt_type, git_branch: $git_branch}')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
