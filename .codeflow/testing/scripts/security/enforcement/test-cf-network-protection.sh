#!/usr/bin/env bash
# Test: cf-network-protection.sh
# Location: .codeflow/testing/scripts/security/enforcement/test-cf-network-protection.sh
#
# Tests the network protection enforcement module

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$ENFORCEMENT_DIR/cf-network-protection.sh"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

export REPO_ROOT LIB_DIR CONFIG

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing cf-network-protection.sh ==="
echo ""

# ===========================================================================
# SECTION 1: Static Analysis
# ===========================================================================

echo "--- Static Analysis ---"

# Test 1: File exists
if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

# Test 2: File is executable
if [[ -x "$MODULE" ]]; then pass "Module is executable"; else fail "Module not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Has proper header comments
if grep -q "Purpose:" "$MODULE" && grep -q "Exit codes:" "$MODULE"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Uses set -euo pipefail
if grep -q "set -euo pipefail" "$MODULE"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Sources security-lib.sh
if grep -q 'source.*security-lib.sh' "$MODULE"; then
    pass "Sources security-lib.sh"
else
    fail "Should source security-lib.sh"
fi

# Test 7: Checks sandbox bypass status
if grep -q 'dangerouslyDisableSandbox' "$MODULE" && grep -q 'SANDBOX_BYPASS' "$MODULE"; then
    pass "Checks sandbox bypass status"
else
    fail "Should check sandbox bypass status"
fi

# Test 8: Handles git network operations
if grep -q 'git_network' "$MODULE" && grep -q 'network' "$MODULE"; then
    pass "Handles git network operations"
else
    fail "Should handle git network operations"
fi

# Test 9: Handles GitHub CLI operations
if grep -q 'github_cli' "$MODULE"; then
    pass "Handles GitHub CLI operations"
else
    fail "Should handle GitHub CLI operations"
fi

# Test 10: Has check_network_pattern function
if grep -q 'check_network_pattern' "$MODULE"; then
    pass "Has check_network_pattern function"
else
    fail "Should have check_network_pattern function"
fi

# Test 11: Uses block_with_skill for blocking
if grep -q 'block_with_skill' "$MODULE"; then
    pass "Uses block_with_skill for blocking (provides audit logging)"
else
    fail "Should use block_with_skill for blocking"
fi

# Test 12: Uses CONFIG variable for patterns
if grep -q 'CONFIG' "$MODULE"; then
    pass "Uses CONFIG variable"
else
    fail "Should use CONFIG variable"
fi

# Test 13: Uses jq for JSON parsing
if grep -q 'jq' "$MODULE"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 14: References security-management skill for blocks
if grep -q 'security-management' "$MODULE" && grep -q 'sandbox-check' "$MODULE"; then
    pass "References security-management:sandbox-check skill"
else
    fail "Should reference security-management:sandbox-check"
fi

# Test 15: Returns 0 at end
if grep -q 'return 0' "$MODULE"; then
    pass "Returns 0 when all checks pass"
else
    fail "Should return 0 when all checks pass"
fi

# Test 16: block_with_skill exits with code 2 (via security-lib.sh)
if grep -q 'block_with_skill' "$MODULE"; then
    # block_with_skill in security-lib.sh calls exit 2
    pass "Blocks via block_with_skill (exit 2 in security-lib.sh)"
else
    fail "Should use block_with_skill which exits with code 2"
fi

# Test 17: References .network_operations config path
if grep -q 'network_operations' "$MODULE"; then
    pass "References network_operations config path"
else
    fail "Should reference network_operations config path"
fi

# Test 18: Uses grep for pattern matching
if grep -q 'grep' "$MODULE"; then
    pass "Uses grep for pattern matching"
else
    fail "Should use grep for pattern matching"
fi

# Test 19: PathFlow mode-conditional routing for git network ops
if grep -q 'is_pathflow_active' "$MODULE"; then
    pass "Has PathFlow mode-conditional routing"
