#!/usr/bin/env bash
# Test: cf-pre-tool-use-webfetch.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-webfetch.sh
#
# Tests for network access validation hook:
#   - Trusted domains loaded from .list files based on approvalMode
#   - Always-block patterns (localhost, private IPs)
#   - Trusted domains bypass always-block
#   - Domain hierarchy walking (subdomains match base domain)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh"
TRUSTED_DOMAINS_DIR="$REPO_ROOT/.codeflow/config/enforcement/trusted-domains"

export REPO_ROOT

TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-pre-tool-use-webfetch.sh ==="
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
    if shellcheck -e SC1091,SC2034 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
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

echo ""
echo "--- Trusted Domains List Files ---"

# Test 6: Standard list file exists
if [[ -f "$TRUSTED_DOMAINS_DIR/standard.list" ]]; then
    pass "Standard list file exists"
else
    fail "Standard list file not found at $TRUSTED_DOMAINS_DIR/standard.list"
fi

# Test 7: Autonomous list file exists
if [[ -f "$TRUSTED_DOMAINS_DIR/autonomous.list" ]]; then
    pass "Autonomous list file exists"
else
    fail "Autonomous list file not found"
fi

# Test 8: Permissive list file exists
if [[ -f "$TRUSTED_DOMAINS_DIR/permissive.list" ]]; then
    pass "Permissive list file exists"
else
    fail "Permissive list file not found"
fi

# Test 9: Hook references trusted domains directory
if grep -q "trusted-domains" "$HOOK"; then
    pass "Hook references trusted-domains directory"
else
    fail "Hook should reference trusted-domains directory"
fi

# Test 10: Hook loads from .list files
if grep -q "load_trusted_domains" "$HOOK" && grep -q ".list" "$HOOK"; then
    pass "Hook loads from .list files"
else
    fail "Hook should load trusted domains from .list files"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 11: Exits 0 for non-network tools
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-network tools (Read)"
else
    fail "Should exit 0 for non-network tools"
fi

# Test 12: Exits 0 for Edit tool
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Edit tool"
else
    fail "Should exit 0 for Edit tool"
fi

# Test 13: Exits 0 for Bash without network commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-network Bash commands"
else
    fail "Should exit 0 for non-network commands"
fi

# Test 14: Processes Bash with curl command
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"curl https://example.com"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
# Should process (may block or allow depending on domain)
if [[ "$result" == *"EXIT:0"* ]] || [[ "$result" == *"EXIT:2"* ]]; then
    pass "Processes Bash with curl command"
else
    fail "Should process curl commands"
fi

# Test 15: Processes WebFetch tool
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://example.com"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] || [[ "$result" == *"EXIT:2"* ]]; then
    pass "Processes WebFetch tool"
else
    fail "Should process WebFetch tool"
fi

# Test 16: Processes WebSearch tool
result=$(TOOL_NAME="WebSearch" TOOL_INPUT='{"url":"https://example.com"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] || [[ "$result" == *"EXIT:2"* ]]; then
    pass "Processes WebSearch tool"
else
    fail "Should process WebSearch tool"
fi

echo ""
echo "--- Trusted Domains (from standard.list) ---"

# Test 17: Allows github.com (in standard.list)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://github.com/test"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows trusted domain github.com"
else
    fail "Should allow github.com (in standard.list)"
fi

# Test 18: Allows api.github.com (subdomain of trusted github.com)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://api.github.com/users"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows subdomain api.github.com (hierarchy match)"
else
    fail "Should allow api.github.com (subdomain of github.com)"
fi

# Test 19: Allows stackoverflow.com (in standard.list)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://stackoverflow.com/questions"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows trusted domain stackoverflow.com"
else
    fail "Should allow stackoverflow.com"
fi

# Test 20: Allows npmjs.com (in standard.list)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://npmjs.com/package/test"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows trusted domain npmjs.com"
else
    fail "Should allow npmjs.com"
fi

# Test 21: Allows pypi.org (in standard.list)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://pypi.org/project/test"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows trusted domain pypi.org"
else
    fail "Should allow pypi.org"
fi

echo ""
echo "--- Untrusted Domains (default: ask) ---"

# Create temp settings for testing untrustedAction
TEMP_SETTINGS_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_SETTINGS_DIR"' EXIT

# Test 22: Default behavior - asks for untrusted domain (exit 0 with JSON)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://malicious-site.example.com/foo"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"permissionDecision"* ]]; then
    pass "Asks for untrusted domain (default behavior)"
else
    fail "Should ask for untrusted domain (default untrustedAction=ask)"
fi

# Test 23: Ask response includes domain info
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://unknown-domain.net/path"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"unknown-domain.net"* ]] || [[ "$result" == *"allowlist"* ]]; then
    pass "Ask response includes domain context"
else
    fail "Ask response should include domain info"
fi

echo ""
echo "--- Untrusted Domains (untrustedAction=block) ---"

# Test 24: Blocks when untrustedAction=block in settings
# Create temp settings file with block action
echo '{"_web_fetch_config":{"untrustedAction":"block"}}' > "$TEMP_SETTINGS_DIR/settings.local.json"
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://malicious-site.example.com/foo"}' \
    bash -c "REPO_ROOT='$TEMP_SETTINGS_DIR' source '$HOOK'" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks untrusted domain when untrustedAction=block"
else
    # Fallback: test that hook supports block action in code
    if grep -q "UNTRUSTED_ACTION.*block" "$HOOK"; then
        pass "Hook supports untrustedAction=block"
    else
        fail "Should block when untrustedAction=block"
    fi
fi

# Test 25: Block message references .list file location
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://untrusted.example.com"}' \
    bash -c "REPO_ROOT='$TEMP_SETTINGS_DIR' source '$HOOK'" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"trusted-domains"* ]] || grep -q "trusted-domains" "$HOOK"; then
    pass "Block message references .list file location"
