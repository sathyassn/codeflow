#!/usr/bin/env bash
# Test: cf-pre-tool-use-gh-pr.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-gh-pr.sh
#
# Comprehensive tests for gh pr create enforcement hook:
#   - Tool filtering (Bash only)
#   - Command detection (gh pr create)
#   - Body flag requirement
#   - AI attribution blocking
#   - Required sections validation
#   - Title format (conventional commit)
#   - Title length limits
#   - Config loading

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

echo "=== Testing cf-pre-tool-use-gh-pr.sh ==="
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

echo ""
echo "--- Non-PR Commands (Should Allow) ---"

# Test 13: Allows non-gh commands
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls -la"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows non-gh commands"
else
    fail "Should allow non-gh commands"
fi

# Test 14: Allows gh issue list
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh issue list"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh issue list"
else
    fail "Should allow gh issue list"
fi

# Test 15: Allows gh repo clone
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh repo clone owner/repo"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh repo clone"
else
    fail "Should allow gh repo clone"
fi

# Test 16: Allows gh pr view
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr view 123"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh pr view"
else
    fail "Should allow gh pr view"
fi

# Test 17: Allows gh pr list
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr list"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh pr list"
else
    fail "Should allow gh pr list"
fi

# Test 18: Allows gh workflow run
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh workflow run test.yml"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows gh workflow run"
else
    fail "Should allow gh workflow run"
fi

echo ""
echo "--- Body Flag Requirement ---"

# Test 19: Blocks gh pr create without --body
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks gh pr create without --body"
else
    fail "Should block gh pr create without --body"
fi

# Test 20: Allows with --body flag
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows with --body flag"
else
    fail "Should allow with --body flag"
fi

# Test 21: Allows with -b flag (short form)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" -b \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows with -b flag (short form)"
else
    fail "Should allow with -b flag"
fi

echo ""
echo "--- AI Attribution Blocking ---"

# Test 22: Blocks Claude attribution
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nGenerated with Claude\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Claude attribution"
else
    fail "Should block Claude attribution"
fi

# Test 23: Blocks ChatGPT attribution
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nChatGPT helped\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks ChatGPT attribution"
else
    fail "Should block ChatGPT attribution"
fi

# Test 24: Blocks Copilot attribution
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nUsed Copilot\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Copilot attribution"
else
    fail "Should block Copilot attribution"
fi

# Test 25: Blocks Gemini attribution
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nGemini assisted\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Gemini attribution"
else
    fail "Should block Gemini attribution"
fi

# Test 26: Blocks AI-generated phrase
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nAI-generated code\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks AI-generated phrase"
else
    fail "Should block AI-generated phrase"
fi

# Test 27: Blocks Generated with phrase
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nGenerated with AI\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Generated with phrase"
else
    fail "Should block Generated with phrase"
fi

# Test 28: Blocks Anthropic mention
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nAnthropic model\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks Anthropic mention"
else
    fail "Should block Anthropic mention"
fi

# Test 29: Blocks OpenAI mention
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nOpenAI powered\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks OpenAI mention"
else
    fail "Should block OpenAI mention"
fi

# Test 30: Case-insensitive AI detection (lowercase)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\nclaude helped\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Case-insensitive AI detection"
else
    fail "Should detect AI attribution case-insensitively"
fi

echo ""
echo "--- Required Sections ---"

# Test 31: Blocks missing ## Summary section
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"Some body without sections\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks missing ## Summary section"
else
    fail "Should block missing ## Summary section"
fi

# Test 32: Blocks missing ## Testing section
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"## Summary\ntest content\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks missing ## Testing section"
else
    fail "Should block missing ## Testing section"
fi

# Test 33: Block message mentions missing section
HOOK_OUTPUT=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body \"no sections here\""}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"Missing required section"* ]]; then
    pass "Block message mentions missing section"
else
    fail "Should mention missing section in block message"
fi

echo ""
echo "--- Title Format (Conventional Commit) ---"

# Test 34: Blocks title without type prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"bad title\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks title without type prefix"
else
    fail "Should block title without type prefix"
fi

# Test 35: Blocks title without colon-space
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat test\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks title without colon-space"
else
    fail "Should block title without colon-space"
fi

# Test 36: Allows feat: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: add feature\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows feat: prefix"
else
    fail "Should allow feat: prefix"
fi

# Test 37: Allows fix: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"fix: bug fix\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows fix: prefix"
else
    fail "Should allow fix: prefix"
fi

# Test 38: Allows docs: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"docs: update readme\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows docs: prefix"
else
    fail "Should allow docs: prefix"
fi

# Test 39: Allows refactor: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"refactor: clean up\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows refactor: prefix"
else
    fail "Should allow refactor: prefix"
fi

# Test 40: Allows test: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"test: add tests\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows test: prefix"
else
    fail "Should allow test: prefix"
fi

