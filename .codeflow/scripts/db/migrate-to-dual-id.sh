#!/usr/bin/env bash
# Purpose:   Migrate project management .md files from single-ID to dual-ID format
# Location:  .codeflow/scripts/db/migrate-to-dual-id.sh
# Usage:     ./migrate-to-dual-id.sh [--dry-run]
# Version:   1.0.0
#
# Migration:
#   For each epic .md file:
#     - Current id: value → format_id:
#     - New id: field gets ULID PK via generate_epic_id()
#
#   For each task .md file:
#     - Current id: value → format_id:
#     - New id: field gets ULID PK via generate_task_id()
#     - Current epic_id: value → epic_format_id:
#     - New epic_id: field gets the ULID PK of the parent epic
#
# Process order: Epics first, then tasks (tasks need epic ULID PKs).
#
# Examples:
#   ./migrate-to-dual-id.sh --dry-run    # Preview changes without modifying files
#   ./migrate-to-dual-id.sh              # Apply migration

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PM_DIR="${REPO_ROOT}/project-management"
SHELL_LIB="${REPO_ROOT}/.codeflow/scripts/shell-lib"

# Source ULID generation
source "${SHELL_LIB}/ulid.sh"

# =============================================================================
# USAGE
# =============================================================================

usage() {
    cat <<'EOF'
Usage: migrate-to-dual-id.sh [--dry-run]

Options:
  --dry-run    Show what would change without modifying files
  --help, -h   Show this help message

Description:
  Migrates project management .md files from single-ID to dual-ID format.
  Processes epics before tasks so task files can reference epic ULID PKs.

  Before:
    id: INF-EPC-FEAT-GENL-001

  After:
    id: epic-01ARZ3NDEKTSV4RRFFQ69G5FAV
    format_id: INF-EPC-FEAT-GENL-001
EOF
    exit "${1:-0}"
}

# =============================================================================
# STATE
# =============================================================================

DRY_RUN=false

# Associative array: format_id → ULID PK (for cross-referencing)
declare -A FORMAT_TO_ULID

# Counters
EPICS_MIGRATED=0
TASKS_MIGRATED=0
EPICS_SKIPPED=0
TASKS_SKIPPED=0

# =============================================================================
# HELPERS
# =============================================================================

# Check if a file already has dual-ID format (format_id field present)
is_already_migrated() {
    local file="$1"
    grep -q "^format_id:" "$file" 2>/dev/null
}

# Extract a frontmatter field value from a markdown file
get_frontmatter_field() {
    local file="$1"
    local field="$2"
    # Match field at start of line within frontmatter, capture value after colon+space
    sed -n '/^---$/,/^---$/{/^'"${field}"':/s/^'"${field}"': *//p;}' "$file" | head -1
}

# =============================================================================
# MIGRATION: EPICS
# =============================================================================

migrate_epics() {
    echo "=== Migrating Epics ==="
    echo ""

    local epic_files
    epic_files=$(find "$PM_DIR" -name "*-epic.md" -type f 2>/dev/null | sort)

    if [[ -z "$epic_files" ]]; then
        echo "  No epic files found."
        return
    fi

    while IFS= read -r file; do
        local current_id
        current_id=$(get_frontmatter_field "$file" "id")

        if [[ -z "$current_id" ]]; then
            echo "  SKIP: ${file##"$REPO_ROOT"/} (no id field found)"
            EPICS_SKIPPED=$((EPICS_SKIPPED + 1))
            continue
        fi

        # Check if already migrated
        if is_already_migrated "$file"; then
            # Already has format_id — record existing mapping and skip
            local existing_ulid
            existing_ulid=$(get_frontmatter_field "$file" "id")
            local existing_format
            existing_format=$(get_frontmatter_field "$file" "format_id")
            FORMAT_TO_ULID["$existing_format"]="$existing_ulid"
            echo "  SKIP: ${file##"$REPO_ROOT"/} (already migrated)"
            EPICS_SKIPPED=$((EPICS_SKIPPED + 1))
            continue
        fi

        # Generate new ULID PK
        local new_ulid
        new_ulid=$(generate_epic_id)

        # Record mapping
        FORMAT_TO_ULID["$current_id"]="$new_ulid"

        local rel_path="${file##"$REPO_ROOT"/}"

        if [[ "$DRY_RUN" == true ]]; then
            echo "  DRY-RUN: ${rel_path}"
            echo "    id: ${current_id} → id: ${new_ulid}"
            echo "    (add) format_id: ${current_id}"
        else
            # Replace id: line with new id: and add format_id: after it
            # Use a temp file for safe in-place edit
            local tmp_file
            tmp_file=$(mktemp)
            local in_frontmatter=false
            local id_replaced=false

            while IFS= read -r line; do
                if [[ "$line" == "---" ]]; then
                    if [[ "$in_frontmatter" == false ]]; then
                        in_frontmatter=true
                        echo "$line" >> "$tmp_file"
                        continue
                    else
                        # Closing frontmatter delimiter
                        in_frontmatter=false
                        echo "$line" >> "$tmp_file"
                        continue
                    fi
                fi

                if [[ "$in_frontmatter" == true && "$id_replaced" == false ]] && [[ "$line" =~ ^id:\ * ]]; then
                    # Replace id line with new ULID PK, add format_id after
                    echo "id: ${new_ulid}" >> "$tmp_file"
                    echo "format_id: ${current_id}" >> "$tmp_file"
                    id_replaced=true
                else
                    echo "$line" >> "$tmp_file"
                fi
            done < "$file"

            mv "$tmp_file" "$file"
            echo "  MIGRATED: ${rel_path}"
            echo "    id: ${new_ulid}"
            echo "    format_id: ${current_id}"
        fi

        EPICS_MIGRATED=$((EPICS_MIGRATED + 1))
    done <<< "$epic_files"

    echo ""
}

