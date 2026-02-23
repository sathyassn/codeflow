#!/usr/bin/env bash
# test-validate-task.sh - Tests for validation/validate-task.sh
# Location: .codeflow/testing/scripts/validation/test-validate-task.sh
#
# Usage:
#   ./test-validate-task.sh       Run all tests
#   ./test-validate-task.sh -h    Show help
#   ./test-validate-task.sh -V    Show version

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
readonly SCRIPT_VERSION="1.0.0"
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for validation/validate-task.sh.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information

Examples:
    $SCRIPT_NAME              Run all tests
    $SCRIPT_NAME --help       Show this help
EOF
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        -V|--version) echo "$SCRIPT_NAME version $SCRIPT_VERSION"; exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# Script under test
readonly VALIDATE_SCRIPT="$REPO_ROOT/.codeflow/scripts/validation/validate-task.sh"

# ============================================================================
# TEST SETUP / TEARDOWN
# ============================================================================

setup() {
    setup_test_dir "validate-task"
}

teardown() {
    teardown_test_dir
}

# Helper: create a valid task file with all required fields
create_valid_task() {
    local filepath="$TEST_DIR/valid-task.md"
    cat > "$filepath" <<'TASKEOF'
---
id: "task-01KJ12VSN1NV4T1HXQ5YH705G6"
format_id: "INF-TSK-008-001"
epic_id: "epic-01KJ12VSN1YSWYQ03CDK8ENG78"
title: "Test task"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "PMGT"
origin: planned
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
---

# Test task content
TASKEOF
    echo "$filepath"
}

# ============================================================================
# TESTS
# ============================================================================

test_section "validate-task.sh Tests"

# --------------------------------------------------------------------------
# Test 1: Valid task file passes (exit 0)
# --------------------------------------------------------------------------
test_subsection "Valid task file"

setup
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$(create_valid_task)'" \
    "Valid task file passes validation"
teardown

# Also test with a real task file from the project
setup
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$REPO_ROOT/project-management/epics/INF/INF-EPC-008/tasks/INF-TSK-008-001.md'" \
    "Real task file INF-TSK-008-001.md passes validation"
teardown

# --------------------------------------------------------------------------
# Test 2: Missing required field format_id
# --------------------------------------------------------------------------
test_subsection "Missing required fields"

setup
filepath="$TEST_DIR/missing-format-id.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Missing format_id causes failure"
teardown

setup
filepath="$TEST_DIR/missing-epic-id.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Missing epic_id causes failure"
teardown

# --------------------------------------------------------------------------
# Test 3: Invalid status value
# --------------------------------------------------------------------------
test_subsection "Invalid status value"

setup
filepath="$TEST_DIR/invalid-status.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: invalid_status
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid status value causes failure"

# Verify error message mentions the bad status
output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "invalid_status" \
    "Error message mentions the invalid status"
teardown

# --------------------------------------------------------------------------
# Test 4: Invalid format_id pattern
# --------------------------------------------------------------------------
test_subsection "Invalid format_id pattern"

setup
filepath="$TEST_DIR/invalid-format-id.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INVALID"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid format_id pattern causes failure"
teardown

setup
filepath="$TEST_DIR/bad-format-id-2.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-08-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "format_id with wrong digit count causes failure"
teardown

# --------------------------------------------------------------------------
# Test 5: auto_merge=true with null target_branch
# --------------------------------------------------------------------------
test_subsection "auto_merge + target_branch constraint"

setup
filepath="$TEST_DIR/auto-merge-null-target.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
auto_merge: true
target_branch: null
raise_pr: true
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "auto_merge=true with null target_branch fails"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "auto_merge" \
    "Error message mentions auto_merge constraint"
teardown

# --------------------------------------------------------------------------
# Test 6: auto_merge=true with protected branch target
# --------------------------------------------------------------------------
test_subsection "auto_merge + protected branch"

setup
filepath="$TEST_DIR/auto-merge-protected.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
auto_merge: true
target_branch: main
raise_pr: true
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "auto_merge=true with protected branch 'main' fails"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "protected branch" \
    "Error message mentions protected branch"
