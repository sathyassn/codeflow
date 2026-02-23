#!/usr/bin/env bash
# test-stage-sync.sh - Tests for state/cf-stage-sync.py
# Location: .codeflow/testing/scripts/state/test-stage-sync.sh
#
# Usage:
#   ./test-stage-sync.sh       Run all tests
#   ./test-stage-sync.sh -h    Show help
#   ./test-stage-sync.sh -V    Show version

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

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for state/cf-stage-sync.py two-tier stage transition sync script.

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

# Path to the script under test
STAGE_SYNC="$REPO_ROOT/.codeflow/scripts/state/cf-stage-sync.py"
readonly STAGE_SYNC

# Python interpreter
PYTHON="${PYTHON:-python3}"
readonly PYTHON

# ============================================================================
# TEST SETUP
# ============================================================================

setup_test_stage_sync() {
    setup_test_dir "stage-sync"

    # Create mock repo structure with .codeflow so _find_repo_root() works
    export MOCK_REPO="$TEST_DIR/repo"
    mkdir -p "$MOCK_REPO/.codeflow"
    mkdir -p "$MOCK_REPO/.state/db"
    mkdir -p "$MOCK_REPO/.state/ledger"
    mkdir -p "$MOCK_REPO/.state/logs/db"
    mkdir -p "$MOCK_REPO/epics"
    mkdir -p "$MOCK_REPO/.claude/memory/development"

    # Create test DB with V4 stage columns
    export TEST_DB="$MOCK_REPO/.state/db/codeflow.db"
    sqlite3 "$TEST_DB" <<'SCHEMA'
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS epics (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'planning',
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    epic_id TEXT,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'in_progress', 'complete', 'blocked')),
    stage TEXT CHECK(stage IN ('dev', 'work', 'review', 'qa', 'done') OR stage IS NULL),
    stage_status TEXT CHECK(stage_status IN ('pending', 'in_progress', 'complete', 'failed') OR stage_status IS NULL),
    stage_history TEXT DEFAULT '[]',
    created_at TEXT DEFAULT (datetime('now')),
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (epic_id) REFERENCES epics(id)
);
CREATE TABLE IF NOT EXISTS active_work (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    topic TEXT,
    branch TEXT,
    scope TEXT DEFAULT '[]',
    scope_policy TEXT DEFAULT 'soft',
    current_stage TEXT,
    status TEXT DEFAULT 'in_progress'
        CHECK(status IN ('in_progress', 'complete', 'abandoned')),
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS memory_events (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    work_id TEXT,
    session_id TEXT,
    data TEXT NOT NULL DEFAULT '{}',
    memory_type TEXT DEFAULT 'episodic',
    created_at TEXT DEFAULT (datetime('now')),
    expires_at TEXT
);
INSERT INTO schema_version (version) VALUES (1);
SCHEMA

    # Set env vars so the script finds mock repo paths
    export CODEFLOW_REPO_ROOT="$MOCK_REPO"
    export CODEFLOW_STATE_DIR="$MOCK_REPO/.state"
}

teardown_test_stage_sync() {
    teardown_test_dir
    unset MOCK_REPO TEST_DB CODEFLOW_REPO_ROOT CODEFLOW_STATE_DIR
}

# Helper: run cf-stage-sync.py with cwd set to mock repo
run_stage_sync() {
    (cd "$MOCK_REPO" && "$PYTHON" "$STAGE_SYNC" "$@" 2>/dev/null)
}

# Helper: run and capture both stdout and exit code
run_stage_sync_capture() {
    local output
    local exit_code
    output=$(cd "$MOCK_REPO" && "$PYTHON" "$STAGE_SYNC" "$@" 2>/dev/null) || exit_code=$?
    exit_code=${exit_code:-0}
    echo "$output"
    return "$exit_code"
}

# ============================================================================
# TEST: --help
# ============================================================================

test_help_exits_zero() {
    test_section "--help: exits 0"

    local output
    output=$("$PYTHON" "$STAGE_SYNC" --help 2>&1)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--help exits with code 0"
    else
        test_fail "--help exited with code $rc"
    fi

    assert_contains "$output" "Two-tier stage transition sync" "--help shows description"
    assert_contains "$output" "--sync-all" "--help shows --sync-all"
    assert_contains "$output" "--from-ledger" "--help shows --from-ledger"
    assert_contains "$output" "--sync-markdown" "--help shows --sync-markdown"
    assert_contains "$output" "--task-id" "--help shows --task-id"
    assert_contains "$output" "--json" "--help shows --json"
}

# ============================================================================
# TEST: --from-ledger with no/empty ledger
# ============================================================================

