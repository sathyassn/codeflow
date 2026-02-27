#!/usr/bin/env bash
# Helper script to install Claude Code enterprise managed settings
# Installs system-level settings file that cannot be overridden by project settings
#
# Features:
#   - Detects existing installation and shows diff
#   - Creates timestamped backup before overwriting
#   - Asks for confirmation before overwriting
#
# Usage:
#   .codeflow/scripts/settings/setup-managed-settings.sh [--dry-run]
#
# Options:
#   --dry-run        Show what would be done without making changes
#   --help           Show this help message

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Enterprise settings source location (separate from script location)
CLAUDE_ENTERPRISE_DIR="$REPO_ROOT/.codeflow/docs/security/claude-enterprise"

# Settings file
SETTINGS_FILE="managed-settings.json"

# Default options
DRY_RUN=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --help)
      head -n 15 "$0" | tail -n +2 | sed 's/^# //'
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown option: $1${NC}"
      echo "Use --help for usage information"
      exit 1
      ;;
  esac
done

# Detect operating system
detect_os() {
  if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "macos"
  elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "linux"
  elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    echo "windows"
  else
    echo "unknown"
  fi
}

# Get appropriate settings directory based on OS
get_settings_dir() {
  local os="$1"
  case "$os" in
    macos)
      echo "/Library/Application Support/ClaudeCode"
      ;;
    linux)
      echo "/etc/claude-code"
      ;;
    windows)
      echo "$PROGRAMDATA/ClaudeCode"
      ;;
    *)
      echo ""
      ;;
  esac
}

# Get source settings file path
get_source_file() {
  echo "$CLAUDE_ENTERPRISE_DIR/$SETTINGS_FILE"
}

# Print header
print_header() {
  echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
  echo -e "${BLUE}  Claude Code Enterprise Managed Settings Installer${NC}"
  echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
  echo ""
}

# Print summary
print_summary() {
  local os="$1"
  local settings_dir="$2"
  local source_file="$3"
  local target_file="$settings_dir/managed-settings.json"

  echo -e "${YELLOW}Configuration:${NC}"
  echo "  Operating System: $os"
  echo "  Source File: $source_file"
  echo "  Target File: $target_file"
  echo ""

  if [[ "$DRY_RUN" == true ]]; then
    echo -e "${YELLOW}DRY RUN MODE: No changes will be made${NC}"
    echo ""
  fi
}

# Check if running as root/admin
check_privileges() {
  local os="$1"

  if [[ "$os" == "windows" ]]; then
    # Windows: Check if running as Administrator
    net session &>/dev/null
    return $?
  else
    # Unix-like: Check if running as root or with sudo
    if [[ $EUID -eq 0 ]]; then
      return 0
    elif command -v sudo &>/dev/null && sudo -n true 2>/dev/null; then
      return 0
    else
      return 1
    fi
  fi
}

# Check if target file exists and show diff
check_existing_file() {
  local os="$1"
  local target_file="$2"
  local source_file="$3"

  if [[ "$os" == "windows" ]]; then
    [[ -f "$target_file" ]]
  else
    sudo test -f "$target_file" 2>/dev/null
  fi
}

# Show diff between existing and new file
show_diff() {
  local os="$1"
  local target_file="$2"
  local source_file="$3"

  echo -e "${YELLOW}Changes (existing → new):${NC}"
  echo ""

  if command -v diff &>/dev/null; then
    if [[ "$os" == "windows" ]]; then
      diff --color=auto -u "$target_file" "$source_file" 2>/dev/null || true
    else
      # Use sudo to read the existing file, compare with new
      sudo diff --color=auto -u "$target_file" "$source_file" 2>/dev/null || true
    fi
  else
    echo "(diff command not available - skipping comparison)"
  fi
  echo ""
}

# Create backup of existing file
backup_existing_file() {
  local os="$1"
  local target_file="$2"
  local backup_file
  backup_file="${target_file}.bak.$(date +%Y%m%d-%H%M%S)"

  echo -e "${BLUE}Creating backup...${NC}"
  if [[ "$DRY_RUN" == true ]]; then
    echo "Would backup: $target_file → $backup_file"
  else
    if [[ "$os" == "windows" ]]; then
      cp "$target_file" "$backup_file"
    else
      sudo cp "$target_file" "$backup_file"
    fi
    echo -e "${GREEN}✓${NC} Backup created: $backup_file"
  fi
  echo ""
}