# =============================================================================
# MIGRATION: TASKS
# =============================================================================

migrate_tasks() {
    echo "=== Migrating Tasks ==="
    echo ""

    local task_files
    task_files=$(find "$PM_DIR" -path "*/tasks/*.md" -type f 2>/dev/null | sort)

    if [[ -z "$task_files" ]]; then
        echo "  No task files found."
        return
    fi

    while IFS= read -r file; do
        local current_id
        current_id=$(get_frontmatter_field "$file" "id")

        if [[ -z "$current_id" ]]; then
            echo "  SKIP: ${file##"$REPO_ROOT"/} (no id field found)"
            TASKS_SKIPPED=$((TASKS_SKIPPED + 1))
            continue
        fi

        # Check if already migrated
        if is_already_migrated "$file"; then
            echo "  SKIP: ${file##"$REPO_ROOT"/} (already migrated)"
            TASKS_SKIPPED=$((TASKS_SKIPPED + 1))
            continue
        fi

        local current_epic_id
        current_epic_id=$(get_frontmatter_field "$file" "epic_id")

        # Generate new ULID PK
        local new_ulid
        new_ulid=$(generate_task_id)

        # Record mapping
        FORMAT_TO_ULID["$current_id"]="$new_ulid"

        # Look up the epic's ULID PK from the mapping
        local epic_ulid=""
        if [[ -n "$current_epic_id" ]]; then
            epic_ulid="${FORMAT_TO_ULID[$current_epic_id]:-}"
            if [[ -z "$epic_ulid" ]]; then
                echo "  WARNING: ${file##"$REPO_ROOT"/} — epic format_id '${current_epic_id}' not found in mapping" >&2
            fi
        fi

        local rel_path="${file##"$REPO_ROOT"/}"

        if [[ "$DRY_RUN" == true ]]; then
            echo "  DRY-RUN: ${rel_path}"
            echo "    id: ${current_id} → id: ${new_ulid}"
            echo "    (add) format_id: ${current_id}"
            if [[ -n "$current_epic_id" ]]; then
                echo "    epic_id: ${current_epic_id} → epic_id: ${epic_ulid:-UNKNOWN}"
                echo "    (add) epic_format_id: ${current_epic_id}"
            fi
        else
            # Rewrite frontmatter with dual-ID fields
            local tmp_file
            tmp_file=$(mktemp)
            local in_frontmatter=false
            local id_replaced=false
            local epic_id_replaced=false

            while IFS= read -r line; do
                if [[ "$line" == "---" ]]; then
                    if [[ "$in_frontmatter" == false ]]; then
                        in_frontmatter=true
                        echo "$line" >> "$tmp_file"
                        continue
                    else
                        in_frontmatter=false
                        echo "$line" >> "$tmp_file"
                        continue
                    fi
                fi

                if [[ "$in_frontmatter" == true ]]; then
                    if [[ "$id_replaced" == false ]] && [[ "$line" =~ ^id:\ * ]]; then
                        echo "id: ${new_ulid}" >> "$tmp_file"
                        echo "format_id: ${current_id}" >> "$tmp_file"
                        id_replaced=true
                    elif [[ "$epic_id_replaced" == false ]] && [[ "$line" =~ ^epic_id:\ * ]]; then
                        if [[ -n "$epic_ulid" ]]; then
                            echo "epic_id: ${epic_ulid}" >> "$tmp_file"
                        else
                            # Keep original if we can't resolve ULID
                            echo "$line" >> "$tmp_file"
                        fi
                        echo "epic_format_id: ${current_epic_id}" >> "$tmp_file"
                        epic_id_replaced=true
                    else
                        echo "$line" >> "$tmp_file"
                    fi
                else
                    echo "$line" >> "$tmp_file"
                fi
            done < "$file"

            mv "$tmp_file" "$file"
            echo "  MIGRATED: ${rel_path}"
            echo "    id: ${new_ulid}"
            echo "    format_id: ${current_id}"
            if [[ -n "$current_epic_id" ]]; then
                echo "    epic_id: ${epic_ulid:-UNCHANGED}"
                echo "    epic_format_id: ${current_epic_id}"
            fi
        fi

        TASKS_MIGRATED=$((TASKS_MIGRATED + 1))
    done <<< "$task_files"

    echo ""
}

# =============================================================================
# MAIN
# =============================================================================

# Parse arguments
case "${1:-}" in
    --help|-h)
        usage 0
        ;;
    --dry-run)
        DRY_RUN=true
        echo "=== DRY RUN MODE ==="
        echo ""
        ;;
    "")
        # No arguments, proceed with actual migration
        ;;
    *)
        echo "Error: Unknown argument '${1}'" >&2
        usage 1
        ;;
esac

# Verify project-management directory exists
if [[ ! -d "$PM_DIR" ]]; then
    echo "Error: project-management directory not found at ${PM_DIR}" >&2
    exit 1
fi

# Process epics FIRST (tasks need epic ULID PKs)
migrate_epics
migrate_tasks

# Summary
echo "=== Migration Summary ==="
echo "  Epics migrated: ${EPICS_MIGRATED}"
echo "  Epics skipped:  ${EPICS_SKIPPED}"
echo "  Tasks migrated: ${TASKS_MIGRATED}"
echo "  Tasks skipped:  ${TASKS_SKIPPED}"

if [[ "$DRY_RUN" == true ]]; then
    echo ""
    echo "  (dry-run mode — no files were modified)"
fi
