#!/usr/bin/env bash
# Test: cf-pre-tool-use-protected-resource.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-protected-resource.sh
#
# Comprehensive tests for protected resource hook:
#   - Tool filtering (Edit|Write only)
#   - Tiered protection (critical, high, moderate)
#   - Staging area exception
#   - Config loading
#   - Block messages
#   - Security logging

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

echo "=== Testing cf-pre-tool-use-protected-resource.sh ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

# Test 1: File exists
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    skip "Shellcheck not available"
fi

# Test 4: Uses strict mode
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 5: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 6: Has Matcher for Edit|Write in header
if grep -q "Matcher:" "$HOOK" && grep -q "Edit|Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write in header"
else
    fail "Should have Matcher for Edit|Write"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Edit/Write tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 8: Exits 0 for non-Edit/Write tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 9: Exits 0 for non-Edit/Write tools (WebSearch)
result=$(TOOL_NAME="WebSearch" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for WebSearch tool"
else
    fail "Should exit 0 for WebSearch tool"
fi

# Test 10: Exits 0 for non-Edit/Write tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 11: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Edit" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 12: Exits 0 when empty file_path
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty file_path"
else
    fail "Should exit 0 when empty file_path"
fi

echo ""
echo "--- Allowed Operations (non-protected) ---"

# Test 13: Allows non-protected files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"/tmp/test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows non-protected files"
else
    fail "Should allow non-protected files"
fi

# Test 14: Allows regular project files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"src/component.ts"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows regular project files"
else
    fail "Should allow regular project files"
fi

# Test 15: Allows Write to regular files
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"src/new-file.ts"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Write to regular files"
else
    fail "Should allow Write to regular files"
fi

echo ""
echo "--- Critical Protection Tier ---"

# Test 16: Blocks Edit to .claude/settings.json
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/settings.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .claude/settings.json"
else
    fail "Should block Edit to .claude/settings.json"
fi

# Test 17: Blocks Write to .claude/settings.json
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":".claude/settings.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Write to .claude/settings.json"
else
    fail "Should block Write to .claude/settings.json"
fi

# Test 18: Blocks Edit to .claude/settings.local.json
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/settings.local.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .claude/settings.local.json"
else
    fail "Should block Edit to .claude/settings.local.json"
fi

# Test 19: Blocks Edit to .claude/CLAUDE.md
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/CLAUDE.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .claude/CLAUDE.md"
else
    fail "Should block Edit to .claude/CLAUDE.md"
fi

# Test 20: Critical block message shows CRITICAL tier
HOOK_OUTPUT=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/settings.json"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"CRITICAL"* ]]; then
    pass "Critical block message shows CRITICAL tier"
else
    fail "Should show CRITICAL tier in message"
fi

echo ""
echo "--- High Protection Tier ---"

# Test 21: Blocks Edit to .claude/hooks/codeflow files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/hooks/codeflow/pre-tool-use/test.sh"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .claude/hooks/codeflow files"
else
    fail "Should block Edit to hooks/codeflow files"
fi

# Test 22: Blocks Edit to .codeflow/config files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/config/test.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .codeflow/config files"
else
    fail "Should block Edit to .codeflow/config files"
fi

# Test 23: Blocks Edit to .codeflow/scripts/security files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/scripts/security/test.sh"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .codeflow/scripts/security files"
else
    fail "Should block Edit to security scripts"
fi

# Test 24: High block message shows HIGH tier
HOOK_OUTPUT=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/config/test.json"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"HIGH"* ]]; then
    pass "High block message shows HIGH tier"
else
    fail "Should show HIGH tier in message"
fi

# Test 24b: Blocks Edit to .codeflow/scripts/git-hooks files (NEW)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/scripts/git-hooks/pre-commit"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .codeflow/scripts/git-hooks files"
else
    fail "Should block Edit to git-hooks files"
fi

# Test 24c: Blocks Edit to .codeflow/scripts/shell-lib files (NEW)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/scripts/shell-lib/config.sh"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .codeflow/scripts/shell-lib files"
else
    fail "Should block Edit to shell-lib files"
fi

# Test 24d: Blocks Edit to .github/workflows (elevated to HIGH)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".github/workflows/ci.yml"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Edit to .github/workflows (elevated to HIGH)"
else
    fail "Should block Edit to .github/workflows (now HIGH tier)"
fi

# Test 24e: .github/workflows block message shows HIGH tier (not moderate)
HOOK_OUTPUT=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".github/workflows/ci.yml"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"HIGH"* ]]; then
    pass ".github/workflows block message shows HIGH tier"
else
    fail ".github/workflows should show HIGH tier in message"
fi

echo ""
echo "--- Moderate Protection Tier (warn but allow) ---"

# Test 25: Allows Edit to project/mission.md (with warning)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"project/mission.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit to project/mission.md"
else
    fail "Should allow Edit to project/mission.md (moderate tier)"
fi

# Test 26: project/tech-stack still in moderate tier (warn but allow)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"project/tech-stack/overview.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit to project/tech-stack (moderate tier)"
else
    fail "Should allow Edit to project/tech-stack (moderate tier)"
fi

# Test 27: Moderate tier shows warning message
HOOK_OUTPUT=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"project/mission.md"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"moderately protected"* ]] || [[ "$HOOK_OUTPUT" == *"Note"* ]]; then
    pass "Moderate tier shows warning message"
else
    fail "Should show warning for moderate tier"
fi

echo ""
echo "--- Staging Area Exception ---"

# Test 28: Allows Edit to staging area
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"/tmp/claude/managed/codeflow/protected-edits/test.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Edit to staging area"
else
    fail "Should allow Edit to staging area"
fi

