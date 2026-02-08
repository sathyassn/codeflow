#!/usr/bin/env bash
# Purpose:   Protected path security checks
# Usage:     source "enforcement/cf-path-protection.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - Protected path operations (delete, modify, permission changes)
#   - Redirect overwrites to protected files
#   - Git operations on protected paths
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security lib directory
#   - PROTECTED_PATHS: Array of protected paths (optional, uses defaults)
#   - DANGEROUS_CMDS: Regex pattern of dangerous commands (optional)
#   - PERMISSION_CMDS: Regex pattern of permission commands (optional)
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_command, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# DEFAULT CONSTANTS (if not set by caller)
# =============================================================================

# Protected paths - critical files that should not be modified
# Note: .claude/CLAUDE.md is NOT protected - Claude should be able to edit it
if [[ -z "${PROTECTED_PATHS[*]+x}" ]]; then
    PROTECTED_PATHS=(
        ".claude/settings.json"
        ".claude/settings.local.json"
        ".claude/hooks/codeflow"
        ".claude/settings-templates"
        ".codeflow/config"
        ".codeflow/scripts/security"
        ".github/workflows"
        ".git/hooks"
    )
fi

# Dangerous file commands (including cp which can overwrite)
DANGEROUS_CMDS="${DANGEROUS_CMDS:-rm|unlink|mv|cp|shred|truncate}"

# Permission commands
PERMISSION_CMDS="${PERMISSION_CMDS:-chmod|chown}"

# =============================================================================
# PROTECTED PATH OPERATIONS
# =============================================================================
# Block dangerous operations on protected paths

for path in "${PROTECTED_PATHS[@]}"; do
    # Determine if path contains glob characters
    if [[ "$path" == *"*"* ]] || [[ "$path" == *"?"* ]]; then
        # Use glob matching
        path_matched=false
        if is_glob_path_targeted "$COMMAND" "$path"; then
            path_matched=true
        fi
    else
        # Use standard path matching
        path_matched=false
        if is_path_targeted "$COMMAND" "$path"; then
            path_matched=true
        fi
    fi

    if [[ "$path_matched" == "true" ]]; then
        # Dangerous commands (rm, mv, etc.)
        if [[ "$COMMAND" =~ ($DANGEROUS_CMDS)[[:space:]] ]]; then
            log_protection "path_protected" "Bash" "$path" "critical" "blocked"
            block_command \
                "Protected Path Deletion" \
                "Dangerous operation on protected path" \
                "$path"
        fi

        # Permission commands (chmod, chown)
        if [[ "$COMMAND" =~ ($PERMISSION_CMDS)[[:space:]] ]]; then
            log_protection "path_protected" "Bash" "$path" "critical" "blocked"
            block_command \
                "Protected Path Manipulation" \
                "Permission change on protected path" \
                "$path"
        fi

        # Git rm
        if [[ "$COMMAND" =~ git[[:space:]]+rm ]]; then
            log_protection "path_protected" "Bash" "$path" "critical" "blocked"
            block_command \
                "Protected Path Deletion" \
                "Git removal of protected path" \
                "$path"
        fi
    fi
done

unset path_matched

# =============================================================================
# .CLAUDE DIRECTORY PROTECTION
# =============================================================================
# Block dangerous operations on .claude directory as a whole

if [[ "$COMMAND" =~ ($DANGEROUS_CMDS)[[:space:]] ]]; then
    # Pattern 1: .claude as final path component
    if [[ "$COMMAND" =~ [[:space:]][^[:space:]]*\.claude($|[[:space:]]|[\'\"]) ]]; then
        block_command \
            "Protected Directory" \
            "Cannot delete .claude directory" \
            ".claude"
    fi

    # Pattern 2: .claude/ with trailing slash at end
    if [[ "$COMMAND" =~ [[:space:]][^[:space:]]*\.claude/($|[[:space:]]|[\'\"]) ]]; then
        block_command \
            "Protected Directory" \
            "Cannot delete .claude directory" \
            ".claude/"
    fi
fi

# =============================================================================
# .CODEFLOW DIRECTORY PROTECTION
# =============================================================================
# Block dangerous operations on .codeflow directory

if [[ "$COMMAND" =~ ($DANGEROUS_CMDS)[[:space:]] ]]; then
    # Pattern 1: .codeflow as final path component
    if [[ "$COMMAND" =~ [[:space:]][^[:space:]]*\.codeflow($|[[:space:]]|[\'\"]) ]]; then
        block_command \
            "Protected Directory" \
            "Cannot delete .codeflow directory" \
            ".codeflow"
    fi

    # Pattern 2: .codeflow/ with trailing slash
    if [[ "$COMMAND" =~ [[:space:]][^[:space:]]*\.codeflow/($|[[:space:]]|[\'\"]) ]]; then
        block_command \
            "Protected Directory" \
            "Cannot delete .codeflow directory" \
            ".codeflow/"
    fi
fi

# =============================================================================
# REDIRECT OVERWRITE PROTECTION
# =============================================================================
# Block redirect overwrites to protected files
# MUST NOT block fd redirects (2>&1, >&2) or reading FROM protected paths

for path in "${PROTECTED_PATHS[@]}"; do
    # For glob patterns, use regex matching
    if [[ "$path" == *"*"* ]] || [[ "$path" == *"?"* ]]; then
        regex=$(glob_to_regex "$path")
        # Check for redirect to glob pattern path
        if [[ "$COMMAND" =~ ([^0-9\&\>]|^)\>[[:space:]]*.*$regex ]]; then
            block_command \
                "Protected File Overwrite" \
                "Redirect overwrite of protected path" \
                "> $path"
        fi
        if [[ "$COMMAND" =~ ([^0-9\&]|^)\>\>[[:space:]]*.*$regex ]]; then
            block_command \
                "Protected File Append" \
                "Redirect append to protected path" \
                ">> $path"
        fi
    else
        # Single > redirect: cmd > protected_path
        # Exclude: 2>path (digit prefix), >&path (& prefix), >> (handled separately)
        if [[ "$COMMAND" =~ ([^0-9\&\>]|^)\>[[:space:]]*"$path"($|[[:space:]\"\'/]) ]]; then
            block_command \
                "Protected File Overwrite" \
                "Redirect overwrite of protected path" \
                "> $path"
        fi
        # Double >> redirect: cmd >> protected_path (append)
        if [[ "$COMMAND" =~ ([^0-9\&]|^)\>\>[[:space:]]*"$path"($|[[:space:]\"\'/]) ]]; then
            block_command \
                "Protected File Append" \
                "Redirect append to protected path" \
                ">> $path"
        fi
    fi
done

# All path protection checks passed
return 0
