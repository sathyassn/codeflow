#!/usr/bin/env bash
# Purpose:   Change CodeFlow V3 approval mode by applying settings templates
# Location:  .codeflow/scripts/settings/cf-change-approval-mode.sh
# Usage:     ./cf-change-approval-mode.sh <mode> [--force]
# Version:   1.0.0
#
# Modes:
#   strict      - Maximum safety, human approval required for most operations
#   standard    - Balanced mode, sensible defaults with some automation
#   autonomous  - High automation, minimal interruptions for routine tasks
#   permissive  - Developer mode, maximum automation with fewer safeguards
#
# This script:
#   1. Validates the requested mode exists as a template
#   2. Backs up current settings.local.json (if exists)
#   3. Copies the template to settings.local.json
#   4. Preserves any custom _meta or environment-specific settings
#
# Settings Templates Location:
#   .claude/settings-templates/{mode}.json
#
# Target Settings File:
#   .claude/settings.local.json
#
# Examples:
#   ./cf-change-approval-mode.sh strict         # Switch to strict mode
#   ./cf-change-approval-mode.sh autonomous     # Switch to autonomous mode
#   ./cf-change-approval-mode.sh --list         # List available modes

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Paths
TEMPLATES_DIR="$PROJECT_ROOT/.claude/settings-templates"
SETTINGS_LOCAL="$PROJECT_ROOT/.claude/settings.local.json"
BACKUP_DIR="$PROJECT_ROOT/.state/backups/settings"

# Valid modes (must match template filenames)
VALID_MODES=("strict" "standard" "autonomous" "permissive")

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# =============================================================================
# LOGGING FUNCTIONS
# =============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

# Check if mode is valid
is_valid_mode() {
    local mode="$1"
    for valid in "${VALID_MODES[@]}"; do
        if [[ "$mode" == "$valid" ]]; then
            return 0
        fi
    done
    return 1
}

# Get current mode from settings
get_current_mode() {
    if [[ ! -f "$SETTINGS_LOCAL" ]]; then
        echo "none"
        return
    fi

    if command -v jq &>/dev/null; then
        local mode
        mode=$(jq -r '._template // "unknown"' "$SETTINGS_LOCAL" 2>/dev/null)
        echo "${mode:-unknown}"
    else
        echo "unknown"
    fi
}

# List available modes
list_modes() {
    local current_mode
    current_mode=$(get_current_mode)

    echo ""
    echo "Available Approval Modes"
    echo "========================"
    echo ""

    for mode in "${VALID_MODES[@]}"; do
        local template="$TEMPLATES_DIR/${mode}.json"
        local marker=""

        if [[ "$mode" == "$current_mode" ]]; then
            marker=" ${GREEN}(current)${NC}"
        fi

        if [[ -f "$template" ]]; then
            echo -e "  ${CYAN}$mode${NC}$marker"

            # Show description if jq available
            if command -v jq &>/dev/null; then
                local desc
                desc=$(jq -r '._description // empty' "$template" 2>/dev/null)
                if [[ -n "$desc" ]]; then
                    echo "    $desc"
                fi
            fi
        else
            echo -e "  ${YELLOW}$mode${NC} (template missing)"
        fi
    done

    echo ""
    echo "Usage: $0 <mode>"
    echo ""
}

# Backup current settings
backup_settings() {
    if [[ ! -f "$SETTINGS_LOCAL" ]]; then
        return 0
    fi

    mkdir -p "$BACKUP_DIR"

    local timestamp
    timestamp=$(date '+%Y%m%d_%H%M%S')
    local backup_file="$BACKUP_DIR/settings.local.${timestamp}.json"

    cp "$SETTINGS_LOCAL" "$backup_file"
    log_info "Backed up current settings to: $backup_file"
}

