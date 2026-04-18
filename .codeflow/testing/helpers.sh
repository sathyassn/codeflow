#!/usr/bin/env bash
# Minimal self-contained shell test helpers.
#
# Location: .codeflow/testing/helpers.sh
# Introduced: INF-TSK-046-008 (replaces the retired .codeflow/testing/lib/
# framework; all 10 lib/*.sh files were deleted in commit 33ee1dc4).
#
# Usage (from a test file):
#   TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   REPO_ROOT="$(git -C "$TEST_DIR" rev-parse --show-toplevel 2>/dev/null)"
#   # shellcheck source=../../helpers.sh
#   source "$REPO_ROOT/.codeflow/testing/helpers.sh"
#
# This file is intentionally tiny. It provides only the minimum needed by
# the surviving shell test bodies — not a full test framework. Adopters
# wanting fuller semantics should use `codeflow test shell-exec` or a
# language-native runner instead.

# Counters ----------------------------------------------------------------
TEST_PASS_COUNT=${TEST_PASS_COUNT:-0}
TEST_FAIL_COUNT=${TEST_FAIL_COUNT:-0}
TEST_SKIP_COUNT=${TEST_SKIP_COUNT:-0}
TEST_TOTAL_COUNT=${TEST_TOTAL_COUNT:-0}

# Colors ------------------------------------------------------------------
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'
BOLD='\033[1m'

# Repo root (derived lazily; tests typically set their own REPO_ROOT).
if [[ -z "${REPO_ROOT:-}" ]]; then
    if command -v git >/dev/null 2>&1; then
        REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "")"
    fi
fi

# Reporting ---------------------------------------------------------------
test_pass() {
    ((TEST_PASS_COUNT++)) || true
    ((TEST_TOTAL_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} $1"
}

test_fail() {
    ((TEST_FAIL_COUNT++)) || true
    ((TEST_TOTAL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} $1${2:+ - }${2:-}"
}

test_skip() {
    ((TEST_SKIP_COUNT++)) || true
    ((TEST_TOTAL_COUNT++)) || true
    echo -e "  ${YELLOW}○${NC} $1${2:+ (}${2}${2:+)}"
}

test_section() {
    echo ""
    echo -e "${BOLD}=== $1 ===${NC}"
}

print_test_summary() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BOLD}Test Summary${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "  ${GREEN}Passed:${NC}  $TEST_PASS_COUNT"
    echo -e "  ${RED}Failed:${NC}  $TEST_FAIL_COUNT"
    echo -e "  ${YELLOW}Skipped:${NC} $TEST_SKIP_COUNT"
    echo -e "  Total:   $TEST_TOTAL_COUNT"
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

# Assertions --------------------------------------------------------------

# assert_equals <expected> <actual> <description>
assert_equals() {
    local expected="$1" actual="$2" desc="$3"
    if [[ "$expected" == "$actual" ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "expected '$expected', got '$actual'"
    fi
}

# assert_not_equals <expected> <actual> <description>
assert_not_equals() {
    local expected="$1" actual="$2" desc="$3"
    if [[ "$expected" != "$actual" ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "both values equal '$expected'"
    fi
}

# assert_file_exists <path> <description>
assert_file_exists() {
    local path="$1" desc="$2"
    if [[ -f "$path" ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "file not found: $path"
    fi
}

# assert_file_contains <path> <pattern> <description>
assert_file_contains() {
    local path="$1" pattern="$2" desc="$3"
    if [[ ! -f "$path" ]]; then
        test_fail "$desc" "file not found: $path"
        return
    fi
    if grep -q -- "$pattern" "$path" 2>/dev/null; then
        test_pass "$desc"
    else
        test_fail "$desc" "pattern '$pattern' not found in $path"
    fi
}

# assert_contains <haystack> <needle> <description>
assert_contains() {
    local haystack="$1" needle="$2" desc="$3"
    if [[ "$haystack" == *"$needle"* ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "'$needle' not found in input"
    fi
}

# assert_not_contains <haystack> <needle> <description>
assert_not_contains() {
    local haystack="$1" needle="$2" desc="$3"
    if [[ "$haystack" != *"$needle"* ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "'$needle' unexpectedly present in input"
    fi
}

# assert_success <command-string> <description>
assert_success() {
    local cmd="$1" desc="$2"
    if eval "$cmd" >/dev/null 2>&1; then
        test_pass "$desc"
    else
        test_fail "$desc" "command failed: $cmd"
    fi
}

# assert_failure <command-string> <description>
assert_failure() {
    local cmd="$1" desc="$2"
    if ! eval "$cmd" >/dev/null 2>&1; then
        test_pass "$desc"
    else
        test_fail "$desc" "command unexpectedly succeeded: $cmd"
    fi
}

# Isolated env ------------------------------------------------------------

# setup_isolated_env <prefix?>
# Creates a per-test tempdir. Returns path via stdout; caller captures it.
setup_isolated_env() {
    local prefix="${1:-codeflow-test}"
    mktemp -d -t "${prefix}.XXXXXX"
}

# cleanup_isolated_env <path>
cleanup_isolated_env() {
    local path="$1"
    if [[ -n "$path" && -d "$path" && "$path" == /tmp/* || "$path" == /var/folders/* ]]; then
        rm -rf "$path"
    fi
}
