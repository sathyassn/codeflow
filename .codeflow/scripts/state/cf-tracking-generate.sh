#!/usr/bin/env bash
# CodeFlow: Generate Epic Tracker
# Location: .codeflow/scripts/state/cf-tracking-generate.sh
#
# Scans project-management/epics/ and generates tracking/epic-tracker.md
# This is a Tier 2 (presentation) file — auto-generated, not manually edited.
#
# Usage: bash .codeflow/scripts/state/cf-tracking-generate.sh [repo_root]

set -euo pipefail

# --- Constants ---
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME

# --- Usage ---
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS] [repo_root]

Scans project-management/epics/ and generates tracking/epic-tracker.md.

Arguments:
    repo_root       Repository root directory (default: auto-detect via git)

Options:
    -h, --help      Show this help message
    -V, --version   Show version information
EOF
}

# --- Functions ---

# Extract YAML frontmatter value from a markdown file.
# Strips surrounding quotes from values.
# Args: $1=file, $2=key
get_frontmatter() {
    local file="$1" key="$2"
    local value
    # Simple YAML frontmatter parser — reads between --- markers
    value=$(sed -n '/^---$/,/^---$/p' "$file" 2>/dev/null \
        | grep "^${key}:" | head -1 \
        | sed "s/^${key}:[[:space:]]*//")
    # Strip surrounding double or single quotes
    value="${value#\"}"
    value="${value%\"}"
    value="${value#\'}"
    value="${value%\'}"
    printf '%s' "$value"
}