teardown

# --------------------------------------------------------------------------
# Test 7: raise_pr=false with auto_merge=true
# --------------------------------------------------------------------------
test_subsection "raise_pr + auto_merge constraint"

setup
filepath="$TEST_DIR/raise-pr-false-auto-merge-true.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
auto_merge: true
raise_pr: false
target_branch: "feat/test"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "raise_pr=false with auto_merge=true fails"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "raise_pr" \
    "Error message mentions raise_pr constraint"
teardown

# --------------------------------------------------------------------------
# Test 8: Empty frontmatter
# --------------------------------------------------------------------------
test_subsection "Empty frontmatter"

setup
filepath="$TEST_DIR/empty-frontmatter.md"
printf '%s\n' "---" "---" "# Empty content" > "$filepath"
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Empty frontmatter causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "No frontmatter fields found" \
    "Error message mentions empty frontmatter"
teardown

# --------------------------------------------------------------------------
# Test 9: Valid file with all optional fields null
# --------------------------------------------------------------------------
test_subsection "Valid file with optional fields null"

setup
filepath="$TEST_DIR/optional-null.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task with null optionals"
status: todo
area_type: "INF"
work_type: "FEAT"
domain: null
origin: null
scope_policy: null
scope_root: null
estimate: null
priority: null
assignee_id: null
autorun_eligible: null
auto_commit: null
raise_pr: null
auto_merge: null
target_branch: null
branch: null
pr_number: null
external_id: null
external_url: null
---

# Task content
TASKEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Valid file with all optional fields null passes"
teardown

# --------------------------------------------------------------------------
# Test 10: Help and version flags
# --------------------------------------------------------------------------
test_subsection "Help and version flags"

assert_success "bash '$VALIDATE_SCRIPT' --help" \
    "--help flag exits 0"

assert_success "bash '$VALIDATE_SCRIPT' --version" \
    "--version flag exits 0"

# --------------------------------------------------------------------------
# Test 11: Missing file argument
# --------------------------------------------------------------------------
test_subsection "Missing file argument"

assert_fails "bash '$VALIDATE_SCRIPT' 2>/dev/null" \
    "Missing file argument causes failure"

# --------------------------------------------------------------------------
# Test 12: Non-existent file
# --------------------------------------------------------------------------
test_subsection "Non-existent file"

assert_fails "bash '$VALIDATE_SCRIPT' --quiet /tmp/claude/nonexistent-task-file.md" \
    "Non-existent file causes failure"

# --------------------------------------------------------------------------
# Test 13: All valid statuses accepted
# --------------------------------------------------------------------------
test_subsection "All valid statuses"

for valid_status in todo blocked in_progress complete cancelled; do
    setup
    filepath="$TEST_DIR/status-$valid_status.md"
    cat > "$filepath" <<TASKEOF
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: $valid_status
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
    assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
        "Status '$valid_status' is accepted"
    teardown
done

# --------------------------------------------------------------------------
# Test 14: Multiple errors reported at once
# --------------------------------------------------------------------------
test_subsection "Multiple errors collected"

setup
filepath="$TEST_DIR/multiple-errors.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INVALID"
title: "Test task"
status: bad_status
area_type: "INF"
work_type: "FEAT"
---
TASKEOF
output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "epic_id" \
    "Reports missing epic_id"
assert_contains "$output" "format_id" \
    "Reports invalid format_id"
assert_contains "$output" "bad_status" \
    "Reports invalid status"
teardown

# --------------------------------------------------------------------------
# Test 15: Valid work_type values accepted
# --------------------------------------------------------------------------
test_subsection "Valid work_type values"

for valid_wt in FEAT FIX HTFX RFCT DOCS TEST CHOR CICD SPKE PLAN; do
    setup
    filepath="$TEST_DIR/wt-$valid_wt.md"
    cat > "$filepath" <<TASKEOF
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "$valid_wt"
---
TASKEOF
    assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
        "work_type '$valid_wt' is accepted"
    teardown