test_from_ledger_no_file() {
    test_section "--from-ledger: no ledger file"

    setup_test_stage_sync

    # No ledger file exists — should still succeed
    local output
    output=$(run_stage_sync --from-ledger --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--from-ledger exits 0 with no ledger file"
    else
        test_fail "--from-ledger exited with code $rc"
    fi

    assert_contains "$output" '"success": true' "JSON reports success"
    assert_contains "$output" '"events_processed": 0' "0 events processed"

    teardown_test_stage_sync
}

test_from_ledger_empty_file() {
    test_section "--from-ledger: empty ledger file"

    setup_test_stage_sync

    # Create empty ledger file
    touch "$MOCK_REPO/.state/ledger/memory-events.jsonl"

    local output
    output=$(run_stage_sync --from-ledger --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--from-ledger exits 0 with empty ledger"
    else
        test_fail "--from-ledger exited with code $rc"
    fi

    assert_contains "$output" '"success": true' "JSON reports success"

    teardown_test_stage_sync
}

test_from_ledger_text_output() {
    test_section "--from-ledger: text output"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --from-ledger)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--from-ledger text mode exits 0"
    else
        test_fail "--from-ledger text mode exited with code $rc"
    fi

    assert_contains "$output" "0" "Output contains 0 (tasks processed)"

    teardown_test_stage_sync
}

# ============================================================================
# TEST: --sync-all with empty DB
# ============================================================================

test_sync_all_empty_db() {
    test_section "--sync-all: empty DB"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --sync-all --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--sync-all exits 0 with empty DB"
    else
        test_fail "--sync-all exited with code $rc"
    fi

    assert_contains "$output" '"success": true' "JSON reports success"
    assert_contains "$output" '"total": 0' "0 total items"
    assert_contains "$output" '"synced": 0' "0 synced"

    teardown_test_stage_sync
}

test_sync_all_text_output() {
    test_section "--sync-all: text output"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --sync-all)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--sync-all text mode exits 0"
    else
        test_fail "--sync-all text mode exited with code $rc"
    fi

    assert_contains "$output" "0" "Output contains 0"

    teardown_test_stage_sync
}

# ============================================================================
# TEST: --task-id with nonexistent ID returns proper error
# ============================================================================

test_task_id_nonexistent() {
    test_section "--task-id: nonexistent task"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --task-id NONEXISTENT-TASK --stage dev --stage-status pending --json) || true

    assert_contains "$output" '"success": true' "JSON reports success for ledger append"

    teardown_test_stage_sync
}

test_sync_markdown_nonexistent() {
    test_section "--sync-markdown: nonexistent task"

    setup_test_stage_sync

    # --sync-markdown should return success but with no updates
    local output
    output=$(run_stage_sync --task-id NONEXISTENT-TASK --sync-markdown --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "--sync-markdown exits 0 for nonexistent task"
    else
        test_fail "--sync-markdown exited with code $rc"
    fi

    assert_contains "$output" '"success": true' "JSON reports success"

    teardown_test_stage_sync
}

# ============================================================================
# TEST: --task-id with missing required args
# ============================================================================

test_task_id_missing_stage() {
    test_section "--task-id: missing --stage"

    # --task-id alone without --stage and --stage-status should error
    local output rc=0
    output=$("$PYTHON" "$STAGE_SYNC" --task-id TSK-001 2>&1) || rc=$?

    if [[ $rc -ne 0 ]]; then
        test_pass "Exits non-zero when --stage missing"
    else
        test_fail "Should exit non-zero when --stage missing"
    fi
}

test_task_id_missing_stage_status() {
    test_section "--task-id: missing --stage-status"

    local output rc=0
    output=$("$PYTHON" "$STAGE_SYNC" --task-id TSK-001 --stage dev 2>&1) || rc=$?

    if [[ $rc -ne 0 ]]; then
        test_pass "Exits non-zero when --stage-status missing"
    else
        test_fail "Should exit non-zero when --stage-status missing"
    fi
}

# ============================================================================
# TEST: --task-id with invalid stage value
# ============================================================================

test_invalid_stage_value() {
    test_section "--task-id: invalid stage"

    local output rc=0
    output=$("$PYTHON" "$STAGE_SYNC" --task-id TSK-001 --stage invalid_stage --stage-status pending 2>&1) || rc=$?

    if [[ $rc -ne 0 ]]; then
        test_pass "Exits non-zero for invalid stage"
    else
        test_fail "Should reject invalid stage value"
    fi

    assert_contains "$output" "invalid choice" "Error mentions invalid choice"
}

test_invalid_stage_status_value() {
    test_section "--task-id: invalid stage-status"

    local output rc=0
    output=$("$PYTHON" "$STAGE_SYNC" --task-id TSK-001 --stage dev --stage-status bogus 2>&1) || rc=$?

    if [[ $rc -ne 0 ]]; then
        test_pass "Exits non-zero for invalid stage-status"
    else
        test_fail "Should reject invalid stage-status value"
    fi

    assert_contains "$output" "invalid choice" "Error mentions invalid choice"
}

# ============================================================================
# TEST: --json produces valid JSON
# ============================================================================

test_json_from_ledger() {
    test_section "--json: from-ledger output"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --from-ledger --json)

    # Validate with python json module
    if echo "$output" | "$PYTHON" -m json.tool >/dev/null 2>&1; then
        test_pass "--from-ledger --json produces valid JSON"
    else
        test_fail "--from-ledger --json output is not valid JSON: $output"
    fi

    teardown_test_stage_sync
}