# Count tasks in a directory by status.
# Outputs: "todo in_prog complete blocked" (space-separated counts)
# Note: Does NOT update global counters (runs in subshell via $()).
# Caller must accumulate totals from returned counts.
# Args: $1=tasks_dir
count_tasks_by_status() {
    local tasks_dir="$1"
    local todo=0 in_prog=0 complete=0 blocked=0

    if [[ -d "$tasks_dir" ]]; then
        for task_file in "$tasks_dir"/*.md; do
            [[ -f "$task_file" ]] || continue
            local status
            status=$(get_frontmatter "$task_file" "status")
            case "$status" in
                todo|"") ((todo++)) || true ;;
                in_progress) ((in_prog++)) || true ;;
                complete|completed|done) ((complete++)) || true ;;
                blocked) ((blocked++)) || true ;;
            esac
        done
    fi
    printf '%s' "$todo $in_prog $complete $blocked"
}

# Write the tracker markdown file.
# Args: $1=output_file, $2=total_epics, $3=active_epics, $4=total_tasks,
#       $5=completed_tasks, $6=area_table, $7=active_section, $8=completed_section
_write_tracker() {
    local output_file="$1"
    local t_epics="$2" a_epics="$3" t_tasks="$4" c_tasks="$5"
    local area_tbl="$6" active_sec="$7" completed_sec="$8"

    {
        printf '%s\n' "---"
        printf '%s\n' "# GENERATED — DO NOT EDIT"
        printf '%s\n' "# Regenerate with: bash .codeflow/scripts/state/cf-tracking-generate.sh"
        printf 'generated_at: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        printf 'total_epics: %s\n' "$t_epics"
        printf 'active_epics: %s\n' "$a_epics"
        printf 'total_tasks: %s\n' "$t_tasks"
        printf 'completed_tasks: %s\n' "$c_tasks"
        printf '%s\n' "source: filesystem scan"
        printf '%s\n' "---"
        printf '\n'
        printf '%s\n' "# Epic Tracker"
        printf '\n'
        printf '%s\n' "## Summary"
        printf '\n'
        printf '%s\n' "| Metric | Count |"
        printf '%s\n' "|--------|-------|"
        printf '| Total Epics | %s |\n' "$t_epics"
        printf '| Active Epics | %s |\n' "$a_epics"
        printf '| Total Tasks | %s |\n' "$t_tasks"
        printf '| Completed Tasks | %s |\n' "$c_tasks"
        printf '\n'
        printf '%s\n' "## Epics by Area"
        printf '\n'
        printf '%s\n' "| Area | Total | Active | Ongoing |"
        printf '%s\n' "|------|-------|--------|---------|"
        printf '%s' "$area_tbl"
        printf '\n'
        printf '%s\n' "## Active Epics"
        printf '\n'
        printf '%s' "$active_sec"
        printf '%s\n' "## Completed Epics"
        printf '\n'
        printf '%s' "$completed_sec"
    } > "$output_file"
}

# --- Main ---
main() {
    # Parse options
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) usage; exit 0 ;;
            -V|--version) echo "$SCRIPT_NAME 1.0.0"; exit 0 ;;
            -*) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
            *) break ;;
        esac
        shift
    done

    local REPO_ROOT="${1:-${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || echo ".")}}"
    local EPICS_DIR="$REPO_ROOT/project-management/epics"
    local TRACKER_FILE="$REPO_ROOT/project-management/tracking/epic-tracker.md"

    # Validate REPO_ROOT
    if [[ ! -d "$REPO_ROOT" ]]; then
        echo "Error: Repository root not found: $REPO_ROOT" >&2
        exit 1
    fi

    # Counters (global for count_tasks_by_status access)
    total_tasks=0
    completed_tasks=0
    local total_epics=0
    local active_epics=0

    # Tab-delimited records for epic data (avoids pipe collision with titles)
    local TAB=$'\t'
    local epic_records=""
    local area_records=""

    # Dynamically discover area folders (no hardcoded list)
    if [[ ! -d "$EPICS_DIR" ]]; then
        # No epics directory — generate empty tracker
        mkdir -p "$(dirname "$TRACKER_FILE")"
        _write_tracker "$TRACKER_FILE" "$total_epics" "$active_epics" \
            "$total_tasks" "$completed_tasks" "" "" $'(none)\n'
        echo "Generated: $TRACKER_FILE"
        echo "Epics: 0 (0 active), Tasks: 0 (0 completed)"
        return 0
    fi

    for area_dir in "$EPICS_DIR"/*/; do
        [[ -d "$area_dir" ]] || continue
        local area_folder
        area_folder=$(basename "$area_dir")

        local local_area_count=0
        local local_area_active=0
        local local_area_ongoing=0

        for epic_dir in "$area_dir"/*/; do
            [[ -d "$epic_dir" ]] || continue

            # Find epic file ({EPIC-ID}-epic.md)
            local epic_file=""
            for f in "$epic_dir"*-epic.md; do
                [[ -f "$f" ]] && epic_file="$f" && break
            done
            [[ -z "$epic_file" ]] && continue

            ((total_epics++)) || true
            ((local_area_count++)) || true

            # Extract metadata
            local epic_id title status priority is_ongoing area_type work_type domain
            epic_id=$(basename "$epic_dir")
            title=$(get_frontmatter "$epic_file" "title")
            status=$(get_frontmatter "$epic_file" "status")
            priority=$(get_frontmatter "$epic_file" "priority")
            is_ongoing=$(get_frontmatter "$epic_file" "is_ongoing")
            area_type=$(get_frontmatter "$epic_file" "area_type")
            work_type=$(get_frontmatter "$epic_file" "work_type")
            domain=$(get_frontmatter "$epic_file" "domain")

            # Count as active if not complete/archived
            if [[ "$status" != "complete" && "$status" != "archived" ]]; then
                ((active_epics++)) || true
                ((local_area_active++)) || true
            fi

            if [[ "$is_ongoing" == "true" ]]; then
                ((local_area_ongoing++)) || true
            fi

            # Count tasks
            local tasks_dir="$epic_dir/tasks"
            local task_counts
            task_counts=$(count_tasks_by_status "$tasks_dir")

            # Accumulate global totals from returned counts (subshell-safe)
            local t_todo t_inprog t_comp t_blk
            read -r t_todo t_inprog t_comp t_blk <<< "$task_counts"
            ((total_tasks += t_todo + t_inprog + t_comp + t_blk)) || true
            ((completed_tasks += t_comp)) || true

            # Store epic record (tab-delimited)
            epic_records+="${area_folder}${TAB}${epic_id}${TAB}${title:-Untitled}${TAB}${status:-draft}${TAB}${area_type:-}${TAB}${work_type:-}${TAB}${domain:-}${TAB}${priority:-normal}${TAB}${is_ongoing:-false}${TAB}${task_counts}"$'\n'
        done

        area_records+="${area_folder}${TAB}${local_area_count}${TAB}${local_area_active}${TAB}${local_area_ongoing}"$'\n'
    done

    # Build output sections
    local area_table=""
    while IFS=$'\t' read -r folder count act ong; do
        [[ -z "$folder" ]] && continue
        area_table+="| $folder | ${count:-0} | ${act:-0} | ${ong:-0} |"$'\n'
    done <<< "$area_records"

    local active_section=""
    local completed_section=""
    while IFS=$'\t' read -r area eid title status atype wtype dom pri ongoing tasks; do
        [[ -z "$eid" ]] && continue
        local todo in_prog comp blocked
        read -r todo in_prog comp blocked <<< "$tasks"

        if [[ "$status" == "complete" || "$status" == "archived" ]]; then
            completed_section+="- **${eid}**: ${title} (${status})"$'\n'
        else
            active_section+="### ${eid}: ${title}"$'\n'$'\n'
            active_section+="- **Status:** ${status}"$'\n'
            active_section+="- **Area:** ${atype:-?} | **Type:** ${wtype:-?} | **Domain:** ${dom:-?}"$'\n'
            active_section+="- **Priority:** ${pri} | **Ongoing:** ${ongoing}"$'\n'
            active_section+="- **Tasks:** ${todo:-0} todo, ${in_prog:-0} in_progress, ${comp:-0} complete, ${blocked:-0} blocked"$'\n'
            active_section+="- **Path:** \`project-management/epics/${area}/${eid}/\`"$'\n'$'\n'
        fi
    done <<< "$epic_records"

    [[ -z "$completed_section" ]] && completed_section=$'(none)\n'

    # Generate the tracker file
    mkdir -p "$(dirname "$TRACKER_FILE")"
    _write_tracker "$TRACKER_FILE" "$total_epics" "$active_epics" \
        "$total_tasks" "$completed_tasks" "$area_table" "$active_section" "$completed_section"

    echo "Generated: $TRACKER_FILE"
    echo "Epics: $total_epics ($active_epics active), Tasks: $total_tasks ($completed_tasks completed)"
}

# --- Entry Point ---
main "$@"
