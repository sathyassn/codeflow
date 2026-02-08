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

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
cf-worktree-list.sh - List tracked worktrees

USAGE:
    cf-worktree-list.sh [options]

OPTIONS:
    --format <f>   Output format: json, yaml, table (default: table)
    --status <s>   Filter by status: active, stale, merged, abandoned
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    cf-worktree-list.sh
    cf-worktree-list.sh --format yaml
    cf-worktree-list.sh --status active
EOF
}

show_version() {
    echo "cf-worktree-list.sh version 1.0.0"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

case "${1:-}" in
    -h|--help)
        show_help
        exit 0
        ;;
    -V|--version)
        show_version
        exit 0
        ;;
esac

FORMAT="table"
FILTER_STATUS=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --format)
            FORMAT="$2"
            shift 2
            ;;
        --status)
            FILTER_STATUS="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

# =============================================================================
# MAIN
# =============================================================================

STATE_DIR=".state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"

if [[ ! -f "$WORKTREES_FILE" ]]; then
    echo "No tracked worktrees"
    exit 0
fi

case "$FORMAT" in
    json)
        if command -v yq &>/dev/null; then
            yq -o=json "$WORKTREES_FILE"
        else
            echo "yq not installed, showing YAML"
            cat "$WORKTREES_FILE"
        fi
        ;;
    yaml)
        cat "$WORKTREES_FILE"
        ;;
    table|*)
        echo "=== Tracked Worktrees ==="
        echo ""
        printf "%-40s %-20s %-10s %s\n" "PATH" "BRANCH" "STATUS" "LAST ACTIVE"
        printf "%-40s %-20s %-10s %s\n" "----" "------" "------" "-----------"

        awk -v filter="$FILTER_STATUS" '
        /^  - path:/ { path=$3; gsub(/"/, "", path) }
        /branch:/ { branch=$2; gsub(/"/, "", branch) }
        /status:/ { status=$2 }
        /last_active:/ {
            active=$2; gsub(/"/, "", active)
            if (filter == "" || status == filter) {
                printf "%-40s %-20s %-10s %s\n", path, branch, status, active
            }
        }
        ' "$WORKTREES_FILE"

        echo ""
        echo "=== Git Worktrees (actual) ==="
        git worktree list 2>/dev/null || echo "Not in a git repository"
        ;;
esac
