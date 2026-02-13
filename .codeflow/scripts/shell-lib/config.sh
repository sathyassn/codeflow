#!/usr/bin/env bash
# CodeFlow Shell Library: Configuration
# Location: .codeflow/scripts/shell-lib/config.sh

# Requires: common.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# ============================================================================
# CONFIGURATION PATHS
# ============================================================================

# Config directory
get_config_dir() {
    echo "$(find_repo_root)/.codeflow/config"
}

# Get config file path
get_config_path() {
    local config_name="$1"
    echo "$(get_config_dir)/${config_name}"
}

# ============================================================================
# JSON CONFIGURATION
# ============================================================================

# Read JSON config value (requires jq)
config_get_json() {
    local config_file="$1"
    local json_path="$2"
    local default="${3:-}"

    local full_path
    full_path=$(get_config_path "$config_file")

    if [[ -f "$full_path" ]] && command_exists jq; then
        local value
        value=$(jq -r "$json_path // empty" "$full_path" 2>/dev/null)
        echo "${value:-$default}"
    else
        echo "$default"
    fi
}

# Check if JSON key exists
config_has_json() {
    local config_file="$1"
    local json_path="$2"

    local full_path
    full_path=$(get_config_path "$config_file")

    if [[ -f "$full_path" ]] && command_exists jq; then
        jq -e "$json_path" "$full_path" &>/dev/null
    else
        return 1
    fi
}

# ============================================================================
# YAML CONFIGURATION
# ============================================================================

# Read YAML config value (requires yq or python)
config_get_yaml() {
    local config_file="$1"
    local yaml_path="$2"
    local default="${3:-}"

    local full_path
    full_path=$(get_config_path "$config_file")

    if [[ -f "$full_path" ]]; then
        if command_exists yq; then
            local value
            value=$(yq -r "$yaml_path // empty" "$full_path" 2>/dev/null)
            echo "${value:-$default}"
        elif command_exists python3; then
            local value
            value=$(python3 -c "
import yaml, sys
with open(sys.argv[1]) as f:
    data = yaml.safe_load(f)
path = sys.argv[2].lstrip('.').split('.')
for p in path:
    if p and data:
        data = data.get(p)
print(data if data else '')
" "$full_path" "$yaml_path" 2>/dev/null)
            echo "${value:-$default}"
        else
            echo "$default"
        fi
    else
        echo "$default"
    fi
}

# ============================================================================
# ENVIRONMENT CONFIGURATION
# ============================================================================

# Load environment file
load_env_file() {
    local env_file="$1"
    if [[ -f "$env_file" ]]; then
        while IFS='=' read -r key value; do
            # Skip comments and empty lines
            [[ "$key" =~ ^[[:space:]]*# ]] && continue
            [[ -z "$key" ]] && continue
            # Export non-empty values
            key=$(trim "$key")
            value=$(trim "$value")
            [[ -n "$key" && -n "$value" ]] && export "$key"="$value"
        done < "$env_file"
    fi
}

# ============================================================================
# COMMON CONFIGURATIONS
# ============================================================================

# Get enforcement policy value
get_enforcement_policy() {
    local key="$1"
    local default="${2:-}"
    config_get_json "enforcement/enforcement-policy.json" ".$key" "$default"
}

# Get work graph config
get_work_graph_config() {
    local key="$1"
    local default="${2:-}"
    config_get_yaml "work-graph.yaml" ".$key" "$default"
}

# Check if feature is enabled
is_feature_enabled() {
    local feature="$1"
    local value
    value=$(get_enforcement_policy "$feature" "false")
    [[ "$value" == "true" ]]
}
