#!/usr/bin/env bash
# test-cf-pathflow-scripts.sh - Tests for PathFlow JSONL interim scripts
# Location: .codeflow/testing/scripts/pathflow/test-cf-pathflow-scripts.sh
#
# Tests all 5 PathFlow JSONL scripts:
#   - cf-pathflow-session-register.sh
#   - cf-pathflow-phase-transition.sh
#   - cf-pathflow-stage-transition.sh
#   - cf-pathflow-task-update.sh
#   - cf-pathflow-session-metadata.sh
#
# Usage:
#   ./test-cf-pathflow-scripts.sh       Run all tests
#   ./test-cf-pathflow-scripts.sh -h    Show help
#   ./test-cf-pathflow-scripts.sh -V    Show version

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="1.0.0"
readonly SCRIPT_VERSION
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT
export REPO_ROOT

# PathFlow script paths
PATHFLOW_DIR="$REPO_ROOT/.codeflow/scripts/pathflow"
readonly PATHFLOW_DIR

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for PathFlow JSONL interim scripts.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information

Examples:
    $SCRIPT_NAME              Run all tests
    $SCRIPT_NAME --help       Show this help
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# ============================================================================
# TEST SETUP / TEARDOWN
# ============================================================================

setup_pathflow_test() {
    setup_test_dir "pathflow"
    export CODEFLOW_LEDGER_PATH="$TEST_DIR/ledger"
    export LEDGER_PATH="$CODEFLOW_LEDGER_PATH"
    mkdir -p "$LEDGER_PATH"
}

teardown_pathflow_test() {
    teardown_test_dir
    unset CODEFLOW_LEDGER_PATH
    unset LEDGER_PATH
}

# Helper: count lines in pathflow-events.jsonl
count_events() {
    local ledger="$LEDGER_PATH/pathflow-events.jsonl"
    if [[ -f "$ledger" ]]; then
        wc -l < "$ledger" | tr -d ' '
    else
        echo "0"
    fi
}

# Helper: get last event from JSONL
last_event() {
    local ledger="$LEDGER_PATH/pathflow-events.jsonl"
    [[ -f "$ledger" ]] && tail -1 "$ledger"
}

# Helper: get field from JSON
json_field() {
    local json="$1"
    local field="$2"
    echo "$json" | jq -r ".$field // empty"
}

# ============================================================================
# TEST: cf-pathflow-session-register.sh
# ============================================================================

test_session_register_basic() {
    test_section "session-register: basic registration"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-TEST001")

    # Check stdout JSON
    assert_contains "$output" '"status":"registered"' "Output contains status=registered"
    assert_contains "$output" '"session_id":"SES-TEST001"' "Output contains session_id"
    assert_contains "$output" '"tracking_level":"pending"' "Output contains tracking_level=pending"
    assert_contains "$output" '"interaction_mode":"interactive"' "Output contains default mode=interactive"

    # Check JSONL was written (2 events: tracking_level + interaction_mode)
    local count
    count=$(count_events)
    assert_equals "2" "$count" "Two events written to JSONL"

    # Check first event (tracking_level)
    local first_event
    first_event=$(head -1 "$LEDGER_PATH/pathflow-events.jsonl")
    assert_contains "$first_event" '"type":"session_metadata"' "First event type is session_metadata"
    assert_contains "$first_event" '"key":"tracking_level"' "First event key is tracking_level"
    assert_contains "$first_event" '"value":"pending"' "First event value is pending"
    assert_contains "$first_event" '"session_id":"SES-TEST001"' "First event has session_id"

    # Check second event (interaction_mode)
    local second_event
    second_event=$(tail -1 "$LEDGER_PATH/pathflow-events.jsonl")
    assert_contains "$second_event" '"key":"interaction_mode"' "Second event key is interaction_mode"
    assert_contains "$second_event" '"value":"interactive"' "Second event value is interactive"

    teardown_pathflow_test
}

test_session_register_autorun() {
    test_section "session-register: autorun mode"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-TEST002" -m autorun)

    assert_contains "$output" '"interaction_mode":"autorun"' "Output contains mode=autorun"

    local second_event
    second_event=$(tail -1 "$LEDGER_PATH/pathflow-events.jsonl")
    assert_contains "$second_event" '"value":"autorun"' "JSONL records autorun mode"

    teardown_pathflow_test
}

test_session_register_missing_session_id() {
    test_section "session-register: missing session_id"

    setup_pathflow_test

    local output exit_code=0
    output=$("$PATHFLOW_DIR/cf-pathflow-session-register.sh" 2>&1) || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for missing session_id"

    teardown_pathflow_test
}