# Install settings file
install_settings() {
  local os="$1"
  local settings_dir="$2"
  local source_file="$3"
  local target_file="$settings_dir/managed-settings.json"
  local file_exists=false

  echo -e "${YELLOW}Installation Steps:${NC}"
  echo ""

  # Step 1: Verify source file exists
  echo -e "${BLUE}[1/5]${NC} Verifying source file..."
  if [[ ! -f "$source_file" ]]; then
    echo -e "${RED}ERROR: Source file not found: $source_file${NC}"
    return 1
  fi
  echo -e "${GREEN}✓${NC} Source file found"
  echo ""

  # Step 2: Check for existing file
  echo -e "${BLUE}[2/5]${NC} Checking for existing installation..."
  if check_existing_file "$os" "$target_file" "$source_file"; then
    file_exists=true
    echo -e "${YELLOW}⚠${NC} Existing file found: $target_file"
    echo ""

    # Show diff
    show_diff "$os" "$target_file" "$source_file"

    # Confirm overwrite
    if [[ "$DRY_RUN" == false ]]; then
      read -p "Overwrite existing file? [y/N] " -n 1 -r
      echo
      if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Aborted. Existing file preserved."
        return 1
      fi
      echo ""
    fi
  else
    echo -e "${GREEN}✓${NC} No existing file (fresh install)"
  fi
  echo ""

  # Step 3: Backup existing file (if exists)
  if [[ "$file_exists" == true ]]; then
    echo -e "${BLUE}[3/5]${NC} Backing up existing file..."
    backup_existing_file "$os" "$target_file"
  else
    echo -e "${BLUE}[3/5]${NC} Backup step..."
    echo -e "${YELLOW}⊘${NC} Skipped (no existing file)"
    echo ""
  fi

  # Step 4: Create target directory
  echo -e "${BLUE}[4/5]${NC} Creating target directory..."
  if [[ "$DRY_RUN" == true ]]; then
    echo "Would create: $settings_dir"
  else
    if [[ "$os" == "windows" ]]; then
      mkdir -p "$settings_dir"
    else
      sudo mkdir -p "$settings_dir"
    fi
    echo -e "${GREEN}✓${NC} Directory ready"
  fi
  echo ""

  # Step 5: Copy settings file
  echo -e "${BLUE}[5/5]${NC} Installing settings file..."
  if [[ "$DRY_RUN" == true ]]; then
    echo "Would copy: $source_file → $target_file"
  else
    if [[ "$os" == "windows" ]]; then
      cp "$source_file" "$target_file"
    else
      sudo cp "$source_file" "$target_file"
    fi
    # Set permissions (Unix-like only)
    if [[ "$os" != "windows" ]]; then
      sudo chmod 644 "$target_file"
      echo -e "${GREEN}✓${NC} Settings file installed (permissions: 644)"
    else
      echo -e "${GREEN}✓${NC} Settings file installed"
    fi
  fi
  echo ""
}

# Print verification instructions
print_verification() {
  local settings_dir="$1"
  local target_file="$settings_dir/managed-settings.json"

  echo -e "${YELLOW}Verification:${NC}"
  echo ""
  echo "To verify the installation:"
  echo ""
  echo "  1. Check file exists:"
  echo "     ls -la \"$target_file\""
  echo ""
  echo "  2. View contents:"
  echo "     cat \"$target_file\""
  echo ""
  echo "  3. Test restrictions (should be blocked):"
  echo "     # In Claude Code, try:"
  echo "     git commit --no-verify -m \"test\""
  echo ""
  echo "For complete testing instructions, see:"
  echo "  $CLAUDE_ENTERPRISE_DIR/README.md"
  echo ""
}

# Print next steps
print_next_steps() {
  echo -e "${YELLOW}Next Steps:${NC}"
  echo ""
  echo "  1. Restart Claude Code for settings to take effect"
  echo "  2. Test restrictions (see verification section above)"
  echo "  3. Review security documentation:"
  echo "     - $REPO_ROOT/.codeflow/docs/security/"
  echo ""
}

# Main execution
main() {
  print_header

  # Detect OS
  local os
  os=$(detect_os)
  if [[ "$os" == "unknown" ]]; then
    echo -e "${RED}ERROR: Unsupported operating system: $OSTYPE${NC}"
    echo "Supported: macOS, Linux, Windows"
    exit 1
  fi

  # Get paths
  local settings_dir
  settings_dir=$(get_settings_dir "$os")
  local source_file
  source_file=$(get_source_file)

  # Print summary
  print_summary "$os" "$settings_dir" "$source_file"

  # Check source file exists
  if [[ ! -f "$source_file" ]]; then
    echo -e "${RED}ERROR: Source file not found: $source_file${NC}"
    echo ""
    echo "Expected location: $CLAUDE_ENTERPRISE_DIR/$SETTINGS_FILE"
    exit 1
  fi

  # Check privileges (warn only, don't block)
  if ! check_privileges "$os"; then
    echo -e "${YELLOW}WARNING: Not running with elevated privileges${NC}"
    echo "Installation may fail. On Unix-like systems, you may need sudo."
    echo ""
    if [[ "$DRY_RUN" == false ]]; then
      read -p "Continue anyway? [y/N] " -n 1 -r
      echo
      if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 1
      fi
      echo ""
    fi
  fi

  # Confirm installation
  if [[ "$DRY_RUN" == false ]]; then
    echo -e "${YELLOW}Ready to install enterprise managed settings.${NC}"
    echo ""
    echo "This will:"
    echo "  - Check for existing installation"
    echo "  - Show diff if file exists"
    echo "  - Create timestamped backup if overwriting"
    echo "  - Create system-level settings directory"
    echo "  - Install managed-settings.json"
    echo "  - Set appropriate permissions (644)"
    echo ""
    echo "These settings CANNOT be overridden by project-level settings."
    echo "They apply to ALL Claude Code usage on this machine."
    echo ""
    read -p "Proceed with installation? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
      echo "Aborted."
      exit 0
    fi
    echo ""
  fi

  # Install
  if install_settings "$os" "$settings_dir" "$source_file"; then
    echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}  Installation Successful!${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════════════════════${NC}"
    echo ""

    if [[ "$DRY_RUN" == false ]]; then
      print_verification "$settings_dir"
      print_next_steps
    else
      echo "Dry run complete. No changes were made."
      echo ""
      echo "To perform actual installation, run without --dry-run flag:"
      echo "  $0"
      echo ""
    fi
  else
    echo -e "${RED}Installation failed. See errors above.${NC}"
    exit 1
  fi
}

# Run main
main
