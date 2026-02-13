#!/usr/bin/env bash
# Test: cf-pattern-matching.sh
# Location: .codeflow/testing/scripts/security/test-cf-pattern-matching.sh
#
# Tests the pattern matching utility module

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-pattern-matching.sh"

export REPO_ROOT LIB_DIR

# Source the module to get functions
# shellcheck source=/dev/null
source "$LIB_DIR/security-lib.sh"
# shellcheck source=/dev/null
source "$MODULE"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Test assertion helper
assert_true() {
    local condition="$1"
    local description="$2"

    # Disable glob expansion to prevent ** patterns from expanding during eval
    set -f
    if eval "$condition"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    set +f
}

assert_false() {
    local condition="$1"
    local description="$2"

    set -f
    if ! eval "$condition"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    set +f
}

assert_equals() {
    local expected="$1"
    local actual="$2"
    local description="$3"

    if [[ "$expected" == "$actual" ]]; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description (expected '$expected', got '$actual')"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-pattern-matching.sh ==="
echo ""

# Test 1: matches_extended_glob with simple pattern
echo "Test 1: Simple glob match"
assert_true "matches_extended_glob 'src/main.py' 'src/*.py'" "Should match src/*.py"

# Test 2: matches_extended_glob with ** recursive
echo "Test 2: Recursive glob match"
assert_true "matches_extended_glob 'src/lib/util.py' 'src/**/*.py'" "Should match src/**/*.py"

# Test 3: matches_extended_glob negative case
echo "Test 3: Glob non-match"
assert_false "matches_extended_glob 'src/main.js' 'src/*.py'" "Should not match .js with *.py"

# Test 4: matches_any_pattern with list
echo "Test 4: Match any pattern"
assert_true "matches_any_pattern 'test.py' '*.py' '*.js'" "Should match one of patterns"

# Test 5: is_valid_pattern with valid pattern
echo "Test 5: Valid pattern check"
assert_true "is_valid_pattern 'src/**/*.py'" "Pattern should be valid"

# Test 6: is_valid_pattern with invalid pattern (too broad)
echo "Test 6: Invalid pattern check"
assert_false "is_valid_pattern '*'" "Single * should be invalid"

# Test 7: is_risky_pattern check
echo "Test 7: Risky pattern check"
assert_true "is_risky_pattern '/*'" "/* should be risky"

# Test 8: is_risky_pattern safe pattern
echo "Test 8: Safe pattern check"
assert_false "is_risky_pattern 'src/*.py'" "src/*.py should not be risky"

# Test 9: normalize_path removes ./
echo "Test 9: Normalize path - remove ./"
RESULT=$(normalize_path "./src/main.py")
assert_equals "src/main.py" "$RESULT" "Should remove ./"

# Test 10: normalize_path removes trailing /
echo "Test 10: Normalize path - remove trailing /"
RESULT=$(normalize_path "src/lib/")
assert_equals "src/lib" "$RESULT" "Should remove trailing /"

# Test 11: is_path_within check
echo "Test 11: Path within base check"
assert_true "is_path_within 'src/lib/util.py' 'src'" "Should be within src"

# Test 12: is_path_within negative
echo "Test 12: Path not within base check"
assert_false "is_path_within 'tests/test.py' 'src'" "tests should not be within src"

# Test 13: extract_redirect_target
echo "Test 13: Extract redirect target"
RESULT=$(extract_redirect_target "echo test > output.txt")
assert_equals "output.txt" "$RESULT" "Should extract output.txt"

# Test 14: extract_redirect_target with append
echo "Test 14: Extract append target"
RESULT=$(extract_redirect_target "cat file >> log.txt")
assert_equals "log.txt" "$RESULT" "Should extract log.txt"

# Test 15: extract_file_paths
echo "Test 15: Extract file paths"
RESULT=$(extract_file_paths "cp ./src/main.py /tmp/backup.py")
# Just check it extracted something
if [[ "$RESULT" == *"src/main.py"* ]] || [[ "$RESULT" == *"/tmp/backup.py"* ]]; then
    echo "PASS: Should extract paths"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Should extract paths (got '$RESULT')"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 16: normalize_path handles ..
echo "Test 16: Normalize path - handle .."
RESULT=$(normalize_path "src/lib/../util.py")
assert_equals "src/util.py" "$RESULT" "Should resolve .."

# Test 17: normalize_path handles leading ../
echo "Test 17: Normalize path - handle leading ../"
RESULT=$(normalize_path "../src/main.py")
assert_equals "src/main.py" "$RESULT" "Should remove leading ../"

# Test 18: is_valid_pattern rejects **
echo "Test 18: Invalid pattern - standalone **"
assert_false "is_valid_pattern '**'" "Standalone ** should be invalid"

# Test 19: is_valid_pattern rejects /*
echo "Test 19: Invalid pattern - /*"
assert_false "is_valid_pattern '/*'" "/* should be invalid"

# Test 20: is_valid_pattern rejects empty
echo "Test 20: Invalid pattern - empty"
assert_false "is_valid_pattern ''" "Empty pattern should be invalid"

# Test 21: is_valid_pattern rejects unbalanced brackets
echo "Test 21: Invalid pattern - unbalanced brackets"
assert_false "is_valid_pattern 'test[.py'" "Unbalanced [ should be invalid"

# Test 22: is_risky_pattern detects /**
echo "Test 22: Risky pattern - /**"
assert_true "is_risky_pattern '/**'" "/** should be risky"

# Test 23: is_risky_pattern detects /.*
echo "Test 23: Risky pattern - /.*"
assert_true "is_risky_pattern '/.*'" "/.* should be risky"

# Test 24: is_risky_pattern detects .*
echo "Test 24: Risky pattern - .*"
assert_true "is_risky_pattern '.*'" ".* should be risky"

# Test 25: matches_any_pattern negative case
echo "Test 25: Match no pattern"
assert_false "matches_any_pattern 'test.txt' '*.py' '*.js'" "Should not match txt"

# Test 26: extract_redirect_target no redirect
echo "Test 26: Extract redirect - no redirect"
RESULT=$(extract_redirect_target "echo test" || true)
assert_equals "" "$RESULT" "Should return empty for no redirect"

# Test 27: is_path_within with normalized paths
echo "Test 27: Path within - with ./prefix"
assert_true "is_path_within './src/file.py' 'src'" "Should handle ./prefix"

# Test 28: extract_file_paths with multiple paths
echo "Test 28: Extract multiple file paths"
RESULT=$(extract_file_paths "mv ./a.txt ./b.txt /c.txt")
if [[ "$RESULT" == *"a.txt"* ]] && [[ "$RESULT" == *"b.txt"* ]]; then
    echo "PASS: Should extract multiple paths"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Should extract multiple paths (got '$RESULT')"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 29: is_path_within - prefix boundary check (prevents false positives)
echo "Test 29: Path within - prefix boundary"
assert_false "is_path_within 'srcextra/file.py' 'src'" "srcextra should not be within src"

# Test 30: matches_extended_glob with deep recursive
echo "Test 30: Deep recursive glob match"
if matches_extended_glob 'a/b/c/d/e.py' 'a/**/*.py'; then
    echo "PASS: Should match deep path"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Should match deep path"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 31: matches_extended_glob with ** at end (directory match)
echo "Test 31: Recursive glob at end"
if matches_extended_glob '.claude/hooks/codeflow/pre-tool-use/hook.sh' '.claude/hooks/codeflow/**'; then
    echo "PASS: Should match ** at end"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Should match ** at end"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 32: normalize_path handles double slashes
echo "Test 32: Normalize path - double slashes"
RESULT=$(normalize_path "src//lib//file.py")
assert_equals "src/lib/file.py" "$RESULT" "Should remove double slashes"

# Test 33: source guard prevents double-sourcing
echo "Test 33: Source guard"
if [[ -n "${_PATTERN_MATCHING_SOURCED:-}" ]]; then
    echo "PASS: Source guard variable is set"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Source guard variable should be set after sourcing"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Test 34: is_risky_pattern - /tmp/* is risky
echo "Test 34: Risky pattern - /tmp/*"
assert_true "is_risky_pattern '/tmp/*'" "/tmp/* should be risky"

# Test 35: matches_extended_glob with dotfiles
echo "Test 35: Glob match with dotfile pattern"
assert_true "matches_extended_glob '.claude/settings.json' '.claude/*.json'" "Should match dotfile glob"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
