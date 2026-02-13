#!/usr/bin/env bash
# Purpose:   PreToolUse hook to guide agent to use LSP for symbol searches
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-grep-sentinel.sh
# Hook Type: PreToolUse
# Matcher:   Grep
#
# Grep produces text matches while LSP provides semantic accuracy.
# This hook suggests using the code-exploration skill for symbol searches.
#
# Configuration: Reads protected patterns from enforcement-policy.json config.
#                No hardcoded patterns - all config-driven.
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Command allowed (no sentinel required or sentinel valid)
#   - 2: Command blocked (sentinel required but missing/expired)

set -euo pipefail


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
# EARLY EXIT FOR NON-GREP TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Grep" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
SENTINEL_LIB="$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Source sentinel library if available
if [[ -f "$SENTINEL_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$SENTINEL_LIB"
else
    # Sentinel library not available, allow operation
    exit 0
fi

# =============================================================================
# PARSE INPUT
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract relevant fields
PATTERN=""
PATH_FILTER=""
GLOB_FILTER=""
TYPE_FILTER=""

if command -v jq &>/dev/null; then
    PATTERN=$(echo "$TOOL_INPUT" | jq -r '.pattern // empty' 2>/dev/null)
    PATH_FILTER=$(echo "$TOOL_INPUT" | jq -r '.path // empty' 2>/dev/null)
    GLOB_FILTER=$(echo "$TOOL_INPUT" | jq -r '.glob // empty' 2>/dev/null)
    TYPE_FILTER=$(echo "$TOOL_INPUT" | jq -r '.type // empty' 2>/dev/null)
else
    # Fallback: basic extraction without jq
    PATTERN=$(echo "$TOOL_INPUT" | grep -o '"pattern"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":\s*"\([^"]*\)"/\1/' || true)
    PATH_FILTER=$(echo "$TOOL_INPUT" | grep -o '"path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":\s*"\([^"]*\)"/\1/' || true)
    GLOB_FILTER=$(echo "$TOOL_INPUT" | grep -o '"glob"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":\s*"\([^"]*\)"/\1/' || true)
    TYPE_FILTER=$(echo "$TOOL_INPUT" | grep -o '"type"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":\s*"\([^"]*\)"/\1/' || true)
fi

# Build file context for matching
FILE_CONTEXT=""
if [[ -n "$TYPE_FILTER" ]]; then
    FILE_CONTEXT=".$TYPE_FILTER"
elif [[ -n "$PATH_FILTER" ]]; then
    FILE_CONTEXT="$PATH_FILTER"
elif [[ -n "$GLOB_FILTER" ]]; then
    FILE_CONTEXT="$GLOB_FILTER"
fi

[[ -z "$FILE_CONTEXT" ]] && exit 0
[[ -z "$PATTERN" ]] && exit 0

# =============================================================================
# SENTINEL CHECK
# =============================================================================

# Find required skill from config using sentinel_find_skill_for_grep()
# Returns skill:operation format if file and content patterns match
if declare -f sentinel_find_skill_for_grep &>/dev/null; then
    RESULT=$(sentinel_find_skill_for_grep "$FILE_CONTEXT" "$PATTERN" || true)

    [[ -z "$RESULT" ]] && exit 0

    # Parse result
    REQUIRED_SKILL="${RESULT%%:*}"
    OPERATION="${RESULT#*:}"

    # Check for valid sentinel
    if declare -f sentinel_validate &>/dev/null && ! sentinel_validate "$REQUIRED_SKILL" "$PATTERN"; then
        # Log the block event
        if declare -f log_security_event &>/dev/null; then
            log_security_event "blocked" "grep_sentinel_missing" "Grep" "$PATTERN" "No sentinel for $REQUIRED_SKILL"
        fi

        # Get guidance from config if available
        GUIDANCE=""
        if declare -f sentinel_get_operation_guidance &>/dev/null; then
            GUIDANCE=$(sentinel_get_operation_guidance "$REQUIRED_SKILL" "$OPERATION")
        fi

        cat >&2 << EOF
BLOCKED: Symbol search requires skill (LSP-first for accuracy)
Pattern: $PATTERN | Skill: $REQUIRED_SKILL

MUST: Skill('$REQUIRED_SKILL', args='...') FIRST
EOF

        if [[ -n "$GUIDANCE" ]]; then
            echo "" >&2
            echo "$GUIDANCE" >&2
        fi

        exit 2
    fi
fi

# =============================================================================
# ALL CHECKS PASSED
# =============================================================================

# Log allowed operation (audit trail)
if declare -f log_security_event &>/dev/null; then
    log_security_event "audit" "grep_sentinel_allowed" "Grep" "$PATTERN" ""
fi

exit 0
