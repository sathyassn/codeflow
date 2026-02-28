#!/usr/bin/env bash
# test-normalize-jsonl.sh - Tests for normalize-jsonl.sh
# Location: .codeflow/testing/scripts/db/test-normalize-jsonl.sh
#
# Tests .codeflow/scripts/db/normalize-jsonl.sh which normalizes JSONL
# ledger files to canonical format.
#
# Test cases:
#   1. Script file exists and is executable
#   2. Help and version flags
#   3. Canonical format conversion (type->event, ts->timestamp)
#   4. Idempotency (running twice produces same output)
#   5. Data preservation (line counts match before/after)
#   6. memory-events.jsonl pattern handling (all 4 formats)
#   7. Empty file handling
#   8. op:INSERT DB format handling
#   9. Already-canonical files pass through unchanged
#
# Usage:
#   ./test-normalize-jsonl.sh           Run all tests
#   ./test-normalize-jsonl.sh -h        Show help

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
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
readonly REPO_ROOT

usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for .codeflow/scripts/db/normalize-jsonl.sh

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

SOURCE_SCRIPT="$REPO_ROOT/.codeflow/scripts/db/normalize-jsonl.sh"

# Helper: create a test ledger directory with sample data
setup_test_ledger() {
    local test_dir
    test_dir=$(mktemp -d "${TMPDIR:-/tmp}/normalize-test-XXXXXX")

    local ledger_dir="$test_dir/.state/ledger"
    mkdir -p "$ledger_dir"

    echo "$test_dir"
}

# Helper: clean up test directory
cleanup_test_dir() {
    local test_dir="$1"
    rm -rf "$test_dir"
}

# ============================================================================
# TEST: Script file exists and is executable
# ============================================================================

test_script_exists() {
    test_section "normalize-jsonl: script exists and is executable"
    assert_file_exists "$SOURCE_SCRIPT" "normalize-jsonl.sh exists"

    if [[ -x "$SOURCE_SCRIPT" ]]; then
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} normalize-jsonl.sh is executable"
    else
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} normalize-jsonl.sh is not executable"
    fi
}

# ============================================================================
# TEST: Help and version flags
# ============================================================================

test_help_and_version() {
    test_section "normalize-jsonl: --help and --version flags"

    assert_success "bash \"$SOURCE_SCRIPT\" --help" "--help exits 0"
    assert_success "bash \"$SOURCE_SCRIPT\" -h" "-h exits 0"
    assert_success "bash \"$SOURCE_SCRIPT\" --version" "--version exits 0"
    assert_success "bash \"$SOURCE_SCRIPT\" -V" "-V exits 0"

    local help_output
    help_output=$(bash "$SOURCE_SCRIPT" --help 2>&1)
    assert_contains "$help_output" "Usage:" "--help shows Usage"
    assert_contains "$help_output" "normalize" "--help mentions normalize"

    local version_output
    version_output=$(bash "$SOURCE_SCRIPT" --version 2>&1)
    assert_contains "$version_output" "1.0.0" "--version shows version number"
}

# ============================================================================
# TEST: Canonical format conversion
# ============================================================================

