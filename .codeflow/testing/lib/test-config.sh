#!/usr/bin/env bash
# CodeFlow Test Framework: Configuration
# Location: .codeflow/testing/lib/test-config.sh

# Requires: test-common.sh
CONFIG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$CONFIG_DIR/test-common.sh"

# ============================================================================
# CONFIGURATION PATHS
# ============================================================================

readonly TEST_CONFIG_FILE="${CONFIG_DIR}/../test-config.json"

# ============================================================================
# DEFAULT CONFIGURATION
# ============================================================================

declare -A TEST_CONFIG=(
    ["default_mode"]="standard"
    ["essential_priorities"]="CRITICAL"
    ["standard_priorities"]="CRITICAL HIGH"
    ["full_priorities"]="CRITICAL HIGH MEDIUM LOW"
    ["stop_on_fail"]="false"
    ["verbose"]="false"
)

# ============================================================================
# CONFIGURATION LOADING
# ============================================================================

load_test_config() {
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        # Load mode configurations
        local default_mode
        default_mode=$(jq -r '.pre_commit.default_mode // "standard"' "$TEST_CONFIG_FILE" 2>/dev/null)
        TEST_CONFIG["default_mode"]="$default_mode"

        # Load priority mappings
        local essential
        essential=$(jq -r '.modes.essential.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$essential" && "$essential" != "null" ]] && TEST_CONFIG["essential_priorities"]="$essential"

        local standard
        standard=$(jq -r '.modes.standard.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$standard" && "$standard" != "null" ]] && TEST_CONFIG["standard_priorities"]="$standard"

        local full
        full=$(jq -r '.modes.full.priorities | join(" ")' "$TEST_CONFIG_FILE" 2>/dev/null)
        [[ -n "$full" && "$full" != "null" ]] && TEST_CONFIG["full_priorities"]="$full"

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
    echo "${TEST_CONFIG[$key]:-$default}"
}

set_test_config() {
    local key="$1"
    local value="$2"
    TEST_CONFIG["$key"]="$value"
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

should_run_priority() {
    local priority="$1"
    local mode="${2:-$(get_current_mode)}"
    local priorities
    priorities=$(get_mode_priorities "$mode")

    [[ " $priorities " == *" $priority "* ]]
}

# ============================================================================
# INITIALIZATION
# ============================================================================

# Auto-load configuration
load_test_config
