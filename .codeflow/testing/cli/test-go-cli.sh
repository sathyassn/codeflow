#!/usr/bin/env bash
set -euo pipefail

# test-go-cli.sh -- Bridge test script for the Go CLI.
# Runs Go unit tests and basic smoke tests (build, --version, --help).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
readonly REPO_ROOT
readonly CLI_DIR="$REPO_ROOT/codeflow-cli"

# Skip if Go is not available or version is insufficient for go.mod.
if ! command -v go &>/dev/null; then
    echo "=== Go CLI Bridge Tests ==="
    echo ""
    echo "SKIP: 'go' not found on PATH. Skipping Go CLI tests."
    exit 0
fi

# Check Go version meets go.mod requirement.
required_go=$(grep '^go ' "$CLI_DIR/go.mod" 2>/dev/null | awk '{print $2}')
if [[ -n "$required_go" ]]; then
    current_go=$(go version | grep -oE 'go[0-9]+\.[0-9]+' | sed 's/go//')
    if [[ "$(printf '%s\n' "$required_go" "$current_go" | sort -V | head -1)" != "$required_go" ]]; then
        echo "=== Go CLI Bridge Tests ==="
        echo ""
        echo "SKIP: Go $current_go < required $required_go. Skipping Go CLI tests."
        exit 0
    fi
fi

# Track pass/fail counts.
passed=0
failed=0
total=0

pass() {
    ((passed++))
    ((total++))
    echo "  PASS: $1"
}

fail() {
    ((failed++))
    ((total++))
    echo "  FAIL: $1"
}

# ---- Smoke tests ----

echo "=== Go CLI Bridge Tests ==="
echo ""

echo "--- Build test ---"
if (cd "$CLI_DIR" && go build -o "$CLI_DIR/bin/codeflow-test" ./cmd/codeflow/) 2>/dev/null; then
    pass "go build succeeds"
else
    fail "go build failed"
fi

readonly TEST_BINARY="$CLI_DIR/bin/codeflow-test"

if [[ -x "$TEST_BINARY" ]]; then
    echo ""
    echo "--- Smoke: codeflow --version ---"
    version_output=$("$TEST_BINARY" --version 2>&1) || true
    if [[ "$version_output" == codeflow* ]]; then
        pass "codeflow --version outputs version string"
    else
        fail "codeflow --version unexpected output: $version_output"
    fi

    echo ""
    echo "--- Smoke: codeflow --help ---"
    help_output=$("$TEST_BINARY" --help 2>&1) || true
    if echo "$help_output" | grep -q "Available Commands:"; then
        pass "codeflow --help shows Available Commands"
    else
        fail "codeflow --help missing 'Available Commands'"
    fi

    if echo "$help_output" | grep -q "version"; then
        pass "codeflow --help lists version subcommand"
    else
        fail "codeflow --help missing version subcommand"
    fi

    if echo "$help_output" | grep -q "uninstall"; then
        pass "codeflow --help lists uninstall subcommand"
    else
        fail "codeflow --help missing uninstall subcommand"
    fi

    echo ""
    echo "--- Smoke: codeflow version ---"
    ver_output=$("$TEST_BINARY" version 2>&1) || true
    if [[ "$ver_output" == codeflow* ]]; then
        pass "codeflow version outputs version string"
    else
        fail "codeflow version unexpected output: $ver_output"
    fi
else
    fail "test binary not found or not executable at $TEST_BINARY"
fi

echo ""
echo "--- Go unit tests ---"
if (cd "$CLI_DIR" && go test ./... -v -count=1) 2>&1; then
    pass "go test ./... passes"
else
    fail "go test ./... failed"
fi

# Cleanup test binary.
rm -f "$TEST_BINARY"

echo ""
echo "=== Results: $passed/$total passed, $failed failed ==="

if [[ $failed -gt 0 ]]; then
    exit 1
fi
