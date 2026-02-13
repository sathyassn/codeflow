#!/usr/bin/env bash
# Purpose:   Test cf-worktree-list.sh functionality
# Usage:     bash test-cf-worktree-list.sh
# Platform:  macOS/Linux

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck disable=SC2034  # REPO_ROOT used by sourced test-common.sh and test-isolation.sh
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# shellcheck disable=SC2034
TEST_DIR="$SCRIPT_DIR"
source "$TESTING_DIR/lib/test-isolation.sh"

SCRIPT_UNDER_TEST="$REAL_REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-list.sh"

create_test_worktrees_yaml() {
    local state_dir="$TEST_REPO_ROOT/.state"
    mkdir -p "$state_dir"
    cat > "$state_dir/worktrees.yaml" << 'YAML'
# Worktree Tracking
worktrees:
  - path: ".git-worktrees/feat-auth"
    branch: "feat/auth-jwt"
    status: active
    last_active: "2026-02-10T10:00:00Z"
  - path: ".git-worktrees/fix-bug"
    branch: "fix/memory-leak"
    status: stale
    last_active: "2026-01-15T08:00:00Z"
  - path: ".git-worktrees/feat-api"
    branch: "feat/api-v2"
    status: merged
    last_active: "2026-02-01T12:00:00Z"
  - path: ".git-worktrees/old-feature"
    branch: "feat/old-thing"
    status: abandoned
    last_active: "2025-12-01T06:00:00Z"

metadata:
  version: "1.0.0"
  last_updated: "2026-02-10T10:00:00Z"
YAML
}

test_section "Help and Version"

output=$("$SCRIPT_UNDER_TEST" --help 2>&1)
assert_contains "$output" "cf-worktree-list.sh" "--help shows script name"
assert_contains "$output" "--format" "--help shows --format option"
assert_contains "$output" "--status" "--help shows --status option"
assert_contains "$output" "SKILL REFERENCE" "--help shows skill references"
assert_contains "$output" "cf-git-workflow" "--help references git-workflow skill"

output=$("$SCRIPT_UNDER_TEST" -h 2>&1)
assert_contains "$output" "cf-worktree-list.sh" "-h shows help"

output=$("$SCRIPT_UNDER_TEST" --version 2>&1)
assert_contains "$output" "version" "--version shows version"
assert_contains "$output" "1.0.0" "--version shows version number"

output=$("$SCRIPT_UNDER_TEST" -V 2>&1)
assert_contains "$output" "version" "-V shows version"

test_section "Script Basics"
assert_file_exists "$SCRIPT_UNDER_TEST" "Script file exists"
if [[ -x "$SCRIPT_UNDER_TEST" ]]; then test_pass "Script is executable"; else test_fail "Script is not executable"; fi
if bash -n "$SCRIPT_UNDER_TEST" 2>/dev/null; then test_pass "Script has valid syntax"; else test_fail "Script has syntax errors"; fi

test_section "Missing State File"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" 2>&1) || true
assert_contains "$output" "No tracked worktrees" "Shows message when no state file"

test_section "Table Format Output"
create_test_worktrees_yaml
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" 2>&1) || true
assert_contains "$output" "=== Tracked Worktrees ===" "Table shows header"
assert_contains "$output" "PATH" "Table shows PATH column"
assert_contains "$output" "BRANCH" "Table shows BRANCH column"
assert_contains "$output" "STATUS" "Table shows STATUS column"
assert_contains "$output" "LAST ACTIVE" "Table shows LAST ACTIVE column"
assert_contains "$output" "feat/auth-jwt" "Table shows auth branch"
assert_contains "$output" "fix/memory-leak" "Table shows fix branch"
assert_contains "$output" "active" "Table shows active status"
assert_contains "$output" "stale" "Table shows stale status"
assert_contains "$output" "=== Git Worktrees (actual) ===" "Table shows git worktree section"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format table 2>&1) || true
assert_contains "$output" "=== Tracked Worktrees ===" "Explicit --format table works"