# Test 29: Allows Write to staging area
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"/tmp/claude/managed/codeflow/protected-edits/test.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows Write to staging area"
else
    fail "Should allow Write to staging area"
fi

# Test 30: Has staging area constant
if grep -q "STAGING_AREA" "$HOOK" && grep -q "protected-edits" "$HOOK"; then
    pass "Has staging area constant"
else
    fail "Should have staging area constant"
fi

echo ""
echo "--- Block Messages ---"

# Test 31: Block message includes path
HOOK_OUTPUT=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/settings.json"}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"Path:"* ]]; then
    pass "Block message includes path"
else
    fail "Should include path in block message"
fi

# Test 32: Block message includes tool name
if [[ "$HOOK_OUTPUT" == *"Tool:"* ]]; then
    pass "Block message includes tool name"
else
    fail "Should include tool name in block message"
fi

# Test 33: Block message includes MUST: Delegate direction
if [[ "$HOOK_OUTPUT" == *"MUST:"* ]] && [[ "$HOOK_OUTPUT" == *"Delegate"* ]]; then
    pass "Block message includes MUST: Delegate direction"
else
    fail "Should include MUST: Delegate direction"
fi

# Test 34: Block message mentions cf-security teammate
if [[ "$HOOK_OUTPUT" == *"cf-security"* ]]; then
    pass "Block message mentions cf-security teammate"
else
    fail "Should mention cf-security teammate"
fi

# Test 35: Block message suggests handle-protected-resource
if [[ "$HOOK_OUTPUT" == *"handle-protected-resource"* ]]; then
    pass "Block message suggests handle-protected-resource"
else
    fail "Should suggest handle-protected-resource"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 36: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 37: Reads protected_resources from config
if grep -q "protected_resources" "$HOOK"; then
    pass "Reads protected_resources from config"
else
    fail "Should read protected_resources from config"
fi

# Test 38: Has default CRITICAL_PATHS
if grep -q "CRITICAL_PATHS" "$HOOK"; then
    pass "Has default CRITICAL_PATHS"
else
    fail "Should have default CRITICAL_PATHS"
fi

# Test 39: Has default HIGH_PATHS
if grep -q "HIGH_PATHS" "$HOOK"; then
    pass "Has default HIGH_PATHS"
else
    fail "Should have default HIGH_PATHS"
fi

# Test 40: Has default MODERATE_PATHS
if grep -q "MODERATE_PATHS" "$HOOK"; then
    pass "Has default MODERATE_PATHS"
else
    fail "Should have default MODERATE_PATHS"
fi

# Test 41: Loads config arrays with while-read pattern
if grep -q 'while IFS= read -r' "$HOOK"; then
    pass "Loads config arrays with bash 3.2+ compatible pattern"
else
    fail "Should use while-read for config loading"
fi

echo ""
echo "--- Pattern Matching ---"

# Test 42: Has matches_pattern function
if grep -q "matches_pattern()" "$HOOK"; then
    pass "Has matches_pattern function"
else
    fail "Should have matches_pattern function"
fi

# Test 43: Has check_tier function
if grep -q "check_tier()" "$HOOK"; then
    pass "Has check_tier function"
else
    fail "Should have check_tier function"
fi

# Test 44: Supports glob patterns with wildcard
if grep -q '\*' "$HOOK" && grep -q "regex" "$HOOK"; then
    pass "Supports glob patterns with wildcard"
else
    fail "Should support glob patterns"
fi

# Test 44b: ** glob matches deeply nested files (fixed bug: ** was becoming .*.*)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".claude/hooks/codeflow/pre-tool-use/deeply/nested/test.sh"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]]; then
    pass "** glob matches deeply nested paths"
else
    fail "** glob should match deeply nested paths (was broken: ** became .*.*)"
fi

# Test 44c: ** glob matches single-level files
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":".codeflow/scripts/git-hooks/pre-commit"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]]; then
    pass "** glob matches single-level files under protected dir"
else
    fail "** glob should match single-level files under protected dir"
fi

# Test 44d: Sources matches_extended_glob or has correct fallback
if grep -q "matches_extended_glob" "$HOOK"; then
    pass "Uses matches_extended_glob for proper ** handling"
else
    fail "Should use matches_extended_glob for ** handling"
fi

echo ""
echo "--- Security Logging ---"

# Test 45: Has protection logging
if grep -q "log_protection" "$HOOK"; then
    pass "Has protection logging"
else
    fail "Should have protection logging"
fi

# Test 46: Logs blocked events
if grep -q 'log_protection.*blocked' "$HOOK"; then
    pass "Logs blocked events"
else
    fail "Should log blocked events"
fi

# Test 47: Logs staging area access
if grep -q 'staging_area_access' "$HOOK" || grep -q 'log_protection.*staging' "$HOOK"; then
    pass "Logs staging area access"
else
    fail "Should log staging area access"
fi

# Test 48: Sources security library
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security library"
else
    fail "Should source security library"
fi

echo ""
echo "--- Code Quality ---"

# Test 49: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 50: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 51: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 52: Has jq fallback for file_path extraction
if grep -q "grep -o" "$HOOK" && grep -q "file_path" "$HOOK"; then
    pass "Has jq fallback for file_path extraction"
else
    fail "Should have jq fallback"
fi

# Test 53: Documents bash 3.2+ compatibility
if grep -q "bash 3.2" "$HOOK" || grep -q "Compatibility" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash compatibility"
fi

# Test 54: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK"; then
    pass "Has exit code 2 for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

# Test 55: Normalizes paths to relative
if grep -q 'FILE_PATH=.*REPO_ROOT' "$HOOK"; then
    pass "Normalizes paths to relative"
else
    fail "Should normalize paths"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
