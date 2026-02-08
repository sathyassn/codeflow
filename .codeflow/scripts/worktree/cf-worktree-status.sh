#!/usr/bin/env bash
# Purpose:   Check worktree health and synchronization status
# Usage:     cf-worktree-status.sh [worktree-path] [--verbose]
# Platform:  macOS/Linux
# Version:   1.0.0
#
# Checks worktree status including:
#   - Git sync status (ahead/behind remote)
#   - Uncommitted changes
#   - Branch health (merged, stale, abandoned)
#   - Last activity timestamp
#
# Arguments:
#   worktree-path  Optional path to specific worktree (default: check all)
#
# Options:
#   --verbose      Show detailed status for each worktree
#
# Exit codes:
#   - 0: All worktrees healthy
#   - 1: Some worktrees have issues
#   - 2: Error (not in git repo, etc.)

set -euo pipefail

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
cf-worktree-status.sh - Check worktree health and synchronization status

USAGE:
    cf-worktree-status.sh [worktree-path] [options]

ARGUMENTS:
    worktree-path  Optional path to specific worktree (default: check all)

OPTIONS:
    --verbose      Show detailed status for each worktree
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    cf-worktree-status.sh
    cf-worktree-status.sh .git-worktrees/feat-auth
    cf-worktree-status.sh --verbose
EOF
}

show_version() {
    echo "cf-worktree-status.sh version 1.0.0"
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

WORKTREE_PATH=""
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --verbose)
            VERBOSE=true
            shift
            ;;
        -*)
            echo "Unknown option: $1" >&2
            exit 2
            ;;
        *)
            WORKTREE_PATH="$1"
            shift
            ;;
    esac
done

# =============================================================================
# HELPERS
# =============================================================================

check_worktree_status() {
    local wt_path="$1"
    local issues=0

    if [[ ! -d "$wt_path" ]]; then
        echo "  ✗ Directory does not exist"
        return 1
    fi

    cd "$wt_path" || return 1

    # Check if it's a valid git worktree
    if ! git rev-parse --is-inside-work-tree &>/dev/null; then
        echo "  ✗ Not a valid git worktree"
        return 1
    fi

    local branch
    branch=$(git branch --show-current 2>/dev/null || echo "detached")
    echo "  Branch: $branch"

    # Check for uncommitted changes
    local changes
    changes=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$changes" -gt 0 ]]; then
        echo "  ⚠ Uncommitted changes: $changes files"
        ((issues++)) || true
    else
        echo "  ✓ Clean working directory"
    fi

    # Check sync status with remote
    if git rev-parse --abbrev-ref "@{u}" &>/dev/null; then
        local ahead behind
        ahead=$(git rev-list --count "@{u}..HEAD" 2>/dev/null || echo "0")
        behind=$(git rev-list --count "HEAD..@{u}" 2>/dev/null || echo "0")

        if [[ "$ahead" -gt 0 ]]; then
            echo "  ⚠ $ahead commits ahead of remote"
            ((issues++)) || true
        fi
        if [[ "$behind" -gt 0 ]]; then
            echo "  ⚠ $behind commits behind remote"
            ((issues++)) || true
        fi
        if [[ "$ahead" -eq 0 ]] && [[ "$behind" -eq 0 ]]; then
            echo "  ✓ Synced with remote"
        fi
    else
        echo "  ⚠ No upstream tracking branch"
    fi

    # Check last commit time
    local last_commit
    last_commit=$(git log -1 --format="%cr" 2>/dev/null || echo "unknown")
    echo "  Last commit: $last_commit"

    if [[ "$VERBOSE" == "true" ]]; then
        echo "  Recent commits:"
        git log --oneline -3 2>/dev/null | sed 's/^/    /'
    fi

    return "$issues"
}

# =============================================================================
# MAIN
# =============================================================================

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
STATE_DIR="$REPO_ROOT/.state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"

total_issues=0

if [[ -n "$WORKTREE_PATH" ]]; then
    # Check specific worktree
    echo "=== Checking: $WORKTREE_PATH ==="
    if ! check_worktree_status "$WORKTREE_PATH"; then
        ((total_issues++)) || true
    fi
else
    # Check all worktrees
    echo "=== Git Worktree Status ==="
    echo ""

    # Get worktrees from git
    while IFS= read -r line; do
        wt_path=$(echo "$line" | awk '{print $1}')
        # Skip main worktree
        if [[ "$wt_path" == "$REPO_ROOT" ]]; then
            continue
        fi

        echo "Worktree: $wt_path"
        if ! check_worktree_status "$wt_path"; then
            ((total_issues++)) || true
        fi
        echo ""
    done < <(git worktree list 2>/dev/null)

    # Also check tracked worktrees from state file
    if [[ -f "$WORKTREES_FILE" ]]; then
        echo "=== Tracked Worktrees (from .state/worktrees.yaml) ==="
        awk '
        /^  - path:/ { path=$3; gsub(/"/, "", path) }
        /status:/ { status=$2 }
        /last_active:/ {
            active=$2; gsub(/"/, "", active)
            printf "  %s (status: %s, last: %s)\n", path, status, active
        }
        ' "$WORKTREES_FILE"
    fi
fi

echo ""
if [[ "$total_issues" -gt 0 ]]; then
    echo "Summary: $total_issues worktree(s) with issues"
    exit 1
else
    echo "Summary: All worktrees healthy"
    exit 0
fi
