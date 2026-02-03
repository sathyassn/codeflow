#!/usr/bin/env bash
# run-coverage.sh - Generate comprehensive test coverage reports
#
# Usage:
#   ./run-coverage.sh           Run all tests with coverage
#   ./run-coverage.sh python    Run Python tests only
#   ./run-coverage.sh shell     Run shell tests only (requires kcov)
#   ./run-coverage.sh report    Generate combined report

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COVERAGE_DIR="$SCRIPT_DIR/coverage_reports"

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

    cd "$SCRIPT_DIR"

    # Activate venv and run pytest with coverage
    if [[ -f ".venv/bin/python" ]]; then
        # Set PYTHONPATH for proper coverage tracking
        PYTHONPATH=../scripts .venv/bin/python -m pytest scripts/ \
            --cov=codeflow_py_lib \
            --cov-report=term-missing \
            --cov-report=html:"$COVERAGE_DIR/python_html" \
            --cov-report=xml:"$COVERAGE_DIR/python_coverage.xml" \
            -v --tb=short

        echo -e "\n${GREEN}✓ Python coverage report generated at: $COVERAGE_DIR/python_html/index.html${NC}"
    else
        echo -e "${RED}Error: Python venv not found. Run: python3 -m venv .venv && .venv/bin/pip install -r requirements-test.txt${NC}"
        return 1
    fi
}

run_shell_coverage() {
    print_header "Running Shell Tests with Coverage"

    # Check if kcov is available
    if command -v kcov &> /dev/null; then
        echo "Using kcov for shell coverage..."

        SHELL_COVERAGE_DIR="$COVERAGE_DIR/shell_html"
        mkdir -p "$SHELL_COVERAGE_DIR"

        # Run ledger tests with kcov
        if [[ -f "$SCRIPT_DIR/scripts/state/test-ledger.sh" ]]; then
            echo "Running ledger tests..."
            kcov --include-path="$REPO_ROOT/.codeflow/scripts" \
                 "$SHELL_COVERAGE_DIR/ledger" \
                 bash "$SCRIPT_DIR/scripts/state/test-ledger.sh" 2>/dev/null || true
        fi

        # Run memory tests with kcov
        if [[ -f "$SCRIPT_DIR/scripts/state/test-memory.sh" ]]; then
            echo "Running memory tests..."
            kcov --include-path="$REPO_ROOT/.codeflow/scripts" \
                 "$SHELL_COVERAGE_DIR/memory" \
                 bash "$SCRIPT_DIR/scripts/state/test-memory.sh" 2>/dev/null || true
        fi

        # Merge reports
        if [[ -d "$SHELL_COVERAGE_DIR/ledger" ]] && [[ -d "$SHELL_COVERAGE_DIR/memory" ]]; then
            kcov --merge "$SHELL_COVERAGE_DIR/merged" \
                 "$SHELL_COVERAGE_DIR/ledger" \
                 "$SHELL_COVERAGE_DIR/memory" 2>/dev/null || true
            echo -e "\n${GREEN}✓ Shell coverage report generated at: $SHELL_COVERAGE_DIR/merged/index.html${NC}"
        fi
    else
        echo -e "${YELLOW}⚠ kcov not found. Running tests without coverage.${NC}"
        echo "  To install kcov on macOS: brew install kcov"
        echo "  To install kcov on Linux: apt-get install kcov"
        echo ""

        # Run tests without coverage
        cd "$SCRIPT_DIR/scripts/state"

        echo "Running ledger tests..."
        bash test-ledger.sh

        echo "Running memory tests..."
        bash test-memory.sh
    fi
}

generate_summary() {
    print_header "Coverage Summary"

    echo "Coverage reports generated:"
    echo ""

    if [[ -f "$COVERAGE_DIR/python_html/index.html" ]]; then
        echo -e "  ${GREEN}✓${NC} Python: $COVERAGE_DIR/python_html/index.html"

        # Extract coverage percentage if available
        if [[ -f "$COVERAGE_DIR/python_coverage.xml" ]]; then
            coverage_pct=$(grep -oP 'line-rate="\K[0-9.]+' "$COVERAGE_DIR/python_coverage.xml" 2>/dev/null | head -1 || echo "")
            if [[ -n "$coverage_pct" ]]; then
                pct_display=$(echo "$coverage_pct * 100" | bc 2>/dev/null || echo "$coverage_pct")
                echo "       Line coverage: ${pct_display}%"
            fi
        fi
    fi

    if [[ -d "$COVERAGE_DIR/shell_html/merged" ]]; then
        echo -e "  ${GREEN}✓${NC} Shell:  $COVERAGE_DIR/shell_html/merged/index.html"
    elif [[ -d "$COVERAGE_DIR/shell_html" ]]; then
        echo -e "  ${YELLOW}⚠${NC} Shell:  Individual reports in $COVERAGE_DIR/shell_html/"
    fi

    echo ""
    echo "To view HTML reports, open in browser:"
    echo "  open $COVERAGE_DIR/python_html/index.html"
}

# Main
case "${1:-all}" in
    python)
        run_python_coverage
        ;;
    shell)
        run_shell_coverage
        ;;
    report)
        generate_summary
        ;;
    all)
        run_python_coverage
        run_shell_coverage
        generate_summary
        ;;
    *)
        echo "Usage: $0 [python|shell|report|all]"
        exit 1
        ;;
esac
