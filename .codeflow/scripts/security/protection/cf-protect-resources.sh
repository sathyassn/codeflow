#!/usr/bin/env bash
# Purpose:   OS-level protection for CodeFlow security-critical files and directories
# Location:  .codeflow/scripts/security/protection/cf-protect-resources.sh
# Usage:     sudo ./cf-protect-resources.sh [command] [options]
# Version:   2.0.0
#
# Changelog:
#   2.0.0 - Aligned commands with workflow repo (protect/unprotect)
#         - Added protect core sub-mode, extend command
#         - Added confirmation prompts for destructive unprotect operations
#         - Added path count, re-protect reminder, extended list check
#   1.0.0 - Initial modular release with protection tiers
#
# Purpose:
#   Provides kernel-level protection against AI agent file manipulation.
#   Claude Code runs as the same user who launched it, so if files are
#   owned by root with restrictive permissions, the agent cannot modify them.
#
# Protection Tiers:
#   CORE     - Hardcoded security-critical paths (cannot be removed from list)
#   EXTENDED - User-managed list in protected-extended.list
#   AD-HOC   - Dynamically tracked in protected-adhoc.list
#
# Usage:
#   sudo ./cf-protect-resources.sh protect all
#   sudo ./cf-protect-resources.sh protect core
#   sudo ./cf-protect-resources.sh protect .claude/hooks/codeflow
#   sudo ./cf-protect-resources.sh unprotect .claude/hooks/codeflow/script.sh
#   sudo ./cf-protect-resources.sh status
#   sudo ./cf-protect-resources.sh verify
#   ./cf-protect-resources.sh list
#   sudo ./cf-protect-resources.sh extend add path/to/protect

set -euo pipefail

# =============================================================================
# INITIALIZATION
# =============================================================================

# Script location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"

# Export for library scripts
export SCRIPT_DIR

# Source library files in order
source "$SCRIPT_DIR/lib/cf-protection-common.sh"
source "$SCRIPT_DIR/lib/cf-protection-core.sh"
source "$SCRIPT_DIR/lib/cf-protection-ops.sh"
source "$SCRIPT_DIR/lib/cf-protection-verify.sh"

# =============================================================================
# COMMAND IMPLEMENTATIONS
# =============================================================================

# Protect paths
cmd_protect() {
    local target="${1:-}"
    local extended
    local adhoc
    local count=0

    if [[ -z "$target" ]]; then
        log_error "Missing target. Usage: $SCRIPT_NAME protect <all|core|path>"
        echo ""
        echo "  all    - Protect all paths (core + extended + adhoc)"
        echo "  core   - Protect only core paths"
        echo "  <path> - Protect specific path"
        exit 1
    fi

    case "$target" in
        all)
            echo ""
            echo "Protecting All Paths"
            echo "===================="
            echo ""
            echo -e "${CYAN}Core:${NC}"
            for path in "${CORE_PATHS[@]}"; do
                protect_path "$path"
                ((count++)) || true
            done

            echo ""
            echo -e "${CYAN}Extended:${NC}"
            extended=$(read_list_file "$EXTENDED_LIST")
            if [[ -n "$extended" ]]; then
                while IFS= read -r path; do
                    protect_path "$path"
                    ((count++)) || true
                done <<< "$extended"
            else
                echo "  (none)"
            fi

            echo ""
            echo -e "${CYAN}Ad-hoc:${NC}"
            adhoc=$(read_list_file "$ADHOC_LIST")
            if [[ -n "$adhoc" ]]; then
                while IFS= read -r path; do
                    protect_path "$path"
                    ((count++)) || true
                done <<< "$adhoc"
            else
                echo "  (none)"
            fi

            echo ""
            log_success "Protected $count paths"
            ;;
        core)
            echo ""
            echo "Protecting Core Paths"
            echo "====================="
            echo ""
            for path in "${CORE_PATHS[@]}"; do
                protect_path "$path"
                ((count++)) || true
            done
            echo ""
            log_success "Protected $count core paths"
            ;;
        *)
            # Specific path
            if ! validate_path "$target"; then
                exit 1
            fi

            echo ""
            protect_path "$target"

            # If not in any list, add to adhoc
            if ! is_core_path "$target"; then
                local in_extended
                in_extended=$(read_list_file "$EXTENDED_LIST" | grep -xF "$target" || true)
                if [[ -z "$in_extended" ]]; then
                    add_to_list "$ADHOC_LIST" "$target"
                    log_info "Added to ad-hoc list"
                fi
            fi

            echo ""
            log_success "Protection complete"
            ;;
    esac
}

