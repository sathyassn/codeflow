#!/usr/bin/env bash
# CodeFlow Shell Library: Common Utilities
# Location: .codeflow/scripts/shell-lib/common.sh

# Source guard to prevent multiple loads
[[ -n "${_CODEFLOW_COMMON_LOADED:-}" ]] && return 0
_CODEFLOW_COMMON_LOADED=1

set -euo pipefail

# Version
CODEFLOW_LIB_VERSION="1.0.0"
readonly CODEFLOW_LIB_VERSION
export CODEFLOW_LIB_VERSION

# ============================================================================
# PATH UTILITIES
# ============================================================================

# Find repository root (git or fallback to current dir)
find_repo_root() {
    git rev-parse --show-toplevel 2>/dev/null || pwd
}

# Get absolute path (works on macOS and Linux)
get_absolute_path() {
    local path="$1"
    if [[ -d "$path" ]]; then
        (cd "$path" && pwd)
    elif [[ -f "$path" ]]; then
        local dir
        dir=$(dirname "$path")
        local file
        file=$(basename "$path")
        echo "$(cd "$dir" && pwd)/$file"
    else
        echo "$path"
    fi
}

# Resolve path relative to repo root
resolve_repo_path() {
    local relative_path="$1"
    local repo_root
    repo_root=$(find_repo_root)
    echo "$repo_root/$relative_path"
}

# Check if path is within repo
is_within_repo() {
    local path="$1"
    local repo_root
    repo_root=$(find_repo_root)
    local abs_path
    abs_path=$(get_absolute_path "$path")
    [[ "$abs_path" == "$repo_root"* ]]
}

# ============================================================================
# STRING UTILITIES
# ============================================================================

# Trim whitespace from string
trim() {
    local str="$1"
    str="${str#"${str%%[![:space:]]*}"}"
    str="${str%"${str##*[![:space:]]}"}"
    echo "$str"
}

# Check if string contains substring
contains() {
    local string="$1"
    local substring="$2"
    [[ "$string" == *"$substring"* ]]
}

# Check if string starts with prefix
starts_with() {
    local string="$1"
    local prefix="$2"
    [[ "$string" == "$prefix"* ]]
}

# Check if string ends with suffix
ends_with() {
    local string="$1"
    local suffix="$2"
    [[ "$string" == *"$suffix" ]]
}

# Convert to lowercase
to_lower() {
    echo "$1" | tr '[:upper:]' '[:lower:]'
}

# Convert to uppercase
to_upper() {
    echo "$1" | tr '[:lower:]' '[:upper:]'
}

# ============================================================================
# FILE UTILITIES
# ============================================================================

# Ensure directory exists
ensure_dir() {
    local dir="$1"
    [[ -d "$dir" ]] || mkdir -p "$dir"
}

# Safe file read (returns empty if file doesn't exist)
safe_read() {
    local file="$1"
    [[ -f "$file" ]] && cat "$file" || echo ""
}

# Atomic write (write to temp, then move)
atomic_write() {
    local file="$1"
    local content="$2"
    local temp_file
    temp_file=$(mktemp)
    echo "$content" > "$temp_file"
    mv "$temp_file" "$file"
}

# Backup file with timestamp
backup_file() {
    local file="$1"
    if [[ -f "$file" ]]; then
        local timestamp
        timestamp=$(date +%Y%m%d_%H%M%S)
        cp "$file" "${file}.backup.${timestamp}"
    fi
}

# ============================================================================
# ENVIRONMENT UTILITIES
# ============================================================================

# Get environment variable with default
get_env() {
    local var_name="$1"
    local default="${2:-}"
    echo "${!var_name:-$default}"
}

# Check if command exists
command_exists() {
    command -v "$1" &>/dev/null
}

# ============================================================================
# ARRAY UTILITIES
# ============================================================================

# Check if array contains element
array_contains() {
    local element="$1"
    shift
    local arr=("$@")
    for item in "${arr[@]}"; do
        [[ "$item" == "$element" ]] && return 0
    done
    return 1
}

# Join array elements with delimiter
array_join() {
    local delimiter="$1"
    shift
    local arr=("$@")
    local result=""
    for item in "${arr[@]}"; do
        [[ -n "$result" ]] && result+="$delimiter"
        result+="$item"
    done
    echo "$result"
}

# ============================================================================
# AUTO-INITIALIZATION
# ============================================================================

# Set CODEFLOW_ROOT if not already set
export CODEFLOW_ROOT="${CODEFLOW_ROOT:-$(find_repo_root)}"