# Test 41: Allows chore: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"chore: update deps\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows chore: prefix"
else
    fail "Should allow chore: prefix"
fi

# Test 42: Allows perf: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"perf: optimize\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows perf: prefix"
else
    fail "Should allow perf: prefix"
fi

# Test 43: Allows ci: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"ci: update workflow\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows ci: prefix"
else
    fail "Should allow ci: prefix"
fi

# Test 44: Allows bugfix: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"bugfix: resolve edge case\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows bugfix: prefix"
else
    fail "Should allow bugfix: prefix"
fi

# Test 45: Allows hotfix: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"hotfix: critical patch\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows hotfix: prefix"
else
    fail "Should allow hotfix: prefix"
fi

# Test 46: Allows style: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"style: fix formatting\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows style: prefix"
else
    fail "Should allow style: prefix"
fi

# Test 47: Allows build: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"build: update makefile\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows build: prefix"
else
    fail "Should allow build: prefix"
fi

# Test 48: Allows revert: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"revert: undo bad change\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows revert: prefix"
else
    fail "Should allow revert: prefix"
fi

# Test 49: Allows merge: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"merge: combine branches\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows merge: prefix"
else
    fail "Should allow merge: prefix"
fi

# Test 50: Allows plan: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"plan: design new feature\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows plan: prefix"
else
    fail "Should allow plan: prefix"
fi

# Test 51: Allows refine: prefix
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"refine: polish workflow\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows refine: prefix"
else
    fail "Should allow refine: prefix"
fi

echo ""
echo "--- Title Length Limits ---"

# Test 44: Blocks title exceeding max length
long_title="feat: this is a very long title that exceeds the maximum allowed character limit"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"$long_title\\\" --body \\\"## Summary\\ntest\\n## Testing\\ntest\\\"\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks title exceeding max length"
else
    fail "Should block title exceeding max length"
fi

# Test 45: Block message shows character count
HOOK_OUTPUT=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"$long_title\\\" --body \\\"## Summary\\ntest\\n## Testing\\ntest\\\"\"}" bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"chars"* ]]; then
    pass "Block message shows character count"
else
    fail "Should show character count in block message"
fi

# Test 46: Allows title at max length (50 chars)
title_50="feat: exactly fifty characters title ok"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"$title_50\\\" --body \\\"## Summary\\ntest\\n## Testing\\ntest\\\"\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows title at max length"
else
    fail "Should allow title at max length (50 chars)"
fi

echo ""
echo "--- Title Extraction Patterns ---"

# Test 47: Extracts title with double quotes
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: double quoted\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Extracts title with double quotes"
else
    fail "Should extract title with double quotes"
fi

# Test 48: Extracts title with single quotes
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title 'feat: single quoted' --body '## Summary\ntest\n## Testing\ntest'\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Extracts title with single quotes"
else
    fail "Should extract title with single quotes"
fi

# Test 49: Extracts title with -t flag
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create -t \"feat: short flag\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Extracts title with -t flag"
else
    fail "Should extract title with -t flag"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 50: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 51: Reads commit_types from config
if grep -q "commit_types" "$HOOK"; then
    pass "Reads commit_types from config"
else
    fail "Should read commit_types from config"
fi

# Test 52: Reads ai_attribution_patterns from config
if grep -q "ai_attribution_patterns" "$HOOK"; then
    pass "Reads ai_attribution_patterns from config"
else
    fail "Should read ai_attribution_patterns from config"
fi

# Test 53: Reads required_sections from config
if grep -q "required_sections" "$HOOK"; then
    pass "Reads required_sections from config"
else
    fail "Should read required_sections from config"
fi

# Test 54: Has default fallbacks for config values
if grep -q "DEFAULT_VALID_TYPES" "$HOOK" && grep -q "DEFAULT_AI_PATTERN" "$HOOK"; then
    pass "Has default fallbacks for config values"
else
    fail "Should have default fallbacks"
fi

# Test 55: Reads max_length from config
if grep -q "max_length" "$HOOK"; then
    pass "Reads max_length from config"
else
    fail "Should read max_length from config"
fi

echo ""
echo "--- Skill Direction ---"

# Test 56: Prints skill direction on block
if grep -q "Skill" "$HOOK" && grep -q "cf-git-workflow" "$HOOK"; then
    pass "Prints skill direction on block"
else
    fail "Should print skill direction on block"
fi

# Test 57: Block message includes MUST: Skill
HOOK_OUTPUT=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"bad\" --body \"no sections\""}' bash "$HOOK" </dev/null 2>&1 || true)
if [[ "$HOOK_OUTPUT" == *"MUST:"* ]] && [[ "$HOOK_OUTPUT" == *"Skill"* ]]; then
    pass "Block message includes MUST: Skill"
else
    fail "Should include MUST: Skill in block message"
fi

echo ""
echo "--- Security Logging ---"

# Test 58: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should have security event logging"
fi

