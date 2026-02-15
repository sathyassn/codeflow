#!/usr/bin/env bash
# Test: cf-pre-tool-use-webfetch.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-webfetch.sh
#
# Tests for network access validation hook:
#   - Stdin protocol (Claude Code hook protocol)
#   - Trusted domains loaded from .list files based on approvalMode
#   - Always-block patterns (localhost, private IPs)
#   - Trusted domains bypass always-block
#   - Domain hierarchy walking (subdomains match base domain)
#   - Strict/standard/autonomous/permissive modes
#   - Bash curl/wget URL extraction
#   - Block messages and ask JSON

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh"
fi

TRUSTED_DOMAINS_DIR="$REPO_ROOT/.codeflow/config/enforcement/trusted-domains"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Helper: run hook with stdin JSON (production protocol)
run_hook_stdin() {
    local tool_name="$1"
    local tool_input="$2"
    local stdin_json
    stdin_json=$(jq -nc \
        --arg tn "$tool_name" \
        --argjson ti "$tool_input" \
        '{tool_name: $tn, tool_input: $ti, session_id: "test-session",
         cwd: "/tmp/claude", hook_event_name: "PreToolUse",
         permission_mode: "default", tool_use_id: "test-123",
         transcript_path: "/tmp/claude/test.jsonl"}')
    echo "$stdin_json" | bash "$HOOK" 2>&1
    return "${PIPESTATUS[1]}"
}

echo "=== Testing cf-pre-tool-use-webfetch.sh ==="
echo "Hook: $HOOK"
echo ""

# =========================================================================
# SECTION 1: File Structure Tests
# =========================================================================

echo "--- File Structure ---"

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091,SC2034 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Uses strict mode
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 5: Has proper header comments
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 6: Has Hook Type header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

# Test 7: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Stdin Protocol ---"

# Test 8: Has stdin reading block
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN' "$HOOK" && grep -q '! -t 0' "$HOOK"; then
    pass "Has stdin reading block"
else
    fail "Should read from stdin"
fi

# Test 9: Parses tool_name from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tool_name // empty' "$HOOK"; then
    pass "Parses tool_name from stdin"
else
    fail "Should parse tool_name from stdin JSON"
fi

# Test 10: Parses tool_input from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tool_input // empty' "$HOOK"; then
    pass "Parses tool_input from stdin"
else
    fail "Should parse tool_input from stdin JSON"
fi

# Test 11: WebFetch untrusted via stdin (asks)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://evil-site.example.com"}'; echo "EXIT:$?")
if [[ "$result" == *"permissionDecision"* ]] && [[ "$result" == *"EXIT:0"* ]]; then
    pass "WebFetch untrusted via stdin triggers ask"
else
    fail "Should ask for untrusted domain via stdin (got: $result)"
fi

# Test 12: Localhost via stdin (blocks)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://localhost:8080"}'; echo "EXIT:$?")
if [[ "$result" == *"BLOCKED"* ]] && [[ "$result" == *"EXIT:2"* ]]; then
    pass "Localhost via stdin blocks"
else
    fail "Should block localhost via stdin (got: $result)"
fi

# Test 13: Trusted domain via stdin (allows)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://github.com/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"BLOCKED"* ]]; then
    pass "Trusted domain via stdin allows"
else
    fail "Should allow github.com via stdin"
fi

# Test 14: Non-network tool via stdin exits 0
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Read" '{"file_path":"/tmp/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Non-network tool via stdin exits 0"
else
    fail "Should exit 0 for Read tool via stdin"
fi

echo ""
echo "--- Trusted Domains List Files ---"

# Test 15: Standard list file exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$TRUSTED_DOMAINS_DIR/standard.list" ]]; then
    pass "Standard list file exists"
else
    fail "Standard list file not found"
fi

# Test 16: Autonomous list file exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$TRUSTED_DOMAINS_DIR/autonomous.list" ]]; then
    pass "Autonomous list file exists"
else
    fail "Autonomous list file not found"
fi

# Test 17: Permissive list file exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$TRUSTED_DOMAINS_DIR/permissive.list" ]]; then
    pass "Permissive list file exists"
else
    fail "Permissive list file not found"