test_session_register_invalid_mode() {
    test_section "session-register: invalid mode"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-TEST003" -m "badmode" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid mode"

    teardown_pathflow_test
}

test_session_register_event_ids() {
    test_section "session-register: unique event IDs"

    setup_pathflow_test

    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-TEST004" >/dev/null

    local first_id second_id
    first_id=$(head -1 "$LEDGER_PATH/pathflow-events.jsonl" | jq -r '.id')
    second_id=$(tail -1 "$LEDGER_PATH/pathflow-events.jsonl" | jq -r '.id')

    assert_matches "$first_id" "^EVT-" "First event ID has EVT- prefix"
    assert_matches "$second_id" "^EVT-" "Second event ID has EVT- prefix"
    assert_not_equals "$first_id" "$second_id" "Event IDs are unique"

    teardown_pathflow_test
}

test_session_register_timestamp() {
    test_section "session-register: timestamp added"

    setup_pathflow_test

    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-TEST005" >/dev/null

    local event
    event=$(head -1 "$LEDGER_PATH/pathflow-events.jsonl")
    assert_contains "$event" '"ts":"' "Event has timestamp"

    teardown_pathflow_test
}

# ============================================================================
# TEST: cf-pathflow-phase-transition.sh
# ============================================================================

test_phase_transition_entered() {
    test_section "phase-transition: entered"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-PH001" -p "PF1-INIT" -t "entered")

    assert_contains "$output" '"status":"recorded"' "Output contains status=recorded"
    assert_contains "$output" '"phase":"PF1-INIT"' "Output contains phase"
    assert_contains "$output" '"transition":"entered"' "Output contains transition=entered"

    local event
    event=$(last_event)
    assert_contains "$event" '"type":"phase_transition"' "JSONL event type is phase_transition"
    assert_contains "$event" '"phase":"PF1-INIT"' "JSONL has phase"
    assert_contains "$event" '"status":"entered"' "JSONL has status=entered"
    assert_contains "$event" '"session_id":"SES-PH001"' "JSONL has session_id"

    teardown_pathflow_test
}

test_phase_transition_completed() {
    test_section "phase-transition: completed"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-PH002" -p "PF4-EXECUTE" -t "completed")

    assert_contains "$output" '"transition":"completed"' "Output transition is completed"

    local event
    event=$(last_event)
    assert_contains "$event" '"status":"completed"' "JSONL status is completed"
    assert_contains "$event" '"phase":"PF4-EXECUTE"' "Phase is PF4-EXECUTE"

    teardown_pathflow_test
}

test_phase_transition_all_phases() {
    test_section "phase-transition: all valid phases"

    setup_pathflow_test

    local phases="PF1-INIT PF2-CONTEXT PF3-CLASSIFY PF4-EXECUTE PF5-VERIFY PF6-COMPLETE PF7-END"
    for phase in $phases; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-ALL" -p "$phase" -t "entered" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Phase $phase accepted"
        else
            test_fail "Phase $phase rejected (exit $exit_code)"
        fi
    done

    local count
    count=$(count_events)
    assert_equals "7" "$count" "All 7 phases recorded"

    teardown_pathflow_test
}

test_phase_transition_invalid_phase() {
    test_section "phase-transition: invalid phase"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-BAD" -p "PF99-INVALID" -t "entered" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid phase"

    teardown_pathflow_test
}

test_phase_transition_invalid_status() {
    test_section "phase-transition: invalid status"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-BAD" -p "PF1-INIT" -t "running" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid status"

    teardown_pathflow_test
}

test_phase_transition_missing_args() {
    test_section "phase-transition: missing args"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-X" -p "PF1-INIT" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing status rejected"

    exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-X" -t "entered" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing phase rejected"

    exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -p "PF1-INIT" -t "entered" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing session_id rejected"

    teardown_pathflow_test
}

# ============================================================================
# TEST: cf-pathflow-stage-transition.sh
# ============================================================================

test_stage_transition_in_progress() {
    test_section "stage-transition: in_progress"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-ST001" -g "WS-DEV" -t "in_progress" -i 1)

    assert_contains "$output" '"status":"recorded"' "Output status=recorded"
    assert_contains "$output" '"stage":"WS-DEV"' "Output has stage"
    assert_contains "$output" '"transition":"in_progress"' "Output has transition"
    assert_contains "$output" '"iteration":1' "Output has iteration"

    local event
    event=$(last_event)
    assert_contains "$event" '"type":"stage_transition"' "JSONL type is stage_transition"
    assert_contains "$event" '"stage":"WS-DEV"' "JSONL has stage"
    assert_contains "$event" '"iteration":1' "JSONL has iteration"

    teardown_pathflow_test
}

