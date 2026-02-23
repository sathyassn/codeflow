#!/usr/bin/env bash
# Purpose:   Validate task YAML frontmatter fields against schema rules
# Usage:     validate-task.sh [OPTIONS] <file-path>
# Arguments: file-path - Path to task markdown file with YAML frontmatter
# Exit codes:
#   0 - Valid file
#   1 - Invalid file (errors reported to stderr)

set -euo pipefail

# =============================================================================
# CONSTANTS
# =============================================================================

readonly SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly VERSION="1.0.0"

readonly REQUIRED_FIELDS="id format_id epic_id title status area_type work_type"
readonly VALID_STATUSES="todo blocked in_progress awaiting_review complete cancelled"
readonly VALID_WORK_TYPES="FEAT FIX HTFX RFCT DOCS TEST CHOR CICD SPKE PLAN"
readonly CODE_WORK_TYPES="FEAT FIX RFCT HTFX CHOR CICD TEST"
readonly FORMAT_ID_PATTERN='^[A-Z]{2,4}-TSK-[0-9]{3}-[0-9]{3}$'

# =============================================================================
# HELP AND VERSION
# =============================================================================

usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS] <file-path>

Validate task YAML frontmatter fields against schema rules.

Arguments:
    file-path    Path to task markdown file with YAML frontmatter

Options:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress info messages, only show errors

Exit Codes:
    0    Valid file
    1    Invalid file (errors reported to stderr)

Examples:
    $SCRIPT_NAME project-management/epics/INF/INF-EPC-008/tasks/INF-TSK-008-001.md
    $SCRIPT_NAME --quiet task.md
EOF
}

show_version() {
    echo "$SCRIPT_NAME version $VERSION"
}

# =============================================================================
# UTILITY FUNCTIONS
# =============================================================================

info() {
    if [[ "$QUIET" == "false" ]]; then
        echo "[INFO] $*"
    fi
}

error() {
    echo "[ERROR] $*" >&2
}

warn() {
    echo "[WARN] $*" >&2
}

