#!/usr/bin/env bash
# CodeFlow Shell Library: Logging
# Location: .codeflow/scripts/shell-lib/logging.sh

# Source guard to prevent multiple loads
[[ -n "${_CODEFLOW_LOGGING_LOADED:-}" ]] && return 0
_CODEFLOW_LOGGING_LOADED=1

# Requires: common.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# ============================================================================
# LOG LEVELS
# ============================================================================

readonly LOG_LEVEL_DEBUG=0
readonly LOG_LEVEL_INFO=1
readonly LOG_LEVEL_WARN=2
readonly LOG_LEVEL_ERROR=3

# Default log level (can be overridden via environment)
CODEFLOW_LOG_LEVEL="${CODEFLOW_LOG_LEVEL:-$LOG_LEVEL_INFO}"

# ============================================================================
# COLORS
# ============================================================================

# Respect NO_COLOR (https://no-color.org/) and non-terminal output
if [[ -z "${NO_COLOR:-}" ]] && [[ -t 1 ]]; then
    readonly COLOR_RED='\033[0;31m'
    readonly COLOR_GREEN='\033[0;32m'
    readonly COLOR_YELLOW='\033[0;33m'
    readonly COLOR_BLUE='\033[0;34m'
    readonly COLOR_CYAN='\033[0;36m'
    readonly COLOR_GRAY='\033[0;90m'
    readonly COLOR_BOLD='\033[1m'
    readonly COLOR_RESET='\033[0m'
else
    readonly COLOR_RED=''
    readonly COLOR_GREEN=''
    readonly COLOR_YELLOW=''
    readonly COLOR_BLUE=''
    readonly COLOR_CYAN=''
    readonly COLOR_GRAY=''
    readonly COLOR_BOLD=''
    readonly COLOR_RESET=''
fi

# ============================================================================
# TIMESTAMP UTILITIES
# ============================================================================

# ISO 8601 timestamp (UTC)
get_timestamp_iso() {
    date -u +%Y-%m-%dT%H:%M:%SZ
}

# Unix timestamp
get_timestamp_unix() {
    date +%s
}

# Human-readable timestamp
get_timestamp_human() {
    date '+%Y-%m-%d %H:%M:%S'
}

# ============================================================================
# CORE LOGGING
# ============================================================================

# Internal log function
_log() {
    local level="$1"
    local level_name="$2"
    local color="$3"
    shift 3
    local message="$*"

    # Check log level
    [[ $level -lt $CODEFLOW_LOG_LEVEL ]] && return 0

    local timestamp
    timestamp=$(get_timestamp_iso)
    local script_name
    script_name=$(basename "${BASH_SOURCE[2]:-unknown}")

    # Format: [timestamp] [LEVEL] [script] message
    echo -e "${COLOR_GRAY}[${timestamp}]${COLOR_RESET} ${color}[${level_name}]${COLOR_RESET} ${COLOR_CYAN}[${script_name}]${COLOR_RESET} ${message}" >&2
}

# Public logging functions
log_debug() {
    _log "$LOG_LEVEL_DEBUG" "DEBUG" "$COLOR_GRAY" "$@"
}

log_info() {
    _log "$LOG_LEVEL_INFO" "INFO" "$COLOR_BLUE" "$@"
}

log_warn() {
    _log "$LOG_LEVEL_WARN" "WARN" "$COLOR_YELLOW" "$@"
}

log_error() {
    _log "$LOG_LEVEL_ERROR" "ERROR" "$COLOR_RED" "$@"
}

# ============================================================================
# FILE LOGGING
# ============================================================================

# Get log directory
get_log_dir() {
    local category="${1:-general}"
    echo "$(find_repo_root)/.state/logs/${category}"
}

# Ensure log directory exists
ensure_log_dir() {
    local category="${1:-general}"
    local log_dir
    log_dir=$(get_log_dir "$category")
    ensure_dir "$log_dir"
    echo "$log_dir"
}

# Get log file path with date rotation
get_log_file() {
    local category="${1:-general}"
    local prefix="${2:-log}"
    local date_suffix
    date_suffix=$(date +%Y-%m-%d)
    local log_dir
    log_dir=$(ensure_log_dir "$category")
    echo "${log_dir}/${prefix}-${date_suffix}.log"
}

# Append to log file
log_to_file() {
    local log_file="$1"
    shift
    local message="$*"
    local timestamp
    timestamp=$(get_timestamp_iso)
    echo "[${timestamp}] ${message}" >> "$log_file"
}

# ============================================================================
# STRUCTURED LOGGING (JSON)
# ============================================================================

# Log JSON event
log_json() {
    local log_file="$1"
    local event_type="$2"
    shift 2

    local timestamp
    timestamp=$(get_timestamp_iso)
    local json_data

    # Build JSON object
    json_data=$(cat <<EOF
{"timestamp":"${timestamp}","event":"${event_type}","data":{$*}}
EOF
)

    echo "$json_data" >> "$log_file"
}

# ============================================================================
# CONVENIENCE FUNCTIONS
# ============================================================================

# Success message (green checkmark)
log_success() {
    echo -e "${COLOR_GREEN}✓${COLOR_RESET} $*"
}

# Failure message (red X)
log_failure() {
    echo -e "${COLOR_RED}✗${COLOR_RESET} $*"
}

# Warning message (yellow warning)
log_warning() {
    echo -e "${COLOR_YELLOW}⚠${COLOR_RESET} $*"
}

# Section header
log_section() {
    echo ""
    echo -e "${COLOR_BOLD}=== $* ===${COLOR_RESET}"
}

# Subsection header
log_subsection() {
    echo -e "  ${COLOR_CYAN}--- $* ---${COLOR_RESET}"
}
