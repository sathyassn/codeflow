#!/usr/bin/env bash
# Test: Git pre-commit hook
# Location: .codeflow/testing/scripts/git-hooks/test-pre-commit.sh
#
# Tests the pre-commit hook functionality including:
#   - Static checks (source patterns present)
#   - Behavioral tests using temp git repos:
#     * Main branch protection (BLOCKS)
#     * Branch name validation (BLOCKS)
#     * Sensitive file detection (BLOCKS)
#     * JSON validation (BLOCKS)
#     * Valid feature branch (ALLOWS)

set -euo pipefail

# Setup
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-commit"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Temp dir for behavioral tests
TEMP_BASE="/tmp/claude/test-pre-commit-$$"

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Cleanup on exit
cleanup() {
    rm -rf "$TEMP_BASE" 2>/dev/null || true
}
trap cleanup EXIT

# ============================================================================
# Helper: Create a minimal git repo on a specific branch
# ============================================================================
# Creates a temp git repo with an initial commit, checks out the given branch,
# and copies the enforcement-policy.json so config-driven features work.
# Also stubs out the settings sync test so it doesn't interfere.
#
# Usage: setup_temp_repo "branch-name"
# Sets: TEMP_REPO (path to repo root)
setup_temp_repo() {
    local branch_name="$1"
    TEMP_REPO="$TEMP_BASE/repo-$(date +%s%N 2>/dev/null || echo $RANDOM)"
    mkdir -p "$TEMP_REPO"

    # Initialize git repo with initial commit
    git -C "$TEMP_REPO" init -b main --quiet 2>/dev/null
    git -C "$TEMP_REPO" config user.email "test@test.com"
    git -C "$TEMP_REPO" config user.name "Test"

    # Create initial commit (required for branch operations)
    echo "init" > "$TEMP_REPO/README.md"
    git -C "$TEMP_REPO" add README.md
    git -C "$TEMP_REPO" commit -m "init" --quiet 2>/dev/null

    # Create config directory structure so the hook can read enforcement-policy.json
    mkdir -p "$TEMP_REPO/.codeflow/config/enforcement"
    if [[ -f "$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json" ]]; then
        cp "$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json" \
           "$TEMP_REPO/.codeflow/config/enforcement/enforcement-policy.json"
    fi

    # Create a stub settings sync test that always passes
    # (the real one checks templates that don't exist in the temp repo)
    mkdir -p "$TEMP_REPO/.codeflow/testing/consistency"
    cat > "$TEMP_REPO/.codeflow/testing/consistency/test-settings-sync.sh" << 'STUB'
#!/usr/bin/env bash
echo "PASS: stub settings sync"
exit 0
STUB
    chmod +x "$TEMP_REPO/.codeflow/testing/consistency/test-settings-sync.sh"

    # Stub out the coverage library to avoid sourcing the real one
    mkdir -p "$TEMP_REPO/.codeflow/testing/lib"
    cat > "$TEMP_REPO/.codeflow/testing/lib/test-coverage.sh" << 'STUB'
#!/usr/bin/env bash
validate_staged_coverage() { return 0; }
STUB

    # Switch to target branch if not main
    if [[ "$branch_name" != "main" ]]; then
        git -C "$TEMP_REPO" checkout -b "$branch_name" --quiet 2>/dev/null
    fi
}

# Helper: run the hook inside a temp repo
# Usage: run_hook_in_repo
# Returns: exit code from hook
run_hook_in_repo() {
    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    local rc=$?
    set -e
    return "$rc"
}

# Helper: run hook and capture output
# Usage: output=$(run_hook_in_repo_with_output)
run_hook_in_repo_with_output() {
    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") 2>&1
    set -e
}

echo "=== Testing Git Pre-Commit Hook ==="
echo ""

# ============================================================================
# SECTION 1: Static checks (existing tests preserved)
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$HOOK" ]]; then
    pass "Hook is executable"
else
    fail "Hook is not executable"
fi

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Hook passes shellcheck"
    else
        fail "Hook fails shellcheck"
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

echo ""
echo "--- Sensitive file detection (static) ---"

if grep -q "\.env" "$HOOK"; then
    pass ".env pattern defined"
