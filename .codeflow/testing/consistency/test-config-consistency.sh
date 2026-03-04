#!/usr/bin/env bash
# Test: Config consistency between enforcement-policy.json and Go binary/hook fallbacks
# Location: .codeflow/testing/consistency/test-config-consistency.sh
#
# Verifies that:
#   1. Go binary reads enforcement-policy.json correctly (config get matches JSON)
#   2. Go binary behavior matches config (commit-msg accepts/rejects correctly)
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

# Source test helpers
if [[ -f "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh" ]]; then
    source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"
else
    echo "ERROR: test-helpers.sh not found" >&2
    exit 1
fi

# Paths
CONFIG_FILE="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
GO_BIN="codeflow"
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

if ! command -v "$GO_BIN" &>/dev/null; then
    test_fail "Go binary on PATH"
    print_test_summary
    exit 1
fi
test_pass "Go binary on PATH"

# ============================================================================
# LOAD CONFIG VALUES
# ============================================================================

CONFIG_COMMIT_TYPES=$(jq -r '.git_format.commit_types[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_COMMIT_TYPE_COUNT=$(echo "$CONFIG_COMMIT_TYPES" | wc -l | tr -d ' ')
CONFIG_BRANCH_TYPES=$(jq -r '.git_format.branch_types[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_MAX_LENGTH=$(jq -r '.git_format.subject.max_length' "$CONFIG_FILE" 2>/dev/null)

# ============================================================================
# TEST 1: Go binary reads commit types correctly
# ============================================================================

test_section "Go Binary: Commit Types"

# Get commit types from Go binary config get
GO_COMMIT_TYPES_RAW=$("$GO_BIN" config get enforcement.enforcement-policy.git_format.commit_types 2>/dev/null)
# Parse bracketed list: [feat fix bugfix ...] -> one per line, sorted
GO_COMMIT_TYPES=$(echo "$GO_COMMIT_TYPES_RAW" | tr -d '[]' | tr ' ' '\n' | grep -v '^$' | sort)
GO_COMMIT_TYPE_COUNT=$(echo "$GO_COMMIT_TYPES" | wc -l | tr -d ' ')

assert_equals "$CONFIG_COMMIT_TYPE_COUNT" "$GO_COMMIT_TYPE_COUNT" \
    "Go binary has $CONFIG_COMMIT_TYPE_COUNT commit types (matches config)"

# Check each config type is present in Go output
MISSING_IN_GO=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! echo "$GO_COMMIT_TYPES" | grep -qx "$type"; then
        MISSING_IN_GO="${MISSING_IN_GO} $type"
    fi
done <<< "$CONFIG_COMMIT_TYPES"

if [[ -z "$MISSING_IN_GO" ]]; then
    test_pass "All config commit types present in Go binary"
else
    test_fail "Missing commit types in Go binary:$MISSING_IN_GO"
fi

# Check no extra types in Go that aren't in config
EXTRA_IN_GO=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! echo "$CONFIG_COMMIT_TYPES" | grep -qx "$type"; then
        EXTRA_IN_GO="${EXTRA_IN_GO} $type"
    fi
done <<< "$GO_COMMIT_TYPES"

if [[ -z "$EXTRA_IN_GO" ]]; then
    test_pass "No extra commit types in Go binary"
else
    test_fail "Extra commit types in Go binary not in config:$EXTRA_IN_GO"
fi

# ============================================================================
# TEST 2: Go binary reads subject max_length correctly
# ============================================================================

test_section "Go Binary: Subject Max Length"

GO_MAX_LENGTH=$("$GO_BIN" config get enforcement.enforcement-policy.git_format.subject.max_length 2>/dev/null)
assert_equals "$CONFIG_MAX_LENGTH" "$GO_MAX_LENGTH" \
    "Go max_length=$GO_MAX_LENGTH matches config=$CONFIG_MAX_LENGTH"

# ============================================================================
# TEST 3: Go commit-msg behavioral consistency
# ============================================================================

test_section "Go Binary: Commit-Msg Behavior"

# Test that Go binary accepts all config commit types
ALL_TYPES_ACCEPTED=true
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    echo "$type: test message" > "$TEMP_MSG"
    if ! "$GO_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
        test_fail "Go commit-msg accepts type '$type' from config"
        ALL_TYPES_ACCEPTED=false
    fi
done <<< "$CONFIG_COMMIT_TYPES"

if [[ "$ALL_TYPES_ACCEPTED" == "true" ]]; then
    test_pass "Go commit-msg accepts all $CONFIG_COMMIT_TYPE_COUNT config commit types"
fi

# Test that Go binary rejects invalid types
echo "invalid: bad type" > "$TEMP_MSG"
if ! "$GO_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
    test_pass "Go commit-msg rejects invalid type"
else
    test_fail "Go commit-msg rejects invalid type"
fi

# Test max_length enforcement
LONG_DESC=$(printf 'x%.0s' $(seq 1 "$CONFIG_MAX_LENGTH"))
echo "feat: $LONG_DESC" > "$TEMP_MSG"
if ! "$GO_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null; then
    test_pass "Go commit-msg enforces max_length=$CONFIG_MAX_LENGTH"
else
    test_fail "Go commit-msg enforces max_length=$CONFIG_MAX_LENGTH"
fi

# ============================================================================
# TEST 4: Go binary reads branch types correctly
# ============================================================================

test_section "Go Binary: Branch Types"

GO_BRANCH_TYPES_RAW=$("$GO_BIN" config get enforcement.enforcement-policy.git_format.branch_types 2>/dev/null)
GO_BRANCH_TYPES=$(echo "$GO_BRANCH_TYPES_RAW" | tr -d '[]' | tr ' ' '\n' | grep -v '^$' | sort)
CONFIG_BRANCH_TYPE_COUNT=$(echo "$CONFIG_BRANCH_TYPES" | wc -l | tr -d ' ')
GO_BRANCH_TYPE_COUNT=$(echo "$GO_BRANCH_TYPES" | wc -l | tr -d ' ')

assert_equals "$CONFIG_BRANCH_TYPE_COUNT" "$GO_BRANCH_TYPE_COUNT" \
    "Go binary has $CONFIG_BRANCH_TYPE_COUNT branch types (matches config)"

# ============================================================================
# TEST 5: Go binary reads protected branches correctly
# ============================================================================

test_section "Go Binary: Protected Branches"

CONFIG_PROTECTED=$(jq -r '.protected_branches[]' "$CONFIG_FILE" 2>/dev/null | sort)
GO_PROTECTED_RAW=$("$GO_BIN" config get enforcement.enforcement-policy.protected_branches 2>/dev/null)
GO_PROTECTED=$(echo "$GO_PROTECTED_RAW" | tr -d '[]' | tr ' ' '\n' | grep -v '^$' | sort)

MISSING_PROTECTED=""
while IFS= read -r branch; do
    [[ -z "$branch" ]] && continue
    # Handle glob patterns (release/*) by checking exact match
    if ! echo "$GO_PROTECTED" | grep -qxF "$branch"; then
        MISSING_PROTECTED="${MISSING_PROTECTED} $branch"
    fi
done <<< "$CONFIG_PROTECTED"

if [[ -z "$MISSING_PROTECTED" ]]; then
    test_pass "Go binary includes all protected branches from config"
else
    test_fail "Go binary missing protected branches:$MISSING_PROTECTED"
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
        if ! echo "$GH_PR_TYPES" | grep -qx "$type"; then
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
    if ! echo "$CONFIG_BRANCH_TYPES" | grep -qx "$type"; then
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
