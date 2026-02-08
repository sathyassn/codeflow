#!/usr/bin/env bash
# Purpose:   Main orchestrator for CodeFlow V3 resource protection
# Location:  .codeflow/scripts/security/protection/cf-protect-resources.sh
# Usage:     sudo ./cf-protect-resources.sh [command] [options]
# Version:   1.0.0
#
# Commands:
#   enable [path]      - Enable protection (all paths or specific path)
#   disable [path]     - Disable protection (all paths or specific path)
#   status [path]      - Show protection status
#   verify             - Test protection actually works
#   add <path>         - Add path to ad-hoc protection list
#   remove <path>      - Remove path from ad-hoc list (core paths cannot be removed)
#   list               - List all protected paths by tier
#
# Protection Tiers:
#   CORE      - Hardcoded security-critical paths (cannot be removed)
#   EXTENDED  - User-managed list in protected-extended.list
#   AD-HOC    - Dynamically tracked in protected-adhoc.list
#
# Protection Method:
#   - Root ownership with appropriate permissions
#   - Immutable flag (chflags uchg on macOS, chattr +i on Linux)
#
# Requirements:
#   - Must be run with sudo/root privileges
#   - macOS or Linux operating system
#
# Examples:
#   sudo ./cf-protect-resources.sh enable              # Protect all paths
#   sudo ./cf-protect-resources.sh status              # Show all statuses
#   sudo ./cf-protect-resources.sh add src/critical.sh # Add to ad-hoc list
#   sudo ./cf-protect-resources.sh verify              # Test protection works

set -euo pipefail

# =============================================================================
# INITIALIZATION
# =============================================================================

# Script location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Export for library scripts
export SCRIPT_DIR

# Source library files in order
source "$SCRIPT_DIR/lib/cf-protection-common.sh"
source "$SCRIPT_DIR/lib/cf-protection-core.sh"
source "$SCRIPT_DIR/lib/cf-protection-ops.sh"
source "$SCRIPT_DIR/lib/cf-protection-verify.sh"

# =============================================================================
# ROOT CHECK
# =============================================================================

check_root() {
    if [[ $EUID -ne 0 ]]; then
        log_error "This script must be run with sudo/root privileges"
        echo ""
        echo "Usage: sudo $0 [command] [options]"
        exit 1
    fi
}

# =============================================================================
# COMMAND IMPLEMENTATIONS
# =============================================================================

# Enable protection
cmd_enable() {
    local specific_path="${1:-}"

    echo ""
    echo "Enabling Protection"
    echo "==================="
    echo "OS: $OS | Project: $(basename "$PROJECT_ROOT")"
    echo ""

    if [[ -n "$specific_path" ]]; then
        if ! validate_path "$specific_path"; then
            return 1
        fi
        protect_path "$specific_path"
        # Track in ad-hoc list if not already tracked
        if ! is_core_path "$specific_path"; then
            add_to_list "$ADHOC_LIST" "$specific_path"
        fi
        return
    fi

    local extended_paths
    local adhoc_paths

    # Protect Core paths
    echo -e "${CYAN}Core:${NC}"
    for path in "${CORE_PATHS[@]}"; do
        protect_path "$path"
    done

    # Protect Extended paths
    echo ""
    echo -e "${CYAN}Extended:${NC}"
    extended_paths=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            protect_path "$path"
        done <<< "$extended_paths"
    fi

    # Protect Ad-hoc paths
    echo ""
    echo -e "${CYAN}Ad-hoc:${NC}"
    adhoc_paths=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            protect_path "$path"
        done <<< "$adhoc_paths"
    fi

    echo ""
    log_success "Protection enabled"
}

# Disable protection
cmd_disable() {
    local specific_path="${1:-}"

    echo ""
    echo "Disabling Protection"
    echo "===================="
    echo "OS: $OS | Project: $(basename "$PROJECT_ROOT")"
    echo ""

    if [[ -n "$specific_path" ]]; then
        if ! validate_path "$specific_path"; then
            return 1
        fi
        unprotect_path "$specific_path"
        return
    fi

    local extended_paths
    local adhoc_paths

    # Unprotect Core paths
    echo -e "${CYAN}Core:${NC}"
    for path in "${CORE_PATHS[@]}"; do
        unprotect_path "$path"
    done

    # Unprotect Extended paths
    echo ""
    echo -e "${CYAN}Extended:${NC}"
    extended_paths=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            unprotect_path "$path"
        done <<< "$extended_paths"
    fi

    # Unprotect Ad-hoc paths
    echo ""
    echo -e "${CYAN}Ad-hoc:${NC}"
    adhoc_paths=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            unprotect_path "$path"
        done <<< "$adhoc_paths"
    fi

    echo ""
    log_success "Protection disabled"
}

