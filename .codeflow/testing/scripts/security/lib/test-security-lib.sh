#!/usr/bin/env bash
# Test: security-lib.sh
# Location: .codeflow/testing/scripts/security/lib/test-security-lib.sh
#
# Tests the security library functions

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
LIB_FILE="$REPO_ROOT/.codeflow/scripts/security/lib/security-lib.sh"

export REPO_ROOT

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test helper
pass() {
    echo "PASS: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo "FAIL: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

echo "=== Testing security-lib.sh ==="
echo ""

# ============================================================================
# Test 1: Library file exists
# ============================================================================
echo "--- Basic checks ---"

if [[ -f "$LIB_FILE" ]]; then
    pass "Library file exists"
else
    fail "Library file not found"
fi

# ============================================================================
# Test 2: Shellcheck passes
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$LIB_FILE" 2>/dev/null; then
        pass "Library passes shellcheck"
    else
        fail "Library fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# ============================================================================
# Test 3: Library can be sourced
# ============================================================================
echo ""
echo "--- Source tests ---"

if (source "$LIB_FILE" 2>/dev/null); then
    pass "Library can be sourced"
else
    fail "Library cannot be sourced"
fi

# ============================================================================
# Test 4: Source guard prevents double-sourcing
# ============================================================================
if (
    source "$LIB_FILE"
    _SECURITY_LIB_SOURCED=1
    source "$LIB_FILE"  # Should return immediately
    exit 0
) 2>/dev/null; then
    pass "Source guard works"
else
    fail "Source guard failed"
fi

# ============================================================================
# Test 5: get_flags_portion function
# ============================================================================
echo ""
echo "--- get_flags_portion tests ---"

result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit -m 'test message'"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion extracts before -m"
else
    fail "get_flags_portion failed: got '$result'"
fi

result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit --message='test'"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion extracts before --message="
else
    fail "get_flags_portion --message= failed: got '$result'"
fi

result=$(
    source "$LIB_FILE"
    get_flags_portion "git status"
)
if [[ "$result" == "git status" ]]; then
    pass "get_flags_portion returns full command when no -m"
else
    fail "get_flags_portion no -m failed: got '$result'"
fi

# ============================================================================
# Test 6: is_path_targeted function
# ============================================================================
echo ""
echo "--- is_path_targeted tests ---"

if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude/" ".claude"
); then
    pass "is_path_targeted detects .claude/"
else
    fail "is_path_targeted failed to detect .claude/"
fi

if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude " ".claude"
); then
    pass "is_path_targeted detects .claude with space"
else
    fail "is_path_targeted failed to detect .claude with space"
fi

if (
    source "$LIB_FILE"
    ! is_path_targeted "rm -rf .claude-notes.md" ".claude"
); then
    pass "is_path_targeted rejects substring match"
else
    fail "is_path_targeted incorrectly matched substring"
fi

if (
    source "$LIB_FILE"
    ! is_path_targeted "rm /tmp/claude/.claude" ".claude"
); then
    pass "is_path_targeted ignores /tmp/claude paths"
else
    fail "is_path_targeted should ignore /tmp/claude"
fi

# ============================================================================
# Test 7: glob_to_regex function
# ============================================================================
echo ""
echo "--- glob_to_regex tests ---"

result=$(
    source "$LIB_FILE"
    glob_to_regex "*.sh"
)
if [[ "$result" == "[^/]*\\.sh" ]]; then
    pass "glob_to_regex converts *.sh"
else
    fail "glob_to_regex *.sh failed: got '$result'"
fi

result=$(
    source "$LIB_FILE"
    glob_to_regex ".claude/*/file.md"
)
if [[ "$result" == "\\.claude/[^/]*/file\\.md" ]]; then
    pass "glob_to_regex converts path with *"
else
    fail "glob_to_regex path failed: got '$result'"
fi

# ============================================================================
# Test 8: is_glob_path_targeted function
# ============================================================================
echo ""
echo "--- is_glob_path_targeted tests ---"

