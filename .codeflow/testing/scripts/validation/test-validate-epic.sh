#!/usr/bin/env bash
# test-validate-epic.sh - Tests for validation/validate-epic.sh
# Location: .codeflow/testing/scripts/validation/test-validate-epic.sh
#
# Usage:
#   ./test-validate-epic.sh       Run all tests
#   ./test-validate-epic.sh -h    Show help
#   ./test-validate-epic.sh -V    Show version

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

Tests for validation/validate-epic.sh.

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
readonly VALIDATE_SCRIPT="$REPO_ROOT/.codeflow/scripts/validation/validate-epic.sh"

# ============================================================================
# TEST SETUP / TEARDOWN
# ============================================================================

setup() {
    setup_test_dir "validate-epic"
}

teardown() {
    teardown_test_dir
}

# Helper: create a valid epic file with all required fields
create_valid_epic() {
    local filepath="$TEST_DIR/valid-epic.md"
    cat > "$filepath" <<'EPICEOF'
---
id: "epic-01KJ12VSN1YSWYQ03CDK8ENG78"
format_id: "INF-EPC-008"
title: "Test Epic"
status: in_progress
area_type: "INF"
work_type: "CHOR"
domain: "PMGT"
is_ongoing: false
priority: normal
---

# Test epic content
EPICEOF
    echo "$filepath"
}

# ============================================================================
# TESTS
# ============================================================================

test_section "validate-epic.sh Tests"

# --------------------------------------------------------------------------
# Test 1: Valid epic file passes (exit 0)
# --------------------------------------------------------------------------
test_subsection "Valid epic file"

setup
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$(create_valid_epic)'" \
    "Valid epic file passes validation"
teardown

# Also test with a real epic file from the project
setup
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$REPO_ROOT/project-management/epics/INF/INF-EPC-008/INF-EPC-008.md'" \
    "Real epic file INF-EPC-008.md passes validation"
teardown

# --------------------------------------------------------------------------
# Test 2: Missing required field format_id
# --------------------------------------------------------------------------
test_subsection "Missing required fields"

setup
filepath="$TEST_DIR/missing-format-id.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Missing format_id causes failure"
teardown

setup
filepath="$TEST_DIR/missing-title.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Missing title causes failure"
teardown

# --------------------------------------------------------------------------
# Test 3: Invalid status value
# --------------------------------------------------------------------------
test_subsection "Invalid status value"

setup
filepath="$TEST_DIR/invalid-status.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: todo
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid status 'todo' causes failure (not valid for epics)"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "todo" \
    "Error message mentions the invalid status"
teardown

# --------------------------------------------------------------------------
# Test 4: Invalid format_id pattern
# --------------------------------------------------------------------------
test_subsection "Invalid format_id pattern"

setup
filepath="$TEST_DIR/invalid-format-id.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INVALID"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid format_id pattern causes failure"
teardown

setup
filepath="$TEST_DIR/task-format-id.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-TSK-008-001"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Task format_id (TSK) used in epic causes failure"
teardown

setup
filepath="$TEST_DIR/bad-format-id-digits.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-08"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "format_id with wrong digit count causes failure"
teardown

# --------------------------------------------------------------------------
# Test 5: Empty frontmatter
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
# Test 6: All valid statuses accepted
# --------------------------------------------------------------------------
test_subsection "All valid statuses"

for valid_status in draft planning in_progress blocked complete archived; do
    setup
    filepath="$TEST_DIR/status-$valid_status.md"
    cat > "$filepath" <<EPICEOF
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: $valid_status
area_type: "INF"
---
EPICEOF
    assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
        "Status '$valid_status' is accepted"
    teardown
done

# --------------------------------------------------------------------------
# Test 7: Help and version flags
# --------------------------------------------------------------------------
test_subsection "Help and version flags"

assert_success "bash '$VALIDATE_SCRIPT' --help" \
    "--help flag exits 0"

assert_success "bash '$VALIDATE_SCRIPT' --version" \
    "--version flag exits 0"

# --------------------------------------------------------------------------
# Test 8: Missing file argument
# --------------------------------------------------------------------------
test_subsection "Missing file argument"

assert_fails "bash '$VALIDATE_SCRIPT' 2>/dev/null" \
    "Missing file argument causes failure"

# --------------------------------------------------------------------------
# Test 9: Non-existent file
# --------------------------------------------------------------------------
test_subsection "Non-existent file"

assert_fails "bash '$VALIDATE_SCRIPT' --quiet /tmp/claude/nonexistent-epic-file.md" \
    "Non-existent file causes failure"

# --------------------------------------------------------------------------
# Test 10: Multiple errors reported at once
# --------------------------------------------------------------------------
test_subsection "Multiple errors collected"

setup
filepath="$TEST_DIR/multiple-errors.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INVALID"
status: bad_status
area_type: "INF"
---
EPICEOF
output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "title" \
    "Reports missing title"
assert_contains "$output" "format_id" \
    "Reports invalid format_id"
assert_contains "$output" "bad_status" \
    "Reports invalid status"
teardown

# --------------------------------------------------------------------------
# Test 11: Valid epic with optional fields null
# --------------------------------------------------------------------------
test_subsection "Valid epic with optional fields null"

setup
filepath="$TEST_DIR/optional-null.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic with null optionals"
status: draft
area_type: "INF"
work_type: null
domain: null
is_ongoing: null
priority: null
pr_number: null
external_id: null
external_url: null
---

# Epic content
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Valid epic with all optional fields null passes"
teardown

# --------------------------------------------------------------------------
# Test 12: Valid work_type values accepted
# --------------------------------------------------------------------------
test_subsection "Valid work_type values"

for valid_wt in FEAT FIX HTFX RFCT DOCS TEST CHOR CICD SPKE PLAN; do
    setup
    filepath="$TEST_DIR/wt-$valid_wt.md"
    cat > "$filepath" <<EPICEOF
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "$valid_wt"
---
EPICEOF
    assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
        "work_type '$valid_wt' is accepted"
    teardown
done

# --------------------------------------------------------------------------
# Test 13: Invalid work_type value
# --------------------------------------------------------------------------
test_subsection "Invalid work_type value"

setup
filepath="$TEST_DIR/invalid-work-type.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "INVALID"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid work_type 'INVALID' causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "INVALID" \
    "Error message mentions the invalid work_type"
assert_contains "$output" "work_type" \
    "Error message mentions the field name"
teardown

# --------------------------------------------------------------------------
# Test 14: Null work_type is accepted (optional field)
# --------------------------------------------------------------------------
test_subsection "Null work_type accepted"

setup
filepath="$TEST_DIR/null-work-type.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: null
---
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Null work_type passes (field is optional for epics)"
teardown

# --------------------------------------------------------------------------
# Test 15: Placeholder ID rejected
# --------------------------------------------------------------------------
test_subsection "Placeholder ID rejected"

setup
filepath="$TEST_DIR/placeholder-id.md"
cat > "$filepath" <<'EPICEOF'
---
id: "PLACEHOLDER-epic-id"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Placeholder ID causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "placeholder" \
    "Error message mentions placeholder"
teardown

# Case-insensitive placeholder detection
setup
filepath="$TEST_DIR/placeholder-id-lowercase.md"
cat > "$filepath" <<'EPICEOF'
---
id: "some-placeholder-value"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Lowercase placeholder ID causes failure"
teardown

setup
filepath="$TEST_DIR/placeholder-id-mixed.md"
cat > "$filepath" <<'EPICEOF'
---
id: "PlAcEhOlDeR_epic"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Mixed-case placeholder ID causes failure"
teardown

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
