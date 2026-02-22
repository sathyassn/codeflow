#!/usr/bin/env bash
# test-tracking-generate.sh - Tests for state/cf-tracking-generate.sh
# Location: .codeflow/testing/scripts/state/test-tracking-generate.sh
#
# Usage:
#   ./test-tracking-generate.sh       Run all tests
#   ./test-tracking-generate.sh -h    Show help

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

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for state/cf-tracking-generate.sh.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Source test isolation
# shellcheck disable=SC2034  # TEST_DIR used by sourced test-isolation.sh
TEST_DIR="$SCRIPT_DIR"
source "$TESTING_DIR/lib/test-isolation.sh"

SCRIPT_UNDER_TEST="$REAL_REPO_ROOT/.codeflow/scripts/state/cf-tracking-generate.sh"

# ============================================================================
# TEST SETUP
# ============================================================================

setup_tracking_env() {
    rm -rf "$REPO_ROOT/project-management"
    mkdir -p "$REPO_ROOT/project-management/epics"
    mkdir -p "$REPO_ROOT/project-management/tracking"
}

create_epic() {
    local area="$1" epic_id="$2" title="$3" status="${4:-active}" priority="${5:-normal}"
    local is_ongoing="${6:-false}"
    local epic_dir="$REPO_ROOT/project-management/epics/$area/$epic_id"
    mkdir -p "$epic_dir/tasks"
    cat > "$epic_dir/${epic_id}-epic.md" <<EPICEOF
---
title: $title
status: $status
priority: $priority
area_type: $area
work_type: feature
domain: test
is_ongoing: $is_ongoing
---

# $title

Epic description.
EPICEOF
}

create_task() {
    local epic_dir="$1" task_name="$2" status="${3:-todo}"
    cat > "$epic_dir/tasks/${task_name}.md" <<TASKEOF
---
status: $status
---

# $task_name
TASKEOF
}

# ============================================================================
# TESTS
# ============================================================================

test_script_exists() {
    test_section "Script exists"
    if [[ -f "$SCRIPT_UNDER_TEST" ]]; then
        test_pass "cf-tracking-generate.sh exists"
    else
        test_fail "cf-tracking-generate.sh not found"
    fi
}

test_script_has_shebang() {
    test_section "Script has shebang"
    local first_line
    first_line=$(head -1 "$SCRIPT_UNDER_TEST")
    if [[ "$first_line" == "#!/usr/bin/env bash" ]]; then
        test_pass "Has correct shebang"
    else
        test_fail "Expected #!/usr/bin/env bash, got: $first_line"
    fi
}

test_empty_project() {
    test_section "Empty project generates tracker"
    setup_tracking_env
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    if [[ -f "$tracker" ]]; then
        test_pass "Tracker file created"
    else
        test_fail "Tracker file not created"
        return
    fi
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Total Epics | 0"; then
        test_pass "Shows 0 total epics"
    else
        test_fail "Should show 0 total epics"
    fi
}

test_single_active_epic() {
    test_section "Single active epic"
    setup_tracking_env
    create_epic "INF" "INF-EPA-AUTH-001" "Auth System" "active" "high"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Total Epics | 1"; then
        test_pass "Shows 1 total epic"
    else
        test_fail "Should show 1 total epic"
    fi
    if echo "$content" | grep -q "Active Epics | 1"; then
        test_pass "Shows 1 active epic"
    else
        test_fail "Should show 1 active epic"
    fi
    if echo "$content" | grep -q "INF-EPA-AUTH-001"; then
        test_pass "Epic ID in output"
    else
        test_fail "Epic ID should be in output"
    fi
}

test_completed_epic() {
    test_section "Completed epic not active"
    setup_tracking_env
    create_epic "INF" "INF-EPC-DONE-001" "Done Feature" "complete"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Active Epics | 0"; then
        test_pass "Completed epic not active"
    else
        test_fail "Completed epic should not be active"
    fi
}

test_multiple_areas() {
    test_section "Multiple areas"
    setup_tracking_env
    create_epic "frontend" "FE-EPC-UI-001" "UI Redesign" "active"
    create_epic "backend" "BE-EPC-API-001" "API Layer" "active"
    create_epic "INF" "INF-EPA-CI-001" "CI Pipeline" "active"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Total Epics | 3"; then
        test_pass "Shows 3 total epics"
    else
        test_fail "Should show 3 total epics"
    fi
    if echo "$content" | grep -q "frontend"; then
        test_pass "Frontend area in table"
    else
        test_fail "Frontend area should be in table"
    fi
}

