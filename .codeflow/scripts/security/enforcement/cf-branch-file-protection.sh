#!/usr/bin/env bash
# Purpose:   Branch-aware file write protection
# Usage:     source "enforcement/cf-branch-file-protection.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - File writes on protected branches (main, master)
#   - Blocks echo/printf redirects (>, >>)
#   - Blocks sed -i in-place edits
#   - Blocks touch/tee file creation to in-project paths
#   - Allows /tmp/claude/ operations (safe scratch space)
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security lib directory
#   - CONFIG: Path to enforcement-policy.json (optional)
#   - PROTECTED_PATHS: Array of protected paths (optional, for skip logic)
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_with_skill, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# BRANCH DETECTION
# =============================================================================

# Get current branch
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")

# Default protected branches
BRANCH_PROTECTED_BRANCHES=("main" "master" "production")

# Read protected branches from config if available
if [[ -n "${CONFIG:-}" ]] && [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    BRANCH_PROTECTED_BRANCHES=()
    while IFS= read -r branch; do
        [[ -n "$branch" ]] && BRANCH_PROTECTED_BRANCHES+=("$branch")
    done < <(jq -r '.protected_branches[]? // empty' "$CONFIG" 2>/dev/null)

    # Fallback to defaults if config didn't have branches
    if [[ ${#BRANCH_PROTECTED_BRANCHES[@]} -eq 0 ]]; then
        BRANCH_PROTECTED_BRANCHES=("main" "master" "production")
    fi
fi

# Check if on protected branch
is_on_protected_branch() {
    for protected in "${BRANCH_PROTECTED_BRANCHES[@]}"; do
        # Support wildcard patterns like release/*
        if [[ "$protected" == *"*"* ]]; then
            # Convert to regex pattern
            local pattern="${protected//\*/.*}"
            if [[ "$CURRENT_BRANCH" =~ ^$pattern$ ]]; then
                return 0
            fi
        elif [[ "$CURRENT_BRANCH" == "$protected" ]]; then
            return 0
        fi
    done
    return 1
}

# Skip if not on protected branch - this module only applies on main/master
if ! is_on_protected_branch; then
    return 0
fi

# =============================================================================
# SAFE PATH DETECTION
# =============================================================================

# Skip if targeting /tmp/claude/ (safe scratch space)
if [[ "$COMMAND" =~ /tmp/claude/ ]]; then
    return 0
fi

# Skip if targeting /tmp/ directly (also safe)
if [[ "$COMMAND" =~ [[:space:]]/tmp/[^c] ]] || [[ "$COMMAND" =~ [[:space:]]/tmp/$ ]]; then
    return 0
fi

# =============================================================================
# PROTECTED PATH SKIP LOGIC
# =============================================================================
# If the command targets a path already in PROTECTED_PATHS, let cf-path-protection.sh
# handle it (it has more specific error messages).

is_targeting_protected_path() {
    if [[ -z "${PROTECTED_PATHS[*]+x}" ]]; then
        return 1  # No protected paths defined
    fi

    for path in "${PROTECTED_PATHS[@]}"; do
        if [[ "$COMMAND" == *"$path"* ]]; then
            return 0  # Target is a protected path - skip this module
        fi
    done
    return 1
}

if is_targeting_protected_path; then
    return 0  # Let cf-path-protection.sh handle it
fi

# =============================================================================
# FILE WRITES ON PROTECTED BRANCHES
# =============================================================================
# Block file-writing Bash operations that bypass Edit/Write tool hooks

# -----------------------------------------------------------------------------
# Block redirect operations (>, >>)
# Pattern: detects > or >> NOT preceded by & (fd redirect) or digit (2>)
# This catches: echo x > file, printf x > file, cat > file
# Does NOT catch: 2>&1, >&2, 2>file (fd redirects are allowed)
# -----------------------------------------------------------------------------

# Match redirect to a file path (not fd redirect like 2>&1)
if [[ "$COMMAND" =~ ([^0-9\&\>]|^)\>[[:space:]]*[^\>\&[:space:]] ]]; then
    block_with_skill \
        "Branch Protection" \
        "File redirect on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
        ">" \
        "git-workflow" \
        "create-feature-branch"
fi

# Match append redirect (>>)
if [[ "$COMMAND" =~ ([^0-9\&]|^)\>\>[[:space:]]*[^\>\&[:space:]] ]]; then
    block_with_skill \
        "Branch Protection" \
        "File append on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
        ">>" \
        "git-workflow" \
        "create-feature-branch"
fi

# -----------------------------------------------------------------------------
# Block sed -i (in-place edit)
# This catches: sed -i '' 's/a/b/' file, sed -i.bak 's/a/b/' file
# -----------------------------------------------------------------------------

if [[ "$COMMAND" =~ sed[[:space:]].*-i ]]; then
    block_with_skill \
        "Branch Protection" \
        "In-place file edit on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
        "sed -i" \
        "git-workflow" \
        "create-feature-branch"
fi

# -----------------------------------------------------------------------------
# Block file creation commands targeting in-project paths
# Commands: touch, tee (without /tmp)
# -----------------------------------------------------------------------------

# Block touch for in-project files
# Pattern: touch followed by relative path (. or word char, not /)
if [[ "$COMMAND" =~ (^|[[:space:]])touch[[:space:]]+[^\-/] ]]; then
    # Additional check: not targeting absolute path that's safe
    if [[ ! "$COMMAND" =~ touch[[:space:]]+/tmp ]]; then
        block_with_skill \
            "Branch Protection" \
            "File creation on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
            "touch" \
            "git-workflow" \
            "create-feature-branch"
    fi
fi

# Block tee for in-project files (piped output to file)
# Pattern: | tee followed by relative path
if [[ "$COMMAND" =~ \|[[:space:]]*tee[[:space:]]+[^\-/] ]]; then
    if [[ ! "$COMMAND" =~ tee[[:space:]]+/tmp ]]; then
        block_with_skill \
            "Branch Protection" \
            "Piped file write on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
            "tee" \
            "git-workflow" \
            "create-feature-branch"
    fi
fi

# -----------------------------------------------------------------------------
# Block cp/mv to in-project paths on protected branches
# -----------------------------------------------------------------------------

# Block cp to relative paths (not /tmp)
if [[ "$COMMAND" =~ (^|[[:space:]])cp[[:space:]].*[[:space:]][^\-/][^[:space:]]* ]]; then
    # Exclude if destination is /tmp
    if [[ ! "$COMMAND" =~ cp[[:space:]].*[[:space:]]/tmp ]]; then
        block_with_skill \
            "Branch Protection" \
            "File copy on protected branch ($CURRENT_BRANCH). Create a feature branch first." \
            "cp" \
            "git-workflow" \
            "create-feature-branch"
    fi
fi

# All branch file protection checks passed
return 0
