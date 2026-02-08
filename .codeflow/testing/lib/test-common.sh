#!/usr/bin/env bash
# CodeFlow Test Framework: Common Utilities
# Location: .codeflow/testing/lib/test-common.sh

set -euo pipefail

# ============================================================================
# VERSION
# ============================================================================

# shellcheck disable=SC2034  # Used by sourcing scripts
readonly TEST_FRAMEWORK_VERSION="1.0.0"

# ============================================================================
# COLORS (exported for use by sourcing scripts)
# ============================================================================

# shellcheck disable=SC2034  # Colors used by sourcing scripts
if [[ -t 1 ]]; then
    readonly RED='\033[0;31m'
    readonly GREEN='\033[0;32m'
    readonly YELLOW='\033[0;33m'
    readonly BLUE='\033[0;34m'
    readonly CYAN='\033[0;36m'
    readonly GRAY='\033[0;90m'
    readonly BOLD='\033[1m'
    readonly NC='\033[0m'  # No Color
else
    readonly RED=''
    readonly GREEN=''
    readonly YELLOW=''
    readonly BLUE=''
    readonly CYAN=''
    readonly GRAY=''
    readonly BOLD=''
    readonly NC=''
fi

# ============================================================================
# TEST COUNTERS
# ============================================================================

export TEST_PASS_COUNT=0
export TEST_FAIL_COUNT=0
export TEST_SKIP_COUNT=0
export TEST_TOTAL_COUNT=0

# ============================================================================
# CATEGORY DEFINITIONS
# ============================================================================

# Note: Using function-based lookup for bash 3.2 compatibility (macOS default)
# Associative arrays require bash 4.0+
get_category_path() {
    local category="$1"
    case "$category" in
        hooks-pre-tool-use)         echo "hooks/pre-tool-use" ;;
        hooks-post-tool-use)        echo "hooks/post-tool-use" ;;
        hooks-session-start)        echo "hooks/session-start" ;;
        hooks-session-end)          echo "hooks/session-end" ;;
        hooks-stop)                 echo "hooks/stop" ;;
        hooks-user-prompt)          echo "hooks/user-prompt-submit" ;;
        scripts-db)                 echo "scripts/db" ;;
        scripts-memory)             echo "scripts/memory" ;;
        scripts-coordination)       echo "scripts/coordination" ;;
        scripts-lib)                echo "scripts/codeflow_py_lib" ;;
        scripts-codeflow-py-lib)    echo "scripts/codeflow_py_lib" ;;
        scripts-shell-lib)          echo "scripts/shell-lib" ;;
        scripts-security)           echo "scripts/security" ;;
        scripts-state)              echo "scripts/state" ;;
        scripts-worktree)           echo "scripts/worktree" ;;
        scripts-health)             echo "scripts/health" ;;
        scripts-commands)           echo "scripts/commands" ;;
        consistency)                echo "consistency" ;;
        autorun)                    echo "autorun" ;;
        *)                          echo "" ;;
    esac
}

# ============================================================================
# PRIORITY LEVELS
# ============================================================================

readonly PRIORITY_CRITICAL="CRITICAL"
readonly PRIORITY_HIGH="HIGH"
readonly PRIORITY_MEDIUM="MEDIUM"
readonly PRIORITY_LOW="LOW"

export PRIORITY_CRITICAL PRIORITY_HIGH PRIORITY_MEDIUM PRIORITY_LOW

# ============================================================================
# LOGGING
# ============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

# ============================================================================
# CROSS-PLATFORM UTILITIES
# ============================================================================
# These functions provide portable alternatives to OS-specific commands

# Detect OS type
get_os_type() {
    case "$(uname -s)" in
        Darwin*)    echo "macos" ;;
        Linux*)     echo "linux" ;;
        CYGWIN*|MINGW*|MSYS*) echo "windows" ;;
        *)          echo "unknown" ;;
    esac
}

# Portable date conversion from ISO8601 to epoch
# Usage: iso_to_epoch "2024-01-15T10:30:00Z"
iso_to_epoch() {
    local iso_date="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        date -j -f "%Y-%m-%dT%H:%M:%SZ" "$iso_date" +%s 2>/dev/null || echo ""
    else
        date -d "$iso_date" +%s 2>/dev/null || echo ""
    fi
}

# Portable date conversion from epoch to ISO8601
# Usage: epoch_to_iso 1705315800
epoch_to_iso() {
    local epoch="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        date -r "$epoch" -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo ""
    else
        date -d "@$epoch" -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo ""
    fi
}

# Portable stat for file permissions (returns octal like 644)
# Usage: get_file_perms "/path/to/file"
get_file_perms() {
    local file="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        stat -f '%A' "$file" 2>/dev/null || echo "644"
    else
        stat -c '%a' "$file" 2>/dev/null || echo "644"
    fi
}

# Portable floating point math (using awk instead of bc)
# Usage: float_calc "0.95 * 100"
float_calc() {
    local expr="$1"
    local format="${2:-%.0f}"
    awk "BEGIN {printf \"$format\", $expr}" 2>/dev/null || echo "0"
}

# Portable sed in-place edit (handles macOS vs GNU sed)
# Usage: sed_inplace 's/old/new/g' file.txt
sed_inplace() {
    local expr="$1"
    local file="$2"
    if [[ "$(uname)" == "Darwin" ]]; then
        sed -i '' "$expr" "$file"
    else
        sed -i "$expr" "$file"
    fi
}

# Check if command exists
command_exists() {
    command -v "$1" &>/dev/null
}

log_debug() {
    if [[ "${DEBUG:-0}" == "1" ]]; then
        echo -e "${GRAY}[DEBUG]${NC} $*" >&2
    fi
}

log_section() {
    echo ""
    echo -e "${BOLD}=== $* ===${NC}"
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

get_category_dir() {
    local category="$1"
    get_category_path "$category"
}

get_timestamp() {
    date -u +%Y-%m-%dT%H:%M:%SZ
}

get_repo_root() {
    git rev-parse --show-toplevel 2>/dev/null || pwd
}
