#!/usr/bin/env bash
# CodeFlow Shell Library: Validation
# Location: .codeflow/scripts/shell-lib/validation.sh

# Requires: common.sh, errors.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
[[ -z "${EXIT_INVALID_INPUT:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/errors.sh"

# ============================================================================
# STRING VALIDATION
# ============================================================================

# Check if string is empty
is_empty() {
    [[ -z "$1" ]]
}

# Check if string is not empty
is_not_empty() {
    [[ -n "$1" ]]
}

# Check if string matches regex
matches_regex() {
    local string="$1"
    local pattern="$2"
    [[ "$string" =~ $pattern ]]
}

# Check if string is alphanumeric
is_alphanumeric() {
    [[ "$1" =~ ^[a-zA-Z0-9]+$ ]]
}

# Check if string is numeric
is_numeric() {
    [[ "$1" =~ ^[0-9]+$ ]]
}

# ============================================================================
# PATH VALIDATION
# ============================================================================

# Check if path is safe (no traversal)
is_safe_path() {
    local path="$1"

    # Reject path traversal
    [[ "$path" != *".."* ]] || return 1

    # Reject absolute paths starting with /
    [[ "$path" != /* ]] || return 1

    # Note: Null byte check removed - shell strings cannot contain null bytes
    # (strings are null-terminated in C, which bash uses internally)

    return 0
}

# Validate file path
validate_file_path() {
    local path="$1"
    local message="${2:-Invalid file path}"

    is_safe_path "$path" || die "$message: path traversal detected" $EXIT_INVALID_INPUT
}

# ============================================================================
# ID VALIDATION
# ============================================================================

# Validate ULID format (26 chars, base32)
is_valid_ulid() {
    local id="$1"
    [[ "$id" =~ ^[0-9A-HJKMNP-TV-Z]{26}$ ]]
}

# Validate epic ID format (EPC-ULID)
is_valid_epic_id() {
    local id="$1"
    [[ "$id" =~ ^EPC-[0-9A-HJKMNP-TV-Z]{26}$ ]]
}

# Validate task ID format (TSK-ULID)
is_valid_task_id() {
    local id="$1"
    [[ "$id" =~ ^TSK-[0-9A-HJKMNP-TV-Z]{26}$ ]]
}

# ============================================================================
# BRANCH VALIDATION
# ============================================================================

# Valid branch prefixes
readonly VALID_BRANCH_PREFIXES=(
    "feat/"
    "fix/"
    "refactor/"
    "docs/"
    "test/"
    "chore/"
    "plan/"
    "ops/"
    "deploy/"
)

# Check if branch name is valid
is_valid_branch_name() {
    local branch="$1"

    for prefix in "${VALID_BRANCH_PREFIXES[@]}"; do
        if starts_with "$branch" "$prefix"; then
            return 0
        fi
    done

    return 1
}

# Get branch prefix
get_branch_prefix() {
    local branch="$1"

    for prefix in "${VALID_BRANCH_PREFIXES[@]}"; do
        if starts_with "$branch" "$prefix"; then
            echo "${prefix%/}"
            return 0
        fi
    done

    echo ""
    return 1
}

# ============================================================================
# JSON VALIDATION
# ============================================================================

# Check if string is valid JSON
is_valid_json() {
    local string="$1"

    if command_exists jq; then
        echo "$string" | jq -e . &>/dev/null
    else
        # Fallback: basic check
        [[ "$string" == "{"*"}" || "$string" == "["*"]" ]]
    fi
}

# Validate JSON file
validate_json_file() {
    local file="$1"
    local message="${2:-Invalid JSON file}"

    assert_file_exists "$file"

    if command_exists jq; then
        jq -e . "$file" &>/dev/null || die "$message: $file" $EXIT_INVALID_INPUT
    fi
}

# ============================================================================
# COMMAND VALIDATION
# ============================================================================

# Check if command is dangerous
is_dangerous_command() {
    local cmd="$1"

    # List of dangerous patterns
    local dangerous_patterns=(
        "rm -rf /"
        "rm -rf /*"
        "sudo rm"
        "> /dev/sd"
        "mkfs."
        "dd if="
        ":(){:|:&};:"
    )

    for pattern in "${dangerous_patterns[@]}"; do
        if contains "$cmd" "$pattern"; then
            return 0
        fi
    done

    return 1
}
