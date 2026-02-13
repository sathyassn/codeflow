#!/usr/bin/env bash
# Purpose:   UserPromptSubmit hook for prompt logging
# Location:  .claude/hooks/codeflow/user-prompt-submit/cf-user-prompt-submit-logging.sh
# Hook Type: UserPromptSubmit
# Usage:     Called by Claude Code when user submits a prompt
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Logs user prompts for audit (with optional privacy mode)
#   - Tracks prompt count for session
#   - Records prompt metadata (length, type classification, hash)
#   - Reads logging config from enforcement-policy.json
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (UserPromptSubmit hooks never block)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# STDIN READING (Claude Code hook protocol)
# =============================================================================
STDIN_INPUT=""
if [[ ! -t 0 ]]; then
    STDIN_INPUT=$(cat)
fi

# Extract fields from stdin JSON (primary) or env vars (testing fallback)
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
USER_PROMPT="${USER_PROMPT:-}"

if [[ -n "$STDIN_INPUT" ]] && command -v jq &>/dev/null; then
    stdin_session=$(echo "$STDIN_INPUT" | jq -r '.session_id // empty' 2>/dev/null || echo "")
    [[ -n "$stdin_session" ]] && SESSION_ID="$stdin_session"

    stdin_prompt=$(echo "$STDIN_INPUT" | jq -r '.user_prompt // empty' 2>/dev/null || echo "")
    [[ -n "$stdin_prompt" ]] && USER_PROMPT="$stdin_prompt"
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# CONFIG LOADING
# =============================================================================

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Logging config defaults
LOG_ENABLED=true
CAPTURE_FULL_TEXT=true
DETECT_INTENT=true
PRIVACY_MODE=false
LOG_DIR="$REPO_ROOT/.state/logs/sessions"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    LOG_ENABLED=$(jq -r '.logging.user_prompt.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    CAPTURE_FULL_TEXT=$(jq -r '.logging.user_prompt.capture_full_text // true' "$CONFIG" 2>/dev/null || echo "true")
    DETECT_INTENT=$(jq -r '.logging.user_prompt.detect_intent // true' "$CONFIG" 2>/dev/null || echo "true")
    PRIVACY_MODE=$(jq -r '.logging.user_prompt.privacy_mode // false' "$CONFIG" 2>/dev/null || echo "false")

    CONFIG_LOG_DIR=$(jq -r '.logging.session_start.log_directory // ".state/logs/sessions"' "$CONFIG" 2>/dev/null || echo ".state/logs/sessions")
    [[ -n "$CONFIG_LOG_DIR" ]] && LOG_DIR="$REPO_ROOT/$CONFIG_LOG_DIR"
fi

# Skip if logging disabled
if [[ "$LOG_ENABLED" != "true" ]]; then
    exit 0
fi

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

# =============================================================================
# PROMPT PROCESSING
# =============================================================================

# Calculate prompt length
PROMPT_LENGTH=${#USER_PROMPT}

# Calculate prompt hash (sha256 for deduplication and privacy mode)
PROMPT_HASH=""
if [[ -n "$USER_PROMPT" ]]; then
    PROMPT_HASH=$(printf '%s' "$USER_PROMPT" | shasum -a 256 2>/dev/null | cut -d' ' -f1 || echo "")
fi

# Detect prompt type (basic classification)
PROMPT_TYPE="general"
if [[ "$DETECT_INTENT" == "true" ]] && [[ -n "$USER_PROMPT" ]]; then
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
    elif [[ "$USER_PROMPT" =~ (review|check|verify|test) ]]; then
        PROMPT_TYPE="review"
    elif [[ "$USER_PROMPT" =~ (find|search|locate|where) ]]; then
        PROMPT_TYPE="navigation"
    fi
fi

# Get git context for logging
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")

# =============================================================================
# LOG ENTRY
# =============================================================================

if command -v jq &>/dev/null; then
    if [[ "$PRIVACY_MODE" == "true" ]]; then
        # Privacy mode: hash + length only, no prompt text
        LOG_ENTRY=$(jq -nc \
            --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg session_id "$SESSION_ID" \
            --arg event "prompt_submitted" \
            --argjson prompt_count "$PROMPT_COUNT" \
            --argjson prompt_length "$PROMPT_LENGTH" \
            --arg prompt_hash "$PROMPT_HASH" \
            --arg prompt_type "$PROMPT_TYPE" \
            --arg git_branch "$GIT_BRANCH" \
            '{ts: $ts, session_id: $session_id, event: $event, prompt_count: $prompt_count, prompt_length: $prompt_length, prompt_hash: $prompt_hash, prompt_type: $prompt_type, git_branch: $git_branch}')
    elif [[ "$CAPTURE_FULL_TEXT" == "true" ]] && [[ -n "$USER_PROMPT" ]]; then
        # Full capture mode: includes prompt text
        LOG_ENTRY=$(jq -nc \
            --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg session_id "$SESSION_ID" \
            --arg event "prompt_submitted" \
            --argjson prompt_count "$PROMPT_COUNT" \
            --argjson prompt_length "$PROMPT_LENGTH" \
            --arg prompt_hash "$PROMPT_HASH" \
            --arg prompt_text "$USER_PROMPT" \
            --arg prompt_type "$PROMPT_TYPE" \
            --arg git_branch "$GIT_BRANCH" \
            '{ts: $ts, session_id: $session_id, event: $event, prompt_count: $prompt_count, prompt_length: $prompt_length, prompt_hash: $prompt_hash, prompt_text: $prompt_text, prompt_type: $prompt_type, git_branch: $git_branch}')
    else
        # Default: metadata only (no full text, no explicit privacy)
        LOG_ENTRY=$(jq -nc \
            --arg ts "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg session_id "$SESSION_ID" \
            --arg event "prompt_submitted" \
            --argjson prompt_count "$PROMPT_COUNT" \
            --argjson prompt_length "$PROMPT_LENGTH" \
            --arg prompt_hash "$PROMPT_HASH" \
            --arg prompt_type "$PROMPT_TYPE" \
            --arg git_branch "$GIT_BRANCH" \
            '{ts: $ts, session_id: $session_id, event: $event, prompt_count: $prompt_count, prompt_length: $prompt_length, prompt_hash: $prompt_hash, prompt_type: $prompt_type, git_branch: $git_branch}')
    fi

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
