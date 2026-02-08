#!/usr/bin/env bash
# run-coverage.sh - Generate comprehensive test coverage reports
#
# Usage:
#   ./run-coverage.sh           Run all tests with coverage
#   ./run-coverage.sh python    Run Python tests only
#   ./run-coverage.sh shell     Run shell tests only (requires kcov)
#   ./run-coverage.sh report    Generate combined report
#   ./run-coverage.sh --strict  Enforce coverage thresholds (for CI)
#
# Coverage Policy:
#   - Python: Enforced at 85% line coverage (achievable, accurate)
#   - Shell:  Tests must PASS (kcov tracking is informational only)
#
# Exit Codes:
#   0 - All checks passed
#   1 - Python coverage below threshold (with --strict)
#   2 - Shell tests failed
#   3 - Both failed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COVERAGE_DIR="$SCRIPT_DIR/coverage_reports"

# Coverage thresholds (from test-config.json policy)
PYTHON_FAIL_UNDER=85
# shellcheck disable=SC2034  # SHELL_ENFORCEMENT used by coverage policy checks
SHELL_ENFORCEMENT="tests_pass"  # "tests_pass" or "line_coverage"

# Tracking variables
PYTHON_COVERAGE_PCT=0
SHELL_TESTS_PASSED=0
SHELL_TESTS_FAILED=0
STRICT_MODE=false

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
}

# Ensure coverage directory exists
mkdir -p "$COVERAGE_DIR"

run_python_coverage() {
    print_header "Running Python Tests with Coverage"

    # Save current directory and change to testing dir for Python tests
    local saved_dir
    saved_dir="$(pwd)"
    cd "$SCRIPT_DIR"

    # Activate venv and run pytest with coverage
    if [[ -f ".venv/bin/python" ]]; then
        # Set PYTHONPATH for proper coverage tracking
        local pytest_exit=0
        PYTHONPATH=../scripts .venv/bin/python -m pytest scripts/ \
            --cov=codeflow_py_lib \
            --cov-report=term-missing \
            --cov-report=html:"$COVERAGE_DIR/python_html" \
            --cov-report=xml:"$COVERAGE_DIR/python_coverage.xml" \
            -v --tb=short || pytest_exit=$?

        # Extract coverage percentage (use sed/awk for cross-platform compatibility)
        if [[ -f "$COVERAGE_DIR/python_coverage.xml" ]]; then
            local line_rate
            line_rate=$(sed -n 's/.*line-rate="\([0-9.]*\)".*/\1/p' "$COVERAGE_DIR/python_coverage.xml" | head -1)
            if [[ -n "$line_rate" ]]; then
                # Use awk for floating point math (more portable than bc)
                PYTHON_COVERAGE_PCT=$(awk "BEGIN {printf \"%.0f\", $line_rate * 100}" 2>/dev/null || echo "0")
            fi
        fi

        echo -e "\n${GREEN}✓ Python coverage report generated at: $COVERAGE_DIR/python_html/index.html${NC}"
        echo -e "  Line coverage: ${PYTHON_COVERAGE_PCT}% (threshold: ${PYTHON_FAIL_UNDER}%)"

        # Restore working directory
        cd "$saved_dir"

        if [[ $pytest_exit -ne 0 ]]; then
            echo -e "${RED}✗ Python tests failed${NC}"
            return 1
        fi
    else
        echo -e "${RED}Error: Python venv not found. Run: python3 -m venv .venv && .venv/bin/pip install -r requirements-test.txt${NC}"
        cd "$saved_dir"
        return 1
    fi
}

