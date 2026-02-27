#!/usr/bin/env bash
# Test: Staging workflow - cf-stage-edit.sh
# Location: .codeflow/testing/scripts/security/staging/test-cf-stage-edit.sh
#
# Tests the staging edit functionality for protected files

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/staging/cf-stage-edit.sh"
STAGING_DIR="/tmp/claude/${CF_PROJECT_ROOT:-codeflow}/managed/protected-edits"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup staging artifacts on exit
_test_staging_cleanup() {
    rm -f "$STAGING_DIR"/*test-stage-original* 2>/dev/null || true
    rm -f "$STAGING_DIR"/*test-stage-abs* 2>/dev/null || true
}
trap '_test_staging_cleanup; _test_isolation_cleanup' EXIT

echo "=== Testing cf-stage-edit.sh ==="
echo ""

# ============================================================================
# Test 1: Script exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$SCRIPT" ]]; then
    echo "PASS: Script is executable"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Script is not executable"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 2: Help flag
# ============================================================================
if "$SCRIPT" --help 2>&1 | grep -q "USAGE:"; then
    echo "PASS: --help shows usage"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --help not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 3: Version flag
# ============================================================================
if "$SCRIPT" --version 2>&1 | grep -qE "version [0-9]+\.[0-9]+\.[0-9]+"; then
    echo "PASS: --version shows version"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --version not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 4: Unknown option rejected
# ============================================================================
echo ""
echo "--- Argument validation ---"

OUTPUT=$("$SCRIPT" --invalid-flag 2>&1 || true)
if echo "$OUTPUT" | grep -qi "unknown option"; then
    echo "PASS: Rejects unknown option"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should reject unknown option"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Missing arguments
# ============================================================================
OUTPUT=$("$SCRIPT" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "error"; then
    echo "PASS: Errors on missing arguments"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on missing arguments"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: New content file not found
# ============================================================================
echo "original" > "$TEST_TMPDIR/test-stage-original.txt"
OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-stage-original.txt" "/nonexistent/content.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "new content file not found"; then
    echo "PASS: Errors on nonexistent new content file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent new content file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: Original file not found
# ============================================================================
echo "content" > "$TEST_TMPDIR/test-stage-content.txt"
OUTPUT=$("$SCRIPT" "/nonexistent/file" "$TEST_TMPDIR/test-stage-content.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found"; then
    echo "PASS: Errors on nonexistent original file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Successful staging
# ============================================================================
echo ""
echo "--- Staging functionality ---"

# Create test files
echo "original content" > "$TEST_TMPDIR/test-stage-original.txt"
echo "new content" > "$TEST_TMPDIR/test-stage-new.txt"

OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-stage-original.txt" "$TEST_TMPDIR/test-stage-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "staged successfully"; then
    echo "PASS: Staging succeeds"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Staging failed: $OUTPUT"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Staged files created (.staged, .original, .metadata.json)
# ============================================================================
SAFE_NAME=$(echo "$TEST_TMPDIR/test-stage-original.txt" | sed 's/[\/]/_/g')

if [[ -f "$STAGING_DIR/${SAFE_NAME}.staged" ]]; then
    echo "PASS: Staged file created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Staged file not created"
    ((TESTS_FAILED++)) || true
fi

if [[ -f "$STAGING_DIR/${SAFE_NAME}.original" ]]; then
    echo "PASS: Original backup created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Original backup not created"
    ((TESTS_FAILED++)) || true
fi

if [[ -f "$STAGING_DIR/${SAFE_NAME}.metadata.json" ]]; then
    echo "PASS: Metadata file created"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Metadata file not created"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 10: Staged content matches new content
# ============================================================================
if [[ -f "$STAGING_DIR/${SAFE_NAME}.staged" ]]; then
    STAGED_CONTENT=$(cat "$STAGING_DIR/${SAFE_NAME}.staged")
    if [[ "$STAGED_CONTENT" == "new content" ]]; then
        echo "PASS: Staged content matches new content"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Staged content does not match"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: Staged file missing"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Original backup matches original content
# ============================================================================
if [[ -f "$STAGING_DIR/${SAFE_NAME}.original" ]]; then
    ORIG_CONTENT=$(cat "$STAGING_DIR/${SAFE_NAME}.original")
    if [[ "$ORIG_CONTENT" == "original content" ]]; then
        echo "PASS: Original backup matches original content"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Original backup content does not match"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: Original backup missing"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Metadata is valid JSON with required fields
# ============================================================================
echo ""
echo "--- Metadata validation ---"

if [[ -f "$STAGING_DIR/${SAFE_NAME}.metadata.json" ]] && command -v jq &>/dev/null; then
    if jq empty "$STAGING_DIR/${SAFE_NAME}.metadata.json" 2>/dev/null; then
        echo "PASS: Metadata is valid JSON"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Metadata is not valid JSON"
        ((TESTS_FAILED++)) || true
    fi

    # Check required fields
    META_FIELDS_OK=true
    for field in file_path full_path original_checksum staged_checksum staged_at expires_at ttl_seconds session_id; do
        if ! jq -e ".$field" "$STAGING_DIR/${SAFE_NAME}.metadata.json" &>/dev/null; then
            echo "FAIL: Metadata missing field: $field"
            META_FIELDS_OK=false
            ((TESTS_FAILED++)) || true
            break
        fi
    done
    if [[ "$META_FIELDS_OK" == "true" ]]; then
        echo "PASS: Metadata has all required fields"
        ((TESTS_PASSED++)) || true
    fi

    # Check TTL is 3600 (default - staging TTL, not sentinel TTL)
    META_TTL=$(jq -r '.ttl_seconds' "$STAGING_DIR/${SAFE_NAME}.metadata.json" 2>/dev/null)
    if [[ "$META_TTL" == "3600" ]]; then
        echo "PASS: TTL is 3600 (1 hour staging default)"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: TTL should be 3600, got: $META_TTL"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: jq not available or metadata missing"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 13: Checksums are non-empty and differ
# ============================================================================
if [[ -f "$STAGING_DIR/${SAFE_NAME}.metadata.json" ]] && command -v jq &>/dev/null; then
    ORIG_SUM=$(jq -r '.original_checksum' "$STAGING_DIR/${SAFE_NAME}.metadata.json" 2>/dev/null)
    STAGED_SUM=$(jq -r '.staged_checksum' "$STAGING_DIR/${SAFE_NAME}.metadata.json" 2>/dev/null)

    if [[ -n "$ORIG_SUM" ]] && [[ -n "$STAGED_SUM" ]] && [[ "$ORIG_SUM" != "null" ]] && [[ "$STAGED_SUM" != "null" ]]; then
        echo "PASS: Checksums are non-empty"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Checksums are empty or null"
        ((TESTS_FAILED++)) || true
    fi

    if [[ "$ORIG_SUM" != "$STAGED_SUM" ]]; then
        echo "PASS: Original and staged checksums differ (content changed)"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Checksums should differ for different content"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: Cannot validate checksums"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 14: Duplicate staging blocked
# ============================================================================
echo ""
echo "--- Edge cases ---"

OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-stage-original.txt" "$TEST_TMPDIR/test-stage-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "already exists"; then
    echo "PASS: Blocks duplicate staging"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should block duplicate staging"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 15: Clean up and re-stage with absolute path
# ============================================================================
rm -f "$STAGING_DIR/${SAFE_NAME}.staged" "$STAGING_DIR/${SAFE_NAME}.original" "$STAGING_DIR/${SAFE_NAME}.metadata.json"

# Create a new file for absolute path test
echo "abs original" > "$TEST_TMPDIR/test-stage-abs.txt"
echo "abs new" > "$TEST_TMPDIR/test-stage-abs-new.txt"

# Test with absolute path (already absolute since TEST_TMPDIR is absolute)
OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-stage-abs.txt" "$TEST_TMPDIR/test-stage-abs-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "staged successfully"; then
    echo "PASS: Absolute path staging works"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Absolute path staging failed"
    ((TESTS_FAILED++)) || true
fi

# Clean up absolute path test artifacts
ABS_SAFE=$(echo "$TEST_TMPDIR/test-stage-abs.txt" | sed 's/[\/]/_/g')
rm -f "$STAGING_DIR/${ABS_SAFE}.staged" "$STAGING_DIR/${ABS_SAFE}.original" "$STAGING_DIR/${ABS_SAFE}.metadata.json"

# ============================================================================
# Test 16: Output references correct staging directory
# ============================================================================
echo "re-stage test" > "$TEST_TMPDIR/test-stage-original.txt"
echo "re-stage new" > "$TEST_TMPDIR/test-stage-new.txt"
OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-stage-original.txt" "$TEST_TMPDIR/test-stage-new.txt" 2>&1 || true)
if echo "$OUTPUT" | grep -q "/tmp/claude/${CF_PROJECT_ROOT:-codeflow}/managed/protected-edits"; then
    echo "PASS: Output shows correct staging directory"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Output should reference /tmp/claude/\${CF_PROJECT_ROOT:-codeflow}/managed/protected-edits"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 17: Shellcheck passes
# ============================================================================
echo ""
echo "--- Code quality ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$SCRIPT" 2>/dev/null; then
        echo "PASS: Script passes shellcheck"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Script fails shellcheck"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: shellcheck not available"
    ((TESTS_PASSED++)) || true
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
