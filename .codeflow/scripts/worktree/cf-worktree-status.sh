#!/usr/bin/env bash
# Purpose:   Check worktree health and synchronization status
# Usage:     cf-worktree-status.sh [--path <worktree-path>] [--json] [--verbose]
# Platform:  macOS/Linux
# Version:   1.1.0
#
# Checks worktree status including:
#   - Worktree location and branch
#   - Uncommitted changes count
#   - Unpushed/unpulled commits count
#   - Clean/dirty status
#   - Work item association (from .state/worktrees.yaml)
#   - Last activity timestamp
#
# Options:
#   --path <p>     Check specific worktree (default: check all)
#   --json         Output in JSON format
#   --verbose      Show detailed status for each worktree
#
# Exit codes:
#   - 0: All worktrees healthy
#   - 1: Some worktrees have issues
#   - 2: Error (not in git repo, invalid arguments, etc.)

set -euo pipefail

# =============================================================================
# CONSTANTS
# =============================================================================

VERSION="1.1.0"
SCRIPT_NAME="$(basename "$0")"

# =============================================================================
# USAGE
# =============================================================================

show_help() {
    cat << EOF
${SCRIPT_NAME} - Check worktree health and synchronization status

USAGE:
    ${SCRIPT_NAME} [options] [worktree-path]

ARGUMENTS:
    worktree-path  Optional path to specific worktree (default: check all)

OPTIONS:
    --path <p>     Path to specific worktree to check
    --json         Output in JSON format
    --verbose      Show detailed status for each worktree
    -h, --help     Show this help message
    -V, --version  Show version

EXAMPLES:
    ${SCRIPT_NAME}
    ${SCRIPT_NAME} --path .git-worktrees/feat-auth
    ${SCRIPT_NAME} --json
    ${SCRIPT_NAME} --verbose

RELATED:
    cf-worktree-setup.sh   Set up a new worktree
    cf-worktree-list.sh    List tracked worktrees
    cf-worktree-cleanup.sh Remove stale worktrees
EOF
}

show_version() {
    echo "${SCRIPT_NAME} version ${VERSION}"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

WORKTREE_PATH=""
VERBOSE=false
JSON_OUTPUT=false

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
        --path)
            if [[ -z "${2:-}" ]]; then
                echo "Error: --path requires an argument" >&2
                exit 2
            fi
            WORKTREE_PATH="$2"
            shift 2
            ;;
        --json)
            JSON_OUTPUT=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run '${SCRIPT_NAME} --help' for usage." >&2
            exit 2
            ;;
        *)
            # Accept positional argument as worktree path
            WORKTREE_PATH="$1"
            shift
            ;;
    esac
done

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
WORKTREES_FILE="$REPO_ROOT/.state/worktrees.yaml"

# =============================================================================
# HELPERS
# =============================================================================