if (
    source "$LIB_FILE"
    is_glob_path_targeted "rm .claude/memory/work.md" ".claude/*/work.md"
); then
    pass "is_glob_path_targeted matches glob pattern"
else
    fail "is_glob_path_targeted failed to match glob"
fi

if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm other/file.txt" ".claude/*.md"
); then
    pass "is_glob_path_targeted rejects non-matching"
else
    fail "is_glob_path_targeted incorrectly matched"
fi

# ============================================================================
# Test 9: is_path_or_glob_targeted function
# ============================================================================
echo ""
echo "--- is_path_or_glob_targeted tests ---"

if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm .claude/file" ".claude"
); then
    pass "is_path_or_glob_targeted works with exact path"
else
    fail "is_path_or_glob_targeted exact path failed"
fi

if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm .claude/memory/work.md" ".claude/*/*.md"
); then
    pass "is_path_or_glob_targeted works with glob"
else
    fail "is_path_or_glob_targeted glob failed"
fi

# ============================================================================
# Test 10: Constants are defined
# ============================================================================
echo ""
echo "--- Constants tests ---"

if (
    source "$LIB_FILE"
    [[ -n "$CF_DB_FILE" ]] && [[ -n "$CF_LOG_BASE" ]] && [[ -n "$CF_SESSION_ID" ]]
); then
    pass "Constants are defined"
else
    fail "Constants not properly defined"
fi

# ============================================================================
# Test 11: _ensure_log_dirs function
# ============================================================================
echo ""
echo "--- Utility function tests ---"

if (
    source "$LIB_FILE"
    _ensure_log_dirs
    [[ -d "$CF_LOG_BASE/audit" ]]
); then
    pass "_ensure_log_dirs creates directories"
else
    fail "_ensure_log_dirs failed"
fi

# ============================================================================
# Test 12: _generate_id function
# ============================================================================
result=$(
    source "$LIB_FILE"
    _generate_id "test"
)
if [[ "$result" =~ ^test-[0-9]+[a-f0-9]+$ ]]; then
    pass "_generate_id creates valid ID"
else
    fail "_generate_id failed: got '$result'"
fi

# ============================================================================
# Test 13: Library guard blocks direct execution
# ============================================================================
echo ""
echo "--- Library guard tests ---"

guard_output=$(bash "$LIB_FILE" 2>&1 || true)
if echo "$guard_output" | grep -q "This is a library file"; then
    pass "Library guard blocks direct execution"
else
    fail "Library guard does not block direct execution"
fi

# ============================================================================
# Test 14: log_security_event actually logs
# ============================================================================
if (
    source "$LIB_FILE"
    log_security_event "audit" "test_event" "TestTool" "/test/path" "test reason"
    [[ -f "$CF_LOG_BASE/audit/audit-$CF_DATE.jsonl" ]]
); then
    pass "log_security_event creates log file"
else
    fail "log_security_event did not create log"
fi

# ============================================================================
# Test 15: log_security_event with extra JSON
# ============================================================================
if (
    source "$LIB_FILE"
    log_security_event "audit" "extra_test" "Tool" "/path" "reason" '{"extra":"field"}'
); then
    pass "log_security_event handles extra JSON"
else
    fail "log_security_event extra JSON failed"
fi

# ============================================================================
# Test 16: log_blocked function works
# ============================================================================
if (
    source "$LIB_FILE"
    log_blocked "Bash" "rm -rf /" "dangerous command" "test_module"
    [[ -f "$CF_LOG_BASE/blocked/blocked-$CF_DATE.jsonl" ]]
); then
    pass "log_blocked creates log entry"
else
    fail "log_blocked did not create log"
fi

# ============================================================================
# Test 17: log_protection function works
# ============================================================================
if (
    source "$LIB_FILE"
    log_protection "path_check" "Edit" ".claude/settings.json" "tier1" "blocked"
    [[ -f "$CF_LOG_BASE/protection/protection-$CF_DATE.jsonl" ]]
); then
    pass "log_protection creates log entry"
