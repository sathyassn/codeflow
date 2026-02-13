#!/usr/bin/env bash
# Purpose:   Protected path security checks
# Usage:     source "enforcement/cf-path-protection.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - Protected path operations (delete, modify, permission changes)
#   - Redirect overwrites to protected files
#   - Git operations on protected paths
#   - .claude and .codeflow directory-level protection
#
# Variables (set by caller):
#   - COMMAND: The bash command being checked (required)
#   - LIB_DIR: Path to security lib directory (required)
#   - PROTECTED_PATHS: Array of protected paths (optional, defaults provided)
#   - DANGEROUS_CMDS: Regex pattern of dangerous commands (optional, defaults provided)
#   - PERMISSION_CMDS: Regex pattern of permission commands (optional, defaults provided)
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_with_skill, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# DEFAULT CONSTANTS (if not set by caller)
# =============================================================================

# Protected paths - loaded from enforcement-policy.json by the caller
# (cf-pre-tool-use-security.sh). These defaults are a fallback only.
if [[ -z "${PROTECTED_PATHS[*]+x}" ]]; then
    PROTECTED_PATHS=(
        ".claude/settings.json"
        ".claude/settings.local.json"
        ".claude/hooks/codeflow"
        ".claude/settings-templates"
        ".codeflow/config"
        ".codeflow/scripts/security"
        ".codeflow/scripts/git-hooks"
        ".github/workflows"
        ".git/hooks"
    )
fi

# Dangerous file commands (including cp which can overwrite)
DANGEROUS_CMDS="${DANGEROUS_CMDS:-rm|unlink|mv|cp|shred|truncate}"

# Permission commands
PERMISSION_CMDS="${PERMISSION_CMDS:-chmod|chown}"

# =============================================================================
# COMMAND SEGMENTATION HELPER
# =============================================================================
# Split compound commands into segments to prevent cross-contamination.
# e.g., "bash .claude/hooks/.../hook.sh && rm /tmp/file" becomes two segments.
# Each segment is checked independently so a protected path in one segment
# does not cause a false positive block for a dangerous cmd in another segment.

