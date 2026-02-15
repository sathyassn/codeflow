#!/usr/bin/env bash
# Purpose:   Branch-aware file write protection
# Usage:     source "enforcement/cf-branch-file-protection.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - File writes on protected branches (main, master, production, release/*)
#   - Blocks echo/printf redirects (>, >>)
#   - Blocks sed -i in-place edits
#   - Blocks touch/tee file creation to in-project paths
#   - Blocks cp to in-project paths
#   - Allows /tmp/claude/ operations (safe scratch space)
#   - Allows operations already covered by path-protection (no double-block)
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
# handle it (it has more specific error messages and skill references).
# This prevents double-blocking and ensures appropriate skill guidance.

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
# CONFIG-DRIVEN ENABLE/DISABLE
# =============================================================================
# Check if branch_protection is explicitly disabled via config

if [[ -n "${CONFIG:-}" ]] && [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    if jq -e '.branch_protection' "$CONFIG" >/dev/null 2>&1; then
        BRANCH_PROTECTION_ENABLED=$(jq -r '.branch_protection.enabled // true' "$CONFIG" 2>/dev/null)
        if [[ "$BRANCH_PROTECTION_ENABLED" == "false" ]]; then
            return 0  # Branch protection disabled via config
        fi
    fi
fi

# =============================================================================
# PATHFLOW-CONDITIONAL BLOCK HELPER
# =============================================================================
# In PathFlow mode (agent-teams), instruct to message cf-git-operations teammate.
# In standalone mode, instruct to invoke cf-git-workflow skill.

_block_branch_protection() {
    local reason="$1"
    local pattern="$2"

    if is_pathflow_active; then
        block_with_skill \
            "Branch Protection" \
            "$reason Message your cf-git-operations teammate to create a branch." \
            "$pattern" \
            "git-workflow" \
            "create-branch"
    else
        block_with_skill \
            "Branch Protection" \
            "$reason Create a feature branch first." \
            "$pattern" \
            "git-workflow" \
            "create-branch"
    fi
}

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
# Pattern explanation:
# - ([^0-9\&\>]|^) = not preceded by digit, &, or > (or start of string)
# - \>[[:space:]]* = > followed by optional spaces
# - [^\>\&[:space:]] = followed by something that's not >, &, or space (i.e., a path)
if [[ "$COMMAND" =~ ([^0-9\&\>]|^)\>[[:space:]]*[^\>\&[:space:]] ]]; then
    _block_branch_protection \
        "File redirect on protected branch ($CURRENT_BRANCH)." \
        ">"
fi

# Match append redirect (>>)
if [[ "$COMMAND" =~ ([^0-9\&]|^)\>\>[[:space:]]*[^\>\&[:space:]] ]]; then
    _block_branch_protection \
        "File append on protected branch ($CURRENT_BRANCH)." \
        ">>"
fi

# -----------------------------------------------------------------------------
# Block sed -i (in-place edit)
# This catches: sed -i '' 's/a/b/' file, sed -i.bak 's/a/b/' file
# -----------------------------------------------------------------------------

if [[ "$COMMAND" =~ sed[[:space:]].*-i ]]; then
    _block_branch_protection \
        "In-place file edit on protected branch ($CURRENT_BRANCH)." \
        "sed -i"
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
        _block_branch_protection \
            "File creation on protected branch ($CURRENT_BRANCH)." \
            "touch"
    fi
fi

# Block tee for in-project files (piped output to file)
# Pattern: | tee followed by relative path
if [[ "$COMMAND" =~ \|[[:space:]]*tee[[:space:]]+[^\-/] ]]; then
    if [[ ! "$COMMAND" =~ tee[[:space:]]+/tmp ]]; then
        _block_branch_protection \
            "Piped file write on protected branch ($CURRENT_BRANCH)." \
            "tee"
    fi
fi

# -----------------------------------------------------------------------------
# Block cp to in-project paths on protected branches
# Note: mv is NOT blocked here (mv is primarily a rename/move, not a write)
# -----------------------------------------------------------------------------

# Block cp when last argument (destination) is a relative path (not /tmp or /)
# Extract the last word of cp command to check destination
if [[ "$COMMAND" =~ (^|[[:space:]])cp[[:space:]] ]]; then
    # Get last space-delimited token as the destination
    _cp_dest="${COMMAND##* }"
    # Block if destination is relative (doesn't start with /) or starts with .
    if [[ "$_cp_dest" != /* ]] && [[ -n "$_cp_dest" ]]; then
        _block_branch_protection \
            "File copy on protected branch ($CURRENT_BRANCH)." \
            "cp"
    fi
fi

# All branch file protection checks passed
return 0
