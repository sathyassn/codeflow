#!/usr/bin/env bash
# Purpose:   Stop hook for work verification (PCV)
# Location:  .claude/hooks/codeflow/stop/cf-stop-verify-work.sh
# Hook Type: Stop
# Usage:     Called by Claude Code at stop event
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Reads JSON from stdin (session_id, transcript_path, stop_hook_active, agent_id)
#   - Parses conversation transcript for PCV markers in current turn
#   - Validates tier-specific sections (ARTIFACTS, VERIFICATION, ADVERSARIAL)
#   - Outputs block JSON on stdout when PCV is missing
#   - Implements retry mechanism with state file to prevent deadlock
#
# Verification tiers (from enforcement-policy.json):
#   - Tier 1: verify-work, TIER
#   - Tier 2: verify-work, TIER, ARTIFACTS, VERIFICATION
#   - Tier 3: verify-work, TIER, ARTIFACTS, VERIFICATION, ADVERSARIAL
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (stop hooks must never return non-zero)
#
# Output:
#   stdout - Block JSON when PCV missing (read by Claude Code)
#   stderr - Debug/advisory messages only

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# STDIN READING
# =============================================================================

# Read JSON from stdin (Claude Code provides session context)
INPUT=$(cat)

# Extract fields from stdin JSON
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty' 2>/dev/null || echo "")
TRANSCRIPT_PATH=$(echo "$INPUT" | jq -r '.transcript_path // empty' 2>/dev/null || echo "")
STOP_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo "false")
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id // empty' 2>/dev/null || echo "")

# Set CODEFLOW_SESSION_ID for security-lib and sentinel library
CODEFLOW_SESSION_ID="${SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# INFINITE LOOP GUARD
# =============================================================================

# If stop_hook_active is true, we are in an active retry from a previous block.
# Exit immediately to prevent infinite loop.
if [ "$STOP_ACTIVE" = "true" ]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"

# =============================================================================
# PATHFLOW MODE BYPASS
# =============================================================================
# In PathFlow mode, WS-REV stage replaces PCV verification.
# The cf-reviewer teammate provides quality assurance instead of self-verification.

# Source security library (provides is_pathflow_active via context-lib.sh)
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [ -f "$LIB_DIR/security-lib.sh" ]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

if is_pathflow_active 2>/dev/null; then
    exit 0
fi

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SETTINGS_LOCAL="$REPO_ROOT/.claude/settings.local.json"
SETTINGS="$REPO_ROOT/.claude/settings.json"

# State file for retry tracking
STATE_ID="${SESSION_ID:-${AGENT_ID:-unknown}}"
STATE_FILE="$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/verify-work-retry-${STATE_ID}"

# =============================================================================
# CONFIG LOADING
# =============================================================================

# Read verification config from enforcement-policy.json with defaults
REQUIRE_PCV=true
MAX_RETRIES=3

if [ -f "$CONFIG" ] && command -v jq >/dev/null 2>&1; then
    REQUIRE_PCV=$(jq -r '.stop_verification.enabled // true' "$CONFIG" 2>/dev/null || echo "true")
    MAX_RETRIES=$(jq -r '.stop_verification.max_retries // 3' "$CONFIG" 2>/dev/null || echo "3")
fi

# Skip verification if not required
if [ "$REQUIRE_PCV" != "true" ]; then
    exit 0
fi

# =============================================================================
# SETTINGS OVERRIDE
# =============================================================================

# Read _verify_work_config from settings (local overrides project)
get_settings_config() {
    local key="$1"
    local default="$2"
    local value=""

    # Try settings.local.json first (local overrides)
    if [ -f "$SETTINGS_LOCAL" ] && command -v jq >/dev/null 2>&1; then
        value=$(jq -r "$key // empty" "$SETTINGS_LOCAL" 2>/dev/null || echo "")
    fi

    # Fall back to settings.json
    if [ -z "$value" ] && [ -f "$SETTINGS" ] && command -v jq >/dev/null 2>&1; then
        value=$(jq -r "$key // empty" "$SETTINGS" 2>/dev/null || echo "")
    fi

    echo "${value:-$default}"
}

# Allow settings override for max retries
SETTINGS_RETRIES=$(get_settings_config '._verify_work_config.maxRetries' '')
if [ -n "$SETTINGS_RETRIES" ]; then
    MAX_RETRIES="$SETTINGS_RETRIES"
fi

# =============================================================================
# STOP REASON CHECK
# =============================================================================

# Check stop reason - skip verification on user cancellation or context limit
# Env var fallback for testing compatibility
STOP_REASON="${STOP_REASON:-}"