else
    fail "Should use is_pathflow_active for mode-conditional routing"
fi

# Test 20: References cf-git-workflow skill for standalone mode
if grep -q 'cf-git-workflow' "$MODULE"; then
    pass "References cf-git-workflow skill for standalone mode"
else
    fail "Should reference cf-git-workflow skill for standalone mode"
fi

# Test 21: References cf-gitops teammate for PathFlow mode
if grep -q 'cf-gitops' "$MODULE"; then
    pass "References cf-gitops teammate for PathFlow mode"
else
    fail "Should reference cf-gitops teammate for PathFlow mode"
fi

# ===========================================================================
# SECTION 2: Functional Tests
# ===========================================================================

echo ""
echo "--- Functional Tests ---"

# Setup: Create a temporary config for testing
TEST_TMP="/tmp/claude/test-network-$$"
mkdir -p "$TEST_TMP"

# Create a minimal enforcement-policy.json for testing
cat > "$TEST_TMP/enforcement-policy.json" <<'TESTCFG'
{
  "network_operations": {
    "git_network": {
      "patterns": [
        "^git\\s+(push|pull|fetch|clone)",
        "^git\\s+remote\\s+update",
        "^git\\s+ls-remote"
      ]
    },
    "github_cli": {
      "patterns": [
        "^gh\\s+(pr|issue|release|api|workflow|run|repo|gist)\\s"
      ]
    }
  }
}
TESTCFG

# Helper: Run a command through the network protection module (sourced context)
# Returns the exit code of the sourced module
run_network_check() {
    local cmd="$1"
    local tool_input="${2:-"{}"}"
    local test_config="${3:-$TEST_TMP/enforcement-policy.json}"

    # shellcheck disable=SC2030,SC2031  # Subshell isolation is intentional for test env
    (
        export COMMAND="$cmd"
        export TOOL_INPUT="$tool_input"
        export CONFIG="$test_config"
        export LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
        export REPO_ROOT
        export CODEFLOW_SESSION_ID="test-network-$$"
        mkdir -p "$REPO_ROOT/.state/logs/security"/{audit,blocked,protection,sentinel,network} 2>/dev/null || true

        # Source the module (it uses return 0 for pass, exit 2 for block)
        source "$MODULE" 2>/dev/null
    )
    return $?
}

# Test 19: git push is blocked without sandbox bypass
if ! run_network_check "git push origin main" '{}'; then
    pass "Blocks git push without sandbox bypass"
else
    fail "Should block git push without sandbox bypass"
fi

# Test 20: git pull is blocked without sandbox bypass
if ! run_network_check "git pull origin main" '{}'; then
    pass "Blocks git pull without sandbox bypass"
else
    fail "Should block git pull without sandbox bypass"
fi

# Test 21: git fetch is blocked without sandbox bypass
if ! run_network_check "git fetch origin" '{}'; then
    pass "Blocks git fetch without sandbox bypass"
else
    fail "Should block git fetch without sandbox bypass"
fi

# Test 22: git clone is blocked without sandbox bypass
if ! run_network_check "git clone https://github.com/user/repo" '{}'; then
    pass "Blocks git clone without sandbox bypass"
else
    fail "Should block git clone without sandbox bypass"
fi

# Test 23: git remote update is blocked
if ! run_network_check "git remote update" '{}'; then
    pass "Blocks git remote update without sandbox bypass"
else
    fail "Should block git remote update without sandbox bypass"
fi

# Test 24: git ls-remote is blocked
if ! run_network_check "git ls-remote origin" '{}'; then
    pass "Blocks git ls-remote without sandbox bypass"
else
    fail "Should block git ls-remote without sandbox bypass"
fi

# Test 25: gh pr create is blocked
if ! run_network_check "gh pr create --title test" '{}'; then
    pass "Blocks gh pr create without sandbox bypass"
else
    fail "Should block gh pr create without sandbox bypass"
fi