done

# --------------------------------------------------------------------------
# Test 16: Invalid work_type value
# --------------------------------------------------------------------------
test_subsection "Invalid work_type value"

setup
filepath="$TEST_DIR/invalid-work-type.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "INVALID"
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid work_type 'INVALID' causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "INVALID" \
    "Error message mentions the invalid work_type"
assert_contains "$output" "work_type" \
    "Error message mentions the field name"
teardown

# --------------------------------------------------------------------------
# Test 17: Tests field warning for code-producing types with code files
# --------------------------------------------------------------------------
test_subsection "Tests field warning for code-producing types"

setup
filepath="$TEST_DIR/code-no-tests.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: ["src/feature.sh"]
tests: []
---
TASKEOF
# Should still pass (warning, not error)
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Code task with empty tests passes (warning only)"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_contains "$output" "WARN" \
    "Warning is emitted for code task without tests"
assert_contains "$output" "tests" \
    "Warning mentions the tests field"
teardown

# --------------------------------------------------------------------------
# Test 18: No warning for DOCS work_type (non-code-producing)
# --------------------------------------------------------------------------
test_subsection "No warning for non-code-producing types"

setup
filepath="$TEST_DIR/docs-no-tests.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test docs task"
status: todo
area_type: "DOC"
work_type: "DOCS"
file_scope: ["docs/guide.md"]
tests: []
---
TASKEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "DOCS task with empty tests passes without warning"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "WARN" \
    "No warning emitted for DOCS work_type"
teardown

# Test PLAN work_type also does not warn
setup
filepath="$TEST_DIR/plan-no-tests.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test plan task"
status: todo
area_type: "PLN"
work_type: "PLAN"
file_scope: ["docs/plan.md"]
tests: []
---
TASKEOF
output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "WARN" \
    "No warning emitted for PLAN work_type"
teardown

# --------------------------------------------------------------------------
# Test 19: No warning when file_scope is empty
# --------------------------------------------------------------------------
test_subsection "No warning when file_scope is empty"

setup
filepath="$TEST_DIR/code-empty-scope.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: []
tests: []
---
TASKEOF
output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "WARN" \
    "No warning when file_scope is empty (no specific files yet)"
teardown

# --------------------------------------------------------------------------
# Test 20: No warning when file_scope has no code files
# --------------------------------------------------------------------------
test_subsection "No warning when file_scope has no code files"

setup
filepath="$TEST_DIR/no-code-files.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
file_scope: ["config/settings.json", "README.md"]
tests: []
---
TASKEOF
output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "WARN" \
    "No warning when file_scope has no .sh or .py files"
teardown

# --------------------------------------------------------------------------
# Test 21: autorun_eligible=true with empty acceptance fails
# --------------------------------------------------------------------------
test_subsection "autorun_eligible + acceptance constraint"

setup
filepath="$TEST_DIR/autorun-no-acceptance.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
autorun_eligible: true
acceptance: []
---
TASKEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "autorun_eligible=true with empty acceptance fails"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "autorun_eligible" \
    "Error message mentions autorun_eligible"
assert_contains "$output" "acceptance" \
    "Error message mentions acceptance"
teardown

# autorun_eligible=true with non-empty acceptance passes
setup
filepath="$TEST_DIR/autorun-with-acceptance.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
autorun_eligible: true
acceptance: ["criterion 1", "criterion 2"]
---
TASKEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "autorun_eligible=true with non-empty acceptance passes"
teardown

# autorun_eligible=false with empty acceptance passes
setup
filepath="$TEST_DIR/no-autorun-no-acceptance.md"
cat > "$filepath" <<'TASKEOF'
---
id: "task-123"
format_id: "INF-TSK-008-001"
epic_id: "epic-456"
title: "Test task"
status: todo
area_type: "INF"
work_type: "FEAT"
autorun_eligible: false
acceptance: []
---
TASKEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "autorun_eligible=false with empty acceptance passes"
teardown

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
