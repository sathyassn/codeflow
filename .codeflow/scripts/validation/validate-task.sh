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
readonly VALID_STATUSES="todo blocked in_progress complete cancelled"
readonly VALID_WORK_TYPES="FEAT FIX HTFX RFCT DOCS TEST CHOR CICD SPKE PLAN"
readonly VALID_AREA_TYPES="FRT BKD INF SHR DOC PLN"
readonly VALID_ORIGINS="planned informal auto"
readonly VALID_SCOPE_POLICIES="soft hard permissive"
readonly VALID_PRIORITIES="low normal high critical"
readonly VALID_ESTIMATES="XS S M L XL"
readonly VALID_STAGES="dev review qa done"
readonly VALID_STAGE_STATUSES="pending in_progress complete failed"
readonly CODE_WORK_TYPES="FEAT FIX RFCT HTFX CHOR CICD TEST"
readonly PII_FILE_PATTERNS="auth login user session password credential token account profile identity"
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

    # Validate id field does not contain placeholder or template sentinel
    local id_value
    id_value=$(get_field "id" "$frontmatter")
    if [[ -n "$id_value" ]]; then
        local id_upper
        id_upper=$(echo "$id_value" | tr '[:lower:]' '[:upper:]')
        if [[ "$id_upper" == *"PLACEHOLDER"* ]]; then
            error "id field contains placeholder value — assign a real task ID"
            ERRORS=$((ERRORS + 1))
        fi
        if [[ "$id_value" == *"{"* || "$id_value" == *"}"* ]]; then
            error "id field contains unfilled template sentinel '$id_value' — assign a real task ID"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate title field does not contain template sentinel
    local title_value
    title_value=$(get_field "title" "$frontmatter")
    if [[ -n "$title_value" ]]; then
        if [[ "$title_value" == *"{"* || "$title_value" == *"}"* ]]; then
            error "title field contains unfilled template sentinel '$title_value' — assign a real title"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate epic_id field does not contain placeholder or template sentinel
    local epic_id_value
    epic_id_value=$(get_field "epic_id" "$frontmatter")
    if [[ -n "$epic_id_value" ]]; then
        local epic_id_upper
        epic_id_upper=$(echo "$epic_id_value" | tr '[:lower:]' '[:upper:]')
        if [[ "$epic_id_upper" == *"PLACEHOLDER"* ]]; then
            error "epic_id field contains placeholder value — assign a real epic ID"
            ERRORS=$((ERRORS + 1))
        fi
        if [[ "$epic_id_value" == *"{"* || "$epic_id_value" == *"}"* ]]; then
            error "epic_id field contains unfilled template sentinel '$epic_id_value' — assign a real epic ID"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate format_id pattern
    local format_id
    format_id=$(get_field "format_id" "$frontmatter")
    if [[ -n "$format_id" ]]; then
        if ! echo "$format_id" | grep -Eq "$FORMAT_ID_PATTERN"; then
            error "Invalid format_id '$format_id': must match $FORMAT_ID_PATTERN"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate area_type enum
    local area_type
    area_type=$(get_field "area_type" "$frontmatter")
    if [[ -n "$area_type" ]]; then
        local valid_at="false"
        local at
        for at in $VALID_AREA_TYPES; do
            if [[ "$area_type" == "$at" ]]; then
                valid_at="true"
                break
            fi
        done
        if [[ "$valid_at" == "false" ]]; then
            error "Invalid area_type '$area_type': must be one of: $VALID_AREA_TYPES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate format_id prefix matches area_type
    if [[ -n "$format_id" && -n "$area_type" ]]; then
        local format_prefix
        format_prefix=$(echo "$format_id" | sed 's/-TSK-.*//')
        if [[ "$format_prefix" != "$area_type" ]]; then
            error "format_id prefix '$format_prefix' does not match area_type '$area_type'"
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

    # Validate origin enum (if present)
    local origin
    origin=$(get_field "origin" "$frontmatter")
    if [[ -n "$origin" ]]; then
        local valid_or="false"
        local or_val
        for or_val in $VALID_ORIGINS; do
            if [[ "$origin" == "$or_val" ]]; then
                valid_or="true"
                break
            fi
        done
        if [[ "$valid_or" == "false" ]]; then
            error "Invalid origin '$origin': must be one of: $VALID_ORIGINS"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate scope_policy enum (if present)
    local scope_policy
    scope_policy=$(get_field "scope_policy" "$frontmatter")
    if [[ -n "$scope_policy" ]]; then
        local valid_sp="false"
        local sp
        for sp in $VALID_SCOPE_POLICIES; do
            if [[ "$scope_policy" == "$sp" ]]; then
                valid_sp="true"
                break
            fi
        done
        if [[ "$valid_sp" == "false" ]]; then
            error "Invalid scope_policy '$scope_policy': must be one of: $VALID_SCOPE_POLICIES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate priority enum (if present)
    local priority
    priority=$(get_field "priority" "$frontmatter")
    if [[ -n "$priority" ]]; then
        local valid_pr="false"
        local pr
        for pr in $VALID_PRIORITIES; do
            if [[ "$priority" == "$pr" ]]; then
                valid_pr="true"
                break
            fi
        done
        if [[ "$valid_pr" == "false" ]]; then
            error "Invalid priority '$priority': must be one of: $VALID_PRIORITIES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate estimate enum (if present)
    local estimate
    estimate=$(get_field "estimate" "$frontmatter")
    if [[ -n "$estimate" ]]; then
        local valid_est="false"
        local est
        for est in $VALID_ESTIMATES; do
            if [[ "$estimate" == "$est" ]]; then
                valid_est="true"
                break
            fi
        done
        if [[ "$valid_est" == "false" ]]; then
            error "Invalid estimate '$estimate': must be one of: $VALID_ESTIMATES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate stage enum (if present)
    local stage
    stage=$(get_field "stage" "$frontmatter")
    if [[ -n "$stage" ]]; then
        local valid_stg="false"
        local stg
        for stg in $VALID_STAGES; do
            if [[ "$stage" == "$stg" ]]; then
                valid_stg="true"
                break
            fi
        done
        if [[ "$valid_stg" == "false" ]]; then
            error "Invalid stage '$stage': must be one of: $VALID_STAGES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate stage_status enum (if present)
    local stage_status
    stage_status=$(get_field "stage_status" "$frontmatter")
    if [[ -n "$stage_status" ]]; then
        local valid_ss="false"
        local ss
        for ss in $VALID_STAGE_STATUSES; do
            if [[ "$stage_status" == "$ss" ]]; then
                valid_ss="true"
                break
            fi
        done
        if [[ "$valid_ss" == "false" ]]; then
            error "Invalid stage_status '$stage_status': must be one of: $VALID_STAGE_STATUSES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate boolean syntax for autorun_eligible, raise_pr, auto_merge
    local bool_field
    for bool_field in autorun_eligible raise_pr auto_merge; do
        local bool_raw
        bool_raw=$(echo "$frontmatter" | grep -E "^${bool_field}:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' | sed 's/[[:space:]]*$//' || true)
        if [[ -n "$bool_raw" && "$bool_raw" != "null" && "$bool_raw" != "~" ]]; then
            if [[ "$bool_raw" != "true" && "$bool_raw" != "false" ]]; then
                error "Invalid boolean value for $bool_field '$bool_raw': must be exactly 'true' or 'false'"
                ERRORS=$((ERRORS + 1))
            fi
        fi
    done

    # Validate tests field for code-producing work types (ERROR level)
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
                    error "Code-producing task with code files must have tests defined in the 'tests' field"
                    ERRORS=$((ERRORS + 1))
                fi
            fi
        fi
    fi

    # Validate filename matches format_id (warning only)
    if [[ -n "$format_id" ]]; then
        local basename
        basename=$(basename "$file_path" .md)
        if [[ "$basename" != "$format_id" ]]; then
            warn "Filename '$basename.md' does not match format_id '$format_id'"
            WARNINGS=$((WARNINGS + 1))
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

    # Check file_scope for PII-handling file patterns (warning)
    local file_scope_raw
    file_scope_raw=$(echo "$frontmatter" | grep -E "^file_scope:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' || true)
    if [[ -n "$file_scope_raw" && "$file_scope_raw" != "[]" && "$file_scope_raw" != "null" && "$file_scope_raw" != "~" ]]; then
        local file_scope_lower
        file_scope_lower=$(echo "$file_scope_raw" | tr '[:upper:]' '[:lower:]')
        local pii_pattern
        for pii_pattern in $PII_FILE_PATTERNS; do
            if echo "$file_scope_lower" | grep -q "$pii_pattern"; then
                warn "file_scope contains PII-sensitive path pattern '$pii_pattern' — verify PII handling compliance (encryption, hashing, sanitization, logging redaction) per OWASP/industry standards"
                WARNINGS=$((WARNINGS + 1))
                break
            fi
        done
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
