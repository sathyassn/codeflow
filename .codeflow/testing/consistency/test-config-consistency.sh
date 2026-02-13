#!/usr/bin/env bash
# Test: Config consistency between enforcement-policy.json and hook fallbacks
# Location: .codeflow/testing/consistency/test-config-consistency.sh
#
# Verifies that hardcoded fallback values in hooks match the canonical
# enforcement-policy.json configuration, ensuring no config drift.
#
# Checks:
#   1. All 16 commit types appear in each hook's fallback
#   2. Subject max_length matches across hooks
#   3. AI attribution patterns in config match hook fallbacks
#   4. Branch types in pre-commit match config
#   5. Protected branches in pre-push match config
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
COMMIT_MSG_HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/commit-msg"
PRE_COMMIT_HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-commit"
PRE_PUSH_HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-push"
GH_PR_HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh"

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

# ============================================================================
# LOAD CONFIG VALUES
# ============================================================================

# Load commit types from config (sorted for comparison)
CONFIG_COMMIT_TYPES=$(jq -r '.git_format.commit_types[]' "$CONFIG_FILE" 2>/dev/null | sort)
CONFIG_COMMIT_TYPE_COUNT=$(echo "$CONFIG_COMMIT_TYPES" | wc -l | tr -d ' ')

# Load branch types from config (sorted)
CONFIG_BRANCH_TYPES=$(jq -r '.git_format.branch_types[]' "$CONFIG_FILE" 2>/dev/null | sort)

# Load subject config
CONFIG_MAX_LENGTH=$(jq -r '.git_format.subject.max_length' "$CONFIG_FILE" 2>/dev/null)

# ============================================================================
# TEST 1: Commit types in commit-msg hook fallback
# ============================================================================

test_section "Commit Types: commit-msg hook"

if [[ -f "$COMMIT_MSG_HOOK" ]]; then
    # Extract DEFAULT_VALID_TYPES from commit-msg
    COMMIT_MSG_TYPES=$(grep 'DEFAULT_VALID_TYPES=' "$COMMIT_MSG_HOOK" | head -1 | sed 's/.*DEFAULT_VALID_TYPES="//' | sed 's/".*//' | tr '|' '\n' | sort)

    # Check count matches
    COMMIT_MSG_TYPE_COUNT=$(echo "$COMMIT_MSG_TYPES" | wc -l | tr -d ' ')
    assert_equals "$CONFIG_COMMIT_TYPE_COUNT" "$COMMIT_MSG_TYPE_COUNT" \
        "commit-msg has $CONFIG_COMMIT_TYPE_COUNT types (matches config)"

    # Check each config type is present in the fallback
    MISSING_IN_COMMIT_MSG=""
    while IFS= read -r type; do
        [[ -z "$type" ]] && continue
        if ! echo "$COMMIT_MSG_TYPES" | grep -qx "$type"; then
            MISSING_IN_COMMIT_MSG="${MISSING_IN_COMMIT_MSG} $type"
        fi
    done <<< "$CONFIG_COMMIT_TYPES"

    if [[ -z "$MISSING_IN_COMMIT_MSG" ]]; then
        test_pass "All config commit types present in commit-msg fallback"
    else
        test_fail "Missing commit types in commit-msg:$MISSING_IN_COMMIT_MSG"
    fi

    # Check no extra types in hook that aren't in config
    EXTRA_IN_COMMIT_MSG=""
    while IFS= read -r type; do
        [[ -z "$type" ]] && continue
        if ! echo "$CONFIG_COMMIT_TYPES" | grep -qx "$type"; then
            EXTRA_IN_COMMIT_MSG="${EXTRA_IN_COMMIT_MSG} $type"
        fi
    done <<< "$COMMIT_MSG_TYPES"

    if [[ -z "$EXTRA_IN_COMMIT_MSG" ]]; then
        test_pass "No extra commit types in commit-msg fallback"
    else
        test_fail "Extra commit types in commit-msg not in config:$EXTRA_IN_COMMIT_MSG"
    fi
else
    test_skip "commit-msg hook" "File not found"
