#!/usr/bin/env bash
# Purpose:   PreToolUse hook for Edit/Write tool validation
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-edit-write.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write
#
# This hook:
#   - Validates Edit/Write targets are within project
#   - Blocks writes outside repository (except allowed tmp prefixes)
#   - Blocks writes to configured blocked directories
#   - Checks branch protection (no writes on main/master)
#   - Warns on dangerous file extensions
#   - Logs operations to audit trail
#
# Configuration: Reads from enforcement-policy.json
#   - edit_write.blocked_directories: Directories that cannot be written to
#   - edit_write.allowed_tmp_prefixes: Temp directories that are allowed
#   - edit_write.dangerous_extensions: Extensions to warn about
#   - protected_branches: Branches that block file writes
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed
#   - 2: Operation blocked

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
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# CONFIGURATION (Config-driven with defaults)
# =============================================================================

# Default values (used if config unavailable)
DEFAULT_BLOCKED_DIRS=(".git" "node_modules" "__pycache__" ".venv" "venv")
DEFAULT_ALLOWED_TMP=("/tmp/claude/" "/tmp/")
DEFAULT_BINARY_EXT=("exe" "dll" "so" "dylib" "bin" "o" "a")
DEFAULT_CREDENTIAL_EXT=("pem" "key" "crt" "p12" "pfx" "keystore" "jks")
DEFAULT_PROTECTED_BRANCHES=("main" "master")

# Arrays to be populated from config
BLOCKED_DIRS=()
ALLOWED_TMP_PREFIXES=()
BINARY_EXTENSIONS=()
CREDENTIAL_EXTENSIONS=()
PROTECTED_BRANCHES=()
WARN_ON_DANGEROUS="true"

# Load from config if available
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Load blocked directories
    while IFS= read -r dir; do
        [[ -n "$dir" ]] && BLOCKED_DIRS+=("$dir")
    done < <(jq -r '.edit_write.blocked_directories[]? // empty' "$CONFIG" 2>/dev/null)

    # Load allowed tmp prefixes
    while IFS= read -r prefix; do
        [[ -n "$prefix" ]] && ALLOWED_TMP_PREFIXES+=("$prefix")
    done < <(jq -r '.edit_write.allowed_tmp_prefixes[]? // empty' "$CONFIG" 2>/dev/null)

    # Load binary extensions
    while IFS= read -r ext; do
        [[ -n "$ext" ]] && BINARY_EXTENSIONS+=("$ext")
    done < <(jq -r '.edit_write.dangerous_extensions.binary[]? // empty' "$CONFIG" 2>/dev/null)

    # Load credential extensions
    while IFS= read -r ext; do
        [[ -n "$ext" ]] && CREDENTIAL_EXTENSIONS+=("$ext")
    done < <(jq -r '.edit_write.dangerous_extensions.credential[]? // empty' "$CONFIG" 2>/dev/null)

    # Load protected branches
    while IFS= read -r branch; do
        [[ -n "$branch" ]] && PROTECTED_BRANCHES+=("$branch")
    done < <(jq -r '.protected_branches[]? // empty' "$CONFIG" 2>/dev/null)

    # Load warn_on_dangerous setting
    WARN_ON_DANGEROUS=$(jq -r '.edit_write.warn_on_dangerous // "true"' "$CONFIG" 2>/dev/null)
fi

