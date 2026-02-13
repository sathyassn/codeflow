#!/usr/bin/env bash
# Purpose:   List tracked worktrees
# Usage:     cf-worktree-list.sh [--format json|yaml|table] [--status <status>]
# Platform:  macOS/Linux
# Version:   1.0.0
#
# Query and display tracked worktrees from .state/worktrees.yaml
# and correlate with actual git worktrees.
#
# Options:
#   --format: Output format (json, yaml, table) - default: table
#   --status: Filter by status (active, stale, merged, abandoned)
#
# Exit codes:
#   - 0: Success
#   - 1: Error

set -euo pipefail

VERSION="1.0.0"
SCRIPT_NAME="$(basename "$0")"

# =============================================================================
# REPO ROOT DETECTION
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
${SCRIPT_NAME} - List tracked worktrees

USAGE:
    ${SCRIPT_NAME} [options]

OPTIONS:
    --format <f>   Output format: json, yaml, table (default: table)
    --status <s>   Filter by status: active, stale, merged, abandoned
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    ${SCRIPT_NAME}
    ${SCRIPT_NAME} --format yaml
    ${SCRIPT_NAME} --status active
    ${SCRIPT_NAME} --format json --status stale

SKILL REFERENCE:
    cf-git-workflow:create-worktree   Create a new worktree
    cf-git-workflow:cleanup-worktrees Remove stale worktrees
EOF
}

show_version() {
    echo "${SCRIPT_NAME} version ${VERSION}"
}

# =============================================================================
# OUTPUT FUNCTIONS
# =============================================================================

output_json() {
    local worktrees_file="$1"
    local filter_status="$2"

    if command -v yq &>/dev/null; then
        if [[ -n "$filter_status" ]]; then
            yq -o=json ".worktrees[] | select(.status == \"$filter_status\")" "$worktrees_file" 2>/dev/null || echo "[]"
        else
            yq -o=json "$worktrees_file" 2>/dev/null || echo "{}"
        fi
    else
        echo "Warning: yq not installed, showing raw YAML instead" >&2
        output_yaml "$worktrees_file" "$filter_status"
    fi
}

output_yaml() {
    local worktrees_file="$1"
    local filter_status="$2"

    if [[ -n "$filter_status" ]] && command -v yq &>/dev/null; then
        yq ".worktrees[] | select(.status == \"$filter_status\")" "$worktrees_file" 2>/dev/null || true
    else
        while IFS= read -r line; do
            echo "$line"
        done < "$worktrees_file"
    fi
}

output_table() {
    local worktrees_file="$1"
    local filter_status="$2"

    echo "=== Tracked Worktrees ==="
    echo ""
    printf "%-40s %-20s %-10s %s\n" "PATH" "BRANCH" "STATUS" "LAST ACTIVE"
    printf "%-40s %-20s %-10s %s\n" "----" "------" "------" "-----------"

    awk -v filter="$filter_status" '
    /^  - path:/ { path=$3; gsub(/"/, "", path) }
    /branch:/ { branch=$2; gsub(/"/, "", branch) }
    /status:/ { status=$2 }
    /last_active:/ {
        active=$2; gsub(/"/, "", active)
        if (filter == "" || status == filter) {
            printf "%-40s %-20s %-10s %s\n", path, branch, status, active
        }
    }
    ' "$worktrees_file"

    echo ""
    echo "=== Git Worktrees (actual) ==="
    git -C "$REPO_ROOT" worktree list 2>/dev/null || echo "Not in a git repository"
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    local format="table"
    local filter_status=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                show_help
                exit 0
                ;;
            -V|--version)
                show_version
                exit 0
                ;;
            --format)
                if [[ $# -lt 2 ]]; then
                    echo "Error: --format requires a value (json, yaml, table)" >&2
                    exit 1
                fi
                format="$2"
                if [[ "$format" != "json" && "$format" != "yaml" && "$format" != "table" ]]; then
                    echo "Error: Invalid format '$format'. Use: json, yaml, table" >&2
                    exit 1
                fi
                shift 2
                ;;
            --status)
                if [[ $# -lt 2 ]]; then
                    echo "Error: --status requires a value (active, stale, merged, abandoned)" >&2
                    exit 1
                fi
                filter_status="$2"
                if [[ "$filter_status" != "active" && "$filter_status" != "stale" && "$filter_status" != "merged" && "$filter_status" != "abandoned" ]]; then
                    echo "Error: Invalid status '$filter_status'. Use: active, stale, merged, abandoned" >&2
                    exit 1
                fi
                shift 2
                ;;
            --)
                shift
                break
                ;;
            -*)
                echo "Error: Unknown option '$1'. Use --help for usage." >&2
                exit 1
                ;;
            *)
                echo "Error: Unexpected argument '$1'. Use --help for usage." >&2
                exit 1
                ;;
        esac
    done

    local state_dir="$REPO_ROOT/.state"
    local worktrees_file="$state_dir/worktrees.yaml"

    if [[ ! -f "$worktrees_file" ]]; then
        echo "No tracked worktrees"
        exit 0
    fi

    case "$format" in
        json)
            output_json "$worktrees_file" "$filter_status"
            ;;
        yaml)
            output_yaml "$worktrees_file" "$filter_status"
            ;;
        table)
            output_table "$worktrees_file" "$filter_status"
            ;;
    esac
}

main "$@"