# Also try to extract from stdin if available (future-proofing)
if [ -z "$STOP_REASON" ]; then
    STOP_REASON=$(echo "$INPUT" | jq -r '.stop_reason // empty' 2>/dev/null || echo "")
fi

if [ "$STOP_REASON" = "user_cancelled" ] || [ "$STOP_REASON" = "context_limit" ]; then
    exit 0
fi

# =============================================================================
# TRANSCRIPT VALIDATION
# =============================================================================

# Expand ~ to home directory if present
TRANSCRIPT_PATH="${TRANSCRIPT_PATH/#\~/$HOME}"

# If no transcript path or file doesn't exist, can't validate - allow stop
if [ -z "$TRANSCRIPT_PATH" ] || [ ! -f "$TRANSCRIPT_PATH" ]; then
    exit 0
fi

# =============================================================================
# PCV MARKER CONFIG
# =============================================================================

# Required marker from config
REQUIRED_MARKER="🔍"
REQUIRED_TEXT="verify-work"

# Tier patterns
TIER_1_PATTERN="TIER 1"
TIER_2_PATTERN="TIER 2"
TIER_3_PATTERN="TIER 3"

# Tier section requirements (space-separated)
TIER_2_SECTIONS="ARTIFACTS: VERIFICATION:"
TIER_3_SECTIONS="ARTIFACTS: VERIFICATION: ADVERSARIAL:"

# PCV config from consolidated stop_verification section
if [ -f "$CONFIG" ] && command -v jq >/dev/null 2>&1; then
    REQUIRED_MARKER=$(jq -r '.stop_verification.pcv.required_marker // "🔍"' "$CONFIG" 2>/dev/null || echo "🔍")
    REQUIRED_TEXT=$(jq -r '.stop_verification.pcv.required_text // "verify-work"' "$CONFIG" 2>/dev/null || echo "verify-work")

    # Tier patterns
    CFG_TIER1=$(jq -r '.stop_verification.pcv.tier_patterns.tier_1 // empty' "$CONFIG" 2>/dev/null || echo "")
    CFG_TIER2=$(jq -r '.stop_verification.pcv.tier_patterns.tier_2 // empty' "$CONFIG" 2>/dev/null || echo "")
    CFG_TIER3=$(jq -r '.stop_verification.pcv.tier_patterns.tier_3 // empty' "$CONFIG" 2>/dev/null || echo "")
    [ -n "$CFG_TIER1" ] && TIER_1_PATTERN="$CFG_TIER1"
    [ -n "$CFG_TIER2" ] && TIER_2_PATTERN="$CFG_TIER2"
    [ -n "$CFG_TIER3" ] && TIER_3_PATTERN="$CFG_TIER3"

    # Tier required sections
    CFG_T2_SECTIONS=$(jq -r '.stop_verification.pcv.tier_requirements."2"[]?' "$CONFIG" 2>/dev/null | tr '\n' ' ' || echo "")
    CFG_T3_SECTIONS=$(jq -r '.stop_verification.pcv.tier_requirements."3"[]?' "$CONFIG" 2>/dev/null | tr '\n' ' ' || echo "")
    [ -n "$CFG_T2_SECTIONS" ] && TIER_2_SECTIONS="$CFG_T2_SECTIONS"
    [ -n "$CFG_T3_SECTIONS" ] && TIER_3_SECTIONS="$CFG_T3_SECTIONS"
fi

# =============================================================================
# TRANSCRIPT PARSING - CURRENT TURN
# =============================================================================

# Strategy: Find the last user message, then check all assistant messages after it.
# This ensures we only validate the CURRENT turn, not old markers from previous turns.

# Get line number of last user message
LAST_USER_LINE=$(grep -n '"type":"user"' "$TRANSCRIPT_PATH" 2>/dev/null | tail -1 | cut -d: -f1 || echo "")

if [ -z "$LAST_USER_LINE" ]; then
    # No user messages found - allow stop (edge case)
    exit 0
fi

# Extract assistant text content ONLY from lines after the last user message
CURRENT_TURN_TEXTS=$(tail -n +"$LAST_USER_LINE" "$TRANSCRIPT_PATH" 2>/dev/null | \
    jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "text") | .text // empty' 2>/dev/null || echo "")

# If no assistant text in current turn, treat as missing PCV
if [ -z "$CURRENT_TURN_TEXTS" ]; then
    CURRENT_TURN_TEXTS=""
fi

# =============================================================================
# PCV VALIDATION
# =============================================================================

MISSING_ELEMENTS=""

