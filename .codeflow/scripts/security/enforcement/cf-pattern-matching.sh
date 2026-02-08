#!/usr/bin/env bash
# Purpose:   Pattern matching utilities for security enforcement
# Usage:     source "enforcement/cf-pattern-matching.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module provides:
#   - Advanced glob pattern utilities
#   - File path matching helpers
#   - Command argument extraction
#   - Pattern validation functions
#
# Required variables (set by caller):
#   - LIB_DIR: Path to security lib directory
#
# This module does NOT block commands directly.
# It provides utility functions for other modules.
#
# Exit codes:
#   - 0: Always returns 0 (utility module)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# ADVANCED GLOB PATTERN UTILITIES
# =============================================================================

# Match extended glob patterns including ** (recursive)
# Parameters:
#   $1 - path: The path to check
#   $2 - pattern: Extended glob pattern (supports **)
# Returns: 0 if matches, 1 if not
matches_extended_glob() {
    local path="$1"
    local pattern="$2"

    # Handle ** (recursive match)
    if [[ "$pattern" == *"**"* ]]; then
        # Build regex step by step to avoid escaping issues
        local regex="$pattern"
        # 1. Escape literal dots first (before adding regex dots)
        regex="${regex//./\\.}"
        # 2. Replace ** with placeholder, then with regex .*
        regex="${regex//\*\*/___DOUBLESTAR___}"
        # 3. Replace single * with [^/]* (non-recursive)
        regex="${regex//\*/[^/]*}"
        # 4. Replace placeholder with .* (matches any including /)
        regex="${regex//___DOUBLESTAR___/.*}"
        if [[ "$path" =~ ^$regex$ ]]; then
            return 0
        fi
    else
        # Use standard glob matching via is_glob_path_targeted
        # shellcheck disable=SC2053  # We intentionally want glob matching here
        if [[ "$path" == $pattern ]]; then
            return 0
        fi
    fi

    return 1
}

# Check if path matches any pattern in a list
# Parameters:
#   $1 - path: The path to check
#   $@ - patterns: List of patterns to match against (shift first arg)
# Returns: 0 if any pattern matches, 1 if none match
matches_any_pattern() {
    local path="$1"
    shift

    for pattern in "$@"; do
        if matches_extended_glob "$path" "$pattern"; then
            return 0
        fi
    done

    return 1
}

# =============================================================================
# COMMAND ARGUMENT EXTRACTION
# =============================================================================

# Extract file paths from a command
# Parameters:
#   $1 - cmd: The command to extract paths from
# Returns: Space-separated list of paths
extract_file_paths() {
    local cmd="$1"
    local paths=""

    # Remove quotes and extract potential paths
    # Match patterns that look like file paths
    local path_pattern='[./][a-zA-Z0-9_/.-]+'

    while [[ "$cmd" =~ ($path_pattern) ]]; do
        local match="${BASH_REMATCH[1]}"
        paths="$paths $match"
        cmd="${cmd/"$match"/}"
    done

    echo "$paths"
}

# Extract the target path from redirect operations
# Parameters:
#   $1 - cmd: The command with redirect
# Returns: The redirect target path or empty string
extract_redirect_target() {
    local cmd="$1"

    # Match > or >> followed by path
    if [[ "$cmd" =~ \>{1,2}[[:space:]]*([^[:space:]\>\|]+) ]]; then
        echo "${BASH_REMATCH[1]}"
        return 0
    fi

    echo ""
    return 1
}

# =============================================================================
# PATTERN VALIDATION
# =============================================================================

# Validate that a pattern is well-formed
# Parameters:
#   $1 - pattern: The pattern to validate
# Returns: 0 if valid, 1 if invalid
is_valid_pattern() {
    local pattern="$1"

    # Check for empty pattern
    if [[ -z "$pattern" ]]; then
        return 1
    fi

    # Check for unsafe patterns (would match too broadly)
    if [[ "$pattern" == "*" ]] || [[ "$pattern" == "**" ]] || [[ "$pattern" == "/*" ]]; then
        return 1
    fi

    # Check for unbalanced brackets
    local open_brackets="${pattern//[^\[]/}"
    local close_brackets="${pattern//[^\]]/}"
    if [[ ${#open_brackets} -ne ${#close_brackets} ]]; then
        return 1
    fi

    return 0
}

# Check if pattern is a security risk (too broad)
# Parameters:
#   $1 - pattern: The pattern to check
# Returns: 0 if risky, 1 if safe
is_risky_pattern() {
    local pattern="$1"

    # Patterns that could match system-wide
    local risky_patterns=(
        "/*"
        "/.*"
        "/**"
        "/tmp/*"
        "*"
        ".*"
    )

    for risky in "${risky_patterns[@]}"; do
        if [[ "$pattern" == "$risky" ]]; then
            return 0  # Is risky
        fi
    done

    return 1  # Not risky
}

# =============================================================================
# PATH NORMALIZATION
# =============================================================================

# Normalize a path (remove ./ and resolve ..)
# Parameters:
#   $1 - path: The path to normalize
# Returns: Normalized path
normalize_path() {
    local path="$1"

    # Remove leading ./
    path="${path#./}"

    # Remove trailing /
    path="${path%/}"

    # Handle .. components (basic)
    while [[ "$path" == *"/.."* ]]; do
        # Remove one directory level before /..
        # shellcheck disable=SC2001  # Complex regex requires sed
        path=$(echo "$path" | sed 's|/[^/]*/\.\./|/|g; s|/[^/]*/\.\.$||g')
    done

    # Clean up any double slashes
    while [[ "$path" == *//* ]]; do
        path="${path//\/\//\/}"
    done

    # Remove leading ../ if present
    while [[ "$path" == ../* ]]; do
        path="${path#../}"
    done

    echo "$path"
}

# Check if path is within a base directory
# Parameters:
#   $1 - path: The path to check
#   $2 - base: The base directory
# Returns: 0 if within, 1 if outside
is_path_within() {
    local path="$1"
    local base="$2"

    # Normalize both paths
    path=$(normalize_path "$path")
    base=$(normalize_path "$base")

    # Check if path starts with base
    if [[ "$path" == "$base"* ]]; then
        return 0
    fi

    return 1
}

# =============================================================================
# MODULE COMPLETE
# =============================================================================
# This module provides utility functions only, no blocking
return 0