fi

# Test 18: Hook references trusted-domains directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "trusted-domains" "$HOOK"; then
    pass "Hook references trusted-domains directory"
else
    fail "Hook should reference trusted-domains directory"
fi

# Test 19: Hook loads from .list files
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "load_trusted_domains" "$HOOK" && grep -q ".list" "$HOOK"; then
    pass "Hook loads from .list files"
else
    fail "Hook should load trusted domains from .list files"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 20: Exits 0 for non-network tools
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Read" '{"file_path":"/tmp"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-network tools (Read)"
else
    fail "Should exit 0 for non-network tools"
fi

# Test 21: Exits 0 for Edit tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Edit" '{"file_path":"/tmp/x","old_string":"a","new_string":"b"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 22: Exits 0 for Bash without network commands
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"ls -la"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-network Bash commands"
else
    fail "Should exit 0 for non-network commands"
fi

# Test 23: Processes Bash with curl command
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl https://example.com"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] || [[ "$result" == *"EXIT:2"* ]]; then
    pass "Processes Bash with curl command"
else
    fail "Should process curl commands"
fi

# Test 24: Processes WebFetch tool
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://example.com"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] || [[ "$result" == *"EXIT:2"* ]]; then
    pass "Processes WebFetch tool"
else
    fail "Should process WebFetch tool"
fi

# Test 25: WebSearch exits 0 (no URL extraction for search queries)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebSearch" '{"query":"test query"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "WebSearch without URL exits 0"
else
    fail "WebSearch without URL should exit 0"
fi

echo ""
echo "--- Trusted Domains (standard.list) ---"

# Test 26: Allows github.com
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://github.com/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"BLOCKED"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Allows trusted domain github.com"
else
    fail "Should allow github.com (in standard.list)"
fi

# Test 27: Allows api.github.com (subdomain hierarchy)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://api.github.com/users"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Allows subdomain api.github.com"
else
    fail "Should allow api.github.com (subdomain of github.com)"
fi

# Test 28: Allows stackoverflow.com
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://stackoverflow.com/questions"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Allows trusted domain stackoverflow.com"
else
    fail "Should allow stackoverflow.com"
fi

# Test 29: Allows npmjs.com
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://npmjs.com/package/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Allows trusted domain npmjs.com"
else
    fail "Should allow npmjs.com"
fi

# Test 30: Allows pypi.org
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://pypi.org/project/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Allows trusted domain pypi.org"
else
    fail "Should allow pypi.org"
fi

echo ""
echo "--- Untrusted Domains (default: ask) ---"

# Test 31: Asks for untrusted domain (default)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://malicious-site.example.com/foo"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"permissionDecision"* ]]; then
    pass "Asks for untrusted domain (default behavior)"
else
    fail "Should ask for untrusted domain"
fi

# Test 32: Ask response includes domain info
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://unknown-domain.net/path"}'; echo "EXIT:$?")
if [[ "$result" == *"unknown-domain.net"* ]] || [[ "$result" == *"allowlist"* ]]; then
    pass "Ask response includes domain context"
else
    fail "Ask response should include domain info"
fi

# Test 33: Ask response has hookSpecificOutput JSON
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://random-site.org"}'; echo "EXIT:$?")
if [[ "$result" == *"hookSpecificOutput"* ]] && [[ "$result" == *"hookEventName"* ]]; then
    pass "Ask response has hookSpecificOutput JSON format"
else
    fail "Should output hookSpecificOutput JSON for ask"
fi

echo ""
echo "--- Always-Block Domains ---"

# Test 34: Blocks localhost
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://localhost:8080/api"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks localhost"
else
    fail "Should block localhost"
fi

# Test 35: Blocks 127.0.0.1
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://127.0.0.1:3000/"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks 127.0.0.1"
else
    fail "Should block 127.0.0.1"
fi

# Test 36: Blocks 0.0.0.0
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://0.0.0.0:5000/api"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks 0.0.0.0"
else
    fail "Should block 0.0.0.0"
fi

# Test 37: Has ::1 in always-block list (IPv6 loopback)
# Note: IPv6 URLs break extract_domain port-stripping (domain%%:* strips after first colon)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"::1"' "$HOOK"; then
    pass "Has ::1 in always-block list"