run_shell_coverage() {
    print_header "Running Shell Tests with Coverage"

    # Shell tests must run from repo root
    cd "$REPO_ROOT"

    # Check if kcov is available
    if command -v kcov &> /dev/null; then
        echo "Using kcov for shell coverage..."

        # Check for Homebrew bash on macOS (required for full coverage)
        HOMEBREW_BASH=""
        if [[ "$(uname)" == "Darwin" ]]; then
            if [[ -x "/opt/homebrew/bin/bash" ]]; then
                HOMEBREW_BASH="/opt/homebrew/bin/bash"
            elif [[ -x "/usr/local/bin/bash" ]]; then
                HOMEBREW_BASH="/usr/local/bin/bash"
            fi

            if [[ -n "$HOMEBREW_BASH" ]]; then
                echo -e "${GREEN}Found Homebrew bash: $HOMEBREW_BASH${NC}"
                echo "Using Homebrew bash for full coverage (bypasses SIP)."
            else
                echo -e "${YELLOW}Warning: Homebrew bash not found.${NC}"
                echo -e "${YELLOW}Install with: brew install bash${NC}"
                echo -e "${YELLOW}Sourced scripts may not be tracked on macOS.${NC}"
            fi
        fi
        echo ""

        SHELL_COVERAGE_DIR="$COVERAGE_DIR/shell_html"
        mkdir -p "$SHELL_COVERAGE_DIR"

        local test_dirs=(
            "consistency"
            "scripts/state"
            "scripts/shell-lib"
            "scripts/security"
            "scripts/security/lib"
            "scripts/db/lib"
            "scripts/git-hooks"
            "claude-hooks/pre-tool-use"
            "claude-hooks/post-tool-use"
            "claude-hooks/session-start"
            "claude-hooks/session-end"
            "claude-hooks/stop"
            "claude-hooks/user-prompt-submit"
        )

        local coverage_dirs=()

        # Create wrapper script for macOS (uses Homebrew bash to bypass SIP)
        # Note: Don't use set -euo pipefail in wrapper - kcov instrumentation conflicts with it
        local wrapper_script=""
        if [[ -n "$HOMEBREW_BASH" ]]; then
            wrapper_script="$SHELL_COVERAGE_DIR/.test-wrapper.sh"
            cat > "$wrapper_script" << WRAPPER
#!$HOMEBREW_BASH
exec "$HOMEBREW_BASH" "\$@"
WRAPPER
            chmod +x "$wrapper_script"
        fi

        for test_dir in "${test_dirs[@]}"; do
            local full_dir="$SCRIPT_DIR/$test_dir"
            if [[ -d "$full_dir" ]]; then
                local dir_name
                dir_name=$(basename "$test_dir" | tr '/' '-')

                # Find all test files
                for test_file in "$full_dir"/test-*.sh "$full_dir"/test_*.sh; do
                    if [[ -f "$test_file" ]]; then
                        local test_name
                        test_name=$(basename "$test_file" .sh)
                        echo "Running $test_name..."

                        local output_dir="$SHELL_COVERAGE_DIR/$dir_name-$test_name"

                        # Run test DIRECTLY for accurate pass/fail counting
                        # (kcov instrumentation causes false failures)
                        local test_output
                        local test_exit=0
                        test_output=$("$test_file" 2>&1) || test_exit=$?

                        # Parse test results from output (ensure numeric values)
                        local passed=0 failed=0

                        # Try "Passed: N" format first
                        local pass_match
                        pass_match=$(echo "$test_output" | grep -oE 'Passed:[[:space:]]*[0-9]+' | grep -oE '[0-9]+' | head -1 || true)
                        [[ -n "$pass_match" && "$pass_match" =~ ^[0-9]+$ ]] && passed="$pass_match"

                        local fail_match
                        fail_match=$(echo "$test_output" | grep -oE 'Failed:[[:space:]]*[0-9]+' | grep -oE '[0-9]+' | head -1 || true)
                        [[ -n "$fail_match" && "$fail_match" =~ ^[0-9]+$ ]] && failed="$fail_match"

                        # Try checkmark format if no results yet
                        if [[ "$passed" -eq 0 && "$failed" -eq 0 ]]; then
                            passed=$(echo "$test_output" | grep -c "✓" 2>/dev/null || echo "0")
                            failed=$(echo "$test_output" | grep -c "✗" 2>/dev/null || echo "0")
                        fi

                        # Ensure we have valid integers
                        [[ "$passed" =~ ^[0-9]+$ ]] || passed=0
                        [[ "$failed" =~ ^[0-9]+$ ]] || failed=0

                        SHELL_TESTS_PASSED=$((SHELL_TESTS_PASSED + passed))
                        SHELL_TESTS_FAILED=$((SHELL_TESTS_FAILED + failed))

                        # Run through kcov for informational coverage (don't count results)
                        if [[ -n "$wrapper_script" ]]; then
                            kcov --include-path="$REPO_ROOT/.codeflow/scripts,$REPO_ROOT/.claude/hooks" \
                                 "$output_dir" \
                                 "$wrapper_script" "$test_file" >/dev/null 2>&1 || true
                        else
                            kcov --bash-dont-parse-binary-dir \
                                 --include-path="$REPO_ROOT/.codeflow/scripts,$REPO_ROOT/.claude/hooks" \
                                 "$output_dir" \
                                 "$test_file" >/dev/null 2>&1 || true
                        fi

                        # Track if kcov reports exist
                        if [[ -d "$output_dir" ]]; then
                            coverage_dirs+=("$output_dir")
                        fi

                        # Report per-test status based on direct execution
                        if [[ "$failed" -gt 0 ]]; then
                            echo -e "  ${RED}✗${NC} $test_name (passed: $passed, failed: $failed)"
                        elif [[ $test_exit -ne 0 ]]; then
                            # Test exited with error but we couldn't parse results
                            echo -e "  ${RED}✗${NC} $test_name (exit code: $test_exit)"
                            SHELL_TESTS_FAILED=$((SHELL_TESTS_FAILED + 1))
                        elif [[ "$passed" -gt 0 ]]; then
                            echo -e "  ${GREEN}✓${NC} $test_name (passed: $passed)"
                        else
                            # No pass/fail counts detected but test exited successfully
                            echo -e "  ${GREEN}✓${NC} $test_name (completed)"
                        fi
                    fi
                done
            fi
        done

        # Clean up wrapper
        [[ -n "$wrapper_script" && -f "$wrapper_script" ]] && rm -f "$wrapper_script"

        # Merge all reports
        if [[ ${#coverage_dirs[@]} -gt 1 ]]; then
            echo ""
            echo "Merging ${#coverage_dirs[@]} coverage reports..."
            kcov --merge "$SHELL_COVERAGE_DIR/merged" "${coverage_dirs[@]}" 2>/dev/null || true
            echo -e "\n${GREEN}✓ Shell coverage report generated at: $SHELL_COVERAGE_DIR/merged/index.html${NC}"
        elif [[ ${#coverage_dirs[@]} -eq 1 ]]; then
            echo -e "\n${GREEN}✓ Shell coverage report generated at: ${coverage_dirs[0]}/index.html${NC}"
        fi

        # Shell test summary
        echo ""
        echo -e "${BLUE}Shell Test Summary:${NC}"
        echo -e "  Total passed: ${GREEN}$SHELL_TESTS_PASSED${NC}"
        if [[ $SHELL_TESTS_FAILED -gt 0 ]]; then
            echo -e "  Total failed: ${RED}$SHELL_TESTS_FAILED${NC}"
        else
            echo -e "  Total failed: $SHELL_TESTS_FAILED"
        fi
        echo -e "  ${YELLOW}Note: kcov % is informational only (cannot track sourced files)${NC}"
    else
        echo -e "${YELLOW}⚠ kcov not found. Running tests without coverage.${NC}"
        echo "  To install kcov:"
        echo "    macOS: brew install kcov"
        echo "    Ubuntu: apt-get install kcov"
        echo "    Fedora: dnf install kcov"
        echo ""

        # Run tests without coverage
        "$SCRIPT_DIR/run-all-tests.sh"
    fi
}

generate_summary() {
    print_header "Coverage Summary"

    local python_status="✓"
    local shell_status="✓"
    local exit_code=0

    echo "Coverage reports generated:"
    echo ""

    # Python coverage check
    if [[ -f "$COVERAGE_DIR/python_html/index.html" ]]; then
        if [[ $PYTHON_COVERAGE_PCT -ge $PYTHON_FAIL_UNDER ]]; then
            echo -e "  ${GREEN}✓${NC} Python: $COVERAGE_DIR/python_html/index.html"
            echo -e "       Line coverage: ${GREEN}${PYTHON_COVERAGE_PCT}%${NC} (threshold: ${PYTHON_FAIL_UNDER}%)"
        else
            python_status="✗"
            echo -e "  ${RED}✗${NC} Python: $COVERAGE_DIR/python_html/index.html"
            echo -e "       Line coverage: ${RED}${PYTHON_COVERAGE_PCT}%${NC} (threshold: ${PYTHON_FAIL_UNDER}%)"
            if [[ "$STRICT_MODE" == "true" ]]; then
                exit_code=1
            fi
        fi
    fi

    # Shell tests check
    if [[ -d "$COVERAGE_DIR/shell_html/merged" ]]; then
        echo -e "  ${GREEN}✓${NC} Shell:  $COVERAGE_DIR/shell_html/merged/index.html"
    elif [[ -d "$COVERAGE_DIR/shell_html" ]]; then
        echo -e "  ${YELLOW}⚠${NC} Shell:  Individual reports in $COVERAGE_DIR/shell_html/"
    fi

    if [[ $SHELL_TESTS_FAILED -gt 0 ]]; then
        shell_status="✗"
        echo -e "       Tests: ${RED}$SHELL_TESTS_PASSED passed, $SHELL_TESTS_FAILED failed${NC}"
        exit_code=$((exit_code + 2))
    elif [[ $SHELL_TESTS_PASSED -gt 0 ]]; then
        echo -e "       Tests: ${GREEN}$SHELL_TESTS_PASSED passed${NC}, $SHELL_TESTS_FAILED failed"
    fi
    echo -e "       ${YELLOW}(kcov % is informational - cannot track sourced files)${NC}"

    echo ""
    echo "To view HTML reports, open in browser:"
    # Provide cross-platform command hint
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "  open $COVERAGE_DIR/python_html/index.html"
    elif command -v xdg-open &>/dev/null; then
        echo "  xdg-open $COVERAGE_DIR/python_html/index.html"
    else
        echo "  $COVERAGE_DIR/python_html/index.html"
    fi

    # Policy summary
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  Coverage Policy Enforcement${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo -e "  Python (line coverage ≥ ${PYTHON_FAIL_UNDER}%): $(if [[ "$python_status" == "✓" ]]; then echo -e "${GREEN}PASS${NC}"; else echo -e "${RED}FAIL${NC}"; fi)"
    echo -e "  Shell  (tests must pass):       $(if [[ "$shell_status" == "✓" ]]; then echo -e "${GREEN}PASS${NC}"; else echo -e "${RED}FAIL${NC}"; fi)"
    echo ""

    if [[ $exit_code -eq 0 ]]; then
        echo -e "  ${GREEN}All coverage requirements met.${NC}"
    else
        if [[ "$STRICT_MODE" == "true" ]]; then
            echo -e "  ${RED}Coverage requirements not met (--strict mode).${NC}"
        else
            echo -e "  ${YELLOW}Coverage requirements not met (use --strict to enforce).${NC}"
        fi
    fi

    return $exit_code
}

# Parse arguments
COMMAND="all"
for arg in "$@"; do
    case "$arg" in
        --strict)
            STRICT_MODE=true
            ;;
        python|shell|report|all)
            COMMAND="$arg"
            ;;
        --help|-h)
            echo "Usage: $0 [python|shell|report|all] [--strict]"
            echo ""
            echo "Commands:"
            echo "  python    Run Python tests with coverage"
            echo "  shell     Run shell tests with coverage (kcov)"
            echo "  report    Show coverage summary"
            echo "  all       Run all tests (default)"
            echo ""
            echo "Options:"
            echo "  --strict  Enforce coverage thresholds (exit non-zero if failed)"
            echo ""
            echo "Coverage Policy:"
            echo "  - Python: Line coverage must be ≥ ${PYTHON_FAIL_UNDER}%"
            echo "  - Shell:  All tests must pass (kcov % is informational)"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Usage: $0 [python|shell|report|all] [--strict]"
            exit 1
            ;;
    esac
done

# Main execution
exit_code=0

case "$COMMAND" in
    python)
        run_python_coverage || exit_code=$?
        generate_summary || exit_code=$?
        ;;
    shell)
        run_shell_coverage || exit_code=$?
        generate_summary || exit_code=$?
        ;;
    report)
        generate_summary || exit_code=$?
        ;;
    all)
        run_python_coverage || exit_code=$?
        run_shell_coverage || ((exit_code += $?))
        generate_summary || ((exit_code += $?))
        ;;
esac

if [[ "$STRICT_MODE" == "true" && $exit_code -ne 0 ]]; then
    echo -e "\n${RED}Exiting with code $exit_code due to --strict mode.${NC}"
fi

exit $exit_code