# Remove protection
cmd_unprotect() {
    local target="${1:-}"
    local confirm

    if [[ -z "$target" ]]; then
        log_error "Missing target. Usage: $SCRIPT_NAME unprotect <all|core|path>"
        exit 1
    fi

    case "$target" in
        all)
            echo ""
            log_warn "Unprotecting ALL paths including core security files."
            read -rp "Type 'yes' to confirm: " confirm
            if [[ "$confirm" != "yes" ]]; then
                log_info "Aborted"
                exit 0
            fi

            echo ""
            echo "Unprotecting All Paths"
            echo "======================"
            echo ""
            for path in $(get_all_paths | sort -u); do
                unprotect_path "$path"
            done
            ;;
        core)
            echo ""
            log_warn "Unprotecting CORE security files."
            read -rp "Type 'yes' to confirm: " confirm
            if [[ "$confirm" != "yes" ]]; then
                log_info "Aborted"
                exit 0
            fi

            echo ""
            echo "Unprotecting Core Paths"
            echo "======================="
            echo ""
            for path in "${CORE_PATHS[@]}"; do
                unprotect_path "$path"
            done
            ;;
        *)
            # Specific path
            if ! validate_path "$target"; then
                exit 1
            fi

            # Warn if core path
            if is_core_path "$target"; then
                log_warn "This is a core security path."
                read -rp "Continue? [y/N] " confirm
                if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
                    log_info "Aborted"
                    exit 0
                fi
            fi

            echo ""
            unprotect_path "$target"

            # If in adhoc list, optionally remove
            local in_adhoc
            local remove_confirm
            in_adhoc=$(read_list_file "$ADHOC_LIST" | grep -xF "$target" || true)
            if [[ -n "$in_adhoc" ]]; then
                read -rp "Remove from ad-hoc list? [y/N] " remove_confirm
                if [[ "$remove_confirm" == "y" || "$remove_confirm" == "Y" ]]; then
                    remove_from_list "$ADHOC_LIST" "$target"
                    log_info "Removed from ad-hoc list"
                fi
            fi
            ;;
    esac

    echo ""
    log_warn "Re-protect after changes: sudo $0 protect all"
}

# Show all protection lists (top-level command)
cmd_list() {
    local extended
    local adhoc

    echo ""
    echo "Protection Lists"
    echo "================"
    echo ""

    echo -e "${CYAN}Core:${NC} (hardcoded)"
    for path in "${CORE_PATHS[@]}"; do
        echo "  $path"
    done

    echo ""
    echo -e "${CYAN}Extended:${NC} (user-managed)"
    extended=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended" ]]; then
        echo "  (empty)"
    else
        while IFS= read -r line; do
            echo "  $line"
        done <<< "$extended"
    fi

    echo ""
    echo -e "${CYAN}Ad-hoc:${NC} (auto-tracked)"
    adhoc=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc" ]]; then
        echo "  (empty)"
    else
        while IFS= read -r line; do
            echo "  $line"
        done <<< "$adhoc"
    fi
}

