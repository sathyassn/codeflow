#!/usr/bin/env bash
# Purpose:   Generate human-readable format IDs for epics and tasks
# Location:  .codeflow/scripts/db/generate-format-id.sh
# Usage:
#   Epic: ./generate-format-id.sh epic <AREA>
#   Task: ./generate-format-id.sh task <AREA> <EPIC_NNN>
# Version:   2.0.0
#
# Format:
#   Epic:  {AREA}-EPC-{NNN}           (e.g., INF-EPC-011)
#   Task:  {AREA}-TSK-{NNN}-{NNN}     (e.g., INF-TSK-008-006)
#
# The first NNN in task format matches the parent epic's NNN.
# The second NNN is the task sequence within that epic.
#
# Sequence:
#   NNN is zero-padded to 3 digits, determined by querying the
#   SQLite database at .state/db/codeflow.db for the max existing
#   sequence number.
#
# Examples:
#   ./generate-format-id.sh epic INF           # → INF-EPC-011 (next after INF-EPC-010)
#   ./generate-format-id.sh task INF 008       # → INF-TSK-008-006 (next task in epic 008)

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
readonly REPO_ROOT
DB_PATH="${REPO_ROOT}/.state/db/codeflow.db"
readonly DB_PATH

# =============================================================================
# USAGE
# =============================================================================

usage() {
    cat <<'EOF'
Usage:
  generate-format-id.sh epic <AREA>
  generate-format-id.sh task <AREA> <EPIC_NNN>

Arguments:
  kind       "epic" or "task"
  AREA       Area code: 2-4 uppercase letters (e.g., FRT, BKD, INF)
  EPIC_NNN   Epic sequence number (3 digits, e.g., 008) — task only

Output:
  Prints the next available format ID to stdout.

Format:
  Epic:  {AREA}-EPC-{NNN}           (e.g., INF-EPC-011)
  Task:  {AREA}-TSK-{NNN}-{NNN}     (e.g., INF-TSK-008-006)

Examples:
  generate-format-id.sh epic INF           # → INF-EPC-011
  generate-format-id.sh task INF 008       # → INF-TSK-008-006
EOF
    exit "${1:-0}"
}

# =============================================================================
# VALIDATION
# =============================================================================

validate_uppercase_code() {
    local label="$1"
    local value="$2"
    if [[ ! "$value" =~ ^[A-Z]{2,4}$ ]]; then
        echo "Error: ${label} must be 2-4 uppercase letters, got '${value}'" >&2
        exit 1
    fi
}

validate_epic_nnn() {
    local value="$1"
    if [[ ! "$value" =~ ^[0-9]{3}$ ]]; then
        echo "Error: EPIC_NNN must be exactly 3 digits, got '${value}'" >&2
        exit 1
    fi
}

# =============================================================================
# MAIN
# =============================================================================

# Handle --help
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    usage 0
fi

# Require at least 2 arguments
if [[ $# -lt 2 ]]; then
    echo "Error: Expected at least 2 arguments, got $#" >&2
    usage 1
fi

KIND="$1"
AREA="$2"

# Validate kind
case "$KIND" in
    epic)
        if [[ $# -ne 2 ]]; then
            echo "Error: epic requires exactly 2 arguments (epic AREA), got $#" >&2
            usage 1
        fi
        ;;
    task)
        if [[ $# -ne 3 ]]; then
            echo "Error: task requires exactly 3 arguments (task AREA EPIC_NNN), got $#" >&2
            usage 1
        fi
        EPIC_NNN="$3"
        ;;
    *)
        echo "Error: kind must be 'epic' or 'task', got '${KIND}'" >&2
        exit 1
        ;;
esac

# Validate AREA
validate_uppercase_code "AREA" "$AREA"

# Validate EPIC_NNN for tasks
if [[ "$KIND" == "task" ]]; then
    validate_epic_nnn "$EPIC_NNN"
fi

# =============================================================================
# SEQUENCE LOOKUP
# =============================================================================

if [[ "$KIND" == "epic" ]]; then
    # Find max epic NNN for this AREA
    PREFIX="${AREA}-EPC-"
    MAX_SEQ=0

    if [[ -f "$DB_PATH" ]]; then
        # Query DB for max sequence number matching AREA-EPC-NNN pattern
        DB_RESULT=$(sqlite3 "$DB_PATH" \
            "SELECT format_id FROM epics WHERE format_id LIKE '${PREFIX}%' ORDER BY format_id DESC LIMIT 1;" \
            2>/dev/null || true)
        if [[ -n "$DB_RESULT" && "$DB_RESULT" =~ ${PREFIX}([0-9]{3}) ]]; then
            MAX_SEQ=$((10#${BASH_REMATCH[1]}))
        fi
    fi

    # Also scan markdown files as fallback
    PM_DIR="${REPO_ROOT}/project-management"
    if [[ -d "$PM_DIR" ]]; then
        while IFS= read -r line; do
            if [[ "$line" =~ ${PREFIX}([0-9]{3}) ]]; then
                seq_num=$((10#${BASH_REMATCH[1]}))
                if [[ $seq_num -gt $MAX_SEQ ]]; then
                    MAX_SEQ=$seq_num
                fi
            fi
        done < <(grep -rh "^\(id\|format_id\):.*${PREFIX}" "$PM_DIR" 2>/dev/null || true)
    fi

    NEXT_SEQ=$((MAX_SEQ + 1))
    NEXT_SEQ_PADDED=$(printf "%03d" "$NEXT_SEQ")
    echo "${PREFIX}${NEXT_SEQ_PADDED}"

else
    # Find max task NNN under this epic
    PREFIX="${AREA}-TSK-${EPIC_NNN}-"
    MAX_SEQ=0

    if [[ -f "$DB_PATH" ]]; then
        DB_RESULT=$(sqlite3 "$DB_PATH" \
            "SELECT format_id FROM tasks WHERE format_id LIKE '${PREFIX}%' ORDER BY format_id DESC LIMIT 1;" \
            2>/dev/null || true)
        if [[ -n "$DB_RESULT" && "$DB_RESULT" =~ ${PREFIX}([0-9]{3}) ]]; then
            MAX_SEQ=$((10#${BASH_REMATCH[1]}))
        fi
    fi

    # Also scan markdown files as fallback
    PM_DIR="${REPO_ROOT}/project-management"
    if [[ -d "$PM_DIR" ]]; then
        while IFS= read -r line; do
            if [[ "$line" =~ ${PREFIX}([0-9]{3}) ]]; then
                seq_num=$((10#${BASH_REMATCH[1]}))
                if [[ $seq_num -gt $MAX_SEQ ]]; then
                    MAX_SEQ=$seq_num
                fi
            fi
        done < <(grep -rh "^\(id\|format_id\):.*${PREFIX}" "$PM_DIR" 2>/dev/null || true)
    fi

    NEXT_SEQ=$((MAX_SEQ + 1))
    NEXT_SEQ_PADDED=$(printf "%03d" "$NEXT_SEQ")
    echo "${PREFIX}${NEXT_SEQ_PADDED}"
fi
