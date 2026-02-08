#!/usr/bin/env bash
# Purpose:   Clean up stale, merged, or abandoned worktrees
# Usage:     cf-worktree-cleanup.sh [--dry-run] [--force] [--status <status>]
# Platform:  macOS/Linux
# Version:   1.0.0
#
# Identifies and removes worktrees that are:
#   - Stale (no activity for configured period)
#   - Merged (branch merged to main)
#   - Abandoned (marked as abandoned in tracking)
#   - Orphaned (no longer in git worktree list)
#
# Options:
#   --dry-run    Show what would be cleaned without doing it
#   --force      Skip confirmation prompts
#   --status     Only clean worktrees with specific status
#   --days       Stale threshold in days (default: 14)
#
# Exit codes:
#   - 0: Cleanup successful (or nothing to clean)
#   - 1: Error during cleanup
#   - 2: User cancelled

set -euo pipefail

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
cf-worktree-cleanup.sh - Clean up stale, merged, or abandoned worktrees

USAGE:
    cf-worktree-cleanup.sh [options]

OPTIONS:
    --dry-run      Show what would be cleaned without doing it
    --force        Skip confirmation prompts
    --status <s>   Only clean worktrees with status: stale, merged, abandoned
    --days <n>     Stale threshold in days (default: 14)
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    cf-worktree-cleanup.sh --dry-run
    cf-worktree-cleanup.sh --status merged --force
    cf-worktree-cleanup.sh --days 7
EOF
}

show_version() {
    echo "cf-worktree-cleanup.sh version 1.0.0"
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

DRY_RUN=false
FORCE=false
FILTER_STATUS=""
STALE_DAYS=14

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --force)
            FORCE=true
            shift
            ;;
        --status)
            FILTER_STATUS="$2"
            shift 2
            ;;
        --days)
            STALE_DAYS="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

# =============================================================================
# HELPERS
# =============================================================================

log_action() {
    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[DRY-RUN] $1"
    else
        echo "$1"
    fi
}

confirm_action() {
    local prompt="$1"
    if [[ "$FORCE" == "true" ]] || [[ "$DRY_RUN" == "true" ]]; then
        return 0
    fi

    read -r -p "$prompt [y/N] " response
    case "$response" in
        [yY][eE][sS]|[yY])
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

is_branch_merged() {
    local branch="$1"
    local main_branch="${2:-main}"

    git branch --merged "$main_branch" 2>/dev/null | grep -q "^\s*$branch$"
}

is_stale() {
    local wt_path="$1"
    local days="$2"

    if [[ ! -d "$wt_path/.git" ]] && [[ ! -f "$wt_path/.git" ]]; then
        return 0  # No git directory = stale
    fi

    local last_commit_ts
    last_commit_ts=$(git -C "$wt_path" log -1 --format="%ct" 2>/dev/null || echo "0")

    local now_ts
    now_ts=$(date +%s)

    local age_days=$(( (now_ts - last_commit_ts) / 86400 ))

    [[ "$age_days" -gt "$days" ]]
}

remove_worktree() {
    local wt_path="$1"
    local reason="$2"

    log_action "Removing worktree: $wt_path ($reason)"

    if [[ "$DRY_RUN" == "true" ]]; then
        return 0
    fi

    # Remove from git worktree
    if git worktree remove "$wt_path" --force 2>/dev/null; then
        echo "  ✓ Removed from git worktree"
    else
        echo "  ⚠ Failed to remove from git worktree (may already be removed)"
    fi

    # Remove directory if still exists
    if [[ -d "$wt_path" ]]; then
        rm -rf "$wt_path"
        echo "  ✓ Removed directory"
    fi
}

update_tracking() {
    local wt_path="$1"
    local new_status="$2"
    local worktrees_file="$3"

    if [[ ! -f "$worktrees_file" ]]; then
        return
    fi

    log_action "Updating tracking status for $wt_path to $new_status"

    if [[ "$DRY_RUN" == "true" ]]; then
        return
    fi

    # Update status in YAML (simple sed replacement)
    local escaped_path
    escaped_path=$(echo "$wt_path" | sed 's/[\/&]/\\&/g')

    if [[ "$OSTYPE" == "darwin"* ]]; then
        sed -i '' "/$escaped_path/,/status:/ s/status: .*/status: $new_status/" "$worktrees_file"
    else
        sed -i "/$escaped_path/,/status:/ s/status: .*/status: $new_status/" "$worktrees_file"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
STATE_DIR="$REPO_ROOT/.state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"

echo "=== Worktree Cleanup ==="
echo "  Stale threshold: $STALE_DAYS days"
[[ -n "$FILTER_STATUS" ]] && echo "  Filter: $FILTER_STATUS"
[[ "$DRY_RUN" == "true" ]] && echo "  Mode: DRY RUN"
echo ""

to_clean=()
reasons=()

# Scan git worktrees
while IFS= read -r line; do
    wt_path=$(echo "$line" | awk '{print $1}')
    wt_branch=$(echo "$line" | awk '{print $3}' | tr -d '[]')

    # Skip main worktree
    if [[ "$wt_path" == "$REPO_ROOT" ]]; then
        continue
    fi

    reason=""

    # Check if merged
    if is_branch_merged "$wt_branch"; then
        reason="branch merged"
        if [[ -z "$FILTER_STATUS" ]] || [[ "$FILTER_STATUS" == "merged" ]]; then
            to_clean+=("$wt_path")
            reasons+=("$reason")
        fi
        continue
    fi

    # Check if stale
    if is_stale "$wt_path" "$STALE_DAYS"; then
        reason="stale (>$STALE_DAYS days)"
        if [[ -z "$FILTER_STATUS" ]] || [[ "$FILTER_STATUS" == "stale" ]]; then
            to_clean+=("$wt_path")
            reasons+=("$reason")
        fi
    fi

done < <(git worktree list 2>/dev/null)

# Check tracked worktrees for abandoned status
if [[ -f "$WORKTREES_FILE" ]] && { [[ -z "$FILTER_STATUS" ]] || [[ "$FILTER_STATUS" == "abandoned" ]]; }; then
    while IFS= read -r path; do
        path=$(echo "$path" | tr -d '"')
        if [[ -n "$path" ]] && ! printf '%s\n' "${to_clean[@]}" | grep -q "^$path$" 2>/dev/null; then
            to_clean+=("$path")
            reasons+=("abandoned (tracked)")
        fi
    done < <(awk '/status: abandoned/,/path:/ {if(/path:/) print $3}' "$WORKTREES_FILE" 2>/dev/null)
fi

# Summary
if [[ ${#to_clean[@]} -eq 0 ]]; then
    echo "No worktrees to clean up"
    exit 0
fi

echo "Found ${#to_clean[@]} worktree(s) to clean:"
for i in "${!to_clean[@]}"; do
    echo "  - ${to_clean[$i]} (${reasons[$i]})"
done
echo ""

if ! confirm_action "Proceed with cleanup?"; then
    echo "Cancelled"
    exit 2
fi

# Perform cleanup
cleaned=0
for i in "${!to_clean[@]}"; do
    if remove_worktree "${to_clean[$i]}" "${reasons[$i]}"; then
        ((cleaned++)) || true
        update_tracking "${to_clean[$i]}" "removed" "$WORKTREES_FILE"
    fi
done

echo ""
echo "Summary: Cleaned $cleaned worktree(s)"