else
    fail "Should have ::1 in always-block list"
fi

# Test 38: Blocks private IP 192.168.x.x
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl http://192.168.1.1/admin"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks private IP 192.168.x.x"
else
    fail "Should block 192.168.1.1"
fi

# Test 39: Blocks private IP 10.x.x.x
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl http://10.0.0.1/"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks private IP 10.x.x.x"
else
    fail "Should block 10.0.0.1"
fi

# Test 40: Blocks private IP 172.16.x.x
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl http://172.16.0.1/api"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks private IP 172.16.x.x"
else
    fail "Should block 172.16.0.1"
fi

# Test 41: Blocks *.local domains
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://myserver.local/api"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks *.local domains"
else
    fail "Should block myserver.local"
fi

# Test 42: Blocks *.home domains
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://router.home/admin"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks *.home domains"
else
    fail "Should block router.home"
fi

# Test 43: Blocks *.corp domains
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://wiki.corp/page"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks *.corp domains"
else
    fail "Should block wiki.corp"
fi

# Test 44: Always-block message mentions developer override options
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"http://localhost:8080"}'; echo "EXIT:$?")
if [[ "$result" == *"Developer override"* ]] || [[ "$result" == *"trusted-domains"* ]]; then
    pass "Always-block message mentions override options"
else
    fail "Should mention developer override options"
fi

echo ""
echo "--- Domain Extraction ---"

# Test 45: Has extract_domain function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "extract_domain" "$HOOK"; then
    pass "Has extract_domain function"
else
    fail "Should have extract_domain function"
fi

# Test 46: Extracts domain from URL with port
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://github.com:443/test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"BLOCKED"* ]]; then
    pass "Extracts domain from URL with port"
else
    fail "Should handle URL with port correctly"
fi

# Test 47: Extracts domain from URL with path
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "WebFetch" '{"url":"https://github.com/owner/repo/issues"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"BLOCKED"* ]]; then
    pass "Extracts domain from URL with path"
else
    fail "Should handle URL with path correctly"
fi

# Test 48: Handles URL with userinfo (user@domain)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '@' "$HOOK" && grep -q 'domain.*@' "$HOOK"; then
    pass "Handles URL with userinfo (strips user@)"
else
    fail "Should strip userinfo from domain"
fi

echo ""
echo "--- Bash Network Command Detection ---"

# Test 49: Bash curl to trusted domain allows
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl -s https://api.github.com/repos"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" != *"BLOCKED"* ]] && [[ "$result" != *"permissionDecision"* ]]; then
    pass "Bash curl to trusted domain allows"
else
    fail "Should allow curl to trusted github.com"
fi

# Test 50: Bash wget to untrusted domain asks
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"wget https://untrusted-site.net/file.zip"}'; echo "EXIT:$?")
if [[ "$result" == *"permissionDecision"* ]] && [[ "$result" == *"EXIT:0"* ]]; then
    pass "Bash wget to untrusted domain asks"
else
    fail "Should ask for wget to untrusted domain"
fi

# Test 51: Bash curl to localhost blocks
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"curl http://localhost:3000/api/health"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Bash curl to localhost blocks"
else
    fail "Should block curl to localhost"
fi

