#!/usr/bin/env bash
# Test: Git commit-msg hook
# Location: .codeflow/testing/scripts/git-hooks/test-commit-msg.sh
#
# Tests the commit-msg hook functionality including:
#   - Config-driven validation
#   - Conventional commit format (strict type: format)
#   - AI attribution blocking
#   - Body format enforcement (bullets-only)
#   - Skip conditions (merge, revert, fixup, squash)

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/commit-msg"
TEMP_MSG="/tmp/claude/test-commit-msg-$$"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Ensure temp directory exists
mkdir -p /tmp/claude

# Cleanup (called via trap)
# shellcheck disable=SC2329
cleanup() {
    rm -f "$TEMP_MSG"
}
trap cleanup EXIT

# Test helper - run hook with message, check exit code
test_message() {
    local message="$1"
    local expected_exit="$2"
    local description="$3"

    echo "$message" > "$TEMP_MSG"

    set +e
    bash "$HOOK" "$TEMP_MSG" >/dev/null 2>&1
    local actual_exit=$?
    set -e

    if [[ "$actual_exit" -eq "$expected_exit" ]]; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description (expected exit $expected_exit, got $actual_exit)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Test helper - run hook with message, check exit code AND output contains text
test_message_with_output() {
    local message="$1"
    local expected_exit="$2"
    local expected_output="$3"
    local description="$4"

    echo "$message" > "$TEMP_MSG"

    set +e
    local output
    output=$(bash "$HOOK" "$TEMP_MSG" 2>&1)
    local actual_exit=$?
    set -e

    if [[ "$actual_exit" -eq "$expected_exit" ]] && echo "$output" | grep -qi "$expected_output"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    elif [[ "$actual_exit" -ne "$expected_exit" ]]; then
        echo "FAIL: $description (expected exit $expected_exit, got $actual_exit)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    else
        echo "FAIL: $description (output missing: $expected_output)"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing Git Commit-Msg Hook ==="
echo ""

# ============================================================================
# Test 1: Hook exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -f "$HOOK" ]]; then
    echo "PASS: Hook file exists"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Hook file does not exist"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

if [[ -x "$HOOK" ]]; then
    echo "PASS: Hook is executable"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Hook is not executable"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 2: Shellcheck passes
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        echo "PASS: Hook passes shellcheck"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Hook fails shellcheck"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# ============================================================================
# Test 3: Valid conventional commits (basic types)
# ============================================================================
echo ""
echo "--- Valid commit messages ---"

test_message "feat: add new authentication feature" 0 "Should accept feat type"
test_message "fix: resolve null pointer exception" 0 "Should accept fix type"
test_message "docs: update README with examples" 0 "Should accept docs type"
test_message "refactor: simplify user validation" 0 "Should accept refactor type"
test_message "test: add unit tests for auth flow" 0 "Should accept test type"
test_message "chore: update dependencies to latest" 0 "Should accept chore type"
test_message "perf: optimize database query speed" 0 "Should accept perf type"
test_message "style: fix code formatting issues" 0 "Should accept style type"
test_message "build: update webpack configuration" 0 "Should accept build type"
test_message "ci: add GitHub Actions workflow" 0 "Should accept ci type"
test_message "revert: undo previous auth changes" 0 "Should accept revert type"

# ============================================================================
# Test 4: Additional types from config (beyond original 11)
# ============================================================================
echo ""
echo "--- Config-driven commit types ---"

test_message "bugfix: correct off-by-one error" 0 "Should accept bugfix type"
test_message "hotfix: patch security vulnerability" 0 "Should accept hotfix type"
test_message "merge: combine feature branches" 0 "Should accept merge type"
test_message "plan: design new module structure" 0 "Should accept plan type"
test_message "refine: improve error handling path" 0 "Should accept refine type"

# ============================================================================
# Test 5: Scope support
# ============================================================================
echo ""
echo "--- Scope rejection ---"

test_message "feat(api): add new endpoint for users" 1 "Should reject type with scope"
test_message "fix(db): correct connection pool leak" 1 "Should reject scoped fix"
test_message "docs(readme): update setup instructions" 1 "Should reject scoped docs"
test_message "refactor(auth): simplify token logic" 1 "Should reject scoped refactor"
test_message "fix(my-module): handle edge case input" 1 "Should reject hyphenated scope"

# ============================================================================
# Test 6: Invalid commit messages
# ============================================================================
echo ""
echo "--- Invalid commit messages ---"

test_message "Updated the code" 1 "Should reject non-conventional format"
test_message "feat:missing space" 1 "Should reject missing space after colon"
test_message "invalid: some change description" 1 "Should reject unknown type"
test_message "FEAT: uppercase type not allowed" 1 "Should reject uppercase type"

# ============================================================================
# Test 7: Subject length validation
# ============================================================================
echo ""
echo "--- Subject length ---"

test_message "feat: this is exactly at the fifty char limit ok" 0 "Should accept 50-char subject"
test_message "feat: this commit message subject line is definitely way too long for the limit" 1 "Should reject >50 char subject"

# ============================================================================
# Test 8: Trailing period
# ============================================================================
echo ""
echo "--- Trailing period ---"

test_message "feat: add feature without period" 0 "Should accept no trailing period"
test_message "feat: add feature with a period." 1 "Should reject trailing period"

# ============================================================================
# Test 9: Skip conditions
# ============================================================================
echo ""
echo "--- Skip conditions ---"

test_message "Merge pull request #42 from feat/auth" 0 "Should skip GitHub merge commits"
test_message "Merge branch 'feature' into main" 0 "Should skip git merge commits"
test_message "Revert \"previous commit message\"" 0 "Should skip revert commits"
test_message "fixup! feat: original commit message" 0 "Should skip fixup commits"
test_message "squash! feat: original commit message" 0 "Should skip squash commits"

# ============================================================================
# Test 10: AI attribution blocking
# ============================================================================
echo ""
echo "--- AI attribution blocking ---"

test_message_with_output "feat: add feature by Claude" 1 "AI attribution" "Should block Claude mention"
test_message_with_output "feat: add feature by ChatGPT" 1 "AI attribution" "Should block ChatGPT mention"
test_message_with_output "feat: add feature with Copilot" 1 "AI attribution" "Should block Copilot mention"
test_message_with_output "feat: Generated with AI tools" 1 "AI attribution" "Should block Generated with"

# Test AI patterns in body
test_message_with_output "$(printf 'feat: add auth module\n\n- Co-Authored-By: Claude')" 1 "AI attribution" "Should block AI attribution in body"

# ============================================================================
# Test 11: Body format validation
# ============================================================================
echo ""
echo "--- Body format ---"

# Valid body with bullets
test_message "$(printf 'feat: add auth module\n\n- Add login endpoint\n- Add logout endpoint')" 0 "Should accept valid bullet body"

# Valid: no body at all (subject only)
test_message "feat: add auth module with tests" 0 "Should accept message with no body"

# Valid: subject + blank line + single bullet
test_message "$(printf 'feat: add auth module\n\n- Initial implementation')" 0 "Should accept single bullet body"

# Invalid: prose in body (not bullets)
test_message "$(printf 'feat: add auth module\n\nThis adds authentication.')" 1 "Should reject prose body (not bullets)"

# Invalid: too many bullets
test_message "$(printf 'feat: add auth module\n\n- One\n- Two\n- Three\n- Four')" 1 "Should reject >3 bullets"

# Valid: exactly 3 bullets
test_message "$(printf 'feat: add auth module\n\n- One change\n- Two change\n- Three change')" 0 "Should accept exactly 3 bullets"

# Invalid: squash-merge asterisk bullets
test_message "$(printf 'feat: add auth module\n\n* First commit\n* Second commit')" 1 "Should reject asterisk bullets"

# Invalid: continuation lines (wrapped bullets)
test_message "$(printf 'feat: add auth module\n\n- Fix something for both\n  PathFlow and non-PathFlow')" 1 "Should reject continuation lines"

# Invalid: blank lines between bullets
test_message "$(printf 'feat: add auth module\n\n- First bullet\n\n- Second bullet')" 1 "Should reject blank lines between bullets"

# Invalid: blank lines between three bullets
test_message "$(printf 'feat: add auth module\n\n- One\n\n- Two\n\n- Three')" 1 "Should reject blank lines between three bullets"

# ============================================================================
# Test 12: Body line length
# ============================================================================
echo ""
echo "--- Body line length ---"

# Valid: line within limit
test_message "$(printf 'feat: add auth module\n\n- Short bullet point line')" 0 "Should accept short body line"

# Invalid: line over 72 chars
LONG_LINE="- This is a very long bullet point line that exceeds the seventy-two character limit for body lines"
test_message "$(printf 'feat: add auth module\n\n%s' "$LONG_LINE")" 1 "Should reject body line >72 chars"

# ============================================================================
# Test 13: Git comment lines filtered
# ============================================================================
echo ""
echo "--- Comment filtering ---"

# Message with git comment lines should work (comments filtered out)
test_message "$(printf 'feat: add auth module\n# This is a git comment\n# Another comment')" 0 "Should ignore git comment lines"

# ============================================================================
# Test 14: Missing blank line warning (not an error)
# ============================================================================
echo ""
echo "--- Blank line after subject ---"

# Body directly after subject (no blank line) - should warn but not fail if bullets valid
test_message "$(printf 'feat: add auth module\n- Direct bullet')" 0 "Should accept body without blank line (warn only)"

# ============================================================================
# Summary
# ============================================================================
echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