# Add path to protection
cmd_add() {
    local path="$1"

    if [[ -z "$path" ]]; then
        log_error "Usage: $0 add <path>"
        return 1
    fi

    if ! validate_path "$path"; then
        return 1
    fi

    # Check if already in core
    if is_core_path "$path"; then
        log_info "$path is already in core protection list"
        return 0
    fi

    # Add to ad-hoc list
    add_to_list "$ADHOC_LIST" "$path"

    # Apply protection
    protect_path "$path"

    log_success "Added and protected: $path"
}

# Remove path from protection
cmd_remove() {
    local path="$1"

    if [[ -z "$path" ]]; then
        log_error "Usage: $0 remove <path>"
        return 1
    fi

    # Cannot remove core paths
    if is_core_path "$path"; then
        log_error "Cannot remove core protected path: $path"
        return 1
    fi

    # Remove protection
    if [[ -e "$PROJECT_ROOT/$path" ]]; then
        unprotect_path "$path"
    fi

    # Remove from ad-hoc list
    remove_from_list "$ADHOC_LIST" "$path"

    log_success "Removed from protection: $path"
}

# List all protected paths
cmd_list() {
    local extended_paths
    local adhoc_paths

    echo ""
    echo "Protected Paths"
    echo "==============="
    echo ""

    echo -e "${CYAN}Core (hardcoded):${NC}"
    for path in "${CORE_PATHS[@]}"; do
        echo "  $path"
    done

    echo ""
    echo -e "${CYAN}Extended (user-managed):${NC}"
    extended_paths=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            echo "  $path"
        done <<< "$extended_paths"
    fi

    echo ""
    echo -e "${CYAN}Ad-hoc (session-tracked):${NC}"
    adhoc_paths=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            echo "  $path"
        done <<< "$adhoc_paths"
    fi

    echo ""
}

# =============================================================================
# USAGE
# =============================================================================

show_usage() {
    cat << 'EOF'
CodeFlow V3 Resource Protection

Usage: sudo ./cf-protect-resources.sh [command] [options]

Commands:
  enable [path]      Enable protection (all paths or specific path)
  disable [path]     Disable protection (all paths or specific path)
  status [path]      Show protection status
  verify             Test protection actually works
  add <path>         Add path to ad-hoc protection list
  remove <path>      Remove path from ad-hoc list (core paths cannot be removed)
  list               List all protected paths by tier
  help               Show this help message

Protection Tiers:
  CORE      Hardcoded security-critical paths (cannot be removed)
  EXTENDED  User-managed list in protected-extended.list
  AD-HOC    Dynamically tracked in protected-adhoc.list

Examples:
  sudo ./cf-protect-resources.sh enable              # Protect all paths
  sudo ./cf-protect-resources.sh disable             # Unprotect all paths
  sudo ./cf-protect-resources.sh status              # Show all statuses
  sudo ./cf-protect-resources.sh add src/critical.sh # Add to ad-hoc list
  sudo ./cf-protect-resources.sh verify              # Test protection works

Notes:
  - Must be run with sudo/root privileges
  - Core paths cannot be removed from protection
  - Ad-hoc paths are automatically tracked when added
  - Verification tests actual write protection
EOF
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    local command="${1:-help}"
    shift || true

    case "$command" in
        enable)
            check_root
            cmd_enable "$@"
            ;;
        disable)
            check_root
            cmd_disable "$@"
            ;;
        status)
            show_status "$@"
            ;;
        verify)
            check_root
            verify_protection
            ;;
        add)
            check_root
            cmd_add "$@"
            ;;
        remove)
            check_root
            cmd_remove "$@"
            ;;
        list)
            cmd_list
            ;;
        help|--help|-h)
            show_usage
            ;;
        *)
            log_error "Unknown command: $command"
            echo ""
            show_usage
            exit 1
            ;;
    esac
}

main "$@"
