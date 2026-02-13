#!/usr/bin/env bash
# Purpose:   Change CodeFlow approval mode by applying settings templates
# Location:  .codeflow/scripts/settings/cf-change-approval-mode.sh
# Usage:     ./cf-change-approval-mode.sh <mode> [--force]
# Version:   2.0.0
#
# Changelog:
#   2.0.0 - Added --status option, protection check, behavior summary
#         - Added re-protect reminder, mode detection fallback
#         - Added _use_case display, workflow steps in help
#         - Removed version references from output
#   1.0.0 - Initial release
#
# Modes:
#   strict      - Maximum safety, human approval required for most operations
#   standard    - Balanced mode, sensible defaults with some automation
#   autonomous  - High automation, minimal interruptions for routine tasks
#   permissive  - Developer mode, maximum automation with fewer safeguards
#
# This script:
#   1. Validates the requested mode exists as a template
#   2. Checks if settings file is protected (immutable)
#   3. Backs up current settings.local.json (if exists)
#   4. Copies the template to settings.local.json
#
# Settings Templates Location:
#   .claude/settings-templates/{mode}.json
#
# Target Settings File:
#   .claude/settings.local.json
#
# Security:
#   This script does NOT modify .claude/settings.json (project default).
#   It only modifies .claude/settings.local.json (user override).
#   Both files are protected by cf-protect-resources.sh and must be
#   unprotected first. Security rules (L0 blocks) are always enforced
#   regardless of approval mode.
#
# Examples:
#   ./cf-change-approval-mode.sh strict         # Switch to strict mode
#   ./cf-change-approval-mode.sh autonomous     # Switch to autonomous mode
#   ./cf-change-approval-mode.sh --list         # List available modes
#   ./cf-change-approval-mode.sh --status       # Show current mode status

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Paths
TEMPLATES_DIR="$PROJECT_ROOT/.claude/settings-templates"
PROJECT_SETTINGS="$PROJECT_ROOT/.claude/settings.json"
SETTINGS_LOCAL="$PROJECT_ROOT/.claude/settings.local.json"
BACKUP_DIR="$PROJECT_ROOT/.state/backups/settings"
PROTECT_SCRIPT="$PROJECT_ROOT/.codeflow/scripts/security/protection/cf-protect-resources.sh"

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

# Get mode from a specific settings file
get_mode_from_file() {
    local file="$1"

    if [[ ! -f "$file" ]]; then
        echo "none"
        return
    fi

    if ! command -v jq &>/dev/null; then
        echo "unknown"
        return
    fi

    # Read the _template field if it exists
    local template
    template=$(jq -r '._template // "unknown"' "$file" 2>/dev/null)
    if [[ "$template" != "unknown" && "$template" != "null" ]]; then
        echo "$template"
        return
    fi

    # Fallback: detect from defaultMode + allow list
    local default_mode
    default_mode=$(jq -r '.permissions.defaultMode // "unknown"' "$file" 2>/dev/null)
    local allow_count
    allow_count=$(jq '.permissions.allow | length' "$file" 2>/dev/null || echo "0")

    if [[ "$default_mode" == "askPermissions" ]]; then
        if [[ "$allow_count" -lt 30 ]]; then
            echo "strict (detected)"
        else
            echo "standard (detected)"
        fi
    elif [[ "$default_mode" == "bypassPermissions" ]]; then
        if jq -e '.permissions.allow | index("Bash(git push:*)")' "$file" >/dev/null 2>&1; then
            echo "permissive (detected)"
        else
            echo "autonomous (detected)"
        fi
    else
        echo "unknown"
    fi
}

# Get effective current mode
get_current_mode() {
    local local_mode
    local_mode=$(get_mode_from_file "$SETTINGS_LOCAL")

    if [[ "$local_mode" != "none" ]]; then
        echo "$local_mode"
    else
        local project_mode
        project_mode=$(get_mode_from_file "$PROJECT_SETTINGS")
        if [[ "$project_mode" != "none" ]]; then
            echo "$project_mode (project default)"
        else
            echo "none"
        fi
    fi
}

# Check if file is protected (immutable)
check_protection() {
    if [[ ! -f "$SETTINGS_LOCAL" ]]; then
        return 1  # Doesn't exist, not protected
    fi

    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS: check for uchg (user immutable) flag
        if stat -f "%Sf" "$SETTINGS_LOCAL" 2>/dev/null | grep -q "uchg"; then
            return 0  # Protected
        fi
    else
        # Linux: check immutable attribute
        if lsattr "$SETTINGS_LOCAL" 2>/dev/null | grep -q "i"; then
            return 0  # Protected
        fi
    fi
    return 1  # Not protected
}

# Show mode status (project vs local vs effective)
show_mode_status() {
    local project_mode
    local local_mode

    project_mode=$(get_mode_from_file "$PROJECT_SETTINGS")
    local_mode=$(get_mode_from_file "$SETTINGS_LOCAL")

    echo ""
    echo -e "${BLUE}Current approval modes:${NC}"
    echo ""
    echo -e "  ${CYAN}Project default${NC} (.claude/settings.json):"
    if [[ "$project_mode" == "none" ]]; then
        echo -e "    ${YELLOW}Not configured${NC}"
    else
        echo -e "    ${GREEN}$project_mode${NC}"
    fi
    echo ""
    echo -e "  ${CYAN}Local override${NC} (.claude/settings.local.json):"
    if [[ "$local_mode" == "none" ]]; then
        echo -e "    ${YELLOW}Not set${NC} (using project default)"
    else
        echo -e "    ${GREEN}$local_mode${NC}"
    fi
    echo ""
    echo -e "  ${CYAN}Effective mode${NC} (what Claude uses):"
    if [[ "$local_mode" != "none" ]]; then
        echo -e "    ${GREEN}$local_mode${NC} (from local override)"
    elif [[ "$project_mode" != "none" ]]; then
        echo -e "    ${GREEN}$project_mode${NC} (from project default)"
    else
        echo -e "    ${YELLOW}Claude Code defaults${NC}"
    fi
    echo ""
}