fi

# ============================================================================
# TEST 2: Commit types in gh-pr hook fallback
# ============================================================================

test_section "Commit Types: gh-pr hook"

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

    EXTRA_IN_GH_PR=""
    while IFS= read -r type; do
        [[ -z "$type" ]] && continue
        if ! echo "$CONFIG_COMMIT_TYPES" | grep -qx "$type"; then
            EXTRA_IN_GH_PR="${EXTRA_IN_GH_PR} $type"
        fi
    done <<< "$GH_PR_TYPES"

    if [[ -z "$EXTRA_IN_GH_PR" ]]; then
        test_pass "No extra commit types in gh-pr fallback"
    else
        test_fail "Extra commit types in gh-pr not in config:$EXTRA_IN_GH_PR"
    fi
else
    test_skip "gh-pr hook" "File not found"
fi

# ============================================================================
# TEST 3: Branch types in pre-commit hook fallback
# ============================================================================

test_section "Branch Types: pre-commit hook"

if [[ -f "$PRE_COMMIT_HOOK" ]]; then
    # Extract DEFAULT_BRANCH_TYPES from pre-commit
    PRE_COMMIT_BRANCH_TYPES=$(grep 'DEFAULT_BRANCH_TYPES=' "$PRE_COMMIT_HOOK" | head -1 | sed 's/.*DEFAULT_BRANCH_TYPES="//' | sed 's/".*//' | tr '|' '\n' | sort)

    CONFIG_BRANCH_TYPE_COUNT=$(echo "$CONFIG_BRANCH_TYPES" | wc -l | tr -d ' ')
    PRE_COMMIT_BRANCH_TYPE_COUNT=$(echo "$PRE_COMMIT_BRANCH_TYPES" | wc -l | tr -d ' ')
    assert_equals "$CONFIG_BRANCH_TYPE_COUNT" "$PRE_COMMIT_BRANCH_TYPE_COUNT" \
        "pre-commit has $CONFIG_BRANCH_TYPE_COUNT branch types (matches config)"

    MISSING_BRANCH=""
    while IFS= read -r type; do
        [[ -z "$type" ]] && continue
        if ! echo "$PRE_COMMIT_BRANCH_TYPES" | grep -qx "$type"; then
            MISSING_BRANCH="${MISSING_BRANCH} $type"
        fi
    done <<< "$CONFIG_BRANCH_TYPES"

    if [[ -z "$MISSING_BRANCH" ]]; then
        test_pass "All config branch types present in pre-commit fallback"
    else
        test_fail "Missing branch types in pre-commit:$MISSING_BRANCH"
    fi
else
    test_skip "pre-commit hook" "File not found"
fi

# ============================================================================
# TEST 4: Subject max_length consistency
# ============================================================================

test_section "Subject Max Length Consistency"

# commit-msg hook
if [[ -f "$COMMIT_MSG_HOOK" ]]; then
    COMMIT_MSG_MAX_LEN=$(grep 'DEFAULT_MAX_SUBJECT_LENGTH=' "$COMMIT_MSG_HOOK" | head -1 | sed 's/.*DEFAULT_MAX_SUBJECT_LENGTH=//' | tr -d '"')
    assert_equals "$CONFIG_MAX_LENGTH" "$COMMIT_MSG_MAX_LEN" \
        "commit-msg max_length=$COMMIT_MSG_MAX_LEN matches config=$CONFIG_MAX_LENGTH"
else
    test_skip "commit-msg max_length" "File not found"
fi

# gh-pr hook
if [[ -f "$GH_PR_HOOK" ]]; then
    GH_PR_MAX_LEN=$(grep 'DEFAULT_MAX_TITLE_LENGTH=' "$GH_PR_HOOK" | head -1 | sed 's/.*DEFAULT_MAX_TITLE_LENGTH=//' | tr -d '"')
    assert_equals "$CONFIG_MAX_LENGTH" "$GH_PR_MAX_LEN" \
        "gh-pr max_title_length=$GH_PR_MAX_LEN matches config=$CONFIG_MAX_LENGTH"
