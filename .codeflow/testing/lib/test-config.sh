#!/usr/bin/env bash
# CodeFlow Test Framework: Configuration
# Location: .codeflow/testing/lib/test-config.sh
#
# Bash 3.2+ compatible (no associative arrays)

# Requires: test-common.sh
CONFIG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$CONFIG_DIR/test-common.sh"

# ============================================================================
# CONFIGURATION PATHS
# ============================================================================

readonly TEST_CONFIG_FILE="${CONFIG_DIR}/../test-config.json"

# ============================================================================
# DEFAULT CONFIGURATION (bash 3.2 compatible - no associative arrays)
# ============================================================================

_CONFIG_DEFAULT_MODE="standard"
_CONFIG_ESSENTIAL_PRIORITIES="CRITICAL"
_CONFIG_STANDARD_PRIORITIES="CRITICAL HIGH"
_CONFIG_FULL_PRIORITIES="CRITICAL HIGH MEDIUM LOW"
_CONFIG_STOP_ON_FAIL="false"
_CONFIG_VERBOSE="false"

# ============================================================================
# CONFIGURATION LOADING
# ============================================================================

load_test_config() {
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        # Load mode configurations
        local default_mode
        default_mode=$(jq -r '.pre_commit.default_mode // "standard"' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$default_mode" && "$default_mode" != "null" ]] && _CONFIG_DEFAULT_MODE="$default_mode"

        # Load priority mappings
        local essential
        essential=$(jq -r '.modes.essential.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$essential" && "$essential" != "null" ]] && _CONFIG_ESSENTIAL_PRIORITIES="$essential"

        local standard
        standard=$(jq -r '.modes.standard.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$standard" && "$standard" != "null" ]] && _CONFIG_STANDARD_PRIORITIES="$standard"

        local full
        full=$(jq -r '.modes.full.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$full" && "$full" != "null" ]] && _CONFIG_FULL_PRIORITIES="$full"

        log_debug "Loaded test configuration from $TEST_CONFIG_FILE"
    else
        log_debug "Using default test configuration"
    fi
}

# ============================================================================
# CONFIGURATION ACCESS
# ============================================================================

get_test_config() {
    local key="$1"
    local default="${2:-}"

    case "$key" in
        default_mode)          echo "${_CONFIG_DEFAULT_MODE:-$default}" ;;
        essential_priorities)  echo "${_CONFIG_ESSENTIAL_PRIORITIES:-$default}" ;;
        standard_priorities)   echo "${_CONFIG_STANDARD_PRIORITIES:-$default}" ;;
        full_priorities)       echo "${_CONFIG_FULL_PRIORITIES:-$default}" ;;
        stop_on_fail)          echo "${_CONFIG_STOP_ON_FAIL:-$default}" ;;
        verbose)               echo "${_CONFIG_VERBOSE:-$default}" ;;
        *)                     echo "$default" ;;
    esac
}

set_test_config() {
    local key="$1"
    local value="$2"

    case "$key" in
        default_mode)          _CONFIG_DEFAULT_MODE="$value" ;;
        essential_priorities)  _CONFIG_ESSENTIAL_PRIORITIES="$value" ;;
        standard_priorities)   _CONFIG_STANDARD_PRIORITIES="$value" ;;
        full_priorities)       _CONFIG_FULL_PRIORITIES="$value" ;;
        stop_on_fail)          _CONFIG_STOP_ON_FAIL="$value" ;;
        verbose)               _CONFIG_VERBOSE="$value" ;;
    esac
}

# ============================================================================
# MODE HANDLING
# ============================================================================

get_current_mode() {
    # Check environment override first
    local mode="${TEST_MODE:-}"

    # Fall back to config default
    if [[ -z "$mode" ]]; then
        mode=$(get_test_config "default_mode" "standard")
    fi

    echo "$mode"
}

get_mode_priorities() {
    local mode="${1:-$(get_current_mode)}"

    case "$mode" in
        essential)
            get_test_config "essential_priorities"
            ;;
        standard)
            get_test_config "standard_priorities"
            ;;
        full)
            get_test_config "full_priorities"
            ;;
        *)
            log_warn "Unknown mode: $mode, using standard"
            get_test_config "standard_priorities"
            ;;
    esac
}

print_mode_info() {
    local mode="${1:-$(get_current_mode)}"
    local priorities
    priorities=$(get_mode_priorities "$mode")

    log_info "Mode: $mode"
    log_info "Priorities: $priorities"
}

should_run_priority() {
    local priority="$1"
    local mode="${2:-$(get_current_mode)}"
    local priorities
    priorities=$(get_mode_priorities "$mode")

    [[ " $priorities " == *" $priority "* ]]
}

# ============================================================================
# TEST FILE PRIORITY LOOKUP
# ============================================================================