# Show behavior summary for a mode
show_behavior_summary() {
    local mode="$1"

    echo ""
    echo -e "${BLUE}Behavior summary:${NC}"
    case "$mode" in
        strict)
            echo "  Read/search:         Auto"
            echo "  Edit/Write:          Ask"
            echo "  Git add/commit/push: Ask"
            echo "  npm/pip/yarn:        Ask"
            echo "  All other ops:       Ask"
            ;;
        standard)
            echo "  Read/Edit/Write:     Auto"
            echo "  Git add/checkout:    Auto"
            echo "  npm/pip/yarn:        Auto"
            echo "  Git commit/push:     Ask"
            echo "  Delete (rm):         Ask"
            echo "  gh pr create:        Ask"
            ;;
        autonomous)
            echo "  All file operations: Auto"
            echo "  Git commit:          Auto"
            echo "  Delete (rm):         Auto"
            echo "  Git push/pull:       Ask"
            echo "  gh pr create:        Ask"
            ;;
        permissive)
            echo "  All operations:      Auto"
            echo "  Only L0 security blocks apply"
            ;;
    esac
}

# List available modes
list_modes() {
    local current_mode
    current_mode=$(get_current_mode)

    echo ""
    echo -e "${BLUE}Available Approval Modes${NC}"
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

            # Show description and use_case if jq available
            if command -v jq &>/dev/null; then
                local desc
                desc=$(jq -r '._description // empty' "$template" 2>/dev/null)
                if [[ -n "$desc" ]]; then
                    echo "    $desc"
                fi

                local use_case
                use_case=$(jq -r '._use_case // empty' "$template" 2>/dev/null)
                if [[ -n "$use_case" ]]; then
                    echo -e "    ${CYAN}Use case:${NC} $use_case"
                fi
            fi
            echo ""
        else
            echo -e "  ${YELLOW}$mode${NC} (template missing)"
            echo ""
        fi
    done

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

    # Check if file is protected
    if check_protection; then
        log_warn "settings.local.json is protected (immutable)"
        echo ""
        echo "To change the approval mode, first unprotect the file:"
        echo ""
        echo -e "  ${CYAN}Option 1: Unprotect just this file${NC}"
        echo "  sudo $PROTECT_SCRIPT unprotect .claude/settings.local.json"
        echo ""
        echo -e "  ${CYAN}Option 2: Unprotect all (then re-protect after)${NC}"
        echo "  sudo $PROTECT_SCRIPT unprotect all"
        echo ""
        echo "Then run this script again:"
        echo "  $0 $mode"
        return 1
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

    # Show behavior summary
    show_behavior_summary "$mode"

    # Re-protect reminder
    echo ""
    log_warn "Remember to re-protect the file:"
    echo "  sudo $PROTECT_SCRIPT protect .claude/settings.local.json"
    echo "  # or: sudo $PROTECT_SCRIPT protect all"

    return 0
}

# =============================================================================
# USAGE
# =============================================================================

show_usage() {
    cat << EOF
CodeFlow Approval Mode Switcher

Usage: $0 <mode> [options]

Modes:
  strict        Maximum safety, human approval required for most operations
  standard      Balanced mode, sensible defaults with some automation
  autonomous    High automation, minimal interruptions for routine tasks
  permissive    Developer mode, maximum automation with fewer safeguards

Options:
  --list        List available modes and their descriptions
  --status      Show current mode status (project vs local)
  --force       Reapply mode even if already set
  --help        Show this help message

Examples:
  $0 strict         # Switch to strict mode
  $0 autonomous     # Switch to autonomous mode
  $0 --list         # List available modes
  $0 --status       # Show which settings file is active

Workflow:
  1. Unprotect: sudo $PROTECT_SCRIPT unprotect .claude/settings.local.json
  2. Change:    $0 standard
  3. Protect:   sudo $PROTECT_SCRIPT protect .claude/settings.local.json

  Or use 'all' to unprotect/protect everything:
  sudo $PROTECT_SCRIPT unprotect all
  $0 standard
  sudo $PROTECT_SCRIPT protect all

Notes:
  - Current settings are backed up before changes
  - Templates are in .claude/settings-templates/
  - Settings are applied to .claude/settings.local.json
  - Approval modes only affect workflow ergonomics (ask vs auto-approve)
  - Security rules (L0 blocks) are always enforced regardless of mode

Current mode: $(get_current_mode)
EOF
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    local mode=""
    local force="false"

    # Parse arguments (info commands allowed from any context)
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --list|-l)
                list_modes
                exit 0
                ;;
            --status|-s)
                show_mode_status
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

    # Security: require interactive terminal for mode changes.
    # Info commands (--help, --list, --status) are allowed from any context.
    # Mode changes are blocked from non-interactive contexts to prevent
    # AI agents from self-escalating their permission level.
    # Set CF_ALLOW_MODE_CHANGE=1 to bypass (for CI/automation only).
    if [[ ! -t 0 ]] && [[ "${CF_ALLOW_MODE_CHANGE:-}" != "1" ]]; then
        log_error "Mode changes require an interactive terminal"
        log_error "This prevents AI agents from self-escalating permissions"
        echo ""
        echo "Run directly from your shell:"
        echo "  $0 $mode"
        echo ""
        echo "Or use the CodeFlow CLI:"
        echo "  ./codeflow mode $mode"
        echo ""
        echo "For CI/automation, set CF_ALLOW_MODE_CHANGE=1"
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