# Apply template
apply_template() {
    local mode="$1"
    local force="${2:-false}"
    local template="$TEMPLATES_DIR/${mode}.json"

    # Check template exists
    if [[ ! -f "$template" ]]; then
        log_error "Template not found: $template"
        return 1
    fi

    # Get current mode
    local current_mode
    current_mode=$(get_current_mode)

    if [[ "$current_mode" == "$mode" ]] && [[ "$force" != "true" ]]; then
        log_info "Already in $mode mode. Use --force to reapply."
        return 0
    fi

    # Backup existing settings
    backup_settings

    # Copy template to settings.local.json
    cp "$template" "$SETTINGS_LOCAL"

    log_success "Applied $mode mode"

    # Show what changed
    if command -v jq &>/dev/null; then
        echo ""
        echo "Mode Details:"
        echo "============="

        local desc
        local version
        desc=$(jq -r '._description // "No description"' "$SETTINGS_LOCAL" 2>/dev/null)
        version=$(jq -r '._version // "unknown"' "$SETTINGS_LOCAL" 2>/dev/null)

        echo "  Description: $desc"
        echo "  Version: $version"

        # Show key settings
        echo ""
        echo "Key Settings:"
        local allow_permissions
        allow_permissions=$(jq -r '.permissions.allow | length' "$SETTINGS_LOCAL" 2>/dev/null)
        echo "  Allowed permissions: $allow_permissions entries"

        local deny_permissions
        deny_permissions=$(jq -r '.permissions.deny | length' "$SETTINGS_LOCAL" 2>/dev/null)
        echo "  Denied permissions: $deny_permissions entries"

        local hooks_count
        hooks_count=$(jq -r '[.hooks | to_entries[] | .value | length] | add // 0' "$SETTINGS_LOCAL" 2>/dev/null)
        echo "  Hooks configured: $hooks_count"
    fi

    return 0
}

# =============================================================================
# USAGE
# =============================================================================

show_usage() {
    cat << 'EOF'
CodeFlow V3 Approval Mode Switcher

Usage: ./cf-change-approval-mode.sh <mode> [options]

Modes:
  strict        Maximum safety, human approval required for most operations
  standard      Balanced mode, sensible defaults with some automation
  autonomous    High automation, minimal interruptions for routine tasks
  permissive    Developer mode, maximum automation with fewer safeguards

Options:
  --list        List available modes and their descriptions
  --force       Reapply mode even if already set
  --help        Show this help message

Examples:
  ./cf-change-approval-mode.sh strict         # Switch to strict mode
  ./cf-change-approval-mode.sh autonomous     # Switch to autonomous mode
  ./cf-change-approval-mode.sh --list         # List available modes

Notes:
  - Current settings are backed up before changes
  - Templates are in .claude/settings-templates/
  - Settings are applied to .claude/settings.local.json
EOF
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    local mode=""
    local force="false"

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --list|-l)
                list_modes
                exit 0
                ;;
            --force|-f)
                force="true"
                shift
                ;;
            --help|-h)
                show_usage
                exit 0
                ;;
            -*)
                log_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
            *)
                if [[ -z "$mode" ]]; then
                    mode="$1"
                else
                    log_error "Too many arguments"
                    show_usage
                    exit 1
                fi
                shift
                ;;
        esac
    done

    # Check mode provided
    if [[ -z "$mode" ]]; then
        log_error "No mode specified"
        echo ""
        show_usage
        exit 1
    fi

    # Validate mode
    if ! is_valid_mode "$mode"; then
        log_error "Invalid mode: $mode"
        echo ""
        echo "Valid modes: ${VALID_MODES[*]}"
        exit 1
    fi

    # Apply the template
    echo ""
    echo "Changing Approval Mode"
    echo "======================"
    echo "Current: $(get_current_mode)"
    echo "Target:  $mode"
    echo ""

    if apply_template "$mode" "$force"; then
        echo ""
        log_success "Mode change complete"
    else
        log_error "Mode change failed"
        exit 1
    fi
}

main "$@"