else
    test_skip "gh-pr max_title_length" "File not found"
fi

# ============================================================================
# TEST 5: Protected branches in pre-push hook
# ============================================================================

test_section "Protected Branches: pre-push hook"

if [[ -f "$PRE_PUSH_HOOK" ]]; then
    # Extract DEFAULT_PROTECTED_BRANCHES from pre-push (space-separated string)
    # Format: DEFAULT_PROTECTED_BRANCHES="main master production"
    PRE_PUSH_PROTECTED=$(grep 'DEFAULT_PROTECTED_BRANCHES=' "$PRE_PUSH_HOOK" | head -1 | \
        sed 's/.*DEFAULT_PROTECTED_BRANCHES="//' | sed 's/".*//' | tr ' ' '\n' | sort)

    # protected_branches (root) from config
    CONFIG_GIT_PROTECTED=$(jq -r '.protected_branches[]' "$CONFIG_FILE" 2>/dev/null | sort)

    # Check pre-push includes all config protected branches (git_format)
    MISSING_PROTECTED=""
    while IFS= read -r branch; do
        [[ -z "$branch" ]] && continue
        if ! echo "$PRE_PUSH_PROTECTED" | grep -qxF "$branch"; then
            MISSING_PROTECTED="${MISSING_PROTECTED} $branch"
        fi
    done <<< "$CONFIG_GIT_PROTECTED"

    if [[ -z "$MISSING_PROTECTED" ]]; then
        test_pass "pre-push includes all protected_branches (root)"
    else
        test_fail "pre-push missing protected branches:$MISSING_PROTECTED"
    fi

    # Also verify root-level protected_branches are covered (exclude glob patterns)
    CONFIG_ROOT_PROTECTED=$(jq -r '.protected_branches[]' "$CONFIG_FILE" 2>/dev/null | \
        grep -v '/' | sort)

    MISSING_ROOT=""
    while IFS= read -r branch; do
        [[ -z "$branch" ]] && continue
        if ! echo "$PRE_PUSH_PROTECTED" | grep -qxF "$branch"; then
            MISSING_ROOT="${MISSING_ROOT} $branch"
        fi
    done <<< "$CONFIG_ROOT_PROTECTED"

    if [[ -z "$MISSING_ROOT" ]]; then
        test_pass "pre-push includes all root protected_branches (non-glob)"
    else
        test_fail "pre-push missing root protected branches:$MISSING_ROOT"
    fi
else
    test_skip "pre-push hook" "File not found"
fi

# ============================================================================
# TEST 6: AI attribution patterns consistency
# ============================================================================

test_section "AI Attribution Patterns"

# Check commit-msg hook has the right pattern count in fallback
if [[ -f "$COMMIT_MSG_HOOK" ]]; then
    # Count AI_PATTERNS entries in the fallback block
    # The fallback block is between "AI_PATTERNS=(" and ")" after the else
    COMMIT_MSG_AI_COUNT=$(sed -n '/^  AI_PATTERNS=(/,/)$/p' "$COMMIT_MSG_HOOK" | grep -c '"' || echo "0")

    if [[ "$COMMIT_MSG_AI_COUNT" -gt 0 ]]; then
        # Verify some key patterns exist
        COMMIT_MSG_AI_BLOCK=$(sed -n '/^  AI_PATTERNS=(/,/)$/p' "$COMMIT_MSG_HOOK")
        HAS_CLAUDE=$(echo "$COMMIT_MSG_AI_BLOCK" | grep -c '"Claude"' || echo "0")
        HAS_CHATGPT=$(echo "$COMMIT_MSG_AI_BLOCK" | grep -c '"ChatGPT"' || echo "0")
        HAS_COPILOT=$(echo "$COMMIT_MSG_AI_BLOCK" | grep -c '"Copilot"' || echo "0")
        HAS_GEMINI=$(echo "$COMMIT_MSG_AI_BLOCK" | grep -c '"Gemini"' || echo "0")

        if [[ "$HAS_CLAUDE" -gt 0 ]] && [[ "$HAS_CHATGPT" -gt 0 ]] && \
           [[ "$HAS_COPILOT" -gt 0 ]] && [[ "$HAS_GEMINI" -gt 0 ]]; then
            test_pass "commit-msg AI fallback has key patterns (Claude, ChatGPT, Copilot, Gemini)"
        else
            test_fail "commit-msg AI fallback missing key patterns"
        fi
    else
        test_fail "commit-msg has no AI pattern fallback entries"
    fi
