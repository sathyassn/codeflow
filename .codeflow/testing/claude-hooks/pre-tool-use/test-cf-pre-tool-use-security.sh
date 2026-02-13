#!/usr/bin/env bash
# Test: cf-pre-tool-use-security.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-security.sh
#
# Comprehensive tests for security hook (modular orchestrator):
#   - Basic setup and shellcheck
#   - Tool filtering (Bash only)
#   - Dangerous commands (rm -rf /, fork bombs)
#   - Privilege escalation (sudo, su, doas, pkexec)
#   - Git protection (hook bypass, force push)
#   - Hook bypass (--no-verify, SKIP_HOOKS)
#   - Path protection (protected resources)
#   - File operations (indirect writes to protected)
#   - Branch file protection (writes on main/master)
#   - Tmp protection (managed folders)
#   - Network protection (git push/pull without bypass)
#   - Module orchestration
#   - Code quality

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

echo "=== Testing cf-pre-tool-use-security.sh ==="
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

# Test 6: Has Matcher for Bash in header
if grep -q "Matcher:" "$HOOK" && grep -q "Bash" "$HOOK"; then
    pass "Has Matcher for Bash in header"
else
    fail "Should have Matcher for Bash"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Bash tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 8: Exits 0 for non-Bash tools (Edit)
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 9: Exits 0 for non-Bash tools (Write)
result=$(TOOL_NAME="Write" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Write tool"
else
    fail "Should exit 0 for Write tool"
fi

# Test 10: Exits 0 for non-Bash tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 11: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Bash" TOOL_INPUT="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 12: Exits 0 when empty command
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty command"
else
    fail "Should exit 0 when empty command"
fi

# Test 13: Allows safe commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows safe commands (ls -la)"
else
    fail "Should allow safe commands"
fi

echo ""
echo "--- Dangerous Commands ---"

# Test 14: Blocks rm -rf /
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm -rf /"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm -rf /"
else
    fail "Should block rm -rf /"
fi

# Test 15: Blocks rm -rf /*
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm -rf /*"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm -rf /*"
else
    fail "Should block rm -rf /*"
fi

# Test 16: Blocks rm -fr /
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm -fr /"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm -fr /"
else
    fail "Should block rm -fr /"
fi

# Test 17: Allows rm on safe paths
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm /tmp/claude/test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows rm on safe paths"
else
    fail "Should allow rm on safe paths"
fi

echo ""
echo "--- Privilege Escalation ---"

# Test 18: Blocks sudo commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"sudo apt install foo"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks sudo commands"
else
    fail "Should block sudo commands"
fi

# Test 19: Blocks su commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"su - root"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks su commands"
else
    fail "Should block su commands"
fi

# Test 20: Blocks doas commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"doas cat /etc/passwd"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks doas commands"
else
    fail "Should block doas commands"
fi

# Test 21: Blocks pkexec commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"pkexec cat /etc/shadow"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks pkexec commands"
else
    fail "Should block pkexec commands"
fi

# Test 22: Allows sudo in commit messages
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"docs: explain sudo usage\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows sudo in commit messages"
else
    fail "Should allow sudo in commit messages"
fi

echo ""
echo "--- Git Hook Bypass ---"

# Test 23: Blocks git commit --no-verify
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit --no-verify -m test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks git commit --no-verify"
else
    fail "Should block git commit --no-verify"
fi

# Test 24: Blocks git commit -n
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -n -m test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks git commit -n"
else
    fail "Should block git commit -n"
fi

# Test 25: Blocks HUSKY=0 git commit
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"HUSKY=0 git commit -m test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks HUSKY=0 git commit"
else
    fail "Should block HUSKY=0 git commit"
fi

# Test 26: Blocks SKIP_HOOKS=1 git commit
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"SKIP_HOOKS=1 git commit -m test"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks SKIP_HOOKS=1 git commit"
else
    fail "Should block SKIP_HOOKS=1 git commit"
fi

# Test 27: Allows normal git commit
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git commit -m \"test commit\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows normal git commit"
else
    fail "Should allow normal git commit"
fi

echo ""
echo "--- Git Config Protection ---"

# Test 28: Blocks git config core.hooksPath
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git config core.hooksPath /dev/null"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks git config core.hooksPath"
else
    fail "Should block git config core.hooksPath"
fi

# Test 29: Allows git config user.name
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git config user.name \"Test User\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git config user.name"
else
    fail "Should allow git config user.name"
fi

# Test 30: Allows git config user.email
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"git config user.email \"test@example.com\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows git config user.email"
else
    fail "Should allow git config user.email"
fi

echo ""
echo "--- Path Protection ---"

# Test 31: Blocks rm .claude/settings.json
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm .claude/settings.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm .claude/settings.json"
else
    fail "Should block rm .claude/settings.json"
fi

# Test 32: Blocks rm -rf .claude/hooks/codeflow/
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm -rf .claude/hooks/codeflow/"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm -rf .claude/hooks/codeflow/"
else
    fail "Should block rm -rf .claude/hooks/codeflow/"
fi

# Test 33: Blocks chmod on .claude/hooks/codeflow (protected path)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"chmod 777 .claude/hooks/codeflow"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks chmod on .claude/hooks/codeflow"
else
    fail "Should block chmod on .claude/hooks/codeflow"
fi

# Test 34: Blocks mv .codeflow/config/test.json (matches .codeflow/config/**)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"mv .codeflow/config/test.json /tmp/"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks mv .codeflow/config/* (protected path)"
else
    fail "Should block mv .codeflow/config/*"
fi

# Test 35: Blocks redirect to .claude/settings.json
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"echo x > .claude/settings.json"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks redirect to .claude/settings.json"
else
    fail "Should block redirect to .claude/settings.json"
fi

# Test 36: Allows redirect to /tmp/claude/
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"echo x > /tmp/claude/test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows redirect to /tmp/claude/"
else
    fail "Should allow redirect to /tmp/claude/"
fi

echo ""
echo "--- File Operations ---"

# Test 37: Blocks cp to .claude/hooks/codeflow/
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"cp malicious.sh .claude/hooks/codeflow/test.sh"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks cp to .claude/hooks/codeflow/"
else
    fail "Should block cp to .claude/hooks/codeflow/"
fi

# Test 38: Blocks tee to protected path
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"echo x | tee .claude/settings.json"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks tee to protected path"
else
    fail "Should block tee to protected path"
fi

# Test 39: Allows cp to /tmp/claude/
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"cp file.txt /tmp/claude/"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows cp to /tmp/claude/"
else
    fail "Should allow cp to /tmp/claude/"
fi

# Test 40: Allows tee to /tmp/claude/
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"echo x | tee /tmp/claude/log"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows tee to /tmp/claude/"
else
    fail "Should allow tee to /tmp/claude/"
fi

echo ""
echo "--- Tmp Protection ---"

# Test 41: Blocks rm -rf /tmp/claude/managed
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm -rf /tmp/claude/managed"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks rm -rf /tmp/claude/managed"
else
    fail "Should block rm -rf /tmp/claude/managed"
fi

# Test 42: Blocks mv /tmp/claude/managed
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"mv /tmp/claude/managed /tmp/elsewhere"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks mv /tmp/claude/managed"
else
    fail "Should block mv /tmp/claude/managed"
fi

# Test 43: Allows operations in managed folder contents
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"touch /tmp/claude/managed/codeflow/protected-edits/file.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows operations in managed folder contents"
else
    fail "Should allow operations in managed folder contents"
fi

# Test 44: Allows rm /tmp/claude/temp.txt
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"rm /tmp/claude/temp.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows rm /tmp/claude/temp.txt"
else
    fail "Should allow rm /tmp/claude/temp.txt"
fi

echo ""
echo "--- Module Orchestration ---"

# Test 45: Sources cf-dangerous-commands.sh
if grep -q "cf-dangerous-commands.sh" "$HOOK"; then
    pass "Sources cf-dangerous-commands.sh"
else
    fail "Should source cf-dangerous-commands.sh"
fi

# Test 46: Sources cf-privilege-protection.sh
if grep -q "cf-privilege-protection.sh" "$HOOK"; then
    pass "Sources cf-privilege-protection.sh"
else
    fail "Should source cf-privilege-protection.sh"
fi

# Test 47: Sources cf-git-protection.sh
if grep -q "cf-git-protection.sh" "$HOOK"; then
    pass "Sources cf-git-protection.sh"
else
    fail "Should source cf-git-protection.sh"
fi

# Test 48: Sources cf-hook-bypass.sh
if grep -q "cf-hook-bypass.sh" "$HOOK"; then
    pass "Sources cf-hook-bypass.sh"
else
    fail "Should source cf-hook-bypass.sh"
fi

# Test 49: Sources cf-path-protection.sh
if grep -q "cf-path-protection.sh" "$HOOK"; then
    pass "Sources cf-path-protection.sh"
else
    fail "Should source cf-path-protection.sh"
fi

# Test 50: Sources cf-file-operations.sh
if grep -q "cf-file-operations.sh" "$HOOK"; then
    pass "Sources cf-file-operations.sh"
else
    fail "Should source cf-file-operations.sh"
fi

# Test 51: Sources cf-tmp-protection.sh
if grep -q "cf-tmp-protection.sh" "$HOOK"; then
    pass "Sources cf-tmp-protection.sh"
else
    fail "Should source cf-tmp-protection.sh"
fi

# Test 52: Sources cf-network-protection.sh
if grep -q "cf-network-protection.sh" "$HOOK"; then
    pass "Sources cf-network-protection.sh"
else
    fail "Should source cf-network-protection.sh"
fi

# Test 53: Sources cf-branch-file-protection.sh
if grep -q "cf-branch-file-protection.sh" "$HOOK"; then
    pass "Sources cf-branch-file-protection.sh"
else
    fail "Should source cf-branch-file-protection.sh"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 54: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 55: Loads protected paths from config
if grep -q "protected_resources" "$HOOK" || grep -q "PROTECTED_PATHS" "$HOOK"; then
    pass "Loads protected paths from config"
else
    fail "Should load protected paths from config"
fi

# Test 56: Loads managed tmp folders from config
if grep -q "MANAGED_TMP_FOLDERS" "$HOOK"; then
    pass "Loads managed tmp folders from config"
else
    fail "Should load managed tmp folders from config"
fi

# Test 57: Sources security library
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security library"
else
    fail "Should source security library"
fi

echo ""
echo "--- Code Quality ---"

# Test 58: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 59: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 60: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 61: Exports COMMAND variable
if grep -q "export COMMAND" "$HOOK"; then
    pass "Exports COMMAND variable"
else
    fail "Should export COMMAND variable"
fi

# Test 62: Documents bash 3.2+ compatibility
if grep -q "bash 3.2" "$HOOK" || grep -q "Compatibility" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash compatibility"
fi

# Test 63: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK" || grep -q "source.*enforcement" "$HOOK"; then
    pass "Has exit code 2 (via modules) for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

# Test 64: Uses jq for JSON parsing
if grep -q "command -v jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 65: Has jq fallback for input parsing
if grep -q "grep -o" "$HOOK"; then
    pass "Has jq fallback for input parsing"
else
    fail "Should have jq fallback"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