else
    fail ".env pattern not found"
fi

if grep -q "\.pem" "$HOOK"; then
    pass ".pem pattern defined"
else
    fail ".pem pattern not found"
fi

if grep -q "id_rsa" "$HOOK"; then
    pass "id_rsa pattern defined"
else
    fail "id_rsa pattern not found"
fi

echo ""
echo "--- Linting integration (static) ---"

if grep -q "shellcheck" "$HOOK"; then
    pass "shellcheck integration present"
else
    fail "shellcheck integration missing"
fi

if grep -q "ruff\|flake8" "$HOOK"; then
    pass "Python linting integration present"
else
    fail "Python linting integration missing"
fi

if grep -q "jq empty" "$HOOK"; then
    pass "JSON validation present"
else
    fail "JSON validation missing"
fi

echo ""
echo "--- Exit code handling (static) ---"

if grep -q "exit 0" "$HOOK" && grep -q "exit 1" "$HOOK"; then
    pass "Proper exit codes used"
else
    fail "Missing proper exit codes"
fi

# ============================================================================
# SECTION 2: Behavioral tests (new)
# ============================================================================

mkdir -p "$TEMP_BASE"

echo ""
echo "--- Branch protection: BLOCKS commit on main ---"

setup_temp_repo "main"
# Stage a file so there's something to commit
echo "test" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks commit on main branch (exit 1)"
else
    fail "Should block commit on main branch (expected exit 1, got $exit_code)"
fi

# Verify output mentions branch protection
output=$(run_hook_in_repo_with_output || true)
if echo "$output" | grep -qi "BRANCH PROTECTION\|FORBIDDEN\|main"; then
    pass "Block message mentions branch protection"
else
    fail "Block message should mention branch protection"
fi

echo ""
echo "--- Branch protection: BLOCKS commit on master ---"

setup_temp_repo "main"
# Rename branch to master
git -C "$TEMP_REPO" branch -m main master 2>/dev/null
echo "test" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks commit on master branch (exit 1)"
else
    fail "Should block commit on master branch (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Branch name validation: BLOCKS invalid branch name ---"

setup_temp_repo "bad-branch-name"
echo "test" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks commit on invalid branch name (exit 1)"
else
    fail "Should block invalid branch name (expected exit 1, got $exit_code)"
fi

output=$(cd "$TEMP_REPO" && bash "$HOOK" 2>&1 || true)
if echo "$output" | grep -qi "INVALID BRANCH NAME\|branch-verification"; then
    pass "Block message mentions invalid branch name"
else
    fail "Block message should mention invalid branch name"
fi

echo ""
echo "--- Branch name validation: BLOCKS uppercase branch ---"

setup_temp_repo "FEAT/my-feature"
echo "test" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks uppercase branch name (exit 1)"
else
    fail "Should block uppercase branch name (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Branch name validation: ALLOWS valid feature branch ---"

setup_temp_repo "feat/test-feature"
echo "safe-content" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 0 ]]; then
    pass "Allows commit on valid feat/ branch (exit 0)"
else
    fail "Should allow commit on feat/ branch (expected exit 0, got $exit_code)"
fi

echo ""
echo "--- Branch name validation: ALLOWS various valid types ---"

for branch_type in fix docs refactor test chore experiment; do
    setup_temp_repo "$branch_type/test-thing"
    echo "content" > "$TEMP_REPO/test.txt"
    git -C "$TEMP_REPO" add test.txt

    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    exit_code=$?
    set -e

    if [[ $exit_code -eq 0 ]]; then
        pass "Allows $branch_type/ branch"
    else
        fail "Should allow $branch_type/ branch (got exit $exit_code)"
    fi
done

echo ""
echo "--- Sensitive file detection: BLOCKS .env file ---"

setup_temp_repo "feat/test-sensitive"
echo "SECRET=bad" > "$TEMP_REPO/.env"
git -C "$TEMP_REPO" add .env

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged .env file (exit 1)"
else
    fail "Should block staged .env file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS .pem file ---"

