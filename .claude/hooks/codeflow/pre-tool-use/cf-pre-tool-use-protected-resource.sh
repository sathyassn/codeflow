#!/usr/bin/env bash
# Purpose:   PreToolUse hook for protected resource checks on Edit/Write
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write
#
# This hook:
#   - Checks if Edit/Write targets protected resources
#   - Enforces tiered protection levels (critical, high, moderate)
#   - Requires cf-security-management skill for protected edits
#
# Protected resources (from enforcement-policy.json):
#   - Critical: settings.json, settings.local.json, CLAUDE.md
#   - High: hooks, codeflow config, security scripts
#   - Moderate: mission.md, tech-stack, workflows
#
# Configuration: Reads from enforcement-policy.json (protected_resources section)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Path allowed or not Edit/Write tool
#   - 2: Path blocked (protected resource, requires skill)

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
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# EARLY EXIT FOR NON-EDIT/WRITE TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Source pattern matching for matches_extended_glob()
if [[ -f "$ENFORCEMENT_DIR/cf-pattern-matching.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-pattern-matching.sh"
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract file path from tool input
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$TOOL_INPUT" | jq -r '.file_path // empty')
else
    FILE_PATH=$(echo "$TOOL_INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# Normalize to relative path if absolute
if [[ "$FILE_PATH" == "$REPO_ROOT"/* ]]; then
    FILE_PATH="${FILE_PATH#"$REPO_ROOT"/}"
fi

# =============================================================================
# STAGING AREA EXCEPTION (V3 spec: allow edits to staging area)
# =============================================================================

# Protected edits staging area is allowed
STAGING_AREA="/tmp/claude/managed/codeflow/protected-edits"

# Check if path is in staging area (absolute or relative)
if [[ "$FILE_PATH" == "$STAGING_AREA"/* ]] || [[ "$FILE_PATH" == /tmp/claude/managed/codeflow/protected-edits/* ]]; then
    # Log allowed staging area access
    if declare -f log_protection &>/dev/null; then
        log_protection "staging_area_access" "$TOOL_NAME" "$FILE_PATH" "staging" "allowed"
    fi
    exit 0
fi

# =============================================================================
# PROTECTED RESOURCE DEFINITIONS
# =============================================================================

# Default protected paths (used if config not available)
CRITICAL_PATHS=(
    ".claude/settings.json"
    ".claude/settings.local.json"
    ".claude/CLAUDE.md"
)

HIGH_PATHS=(
    ".claude/hooks/codeflow/*"
    ".codeflow/config/*"
    ".codeflow/scripts/security/*"
    ".codeflow/scripts/git-hooks/*"
    ".codeflow/scripts/shell-lib/*"
    ".github/workflows/*"
)

MODERATE_PATHS=(
    "project/mission.md"
    "project/tech-stack/*"
)

# Load from config if available
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    CRITICAL_PATHS=()
    while IFS= read -r path; do
        [[ -n "$path" ]] && CRITICAL_PATHS+=("$path")
    done < <(jq -r '.protected_resources.critical[]? // empty' "$CONFIG" 2>/dev/null)

    HIGH_PATHS=()
    while IFS= read -r path; do
        [[ -n "$path" ]] && HIGH_PATHS+=("$path")
    done < <(jq -r '.protected_resources.high[]? // empty' "$CONFIG" 2>/dev/null)

    MODERATE_PATHS=()
    while IFS= read -r path; do
        [[ -n "$path" ]] && MODERATE_PATHS+=("$path")
    done < <(jq -r '.protected_resources.moderate[]? // empty' "$CONFIG" 2>/dev/null)
fi

# =============================================================================
# PATTERN MATCHING FUNCTIONS
# =============================================================================

# Check if path matches a glob pattern (supports ** recursive globs)
matches_pattern() {
    local path="$1"
    local pattern="$2"

    # Use matches_extended_glob from cf-pattern-matching.sh if available
    if declare -f matches_extended_glob &>/dev/null; then
        matches_extended_glob "$path" "$pattern"
        return $?
    fi

    # Fallback: handle ** and * separately
    if [[ "$pattern" == *"**"* ]]; then
        local regex="$pattern"
        regex="${regex//./\\.}"
        regex="${regex//\*\*/___DOUBLESTAR___}"
        regex="${regex//\*/[^/]*}"
        regex="${regex//___DOUBLESTAR___/.*}"
        if [[ "$path" =~ ^$regex$ ]]; then
            return 0
        fi
    else
        # shellcheck disable=SC2053
        if [[ "$path" == $pattern ]]; then
            return 0
        fi
    fi
    return 1
}

# Check path against pattern list
check_tier() {
    local path="$1"
    shift
    local patterns=("$@")

    for pattern in "${patterns[@]}"; do
        if [[ "$path" == "$pattern" ]] || matches_pattern "$path" "$pattern"; then
            return 0
        fi
    done
    return 1
}

# =============================================================================
# PROTECTION CHECKS
# =============================================================================

# Check critical tier
if check_tier "$FILE_PATH" "${CRITICAL_PATHS[@]}"; then
    # Log protection event
    if declare -f log_protection &>/dev/null; then
        log_protection "protected_edit_blocked" "$TOOL_NAME" "$FILE_PATH" "critical" "blocked"
    fi

    cat >&2 <<EOF
BLOCKED: Critical resource protection

Path: $FILE_PATH
Tier: CRITICAL
Tool: $TOOL_NAME

This file is critically protected and cannot be modified directly.
Use the cf-security-management skill to request access.

MUST: Skill('cf-security-management', args='handle-protected-resource $FILE_PATH')

Critical files require review and explicit approval.
EOF
    exit 2
fi

# Check high tier
if check_tier "$FILE_PATH" "${HIGH_PATHS[@]}"; then
    # Log protection event
    if declare -f log_protection &>/dev/null; then
        log_protection "protected_edit_blocked" "$TOOL_NAME" "$FILE_PATH" "high" "blocked"
    fi

    cat >&2 <<EOF
BLOCKED: High protection resource

Path: $FILE_PATH
Tier: HIGH
Tool: $TOOL_NAME

This file has high protection level.
Use the cf-security-management skill to request access.

MUST: Skill('cf-security-management', args='handle-protected-resource $FILE_PATH')
EOF
    exit 2
fi

# Check moderate tier - warn but allow
if check_tier "$FILE_PATH" "${MODERATE_PATHS[@]}"; then
    # Log protection event but allow
    if declare -f log_protection &>/dev/null; then
        log_protection "protected_edit_warning" "$TOOL_NAME" "$FILE_PATH" "moderate" "allowed"
    fi

    echo "Note: Editing moderately protected file: $FILE_PATH" >&2
fi

# =============================================================================
# PATH ALLOWED
# =============================================================================

exit 0