test_json_sync_all() {
    test_section "--json: sync-all output"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --sync-all --json)

    if echo "$output" | "$PYTHON" -m json.tool >/dev/null 2>&1; then
        test_pass "--sync-all --json produces valid JSON"
    else
        test_fail "--sync-all --json output is not valid JSON: $output"
    fi

    # Check required keys
    assert_contains "$output" '"success"' "JSON has success key"
    assert_contains "$output" '"total"' "JSON has total key"
    assert_contains "$output" '"synced"' "JSON has synced key"
    assert_contains "$output" '"errors"' "JSON has errors key"

    teardown_test_stage_sync
}

test_json_sync_markdown() {
    test_section "--json: sync-markdown output"

    setup_test_stage_sync

    local output
    output=$(run_stage_sync --task-id NONEXISTENT --sync-markdown --json)

    if echo "$output" | "$PYTHON" -m json.tool >/dev/null 2>&1; then
        test_pass "--sync-markdown --json produces valid JSON"
    else
        test_fail "--sync-markdown --json output is not valid JSON: $output"
    fi

    assert_contains "$output" '"success"' "JSON has success key"

    teardown_test_stage_sync
}

# ============================================================================
# TEST: Mode detection (is-pathflow-active flag)
# ============================================================================

test_mode_flag_absent() {
    test_section "Mode detection: flag absent"

    setup_test_stage_sync

    # Ensure flag does NOT exist
    local flag_file="$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}/pathflow/is-pathflow-active"
    local flag_existed=false
    if [[ -f "$flag_file" ]]; then
        flag_existed=true
        mv "$flag_file" "${flag_file}.bak"
    fi

    # Run sync-all and check output (standalone mode = no PathFlow references)
    # We can't directly test the Python function from shell, but we can verify
    # the script runs without error in standalone mode
    local output
    output=$(run_stage_sync --sync-all --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "Script runs in standalone mode (flag absent)"
    else
        test_fail "Script failed in standalone mode (exit $rc)"
    fi

    # Restore flag if it existed
    if [[ "$flag_existed" == "true" ]]; then
        mv "${flag_file}.bak" "$flag_file"
    fi

    teardown_test_stage_sync
}

test_mode_flag_present() {
    test_section "Mode detection: flag present"

    setup_test_stage_sync

    # Create the flag file
    local flag_dir="$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}/pathflow"
    local flag_file="$flag_dir/is-pathflow-active"
    local flag_existed=false
    if [[ -f "$flag_file" ]]; then
        flag_existed=true
    else
        mkdir -p "$flag_dir"
        touch "$flag_file"
    fi

    # Script should run without error in PathFlow mode
    local output
    output=$(run_stage_sync --sync-all --json)
    local rc=$?

    if [[ $rc -eq 0 ]]; then
        test_pass "Script runs in PathFlow mode (flag present)"
    else
        test_fail "Script failed in PathFlow mode (exit $rc)"
    fi

    # Clean up flag if we created it
    if [[ "$flag_existed" == "false" ]]; then
        rm -f "$flag_file"
    fi

    teardown_test_stage_sync
}

# ============================================================================
# TEST: No arguments
# ============================================================================

test_no_args() {
    test_section "No arguments: errors"

    local output rc=0
    output=$("$PYTHON" "$STAGE_SYNC" 2>&1) || rc=$?

    if [[ $rc -ne 0 ]]; then
        test_pass "Exits non-zero with no arguments"
    else
        test_fail "Should exit non-zero with no arguments"
    fi
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
    if ! command -v "$PYTHON" &>/dev/null; then
        echo "ERROR: $PYTHON not found" >&2
        exit 1
    fi
    if [[ ! -f "$STAGE_SYNC" ]]; then
        echo "ERROR: cf-stage-sync.py not found at $STAGE_SYNC" >&2
        exit 1
    fi

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: state/cf-stage-sync.py${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # --help tests
    test_help_exits_zero

    # --from-ledger tests
    test_from_ledger_no_file
    test_from_ledger_empty_file
    test_from_ledger_text_output

    # --sync-all tests
    test_sync_all_empty_db
    test_sync_all_text_output

    # Nonexistent task tests
    test_task_id_nonexistent
    test_sync_markdown_nonexistent

    # Missing required args tests
    test_task_id_missing_stage
    test_task_id_missing_stage_status

    # Invalid values tests
    test_invalid_stage_value
    test_invalid_stage_status_value

    # JSON output tests
    test_json_from_ledger
    test_json_sync_all
    test_json_sync_markdown

    # Mode detection tests
    test_mode_flag_absent
    test_mode_flag_present

    # No arguments test
    test_no_args

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