test_canonical_format_conversion() {
    test_section "normalize-jsonl: canonical format conversion"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create memory-events.jsonl with non-canonical entries
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","event_type":"milestone","domain":"dev","data":{"content":"test"},"ts":"2026-02-11T21:34:59Z"}
{"event":"memory_event","id":"mem-002","event_type":"progress","domain":"dev","data":{"content":"canonical"},"timestamp":"2026-02-12T00:00:00Z"}
JSONL

    # Create other empty/canonical files
    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization
    local output
    output=$(CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" --verbose 2>&1)

    # Check "type":"memory_stored" -> "event":"memory_store"
    local line1
    line1=$(head -1 "$ledger_dir/memory-events.jsonl")
    assert_not_contains "$line1" '"type"' "type field removed after normalization"
    assert_contains "$line1" '"event"' "event field present after normalization"
    assert_contains "$line1" '"memory_store"' "memory_stored normalized to memory_store"

    # Check "ts" -> "timestamp" for non-canonical entries
    assert_contains "$line1" '"timestamp"' "ts normalized to timestamp"
    assert_not_contains "$line1" '"ts":' "ts field removed (not canonical ts+e format)"

    # Check already-canonical line preserved
    local line2
    line2=$(sed -n '2p' "$ledger_dir/memory-events.jsonl")
    assert_contains "$line2" '"event":"memory_event"' "canonical event field preserved"
    assert_contains "$line2" '"timestamp"' "canonical timestamp field preserved"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Idempotency
# ============================================================================

test_idempotency() {
    test_section "normalize-jsonl: idempotency (running twice = same output)"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create test data with mixed formats
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","event_type":"milestone","data":{"content":"test"},"ts":"2026-02-11T00:00:00Z"}
{"event":"memory_event","id":"mem-002","timestamp":"2026-02-12T00:00:00Z","data":{}}
JSONL

    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization first time
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    # Capture output after first run
    local first_run
    first_run=$(cat "$ledger_dir/memory-events.jsonl")

    # Run normalization second time
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    # Capture output after second run
    local second_run
    second_run=$(cat "$ledger_dir/memory-events.jsonl")

    # Compare
    assert_equals "$first_run" "$second_run" "second run produces identical output"

    # Verify second run reports 0 changes
    local output
    output=$(CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" --verbose 2>&1)
    assert_contains "$output" "0 lines changed" "third run reports 0 changes"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Data preservation (line counts)
# ============================================================================

test_data_preservation() {
    test_section "normalize-jsonl: data preservation (counts match)"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create files with known line counts
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","data":{},"ts":"2026-01-01T00:00:00Z"}
{"type":"memory_stored","id":"mem-002","data":{},"ts":"2026-01-02T00:00:00Z"}
{"event":"already_canonical","id":"mem-003","data":{},"timestamp":"2026-01-03T00:00:00Z"}
{"type":"memory_stored","id":"mem-004","data":{},"ts":"2026-01-04T00:00:00Z"}
{"event":"also_canonical","id":"mem-005","data":{},"timestamp":"2026-01-05T00:00:00Z"}
JSONL

    cat > "$ledger_dir/work-graph.jsonl" << 'JSONL'
{"event":"task_created","id":"task-001","timestamp":"2026-01-01T00:00:00Z"}
{"event":"task_created","id":"task-002","timestamp":"2026-01-02T00:00:00Z"}
JSONL

    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    local before_memory_count before_wg_count
    before_memory_count=$(wc -l < "$ledger_dir/memory-events.jsonl" | tr -d ' ')
    before_wg_count=$(wc -l < "$ledger_dir/work-graph.jsonl" | tr -d ' ')

    # Run normalization
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    local after_memory_count after_wg_count
    after_memory_count=$(wc -l < "$ledger_dir/memory-events.jsonl" | tr -d ' ')
    after_wg_count=$(wc -l < "$ledger_dir/work-graph.jsonl" | tr -d ' ')

    assert_equals "$before_memory_count" "$after_memory_count" "memory-events line count preserved ($before_memory_count)"
    assert_equals "$before_wg_count" "$after_wg_count" "work-graph line count preserved ($before_wg_count)"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: memory-events.jsonl pattern handling
# ============================================================================

test_memory_events_patterns() {
    test_section "normalize-jsonl: memory-events.jsonl pattern handling"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create memory-events.jsonl with all known formats from real data
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","event_type":"milestone","domain":"dev","data":{"content":"test"},"ts":"2026-02-11T21:34:59Z"}
{"event":"memory_event","id":"mem-002","event_type":"progress","domain":"dev","data":{"content":"canonical"},"timestamp":"2026-02-12T00:00:00Z"}
{"op":"INSERT","table":"memory_events","data":{"id":"mem-003","event_type":"milestone","domain":"planning","data":{"type":"finding","content":"test"}},"ts":"2026-02-13T19:08:22Z"}
{"ts":"2026-02-17T16:08:09Z","e":"memory_milestone","id":"mem-004","work_id":"work-001","domain":"dev","summary":"test complete"}
JSONL

    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    # Line 1: "type":"memory_stored" -> "event":"memory_store", "ts" -> "timestamp"
    local line1
    line1=$(sed -n '1p' "$ledger_dir/memory-events.jsonl")
    assert_not_contains "$line1" '"type"' "pattern 1: type field removed"
    assert_contains "$line1" '"event":"memory_store"' "pattern 1: event=memory_store"
    assert_contains "$line1" '"timestamp"' "pattern 1: ts normalized to timestamp"

    # Line 2: already canonical - should be unchanged
    local line2
    line2=$(sed -n '2p' "$ledger_dir/memory-events.jsonl")
    assert_contains "$line2" '"event"' "pattern 2: canonical event preserved"
    assert_contains "$line2" '"timestamp"' "pattern 2: canonical timestamp preserved"

    # Line 3: op:INSERT DB format - should be restructured
    local line3
    line3=$(sed -n '3p' "$ledger_dir/memory-events.jsonl")
    assert_not_contains "$line3" '"op"' "pattern 3: op field removed"
    assert_not_contains "$line3" '"table"' "pattern 3: table field removed"
    assert_contains "$line3" '"event"' "pattern 3: event field present"
    assert_contains "$line3" '"timestamp"' "pattern 3: timestamp field present"

    # Line 4: canonical ts+e format - should pass through unchanged
    local line4
    line4=$(sed -n '4p' "$ledger_dir/memory-events.jsonl")
    assert_contains "$line4" '"e"' "pattern 4: canonical e field preserved"
    assert_contains "$line4" '"ts"' "pattern 4: canonical ts field preserved (not renamed)"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Empty file handling
# ============================================================================

test_empty_file_handling() {
    test_section "normalize-jsonl: empty file handling"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create all files as empty
    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/memory-events.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization - should succeed without errors
    local output
    output=$(CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" --verbose 2>&1)
    local exit_code=$?

    assert_equals "0" "$exit_code" "normalization exits 0 with empty files"
    assert_contains "$output" "SKIP" "empty files are skipped"

    # Verify files still exist and are empty
    assert_file_exists "$ledger_dir/config.jsonl" "config.jsonl still exists"
    local config_size
    config_size=$(wc -c < "$ledger_dir/config.jsonl" | tr -d ' ')
    assert_equals "0" "$config_size" "config.jsonl remains empty"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: op:INSERT DB format handling
# ============================================================================

test_op_insert_handling() {
    test_section "normalize-jsonl: op:INSERT DB format entries"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create memory-events.jsonl with op:INSERT entries (matching real data)
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"op":"INSERT","table":"memory_events","data":{"id":"mem-e2c","event_type":"milestone","domain":"planning","work_id":"work-001","data":{"type":"finding","content":"test proposal"}},"ts":"2026-02-13T19:08:22Z"}
{"op": "INSERT", "table": "memory_events", "data": {"id": "mem-019c", "event_type": "progress", "domain": "development", "work_id": "work-001", "data": "{\"summary\": \"User feedback round 2\"}"}, "ts": "2026-02-13T21:28:51.000Z"}
JSONL

    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    # Verify op:INSERT entries are restructured
    local line1
    line1=$(sed -n '1p' "$ledger_dir/memory-events.jsonl")
    assert_not_contains "$line1" '"op"' "op field removed from DB format entry"
    assert_not_contains "$line1" '"table"' "table field removed from DB format entry"
    assert_contains "$line1" '"timestamp"' "timestamp present in restructured entry"
    assert_contains "$line1" '"event"' "event field present in restructured entry"
    assert_contains "$line1" '"id"' "id field preserved in restructured entry"

    # Verify second entry with JSON string data
    local line2
    line2=$(sed -n '2p' "$ledger_dir/memory-events.jsonl")
    assert_not_contains "$line2" '"op"' "op field removed from second entry"
    assert_contains "$line2" '"timestamp"' "timestamp present in second entry"

    # Verify line count preserved
    local count
    count=$(wc -l < "$ledger_dir/memory-events.jsonl" | tr -d ' ')
    assert_equals "2" "$count" "line count preserved (2 lines)"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Already-canonical files pass through
# ============================================================================

test_canonical_passthrough() {
    test_section "normalize-jsonl: canonical files pass through unchanged"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create work-graph.jsonl with canonical format
    cat > "$ledger_dir/work-graph.jsonl" << 'JSONL'
{"event":"task_created","id":"task-001","format_id":"INF-TSK-001-001","title":"Test task","timestamp":"2026-01-01T00:00:00Z"}
{"event":"task_status_changed","task_id":"task-001","new_status":"in_progress","timestamp":"2026-01-02T00:00:00Z"}
JSONL

    cat > "$ledger_dir/sessions.jsonl" << 'JSONL'
{"event":"session_start","session_id":"ses-001","timestamp":"2026-01-01T00:00:00Z"}
JSONL

    touch "$ledger_dir/memory-events.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Capture originals
    local original_wg original_sessions
    original_wg=$(cat "$ledger_dir/work-graph.jsonl")
    original_sessions=$(cat "$ledger_dir/sessions.jsonl")

    # Run normalization
    CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" >/dev/null 2>&1

    # Verify unchanged
    local after_wg after_sessions
    after_wg=$(cat "$ledger_dir/work-graph.jsonl")
    after_sessions=$(cat "$ledger_dir/sessions.jsonl")

    assert_equals "$original_wg" "$after_wg" "work-graph.jsonl unchanged"
    assert_equals "$original_sessions" "$after_sessions" "sessions.jsonl unchanged"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Backup creation
# ============================================================================

test_backup_creation() {
    test_section "normalize-jsonl: backup creation"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create a file with content to normalize
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","data":{},"ts":"2026-01-01T00:00:00Z"}
JSONL

    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Run normalization
    local output
    output=$(CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" --verbose 2>&1)

    # Verify backup directory was created
    local backup_dir="$ledger_dir/backups"
    if [[ -d "$backup_dir" ]]; then
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} backup directory created"
    else
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} backup directory not created"
    fi

    # Verify backup file exists
    local backup_count
    backup_count=$(find "$backup_dir" -name "memory-events.jsonl" -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$backup_count" -gt 0 ]]; then
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} memory-events.jsonl backup file exists"
    else
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} memory-events.jsonl backup file not found"
    fi

    assert_contains "$output" "Backups saved to" "output mentions backup location"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Dry-run mode
# ============================================================================

test_dry_run() {
    test_section "normalize-jsonl: dry-run mode"

    local test_dir
    test_dir=$(setup_test_ledger)
    local ledger_dir="$test_dir/.state/ledger"

    # Create a file with non-canonical entries
    cat > "$ledger_dir/memory-events.jsonl" << 'JSONL'
{"type":"memory_stored","id":"mem-001","data":{},"ts":"2026-01-01T00:00:00Z"}
JSONL

    touch "$ledger_dir/work-graph.jsonl"
    touch "$ledger_dir/sessions.jsonl"
    touch "$ledger_dir/config.jsonl"

    # Capture original content
    local original
    original=$(cat "$ledger_dir/memory-events.jsonl")

    # Run dry-run
    local output
    output=$(CODEFLOW_LEDGER_PATH="$ledger_dir" bash "$SOURCE_SCRIPT" --dry-run 2>&1)

    # Verify file is unchanged
    local after
    after=$(cat "$ledger_dir/memory-events.jsonl")
    assert_equals "$original" "$after" "file unchanged in dry-run mode"

    assert_contains "$output" "DRY-RUN" "output shows DRY-RUN prefix"
    assert_contains "$output" "No files modified" "output confirms no modification"

    cleanup_test_dir "$test_dir"
}

# ============================================================================
# TEST: Missing ledger directory
# ============================================================================

test_missing_ledger_dir() {
    test_section "normalize-jsonl: missing ledger directory"

    local exit_code=0
    local output
    output=$(CODEFLOW_LEDGER_PATH="/nonexistent/path" bash "$SOURCE_SCRIPT" 2>&1) || exit_code=$?

    # Should exit with error
    assert_not_equals "0" "$exit_code" "exits with error for missing dir"
    assert_contains "$output" "Error" "error message for missing dir"
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) usage; exit 0 ;;
            -V|--version) echo "$SCRIPT_NAME version $SCRIPT_VERSION"; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: normalize-jsonl.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    test_script_exists
    test_help_and_version
    test_canonical_format_conversion
    test_idempotency
    test_data_preservation
    test_memory_events_patterns
    test_empty_file_handling
    test_op_insert_handling
    test_canonical_passthrough
    test_backup_creation
    test_dry_run
    test_missing_ledger_dir

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
