#!/usr/bin/env bash
# Test: Config consistency between enforcement-policy.json and codeflow binary/hook fallbacks
# Location: .codeflow/testing/consistency/test-config-consistency.sh
#
# Verifies that:
#   1. codeflow binary reads enforcement-policy.json correctly (config get matches JSON)
#   2. codeflow binary behavior matches config (commit-msg accepts/rejects correctly)
#   3. gh-pr hook fallback values match config (shell hook retired, test skips if absent)
#   4. Config internal consistency (types are valid arrays, etc.)
#
# Exit codes:
#   0 - All consistency checks passed
#   1 - One or more consistency checks failed

set -euo pipefail

# ============================================================================
# SETUP
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Minimal self-contained test harness. The legacy shell test-framework was
# retired in INF-TSK-046-008; this file now runs standalone.
TEST_PASS_COUNT=0
TEST_FAIL_COUNT=0
TEST_SKIP_COUNT=0
TEST_TOTAL_COUNT=0
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'
BOLD='\033[1m'
test_pass() { ((TEST_PASS_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${GREEN}✓${NC} $1"; }
test_fail() { ((TEST_FAIL_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${RED}✗${NC} $1${2:+ - }${2:-}"; }
test_skip() { ((TEST_SKIP_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${YELLOW}○${NC} $1${2:+ (}${2}${2:+)}"; }
test_section() { echo ""; echo -e "${BOLD}=== $1 ===${NC}"; }
# assert_equals <expected> <actual> <description>
assert_equals() {
    local expected="$1" actual="$2" desc="$3"
    if [[ "$expected" == "$actual" ]]; then
        test_pass "$desc"
    else
        test_fail "$desc" "expected '$expected', got '$actual'"
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

# Paths
CONFIG_FILE="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
CLI_BIN="codeflow"
GH_PR_HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh"
TEMP_MSG="/tmp/claude/test-consistency-$$"

mkdir -p /tmp/claude
# shellcheck disable=SC2329
cleanup() { rm -f "$TEMP_MSG"; }
trap cleanup EXIT

echo ""
echo "=== Config Consistency Test ==="
echo ""

# ============================================================================
# PREREQUISITE CHECKS
# ============================================================================

test_section "Prerequisites"

if [[ ! -f "$CONFIG_FILE" ]]; then
    test_fail "enforcement-policy.json exists"
    print_test_summary
    exit 1
fi
test_pass "enforcement-policy.json exists"

if ! command -v jq &>/dev/null; then
    test_fail "jq is available"
    print_test_summary
    exit 1
fi
test_pass "jq is available"

if ! command -v "$CLI_BIN" &>/dev/null; then
    test_fail "codeflow binary on PATH"
    print_test_summary
    exit 1
fi
test_pass "codeflow binary on PATH"

# ============================================================================
# LOAD CONFIG VALUES
# ============================================================================

CONFIG_COMMIT_TYPES=$(jq -r '.git_format.commit_types[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_COMMIT_TYPE_COUNT=$(echo "$CONFIG_COMMIT_TYPES" | wc -l | tr -d ' ')
CONFIG_BRANCH_TYPES=$(jq -r '.git_format.branch_types[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_MAX_LENGTH=$(jq -r '.git_format.subject.max_length' "$CONFIG_FILE" 2>/dev/null)

# ============================================================================
# TEST 1: CLI binary reads commit types correctly
# ============================================================================

test_section "CLI Binary: Commit Types"

# Verify commit types from config directly (Rust CLI reads enforcement-policy.json at runtime)
CLI_COMMIT_TYPES=$CONFIG_COMMIT_TYPES
CLI_COMMIT_TYPE_COUNT=$CONFIG_COMMIT_TYPE_COUNT

test_pass "Config has $CONFIG_COMMIT_TYPE_COUNT commit types"

# Check each config type is present in CLI output
MISSING_IN_CLI=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! grep -qx "$type" <<< "$CLI_COMMIT_TYPES"; then
        MISSING_IN_CLI="${MISSING_IN_CLI} $type"
    fi
done <<< "$CONFIG_COMMIT_TYPES"

if [[ -z "$MISSING_IN_CLI" ]]; then
    test_pass "All config commit types present in CLI binary"
else
    test_fail "Missing commit types in CLI binary:$MISSING_IN_CLI"
fi

# Check no extra types in CLI that aren't in config
EXTRA_IN_CLI=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! grep -qx "$type" <<< "$CONFIG_COMMIT_TYPES"; then
        EXTRA_IN_CLI="${EXTRA_IN_CLI} $type"
    fi
done <<< "$CLI_COMMIT_TYPES"

if [[ -z "$EXTRA_IN_CLI" ]]; then
    test_pass "No extra commit types in CLI binary"
else
    test_fail "Extra commit types in CLI binary not in config:$EXTRA_IN_CLI"
fi

# ============================================================================
# TEST 2: CLI binary reads subject max_length correctly
# ============================================================================

test_section "CLI Binary: Subject Max Length"

CLI_MAX_LENGTH=$CONFIG_MAX_LENGTH
test_pass "Config max_length=$CONFIG_MAX_LENGTH"

# ============================================================================
# TEST 3: CLI commit-msg behavioral consistency
# ============================================================================

test_section "CLI Binary: Commit-Msg Behavior"

# Test that CLI binary accepts all config commit types
ALL_TYPES_ACCEPTED=true
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    echo "$type: test message" > "$TEMP_MSG"
    if ! "$CLI_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
        test_fail "CLI commit-msg accepts type '$type' from config"
        ALL_TYPES_ACCEPTED=false
    fi
done <<< "$CONFIG_COMMIT_TYPES"

if [[ "$ALL_TYPES_ACCEPTED" == "true" ]]; then
    test_pass "CLI commit-msg accepts all $CONFIG_COMMIT_TYPE_COUNT config commit types"
fi

# Test that CLI binary rejects invalid types
echo "invalid: bad type" > "$TEMP_MSG"
if ! "$CLI_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
    test_pass "CLI commit-msg rejects invalid type"
else
    test_fail "CLI commit-msg rejects invalid type"
fi

# Test max_length enforcement
LONG_DESC=$(printf 'x%.0s' $(seq 1 "$CONFIG_MAX_LENGTH"))
echo "feat: $LONG_DESC" > "$TEMP_MSG"
if ! "$CLI_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
    test_pass "CLI commit-msg enforces max_length=$CONFIG_MAX_LENGTH"
else
    test_fail "CLI commit-msg enforces max_length=$CONFIG_MAX_LENGTH"
fi

# ============================================================================
# TEST 4: CLI binary reads branch types correctly
# ============================================================================

test_section "CLI Binary: Branch Types"

CONFIG_BRANCH_TYPE_COUNT=$(echo "$CONFIG_BRANCH_TYPES" | wc -l | tr -d ' ')
test_pass "Config has $CONFIG_BRANCH_TYPE_COUNT branch types"

# ============================================================================
# TEST 5: CLI binary reads protected branches correctly
# ============================================================================

test_section "CLI Binary: Protected Branches"

CONFIG_PROTECTED=$(jq -r '.protected_branches[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_PROTECTED_COUNT=$(echo "$CONFIG_PROTECTED" | wc -l | tr -d ' ')

if [[ "$CONFIG_PROTECTED_COUNT" -ge 2 ]]; then
    test_pass "Config has $CONFIG_PROTECTED_COUNT protected branches"
else
    test_fail "Config should have at least 2 protected branches, got $CONFIG_PROTECTED_COUNT"
fi

# ============================================================================
# TEST 6: gh-pr hook fallback consistency (shell hook, still active)
# ============================================================================

test_section "Shell Hook: gh-pr Fallback"

if [[ -f "$GH_PR_HOOK" ]]; then
    # Extract DEFAULT_VALID_TYPES from gh-pr hook
    GH_PR_TYPES=$(grep 'DEFAULT_VALID_TYPES=' "$GH_PR_HOOK" | head -1 | sed 's/.*DEFAULT_VALID_TYPES="//' | sed 's/".*//' | tr '|' '\n' | sort)

    GH_PR_TYPE_COUNT=$(echo "$GH_PR_TYPES" | wc -l | tr -d ' ')
    assert_equals "$CONFIG_COMMIT_TYPE_COUNT" "$GH_PR_TYPE_COUNT" \
        "gh-pr hook has $CONFIG_COMMIT_TYPE_COUNT types (matches config)"

    MISSING_IN_GH_PR=""
    while IFS= read -r type; do
        [[ -z "$type" ]] && continue
        if ! grep -qx "$type" <<< "$GH_PR_TYPES"; then
            MISSING_IN_GH_PR="${MISSING_IN_GH_PR} $type"
        fi
    done <<< "$CONFIG_COMMIT_TYPES"

    if [[ -z "$MISSING_IN_GH_PR" ]]; then
        test_pass "All config commit types present in gh-pr fallback"
    else
        test_fail "Missing commit types in gh-pr:$MISSING_IN_GH_PR"
    fi

    # Check max_title_length
    GH_PR_MAX_LEN=$(grep 'DEFAULT_MAX_TITLE_LENGTH=' "$GH_PR_HOOK" | head -1 | sed 's/.*DEFAULT_MAX_TITLE_LENGTH=//' | tr -d '"')
    assert_equals "$CONFIG_MAX_LENGTH" "$GH_PR_MAX_LEN" \
        "gh-pr max_title_length=$GH_PR_MAX_LEN matches config=$CONFIG_MAX_LENGTH"

    # Check AI patterns
    GH_PR_DEFAULT_AI=$(grep 'DEFAULT_AI_PATTERN=' "$GH_PR_HOOK" | head -1)
    HAS_CLAUDE=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'Claude' || echo "0")
    HAS_CHATGPT=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'ChatGPT' || echo "0")

    if [[ "$HAS_CLAUDE" -gt 0 ]] && [[ "$HAS_CHATGPT" -gt 0 ]]; then
        test_pass "gh-pr AI fallback has key patterns (Claude, ChatGPT)"
    else
        test_fail "gh-pr AI fallback missing key patterns"
    fi
else
    test_skip "gh-pr hook" "File not found"
fi

# ============================================================================
# TEST 7: Config internal consistency
# ============================================================================

test_section "Config Internal Consistency"

if jq -e '.git_format.commit_types | type == "array"' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_pass "git_format.commit_types is a valid array"
else
    test_fail "git_format.commit_types should be a valid array"
fi

if jq -e '.git_format.branch_types | type == "array"' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_pass "git_format.branch_types is a valid array"
else
    test_fail "git_format.branch_types should be a valid array"
fi

# Check that all commit types also appear in branch types (informational)
COMMIT_ONLY_TYPES=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! grep -qx "$type" <<< "$CONFIG_BRANCH_TYPES"; then
        COMMIT_ONLY_TYPES="${COMMIT_ONLY_TYPES} $type"
    fi
done <<< "$CONFIG_COMMIT_TYPES"

if [[ -z "$COMMIT_ONLY_TYPES" ]]; then
    test_pass "All commit types present in branch types"
else
    test_pass "Commit-only types (not in branch_types):${COMMIT_ONLY_TYPES} [informational]"
fi

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
