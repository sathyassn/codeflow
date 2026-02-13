#!/usr/bin/env bash
# Purpose:   List management and core path detection for cf-protect-resources.sh
# Location:  .codeflow/scripts/security/protection/lib/cf-protection-core.sh
# Usage:     source lib/cf-protection-core.sh (from cf-protect-resources.sh)
# Version:   1.0.0
#
# This library provides list management functions for the protection tiers:
#   - CORE: Hardcoded security-critical paths (cannot be removed)
#   - EXTENDED: User-managed list in protected-extended.list
#   - AD-HOC: Dynamically tracked in protected-adhoc.list
#
# Requires: cf-protection-common.sh must be sourced first
#
# Module Coupling Note:
#   Functions add_to_list() and remove_from_list() call protect_single_file()
#   which is defined in cf-protection-ops.sh. This creates a runtime dependency:
#   - All modules must be sourced before calling add_to_list/remove_from_list
#   - Sourcing order: common → core → ops → verify (handled by orchestrator)
#   - This module cannot be used standalone without cf-protection-ops.sh
#
# Provides:
#   - read_list_file() - Read paths from a list file
#   - add_to_list() - Add path to a list file (requires cf-protection-ops.sh)
#   - remove_from_list() - Remove path from a list file (requires cf-protection-ops.sh)
#   - is_core_path() - Check if path is in core list
#   - get_all_paths() - Get all paths (core + extended + adhoc)

# Prevent direct execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "ERROR: This script must be sourced, not executed directly." >&2
    exit 1
fi

# Verify dependencies
if [[ -z "${PROJECT_ROOT:-}" ]]; then
    echo "ERROR: cf-protection-common.sh must be sourced before cf-protection-core.sh" >&2
    return 1
fi

# =============================================================================
# LIST MANAGEMENT
# =============================================================================

# Read paths from a list file (ignoring comments and empty lines)
read_list_file() {
    local list_file="$1"
    local full_path="$PROJECT_ROOT/$list_file"

    if [[ ! -f "$full_path" ]]; then
        return
    fi

    grep -v '^#' "$full_path" 2>/dev/null | grep -v '^[[:space:]]*$' || true
}

# Add path to a list file
add_to_list() {
    local list_file="$1"
    local path="$2"
    local full_path="$PROJECT_ROOT/$list_file"

    # Create file if doesn't exist
    if [[ ! -f "$full_path" ]]; then
        mkdir -p "$(dirname "$full_path")"
        echo "# Protection list - managed by cf-protect-resources.sh" > "$full_path"
    fi

    # Check if already in list
    if grep -qxF "$path" "$full_path" 2>/dev/null; then
        return 0  # Already exists
    fi

    # Remove immutable flag temporarily if set
    if [[ "$OS" == "macos" ]]; then
        chflags nouchg "$full_path" 2>/dev/null || true
    else
        chattr -i "$full_path" 2>/dev/null || true
    fi

    # Add to list
    echo "$path" >> "$full_path"

    # Re-protect the list file
    protect_single_file "$full_path"
}

# Remove path from a list file
remove_from_list() {
    local list_file="$1"
    local path="$2"
    local full_path="$PROJECT_ROOT/$list_file"

    if [[ ! -f "$full_path" ]]; then
        return 0
    fi

    # Remove immutable flag temporarily
    if [[ "$OS" == "macos" ]]; then
        chflags nouchg "$full_path" 2>/dev/null || true
    else
        chattr -i "$full_path" 2>/dev/null || true
    fi

    # Remove from list (create temp file to avoid issues)
    local temp_file
    temp_file=$(mktemp "${TMPDIR:-/tmp/claude}/cf-core-XXXXXX")
    grep -vxF "$path" "$full_path" > "$temp_file" 2>/dev/null || true
    mv "$temp_file" "$full_path"

    # Re-protect the list file
    protect_single_file "$full_path"
}

# Check if path is in core list
is_core_path() {
    local path="$1"
    for core in "${CORE_PATHS[@]}"; do
        if [[ "$path" == "$core" ]] || [[ "$path" == "$core/"* ]]; then
            return 0
        fi
    done
    return 1
}

# Get all paths (core + extended + adhoc)
get_all_paths() {
    # Core paths
    for path in "${CORE_PATHS[@]}"; do
        echo "$path"
    done

    # Extended paths
    read_list_file "$EXTENDED_LIST"

    # Ad-hoc paths
    read_list_file "$ADHOC_LIST"
}
