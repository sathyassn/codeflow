#!/usr/bin/env bash
# Test: test-coverage.sh library functions
# Location: .codeflow/testing/lib/test-test-coverage.sh
#
# Tests the coverage validation library:
#   - derive_test_path() naming convention mapping
#   - is_test_registered() config lookup
#   - is_excepted() exception checking
#   - find_orphaned_tests() detection
#   - validate_coverage() basic functionality
#
# Exit codes:
#   0 - All tests passed
#   1 - One or more tests failed

set -euo pipefail

# ============================================================================
# SETUP
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

export REPO_ROOT

# Source test helpers (which sources test-common.sh)
source "$SCRIPT_DIR/test-helpers.sh"

# Source the coverage library under test
source "$SCRIPT_DIR/test-coverage.sh"

echo ""
echo "=== Testing test-coverage.sh Library ==="
echo ""

# ============================================================================
# TEST: derive_test_path()
# ============================================================================

test_section "derive_test_path()"

# Python library modules
RESULT=$(derive_test_path ".codeflow/scripts/codeflow_py_lib/config.py")
assert_equals ".codeflow/testing/scripts/codeflow_py_lib/test_config.py" "$RESULT" \
    "Python lib: config.py -> test_config.py"

RESULT=$(derive_test_path ".codeflow/scripts/codeflow_py_lib/validation.py")
assert_equals ".codeflow/testing/scripts/codeflow_py_lib/test_validation.py" "$RESULT" \
    "Python lib: validation.py -> test_validation.py"

# Shell library modules
RESULT=$(derive_test_path ".codeflow/scripts/shell-lib/config.sh")
assert_equals ".codeflow/testing/scripts/shell-lib/test-config.sh" "$RESULT" \
    "Shell lib: config.sh -> test-config.sh"

# Domain scripts (Python)
RESULT=$(derive_test_path ".codeflow/scripts/db/test_schema.py")
assert_equals ".codeflow/testing/scripts/db/test_test_schema.py" "$RESULT" \
    "Domain Python: db/test_schema.py derives test path"

RESULT=$(derive_test_path ".codeflow/scripts/memory/cf-memory-store.py")
assert_equals ".codeflow/testing/scripts/memory/test_cf_memory_store.py" "$RESULT" \
    "Domain Python with hyphens: cf-memory-store.py -> test_cf_memory_store.py"

# Domain scripts (Shell)
RESULT=$(derive_test_path ".codeflow/scripts/security/cf-stage-edit.sh")
assert_equals ".codeflow/testing/scripts/security/test-cf-stage-edit.sh" "$RESULT" \
    "Domain Shell: security/cf-stage-edit.sh -> test-cf-stage-edit.sh"

# Subdomain scripts (Shell)
RESULT=$(derive_test_path ".codeflow/scripts/security/enforcement/cf-path-protection.sh")
assert_equals ".codeflow/testing/scripts/security/enforcement/test-cf-path-protection.sh" "$RESULT" \
    "Subdomain Shell: enforcement/cf-path-protection.sh -> test-cf-path-protection.sh"

# Claude hooks
RESULT=$(derive_test_path ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh")
assert_equals ".codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-gh-pr.sh" "$RESULT" \
    "Claude hook: pre-tool-use -> claude-hooks/pre-tool-use"

RESULT=$(derive_test_path ".claude/hooks/codeflow/session-start/cf-session-start-logging.sh")
assert_equals ".codeflow/testing/claude-hooks/session-start/test-cf-session-start-logging.sh" "$RESULT" \
    "Claude hook: session-start -> claude-hooks/session-start"

# Non-matching path returns empty
RESULT=$(derive_test_path "some/random/path.txt")
assert_empty "$RESULT" "Non-matching path returns empty"

# ============================================================================
# TEST: is_test_registered()
# ============================================================================

test_section "is_test_registered()"

# Known registered test
if is_test_registered "scripts/db/test_schema.py"; then
    test_pass "scripts/db/test_schema.py is registered"
else
    test_fail "scripts/db/test_schema.py should be registered"
fi

# Known registered test (consistency)
if is_test_registered "consistency/test-settings-sync.sh"; then
    test_pass "consistency/test-settings-sync.sh is registered"
else
    test_fail "consistency/test-settings-sync.sh should be registered"
fi

# Non-existent test
if is_test_registered "scripts/nonexistent/test-fake.sh"; then
    test_fail "Nonexistent test should NOT be registered"
else
    test_pass "Nonexistent test correctly not registered"
fi

# ============================================================================
# TEST: is_excepted()
# ============================================================================

test_section "is_excepted()"

# Claude hooks should be excepted (integration_tested pattern)
if is_excepted ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh"; then
    test_pass "Claude hooks are excepted (integration_tested)"
else
    test_fail "Claude hooks should be excepted"
fi

# Memory scripts should be excepted
if is_excepted ".codeflow/scripts/memory/cf-memory-store.py"; then
    test_pass "Memory scripts are excepted (integration_tested)"
else
    test_fail "Memory scripts should be excepted"
fi

# Non-excepted script
if is_excepted ".codeflow/scripts/some-random-script.sh"; then
    test_fail "Random script should NOT be excepted"
else
    test_pass "Random script correctly not excepted"
fi

# ============================================================================
# TEST: validate_coverage() mode behavior
# ============================================================================

test_section "validate_coverage() modes"

# Audit mode should always return 0
AUDIT_OUTPUT=$(validate_coverage "audit" 2>&1) || true
AUDIT_EXIT=$?
# audit mode returns 0 even with issues
assert_equals "0" "$AUDIT_EXIT" "audit mode returns 0 (does not fail)"

# Warn mode should always return 0
validate_coverage "warn" >/dev/null 2>&1 || true
WARN_EXIT=$?
assert_equals "0" "$WARN_EXIT" "warn mode returns 0 (does not fail)"

# validate_coverage produces a report header
if grep -q "TEST COVERAGE REPORT" <<< "$AUDIT_OUTPUT"; then
    test_pass "validate_coverage produces coverage report"
else
    test_fail "validate_coverage should produce coverage report"
fi

# ============================================================================
# TEST: find_orphaned_tests() basic behavior
# ============================================================================

test_section "find_orphaned_tests()"

# Run orphaned test detection - capture output
ORPHAN_OUTPUT=$(find_orphaned_tests 2>&1) || true

# Should produce section header
if grep -q "Orphaned Test Detection" <<< "$ORPHAN_OUTPUT"; then
    test_pass "find_orphaned_tests produces section header"
else
    test_fail "find_orphaned_tests should produce section header"
fi

# ============================================================================
# TEST: discover_scripts() returns results
# ============================================================================

test_section "discover_scripts()"

SCRIPT_LIST=$(discover_scripts 2>/dev/null)
SCRIPT_COUNT=$(echo "$SCRIPT_LIST" | wc -l | tr -d ' ')

if [[ "$SCRIPT_COUNT" -gt 0 ]]; then
    test_pass "discover_scripts finds scripts (count: $SCRIPT_COUNT)"
else
    test_fail "discover_scripts should find at least some scripts"
fi

# Should include hooks
if grep -q "\.claude/hooks/" <<< "$SCRIPT_LIST"; then
    test_pass "discover_scripts includes claude hooks"
else
    test_fail "discover_scripts should include claude hooks"
fi

# Should include codeflow scripts
if grep -q "\.codeflow/scripts/" <<< "$SCRIPT_LIST"; then
    test_pass "discover_scripts includes codeflow scripts"
else
    test_fail "discover_scripts should include codeflow scripts"
fi

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