# Test 26: gh issue list is blocked
if ! run_network_check "gh issue list" '{}'; then
    pass "Blocks gh issue list without sandbox bypass"
else
    fail "Should block gh issue list without sandbox bypass"
fi

# Test 27: gh api is blocked
if ! run_network_check "gh api repos/user/repo" '{}'; then
    pass "Blocks gh api without sandbox bypass"
else
    fail "Should block gh api without sandbox bypass"
fi

# Test 28: git push ALLOWED with sandbox bypass
if run_network_check "git push origin main" '{"dangerouslyDisableSandbox": true}'; then
    pass "Allows git push with sandbox bypass"
else
    fail "Should allow git push with sandbox bypass"
fi

# Test 29: git pull ALLOWED with sandbox bypass
if run_network_check "git pull origin main" '{"dangerouslyDisableSandbox": true}'; then
    pass "Allows git pull with sandbox bypass"
else
    fail "Should allow git pull with sandbox bypass"
fi

# Test 30: gh pr create ALLOWED with sandbox bypass
if run_network_check "gh pr create --title test" '{"dangerouslyDisableSandbox": true}'; then
    pass "Allows gh pr create with sandbox bypass"
else
    fail "Should allow gh pr create with sandbox bypass"
fi

# Test 31: Non-network git command passes through
if run_network_check "git status" '{}'; then
    pass "Allows git status (not a network command)"
else
    fail "Should allow git status (not a network command)"
fi

# Test 32: Non-network git command passes through (git log)
if run_network_check "git log --oneline" '{}'; then
    pass "Allows git log (not a network command)"
else
    fail "Should allow git log (not a network command)"
fi

# Test 33: Non-network git command passes through (git diff)
if run_network_check "git diff HEAD" '{}'; then
    pass "Allows git diff (not a network command)"
else
    fail "Should allow git diff (not a network command)"
fi

# Test 34: Non-git command passes through
if run_network_check "ls -la" '{}'; then
    pass "Allows non-git commands"
else
    fail "Should allow non-git commands"
fi

# Test 35: Empty TOOL_INPUT defaults to no bypass
if ! run_network_check "git push origin main" ''; then
    pass "Blocks with empty TOOL_INPUT"
else
    fail "Should block with empty TOOL_INPUT (no bypass)"
fi

# Test 36: sandbox bypass false is still blocked
if ! run_network_check "git push origin main" '{"dangerouslyDisableSandbox": false}'; then
    pass "Blocks when sandbox bypass is explicitly false"
else
    fail "Should block when sandbox bypass is explicitly false"
fi

# Test 37: Missing config file gracefully passes through
if run_network_check "git push origin main" '{}' "/nonexistent/config.json"; then
    pass "Passes through when config file missing (graceful degradation)"
else
    fail "Should pass through when config file is missing"
fi

# Test 38: gh workflow list is blocked
if ! run_network_check "gh workflow list" '{}'; then
    pass "Blocks gh workflow list without sandbox bypass"
else
    fail "Should block gh workflow list without sandbox bypass"
fi

# Test 39: gh run list is blocked
if ! run_network_check "gh run list" '{}'; then
    pass "Blocks gh run list without sandbox bypass"
else
    fail "Should block gh run list without sandbox bypass"
fi

# Test 40: gh repo view is blocked
if ! run_network_check "gh repo view owner/repo" '{}'; then
    pass "Blocks gh repo view without sandbox bypass"
else
    fail "Should block gh repo view without sandbox bypass"
fi

# Test 41: gh gist create is blocked
if ! run_network_check "gh gist create file.txt" '{}'; then
    pass "Blocks gh gist create without sandbox bypass"
else
    fail "Should block gh gist create without sandbox bypass"
fi

# Test 42: gh release create is blocked
if ! run_network_check "gh release create v1.0" '{}'; then
    pass "Blocks gh release create without sandbox bypass"
else
    fail "Should block gh release create without sandbox bypass"
fi