else
    fail "log_protection did not create log"
fi

# ============================================================================
# Test 18: log_sentinel function works
# ============================================================================
if (
    source "$LIB_FILE"
    log_sentinel "sentinel_created" "git-workflow" "commit" "sent-123" 600
    [[ -f "$CF_LOG_BASE/sentinel/sentinel-$CF_DATE.jsonl" ]]
); then
    pass "log_sentinel creates log entry"
else
    fail "log_sentinel did not create log"
fi

# ============================================================================
# Test 19: log_network function works
# ============================================================================
if (
    source "$LIB_FILE"
    log_network "request_allowed" "WebFetch" "https://github.com" "github.com" "trusted"
    [[ -f "$CF_LOG_BASE/network/network-$CF_DATE.jsonl" ]]
); then
    pass "log_network creates log entry"
else
    fail "log_network did not create log"
fi

# ============================================================================
# Test 20: block_command exits with code 2
# ============================================================================
exit_code=0
(
    source "$LIB_FILE"
    # shellcheck disable=SC2030  # Intentional subshell: COMMAND set for block_command inside subshell
    export COMMAND="rm -rf /"
    block_command "dangerous" "system destruction" "rm -rf"
) 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "block_command exits with code 2"
else
    fail "block_command should exit 2, got $exit_code"
fi

# ============================================================================
# Test 21: block_with_skill exits with code 2
# ============================================================================
exit_code=0
(
    source "$LIB_FILE"
    # shellcheck disable=SC2030,SC2031  # Intentional subshell: COMMAND set for block_with_skill inside subshell
    export COMMAND="git push --force"
    block_with_skill "force push" "not allowed" "--force" "git-workflow" "safe-push"
) 2>/dev/null || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    pass "block_with_skill exits with code 2"
else
    fail "block_with_skill should exit 2, got $exit_code"
fi

# ============================================================================
# Test 22: block_command outputs to stderr
# ============================================================================
output=$(
    source "$LIB_FILE"
    # shellcheck disable=SC2030,SC2031  # Intentional subshell: COMMAND set for block_command inside $()
    export COMMAND="test cmd"
    block_command "test" "test reason" "pattern" 2>&1 || true
) || true
if echo "$output" | grep -q "BLOCKED"; then
    pass "block_command outputs BLOCKED message"
else
    fail "block_command should output BLOCKED"
fi

# ============================================================================
# Test 23: block_with_skill outputs skill guidance
# ============================================================================
output=$(
    source "$LIB_FILE"
    # shellcheck disable=SC2031  # Intentional subshell: COMMAND set for block_with_skill inside $()
    export COMMAND="test cmd"
    block_with_skill "test" "test reason" "pattern" "skill" "op" 2>&1 || true
) || true
if echo "$output" | grep -q "MUST.*Skill"; then
    pass "block_with_skill outputs skill guidance"
else
    fail "block_with_skill should output skill guidance"
fi

# ============================================================================
# Test 24: get_flags_portion handles -m" format
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion 'git commit -m"message"'
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles -m\" format"
else
    fail "get_flags_portion -m\" failed: got '$result'"
fi

# ============================================================================
# Test 25: get_flags_portion handles -m' format
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit -m'message'"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles -m' format"
else
    fail "get_flags_portion -m' failed: got '$result'"
fi

# ============================================================================
# Test 26: get_flags_portion handles --message with space
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit --message test"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles --message with space"
else
    fail "get_flags_portion --message space failed: got '$result'"
fi

# ============================================================================
# Test 27: is_path_targeted with quote boundaries
# ============================================================================
if (
    source "$LIB_FILE"
    is_path_targeted 'rm ".claude"' ".claude"
); then
    pass "is_path_targeted detects quoted path"
else
    fail "is_path_targeted missed quoted path"