# Look up work item association from worktrees.yaml
# Parameters: $1 = worktree path to look up
# Outputs: purpose string or empty
lookup_work_item() {
    local wt_path="$1"

    if [[ ! -f "$WORKTREES_FILE" ]]; then
        return
    fi

    awk -v target="$wt_path" '
    /^  - path:/ {
        path=$3; gsub(/"/, "", path)
        purpose=""
    }
    /purpose:/ {
        sub(/^[[:space:]]*purpose:[[:space:]]*/, "")
        gsub(/"/, "")
        purpose=$0
    }
    /status:/ {
        if (path == target || index(target, path) > 0) {
            if (purpose != "") print purpose
        }
    }
    ' "$WORKTREES_FILE"
}

# Check status of a single worktree
# Uses git -C to avoid changing the caller's cwd
# Parameters: $1 = worktree path
# Returns: 0 if healthy, 1 if has issues
check_worktree_status() {
    local wt_path="$1"
    local issues=0

    if [[ ! -d "$wt_path" ]]; then
        if [[ "$JSON_OUTPUT" == "true" ]]; then
            printf '{"path":"%s","error":"directory_not_found"}\n' "$wt_path"
        else
            echo "  Status: ERROR - Directory does not exist"
        fi
        return 1
    fi

    # Check if it's a valid git worktree
    if ! git -C "$wt_path" rev-parse --is-inside-work-tree &>/dev/null; then
        if [[ "$JSON_OUTPUT" == "true" ]]; then
            printf '{"path":"%s","error":"not_a_worktree"}\n' "$wt_path"
        else
            echo "  Status: ERROR - Not a valid git worktree"
        fi
        return 1
    fi

    # Gather data using git -C (no cwd change)
    local branch changes ahead behind last_commit has_upstream verbose_log
    branch=$(git -C "$wt_path" branch --show-current 2>/dev/null || echo "detached")
    changes=$(git -C "$wt_path" status --porcelain 2>/dev/null | wc -l | tr -d ' ')

    # Check sync status with remote
    ahead="0"
    behind="0"
    has_upstream="false"
    if git -C "$wt_path" rev-parse --abbrev-ref "@{u}" &>/dev/null; then
        has_upstream="true"
        ahead=$(git -C "$wt_path" rev-list --count "@{u}..HEAD" 2>/dev/null || echo "0")
        behind=$(git -C "$wt_path" rev-list --count "HEAD..@{u}" 2>/dev/null || echo "0")
    fi

    last_commit=$(git -C "$wt_path" log -1 --format="%cr" 2>/dev/null || echo "unknown")

    verbose_log=""
    if [[ "$VERBOSE" == "true" ]]; then
        verbose_log=$(git -C "$wt_path" log --oneline -3 2>/dev/null || true)
    fi

    # Determine clean/dirty status
    local status_label="clean"
    if [[ "$changes" -gt 0 ]]; then
        status_label="dirty"
        ((issues++)) || true
    fi
    if [[ "$ahead" -gt 0 ]]; then
        ((issues++)) || true
    fi
    if [[ "$behind" -gt 0 ]]; then
        ((issues++)) || true
    fi

    # Look up work item association
    local work_item
    work_item=$(lookup_work_item "$wt_path")

    # Output
    if [[ "$JSON_OUTPUT" == "true" ]]; then
        local json_work_item="${work_item:-null}"
        if [[ "$json_work_item" != "null" ]]; then
            json_work_item="\"$json_work_item\""
        fi
        printf '{"path":"%s","branch":"%s","status":"%s","uncommitted_changes":%d,"ahead":%d,"behind":%d,"has_upstream":%s,"last_commit":"%s","work_item":%s}\n' \
            "$wt_path" "$branch" "$status_label" "$changes" "$ahead" "$behind" "$has_upstream" "$last_commit" "$json_work_item"
    else
        echo "  Branch: $branch"
        if [[ -n "$work_item" ]]; then
            echo "  Work item: $work_item"
        fi
        echo "  Status: $status_label"

        if [[ "$changes" -gt 0 ]]; then
            echo "  Uncommitted changes: $changes files"
        fi

        if [[ "$has_upstream" == "true" ]]; then
            if [[ "$ahead" -gt 0 ]]; then
                echo "  Unpushed: $ahead commits ahead of remote"
            fi
            if [[ "$behind" -gt 0 ]]; then
                echo "  Behind: $behind commits behind remote"
            fi
            if [[ "$ahead" -eq 0 ]] && [[ "$behind" -eq 0 ]]; then
                echo "  Remote: synced"
            fi
        else
            echo "  Remote: no upstream tracking branch"
        fi

        echo "  Last commit: $last_commit"

        if [[ "$VERBOSE" == "true" ]] && [[ -n "$verbose_log" ]]; then
            echo "  Recent commits:"
            echo "$verbose_log" | sed 's/^/    /'
        fi
    fi

    if [[ "$issues" -gt 0 ]]; then
        return 1
    fi
    return 0
}

# =============================================================================
# MAIN
# =============================================================================

total_issues=0
worktree_count=0

if [[ -n "$WORKTREE_PATH" ]]; then
    # Check specific worktree
    if [[ "$JSON_OUTPUT" != "true" ]]; then
        echo "=== Checking: $WORKTREE_PATH ==="
    fi
    worktree_count=1
    if ! check_worktree_status "$WORKTREE_PATH"; then
        ((total_issues++)) || true
    fi
else
    # Check all worktrees
    if [[ "$JSON_OUTPUT" == "true" ]]; then
        echo "["
        json_first=true
    else
        echo "=== Git Worktree Status ==="
        echo ""
    fi

    # Get worktrees from git
    while IFS= read -r line; do
        wt_path=$(echo "$line" | awk '{print $1}')
        # Skip main worktree
        if [[ "$wt_path" == "$REPO_ROOT" ]]; then
            continue
        fi

        ((worktree_count++)) || true

        if [[ "$JSON_OUTPUT" == "true" ]]; then
            if [[ "${json_first:-true}" != "true" ]]; then
                echo ","
            fi
            json_first=false
        else
            echo "Worktree: $wt_path"
        fi

        if ! check_worktree_status "$wt_path"; then
            ((total_issues++)) || true
        fi

        if [[ "$JSON_OUTPUT" != "true" ]]; then
            echo ""
        fi
    done < <(git worktree list 2>/dev/null)

    # Also check tracked worktrees from state file (text mode only)
    if [[ "$JSON_OUTPUT" != "true" ]] && [[ -f "$WORKTREES_FILE" ]]; then
        echo "=== Tracked Worktrees (from .state/worktrees.yaml) ==="
        awk '
        /^  - path:/ { path=$3; gsub(/"/, "", path) }
        /branch:/ && !/^  - / { branch=$2; gsub(/"/, "", branch) }
        /status:/ { status=$2 }
        /last_active:/ {
            active=$2; gsub(/"/, "", active)
            printf "  %s (branch: %s, status: %s, last: %s)\n", path, branch, status, active
        }
        ' "$WORKTREES_FILE"
    fi

    if [[ "$JSON_OUTPUT" == "true" ]]; then
        echo ""
        echo "]"
    fi
fi

# Summary
if [[ "$JSON_OUTPUT" != "true" ]]; then
    echo ""
    if [[ "$worktree_count" -eq 0 ]]; then
        echo "Summary: No worktrees found (only main working tree)"
    elif [[ "$total_issues" -gt 0 ]]; then
        echo "Summary: $total_issues of $worktree_count worktree(s) with issues"
    else
        echo "Summary: All $worktree_count worktree(s) healthy"
    fi
fi

if [[ "$total_issues" -gt 0 ]]; then
    exit 1
fi
exit 0