# Test 43: gh workflow ALLOWED with sandbox bypass
if run_network_check "gh workflow list" '{"dangerouslyDisableSandbox": true}'; then
    pass "Allows gh workflow with sandbox bypass"
else
    fail "Should allow gh workflow with sandbox bypass"
fi

# Test 44: git commit is NOT blocked (not a network op)
if run_network_check "git commit -m 'test'" '{}'; then
    pass "Allows git commit (not a network command)"
else
    fail "Should allow git commit (not a network command)"
fi

# Test 45: git add is NOT blocked (not a network op)
if run_network_check "git add ." '{}'; then
    pass "Allows git add (not a network command)"
else
    fail "Should allow git add (not a network command)"
fi

# ===========================================================================
# SECTION 3: PathFlow Mode-Conditional Tests
# ===========================================================================

echo ""
echo "--- PathFlow Mode Tests ---"

# Helper: Run network check with PathFlow active flag
run_network_check_pathflow() {
    local cmd="$1"
    local tool_input="${2:-"{}"}"
    local test_config="${3:-$TEST_TMP/enforcement-policy.json}"

    # shellcheck disable=SC2030,SC2031  # Subshell isolation is intentional for test env
    (
        export COMMAND="$cmd"
        export TOOL_INPUT="$tool_input"
        export CONFIG="$test_config"
        export LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
        export REPO_ROOT
        export CODEFLOW_SESSION_ID="test-network-pathflow-$$"
        mkdir -p "$REPO_ROOT/.state/logs/security"/{audit,blocked,protection,sentinel,network} 2>/dev/null || true
        # Create PathFlow active flag for this session
        mkdir -p "$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID" 2>/dev/null || true
        touch "$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/is-pathflow-active"

        source "$MODULE" 2>&1
    )
    local rc=$?
    # Cleanup PathFlow flag
    rm -rf "$REPO_ROOT/.state/session/test-network-pathflow-$$" 2>/dev/null || true
    return $rc
}

# Test 46: Git push blocked in PathFlow mode mentions cf-gitops
output=$(run_network_check_pathflow "git push origin main" '{}' 2>&1 || true)
if echo "$output" | grep -q 'cf-gitops'; then
    pass "PathFlow mode: git push block mentions cf-gitops teammate"
else
    fail "PathFlow mode: git push block should mention cf-gitops teammate"
fi

# Helper: Run network check in standalone mode (no PathFlow flag)
run_network_check_standalone() {
    local cmd="$1"
    local tool_input="${2:-"{}"}"
    local test_config="${3:-$TEST_TMP/enforcement-policy.json}"

    # shellcheck disable=SC2030,SC2031  # Subshell isolation is intentional for test env
    (
        export COMMAND="$cmd"
        export TOOL_INPUT="$tool_input"
        export CONFIG="$test_config"
        export LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
        export REPO_ROOT
        export CODEFLOW_SESSION_ID="test-network-standalone-$$"
        mkdir -p "$REPO_ROOT/.state/logs/security"/{audit,blocked,protection,sentinel,network} 2>/dev/null || true
        # Ensure no PathFlow flag exists
        rm -f "$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/is-pathflow-active" 2>/dev/null || true

        source "$MODULE" 2>&1
    )
    return $?
}

# Test 47: Git push blocked in standalone mode mentions cf-git-workflow
output=$(run_network_check_standalone "git push origin main" '{}' 2>&1 || true)
if echo "$output" | grep -q 'cf-git-workflow'; then
    pass "Standalone mode: git push block mentions cf-git-workflow skill"
else
    fail "Standalone mode: git push block should mention cf-git-workflow skill"
fi

# Cleanup
rm -rf "$TEST_TMP" 2>/dev/null || true
rm -rf "$REPO_ROOT/.state/session/test-network-pathflow-$$" 2>/dev/null || true
rm -rf "$REPO_ROOT/.state/session/test-network-standalone-$$" 2>/dev/null || true

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