setup_temp_repo "feat/test-sensitive"
echo "PRIVATE KEY" > "$TEMP_REPO/server.pem"
git -C "$TEMP_REPO" add server.pem

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged .pem file (exit 1)"
else
    fail "Should block staged .pem file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS id_rsa ---"

setup_temp_repo "feat/test-sensitive"
echo "SSH KEY" > "$TEMP_REPO/id_rsa"
git -C "$TEMP_REPO" add id_rsa

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged id_rsa file (exit 1)"
else
    fail "Should block staged id_rsa file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS .key file ---"

setup_temp_repo "feat/test-sensitive"
echo "KEY DATA" > "$TEMP_REPO/server.key"
git -C "$TEMP_REPO" add server.key

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged .key file (exit 1)"
else
    fail "Should block staged .key file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS credentials file ---"

setup_temp_repo "feat/test-sensitive"
echo "token=xyz" > "$TEMP_REPO/credentials.json"
git -C "$TEMP_REPO" add credentials.json

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged credentials file (exit 1)"
else
    fail "Should block staged credentials file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: ALLOWS safe files ---"

setup_temp_repo "feat/test-safe"
echo "safe content" > "$TEMP_REPO/readme.md"
git -C "$TEMP_REPO" add readme.md

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 0 ]]; then
    pass "Allows commit with safe files (exit 0)"
else
    fail "Should allow safe files (expected exit 0, got $exit_code)"
fi

echo ""
echo "--- JSON validation: BLOCKS invalid JSON ---"

if command -v jq &>/dev/null; then
    setup_temp_repo "feat/test-json"
    echo "{bad json" > "$TEMP_REPO/config.json"
    git -C "$TEMP_REPO" add config.json

    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    exit_code=$?
    set -e

    if [[ $exit_code -eq 1 ]]; then
        pass "Blocks invalid JSON file (exit 1)"
    else
        fail "Should block invalid JSON (expected exit 1, got $exit_code)"
    fi

    echo ""
    echo "--- JSON validation: ALLOWS valid JSON ---"

    setup_temp_repo "feat/test-json"
    echo '{"valid": true}' > "$TEMP_REPO/config.json"
    git -C "$TEMP_REPO" add config.json

    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    exit_code=$?
    set -e

    if [[ $exit_code -eq 0 ]]; then
        pass "Allows valid JSON file (exit 0)"
    else
        fail "Should allow valid JSON (expected exit 0, got $exit_code)"
    fi
else
    echo "SKIP: jq not available for JSON validation tests"
    TESTS_PASSED=$((TESTS_PASSED + 2))
fi

echo ""
echo "--- Sensitive file detection: BLOCKS id_ed25519 ---"

setup_temp_repo "feat/test-sensitive"
echo "SSH KEY ED25519" > "$TEMP_REPO/id_ed25519"
git -C "$TEMP_REPO" add id_ed25519

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged id_ed25519 file (exit 1)"
else
    fail "Should block staged id_ed25519 file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS .secret file ---"

setup_temp_repo "feat/test-sensitive"
echo "TOP SECRET" > "$TEMP_REPO/api.secret"
git -C "$TEMP_REPO" add api.secret

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged .secret file (exit 1)"
else
    fail "Should block staged .secret file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Sensitive file detection: BLOCKS password file ---"

setup_temp_repo "feat/test-sensitive"
echo "pass=abc123" > "$TEMP_REPO/password.txt"
git -C "$TEMP_REPO" add password.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks staged password file (exit 1)"
else
    fail "Should block staged password file (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Shell linting: BLOCKS bad shell script ---"

if command -v shellcheck &>/dev/null; then
    setup_temp_repo "feat/test-shellcheck"
    # Create a script with intentional shellcheck errors (SC2086, SC2045)
    cat > "$TEMP_REPO/bad-script.sh" << 'BADSH'
#!/bin/bash
echo $UNQUOTED_VAR
for f in $(ls *.txt); do
    echo $f
done
BADSH
    git -C "$TEMP_REPO" add bad-script.sh

    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    exit_code=$?
    set -e

    if [[ $exit_code -eq 1 ]]; then
        pass "Blocks commit with shellcheck-failing .sh file (exit 1)"
    else
        fail "Should block shellcheck-failing .sh (expected exit 1, got $exit_code)"
    fi
