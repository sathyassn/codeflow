#!/usr/bin/env bash
# Purpose:   PreToolUse hook for Read tool validation and sub-context delegation
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh
# Hook Type: PreToolUse
# Matcher:   Read
#
# This hook implements V3 spec for read delegation:
#   1. allowedPaths - paths that bypass line counting
#   2. alwaysBlockPatterns - patterns that always require delegation
#   3. thresholdRules - line count limits by file extension
#
# When a file exceeds threshold or matches block pattern:
#   - Returns permissionDecision: deny
#   - Provides delegation instructions to use Task tool
#
# Configuration:
#   - enforcement-policy.json: read_delegation section
#   - settings.json/_read_delegation_config: enabled, delegationTarget
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Read operation allowed (with optional delegation guidance)
#   - 2: Read blocked (always-block pattern matched)

set -euo pipefail

# =============================================================================
# EARLY EXIT FOR NON-READ TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Read" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SETTINGS_LOCAL="$REPO_ROOT/.claude/settings.local.json"
SETTINGS="$REPO_ROOT/.claude/settings.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# SETTINGS CONFIG
# =============================================================================

# Read _read_delegation_config from settings (local overrides project)
get_settings_config() {
    local key="$1"
    local default="$2"
    local value=""

    # Try settings.local.json first (local overrides)
    if [[ -f "$SETTINGS_LOCAL" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS_LOCAL" 2>/dev/null)
    fi

    # Fall back to settings.json
    if [[ -z "$value" ]] && [[ -f "$SETTINGS" ]] && command -v jq &>/dev/null; then
        value=$(jq -r "$key // empty" "$SETTINGS" 2>/dev/null)
    fi

    echo "${value:-$default}"
}

# Read delegation config from settings
DELEGATION_ENABLED=$(get_settings_config '._read_delegation_config.enabled' 'true')
DELEGATION_TARGET=$(get_settings_config '._read_delegation_config.delegationTarget' 'Explore')
ALWAYS_BLOCK_ENABLED=$(get_settings_config '._read_delegation_config.alwaysBlockEnabled' 'true')

# Early exit if delegation is disabled
if [[ "$DELEGATION_ENABLED" != "true" ]]; then
    exit 0
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
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# Normalize to relative path for pattern matching
REL_PATH="$FILE_PATH"
if [[ "$FILE_PATH" == "$REPO_ROOT"/* ]]; then
    REL_PATH="${FILE_PATH#"$REPO_ROOT"/}"
fi

# =============================================================================
# CONFIGURATION DEFAULTS
# =============================================================================

# Default allowed paths (bypass line counting)
declare -a ALLOWED_PATHS=(
    ".claude/CLAUDE.md"
    ".claude/settings.json"
    "README.md"
    "package.json"
    "Cargo.toml"
    "pyproject.toml"
)

# Default always-block patterns (require delegation regardless of size)
declare -a ALWAYS_BLOCK_PATTERNS=(
    ".claude/memory/**"
    ".codeflow/state/**"
    "*.jsonl"
    "*.db"
    "*.sqlite"
    "*.log"
)

# Default threshold rules by extension (lines)
declare -A THRESHOLD_RULES=(
    ["md"]=500
    ["txt"]=500
    ["ts"]=300
    ["tsx"]=300
    ["js"]=300
    ["jsx"]=300
    ["py"]=300
    ["sh"]=200
    ["json"]=200
    ["yaml"]=200
    ["yml"]=200
    ["toml"]=200
    ["default"]=400
)

# =============================================================================
# LOAD CONFIGURATION FROM enforcement-policy.json
# =============================================================================

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Load allowed paths
    ALLOWED_PATHS=()
    while IFS= read -r path; do
        [[ -n "$path" ]] && ALLOWED_PATHS+=("$path")
    done < <(jq -r '.read_delegation.allowed_paths[]? // empty' "$CONFIG" 2>/dev/null)
    # Fallback if empty
    [[ ${#ALLOWED_PATHS[@]} -eq 0 ]] && ALLOWED_PATHS=(".claude/CLAUDE.md" ".claude/settings.json" "README.md")

    # Load always-block patterns
    ALWAYS_BLOCK_PATTERNS=()
    while IFS= read -r pattern; do
        [[ -n "$pattern" ]] && ALWAYS_BLOCK_PATTERNS+=("$pattern")
    done < <(jq -r '.read_delegation.always_block_patterns[]? // empty' "$CONFIG" 2>/dev/null)
    # Fallback if empty
    [[ ${#ALWAYS_BLOCK_PATTERNS[@]} -eq 0 ]] && ALWAYS_BLOCK_PATTERNS=(".claude/memory/**" ".codeflow/state/**" "*.jsonl" "*.db")

    # Load threshold rules - requires associative array reconstruction
    # Note: jq output format is "ext:lines" pairs
    while IFS=: read -r ext lines; do
        if [[ -n "$ext" ]] && [[ "$lines" =~ ^[0-9]+$ ]]; then
            THRESHOLD_RULES["$ext"]="$lines"
        fi
    done < <(jq -r '.read_delegation.threshold_rules // {} | to_entries[] | "\(.key):\(.value)"' "$CONFIG" 2>/dev/null)
fi

# =============================================================================
# PATTERN MATCHING FUNCTIONS
# =============================================================================

# Convert glob pattern to regex
glob_to_regex() {
    local pattern="$1"
    # Escape special regex chars except * and ?
    local escaped="${pattern//./\\.}"
    escaped="${escaped//\[/\\[}"
    escaped="${escaped//\]/\\]}"
    # Convert ** to match anything including /
    escaped="${escaped//\*\*/.+}"
    # Convert single * to match anything except /
    escaped="${escaped//\*/[^/]*}"
    # Convert ? to match single char
    escaped="${escaped//\?/.}"
    echo "^${escaped}$"
}

# Check if path matches extension pattern (like *.jsonl)
matches_extension_pattern() {
    local path="$1"
    local pattern="$2"

    # If pattern starts with *, it's an extension match
    if [[ "$pattern" == \*.* ]]; then
        local ext="${pattern#\*.}"
        if [[ "$path" == *".$ext" ]]; then
            return 0
        fi
    fi
    return 1
}

# Check if path matches pattern list
matches_pattern_list() {
    local path="$1"
    shift
    local patterns=("$@")

    for pattern in "${patterns[@]}"; do
        # Direct match
        if [[ "$path" == "$pattern" ]]; then
            return 0
        fi
        # Extension pattern match (*.jsonl, *.db, etc.)
        if matches_extension_pattern "$path" "$pattern"; then
            return 0
        fi
        # Glob pattern match
        local regex
        regex=$(glob_to_regex "$pattern")
        if [[ "$path" =~ $regex ]]; then
            return 0
        fi
    done
    return 1
}

# =============================================================================
# CHECK 1: ALLOWED PATHS (bypass all checks)
# =============================================================================

if matches_pattern_list "$REL_PATH" "${ALLOWED_PATHS[@]}"; then
    # Log allowed path access
    if declare -f log_security_event &>/dev/null; then
        log_security_event "audit" "file_read_allowed" "Read" "$FILE_PATH" "Allowed path"
    fi
    exit 0
fi

# =============================================================================
# CHECK 2: ALWAYS-BLOCK PATTERNS (require delegation)
# =============================================================================

if [[ "$ALWAYS_BLOCK_ENABLED" == "true" ]]; then
    if matches_pattern_list "$REL_PATH" "${ALWAYS_BLOCK_PATTERNS[@]}"; then
        # Log blocked pattern
        if declare -f log_security_event &>/dev/null; then
            log_security_event "blocked" "file_read_blocked" "Read" "$FILE_PATH" "Always-block pattern"
        fi

        cat >&2 <<EOF
BLOCKED: File matches always-block pattern (requires delegation)

Path: $FILE_PATH
Pattern: Always-block pattern matched

permissionDecision: deny

This file type requires sub-context delegation for reading.
Large or structured files should be processed by specialized agents.

MUST: Task(subagent_type='$DELEGATION_TARGET', prompt='Read and summarize: $REL_PATH')

Delegation ensures efficient context usage and proper file processing.
EOF
        exit 2
    fi
fi

# =============================================================================
# CHECK 3: THRESHOLD RULES (line counting)
# =============================================================================

# Only check threshold if file exists and is readable
if [[ -f "$FILE_PATH" ]] && [[ -r "$FILE_PATH" ]]; then
    # Get file extension
    EXTENSION="${REL_PATH##*.}"
    EXTENSION="${EXTENSION,,}"  # lowercase

    # Get threshold for this extension (or default)
    THRESHOLD="${THRESHOLD_RULES[$EXTENSION]:-${THRESHOLD_RULES[default]:-400}}"

    # Count lines (fast with wc -l)
    LINE_COUNT=$(wc -l < "$FILE_PATH" 2>/dev/null || echo "0")
    LINE_COUNT="${LINE_COUNT// /}"  # trim whitespace

    # Check against threshold
    if [[ "$LINE_COUNT" =~ ^[0-9]+$ ]] && [[ "$LINE_COUNT" -gt "$THRESHOLD" ]]; then
        # Log threshold exceeded
        if declare -f log_security_event &>/dev/null; then
            log_security_event "warn" "file_read_threshold" "Read" "$FILE_PATH" "Lines: $LINE_COUNT > $THRESHOLD"
        fi

        cat >&2 <<EOF
WARNING: File exceeds line threshold (consider delegation)

Path: $FILE_PATH
Lines: $LINE_COUNT
Threshold: $THRESHOLD (for .$EXTENSION files)

permissionDecision: warn

Large files consume significant context. Consider using delegation:

SUGGEST: Task(subagent_type='$DELEGATION_TARGET', prompt='Read and summarize key sections of: $REL_PATH')

Alternatively, use offset/limit parameters to read specific sections:
  Read(file_path='$FILE_PATH', offset=1, limit=100)
EOF
        # Warning only - allow the read to proceed
    fi
fi

# =============================================================================
# NOTE: SENSITIVE FILES BLOCKED AT SETTINGS LEVEL
# =============================================================================
# Sensitive files (.env, *.pem, *.key, credentials, secrets) are BLOCKED
# in settings.json deny list. No hook-level check needed here.
# See: .claude/settings-templates/*.json -> permissions.deny section

# =============================================================================
# LOG READ OPERATION
# =============================================================================

if declare -f log_security_event &>/dev/null; then
    log_security_event "audit" "file_read" "Read" "$FILE_PATH" ""
fi

# =============================================================================
# READ ALLOWED
# =============================================================================

exit 0