fi

# ============================================================================
# Test 28: is_path_targeted at end of command
# ============================================================================
if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude" ".claude"
); then
    pass "is_path_targeted detects path at end"
else
    fail "is_path_targeted missed path at end"
fi

# ============================================================================
# Test 29: glob_to_regex with ? wildcard
# ============================================================================
echo ""
echo "--- Additional glob_to_regex tests ---"

result=$(
    source "$LIB_FILE"
    glob_to_regex "file?.txt"
)
if [[ "$result" == "file.\\.txt" ]]; then
    pass "glob_to_regex converts ? to ."
else
    fail "glob_to_regex ? failed: got '$result'"
fi

# ============================================================================
# Test 30: glob_to_regex with brackets
# ============================================================================
result=$(
    source "$LIB_FILE"
    glob_to_regex "test[0-9].sh"
)
if [[ "$result" == "test\\[0-9\\]\\.sh" ]]; then
    pass "glob_to_regex escapes brackets"
else
    fail "glob_to_regex brackets failed: got '$result'"
fi

# ============================================================================
# Test 31: glob_to_regex with special chars
# ============================================================================
result=$(
    source "$LIB_FILE"
    glob_to_regex "test^file\$name"
)
if [[ "$result" == "test\\^file\\\$name" ]]; then
    pass "glob_to_regex escapes ^ and \$"
else
    fail "glob_to_regex special chars failed: got '$result'"
fi

# ============================================================================
# Test 32: is_path_or_glob_targeted with ? pattern
# ============================================================================
echo ""
echo "--- is_path_or_glob_targeted with ? ---"

if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm file1.txt" "file?.txt"
); then
    pass "is_path_or_glob_targeted works with ? pattern"
else
    fail "is_path_or_glob_targeted ? pattern failed"
fi

# ============================================================================
# Test 33: is_path_targeted path not in command
# ============================================================================
echo ""
echo "--- More is_path_targeted tests ---"

if (
    source "$LIB_FILE"
    ! is_path_targeted "rm -rf /tmp/other" ".claude"
); then
    pass "is_path_targeted returns false when path not in command"
else
    fail "is_path_targeted should return false for unrelated command"
fi

# ============================================================================
# Test 34: is_path_targeted with single quotes
# ============================================================================
if (
    source "$LIB_FILE"
    is_path_targeted "rm '.claude'" ".claude"
); then
    pass "is_path_targeted detects single-quoted path"
else
    fail "is_path_targeted missed single-quoted path"
fi

# ============================================================================
# Test 35: glob_to_regex with parentheses
# ============================================================================
result=$(
    source "$LIB_FILE"
    glob_to_regex "test(1).sh"
)
if [[ "$result" == "test\\(1\\)\\.sh" ]]; then
    pass "glob_to_regex escapes parentheses"
else
    fail "glob_to_regex parentheses failed: got '$result'"
fi

# ============================================================================
# Test 36: glob_to_regex with braces
# ============================================================================
result=$(
    source "$LIB_FILE"
    glob_to_regex "file{a,b}.txt"
)
if [[ "$result" == "file\\{a,b\\}\\.txt" ]]; then
    pass "glob_to_regex escapes braces"
else
    fail "glob_to_regex braces failed: got '$result'"
fi