# Split a command string into segments by shell compound operators.
# Splits on: &&, ||, ;, | (outside of quotes)
# Results stored in the COMMAND_SEGMENTS array.
split_command_segments() {
    local cmd="$1"
    COMMAND_SEGMENTS=()

    # Simple split by &&, ||, ;, | — this handles the common cases.
    # We use a basic state machine to avoid splitting inside quotes.
    local segment=""
    local i=0
    local len=${#cmd}
    local in_single_quote=false
    local in_double_quote=false
    local ch next_ch

    while (( i < len )); do
        ch="${cmd:i:1}"
        next_ch="${cmd:i+1:1}"

        # Track quote state
        if [[ "$ch" == "'" ]] && [[ "$in_double_quote" == "false" ]]; then
            if [[ "$in_single_quote" == "true" ]]; then
                in_single_quote=false
            else
                in_single_quote=true
            fi
            segment+="$ch"
            i=$((i + 1))
            continue
        fi

        if [[ "$ch" == '"' ]] && [[ "$in_single_quote" == "false" ]]; then
            if [[ "$in_double_quote" == "true" ]]; then
                in_double_quote=false
            else
                in_double_quote=true
            fi
            segment+="$ch"
            i=$((i + 1))
            continue
        fi

        # Only split when outside quotes
        if [[ "$in_single_quote" == "false" ]] && [[ "$in_double_quote" == "false" ]]; then
            # Check for && or ||
            if [[ "$ch" == "&" ]] && [[ "$next_ch" == "&" ]]; then
                COMMAND_SEGMENTS+=("$segment")
                segment=""
                i=$((i + 2))
                continue
            fi
            if [[ "$ch" == "|" ]] && [[ "$next_ch" == "|" ]]; then
                COMMAND_SEGMENTS+=("$segment")
                segment=""
                i=$((i + 2))
                continue
            fi
            # Semicolon
            if [[ "$ch" == ";" ]]; then
                COMMAND_SEGMENTS+=("$segment")
                segment=""
                i=$((i + 1))
                continue
            fi
            # Single pipe (not ||)
            if [[ "$ch" == "|" ]] && [[ "$next_ch" != "|" ]]; then
                COMMAND_SEGMENTS+=("$segment")
                segment=""
                i=$((i + 1))
                continue
            fi
        fi

        segment+="$ch"
        i=$((i + 1))
    done

    # Add the last segment
    if [[ -n "$segment" ]]; then
        COMMAND_SEGMENTS+=("$segment")
    fi
}

# Check if a specific segment contains a dangerous command targeting a protected path.
# For cp: only block when protected path is the destination (last argument).
# Parameters:
#   $1 - segment: The command segment to check
#   $2 - path: The protected path
#   $3 - path_check_func: "exact" or "glob"
# Returns: 0 if segment should be blocked, 1 if safe
segment_has_dangerous_op() {
    local segment="$1"
    local path="$2"
    local path_check_func="$3"

    # Check if the protected path is in this segment
    local path_in_segment=false
    if [[ "$path_check_func" == "glob" ]]; then
        if is_glob_path_targeted "$segment" "$path"; then
            path_in_segment=true
        fi
    else
        if is_path_targeted "$segment" "$path"; then
            path_in_segment=true
        fi
    fi

    if [[ "$path_in_segment" == "false" ]]; then
        return 1
    fi

    # Path is in this segment — check for dangerous commands
    # cp special handling: only block if protected path is the destination
    if [[ "$segment" =~ (^|[[:space:]])(cp)[[:space:]] ]]; then
        if is_cp_destination "$segment" "$path" "$path_check_func"; then
            return 0
        fi
        # cp is present but protected path is source, not destination — allow
    fi

    # Other dangerous commands (rm, mv, unlink, shred, truncate)
    local other_dangerous="rm|unlink|mv|shred|truncate"
    if [[ "$segment" =~ ($other_dangerous)[[:space:]] ]]; then
        return 0
    fi

    # Permission commands (chmod, chown)
    if [[ "$segment" =~ ($PERMISSION_CMDS)[[:space:]] ]]; then
        return 0
    fi

    # Git rm
    if [[ "$segment" =~ git[[:space:]]+rm ]]; then
        return 0
    fi

    return 1
}

# Check if the protected path is the destination of a cp command.
# In cp, the last argument is the destination.
# Parameters:
#   $1 - segment: The command segment
#   $2 - path: The protected path
#   $3 - path_check_func: "exact" or "glob"
# Returns: 0 if path is cp destination, 1 if path is cp source
is_cp_destination() {
    local segment="$1"
    local path="$2"
    local path_check_func="$3"

    # Strip leading whitespace and extract the last non-flag argument
    # Strategy: extract the last whitespace-delimited token that doesn't start with -
    local trimmed
    trimmed="${segment#"${segment%%[![:space:]]*}"}"

    # Get the last argument (simple approach: last whitespace-separated token)
    local last_arg=""
    local token
    for token in $trimmed; do
        # Skip flags
        if [[ "$token" != -* ]]; then
            last_arg="$token"
        fi
    done

    # Remove surrounding quotes from last_arg
    last_arg="${last_arg#\"}"
    last_arg="${last_arg%\"}"
    last_arg="${last_arg#\'}"
    last_arg="${last_arg%\'}"

    # Check if the last argument matches the protected path
    if [[ "$path_check_func" == "glob" ]]; then
        local regex
        regex=$(glob_to_regex "$path")
        if [[ "$last_arg" =~ $regex ]]; then
            return 0
        fi
    else
        # Check if last_arg contains or equals the protected path
        if [[ "$last_arg" == *"$path"* ]]; then
            return 0
        fi
    fi

    return 1
}

# =============================================================================
# PROTECTED PATH OPERATIONS
# =============================================================================
# Block dangerous operations on protected paths.
# Uses command segmentation to avoid cross-contamination between compound commands.
# Segments are split once and reused across all sections.

split_command_segments "$COMMAND"

for path in "${PROTECTED_PATHS[@]}"; do
    # Determine path check type
    path_check_func="exact"
    if [[ "$path" == *"*"* ]] || [[ "$path" == *"?"* ]]; then
        path_check_func="glob"
    fi

    # Check each segment independently
    for segment in "${COMMAND_SEGMENTS[@]}"; do
        if segment_has_dangerous_op "$segment" "$path" "$path_check_func"; then
            # Determine the specific violation type for the log message
            if [[ "$segment" =~ ($PERMISSION_CMDS)[[:space:]] ]]; then
                log_protection "path_protected" "Bash" "$path" "critical" "blocked"
                block_with_skill \
                    "Protected Path Manipulation" \
                    "Permission change on protected path" \
                    "$path" \
                    "security-management" \
                    "handle-protected-resource"
            elif [[ "$segment" =~ git[[:space:]]+rm ]]; then
                log_protection "path_protected" "Bash" "$path" "critical" "blocked"
                block_with_skill \
                    "Protected Path Deletion" \
                    "Git removal of protected path" \
                    "$path" \
                    "security-management" \
                    "handle-protected-resource"
            else
                log_protection "path_protected" "Bash" "$path" "critical" "blocked"
                block_with_skill \
                    "Protected Path Deletion" \
                    "Dangerous operation on protected path" \
                    "$path" \
                    "security-management" \
                    "handle-protected-resource"
            fi
        fi
    done
done

unset path_check_func

# =============================================================================
# .CLAUDE AND .CODEFLOW DIRECTORY PROTECTION
# =============================================================================
# Block dangerous operations on .claude and .codeflow directories as a whole.
# Uses the same COMMAND_SEGMENTS from above (no re-split needed).

for segment in "${COMMAND_SEGMENTS[@]}"; do
    # Check for dangerous commands in this segment
    if [[ "$segment" =~ ($DANGEROUS_CMDS)[[:space:]] ]]; then
        # .claude directory protection
        # Pattern 1: .claude as final path component
        if [[ "$segment" =~ [[:space:]][^[:space:]]*\.claude($|[[:space:]]|[\'\"]) ]]; then
            log_protection "path_protected" "Bash" ".claude" "critical" "blocked"
            block_with_skill \
                "Protected Directory" \
                "Cannot delete .claude directory" \
                ".claude" \
                "security-management" \
                "handle-protected-resource"
        fi

        # Pattern 2: .claude/ with trailing slash at end
        if [[ "$segment" =~ [[:space:]][^[:space:]]*\.claude/($|[[:space:]]|[\'\"]) ]]; then
            log_protection "path_protected" "Bash" ".claude/" "critical" "blocked"
            block_with_skill \
                "Protected Directory" \
                "Cannot delete .claude directory" \
                ".claude/" \
                "security-management" \
                "handle-protected-resource"
        fi

        # .codeflow directory protection
        # Pattern 1: .codeflow as final path component
        if [[ "$segment" =~ [[:space:]][^[:space:]]*\.codeflow($|[[:space:]]|[\'\"]) ]]; then
            log_protection "path_protected" "Bash" ".codeflow" "critical" "blocked"
            block_with_skill \
                "Protected Directory" \
                "Cannot delete .codeflow directory" \
                ".codeflow" \
                "security-management" \
                "handle-protected-resource"
        fi

        # Pattern 2: .codeflow/ with trailing slash
        if [[ "$segment" =~ [[:space:]][^[:space:]]*\.codeflow/($|[[:space:]]|[\'\"]) ]]; then
            log_protection "path_protected" "Bash" ".codeflow/" "critical" "blocked"
            block_with_skill \
                "Protected Directory" \
                "Cannot delete .codeflow directory" \
                ".codeflow/" \
                "security-management" \
                "handle-protected-resource"
        fi
    fi
done

# =============================================================================
# REDIRECT OVERWRITE PROTECTION
# =============================================================================
# Block redirect overwrites to protected files
# MUST NOT block fd redirects (2>&1, >&2) or reading FROM protected paths
# Note: Redirects are inherently segment-local (they apply to the command
# immediately preceding them), so cross-contamination is less of an issue.
# We still check per-segment for consistency.
# Uses the same COMMAND_SEGMENTS from above (no re-split needed).

for path in "${PROTECTED_PATHS[@]}"; do
    for segment in "${COMMAND_SEGMENTS[@]}"; do
        # For glob patterns, use regex matching
        if [[ "$path" == *"*"* ]] || [[ "$path" == *"?"* ]]; then
            regex=$(glob_to_regex "$path")
            # Check for redirect to glob pattern path
            if [[ "$segment" =~ ([^0-9\&\>]|^)\>[[:space:]]*.*$regex ]]; then
                log_protection "path_protected" "Bash" "> $path" "critical" "blocked"
                block_with_skill \
                    "Protected File Overwrite" \
                    "Redirect overwrite of protected path" \
                    "> $path" \
                    "security-management" \
                    "handle-protected-resource"
            fi
            if [[ "$segment" =~ ([^0-9\&]|^)\>\>[[:space:]]*.*$regex ]]; then
                log_protection "path_protected" "Bash" ">> $path" "critical" "blocked"
                block_with_skill \
                    "Protected File Append" \
                    "Redirect append to protected path" \
                    ">> $path" \
                    "security-management" \
                    "handle-protected-resource"
            fi
        else
            # Single > redirect: cmd > protected_path
            # Exclude: 2>path (digit prefix), >&path (& prefix), >> (handled separately)
            if [[ "$segment" =~ ([^0-9\&\>]|^)\>[[:space:]]*"$path"($|[[:space:]\"\'/]) ]]; then
                log_protection "path_protected" "Bash" "> $path" "critical" "blocked"
                block_with_skill \
                    "Protected File Overwrite" \
                    "Redirect overwrite of protected path" \
                    "> $path" \
                    "security-management" \
                    "handle-protected-resource"
            fi
            # Double >> redirect: cmd >> protected_path (append)
            if [[ "$segment" =~ ([^0-9\&]|^)\>\>[[:space:]]*"$path"($|[[:space:]\"\'/]) ]]; then
                log_protection "path_protected" "Bash" ">> $path" "critical" "blocked"
                block_with_skill \
                    "Protected File Append" \
                    "Redirect append to protected path" \
                    ">> $path" \
                    "security-management" \
                    "handle-protected-resource"
            fi
        fi
    done
done

# All path protection checks passed
return 0
