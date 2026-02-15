#!/usr/bin/env bash
# cf-pathflow-task-update.sh - Record PathFlow task status updates in JSONL
#
# Location: .codeflow/scripts/pathflow/cf-pathflow-task-update.sh
#
# Writes a pathflow_task_update event to pathflow-events.jsonl.
# PathFlow tasks (PFn-TSK-nn) are ephemeral, session-scoped task markers
# created from pathflow-config.json. Distinct from project tasks.
#
# Usage:
#   cf-pathflow-task-update.sh -s <session_id> -k <task_id> -t <status>
#   cf-pathflow-task-update.sh -s SES-01HX... -k PF3-TSK-01 -t completed

set -euo pipefail

# Load dependencies
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# Constants
readonly PATHFLOW_LEDGER="pathflow-events.jsonl"
readonly VALID_STATUSES="pending in_progress completed skipped blocked"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: $(basename "$0") -s <session_id> -k <task_id> -t <status>

Record a PathFlow task status update.

Required:
  -s  Session ID (e.g., SES-01HXYZ...)
  -k  PathFlow task ID (format: PFn-TSK-nn, e.g., PF3-TSK-01)
  -t  Status: pending, in_progress, completed, skipped, blocked

Optional:
  -h  Show this help message
EOF
}

validate_task_id() {
    local task_id="$1"
    # Format: PFn-TSK-nn where n is 1-7 and nn is 01-99
    [[ "$task_id" =~ ^PF[1-7]-TSK-[0-9]{2}$ ]]
}

validate_status() {
    local status="$1"
    for valid in $VALID_STATUSES; do
        [[ "$status" == "$valid" ]] && return 0
    done
    return 1
}

record_task_update() {
    local session_id="$1"
    local task_id="$2"
    local status="$3"

    # Ensure ledger exists
    # shellcheck disable=SC2153  # LEDGER_PATH exported from sourced ledger.sh
    local ledger_path="$LEDGER_PATH/$PATHFLOW_LEDGER"
    mkdir -p "$(dirname "$ledger_path")"
    [[ -f "$ledger_path" ]] || touch "$ledger_path"

    # Generate event
    local event_id
    event_id=$(generate_event_id)

    local event
    event=$(jq -c -n \
        --arg id "$event_id" \
        --arg type "pathflow_task_update" \
        --arg session_id "$session_id" \
        --arg task_id "$task_id" \
        --arg status "$status" \
        '{id: $id, type: $type, session_id: $session_id, task_id: $task_id, status: $status}')

    append_event "$PATHFLOW_LEDGER" "$event"

    # Output success
    jq -c -n \
        --arg event_id "$event_id" \
        --arg task_id "$task_id" \
        --arg status "$status" \
        '{status: "recorded", event_id: $event_id, task_id: $task_id, task_status: $status}'
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local session_id=""
    local task_id=""
    local status=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -s) session_id="$2"; shift 2 ;;
            -k) task_id="$2"; shift 2 ;;
            -t) status="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done

    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required (-s)"}' >&2
        exit 2
    fi

    if [[ -z "$task_id" ]]; then
        echo '{"error":"task_id is required (-k)"}' >&2
        exit 2
    fi

    if ! validate_task_id "$task_id"; then
        echo "{\"error\":\"invalid task_id format: $task_id. Expected: PFn-TSK-nn\"}" >&2
        exit 2
    fi

    if [[ -z "$status" ]]; then
        echo '{"error":"status is required (-t)"}' >&2
        exit 2
    fi

    if ! validate_status "$status"; then
        echo "{\"error\":\"invalid status: $status. Valid: $VALID_STATUSES\"}" >&2
        exit 2
    fi

    record_task_update "$session_id" "$task_id" "$status"
}

main "$@"
