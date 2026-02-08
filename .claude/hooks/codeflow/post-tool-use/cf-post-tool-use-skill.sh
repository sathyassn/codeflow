#!/usr/bin/env bash
# Purpose:   PostToolUse hook for skill invocation processing and sentinel creation
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-skill.sh
# Hook Type: PostToolUse
# Matcher:   Skill
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Creates sentinel tokens after skill invocation (V3)
#   - Logs skill usage to audit trail
#   - Logs sentinel creation to security audit log
#
# Sentinel Creation Flow (V3):
#   1. Extract skill name and operation from args
#   2. Look up operation config (pattern, TTL) from enforcement-policy.json
#   3. Create sentinel file via sentinel library
#   4. Log to security audit log
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Always succeeds (post-processing should not block)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# EARLY EXIT FOR NON-SKILL TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Skill" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
# shellcheck disable=SC2034  # CONFIG exported for sentinel library
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SENTINEL_LIB="$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"
LOG_DIR="$REPO_ROOT/.state/logs/sessions"
SECURITY_LOG_DIR="$REPO_ROOT/.state/logs/security/sentinel"

# Get session ID from environment
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# Source sentinel library if available
if [[ -f "$SENTINEL_LIB" ]]; then
    # shellcheck source=.codeflow/scripts/security/sentinel/cf-sentinel.sh
    source "$SENTINEL_LIB"
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
TOOL_RESULT="${TOOL_RESULT:-}"

# Extract skill parameters
SKILL_NAME=""
SKILL_ARGS=""

if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
    SKILL_NAME=$(echo "$TOOL_INPUT" | jq -r '.skill // empty' 2>/dev/null || echo "")
    SKILL_ARGS=$(echo "$TOOL_INPUT" | jq -r '.args // empty' 2>/dev/null || echo "")
fi

if [[ -z "$SKILL_NAME" ]]; then
    exit 0
fi

# Normalize skill name (remove cf- prefix for config lookup)
SKILL_BASE="${SKILL_NAME#cf-}"

# =============================================================================
# SENTINEL CREATION (V3)
# =============================================================================

# Extract operation from args (first word or entire args if simple)
extract_operation() {
    local args="$1"
    # First word of args is typically the operation, remove quotes
    echo "$args" | awk '{print $1}' | tr -d "'\""
}

create_skill_sentinel() {
    local skill="$1"
    local operation="$2"

    # Check if sentinel library is loaded
    if ! declare -f sentinel_create &>/dev/null; then
        return 1
    fi

    # Get operation config from enforcement-policy.json
    local pattern ttl
    pattern=$(sentinel_get_operation_pattern "$skill" "$operation" 2>/dev/null || echo "")
    ttl=$(sentinel_get_operation_ttl "$skill" "$operation" 2>/dev/null || echo "600")

    # If no pattern found, operation may not require sentinel
    if [[ -z "$pattern" ]]; then
        return 1
    fi

    # Create sentinel using library function
    local sentinel_file
    sentinel_file=$(sentinel_create "$skill" "$operation" "$pattern" "$ttl" 2>/dev/null || echo "")

    if [[ -n "$sentinel_file" ]]; then
        echo "$sentinel_file"
        return 0
    fi

    return 1
}

log_sentinel_creation() {
    local skill="$1"
    local operation="$2"
    local sentinel_file="$3"
    local ttl="$4"

    # Ensure security log directory exists
    mkdir -p "$SECURITY_LOG_DIR" 2>/dev/null || true

    local log_date
    log_date=$(date +%Y-%m-%d)
    local log_file="$SECURITY_LOG_DIR/sentinel-$log_date.jsonl"

    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

    if command -v jq &>/dev/null; then
        local log_entry
        log_entry=$(jq -nc \
            --arg ts "$ts" \
            --arg level "INFO" \
            --arg session_id "$SESSION_ID" \
            --arg event "sentinel_created" \
            --arg sentinel_type "skill" \
            --arg skill "$skill" \
            --arg operation "$operation" \
            --arg sentinel_id "$sentinel_file" \
            --argjson ttl_sec "$ttl" \
            --arg path "/tmp/claude/managed/sentinels/$sentinel_file" \
            '{ts: $ts, level: $level, session_id: $session_id, event: $event, sentinel_type: $sentinel_type, skill: $skill, operation: $operation, sentinel_id: $sentinel_id, ttl_sec: $ttl_sec, path: $path}')

        echo "$log_entry" >> "$log_file" 2>/dev/null || true
    fi
}

# Determine if skill was successful
SKILL_SUCCESS="true"
if [[ "$TOOL_RESULT" == *"error"* ]] || [[ "$TOOL_RESULT" == *"BLOCKED"* ]]; then
    SKILL_SUCCESS="false"
fi

# Only create sentinel if skill succeeded
if [[ "$SKILL_SUCCESS" == "true" ]] && [[ -n "$SKILL_ARGS" ]]; then
    OPERATION=$(extract_operation "$SKILL_ARGS")

    if [[ -n "$OPERATION" ]]; then
        SENTINEL_FILE=$(create_skill_sentinel "$SKILL_BASE" "$OPERATION" 2>/dev/null || echo "")

        if [[ -n "$SENTINEL_FILE" ]]; then
            # Get TTL for logging
            TTL=$(sentinel_get_operation_ttl "$SKILL_BASE" "$OPERATION" 2>/dev/null || echo "600")
            log_sentinel_creation "$SKILL_BASE" "$OPERATION" "$SENTINEL_FILE" "$TTL"
        fi
    fi
fi

# =============================================================================
# SKILL RESULT LOGGING
# =============================================================================

# Ensure log directory exists
mkdir -p "$LOG_DIR" 2>/dev/null || true

# Get current date for log file
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$LOG_DIR/skills-$LOG_DATE.jsonl"

# Create log entry
TS=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

if command -v jq &>/dev/null; then
    LOG_ENTRY=$(jq -nc \
        --arg ts "$TS" \
        --arg session_id "$SESSION_ID" \
        --arg skill_name "$SKILL_NAME" \
        --arg skill_args "$SKILL_ARGS" \
        --arg success "$SKILL_SUCCESS" \
        '{ts: $ts, session_id: $session_id, event: "skill_completed", skill_name: $skill_name, skill_args: $skill_args, success: ($success == "true")}')

    echo "$LOG_ENTRY" >> "$LOG_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