# Extract a field value from YAML frontmatter content
# Uses grep+sed to find "field: value" or "field: \"value\""
get_field() {
    local field="$1"
    local content="$2"
    local value

    # Match field: value (with optional quotes)
    value=$(echo "$content" | grep -E "^${field}:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' | sed 's/^"//' | sed 's/"$//' | sed "s/^'//" | sed "s/'$//" | sed 's/[[:space:]]*$//')

    # Treat "null", "~", and empty as null
    if [[ -z "$value" || "$value" == "null" || "$value" == "~" ]]; then
        echo ""
    else
        echo "$value"
    fi
}

# Load protected branches from enforcement-policy.json
load_protected_branches() {
    local repo_root
    repo_root="$(cd "$SCRIPT_DIR/../../.." && pwd)"
    local config_file="$repo_root/.codeflow/config/enforcement/enforcement-policy.json"

    if [[ -f "$config_file" ]]; then
        # Extract protected_branches array values using grep+sed
        # Look for merge_protection.protected_branches array
        local in_section="false"
        local branches=""
        while IFS= read -r line; do
            if echo "$line" | grep -q '"merge_protection"'; then
                in_section="true"
            fi
            if [[ "$in_section" == "true" ]] && echo "$line" | grep -q '"protected_branches"'; then
                in_section="in_array"
            fi
            if [[ "$in_section" == "in_array" ]]; then
                local branch_val
                branch_val=$(echo "$line" | sed -n 's/.*"\([^"]*\)".*/\1/p')
                if [[ -n "$branch_val" && "$branch_val" != "protected_branches" ]]; then
                    branches="$branches $branch_val"
                fi
                if echo "$line" | grep -q ']'; then
                    break
                fi
            fi
        done < "$config_file"

        if [[ -n "$branches" ]]; then
            echo "$branches"
            return
        fi
    fi

    # Default if file not found or parsing fails
    echo "main master"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

QUIET="false"

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                show_version
                exit 0
                ;;
            -q|--quiet)
                QUIET="true"
                shift
                ;;
            -*)
                error "Unknown option: $1"
                echo "Run '$SCRIPT_NAME --help' for usage." >&2
                exit 1
                ;;
            *)
                break
                ;;
        esac
    done

    if [[ $# -lt 1 ]]; then
        error "Missing required argument: file-path"
        echo "Usage: $SCRIPT_NAME <file-path>" >&2
        exit 1
    fi

    local file_path="$1"

    # ==========================================================================
    # FILE VALIDATION
    # ==========================================================================

    if [[ ! -f "$file_path" ]]; then
        error "File not found: $file_path"
        exit 1
    fi

    # ==========================================================================
    # EXTRACT FRONTMATTER
    # ==========================================================================

    # Extract content between first --- and second ---
    local frontmatter=""
    local in_frontmatter="false"
    local found_start="false"
    local line_num=0

    while IFS= read -r line; do
        line_num=$((line_num + 1))
        if [[ "$line" == "---" ]]; then
            if [[ "$found_start" == "false" ]]; then
                found_start="true"
                in_frontmatter="true"
                continue
            else
                in_frontmatter="false"
                break
            fi
        fi
        if [[ "$in_frontmatter" == "true" ]]; then
            frontmatter="$frontmatter
$line"
        fi
    done < "$file_path"

    # Trim leading newline
    frontmatter="${frontmatter#
}"

    if [[ -z "$frontmatter" ]]; then
        error "No frontmatter fields found in $file_path"
        exit 1
    fi

    info "Validating: $file_path"

    # ==========================================================================
    # VALIDATION
    # ==========================================================================

    local ERRORS=0
    local WARNINGS=0

    # Check required fields
    local field
    for field in $REQUIRED_FIELDS; do
        local value
        value=$(get_field "$field" "$frontmatter")
        if [[ -z "$value" ]]; then
            error "Missing required field: $field"
            ERRORS=$((ERRORS + 1))
        fi
    done

    # Validate format_id pattern
    local format_id
    format_id=$(get_field "format_id" "$frontmatter")
    if [[ -n "$format_id" ]]; then
        if ! echo "$format_id" | grep -Eq "$FORMAT_ID_PATTERN"; then
            error "Invalid format_id '$format_id': must match $FORMAT_ID_PATTERN"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate status value
    local status
    status=$(get_field "status" "$frontmatter")
    if [[ -n "$status" ]]; then
        local valid="false"
        local s
        for s in $VALID_STATUSES; do
            if [[ "$status" == "$s" ]]; then
                valid="true"
                break
            fi
        done
        if [[ "$valid" == "false" ]]; then
            error "Invalid status '$status': must be one of: $VALID_STATUSES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate auto_merge + target_branch constraint
    local auto_merge
    auto_merge=$(get_field "auto_merge" "$frontmatter")
    local target_branch
    target_branch=$(get_field "target_branch" "$frontmatter")

    if [[ "$auto_merge" == "true" ]]; then
        if [[ -z "$target_branch" ]]; then
            error "auto_merge is true but target_branch is null: target_branch must be set when auto_merge is enabled"
            ERRORS=$((ERRORS + 1))
        else
            # Check if target_branch is in protected_branches
            local protected_branches
            protected_branches=$(load_protected_branches)
            local pb
            for pb in $protected_branches; do
                if [[ "$target_branch" == "$pb" ]]; then
                    error "auto_merge is true but target_branch '$target_branch' is a protected branch"
                    ERRORS=$((ERRORS + 1))
                    break
                fi
            done
        fi
    fi

    # Validate raise_pr + auto_merge constraint
    local raise_pr
    raise_pr=$(get_field "raise_pr" "$frontmatter")
    if [[ "$raise_pr" == "false" && "$auto_merge" == "true" ]]; then
        error "raise_pr is false but auto_merge is true: auto_merge requires raise_pr to be true"
        ERRORS=$((ERRORS + 1))
    fi

    # Validate work_type enum
    local work_type
    work_type=$(get_field "work_type" "$frontmatter")
    if [[ -n "$work_type" ]]; then
        local valid_wt="false"
        local wt
        for wt in $VALID_WORK_TYPES; do
            if [[ "$work_type" == "$wt" ]]; then
                valid_wt="true"
                break
            fi
        done
        if [[ "$valid_wt" == "false" ]]; then
            error "Invalid work_type '$work_type': must be one of: $VALID_WORK_TYPES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate tests field for code-producing work types
    if [[ -n "$work_type" ]]; then
        local is_code_type="false"
        local ct
        for ct in $CODE_WORK_TYPES; do
            if [[ "$work_type" == "$ct" ]]; then
                is_code_type="true"
                break
            fi
        done

        if [[ "$is_code_type" == "true" ]]; then
            # Check if file_scope contains .sh or .py files
            local file_scope
            file_scope=$(echo "$frontmatter" | grep -E "^file_scope:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' || true)
            local has_code_files="false"
            if [[ -n "$file_scope" && "$file_scope" != "[]" && "$file_scope" != "null" && "$file_scope" != "~" ]]; then
                if echo "$file_scope" | grep -qE '\.(sh|py)'; then
                    has_code_files="true"
                fi
            fi

            if [[ "$has_code_files" == "true" ]]; then
                local tests_field
                tests_field=$(echo "$frontmatter" | grep -E "^tests:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' || true)
                if [[ -z "$tests_field" || "$tests_field" == "[]" || "$tests_field" == "null" || "$tests_field" == "~" ]]; then
                    warn "Tasks modifying code files should have tests defined in the 'tests' field"
                    WARNINGS=$((WARNINGS + 1))
                fi
            fi
        fi
    fi

    # Validate acceptance field when autorun_eligible is true
    local autorun_eligible
    autorun_eligible=$(get_field "autorun_eligible" "$frontmatter")
    if [[ "$autorun_eligible" == "true" ]]; then
        local acceptance_field
        acceptance_field=$(echo "$frontmatter" | grep -E "^acceptance:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' || true)
        if [[ -z "$acceptance_field" || "$acceptance_field" == "[]" || "$acceptance_field" == "null" || "$acceptance_field" == "~" ]]; then
            error "autorun_eligible is true but acceptance is empty: acceptance criteria are required for autorun tasks"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # ==========================================================================
    # RESULT
    # ==========================================================================

    if [[ $ERRORS -gt 0 ]]; then
        error "Validation FAILED with $ERRORS error(s)"
        exit 1
    fi

    if [[ $WARNINGS -gt 0 ]]; then
        info "Validation PASSED with $WARNINGS warning(s)"
    else
        info "Validation PASSED"
    fi
    exit 0
}

main "$@"
