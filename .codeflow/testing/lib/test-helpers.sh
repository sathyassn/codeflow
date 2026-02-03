#!/usr/bin/env bash
# CodeFlow Test Framework: Test Helpers
# Location: .codeflow/testing/lib/test-helpers.sh

# Requires: test-common.sh
HELPERS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$HELPERS_DIR/test-common.sh"

# ============================================================================
# EXIT CODE ASSERTIONS
# ============================================================================

assert_exit_code() {
    local expected="$1"
    local description="${2:-Command exit code check}"
    local actual=$?

    ((TEST_TOTAL_COUNT++))
    if [[ $actual -eq $expected ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (expected $expected, got $actual)"
        return 1
    fi
}

assert_success() {
    local cmd="$1"
    local description="${2:-Command should succeed}"

    ((TEST_TOTAL_COUNT++))
    if eval "$cmd" >/dev/null 2>&1; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description"
        return 1
    fi
}

assert_fails() {
    local cmd="$1"
    local description="${2:-Command should fail}"

    ((TEST_TOTAL_COUNT++))
    if ! eval "$cmd" >/dev/null 2>&1; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description"
        return 1
    fi
}

# ============================================================================
# STRING ASSERTIONS
# ============================================================================

assert_equals() {
    local expected="$1"
    local actual="$2"
    local description="${3:-Values should be equal}"

    ((TEST_TOTAL_COUNT++))
    if [[ "$expected" == "$actual" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description"
        echo -e "    Expected: '$expected'"
        echo -e "    Actual:   '$actual'"
        return 1
    fi
}

assert_not_equals() {
    local unexpected="$1"
    local actual="$2"
    local description="${3:-Values should not be equal}"

    ((TEST_TOTAL_COUNT++))
    if [[ "$unexpected" != "$actual" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (got '$actual')"
        return 1
    fi
}

assert_contains() {
    local haystack="$1"
    local needle="$2"
    local description="${3:-String should contain}"

    ((TEST_TOTAL_COUNT++))
    if [[ "$haystack" == *"$needle"* ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description ('$needle' not found)"
        return 1
    fi
}

assert_not_contains() {
    local haystack="$1"
    local needle="$2"
    local description="${3:-String should not contain}"

    ((TEST_TOTAL_COUNT++))
    if [[ "$haystack" != *"$needle"* ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description ('$needle' was found)"
        return 1
    fi
}

assert_matches() {
    local string="$1"
    local pattern="$2"
    local description="${3:-String should match pattern}"

    ((TEST_TOTAL_COUNT++))
    if [[ "$string" =~ $pattern ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (pattern '$pattern' didn't match)"
        return 1
    fi
}

assert_empty() {
    local value="$1"
    local description="${2:-Value should be empty}"

    ((TEST_TOTAL_COUNT++))
    if [[ -z "$value" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (got '$value')"
        return 1
    fi
}

assert_not_empty() {
    local value="$1"
    local description="${2:-Value should not be empty}"

    ((TEST_TOTAL_COUNT++))
    if [[ -n "$value" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (value was empty)"
        return 1
    fi
}

# ============================================================================
# FILE ASSERTIONS
# ============================================================================

assert_file_exists() {
    local file="$1"
    local description="${2:-File should exist}"

    ((TEST_TOTAL_COUNT++))
    if [[ -f "$file" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description ($file)"
        return 1
    fi
}

assert_file_not_exists() {
    local file="$1"
    local description="${2:-File should not exist}"

    ((TEST_TOTAL_COUNT++))
    if [[ ! -f "$file" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description ($file exists)"
        return 1
    fi
}

assert_dir_exists() {
    local dir="$1"
    local description="${2:-Directory should exist}"

    ((TEST_TOTAL_COUNT++))
    if [[ -d "$dir" ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description ($dir)"
        return 1
    fi
}

assert_file_contains() {
    local file="$1"
    local pattern="$2"
    local description="${3:-File should contain pattern}"

    ((TEST_TOTAL_COUNT++))
    if [[ -f "$file" ]] && grep -q "$pattern" "$file" 2>/dev/null; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description"
        return 1
    fi
}

# ============================================================================
# HOOK TESTING
# ============================================================================

run_hook_test() {
    local hook_path="$1"
    local tool_name="$2"
    local tool_input="$3"

    local json_input
    json_input=$(cat <<EOF
{
    "tool_name": "$tool_name",
    "tool_input": $tool_input
}
EOF
)

    echo "$json_input" | "$hook_path" 2>&1
}

assert_hook_blocks() {
    local description="$1"
    local hook_path="$2"
    local tool_name="$3"
    local tool_input="$4"

    ((TEST_TOTAL_COUNT++))
    local output
    output=$(run_hook_test "$hook_path" "$tool_name" "$tool_input")
    local exit_code=$?

    if [[ $exit_code -eq 2 ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description (blocked)"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (expected block, got exit $exit_code)"
        return 1
    fi
}

assert_hook_allows() {
    local description="$1"
    local hook_path="$2"
    local tool_name="$3"
    local tool_input="$4"

    ((TEST_TOTAL_COUNT++))
    local output
    output=$(run_hook_test "$hook_path" "$tool_name" "$tool_input")
    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        ((TEST_PASS_COUNT++))
        echo -e "  ${GREEN}✓${NC} $description (allowed)"
        return 0
    else
        ((TEST_FAIL_COUNT++))
        echo -e "  ${RED}✗${NC} $description (expected allow, got exit $exit_code)"
        return 1
    fi
}

# ============================================================================
# TEMPORARY DIRECTORY MANAGEMENT
# ============================================================================

setup_test_dir() {
    local prefix="${1:-test}"
    TEST_DIR=$(mktemp -d "/tmp/codeflow-test-${prefix}-XXXXXX")
    export TEST_DIR
    echo "$TEST_DIR"
}

teardown_test_dir() {
    if [[ -n "${TEST_DIR:-}" && -d "$TEST_DIR" ]]; then
        rm -rf "$TEST_DIR"
        unset TEST_DIR
    fi
}

create_test_file() {
    local filename="$1"
    local content="${2:-}"

    local filepath="$TEST_DIR/$filename"
    mkdir -p "$(dirname "$filepath")"
    echo "$content" > "$filepath"
    echo "$filepath"
}

# ============================================================================
# OUTPUT FUNCTIONS
# ============================================================================

test_pass() {
    local description="$1"
    ((TEST_PASS_COUNT++))
    ((TEST_TOTAL_COUNT++))
    echo -e "  ${GREEN}✓${NC} $description"
}

test_fail() {
    local description="$1"
    ((TEST_FAIL_COUNT++))
    ((TEST_TOTAL_COUNT++))
    echo -e "  ${RED}✗${NC} $description"
}

test_skip() {
    local description="$1"
    local reason="${2:-}"
    ((TEST_SKIP_COUNT++))
    ((TEST_TOTAL_COUNT++))
    echo -e "  ${YELLOW}○${NC} $description${reason:+ (}${reason}${reason:+)}"
}

test_section() {
    local title="$1"
    echo ""
    echo -e "${BOLD}=== $title ===${NC}"
}

test_subsection() {
    local title="$1"
    echo -e "  ${CYAN}--- $title ---${NC}"
}

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BOLD}Test Summary${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "  ${GREEN}Passed:${NC}  $TEST_PASS_COUNT"
    echo -e "  ${RED}Failed:${NC}  $TEST_FAIL_COUNT"
    echo -e "  ${YELLOW}Skipped:${NC} $TEST_SKIP_COUNT"
    echo -e "  Total:   $TEST_TOTAL_COUNT"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    if [[ $TEST_FAIL_COUNT -eq 0 ]]; then
        echo -e "${GREEN}All tests passed!${NC}"
    else
        echo -e "${RED}Some tests failed${NC}"
    fi
}

reset_test_counters() {
    TEST_PASS_COUNT=0
    TEST_FAIL_COUNT=0
    TEST_SKIP_COUNT=0
    TEST_TOTAL_COUNT=0
}