# Apply defaults if arrays are empty
[[ ${#BLOCKED_DIRS[@]} -eq 0 ]] && BLOCKED_DIRS=("${DEFAULT_BLOCKED_DIRS[@]}")
[[ ${#ALLOWED_TMP_PREFIXES[@]} -eq 0 ]] && ALLOWED_TMP_PREFIXES=("${DEFAULT_ALLOWED_TMP[@]}")
[[ ${#BINARY_EXTENSIONS[@]} -eq 0 ]] && BINARY_EXTENSIONS=("${DEFAULT_BINARY_EXT[@]}")
[[ ${#CREDENTIAL_EXTENSIONS[@]} -eq 0 ]] && CREDENTIAL_EXTENSIONS=("${DEFAULT_CREDENTIAL_EXT[@]}")
[[ ${#PROTECTED_BRANCHES[@]} -eq 0 ]] && PROTECTED_BRANCHES=("${DEFAULT_PROTECTED_BRANCHES[@]}")

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
    # Fallback: basic extraction
    FILE_PATH=$(echo "$TOOL_INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

block_write() {
    local reason="$1"

    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "edit_write_blocked" "$TOOL_NAME" "$FILE_PATH" "$reason"
    fi

    cat >&2 <<EOF
BLOCKED: $TOOL_NAME operation not allowed

Path: $FILE_PATH
Reason: $reason

This is a safety restriction enforced by CodeFlow.
EOF
    exit 2
}

# Check if path is in allowed tmp prefixes
is_allowed_tmp() {
    local path="$1"
    for prefix in "${ALLOWED_TMP_PREFIXES[@]}"; do
        if [[ "$path" == "$prefix"* ]]; then
            return 0
        fi
    done
    return 1
}

# Check if on protected branch
is_protected_branch() {
    local branch="$1"
    for protected in "${PROTECTED_BRANCHES[@]}"; do
        # Exact match
        if [[ "$branch" == "$protected" ]]; then
            return 0
        fi
        # Glob match (e.g., release/*)
        if [[ "$protected" == *"*"* ]]; then
            local pattern="${protected//\*/.*}"
            if [[ "$branch" =~ ^$pattern$ ]]; then
                return 0
            fi
        fi
    done
    return 1
}

# Check if path matches a blocked directory
is_blocked_directory() {
    local path="$1"
    for blocked in "${BLOCKED_DIRS[@]}"; do
        # Check various patterns: starts with, contains as path segment
        if [[ "$path" == "$blocked/"* ]] || [[ "$path" == *"/$blocked/"* ]] || [[ "$path" == *"/$blocked" ]]; then
            echo "$blocked"
            return 0
        fi
    done
    return 1
}

# Check if extension is in array
extension_in_array() {
    local ext="$1"
    shift
    local arr=("$@")
    for item in "${arr[@]}"; do
        if [[ "$ext" == "$item" ]]; then
            return 0
        fi
    done
    return 1
}

# =============================================================================
# BRANCH PROTECTION CHECK
# =============================================================================

# Get current branch
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"

# Block direct file writes on protected branches (except allowed tmp)
if [[ -n "$CURRENT_BRANCH" ]] && is_protected_branch "$CURRENT_BRANCH"; then
    if ! is_allowed_tmp "$FILE_PATH"; then
        block_write "Cannot write files directly on protected branch '$CURRENT_BRANCH'. Create a feature branch first."
    fi
fi

# =============================================================================
# PATH VALIDATION
# =============================================================================

# Resolve to absolute path
if [[ "$FILE_PATH" != /* ]]; then
    ABS_PATH="$REPO_ROOT/$FILE_PATH"
else
    ABS_PATH="$FILE_PATH"
fi

# Block writes outside repository (except allowed tmp prefixes)
if [[ "$ABS_PATH" != "$REPO_ROOT"/* ]] && ! is_allowed_tmp "$ABS_PATH"; then
    block_write "Write operations must be within the project directory or allowed temp directories"
fi

# =============================================================================
# BLOCKED DIRECTORY CHECK (Config-driven)
# =============================================================================

blocked_dir=""
if blocked_dir=$(is_blocked_directory "$FILE_PATH"); then
    block_write "Cannot write to '$blocked_dir' directory"
fi

# Also check absolute path
if blocked_dir=$(is_blocked_directory "$ABS_PATH"); then
    block_write "Cannot write to '$blocked_dir' directory"
fi

# =============================================================================
# DANGEROUS FILE EXTENSION CHECKS (Config-driven)
# =============================================================================

if [[ "$WARN_ON_DANGEROUS" == "true" ]]; then
    # Get file extension (lowercase for comparison)
    EXTENSION="${FILE_PATH##*.}"
    EXTENSION_LOWER=$(echo "$EXTENSION" | tr '[:upper:]' '[:lower:]')

    # Check binary extensions
    if extension_in_array "$EXTENSION_LOWER" "${BINARY_EXTENSIONS[@]}"; then
        echo "Warning: Writing binary file: $FILE_PATH" >&2
        if declare -f log_security_event &>/dev/null; then
            log_security_event "warn" "binary_file_write" "$TOOL_NAME" "$FILE_PATH" "Binary file extension"
        fi
    fi

    # Check credential extensions
    if extension_in_array "$EXTENSION_LOWER" "${CREDENTIAL_EXTENSIONS[@]}"; then
        echo "Warning: Writing credential file: $FILE_PATH" >&2
        if declare -f log_security_event &>/dev/null; then
            log_security_event "warn" "credential_file_write" "$TOOL_NAME" "$FILE_PATH" "Credential file extension"
        fi
    fi
fi

# =============================================================================
# OPERATION ALLOWED
# =============================================================================

# Log allowed operation (audit trail)
if declare -f log_security_event &>/dev/null; then
    log_security_event "audit" "edit_write_allowed" "$TOOL_NAME" "$FILE_PATH" ""
fi

exit 0