test_section "YAML Format Output"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format yaml 2>&1) || true
assert_contains "$output" "worktrees:" "YAML shows worktrees key"
assert_contains "$output" "feat/auth-jwt" "YAML shows branch data"
assert_contains "$output" "metadata:" "YAML shows metadata section"
assert_not_contains "$output" "=== Tracked Worktrees ===" "YAML omits table header"
assert_not_contains "$output" "=== Git Worktrees ===" "YAML omits git section"

test_section "Status Filter"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status active 2>&1) || true
assert_contains "$output" "feat/auth-jwt" "Active filter shows active worktree"
assert_not_contains "$output" "fix/memory-leak" "Active filter excludes stale worktree"
assert_not_contains "$output" "feat/api-v2" "Active filter excludes merged worktree"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status stale 2>&1) || true
assert_contains "$output" "fix/memory-leak" "Stale filter shows stale worktree"
assert_not_contains "$output" "feat/auth-jwt" "Stale filter excludes active worktree"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status merged 2>&1) || true
assert_contains "$output" "feat/api-v2" "Merged filter shows merged worktree"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status abandoned 2>&1) || true
assert_contains "$output" "feat/old-thing" "Abandoned filter shows abandoned worktree"

test_section "Error Handling"
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --unknown 2>/dev/null; then test_fail "Should reject unknown option"; else test_pass "Rejects unknown option"; fi
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --unknown 2>&1) || true
assert_contains "$output" "Error" "Unknown option shows error message"
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format invalid 2>/dev/null; then test_fail "Should reject invalid format"; else test_pass "Rejects invalid format"; fi
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format invalid 2>&1) || true
assert_contains "$output" "Invalid format" "Invalid format shows descriptive error"
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status invalid 2>/dev/null; then test_fail "Should reject invalid status"; else test_pass "Rejects invalid status"; fi
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status invalid 2>&1) || true
assert_contains "$output" "Invalid status" "Invalid status shows descriptive error"
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format 2>/dev/null; then test_fail "Should reject --format without value"; else test_pass "Rejects --format without value"; fi
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --status 2>/dev/null; then test_fail "Should reject --status without value"; else test_pass "Rejects --status without value"; fi
if REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" somefile 2>/dev/null; then test_fail "Should reject unexpected positional arg"; else test_pass "Rejects unexpected positional argument"; fi

test_section "Combined Options"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format table --status active 2>&1) || true
assert_contains "$output" "feat/auth-jwt" "Combined format+status: shows matching entry"
assert_not_contains "$output" "fix/memory-leak" "Combined format+status: excludes non-matching"
assert_contains "$output" "=== Tracked Worktrees ===" "Combined: table format used"

test_section "REPO_ROOT Detection"
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" --format yaml 2>&1) || true
assert_contains "$output" "worktrees:" "Uses explicit REPO_ROOT"

test_section "Empty Worktrees File"
cat > "$TEST_REPO_ROOT/.state/worktrees.yaml" << 'YAML'
worktrees: []
metadata:
  version: "1.0.0"
  last_updated: null
YAML
output=$(REPO_ROOT="$TEST_REPO_ROOT" "$SCRIPT_UNDER_TEST" 2>&1) || true
assert_contains "$output" "=== Tracked Worktrees ===" "Empty file still shows table header"
assert_contains "$output" "PATH" "Empty file shows column headers"

test_section "Code Quality"
assert_file_contains "$SCRIPT_UNDER_TEST" "set -euo pipefail" "Uses strict mode"
assert_file_contains "$SCRIPT_UNDER_TEST" "REPO_ROOT=" "Detects REPO_ROOT"
assert_file_contains "$SCRIPT_UNDER_TEST" "show_help" "Has show_help function"
assert_file_contains "$SCRIPT_UNDER_TEST" "show_version" "Has show_version function"
assert_file_contains "$SCRIPT_UNDER_TEST" "main()" "Has main() function"
assert_file_contains "$SCRIPT_UNDER_TEST" "VERSION=" "Defines VERSION"

create_test_worktrees_yaml
print_test_summary
[[ "$TEST_FAIL_COUNT" -eq 0 ]]