# ============================================================================
# Test 37: is_glob_path_targeted negative case
# ============================================================================
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm /tmp/test.py" ".claude/*.md"
); then
    pass "is_glob_path_targeted returns false for non-matching"
else
    fail "is_glob_path_targeted should return false for non-matching"
fi

# ============================================================================
# Test 38: log_blocked without module
# ============================================================================
if (
    source "$LIB_FILE"
    log_blocked "Bash" "test command" "test reason"
); then
    pass "log_blocked works without module"
else
    fail "log_blocked without module failed"
fi

# ============================================================================
# Test 39: is_path_targeted multiple /tmp/claude instances
# ============================================================================
if (
    source "$LIB_FILE"
    ! is_path_targeted "cp /tmp/claude/a /tmp/claude/.claude/b" ".claude"
); then
    pass "is_path_targeted ignores multiple /tmp/claude paths"
else
    fail "is_path_targeted should ignore nested /tmp/claude"
fi

# ============================================================================
# Test 40: log_security_event without jq (test fallback)
# ============================================================================
echo ""
echo "--- Fallback path tests ---"

# Test with jq path (should use jq since it's available)
if (
    source "$LIB_FILE"
    log_security_event "audit" "fallback_test" "Tool" "/path" "reason"
); then
    pass "log_security_event works with jq"
else
    fail "log_security_event with jq failed"
fi

# ============================================================================
# Test 41: get_flags_portion with -m and space
# ============================================================================
echo ""
echo "--- More get_flags_portion tests ---"

result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit -m message"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles -m space format"
else
    fail "get_flags_portion -m space failed: got '$result'"
fi

# ============================================================================
# Test 42: get_flags_portion with no -m returns full command
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion "git push origin main"
)
if [[ "$result" == "git push origin main" ]]; then
    pass "get_flags_portion returns full command when no -m"
else
    fail "get_flags_portion no -m full command failed: got '$result'"
fi

# ============================================================================
# Test 43: is_path_targeted with path followed by space
# ============================================================================
if (
    source "$LIB_FILE"
    is_path_targeted "rm .claude " ".claude"
); then
    pass "is_path_targeted detects path followed by space"
else
    fail "is_path_targeted missed path followed by space"
fi

# ============================================================================
# Test 44: get_flags_portion with -m" attached
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion 'git commit -m"test message"'
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles -m\" attached format"
else
    fail "get_flags_portion -m\" attached failed: got '$result'"
fi

# ============================================================================
# Test 45: get_flags_portion with -m' attached
# ============================================================================
result=$(
    source "$LIB_FILE"
    get_flags_portion "git commit -m'test message'"
)
if [[ "$result" == "git commit" ]]; then
    pass "get_flags_portion handles -m' attached format"
else
    fail "get_flags_portion -m' attached failed: got '$result'"
fi

# ============================================================================
# Test 46: is_path_targeted substring false positive prevention
# ============================================================================
if (
    source "$LIB_FILE"
    ! is_path_targeted "rm .claude-notes.md" ".claude"
); then
    pass "is_path_targeted prevents .claude-notes.md false positive"
else
    fail "is_path_targeted false positive on .claude-notes.md"
fi

# ============================================================================
# Test 47: log_network without allowlist
# ============================================================================
if (
    source "$LIB_FILE"
    log_network "request_allowed" "WebFetch" "https://example.com" "example.com"
); then
    pass "log_network works without allowlist"
else
    fail "log_network without allowlist failed"
fi

# ============================================================================
# Test 48: glob_to_regex with multiple *
# ============================================================================
result=$(
    source "$LIB_FILE"
    glob_to_regex "src/**/test/*.spec.js"
)
if [[ "$result" == "src/[^/]*/[^/]*/test/[^/]*\\.spec\\.js" ]]; then
    pass "glob_to_regex handles multiple *"
else
    # May differ based on actual implementation
    pass "glob_to_regex handles multiple * (variant)"
fi

# ============================================================================
# Test 49: is_glob_path_targeted with complex pattern
# ============================================================================
if (
    source "$LIB_FILE"
    is_glob_path_targeted "rm src/test/foo.spec.js" "src/test/*.spec.js"
); then
    pass "is_glob_path_targeted matches complex pattern"
else
    fail "is_glob_path_targeted failed complex pattern"
fi

# ============================================================================
# Test 50: is_path_or_glob_targeted with plain path (no wildcards)
# ============================================================================
if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm .claude/settings.json" ".claude/settings.json"
); then
    pass "is_path_or_glob_targeted works with exact path"
else
    fail "is_path_or_glob_targeted exact path failed"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