test_task_counting() {
    test_section "Task counting"
    setup_tracking_env
    create_epic "INF" "INF-EPA-TSK-001" "Task Test" "active"
    local epic_dir="$REPO_ROOT/project-management/epics/INF/INF-EPA-TSK-001"
    create_task "$epic_dir" "task-001" "todo"
    create_task "$epic_dir" "task-002" "in_progress"
    create_task "$epic_dir" "task-003" "complete"
    create_task "$epic_dir" "task-004" "complete"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "1 todo"; then
        test_pass "Todo count in epic detail"
    else
        test_fail "Should show 1 todo in epic detail"
    fi
    if echo "$content" | grep -q "1 in_progress"; then
        test_pass "In-progress count in epic detail"
    else
        test_fail "Should show 1 in_progress in epic detail"
    fi
    if echo "$content" | grep -q "2 complete"; then
        test_pass "Complete count in epic detail"
    else
        test_fail "Should show 2 complete in epic detail"
    fi
}

test_output_format() {
    test_section "Output has required sections"
    setup_tracking_env
    create_epic "shared" "SH-EPC-FMT-001" "Format Test" "active"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "GENERATED"; then
        test_pass "Has GENERATED notice"
    else
        test_fail "Should have GENERATED notice"
    fi
    if echo "$content" | grep -q "generated_at:"; then
        test_pass "Has generated_at timestamp"
    else
        test_fail "Should have generated_at"
    fi
    if echo "$content" | grep -q "## Summary"; then
        test_pass "Has Summary section"
    else
        test_fail "Should have Summary section"
    fi
    if echo "$content" | grep -q "## Active Epics"; then
        test_pass "Has Active Epics section"
    else
        test_fail "Should have Active Epics section"
    fi
    if echo "$content" | grep -q "## Completed Epics"; then
        test_pass "Has Completed Epics section"
    else
        test_fail "Should have Completed Epics section"
    fi
}

test_stdout_output() {
    test_section "stdout reports generation"
    setup_tracking_env
    create_epic "backend" "BE-EPC-STD-001" "Stdout Test" "active"
    local output
    output=$(bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1) || true
    if echo "$output" | grep -q "Generated:"; then
        test_pass "Reports Generated: on stdout"
    else
        test_fail "Should report Generated:"
    fi
    if echo "$output" | grep -q "Epics:"; then
        test_pass "Reports epic counts"
    else
        test_fail "Should report epic counts"
    fi
}

test_archived_not_active() {
    test_section "Archived epics not active"
    setup_tracking_env
    create_epic "shared" "SH-EPC-ARC-001" "Archived Epic" "archived"
    create_epic "shared" "SH-EPC-ACT-001" "Active Epic" "active"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Total Epics | 2"; then
        test_pass "Total includes archived"
    else
        test_fail "Should count 2 total"
    fi
    if echo "$content" | grep -q "Active Epics | 1"; then
        test_pass "Active excludes archived"
    else
        test_fail "Should count 1 active"
    fi
}

test_creates_tracking_dir() {
    test_section "Creates tracking directory"
    setup_tracking_env
    rm -rf "$REPO_ROOT/project-management/tracking"
    create_epic "frontend" "FE-EPC-DIR-001" "Dir Test" "active"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    if [[ -d "$REPO_ROOT/project-management/tracking" ]]; then
        test_pass "Tracking directory created"
    else
        test_fail "Should create tracking directory"
    fi
}

test_epic_without_tasks() {
    test_section "Epic without tasks"
    setup_tracking_env
    create_epic "backend" "BE-EPC-NOTSK-001" "No Tasks Epic" "active"
    # Do not create any tasks - just the epic with empty tasks dir
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    if [[ -f "$tracker" ]]; then
        test_pass "Handles epic without tasks"
    else
        test_fail "Should handle epic without tasks"
    fi
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "BE-EPC-NOTSK-001"; then
        test_pass "Epic ID present in output"
    else
        test_fail "Epic ID should be in output"
    fi
}

