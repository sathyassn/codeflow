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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
assert_contains "$output" "work_type" \
    "Reports missing work_type"
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
work_type: "CHOR"
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
# Test 14: Missing work_type fails (now required)
# --------------------------------------------------------------------------
test_subsection "Missing work_type fails"

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
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Null work_type fails (field is now required)"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "work_type" \
    "Error mentions work_type"
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
work_type: "CHOR"
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
work_type: "CHOR"
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
work_type: "CHOR"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Mixed-case placeholder ID causes failure"
teardown

# --------------------------------------------------------------------------
# Test 16: Template sentinel ID rejected (curly braces)
# --------------------------------------------------------------------------
test_subsection "Template sentinel ID rejected"

setup
filepath="$TEST_DIR/template-id.md"
cat > "$filepath" <<'EPICEOF'
---
id: "{epic-ULID}"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "CHOR"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Template sentinel epic ID causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "template sentinel" \
    "Error message mentions template sentinel"
teardown

# --------------------------------------------------------------------------
# Test 17: Invalid area_type enum
# --------------------------------------------------------------------------
test_subsection "Invalid area_type enum"

setup
filepath="$TEST_DIR/invalid-area-type.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INFRA"
work_type: "CHOR"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid area_type 'INFRA' causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "area_type" \
    "Error message mentions area_type"
assert_contains "$output" "INFRA" \
    "Error message mentions the invalid value"
teardown

# --------------------------------------------------------------------------
# Test 18: format_id prefix vs area_type cross-check
# --------------------------------------------------------------------------
test_subsection "format_id prefix vs area_type cross-check"

setup
filepath="$TEST_DIR/prefix-mismatch.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "FRT-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "CHOR"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "format_id prefix FRT vs area_type INF causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "prefix" \
    "Error mentions prefix mismatch"
assert_contains "$output" "area_type" \
    "Error mentions area_type"
teardown

# --------------------------------------------------------------------------
# Test 19: Title template sentinel rejected
# --------------------------------------------------------------------------
test_subsection "Title template sentinel rejected"

setup
filepath="$TEST_DIR/template-title.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "{Title}"
status: draft
area_type: "INF"
work_type: "CHOR"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Template sentinel in title causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "title" \
    "Error mentions title field"
assert_contains "$output" "template sentinel" \
    "Error mentions template sentinel"
teardown

# --------------------------------------------------------------------------
# Test 20: Missing work_type fails (now required)
# --------------------------------------------------------------------------
test_subsection "Missing work_type fails"

setup
filepath="$TEST_DIR/missing-work-type.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Missing work_type causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "work_type" \
    "Error mentions work_type"
teardown

# --------------------------------------------------------------------------
# Test 21: Invalid priority enum
# --------------------------------------------------------------------------
test_subsection "Invalid priority enum"

setup
filepath="$TEST_DIR/invalid-priority.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "CHOR"
priority: "hight"
---
EPICEOF
assert_fails "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Invalid priority 'hight' causes failure"

output=$(bash "$VALIDATE_SCRIPT" --quiet "$filepath" 2>&1 || true)
assert_contains "$output" "priority" \
    "Error mentions priority field"
teardown

# --------------------------------------------------------------------------
# Test 22: Filename vs format_id cross-check (warning)
# --------------------------------------------------------------------------
test_subsection "Filename vs format_id cross-check"

setup
filepath="$TEST_DIR/INF-EPC-009.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-123"
format_id: "INF-EPC-008"
title: "Test Epic"
status: draft
area_type: "INF"
work_type: "CHOR"
---
EPICEOF
# Should still pass (warning, not error)
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Filename mismatch is warning only (still passes)"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_contains "$output" "WARN" \
    "Warning emitted for filename mismatch"
assert_contains "$output" "INF-EPC-009" \
    "Warning mentions the filename"
teardown

# --------------------------------------------------------------------------
# Test 23: PII file pattern in file_scope triggers warning
# --------------------------------------------------------------------------
test_subsection "PII file pattern detection"

setup
filepath="$TEST_DIR/INF-EPC-010.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-pii-warn"
format_id: "INF-EPC-010"
title: "Auth system overhaul"
status: planning
area_type: "INF"
work_type: "FEAT"
file_scope: ["internal/auth/handler.go", "internal/auth/token.go"]
---
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "PII pattern is warning only (still passes)"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_contains "$output" "WARN" \
    "Warning emitted for PII file pattern"
assert_contains "$output" "PII" \
    "Warning mentions PII"
teardown

# --------------------------------------------------------------------------
# Test 24: No PII warning when file_scope has no PII patterns
# --------------------------------------------------------------------------
test_subsection "No PII warning for clean file_scope"

setup
filepath="$TEST_DIR/INF-EPC-011.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-no-pii"
format_id: "INF-EPC-011"
title: "Config refactor"
status: planning
area_type: "INF"
work_type: "RFCT"
file_scope: ["internal/config/loader.go", "internal/config/parser.go"]
---
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "No PII patterns passes cleanly"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "PII" \
    "No PII warning when no PII patterns present"
teardown

# --------------------------------------------------------------------------
# Test 25: PII warning for credential-related file_scope
# --------------------------------------------------------------------------
test_subsection "PII warning for credential pattern"

setup
filepath="$TEST_DIR/INF-EPC-012.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-pii-cred"
format_id: "INF-EPC-012"
title: "Credential manager"
status: draft
area_type: "INF"
work_type: "FEAT"
file_scope: ["internal/credential/store.go"]
---
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Credential PII pattern is warning only"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_contains "$output" "PII" \
    "Warning mentions PII for credential pattern"
teardown

# --------------------------------------------------------------------------
# Test 26: No PII warning when file_scope is empty array
# --------------------------------------------------------------------------
test_subsection "No PII warning for empty file_scope"

setup
filepath="$TEST_DIR/INF-EPC-013.md"
cat > "$filepath" <<'EPICEOF'
---
id: "epic-empty-scope"
format_id: "INF-EPC-013"
title: "Empty scope epic"
status: draft
area_type: "INF"
work_type: "CHOR"
file_scope: []
---
EPICEOF
assert_success "bash '$VALIDATE_SCRIPT' --quiet '$filepath'" \
    "Empty file_scope passes cleanly"

output=$(bash "$VALIDATE_SCRIPT" "$filepath" 2>&1 || true)
assert_not_contains "$output" "PII" \
    "No PII warning for empty file_scope"
teardown

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