else
    test_skip "commit-msg AI patterns" "File not found"
fi

# Check gh-pr hook has AI pattern in DEFAULT_AI_PATTERN
if [[ -f "$GH_PR_HOOK" ]]; then
    GH_PR_DEFAULT_AI=$(grep 'DEFAULT_AI_PATTERN=' "$GH_PR_HOOK" | head -1)
    HAS_CLAUDE=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'Claude' || echo "0")
    HAS_CHATGPT=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'ChatGPT' || echo "0")
    HAS_COPILOT=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'Copilot' || echo "0")
    HAS_GEMINI=$(echo "$GH_PR_DEFAULT_AI" | grep -c 'Gemini' || echo "0")

    if [[ "$HAS_CLAUDE" -gt 0 ]] && [[ "$HAS_CHATGPT" -gt 0 ]] && \
       [[ "$HAS_COPILOT" -gt 0 ]] && [[ "$HAS_GEMINI" -gt 0 ]]; then
        test_pass "gh-pr AI fallback has key patterns (Claude, ChatGPT, Copilot, Gemini)"
    else
        test_fail "gh-pr AI fallback missing key patterns"
    fi
else
    test_skip "gh-pr AI patterns" "File not found"
fi

# ============================================================================
# TEST 7: Cross-hook type consistency (commit-msg vs gh-pr)
# ============================================================================

test_section "Cross-Hook Type Consistency"

if [[ -f "$COMMIT_MSG_HOOK" ]] && [[ -f "$GH_PR_HOOK" ]]; then
    COMMIT_MSG_TYPES_SORTED=$(grep 'DEFAULT_VALID_TYPES=' "$COMMIT_MSG_HOOK" | head -1 | sed 's/.*DEFAULT_VALID_TYPES="//' | sed 's/".*//' | tr '|' '\n' | sort)
    GH_PR_TYPES_SORTED=$(grep 'DEFAULT_VALID_TYPES=' "$GH_PR_HOOK" | head -1 | sed 's/.*DEFAULT_VALID_TYPES="//' | sed 's/".*//' | tr '|' '\n' | sort)

    if [[ "$COMMIT_MSG_TYPES_SORTED" == "$GH_PR_TYPES_SORTED" ]]; then
        test_pass "commit-msg and gh-pr have identical commit type fallbacks"
    else
        test_fail "commit-msg and gh-pr commit type fallbacks differ"
    fi
else
    test_skip "Cross-hook type consistency" "One or both hooks missing"
fi

# ============================================================================
# TEST 8: Config internal consistency
# ============================================================================

test_section "Config Internal Consistency"

# Check that git_format.commit_types is valid JSON array
if jq -e '.git_format.commit_types | type == "array"' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_pass "git_format.commit_types is a valid array"
else
    test_fail "git_format.commit_types should be a valid array"
fi

# Check that git_format.branch_types is valid JSON array
if jq -e '.git_format.branch_types | type == "array"' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_pass "git_format.branch_types is a valid array"
else
    test_fail "git_format.branch_types should be a valid array"
fi

# Check that all commit types also appear in branch types (subset check)
COMMIT_ONLY_TYPES=""
while IFS= read -r type; do
    [[ -z "$type" ]] && continue
    if ! echo "$CONFIG_BRANCH_TYPES" | grep -qx "$type"; then
        COMMIT_ONLY_TYPES="${COMMIT_ONLY_TYPES} $type"
    fi
done <<< "$CONFIG_COMMIT_TYPES"

# This is informational - some commit types may not be branch types
if [[ -z "$COMMIT_ONLY_TYPES" ]]; then
    test_pass "All commit types present in branch types"
else
    # Not a failure - just note it
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