test_stage_transition_complete_with_verdict() {
    test_section "stage-transition: complete with verdict"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-ST002" -g "WS-REV" -t "complete" -v "approved")

    assert_contains "$output" '"verdict":"approved"' "Output has verdict"

    local event
    event=$(last_event)
    assert_contains "$event" '"verdict":"approved"' "JSONL has verdict"
    assert_contains "$event" '"status":"complete"' "JSONL has status=complete"

    teardown_pathflow_test
}

test_stage_transition_all_stages() {
    test_section "stage-transition: all valid stages"

    setup_pathflow_test

    local stages="WS-DEV WS-PLAN WS-DOCS WS-TEST WS-REV WS-QA"
    for stage in $stages; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-ALL" -g "$stage" -t "in_progress" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Stage $stage accepted"
        else
            test_fail "Stage $stage rejected (exit $exit_code)"
        fi
    done

    local count
    count=$(count_events)
    assert_equals "6" "$count" "All 6 stages recorded"

    teardown_pathflow_test
}

test_stage_transition_all_verdicts() {
    test_section "stage-transition: all valid verdicts"

    setup_pathflow_test

    local verdicts="pass fail approved changes_requested"
    for verdict in $verdicts; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-VRD" -g "WS-REV" -t "complete" -v "$verdict" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Verdict $verdict accepted"
        else
            test_fail "Verdict $verdict rejected (exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

test_stage_transition_invalid_stage() {
    test_section "stage-transition: invalid stage"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-BAD" -g "WS-INVALID" -t "in_progress" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid stage"

    teardown_pathflow_test
}

test_stage_transition_invalid_verdict() {
    test_section "stage-transition: invalid verdict"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-BAD" -g "WS-REV" -t "complete" -v "maybe" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid verdict"

    teardown_pathflow_test
}

test_stage_transition_default_iteration() {
    test_section "stage-transition: default iteration"

    setup_pathflow_test

    "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-DEF" -g "WS-DEV" -t "in_progress" >/dev/null

    local event
    event=$(last_event)
    assert_contains "$event" '"iteration":1' "Default iteration is 1"

    teardown_pathflow_test
}

test_stage_transition_no_verdict() {
    test_section "stage-transition: complete without verdict"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-NV" -g "WS-DEV" -t "complete")

    # Should succeed without verdict
    assert_contains "$output" '"status":"recorded"' "Accepted without verdict"

    local event
    event=$(last_event)
    # Should NOT contain verdict field
    local has_verdict
    has_verdict=$(echo "$event" | jq 'has("verdict")')
    assert_equals "false" "$has_verdict" "No verdict field in event"

    teardown_pathflow_test
}

# ============================================================================
# TEST: cf-pathflow-task-update.sh
# ============================================================================

test_task_update_basic() {
    test_section "task-update: basic"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-TU001" -k "PF3-TSK-01" -t "completed")

    assert_contains "$output" '"status":"recorded"' "Output status=recorded"
    assert_contains "$output" '"task_id":"PF3-TSK-01"' "Output has task_id"
    assert_contains "$output" '"task_status":"completed"' "Output has task_status"

    local event
    event=$(last_event)
    assert_contains "$event" '"type":"pathflow_task_update"' "JSONL type is pathflow_task_update"
    assert_contains "$event" '"task_id":"PF3-TSK-01"' "JSONL has task_id"
    assert_contains "$event" '"status":"completed"' "JSONL has status"

    teardown_pathflow_test
}

test_task_update_all_statuses() {
    test_section "task-update: all valid statuses"

    setup_pathflow_test

    local statuses="pending in_progress completed skipped blocked"
    for status in $statuses; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-STS" -k "PF4-TSK-01" -t "$status" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Status $status accepted"
        else
            test_fail "Status $status rejected (exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

test_task_update_valid_task_ids() {
    test_section "task-update: valid task ID formats"

    setup_pathflow_test

    local task_ids="PF1-TSK-01 PF3-TSK-05 PF7-TSK-99 PF4-TSK-10"
    for task_id in $task_ids; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-IDS" -k "$task_id" -t "pending" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Task ID $task_id accepted"
        else
            test_fail "Task ID $task_id rejected (exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

test_task_update_invalid_task_ids() {
    test_section "task-update: invalid task ID formats"

    setup_pathflow_test

    local bad_ids="PF0-TSK-01 PF8-TSK-01 PF3-TSK-1 TSK-01 PF3-TSK-100 TASK-01"
    for task_id in $bad_ids; do
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-BAD" -k "$task_id" -t "pending" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 2 ]]; then
            test_pass "Task ID $task_id rejected"
        else
            test_fail "Task ID $task_id should be rejected (got exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

test_task_update_invalid_status() {
    test_section "task-update: invalid status"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-BAD" -k "PF3-TSK-01" -t "done" >/dev/null 2>&1 || exit_code=$?

    assert_equals "2" "$exit_code" "Exit code 2 for invalid status"

    teardown_pathflow_test
}

test_task_update_missing_args() {
    test_section "task-update: missing args"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-X" -k "PF3-TSK-01" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing status rejected"

    exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-X" -t "pending" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing task_id rejected"

    teardown_pathflow_test
}

# ============================================================================
# TEST: cf-pathflow-session-metadata.sh
# ============================================================================

test_session_metadata_basic() {
    test_section "session-metadata: basic"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-MD001" -k "work_type" -v "FEAT")

    assert_contains "$output" '"status":"recorded"' "Output status=recorded"
    assert_contains "$output" '"key":"work_type"' "Output has key"
    assert_contains "$output" '"value":"FEAT"' "Output has value"

    local event
    event=$(last_event)
    assert_contains "$event" '"type":"session_metadata"' "JSONL type is session_metadata"
    assert_contains "$event" '"key":"work_type"' "JSONL has key"
    assert_contains "$event" '"value":"FEAT"' "JSONL has value"
    assert_contains "$event" '"session_id":"SES-MD001"' "JSONL has session_id"

    teardown_pathflow_test
}

test_session_metadata_known_keys() {
    test_section "session-metadata: known keys"

    setup_pathflow_test

    local keys_values="work_type:FEAT area_type:FRT tracking_level:tracked branch:feat/test task_id:TSK-001 interaction_mode:autorun"
    for kv in $keys_values; do
        local key="${kv%%:*}"
        local value="${kv#*:}"
        local exit_code=0
        "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-KEYS" -k "$key" -v "$value" >/dev/null 2>&1 || exit_code=$?
        if [[ $exit_code -eq 0 ]]; then
            test_pass "Key $key accepted"
        else
            test_fail "Key $key rejected (exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

test_session_metadata_custom_key() {
    test_section "session-metadata: custom key"

    setup_pathflow_test

    local output
    output=$("$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-CUST" -k "custom_key" -v "custom_value")

    assert_contains "$output" '"key":"custom_key"' "Custom key accepted"
    assert_contains "$output" '"value":"custom_value"' "Custom value recorded"

    teardown_pathflow_test
}

test_session_metadata_missing_args() {
    test_section "session-metadata: missing args"

    setup_pathflow_test

    local exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-X" -k "work_type" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing value rejected"

    exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-X" -v "FEAT" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing key rejected"

    exit_code=0
    "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -k "work_type" -v "FEAT" >/dev/null 2>&1 || exit_code=$?
    assert_equals "2" "$exit_code" "Missing session_id rejected"

    teardown_pathflow_test
}

# ============================================================================
# TEST: JSONL Integration
# ============================================================================

test_jsonl_valid_format() {
    test_section "JSONL: all events are valid JSON"

    setup_pathflow_test

    # Write events from multiple scripts
    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-INT001" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-INT001" -p "PF1-INIT" -t "entered" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-INT001" -k "work_type" -v "FEAT" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-INT001" -g "WS-DEV" -t "in_progress" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-INT001" -k "PF4-TSK-01" -t "pending" >/dev/null

    # Validate every line is valid JSON
    local ledger="$LEDGER_PATH/pathflow-events.jsonl"
    local line_num=0
    local errors=0
    while IFS= read -r line; do
        ((line_num++)) || true
        if ! echo "$line" | jq . >/dev/null 2>&1; then
            test_fail "Line $line_num is invalid JSON: $line"
            ((errors++)) || true
        fi
    done < "$ledger"

    if [[ $errors -eq 0 ]]; then
        test_pass "All $line_num lines are valid JSON"
    fi

    teardown_pathflow_test
}

test_jsonl_all_events_have_required_fields() {
    test_section "JSONL: all events have required fields"

    setup_pathflow_test

    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-FLD001" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-FLD001" -p "PF1-INIT" -t "entered" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-stage-transition.sh" -s "SES-FLD001" -g "WS-DEV" -t "in_progress" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-task-update.sh" -s "SES-FLD001" -k "PF4-TSK-01" -t "pending" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-session-metadata.sh" -s "SES-FLD001" -k "branch" -v "main" >/dev/null

    local ledger="$LEDGER_PATH/pathflow-events.jsonl"
    local line_num=0
    while IFS= read -r line; do
        ((line_num++)) || true
        # Every event must have: ts, id, type, session_id
        local has_ts has_id has_type has_sid
        has_ts=$(echo "$line" | jq 'has("ts")')
        has_id=$(echo "$line" | jq 'has("id")')
        has_type=$(echo "$line" | jq 'has("type")')
        has_sid=$(echo "$line" | jq 'has("session_id")')

        if [[ "$has_ts" == "true" && "$has_id" == "true" && "$has_type" == "true" && "$has_sid" == "true" ]]; then
            test_pass "Event $line_num has all required fields"
        else
            test_fail "Event $line_num missing fields (ts=$has_ts, id=$has_id, type=$has_type, session_id=$has_sid)"
        fi
    done < "$ledger"

    teardown_pathflow_test
}

test_jsonl_event_ordering() {
    test_section "JSONL: events maintain insertion order"

    setup_pathflow_test

    "$PATHFLOW_DIR/cf-pathflow-session-register.sh" -s "SES-ORD001" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-ORD001" -p "PF1-INIT" -t "entered" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-ORD001" -p "PF1-INIT" -t "completed" >/dev/null
    "$PATHFLOW_DIR/cf-pathflow-phase-transition.sh" -s "SES-ORD001" -p "PF2-CONTEXT" -t "entered" >/dev/null

    local ledger="$LEDGER_PATH/pathflow-events.jsonl"

    # First two are session_metadata (from register), then 3 phase_transitions
    local count
    count=$(wc -l < "$ledger" | tr -d ' ')
    assert_equals "5" "$count" "5 events total (2 register + 3 phase transitions)"

    # Check 3rd event is first phase transition (PF1-INIT entered)
    local third_event
    third_event=$(sed -n '3p' "$ledger")
    assert_contains "$third_event" '"phase":"PF1-INIT"' "3rd event is PF1-INIT"
    assert_contains "$third_event" '"status":"entered"' "3rd event is entered"

    # Check 5th event is PF2-CONTEXT entered
    local fifth_event
    fifth_event=$(sed -n '5p' "$ledger")
    assert_contains "$fifth_event" '"phase":"PF2-CONTEXT"' "5th event is PF2-CONTEXT"

    teardown_pathflow_test
}

test_jsonl_help_flag() {
    test_section "All scripts: -h flag"

    setup_pathflow_test

    local scripts="cf-pathflow-session-register.sh cf-pathflow-phase-transition.sh cf-pathflow-stage-transition.sh cf-pathflow-task-update.sh cf-pathflow-session-metadata.sh"
    for script in $scripts; do
        local output exit_code=0
        output=$("$PATHFLOW_DIR/$script" -h 2>&1) || exit_code=$?
        if [[ $exit_code -eq 0 ]] && [[ "$output" == *"Usage"* ]]; then
            test_pass "$script -h shows usage"
        else
            test_fail "$script -h failed (exit $exit_code)"
        fi
    done

    teardown_pathflow_test
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                echo "$SCRIPT_NAME version $SCRIPT_VERSION"
                exit 0
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
        shift
    done

    # Check prerequisites
    if ! command -v jq &>/dev/null; then
        echo "ERROR: jq is required but not installed" >&2
        exit 1
    fi

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: PathFlow JSONL Interim Scripts${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # session-register tests
    test_session_register_basic
    test_session_register_autorun
    test_session_register_missing_session_id
    test_session_register_invalid_mode
    test_session_register_event_ids
    test_session_register_timestamp

    # phase-transition tests
    test_phase_transition_entered
    test_phase_transition_completed
    test_phase_transition_all_phases
    test_phase_transition_invalid_phase
    test_phase_transition_invalid_status
    test_phase_transition_missing_args

    # stage-transition tests
    test_stage_transition_in_progress
    test_stage_transition_complete_with_verdict
    test_stage_transition_all_stages
    test_stage_transition_all_verdicts
    test_stage_transition_invalid_stage
    test_stage_transition_invalid_verdict
    test_stage_transition_default_iteration
    test_stage_transition_no_verdict

    # task-update tests
    test_task_update_basic
    test_task_update_all_statuses
    test_task_update_valid_task_ids
    test_task_update_invalid_task_ids
    test_task_update_invalid_status
    test_task_update_missing_args

    # session-metadata tests
    test_session_metadata_basic
    test_session_metadata_known_keys
    test_session_metadata_custom_key
    test_session_metadata_missing_args

    # JSONL integration tests
    test_jsonl_valid_format
    test_jsonl_all_events_have_required_fields
    test_jsonl_event_ordering
    test_jsonl_help_flag

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