# Get the priority of a test file from config
# Args: $1 = test file path (relative to testing dir, e.g., "scripts/db/test_schema.py")
# Returns: CRITICAL, HIGH, MEDIUM, LOW, or UNKNOWN
get_test_priority() {
    local test_file="$1"

    # Normalize path to be relative to testing dir
    local relative_path="$test_file"
    if [[ "$test_file" == /* ]]; then
        # Convert absolute path to relative
        relative_path="${test_file#*/.codeflow/testing/}"
    fi

    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        # Check each priority level
        for priority in CRITICAL HIGH MEDIUM LOW; do
            local found
            found=$(jq -r --arg p "$priority" --arg f "$relative_path" \
                '.priorities[$p].files // [] | map(select(. == $f or endswith($f))) | length' \
                "$TEST_CONFIG_FILE" 2>/dev/null)
            if [[ "$found" -gt 0 ]]; then
                echo "$priority"
                return 0
            fi
        done
    fi

    # Default to MEDIUM if not found in config
    echo "MEDIUM"
}

# Check if a specific test file should run based on current mode
# Args: $1 = test file path, $2 = mode (optional)
# Returns: 0 if should run, 1 if should skip
should_run_test() {
    local test_file="$1"
    local mode="${2:-$(get_current_mode)}"
    local priority
    priority=$(get_test_priority "$test_file")

    should_run_priority "$priority" "$mode"
}

# Get list of test files for a specific priority
# Args: $1 = priority level (CRITICAL, HIGH, MEDIUM, LOW)
# Returns: newline-separated list of test file paths
get_priority_files() {
    local priority="$1"

    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        jq -r --arg p "$priority" '.priorities[$p].files // [] | .[]' "$TEST_CONFIG_FILE" 2>/dev/null
    fi
}

# Get all test files for current mode
# Args: $1 = mode (optional)
# Returns: newline-separated list of test file paths
get_mode_test_files() {
    local mode="${1:-$(get_current_mode)}"
    local priorities
    priorities=$(get_mode_priorities "$mode")

    for priority in $priorities; do
        get_priority_files "$priority"
    done
}

# ============================================================================
# COVERAGE THRESHOLD CONFIGURATION
# ============================================================================

# Default coverage thresholds (can be overridden by test-config.json)
_CONFIG_COV_LINE=90
_CONFIG_COV_BRANCH=90
_CONFIG_COV_FUNCTION=90
_CONFIG_COV_FAIL_UNDER=90
_CONFIG_COV_ENABLED="true"

# Load coverage thresholds from config
load_coverage_config() {
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        local line branch func fail_under enabled

        line=$(jq -r '.coverage_enforcement.thresholds.line // 90' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$line" && "$line" != "null" ]] && _CONFIG_COV_LINE="$line"

        branch=$(jq -r '.coverage_enforcement.thresholds.branch // 90' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$branch" && "$branch" != "null" ]] && _CONFIG_COV_BRANCH="$branch"

        func=$(jq -r '.coverage_enforcement.thresholds.function // 90' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$func" && "$func" != "null" ]] && _CONFIG_COV_FUNCTION="$func"

        fail_under=$(jq -r '.coverage_enforcement.thresholds.fail_under // 90' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$fail_under" && "$fail_under" != "null" ]] && _CONFIG_COV_FAIL_UNDER="$fail_under"

        enabled=$(jq -r '.coverage_enforcement.enabled // true' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$enabled" && "$enabled" != "null" ]] && _CONFIG_COV_ENABLED="$enabled"

        log_debug "Loaded coverage thresholds: line=$_CONFIG_COV_LINE, branch=$_CONFIG_COV_BRANCH, fail_under=$_CONFIG_COV_FAIL_UNDER"
    fi
}

# Get coverage threshold
# Args: $1 = threshold type (line, branch, function, fail_under)
get_coverage_threshold() {
    local type="$1"
    case "$type" in
        line)       echo "$_CONFIG_COV_LINE" ;;
        branch)     echo "$_CONFIG_COV_BRANCH" ;;
        function)   echo "$_CONFIG_COV_FUNCTION" ;;
        fail_under) echo "$_CONFIG_COV_FAIL_UNDER" ;;
        enabled)    echo "$_CONFIG_COV_ENABLED" ;;
        *)          echo "90" ;;
    esac
}

# Check if coverage enforcement is enabled
is_coverage_enabled() {
    [[ "$(get_coverage_threshold enabled)" == "true" ]]
}

# ============================================================================
# CATEGORY LISTING
# ============================================================================

list_categories() {
    # These match the categories in test-config.json
    echo "scripts-db"
    echo "scripts-memory"
    echo "scripts-coordination"
    echo "scripts-codeflow-py-lib"
    echo "scripts-shell-lib"
    echo "scripts-state"
}

# ============================================================================
# INITIALIZATION
# ============================================================================

# Auto-load configuration
load_test_config
load_coverage_config
