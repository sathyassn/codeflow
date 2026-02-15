#!/usr/bin/env bash
# Purpose:   Clean up stale, merged, or abandoned worktrees
# Usage:     cf-worktree-cleanup.sh [--dry-run] [--force] [--path <path>] [--status <status>] [--days <n>] [--prune]
# Platform:  macOS/Linux
# Version:   1.1.0
#
# NOTE: V4 spec names this cf-cleanup.sh but current codebase uses cf-worktree-
#       prefix for all worktree scripts. Do not rename without coordinating
#       across all worktree scripts.
#
# Identifies and removes worktrees that are:
#   - Stale (no activity for configured period)
#   - Merged (branch merged to main)
#   - Abandoned (marked as abandoned in tracking)
#
# Also supports pruning stale git worktree references (--prune).
#
# Safety: Checks for uncommitted changes and unpushed commits before removal.
#         In PathFlow mode (PF-1 through PF-6), cleanup is blocked unless
#         --force is used or the session is at PF-7 teardown.
#
# Options:
#   --path <path>    Remove a specific worktree by path
#   --dry-run        Show what would be cleaned without doing it
#   --force          Skip safety checks and confirmation prompts
#   --status <s>     Only clean worktrees with status: stale, merged, abandoned
#   --days <n>       Stale threshold in days (default: from config or 14)
#   --prune          Prune stale git worktree references
#   -h, --help       Show this help message
#   -V, --version    Show version
#
# Exit codes:
#   0 - Cleanup successful (or nothing to clean)
#   1 - Error during cleanup
#   2 - User cancelled or PathFlow blocked

set -euo pipefail

# =============================================================================
# SOURCE GUARD
# =============================================================================

if [[ "${BASH_SOURCE[0]}" != "${0}" ]]; then
    echo "Error: This script must be executed, not sourced." >&2
    return 1 2>/dev/null || exit 1
fi

# =============================================================================
# SETUP
# =============================================================================

VERSION="1.1.0"

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
export REPO_ROOT

# Source context-lib for PathFlow detection
SECURITY_LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$SECURITY_LIB_DIR/context-lib.sh" ]]; then
    # shellcheck source=../security/lib/context-lib.sh
    source "$SECURITY_LIB_DIR/context-lib.sh"
fi

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << 'EOF'
cf-worktree-cleanup.sh - Clean up stale, merged, or abandoned worktrees

USAGE:
    cf-worktree-cleanup.sh [options]

OPTIONS:
    --path <path>    Remove a specific worktree by path
    --dry-run        Show what would be cleaned without doing it
    --force          Skip safety checks and confirmation prompts
    --status <s>     Only clean worktrees with status: stale, merged, abandoned
    --days <n>       Stale threshold in days (default: from config or 14)
    --prune          Prune stale git worktree references
    -h, --help       Show this help message
    -V, --version    Show version

PATHFLOW:
    During active PathFlow sessions (PF-1 through PF-6), cleanup is blocked
    to prevent disrupting active teammate worktrees. Use --force to override,
    or wait until PF-7 (session teardown).

EXAMPLES:
    cf-worktree-cleanup.sh --dry-run
    cf-worktree-cleanup.sh --path /path/to/worktree
    cf-worktree-cleanup.sh --status merged --force
    cf-worktree-cleanup.sh --days 7
    cf-worktree-cleanup.sh --prune
EOF
}

show_version() {
    echo "cf-worktree-cleanup.sh version $VERSION"
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
STALE_DAYS=""
TARGET_PATH=""
PRUNE_MODE=false

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
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --force)
            FORCE=true
            shift
            ;;
        --path)
            if [[ -z "${2:-}" ]]; then
                echo "Error: --path requires an argument" >&2
                exit 1
            fi
            TARGET_PATH="$2"
            shift 2
            ;;
        --status)
            if [[ -z "${2:-}" ]]; then
                echo "Error: --status requires an argument" >&2
                exit 1
            fi
            FILTER_STATUS="$2"
            if [[ "$FILTER_STATUS" != "stale" && "$FILTER_STATUS" != "merged" && "$FILTER_STATUS" != "abandoned" ]]; then
                echo "Error: --status must be one of: stale, merged, abandoned" >&2
                exit 1
            fi
            shift 2
            ;;
        --days)
            if [[ -z "${2:-}" ]]; then
                echo "Error: --days requires a numeric argument" >&2
                exit 1
            fi
            if ! [[ "$2" =~ ^[0-9]+$ ]]; then
                echo "Error: --days must be a positive integer" >&2
                exit 1
            fi
            STALE_DAYS="$2"
            shift 2
            ;;
        --prune)
            PRUNE_MODE=true
            shift
            ;;
        *)
            echo "Error: Unknown option: $1" >&2
            echo "Run with --help for usage information" >&2
            exit 1
            ;;
    esac