# Test 59: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

echo ""
echo "--- Code Quality ---"

# Test 60: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 61: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 62: Has jq fallback for command extraction
if grep -q "grep -o" "$HOOK" && grep -q "command" "$HOOK"; then
    pass "Has jq fallback for command extraction"
else
    fail "Should have jq fallback"
fi

# Test 63: Exports REPO_ROOT and LIB_DIR
if grep -q "export REPO_ROOT LIB_DIR" "$HOOK"; then
    pass "Exports REPO_ROOT and LIB_DIR"
else
    fail "Should export REPO_ROOT and LIB_DIR"
fi

# Test 64: Has bash 3.2+ compatible required_sections loading
if grep -q 'while IFS= read -r' "$HOOK" && grep -q 'REQUIRED_SECTIONS' "$HOOK"; then
    pass "Has bash 3.2+ compatible required_sections loading"
else
    fail "Should have bash 3.2+ compatible array loading"
fi

echo ""
echo "--- Scoped Title Format ---"

# Test 65: Allows feat(api): scoped title
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat(api): add endpoint\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows feat(api): scoped title"
else
    fail "Should allow feat(api): scoped title"
fi

# Test 66: Allows fix(auth): scoped title
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"fix(auth): resolve login bug\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows fix(auth): scoped title"
else
    fail "Should allow fix(auth): scoped title"
fi

# Test 67: Allows docs(readme): scoped title
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"docs(readme): update usage\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows docs(readme): scoped title"
else
    fail "Should allow docs(readme): scoped title"
fi

# Test 68: Allows chore(deps-2): hyphenated scope with number
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"chore(deps-2): bump versions\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows hyphenated scope with number"
else
    fail "Should allow hyphenated scope with number"
fi

# Test 69: Still allows non-scoped titles
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: plain title\" --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Still allows non-scoped titles"
else
    fail "Should still allow non-scoped titles"
fi

echo ""
echo "--- Body File Support (--body-file / -F) ---"

# Test 70: Allows --body-file flag (satisfies body requirement)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body-file /tmp/claude/pr-body.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows --body-file flag (satisfies body requirement)"
else
    fail "Should allow --body-file flag"
fi

# Test 71: Allows -F flag (satisfies body requirement)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" -F /tmp/claude/pr-body.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows -F flag (satisfies body requirement)"
else
    fail "Should allow -F flag"
fi

# Test 72: --body-file with AI attribution in file content
BODY_FILE="/tmp/claude/test-pr-ai-body.md"
mkdir -p /tmp/claude
printf '## Summary\nGenerated with Claude\n## Testing\ntest' > "$BODY_FILE"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"feat: test\\\" --body-file $BODY_FILE\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]] && [[ "$result" == *"AI attribution"* ]]; then
    pass "Blocks AI attribution in --body-file content"
else
    fail "Should block AI attribution found in --body-file content"
fi

# Test 73: --body-file with missing required sections
printf 'Just some text without sections' > "$BODY_FILE"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"feat: test\\\" --body-file $BODY_FILE\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]] && [[ "$result" == *"Missing required section"* ]]; then
    pass "Blocks --body-file with missing required sections"
else
    fail "Should block --body-file with missing required sections"
fi

# Test 74: --body-file with valid content passes
printf '## Summary\nGood content\n## Testing\nAll tests pass' > "$BODY_FILE"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"feat: test\\\" --body-file $BODY_FILE\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows --body-file with valid content"
else
    fail "Should allow --body-file with valid content"
fi

# Test 75: --body-file with nonexistent file allows through
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body-file /tmp/claude/nonexistent-file.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Allows --body-file with nonexistent file (graceful)"
else
    fail "Should allow --body-file with nonexistent file"
fi

# Test 76: -F with AI attribution in file content
printf '## Summary\nChatGPT wrote this\n## Testing\ntest' > "$BODY_FILE"
result=$(TOOL_NAME="Bash" TOOL_INPUT="{\"command\":\"gh pr create --title \\\"feat: test\\\" -F $BODY_FILE\"}" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:2"* ]] && [[ "$result" == *"BLOCKED"* ]]; then
    pass "Blocks AI attribution via -F flag"
else
    fail "Should block AI attribution via -F flag"
fi

# Clean up temp file
rm -f "$BODY_FILE"

echo ""
echo "--- File Path Exclusion from AI Check ---"

# Test 77: File path containing 'Claude' does NOT trigger AI check
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body-file /tmp/claude/pr-body.md --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "File path with claude does not false-positive AI check"
else
    fail "Should not false-positive on /tmp/claude/ in file path"
fi

# Test 78: Path with Anthropic does not false-positive
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"gh pr create --title \"feat: test\" --body-file /home/Anthropic/docs/body.md --body \"## Summary\ntest\n## Testing\ntest\""}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "File path with Anthropic does not false-positive AI check"
else
    fail "Should not false-positive on Anthropic in file path"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