test_quoted_title_stripped() {
    test_section "Quoted titles have quotes stripped"
    setup_tracking_env
    local epic_dir="$REPO_ROOT/project-management/epics/INF/INF-EPC-QT-001"
    mkdir -p "$epic_dir/tasks"
    cat > "$epic_dir/INF-EPC-QT-001-epic.md" <<'EPICEOF'
---
title: "Dual-ID System: ULID Primary Keys"
status: active
priority: high
area_type: INF
work_type: feature
domain: test
is_ongoing: false
---

# Quoted Title Epic
EPICEOF
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    # Should contain title without surrounding quotes
    if echo "$content" | grep -q 'Dual-ID System: ULID Primary Keys'; then
        test_pass "Title present in output"
    else
        test_fail "Title should be in output"
    fi
    # Should NOT contain title wrapped in quotes (i.e. no leading ")
    if echo "$content" | grep -q '### INF-EPC-QT-001: "Dual-ID'; then
        test_fail "Title should NOT have surrounding quotes"
    else
        test_pass "Quotes stripped from title"
    fi
}

test_blocked_task_counting() {
    test_section "Blocked task counting"
    setup_tracking_env
    create_epic "shared" "SH-EPC-BLK-001" "Blocked Test" "active"
    local epic_dir="$REPO_ROOT/project-management/epics/shared/SH-EPC-BLK-001"
    create_task "$epic_dir" "task-001" "blocked"
    create_task "$epic_dir" "task-002" "todo"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "1 blocked"; then
        test_pass "Blocked task counted"
    else
        test_fail "Should show 1 blocked task"
    fi
}

test_no_epics_directory() {
    test_section "No epics directory generates tracker"
    rm -rf "$REPO_ROOT/project-management"
    mkdir -p "$REPO_ROOT/project-management/tracking"
    # epics/ does not exist
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    if [[ -f "$tracker" ]]; then
        test_pass "Tracker created without epics dir"
    else
        test_fail "Should create tracker even without epics dir"
        return
    fi
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "Total Epics | 0"; then
        test_pass "Shows 0 epics"
    else
        test_fail "Should show 0 epics"
    fi
}

test_ongoing_epic_tracking() {
    test_section "Ongoing epic tracking"
    setup_tracking_env
    create_epic "INF" "INF-EPC-ONG-001" "Ongoing Epic" "active" "normal" "true"
    create_epic "INF" "INF-EPC-REG-001" "Regular Epic" "active" "normal" "false"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    # Area table should show 1 ongoing
    if echo "$content" | grep -q "| INF | 2 | 2 | 1 |"; then
        test_pass "Ongoing count correct in area table"
    else
        test_fail "Should show 1 ongoing in INF area"
    fi
}

test_invalid_repo_root() {
    test_section "Invalid repo root exits with error"
    local output
    output=$(bash "$SCRIPT_UNDER_TEST" "/nonexistent/path" 2>&1) || true
    if echo "$output" | grep -q "Error:"; then
        test_pass "Error message on invalid repo root"
    else
        test_fail "Should show error on invalid repo root"
    fi
}

test_help_flag() {
    test_section "Help flag"
    local output
    output=$(bash "$SCRIPT_UNDER_TEST" -h 2>&1) || true
    if echo "$output" | grep -q "Usage:"; then
        test_pass "Help flag shows usage"
    else
        test_fail "Should show usage on -h"
    fi
}

test_regenerate_command_updated() {
    test_section "Regenerate command references cf- prefix"
    setup_tracking_env
    create_epic "backend" "BE-EPC-CMD-001" "Command Test" "active"
    bash "$SCRIPT_UNDER_TEST" "$REPO_ROOT" 2>&1 || true
    local tracker="$REPO_ROOT/project-management/tracking/epic-tracker.md"
    local content
    content=$(cat "$tracker")
    if echo "$content" | grep -q "cf-tracking-generate.sh"; then
        test_pass "Regenerate command has cf- prefix"
    else
        test_fail "Regenerate command should reference cf-tracking-generate.sh"
    fi
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
    echo -e "${BOLD}Testing: state/cf-tracking-generate.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    test_script_exists
    test_script_has_shebang
    test_empty_project
    test_single_active_epic
    test_completed_epic
    test_multiple_areas
    test_task_counting
    test_output_format
    test_stdout_output
    test_archived_not_active
    test_creates_tracking_dir
    test_epic_without_tasks
    test_quoted_title_stripped
    test_blocked_task_counting
    test_no_epics_directory
    test_ongoing_epic_tracking
    test_invalid_repo_root
    test_help_flag
    test_regenerate_command_updated

    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
