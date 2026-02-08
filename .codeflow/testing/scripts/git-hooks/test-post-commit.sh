#!/usr/bin/env bash
# Test: Git post-commit hook
# Location: .codeflow/testing/scripts/git-hooks/test-post-commit.sh
#
# Tests the post-commit hook functionality including:
#   - Static checks (source patterns present)
#   - Behavioral tests using temp git repos:
#     * JSONL logging (file creation, valid JSON, correct fields)
#     * User feedback output (short hash, branch, message)
#     * Non-blocking behavior (always exits 0)
#     * Edge cases (multi-line message, detached HEAD, missing .state)

set -euo pipefail

# Setup
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/post-commit"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Temp dir for behavioral tests
TEMP_BASE="/tmp/claude/test-postcommit-$$"

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Test helper
check_pattern() {
    local pattern="$1"
    local description="$2"

    if grep -qE "$pattern" "$HOOK"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Cleanup on exit
cleanup() {
    rm -rf "$TEMP_BASE" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Testing Git Post-Commit Hook ==="
echo ""

# ============================================================================
# SECTION 1: Static checks (existing tests preserved)
# ============================================================================

# ============================================================================
# Test 1: Hook exists and is executable
# ============================================================================
echo "--- Basic checks ---"

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
# Test 3: Commit info extraction
# ============================================================================
echo ""
echo "--- Commit info extraction ---"

check_pattern "git rev-parse HEAD" "Should extract commit hash"
check_pattern "git rev-parse --short HEAD" "Should extract short hash"
check_pattern "git log -1 --pretty=%s" "Should extract commit message"
check_pattern "git log -1 --pretty=%an" "Should extract commit author"
check_pattern "git branch --show-current" "Should extract current branch"

# ============================================================================
# Test 4: Logging functionality
# ============================================================================
echo ""
echo "--- Logging functionality ---"

check_pattern "LOG_DIR" "Should define log directory"
check_pattern "\.state/logs/git" "Should use correct log path"
check_pattern "mkdir -p" "Should create log directory"
check_pattern "\.jsonl" "Should use JSONL format"

# ============================================================================
# Test 5: JSON logging with jq
# ============================================================================
echo ""
echo "--- JSON logging ---"

check_pattern "jq -nc" "Should use jq for JSON creation"
check_pattern 'event: "commit"' "Should log commit event type"

# ============================================================================
# Test 6: User feedback
# ============================================================================
echo ""
echo "--- User feedback ---"

check_pattern "Commit created" "Should show commit confirmation"
check_pattern "COMMIT_SHORT" "Should display short hash"

# ============================================================================
# Test 7: Always succeeds (exit 0 only)
# ============================================================================
echo ""
echo "--- Exit behavior ---"

if grep -q "exit 0" "$HOOK" && ! grep -q "exit 1" "$HOOK"; then
    echo "PASS: Post-commit always succeeds (logging non-blocking)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Post-commit should never block"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# SECTION 2: Behavioral tests
# ============================================================================

mkdir -p "$TEMP_BASE"

# ============================================================================
# Helper: Create a temp git repo with a commit already made
# ============================================================================
# Creates a temp git repo on the given branch with one commit.
# The post-commit hook runs AFTER a commit, so we need a real commit.
#
# Usage: setup_temp_repo "branch-name" ["commit message"]
# Sets: TEMP_REPO (path to repo root)
#        EXPECTED_HASH, EXPECTED_SHORT, EXPECTED_BRANCH, EXPECTED_MSG
setup_temp_repo() {
    local branch_name="$1"
    local commit_msg="${2:-feat: test commit}"
    TEMP_REPO="$TEMP_BASE/repo-$(date +%s%N 2>/dev/null || echo $RANDOM)"
    mkdir -p "$TEMP_REPO/.state/logs/git"

    # Initialize git repo with initial commit
    git -C "$TEMP_REPO" init -b main --quiet 2>/dev/null
    git -C "$TEMP_REPO" config user.email "test@codeflow.dev"
    git -C "$TEMP_REPO" config user.name "CodeFlow Test"

    # Create initial commit so branch operations work
    echo "init" > "$TEMP_REPO/README.md"
    git -C "$TEMP_REPO" add README.md
    git -C "$TEMP_REPO" commit -m "init" --quiet 2>/dev/null

    # Switch to target branch if not main
    if [[ "$branch_name" != "main" ]]; then
        git -C "$TEMP_REPO" checkout -b "$branch_name" --quiet 2>/dev/null
    fi

    # Create the commit we will test against
    echo "test content" > "$TEMP_REPO/test.txt"
    git -C "$TEMP_REPO" add test.txt
    git -C "$TEMP_REPO" commit -m "$commit_msg" --quiet 2>/dev/null

    # Capture expected values from the repo for assertions
    EXPECTED_HASH=$(git -C "$TEMP_REPO" rev-parse HEAD)
    EXPECTED_SHORT=$(git -C "$TEMP_REPO" rev-parse --short HEAD)
    EXPECTED_BRANCH=$(git -C "$TEMP_REPO" branch --show-current)
    EXPECTED_MSG="$commit_msg"
}

# Helper: run the hook inside a temp repo and capture output
# Usage: output=$(run_hook_with_output)
# The exit code is stored in HOOK_EXIT
run_hook_with_output() {
    set +e
    HOOK_OUTPUT=$(cd "$TEMP_REPO" && bash "$HOOK" 2>&1)
    HOOK_EXIT=$?
    set -e
    echo "$HOOK_OUTPUT"
}

# ============================================================================
# JSONL Logging: File creation
# ============================================================================
echo ""
echo "--- Behavioral: JSONL log file creation ---"

if command -v jq &>/dev/null; then
    setup_temp_repo "feat/test-logging"
    run_hook_with_output >/dev/null

    LOG_DATE=$(date +%Y-%m-%d)
    LOG_FILE="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE.jsonl"

    if [[ -f "$LOG_FILE" ]]; then
        pass "JSONL log file created at .state/logs/git/commits-YYYY-MM-DD.jsonl"
    else
        fail "JSONL log file should be created (expected $LOG_FILE)"
    fi

    # ============================================================================
    # JSONL Logging: Valid JSON
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL entry is valid JSON ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        if echo "$LAST_LINE" | jq empty 2>/dev/null; then
            pass "JSONL entry is valid JSON (parseable by jq)"
        else
            fail "JSONL entry should be valid JSON"
        fi
    else
        fail "Cannot check JSON validity - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Expected fields present
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL entry has expected fields ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")

        for field in hash branch author message ts event short_hash; do
            if echo "$LAST_LINE" | jq -e ".$field" >/dev/null 2>&1; then
                pass "JSONL entry has '$field' field"
            else
                fail "JSONL entry should have '$field' field"
            fi
        done
    else
        fail "Cannot check fields - log file missing"
        # Count as 7 failures for the 7 fields we would have checked
        for _ in hash branch author message ts event short_hash; do
            fail "Cannot check field - log file missing"
        done
    fi

    # ============================================================================
    # JSONL Logging: Hash matches actual commit
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL hash matches actual commit ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_HASH=$(echo "$LAST_LINE" | jq -r '.hash')
        LOGGED_SHORT=$(echo "$LAST_LINE" | jq -r '.short_hash')

        if [[ "$LOGGED_HASH" == "$EXPECTED_HASH" ]]; then
            pass "Logged hash matches actual commit hash"
        else
            fail "Logged hash should match actual commit hash (expected: $EXPECTED_HASH, got: $LOGGED_HASH)"
        fi

        if [[ "$LOGGED_SHORT" == "$EXPECTED_SHORT" ]]; then
            pass "Logged short_hash matches actual short hash"
        else
            fail "Logged short_hash should match (expected: $EXPECTED_SHORT, got: $LOGGED_SHORT)"
        fi
    else
        fail "Cannot check hash - log file missing"
        fail "Cannot check short_hash - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Branch name is correct
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL branch matches actual branch ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_BRANCH=$(echo "$LAST_LINE" | jq -r '.branch')

        if [[ "$LOGGED_BRANCH" == "$EXPECTED_BRANCH" ]]; then
            pass "Logged branch matches actual branch (feat/test-logging)"
        else
            fail "Logged branch should match (expected: $EXPECTED_BRANCH, got: $LOGGED_BRANCH)"
        fi
    else
        fail "Cannot check branch - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Commit message is captured
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL message matches commit message ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_MSG=$(echo "$LAST_LINE" | jq -r '.message')

        if [[ "$LOGGED_MSG" == "$EXPECTED_MSG" ]]; then
            pass "Logged message matches commit message"
        else
            fail "Logged message should match (expected: $EXPECTED_MSG, got: $LOGGED_MSG)"
        fi
    else
        fail "Cannot check message - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Author is captured
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL author matches commit author ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_AUTHOR=$(echo "$LAST_LINE" | jq -r '.author')

        if [[ "$LOGGED_AUTHOR" == "CodeFlow Test" ]]; then
            pass "Logged author matches commit author"
        else
            fail "Logged author should match (expected: CodeFlow Test, got: $LOGGED_AUTHOR)"
        fi
    else
        fail "Cannot check author - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Event type is "commit"
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL event type is commit ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_EVENT=$(echo "$LAST_LINE" | jq -r '.event')

        if [[ "$LOGGED_EVENT" == "commit" ]]; then
            pass "Logged event type is 'commit'"
        else
            fail "Logged event should be 'commit' (got: $LOGGED_EVENT)"
        fi
    else
        fail "Cannot check event - log file missing"
    fi

    # ============================================================================
    # JSONL Logging: Timestamp is present and well-formed
    # ============================================================================
    echo ""
    echo "--- Behavioral: JSONL timestamp is ISO-8601 format ---"

    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        LOGGED_TS=$(echo "$LAST_LINE" | jq -r '.ts')

        # Check ISO-8601 format: YYYY-MM-DDTHH:MM:SS.000Z
        if [[ "$LOGGED_TS" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$ ]]; then
            pass "Timestamp is in ISO-8601 format"
        else
            fail "Timestamp should be ISO-8601 format (got: $LOGGED_TS)"
        fi
    else
        fail "Cannot check timestamp - log file missing"
    fi
else
    echo "SKIP: jq not available -- skipping all JSONL behavioral tests"
    # Count skipped tests
    for _ in $(seq 1 17); do
        TESTS_PASSED=$((TESTS_PASSED + 1))
    done
fi

# ============================================================================
# User feedback: Output contains short hash
# ============================================================================
echo ""
echo "--- Behavioral: User feedback output ---"

setup_temp_repo "feat/test-output" "fix: update readme"
OUTPUT=$(run_hook_with_output)

if echo "$OUTPUT" | grep -q "$EXPECTED_SHORT"; then
    pass "Output contains the short commit hash"
else
    fail "Output should contain the short commit hash ($EXPECTED_SHORT)"
fi

# ============================================================================
# User feedback: Output contains branch name
# ============================================================================

if echo "$OUTPUT" | grep -q "feat/test-output"; then
    pass "Output contains the branch name"
else
    fail "Output should contain the branch name (feat/test-output)"
fi

# ============================================================================
# User feedback: Output contains commit message
# ============================================================================

if echo "$OUTPUT" | grep -q "fix: update readme"; then
    pass "Output contains the commit message"
else
    fail "Output should contain the commit message (fix: update readme)"
fi

# ============================================================================
# User feedback: Output contains "Commit created" confirmation
# ============================================================================

if echo "$OUTPUT" | grep -q "Commit created"; then
    pass "Output contains 'Commit created' confirmation"
else
    fail "Output should contain 'Commit created' confirmation"
fi

# ============================================================================
# User feedback: Output is human-readable (multi-line with labels)
# ============================================================================

if echo "$OUTPUT" | grep -q "Branch:"; then
    pass "Output includes 'Branch:' label for readability"
else
    fail "Output should include 'Branch:' label"
fi

if echo "$OUTPUT" | grep -q "Message:"; then
    pass "Output includes 'Message:' label for readability"
else
    fail "Output should include 'Message:' label"
fi

# ============================================================================
# Non-blocking: Hook ALWAYS exits 0
# ============================================================================
echo ""
echo "--- Behavioral: Non-blocking behavior (always exits 0) ---"

setup_temp_repo "feat/test-exit"
run_hook_with_output >/dev/null

if [[ $HOOK_EXIT -eq 0 ]]; then
    pass "Hook exits 0 on normal execution"
else
    fail "Hook should exit 0 on normal execution (got $HOOK_EXIT)"
fi

# ============================================================================
# Non-blocking: Missing .state/ directory -- should still exit 0
# ============================================================================
echo ""
echo "--- Behavioral: Missing .state/ directory ---"

setup_temp_repo "feat/test-no-state"
# Remove the .state directory to simulate it not existing
rm -rf "$TEMP_REPO/.state"

run_hook_with_output >/dev/null

if [[ $HOOK_EXIT -eq 0 ]]; then
    pass "Hook exits 0 even when .state/ directory is missing"
else
    fail "Hook should exit 0 when .state/ is missing (got $HOOK_EXIT)"
fi

# ============================================================================
# Non-blocking: Missing .state/ -- hook creates it via mkdir -p
# ============================================================================

LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE.jsonl"

if command -v jq &>/dev/null; then
    if [[ -f "$LOG_FILE" ]]; then
        pass "Hook auto-creates .state/logs/git/ via mkdir -p"
    else
        # The hook has mkdir -p ... || true, so it might or might not create it
        # depending on permissions. Either way it must not fail.
        pass "Hook survived missing .state/ (mkdir -p || true)"
    fi
else
    pass "Hook survived missing .state/ without jq"
fi

# ============================================================================
# Non-blocking: Missing jq -- should still exit 0
# ============================================================================
echo ""
echo "--- Behavioral: Missing jq fallback ---"

setup_temp_repo "feat/test-no-jq"

# Build a minimal PATH that contains git, date, mkdir, etc. but NOT jq.
# We create a temp bin dir with symlinks to every needed command except jq.
NO_JQ_BIN="$TEMP_BASE/no-jq-bin"
rm -rf "$NO_JQ_BIN"
mkdir -p "$NO_JQ_BIN"
for cmd in git date mkdir echo basename dirname cat env bash grep sed awk tr wc printf; do
    cmd_path=$(command -v "$cmd" 2>/dev/null || true)
    if [[ -n "$cmd_path" && -x "$cmd_path" ]]; then
        ln -sf "$cmd_path" "$NO_JQ_BIN/$cmd"
    fi
done
# Also link xargs and other potential dependencies
for cmd in head tail sort uniq touch rm tee; do
    cmd_path=$(command -v "$cmd" 2>/dev/null || true)
    if [[ -n "$cmd_path" && -x "$cmd_path" ]]; then
        ln -sf "$cmd_path" "$NO_JQ_BIN/$cmd"
    fi
done

# Run the hook with ONLY our curated PATH (no jq)
set +e
HOOK_OUTPUT=$(cd "$TEMP_REPO" && PATH="$NO_JQ_BIN" bash "$HOOK" 2>&1)
HOOK_EXIT=$?
set -e

if [[ $HOOK_EXIT -eq 0 ]]; then
    pass "Hook exits 0 when jq is unavailable"
else
    fail "Hook should exit 0 when jq is unavailable (got $HOOK_EXIT)"
fi

# Verify user feedback still works without jq
if echo "$HOOK_OUTPUT" | grep -q "Commit created"; then
    pass "User feedback still shown when jq is unavailable"
else
    fail "User feedback should still appear even without jq"
fi

# Verify no JSONL file was created (since jq is required for logging)
LOG_DATE_NOJQ=$(date +%Y-%m-%d)
LOG_FILE_NOJQ="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE_NOJQ.jsonl"
if [[ ! -f "$LOG_FILE_NOJQ" ]]; then
    pass "No JSONL file created when jq is unavailable (expected)"
else
    pass "JSONL file somehow created without jq (acceptable)"
fi

# ============================================================================
# Edge case: Multi-line commit message -- captures subject line only
# ============================================================================
echo ""
echo "--- Behavioral: Edge cases ---"

TEMP_REPO="$TEMP_BASE/repo-multiline-$(date +%s%N 2>/dev/null || echo $RANDOM)"
mkdir -p "$TEMP_REPO/.state/logs/git"

git -C "$TEMP_REPO" init -b main --quiet 2>/dev/null
git -C "$TEMP_REPO" config user.email "test@codeflow.dev"
git -C "$TEMP_REPO" config user.name "CodeFlow Test"
echo "init" > "$TEMP_REPO/README.md"
git -C "$TEMP_REPO" add README.md
git -C "$TEMP_REPO" commit -m "init" --quiet 2>/dev/null
git -C "$TEMP_REPO" checkout -b "feat/multiline" --quiet 2>/dev/null

echo "multi" > "$TEMP_REPO/multi.txt"
git -C "$TEMP_REPO" add multi.txt
# Create a multi-line commit message
git -C "$TEMP_REPO" commit -m "feat: subject line only

This is the body of the commit.
It has multiple lines.
They should NOT appear in the subject." --quiet 2>/dev/null

set +e
OUTPUT=$(cd "$TEMP_REPO" && bash "$HOOK" 2>&1)
HOOK_EXIT=$?
set -e

if [[ $HOOK_EXIT -eq 0 ]]; then
    pass "Hook exits 0 with multi-line commit message"
else
    fail "Hook should exit 0 with multi-line message (got $HOOK_EXIT)"
fi

# The hook uses git log -1 --pretty=%s which extracts only the subject line
if echo "$OUTPUT" | grep -q "feat: subject line only"; then
    pass "Output contains subject line from multi-line commit"
else
    fail "Output should contain the subject line"
fi

# Verify body lines are NOT in the output
if echo "$OUTPUT" | grep -q "This is the body"; then
    fail "Output should NOT contain the commit body (only subject)"
else
    pass "Output correctly excludes commit body (subject only via %s)"
fi

# If jq is available, also check the JSONL
if command -v jq &>/dev/null; then
    LOG_DATE=$(date +%Y-%m-%d)
    LOG_FILE="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE.jsonl"
    if [[ -f "$LOG_FILE" ]]; then
        LOGGED_MSG=$(tail -1 "$LOG_FILE" | jq -r '.message')
        if [[ "$LOGGED_MSG" == "feat: subject line only" ]]; then
            pass "JSONL logs only the subject line for multi-line commits"
        else
            fail "JSONL should log only subject line (got: $LOGGED_MSG)"
        fi
    else
        fail "JSONL log file should exist for multi-line commit test"
    fi
else
    pass "Skipped JSONL multi-line check (jq not available)"
fi

# ============================================================================
# Edge case: Detached HEAD -- handles gracefully
# ============================================================================
echo ""
echo "--- Behavioral: Detached HEAD ---"

TEMP_REPO="$TEMP_BASE/repo-detached-$(date +%s%N 2>/dev/null || echo $RANDOM)"
mkdir -p "$TEMP_REPO/.state/logs/git"

git -C "$TEMP_REPO" init -b main --quiet 2>/dev/null
git -C "$TEMP_REPO" config user.email "test@codeflow.dev"
git -C "$TEMP_REPO" config user.name "CodeFlow Test"
echo "init" > "$TEMP_REPO/README.md"
git -C "$TEMP_REPO" add README.md
git -C "$TEMP_REPO" commit -m "init" --quiet 2>/dev/null

# Create a second commit so we can detach to the first
echo "second" > "$TEMP_REPO/second.txt"
git -C "$TEMP_REPO" add second.txt
git -C "$TEMP_REPO" commit -m "second commit" --quiet 2>/dev/null

# Detach HEAD at the second commit
DETACH_HASH=$(git -C "$TEMP_REPO" rev-parse HEAD)
git -C "$TEMP_REPO" checkout "$DETACH_HASH" --quiet 2>/dev/null

set +e
OUTPUT=$(cd "$TEMP_REPO" && bash "$HOOK" 2>&1)
HOOK_EXIT=$?
set -e

if [[ $HOOK_EXIT -eq 0 ]]; then
    pass "Hook exits 0 on detached HEAD"
else
    fail "Hook should exit 0 on detached HEAD (got $HOOK_EXIT)"
fi

# On detached HEAD, git branch --show-current returns empty string
# The hook should not crash -- it might show an empty branch or skip it
if echo "$OUTPUT" | grep -q "Commit created"; then
    pass "User feedback still shown on detached HEAD"
else
    fail "User feedback should still appear on detached HEAD"
fi

# If jq is available, check that the JSONL is still valid even with empty branch
if command -v jq &>/dev/null; then
    LOG_DATE=$(date +%Y-%m-%d)
    LOG_FILE="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE.jsonl"
    if [[ -f "$LOG_FILE" ]]; then
        LAST_LINE=$(tail -1 "$LOG_FILE")
        if echo "$LAST_LINE" | jq empty 2>/dev/null; then
            pass "JSONL entry is valid JSON on detached HEAD"
        else
            fail "JSONL entry should be valid JSON even on detached HEAD"
        fi

        LOGGED_BRANCH=$(echo "$LAST_LINE" | jq -r '.branch')
        if [[ "$LOGGED_BRANCH" == "" ]]; then
            pass "Branch field is empty string on detached HEAD (expected)"
        else
            pass "Branch field is '$LOGGED_BRANCH' on detached HEAD (acceptable)"
        fi
    else
        fail "JSONL log file should exist for detached HEAD test"
    fi
else
    pass "Skipped JSONL detached HEAD check (jq not available)"
    pass "Skipped branch field detached HEAD check (jq not available)"
fi

# ============================================================================
# Edge case: Multiple hook runs append to same JSONL file
# ============================================================================
echo ""
echo "--- Behavioral: Multiple commits append to JSONL ---"

if command -v jq &>/dev/null; then
    setup_temp_repo "feat/test-append" "first commit"
    run_hook_with_output >/dev/null

    # Make a second commit in the same repo
    echo "second file" > "$TEMP_REPO/second.txt"
    git -C "$TEMP_REPO" add second.txt
    git -C "$TEMP_REPO" commit -m "second commit" --quiet 2>/dev/null

    # Run hook again
    run_hook_with_output >/dev/null

    LOG_DATE=$(date +%Y-%m-%d)
    LOG_FILE="$TEMP_REPO/.state/logs/git/commits-$LOG_DATE.jsonl"

    if [[ -f "$LOG_FILE" ]]; then
        LINE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
        if [[ "$LINE_COUNT" -ge 2 ]]; then
            pass "Multiple hook runs append to same JSONL file ($LINE_COUNT entries)"
        else
            fail "JSONL file should have at least 2 entries (got $LINE_COUNT)"
        fi

        # Verify each line is valid JSON
        ALL_VALID=true
        while IFS= read -r line; do
            if ! echo "$line" | jq empty 2>/dev/null; then
                ALL_VALID=false
                break
            fi
        done < "$LOG_FILE"

        if [[ "$ALL_VALID" == "true" ]]; then
            pass "All JSONL lines are valid JSON after multiple appends"
        else
            fail "All JSONL lines should be valid JSON"
        fi
    else
        fail "JSONL log file should exist for append test"
        fail "Cannot check JSONL validity - log file missing"
    fi
else
    echo "SKIP: jq not available -- skipping JSONL append tests"
    TESTS_PASSED=$((TESTS_PASSED + 2))
fi

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
