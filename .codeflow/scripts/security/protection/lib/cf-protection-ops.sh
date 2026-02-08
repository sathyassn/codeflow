#!/usr/bin/env bash
# Purpose:   Protection and unprotection operations for cf-protect-resources.sh
# Location:  .codeflow/scripts/security/protection/lib/cf-protection-ops.sh
# Usage:     source lib/cf-protection-ops.sh (from cf-protect-resources.sh)
# Version:   1.0.0
#
# This library provides the core protection operations:
#   - protect_single_file() - Protect a single file
#   - protect_path() - Protect a file or directory
#   - unprotect_path() - Remove protection from a file or directory
#
# Protection method:
#   - Set ownership to root
#   - Set appropriate permissions (644 for files, 755 for dirs/executables)
#   - Set immutable flag (chflags uchg on macOS, chattr +i on Linux)
#
# Requires: cf-protection-common.sh must be sourced first
#
# Provides:
#   - protect_single_file()
#   - protect_path()
#   - unprotect_path()

# Prevent direct execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "ERROR: This script must be sourced, not executed directly." >&2
    exit 1
fi

# Verify dependencies
if [[ -z "${PROJECT_ROOT:-}" ]]; then
    echo "ERROR: cf-protection-common.sh must be sourced before cf-protection-ops.sh" >&2
    return 1
fi

# =============================================================================
# PROTECTION FUNCTIONS
# =============================================================================

# Protect a single file (not directory)
protect_single_file() {
    local file="$1"

    if [[ ! -f "$file" ]]; then
        return
    fi

    # Set ownership to root
    chown root:"$ROOT_GROUP" "$file"

    # Check if it's an executable script
    if [[ -x "$file" ]] || [[ "$file" == *.sh ]]; then
        chmod 755 "$file"  # rwxr-xr-x (executable)
    else
        chmod 644 "$file"  # rw-r--r-- (not executable)
    fi

    # Set immutable flag
    if [[ "$OS" == "macos" ]]; then
        chflags uchg "$file"
    else
        chattr +i "$file"
    fi
}

# Protect a path (file or directory)
protect_path() {
    local path="$1"
    local full_path="$PROJECT_ROOT/$path"

    if [[ ! -e "$full_path" ]]; then
        log_warn "Path does not exist, skipping: $path"
        return
    fi

    # Remove immutable flags first (if any) to allow changes
    if [[ "$OS" == "macos" ]]; then
        chflags -R nouchg "$full_path" 2>/dev/null || true
    else
        chattr -R -i "$full_path" 2>/dev/null || true
    fi

    # Set ownership to root
    chown -R root:"$ROOT_GROUP" "$full_path"

    if [[ -d "$full_path" ]]; then
        # Directory: rwxr-xr-x (755) - everyone can read/traverse
        chmod 755 "$full_path"

        # Process all files inside
        find "$full_path" -type f | while read -r file; do
            protect_single_file "$file"
        done

        # Subdirectories: same permissions
        find "$full_path" -type d -exec chmod 755 {} \;
    else
        # Single file
        protect_single_file "$full_path"
    fi

    log_audit "PROTECT" "$path"
    echo -e "  ${GREEN}✓${NC} $path"
}

# Unprotect a path (file or directory)
unprotect_path() {
    local path="$1"
    local full_path="$PROJECT_ROOT/$path"

    if [[ ! -e "$full_path" ]]; then
        log_warn "Path does not exist, skipping: $path"
        return
    fi

    # Remove immutable flags
    if [[ "$OS" == "macos" ]]; then
        chflags -R nouchg "$full_path" 2>/dev/null || true
    else
        chattr -R -i "$full_path" 2>/dev/null || true
    fi

    # Restore user ownership
    local original_user="${SUDO_USER:-$(logname 2>/dev/null || echo "$USER")}"
    chown -R "$original_user":"$ROOT_GROUP" "$full_path"

    log_audit "UNPROTECT" "$path"
    echo -e "  ${YELLOW}✓${NC} $path (owner: $original_user)"
}
