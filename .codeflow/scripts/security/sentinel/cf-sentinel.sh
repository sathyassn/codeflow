#!/usr/bin/env bash
# Sentinel Library for Skills Enforcement
# Location: .codeflow/scripts/security/sentinel/cf-sentinel.sh
#
# Purpose: Shared functions for sentinel creation, validation, and cleanup.
# Sentinels are file tokens proving skill invocation before protected operations.
#
# Usage: source "$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"
#
# Configuration: Reads from enforcement-policy.json in .codeflow/config/.
#                Falls back to defaults if config is missing.
#
# Compatibility: bash 3.2+ (macOS compatible)

# Resolve paths - script may be sourced from different locations
# Use git rev-parse (most robust) or fallback to relative path from script location
SENTINEL_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
SENTINEL_CONFIG="${SENTINEL_REPO_ROOT}/.codeflow/config/enforcement/enforcement-policy.json"

# Fallback defaults (used if config is missing or invalid)
SENTINEL_DIR_DEFAULT="/tmp/claude/managed/sentinels"
SENTINEL_TTL_DEFAULT=600  # 10 minutes - matches config default for consistency

# Will be set by sentinel_load_config()
SENTINEL_DIR=""
SENTINEL_TTL=""
SENTINEL_CONFIG_LOADED=""

sentinel_load_config() {
    # Load configuration from enforcement-policy.json
    # Sets SENTINEL_DIR and SENTINEL_TTL from config, with fallback to defaults

    # Skip if already loaded
    [ -n "$SENTINEL_CONFIG_LOADED" ] && return 0

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        # Load from config
        local dir ttl
        dir=$(jq -r '.sentinel.directory // empty' "$SENTINEL_CONFIG" 2>/dev/null)
        ttl=$(jq -r '.sentinel.default_ttl // empty' "$SENTINEL_CONFIG" 2>/dev/null)

        # Use config values or fall back to defaults
        SENTINEL_DIR="${dir:-$SENTINEL_DIR_DEFAULT}"

        # Validate TTL is a number
        if [[ "$ttl" =~ ^[0-9]+$ ]]; then
            SENTINEL_TTL="$ttl"
        else
            SENTINEL_TTL="$SENTINEL_TTL_DEFAULT"
        fi

        SENTINEL_CONFIG_LOADED="true"
    else
        # Config not found or jq not available - use defaults
        SENTINEL_DIR="$SENTINEL_DIR_DEFAULT"
        SENTINEL_TTL="$SENTINEL_TTL_DEFAULT"
        SENTINEL_CONFIG_LOADED="fallback"

        if [ ! -f "$SENTINEL_CONFIG" ]; then
            echo "Warning: Config not found at $SENTINEL_CONFIG, using defaults" >&2
        fi
    fi
}

sentinel_get_operation_ttl() {
    # Get TTL for a specific skill operation from config
    # Falls back to default TTL if not specified
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        local op_ttl
        op_ttl=$(jq -r ".skills[\"$skill\"].operations[\"$operation\"].ttl // empty" "$SENTINEL_CONFIG" 2>/dev/null)

        if [[ "$op_ttl" =~ ^[0-9]+$ ]]; then
            echo "$op_ttl"
            return 0
        fi
    fi

    # Return default TTL
    echo "$SENTINEL_TTL"
}

sentinel_get_operation_pattern() {
    # Get pattern for a specific skill operation from config
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].operations[\"$operation\"].pattern // empty" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_get_skill_operations() {
    # Get all operation names for a skill from config
    local skill="$1"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].operations | keys[]" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_get_all_skills() {
    # Get all skill names from config
    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r '.skills | keys[]' "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_get_skill_exclude_patterns() {
    # Get exclude_patterns array for a skill from config
    # Returns newline-separated patterns, empty if none defined
    local skill="$1"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].exclude_patterns // [] | .[]" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_matches_exclude_pattern() {
    # Check if a file path matches any exclude pattern for a skill
    # Returns 0 (true) if file should be excluded, 1 (false) otherwise
    local skill="$1"
    local file_path="$2"

    local exclude_patterns
    exclude_patterns=$(sentinel_get_skill_exclude_patterns "$skill")
    [ -z "$exclude_patterns" ] && return 1  # No patterns = not excluded

    while IFS= read -r pattern; do
        [ -z "$pattern" ] && continue
        if echo "$file_path" | grep -qE "$pattern"; then
            return 0  # File matches exclude pattern
        fi
    done <<< "$exclude_patterns"

    return 1  # No exclude pattern matched
}

