#!/usr/bin/env bash
# CodeFlow Test Framework: Common Utilities
# Location: .codeflow/testing/lib/test-common.sh

set -euo pipefail

# ============================================================================
# VERSION
# ============================================================================

readonly TEST_FRAMEWORK_VERSION="1.0.0"

# ============================================================================
# COLORS
# ============================================================================

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

declare -A CATEGORIES=(
    # Hook tests
    ["hooks-pre-tool-use"]="hooks/pre-tool-use"
    ["hooks-post-tool-use"]="hooks/post-tool-use"
    ["hooks-session-start"]="hooks/session-start"
    ["hooks-session-end"]="hooks/session-end"
    ["hooks-stop"]="hooks/stop"
    ["hooks-user-prompt"]="hooks/user-prompt-submit"
    # Script tests
    ["scripts-db"]="scripts/db"
    ["scripts-memory"]="scripts/memory"
    ["scripts-coordination"]="scripts/coordination"
    ["scripts-lib"]="scripts/py-lib"
    ["scripts-shell-lib"]="scripts/shell-lib"
    ["scripts-security"]="scripts/security"
    ["scripts-state"]="scripts/state"
    ["scripts-worktree"]="scripts/worktree"
    ["scripts-health"]="scripts/health"
    ["scripts-commands"]="scripts/commands"
    # Cross-component
    ["consistency"]="consistency"
    # Autorun
    ["autorun"]="autorun"
)

export CATEGORIES

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

log_debug() {
    if [[ "${DEBUG:-0}" == "1" ]]; then
        echo -e "${GRAY}[DEBUG]${NC} $*" >&2
    fi
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

get_category_dir() {
    local category="$1"
    echo "${CATEGORIES[$category]:-}"
}

get_timestamp() {
    date -u +%Y-%m-%dT%H:%M:%SZ
}

get_repo_root() {
    git rev-parse --show-toplevel 2>/dev/null || pwd
}