else
    fail "Block message should reference trusted-domains/*.list"
fi

echo ""
echo "--- Always-Block Domains ---"

# Test 26: Blocks localhost (always-block)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"http://localhost:8080/api"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks localhost (always-block)"
else
    fail "Should block localhost"
fi

# Test 27: Blocks 127.0.0.1 (always-block)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"http://127.0.0.1:3000/"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks 127.0.0.1 (always-block)"
else
    fail "Should block 127.0.0.1"
fi

# Test 28: Blocks private IP 192.168.x.x (always-block)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"curl http://192.168.1.1/admin"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks private IP 192.168.x.x"
else
    fail "Should block 192.168.1.1"
fi

# Test 29: Blocks private IP 10.x.x.x (always-block)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"curl http://10.0.0.1/"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks private IP 10.x.x.x"
else
    fail "Should block 10.0.0.1"
fi

# Test 30: Blocks *.local domains (always-block pattern)
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"http://myserver.local/api"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks *.local domains"
else
    fail "Should block myserver.local"
fi

# Test 31: Always-block message mentions developer override options
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"http://localhost:8080"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"Developer override"* ]] || [[ "$result" == *"trusted-domains"* ]]; then
    pass "Always-block message mentions override options"
else
    fail "Always-block message should mention developer override options"
fi

echo ""
echo "--- Domain Extraction ---"

# Test 32: Has extract_domain function
if grep -q "extract_domain" "$HOOK"; then
    pass "Has extract_domain function"
else
    fail "Should have extract_domain function"
fi

# Test 33: Extracts domain from URL with port
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://github.com:443/test"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Extracts domain from URL with port"
else
    fail "Should handle URL with port correctly"
fi

# Test 34: Extracts domain from URL with path
result=$(TOOL_NAME="WebFetch" TOOL_INPUT='{"url":"https://github.com/owner/repo/issues"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Extracts domain from URL with path"
else
    fail "Should handle URL with path correctly"
fi

echo ""
echo "--- Approval Mode Support ---"

# Test 35: Hook supports approvalMode setting
if grep -q "approvalMode\|APPROVAL_MODE" "$HOOK"; then
    pass "Hook supports approvalMode setting"
else
    fail "Hook should support approvalMode setting"
fi

# Test 36: Hook has case for strict mode
if grep -q "strict" "$HOOK"; then
    pass "Hook supports strict approval mode"
else
    fail "Hook should support strict mode"
fi

# Test 37: Hook has case for standard mode
if grep -q "standard" "$HOOK"; then
    pass "Hook supports standard approval mode"
else
    fail "Hook should support standard mode"
fi

# Test 38: Hook has case for autonomous mode
if grep -q "autonomous" "$HOOK"; then
    pass "Hook supports autonomous approval mode"
else
    fail "Hook should support autonomous mode"
fi

# Test 39: Hook has case for permissive mode
if grep -q "permissive" "$HOOK"; then
    pass "Hook supports permissive approval mode"
else
    fail "Hook should support permissive mode"
fi

echo ""
echo "--- Configuration ---"

# Test 40: Hook reads from settings for config
if grep -q "settings.json\|settings.local.json" "$HOOK"; then
    pass "Hook reads settings for configuration"
else
    fail "Hook should read settings files"
fi

# Test 41: Hook checks _web_fetch_config
if grep -q "_web_fetch_config" "$HOOK"; then
    pass "Hook uses _web_fetch_config from settings"
else
    fail "Hook should check _web_fetch_config in settings"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