sentinel_get_operation_tool() {
    # Get the tool type (Bash, Write, etc.) for a specific operation
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].operations[\"$operation\"].tool // empty" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_get_operation_prerequisite() {
    # Get prerequisite operation for a specific skill operation from config
    # Returns empty string if no prerequisite defined
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].operations[\"$operation\"].prerequisite // empty" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_find_by_operation() {
    # Find a valid (non-expired) sentinel for a specific skill:operation pair
    # Unlike sentinel_validate, this matches by operation name, not by pattern
    # Returns 0 if found, 1 if not found
    #
    # Usage: sentinel_find_by_operation "memory-management" "search-related-work"
    local skill="$1"
    local operation="$2"

    sentinel_init
    local now
    now=$(date +%s)

    # Look for sentinel files matching skill:operation-*.json pattern
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        [ -f "$file" ] || continue

        local expires
        local file_operation
        expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
        file_operation=$(jq -r '.operation // ""' "$file" 2>/dev/null)

        # Validate expires is a number
        if ! [[ "$expires" =~ ^[0-9]+$ ]]; then
            continue
        fi

        # Check operation matches
        [ "$file_operation" != "$operation" ] && continue

        # Check not expired
        if [ "$now" -lt "$expires" ]; then
            return 0  # Valid sentinel found for this operation
        fi
    done < <(find "$SENTINEL_DIR" -maxdepth 1 -name "${skill}:${operation}-*.json" -type f 2>/dev/null)

    return 1  # No valid sentinel for this operation
}

sentinel_find_skill_for_command() {
    # Find which skill is required for a given command (Bash tool input)
    # Iterates over all skills/operations in config to find matching pattern
    # Returns skill name via echo, empty if no match
    local cmd="$1"
    local tool_type="${2:-Bash}"  # Default to Bash for backward compatibility

    sentinel_load_config

    if [ ! -f "$SENTINEL_CONFIG" ] || ! command -v jq &>/dev/null; then
        return 1
    fi

    # Get all skills
    local skills
    skills=$(sentinel_get_all_skills)
    [ -z "$skills" ] && return 1

    # Check each skill's operations
    while IFS= read -r skill; do
        [ -z "$skill" ] && continue

        local operations
        operations=$(sentinel_get_skill_operations "$skill")
        [ -z "$operations" ] && continue

        while IFS= read -r operation; do
            [ -z "$operation" ] && continue

            # Check if this operation matches the tool type
            local op_tool
            op_tool=$(sentinel_get_operation_tool "$skill" "$operation")
            [ "$op_tool" != "$tool_type" ] && continue

            # Check if pattern matches
            local pattern
            pattern=$(sentinel_get_operation_pattern "$skill" "$operation")
            [ -z "$pattern" ] && continue

            if echo "$cmd" | grep -qE "$pattern"; then
                echo "$skill"
                return 0
            fi
        done <<< "$operations"
    done <<< "$skills"

    return 1
}

sentinel_find_skill_for_file() {
    # Find which skill is required for a file operation (Write/Edit tool input)
    # Iterates over all skills/operations in config to find matching pattern
    # Returns the MOST SPECIFIC match (longest pattern) when multiple match
    # Returns: skill:operation via echo, empty if no match
    #
    # Usage: result=$(sentinel_find_skill_for_file "/path/to/file.md" "Write")
    #        skill="${result%%:*}"
    #        operation="${result#*:}"
    local file_path="$1"
    local tool_type="${2:-Write}"

    sentinel_load_config

    if [ ! -f "$SENTINEL_CONFIG" ] || ! command -v jq &>/dev/null; then
        return 1
    fi

    # Get all skills
    local skills
    skills=$(sentinel_get_all_skills)
    [ -z "$skills" ] && return 1

    # Track best match (longest pattern = most specific)
    local best_match=""
    local best_pattern_len=0

    # Check each skill's operations
    while IFS= read -r skill; do
        [ -z "$skill" ] && continue

        # Check if file matches skill's exclude_patterns - skip if excluded
        if sentinel_matches_exclude_pattern "$skill" "$file_path"; then
            continue  # File is excluded from this skill's enforcement
        fi

        local operations
        operations=$(sentinel_get_skill_operations "$skill")
        [ -z "$operations" ] && continue

        while IFS= read -r operation; do
            [ -z "$operation" ] && continue

            # Check if this operation matches the tool type
            local op_tool
            op_tool=$(sentinel_get_operation_tool "$skill" "$operation")
            [ "$op_tool" != "$tool_type" ] && continue

            # Check if pattern matches file path
            local pattern
            pattern=$(sentinel_get_operation_pattern "$skill" "$operation")
            [ -z "$pattern" ] && continue

            if echo "$file_path" | grep -qE "$pattern"; then
                # Track longest pattern (most specific match)
                local pattern_len=${#pattern}
                if [ "$pattern_len" -gt "$best_pattern_len" ]; then
                    best_match="${skill}:${operation}"
                    best_pattern_len=$pattern_len
                fi
            fi
        done <<< "$operations"
    done <<< "$skills"

    if [ -n "$best_match" ]; then
        echo "$best_match"
        return 0
    fi

    return 1
}

sentinel_find_skill_for_grep() {
    # Find which skill is required for a Grep operation
    # Checks both file pattern and content pattern from config
    # Returns: skill:operation via echo, empty if no match
    #
    # Usage: result=$(sentinel_find_skill_for_grep "/path/to/file.py" "def my_func")
    #        skill="${result%%:*}"
    #        operation="${result#*:}"
    local file_path="$1"
    local content_pattern="$2"

    sentinel_load_config

    if [ ! -f "$SENTINEL_CONFIG" ] || ! command -v jq &>/dev/null; then
        return 1
    fi

    # Get all skills
    local skills
    skills=$(sentinel_get_all_skills)
    [ -z "$skills" ] && return 1

    # Check each skill's operations
    while IFS= read -r skill; do
        [ -z "$skill" ] && continue

        local operations
        operations=$(sentinel_get_skill_operations "$skill")
        [ -z "$operations" ] && continue

        while IFS= read -r operation; do
            [ -z "$operation" ] && continue

            # Check if this operation is for Grep tool
            local op_tool
            op_tool=$(sentinel_get_operation_tool "$skill" "$operation")
            [ "$op_tool" != "Grep" ] && continue

            # Check if file pattern matches
            local file_pattern
            file_pattern=$(sentinel_get_operation_pattern "$skill" "$operation")
            [ -z "$file_pattern" ] && continue

            if ! echo "$file_path" | grep -qE "$file_pattern"; then
                continue
            fi

            # Check content pattern if specified in config
            local cfg_content_pattern
            cfg_content_pattern=$(jq -r ".skills[\"$skill\"].operations[\"$operation\"].content_pattern // empty" "$SENTINEL_CONFIG" 2>/dev/null)

            if [ -n "$cfg_content_pattern" ]; then
                # Content pattern specified - check if search pattern matches
                if echo "$content_pattern" | grep -qE "$cfg_content_pattern"; then
                    echo "${skill}:${operation}"
                    return 0
                fi
            else
                # No content pattern - file pattern match is sufficient
                echo "${skill}:${operation}"
                return 0
            fi
        done <<< "$operations"
    done <<< "$skills"

    return 1
}

sentinel_get_operation_guidance() {
    # Get guidance message for a specific skill operation
    # Returns guidance text if defined, empty otherwise
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        jq -r ".skills[\"$skill\"].operations[\"$operation\"].guidance // empty" "$SENTINEL_CONFIG" 2>/dev/null
    fi
}

sentinel_is_new_file_only() {
    # Check if an operation is only for new file creation
    # Returns 0 (true) if new_file_only is true, 1 (false) otherwise
    local skill="$1"
    local operation="$2"

    sentinel_load_config

    if [ -f "$SENTINEL_CONFIG" ] && command -v jq &>/dev/null; then
        local value
        value=$(jq -r ".skills[\"$skill\"].operations[\"$operation\"].new_file_only // false" "$SENTINEL_CONFIG" 2>/dev/null)
        [ "$value" = "true" ] && return 0
    fi
    return 1
}

sentinel_init() {
    sentinel_load_config
    mkdir -p "$SENTINEL_DIR"
}

sentinel_create() {
    local skill="$1"
    local operation="$2"
    local tool_pattern="$3"
    local ttl="${4:-$SENTINEL_TTL}"

    sentinel_init

    # Portable ID generation: uuidgen (macOS/Linux), /proc (Linux), fallback to timestamp+PID
    local id
    id=$(uuidgen 2>/dev/null || cat /proc/sys/kernel/random/uuid 2>/dev/null || echo "$$-$(date +%s)")
    local filename="${skill}:${operation}-${id}.json"
    local created
    created=$(date +%s)
    local expires=$((created + ttl))

    # Escape backslashes for valid JSON (\ -> \\)
    local json_pattern="${tool_pattern//\\/\\\\}"

    cat > "$SENTINEL_DIR/$filename" << EOF
{
  "sentinel": "${skill}:${operation}",
  "skill": "$skill",
  "operation": "$operation",
  "tool_pattern": "$json_pattern",
  "created": $created,
  "expires": $expires,
  "id": "$id"
}
EOF
    echo "$filename"
}

sentinel_validate() {
    # Validate sentinel exists and is not expired
    # Does NOT consume sentinel - sentinels remain valid until TTL expires
    local skill="$1"
    local tool_input="$2"

    sentinel_init
    local now
    now=$(date +%s)

    # Find matching sentinel for this skill
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        [ -f "$file" ] || continue

        local expires
        local tool_pattern
        expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
        tool_pattern=$(jq -r '.tool_pattern // ""' "$file" 2>/dev/null)

        # Validate expires is a number
        if ! [[ "$expires" =~ ^[0-9]+$ ]]; then
            continue
        fi

        # Check not expired
        [ "$now" -lt "$expires" ] || continue

        # Check pattern matches tool input (skip if pattern is empty)
        [ -z "$tool_pattern" ] && continue
        if echo "$tool_input" | grep -qE "$tool_pattern"; then
            return 0  # Valid sentinel found
        fi
    done < <(find "$SENTINEL_DIR" -maxdepth 1 -name "${skill}:*.json" -type f 2>/dev/null)

    return 1  # No valid sentinel
}

sentinel_exists() {
    # Alias for sentinel_validate (validate no longer consumes)
    sentinel_validate "$1" "$2"
}

sentinel_cleanup_skill() {
    # Remove ALL sentinels for a specific skill (fresh slate on re-invocation)
    local skill="$1"
    sentinel_init
    local cleaned=0

    for file in "$SENTINEL_DIR/${skill}:"*.json; do
        [ -f "$file" ] || continue
        rm -f "$file"
        ((cleaned++)) || true
    done

    echo "$cleaned"
}

sentinel_cleanup_all() {
    # Remove ALL sentinels regardless of expiry (for session boundaries)
    sentinel_init
    local cleaned=0

    for file in "$SENTINEL_DIR"/*.json; do
        [ -f "$file" ] || continue
        rm -f "$file"
        ((cleaned++)) || true
    done

    echo "$cleaned"
}

sentinel_cleanup_expired() {
    sentinel_init
    local now
    now=$(date +%s)
    local cleaned=0

    for file in "$SENTINEL_DIR"/*.json; do
        [ -f "$file" ] || continue

        local expires
        expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)

        # Validate expires is a number
        if ! [[ "$expires" =~ ^[0-9]+$ ]]; then
            rm -f "$file"
            ((cleaned++)) || true
            continue
        fi

        if [ "$now" -ge "$expires" ]; then
            rm -f "$file"
            ((cleaned++)) || true
        fi
    done

    echo "$cleaned"
}

sentinel_list() {
    sentinel_init
    local now
    now=$(date +%s)

    echo "Active Sentinels:"
    for file in "$SENTINEL_DIR"/*.json; do
        [ -f "$file" ] || continue

        local sentinel
        local expires
        sentinel=$(jq -r '.sentinel // "unknown"' "$file" 2>/dev/null)
        expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)

        # Validate expires is a number
        if ! [[ "$expires" =~ ^[0-9]+$ ]]; then
            echo "  $sentinel (INVALID)"
            continue
        fi

        local remaining=$((expires - now))

        if [ "$remaining" -gt 0 ]; then
            echo "  $sentinel (expires in ${remaining}s)"
        else
            echo "  $sentinel (EXPIRED)"
        fi
    done
}