else
    echo "SKIP: shellcheck not available for linting behavioral test"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

echo ""
echo "--- Shell linting: ALLOWS clean shell script ---"

if command -v shellcheck &>/dev/null; then
    setup_temp_repo "feat/test-shellcheck"
    cat > "$TEMP_REPO/good-script.sh" << 'GOODSH'
#!/bin/bash
set -euo pipefail
echo "hello world"
GOODSH
    git -C "$TEMP_REPO" add good-script.sh

    set +e
    (cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
    exit_code=$?
    set -e

    if [[ $exit_code -eq 0 ]]; then
        pass "Allows commit with clean shell script (exit 0)"
    else
        fail "Should allow clean shell script (expected exit 0, got $exit_code)"
    fi
else
    echo "SKIP: shellcheck not available for clean shell behavioral test"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

echo ""
echo "--- Python files: ALLOWS commit with only .py staged ---"

setup_temp_repo "feat/test-python"
cat > "$TEMP_REPO/hello.py" << 'PYEOF'
def hello():
    print("hello")
PYEOF
git -C "$TEMP_REPO" add hello.py

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 0 ]]; then
    pass "Allows commit with only .py files staged (exit 0)"
else
    fail "Should allow .py-only commit (expected exit 0, got $exit_code)"
fi

echo ""
echo "--- Settings sync: hook invokes consistency check ---"

setup_temp_repo "feat/test-settings"
echo "data" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
output=$(cd "$TEMP_REPO" && bash "$HOOK" 2>&1)
exit_code=$?
set -e

if echo "$output" | grep -qi "settings consistency\|stub settings sync"; then
    pass "Hook invokes settings consistency check"
else
    fail "Hook should invoke settings consistency check"
fi

echo ""
echo "--- Settings sync: BLOCKS commit when sync fails ---"

setup_temp_repo "feat/test-settings-fail"
echo "data" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

# Override the stub to make it fail
cat > "$TEMP_REPO/.codeflow/testing/consistency/test-settings-sync.sh" << 'FAILSTUB'
#!/usr/bin/env bash
echo "FAIL: settings out of sync"
exit 1
FAILSTUB
chmod +x "$TEMP_REPO/.codeflow/testing/consistency/test-settings-sync.sh"

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks commit when settings sync fails (exit 1)"
else
    fail "Should block when settings sync fails (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Branch protection: BLOCKS commit on production ---"

setup_temp_repo "main"
git -C "$TEMP_REPO" branch -m main production 2>/dev/null
echo "test" > "$TEMP_REPO/test.txt"
git -C "$TEMP_REPO" add test.txt

set +e
(cd "$TEMP_REPO" && bash "$HOOK") >/dev/null 2>&1
exit_code=$?
set -e

if [[ $exit_code -eq 1 ]]; then
    pass "Blocks commit on production branch (exit 1)"
else
    fail "Should block commit on production branch (expected exit 1, got $exit_code)"
fi

echo ""
echo "--- Config-driven: reads branch_types from enforcement-policy.json ---"

if grep -q "CONFIG_FILE" "$HOOK" && grep -q "branch_types" "$HOOK"; then
    pass "Hook reads branch_types from config"
else
    fail "Hook should read branch_types from config"
fi

echo ""
echo "--- Config-driven: reads protected_branches from enforcement-policy.json ---"

if grep -q "protected_branches" "$HOOK" && grep -q "DEFAULT_PROTECTED_BRANCHES" "$HOOK"; then
    pass "Hook reads protected_branches from config with fallback"
else
    fail "Hook should read protected_branches from config"
fi

echo ""
echo "--- Coverage check: validate_staged_coverage integration ---"

if grep -q "validate_staged_coverage" "$HOOK"; then
    pass "Hook calls validate_staged_coverage"
else
    fail "Hook should call validate_staged_coverage"
fi

if grep -q "COVERAGE_LIB" "$HOOK" && grep -q "test-coverage.sh" "$HOOK"; then
    pass "Hook sources test-coverage.sh library"
else
    fail "Hook should source test-coverage.sh library"
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