done

# =============================================================================
# CONFIG
# =============================================================================

STATE_DIR="$REPO_ROOT/.state"
WORKTREES_FILE="$STATE_DIR/worktrees.yaml"
CONFIG_FILE="$REPO_ROOT/.codeflow/config/enforcement-policy.json"

# Read stale threshold from config if not set via --days
if [[ -z "$STALE_DAYS" ]]; then
    if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        STALE_DAYS=$(jq -r '.skills."git-workflow".operations."cleanup-worktrees".stale_days // empty' "$CONFIG_FILE" 2>/dev/null) || true
    fi
    STALE_DAYS="${STALE_DAYS:-14}"
fi

# =============================================================================
# PATHFLOW CHECK
# =============================================================================

check_pathflow() {
    if ! type -t is_pathflow_active &>/dev/null; then
        return 0  # No context-lib available, skip check
    fi

    if ! is_pathflow_active; then
        return 0  # Not in PathFlow mode
    fi

    # PathFlow is active - cleanup is restricted
    if [[ "$FORCE" == "true" ]]; then
        echo "WARNING: PathFlow is active. Proceeding with --force." >&2
        return 0
    fi

    cat >&2 <<'EOF'
BLOCKED: PathFlow session is active (PF-1 through PF-6).

Worktree cleanup is restricted during active PathFlow sessions to prevent
disrupting teammate worktrees. Cleanup is only safe at PF-7 (teardown).

Options:
  - Wait until PF-7 (session teardown) to clean up worktrees
  - Use --force to override this check
  - Message cf-git-operations teammate for coordinated worktree cleanup

EOF
    exit 2
}

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

    git -C "$REPO_ROOT" branch --merged "$main_branch" 2>/dev/null | grep -q "^[[:space:]]*${branch}$"
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

has_uncommitted_changes() {
    local wt_path="$1"

    if [[ ! -d "$wt_path" ]]; then
        return 1
    fi

    # Check for any uncommitted changes (staged or unstaged)
    ! git -C "$wt_path" diff --quiet 2>/dev/null || \
    ! git -C "$wt_path" diff --cached --quiet 2>/dev/null
}

has_unpushed_commits() {
    local wt_path="$1"

    if [[ ! -d "$wt_path" ]]; then
        return 1
    fi

    local branch
    branch=$(git -C "$wt_path" branch --show-current 2>/dev/null) || return 1

    if [[ -z "$branch" ]]; then
        return 1
    fi

    # Check if remote tracking branch exists
    local upstream
    upstream=$(git -C "$wt_path" rev-parse --abbrev-ref "${branch}@{upstream}" 2>/dev/null) || return 1

    # Check if there are commits ahead of upstream
    local ahead
    ahead=$(git -C "$wt_path" rev-list --count "${upstream}..${branch}" 2>/dev/null) || return 1

    [[ "$ahead" -gt 0 ]]
}