# Manage extended list
cmd_extend() {
    local action="${1:-}"
    local path="${2:-}"

    case "$action" in
        add)
            if [[ -z "$path" ]]; then
                log_error "Usage: $SCRIPT_NAME extend add <path>"
                exit 1
            fi

            if ! validate_path "$path"; then
                exit 1
            fi

            if is_core_path "$path"; then
                log_error "Cannot add core path (already protected)"
                exit 1
            fi

            add_to_list "$EXTENDED_LIST" "$path"
            log_success "Added: $path"
            log_info "Run 'protect all' to apply"
            ;;
        remove)
            if [[ -z "$path" ]]; then
                log_error "Usage: $SCRIPT_NAME extend remove <path>"
                exit 1
            fi

            remove_from_list "$EXTENDED_LIST" "$path"
            log_success "Removed: $path"
            log_warn "Still protected until 'unprotect $path'"
            ;;
        *)
            log_error "Usage: $SCRIPT_NAME extend <add|remove> <path>"
            echo ""
            echo "  add <path>    - Add path to extended list"
            echo "  remove <path> - Remove path from extended list"
            echo ""
            echo "To view all lists, use: $SCRIPT_NAME list"
            exit 1
            ;;
    esac
}

# =============================================================================
# USAGE
# =============================================================================

usage() {
    cat << EOF
Usage: sudo $SCRIPT_NAME <command> [options]

Commands:
  protect <all|core|path>     Protect paths (requires explicit target)
  unprotect <all|core|path>   Remove protection (confirms for core)
  status [path]               Show protection status
  verify                      Test if protection works
  list                        Show all protection lists
  extend <add|remove> <path>  Manage extended list

Tiers:
  CORE     - Security-critical (hardcoded)
  EXTENDED - User-managed list
  AD-HOC   - Auto-tracked specific paths

Core Paths:
EOF
    for path in "${CORE_PATHS[@]}"; do
        echo "  - $path"
    done

    cat << EOF

Examples:
  sudo $0 protect all           # Protect everything
  sudo $0 protect core          # Protect core only
  sudo $0 protect .claude/hooks # Protect specific path
  sudo $0 unprotect .claude/hooks/script.sh  # Unprotect specific path
  sudo $0 status                # Check status
  sudo $0 verify                # Test protection
  $0 list                       # Show all lists
  sudo $0 extend add mydir      # Add to extended list
EOF
}

# =============================================================================
# MAIN
# =============================================================================

# Change to project root
cd "$PROJECT_ROOT"

# Check for root (except for status, list, and help)
if [[ "${1:-}" != "status" ]] && [[ "${1:-}" != "list" ]] && \
   [[ "${1:-}" != "help" ]] && [[ "${1:-}" != "--help" ]] && [[ "${1:-}" != "-h" ]]; then
    if [[ "$EUID" -ne 0 ]]; then
        log_error "This script requires sudo"
        echo ""
        echo "  sudo $0 ${1:-<command>}${2:+ $2}${3:+ $3}"
        echo ""
        echo "Run '$0 --help' for usage."
        exit 2
    fi
fi

# Ensure list files exist (may fail if protected, that's ok)
mkdir -p "$PROJECT_ROOT/.codeflow/config/enforcement/protection" 2>/dev/null || true
touch "$PROJECT_ROOT/$EXTENDED_LIST" 2>/dev/null || true
touch "$PROJECT_ROOT/$ADHOC_LIST" 2>/dev/null || true

# Ensure audit log directory exists
mkdir -p "$(dirname "$PROJECT_ROOT/$AUDIT_LOG")" 2>/dev/null || true

# Parse command
case "${1:-}" in
    protect)
        cmd_protect "${2:-}"
        ;;
    unprotect)
        cmd_unprotect "${2:-}"
        ;;
    status)
        show_status "${2:-}"
        ;;
    verify)
        verify_protection
        ;;
    list)
        cmd_list
        ;;
    extend)
        cmd_extend "${2:-}" "${3:-}"
        ;;
    --help|-h|help)
        usage
        ;;
    *)
        if [[ -n "${1:-}" ]]; then
            log_error "Unknown command: $1"
            echo ""
        fi
        usage
        exit 1
        ;;
esac