# Check 1: Required marker AND required text
HAS_MARKER=false
HAS_TEXT=false

if echo "$CURRENT_TURN_TEXTS" | grep -q "$REQUIRED_MARKER" 2>/dev/null; then
    HAS_MARKER=true
fi

if echo "$CURRENT_TURN_TEXTS" | grep -qi "$REQUIRED_TEXT" 2>/dev/null; then
    HAS_TEXT=true
fi

# Also check for explicit verification statements
if echo "$CURRENT_TURN_TEXTS" | grep -qi "verification complete" 2>/dev/null; then
    HAS_TEXT=true
fi
if echo "$CURRENT_TURN_TEXTS" | grep -qi "work verified" 2>/dev/null; then
    HAS_TEXT=true
fi

if [ "$HAS_MARKER" = "false" ] || [ "$HAS_TEXT" = "false" ]; then
    MISSING_ELEMENTS="verify-work marker"
fi

# Check 2: TIER indicator
TIER_LEVEL=""
if echo "$CURRENT_TURN_TEXTS" | grep -qi "$TIER_3_PATTERN" 2>/dev/null; then
    TIER_LEVEL="3"
elif echo "$CURRENT_TURN_TEXTS" | grep -qi "$TIER_2_PATTERN" 2>/dev/null; then
    TIER_LEVEL="2"
elif echo "$CURRENT_TURN_TEXTS" | grep -qi "$TIER_1_PATTERN" 2>/dev/null; then
    TIER_LEVEL="1"
else
    if [ -n "$MISSING_ELEMENTS" ]; then
        MISSING_ELEMENTS="${MISSING_ELEMENTS}, TIER indicator"
    else
        MISSING_ELEMENTS="TIER indicator"
    fi
fi

# Check 3: Tier 2 required sections
if [ "$TIER_LEVEL" = "2" ]; then
    for section in $TIER_2_SECTIONS; do
        if ! echo "$CURRENT_TURN_TEXTS" | grep -qi "$section" 2>/dev/null; then
            if [ -n "$MISSING_ELEMENTS" ]; then
                MISSING_ELEMENTS="${MISSING_ELEMENTS}, ${section} section"
            else
                MISSING_ELEMENTS="${section} section"
            fi
        fi
    done
fi

# Check 4: Tier 3 required sections
if [ "$TIER_LEVEL" = "3" ]; then
    for section in $TIER_3_SECTIONS; do
        if ! echo "$CURRENT_TURN_TEXTS" | grep -qi "$section" 2>/dev/null; then
            if [ -n "$MISSING_ELEMENTS" ]; then
                MISSING_ELEMENTS="${MISSING_ELEMENTS}, ${section} section"
            else
                MISSING_ELEMENTS="${section} section"
            fi
        fi
    done
fi

# =============================================================================
# SUCCESS - All checks passed
# =============================================================================

if [ -z "$MISSING_ELEMENTS" ]; then
    # Clean up retry state file on success
    rm -f "$STATE_FILE" 2>/dev/null || true
    exit 0
fi

# =============================================================================
# RETRY MECHANISM
# =============================================================================

# Ensure state directory exists
mkdir -p "$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID" 2>/dev/null || true

RETRY_COUNT=0
if [ -f "$STATE_FILE" ]; then
    RETRY_COUNT=$(cat "$STATE_FILE" 2>/dev/null || echo "0")
fi

# If max retries reached, allow stop to prevent deadlock
if [ "$RETRY_COUNT" -ge "$MAX_RETRIES" ]; then
    rm -f "$STATE_FILE" 2>/dev/null || true
    echo "verify-work: allowing stop after ${MAX_RETRIES} retries (deadlock prevention)" >&2
    exit 0
fi

# Increment retry count
echo $((RETRY_COUNT + 1)) > "$STATE_FILE"

# =============================================================================
# BLOCK OUTPUT
# =============================================================================

RETRY_NUM=$((RETRY_COUNT + 1))

BLOCK_INSTRUCTION="ACTION REQUIRED: INVOKE Skill('working-protocol', args='verify-work') and follow the procedure."

# Output block JSON to stdout (Claude Code reads this)
cat <<EOF
{"decision":"block","continue":true,"stopReason":"PCV incomplete: missing ${MISSING_ELEMENTS} (attempt ${RETRY_NUM}/${MAX_RETRIES})","reason":"⛔ BLOCKED: verify-work missing (attempt ${RETRY_NUM}/${MAX_RETRIES}). Missing: ${MISSING_ELEMENTS}. ${BLOCK_INSTRUCTION}"}
EOF

exit 0