remove_worktree() {
    local wt_path="$1"
    local reason="$2"

    log_action "Removing worktree: $wt_path ($reason)"

    if [[ "$DRY_RUN" == "true" ]]; then
        return 0
    fi

    # Safety checks (skip with --force)
    if [[ "$FORCE" != "true" ]]; then
        if has_uncommitted_changes "$wt_path"; then
            echo "  WARNING: Worktree has uncommitted changes: $wt_path" >&2
            if ! confirm_action "  Remove worktree with uncommitted changes?"; then
                echo "  Skipped (uncommitted changes)"
                return 1
            fi
        fi

        if has_unpushed_commits "$wt_path"; then
            echo "  WARNING: Worktree has unpushed commits: $wt_path" >&2
            if ! confirm_action "  Remove worktree with unpushed commits?"; then
                echo "  Skipped (unpushed commits)"
                return 1
            fi
        fi
    fi

    # Try normal removal first, fall back to force
    if git -C "$REPO_ROOT" worktree remove "$wt_path" 2>/dev/null; then
        echo "  Removed from git worktree"
    elif git -C "$REPO_ROOT" worktree remove "$wt_path" --force 2>/dev/null; then
        echo "  Force-removed from git worktree"
    else
        echo "  Failed to remove from git worktree (may already be removed)" >&2
    fi

    # Remove directory if still exists
    if [[ -d "$wt_path" ]]; then
        rm -rf "$wt_path"
        echo "  Removed directory"
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
# PRUNE MODE
# =============================================================================

run_prune() {
    echo "=== Worktree Prune ==="

    if [[ "$DRY_RUN" == "true" ]]; then
        echo "  Mode: DRY RUN"
        echo ""
        git -C "$REPO_ROOT" worktree prune --dry-run 2>&1
    else
        git -C "$REPO_ROOT" worktree prune 2>&1
        echo "  Pruned stale worktree references"
    fi

    exit 0
}

# =============================================================================
# SINGLE PATH MODE
# =============================================================================

run_single_path() {
    local target="$1"

    # Resolve symlinks (macOS: /tmp → /private/tmp)
    if [[ -d "$target" ]]; then
        target=$(cd "$target" && pwd -P)
    fi

    echo "=== Worktree Cleanup (single) ==="
    echo "  Target: $target"
    [[ "$DRY_RUN" == "true" ]] && echo "  Mode: DRY RUN"
    echo ""

    if [[ ! -d "$target" ]]; then
        echo "Error: Path does not exist: $target" >&2
        exit 1
    fi

    # Verify it's a worktree
    local is_worktree=false
    while IFS= read -r line; do
        local wt_path
        wt_path=$(echo "$line" | awk '{print $1}')
        if [[ "$wt_path" == "$target" ]]; then
            is_worktree=true
            break
        fi
    done < <(git -C "$REPO_ROOT" worktree list 2>/dev/null)

    if [[ "$is_worktree" != "true" ]]; then
        echo "Error: Path is not a registered worktree: $target" >&2
        exit 1
    fi

    if remove_worktree "$target" "requested by --path"; then
        update_tracking "$target" "removed" "$WORKTREES_FILE"
        echo ""
        echo "Summary: Cleaned 1 worktree"
    else
        echo ""
        echo "Summary: Cleanup skipped"
    fi

    exit 0
}

# =============================================================================
# MAIN
# =============================================================================

# PathFlow safety check
check_pathflow

# Handle prune mode
if [[ "$PRUNE_MODE" == "true" ]]; then
    run_prune
fi

# Handle single path mode
if [[ -n "$TARGET_PATH" ]]; then
    run_single_path "$TARGET_PATH"
fi

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

done < <(git -C "$REPO_ROOT" worktree list 2>/dev/null)

# Check tracked worktrees for abandoned status
if [[ -f "$WORKTREES_FILE" ]] && { [[ -z "$FILTER_STATUS" ]] || [[ "$FILTER_STATUS" == "abandoned" ]]; }; then
    while IFS= read -r path; do
        path=$(echo "$path" | tr -d '"')
        if [[ -n "$path" ]] && ! printf '%s\n' "${to_clean[@]}" | grep -q "^${path}$" 2>/dev/null; then
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
skipped=0
for i in "${!to_clean[@]}"; do
    if remove_worktree "${to_clean[$i]}" "${reasons[$i]}"; then
        ((cleaned++)) || true
        update_tracking "${to_clean[$i]}" "removed" "$WORKTREES_FILE"
    else
        ((skipped++)) || true
    fi
done

echo ""
echo "Summary: Cleaned $cleaned worktree(s), skipped $skipped"