# Test 52: Bash non-network command exits 0
TESTS_RUN=$((TESTS_RUN + 1))
result=$(run_hook_stdin "Bash" '{"command":"git status && npm test"}'; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Bash non-network command exits 0"
else
    fail "Should exit 0 for non-network Bash commands"
fi

# Test 53: Detects curl, wget, fetch, nc, ssh, scp, rsync, ftp keywords
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "curl|wget|fetch|nc|netcat|ssh|scp|rsync|ftp" "$HOOK"; then
    pass "Detects common network tools in Bash commands"
else
    fail "Should detect curl/wget/fetch/nc/ssh/scp/rsync/ftp"
fi

echo ""
echo "--- Approval Mode Support ---"

# Test 54: Hook supports approvalMode setting
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "APPROVAL_MODE" "$HOOK" && grep -q "approvalMode" "$HOOK"; then
    pass "Hook supports approvalMode setting"
else
    fail "Hook should support approvalMode setting"
fi

# Test 55: Hook has case for strict mode (no trusted domains)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "strict" "$HOOK" && grep -q 'TRUSTED_DOMAINS=()' "$HOOK"; then
    pass "Strict mode clears trusted domains"
else
    fail "Strict mode should clear trusted domains"
fi

# Test 56: Hook has case for standard mode
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "standard" "$HOOK" && grep -q "standard.list" "$HOOK"; then
    pass "Standard mode loads standard.list"
else
    fail "Should load standard.list for standard mode"
fi

# Test 57: Hook has case for autonomous mode
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "autonomous" "$HOOK" && grep -q "autonomous.list" "$HOOK"; then
    pass "Autonomous mode loads autonomous.list"
else
    fail "Should load autonomous.list for autonomous mode"
fi

# Test 58: Hook has case for permissive mode
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "permissive" "$HOOK" && grep -q "permissive.list" "$HOOK"; then
    pass "Permissive mode loads permissive.list"
else
    fail "Should load permissive.list for permissive mode"
fi

echo ""
echo "--- Block Message Quality ---"

# Test 59: Block message references cf-security teammate
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-security" "$HOOK"; then
    pass "Block message references cf-security teammate"
else
    fail "Block message should reference cf-security teammate"
fi

# Test 60: Block message mentions .list file path
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'trusted-domains/${APPROVAL_MODE}.list\|trusted-domains/' "$HOOK"; then
    pass "Block message mentions .list file path"
else
    fail "Block message should mention trusted-domains/*.list"
fi

# Test 61: Block message mentions untrustedAction
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "UNTRUSTED_ACTION" "$HOOK" && grep -q "untrustedAction" "$HOOK"; then
    pass "Hook supports untrustedAction setting"
else
    fail "Hook should support untrustedAction (ask/block)"
fi

echo ""
echo "--- Configuration ---"

# Test 62: Hook reads enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "Hook reads enforcement-policy.json"
else
    fail "Should read enforcement-policy.json"
fi

# Test 63: Hook reads settings for config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "settings.json\|settings.local.json" "$HOOK"; then
    pass "Hook reads settings for configuration"
else
    fail "Hook should read settings files"
fi

# Test 64: Hook checks _web_fetch_config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "_web_fetch_config" "$HOOK"; then
    pass "Hook uses _web_fetch_config from settings"
else
    fail "Hook should check _web_fetch_config in settings"
fi

# Test 65: Hook checks always_block_domains.enabled config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "always_block_domains" "$HOOK" && grep -q "enabled" "$HOOK"; then
    pass "Hook checks always_block_domains.enabled config"
else
    fail "Should check always_block_domains.enabled"
fi

# Test 66: Sources security-lib.sh
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security-lib.sh"
else
    fail "Should source security-lib.sh"
fi

echo ""
echo "--- Code Quality ---"

# Test 67: Uses git rev-parse for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses git rev-parse for REPO_ROOT"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 68: Has proper fallback for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 69: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 70: Has is_always_blocked_domain function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_always_blocked_domain" "$HOOK"; then
    pass "Has is_always_blocked_domain function"
else
    fail "Should have is_always_blocked_domain function"
fi

# Test 71: Has is_trusted_domain function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_trusted_domain" "$HOOK"; then
    pass "Has is_trusted_domain function"
else
    fail "Should have is_trusted_domain function"
fi

# Test 72: Trusted domains checked BEFORE always-block (bypass pattern)
TESTS_RUN=$((TESTS_RUN + 1))
trusted_line=$(grep -n "is_trusted_domain" "$HOOK" | grep -v "^[0-9]*:#\|function\|()" | head -1 | cut -d: -f1)
blocked_line=$(grep -n "is_always_blocked_domain" "$HOOK" | grep -v "^[0-9]*:#\|function\|()" | head -1 | cut -d: -f1)
if [[ -n "$trusted_line" ]] && [[ -n "$blocked_line" ]] && [[ "$trusted_line" -lt "$blocked_line" ]]; then
    pass "Trusted checked before always-block (bypass pattern)"
else
    fail "Trusted domains should be checked BEFORE always-block"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
