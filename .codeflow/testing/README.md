# CodeFlow Testing Framework

The CodeFlow testing framework provides **config-driven test execution** with **coverage enforcement** for both Python and shell scripts. Tests are organized by priority levels, allowing you to run quick validations during development or comprehensive suites before merging.

## Table of Contents

- [Quick Start](#quick-start)
- [Command Reference](#command-reference)
- [Concepts](#concepts)
  - [Test Modes](#test-modes)
  - [Priority Levels](#priority-levels)
  - [Test Categories](#test-categories)
- [CLI Tools](#cli-tools)
  - [codeflow test](#codeflow-test-primary-cli)
  - [run-all-tests.sh](#run-all-testssh-shell-runner)
  - [run-coverage.sh](#run-coveragesh-coverage-runner)
- [Coverage Policy](#coverage-policy)
- [Configuration](#configuration)
- [Writing Tests](#writing-tests)
- [Directory Structure](#directory-structure)
- [Git Integration](#git-integration)
- [Troubleshooting](#troubleshooting)

---

## Quick Start

Run tests from the **project root** directory:

```bash
# Run tests in standard mode (recommended for development)
codeflow test

# Run with coverage enforcement (85% threshold)
codeflow test --coverage

# Run full test suite before creating a PR
codeflow test --mode full --coverage
```

---

## Command Reference

All commands are run from the **project root**. This section provides a quick lookup for common testing scenarios.

### Running Tests

| Goal | Command |
|------|---------|
| Run standard tests | `codeflow test` |
| Run essential tests only | `codeflow test --mode essential` |
| Run full test suite | `codeflow test --mode full` |
| Run with coverage check | `codeflow test --coverage` |
| Full suite + coverage | `codeflow test --mode full --coverage` |
| Verbose output | `codeflow test --verbose` |

### Running Specific Tests

| Goal | Command |
|------|---------|
| Run single file | `codeflow test scripts/db/test_schema.py` |
| Single file + coverage | `codeflow test scripts/db/test_schema.py -c` |
| Run by category | `.codeflow/testing/run-all-tests.sh --category scripts-db` |
| Category + verbose | `.codeflow/testing/run-all-tests.sh --category scripts-security -v` |

### Coverage Reports

| Goal | Command |
|------|---------|
| Generate full report | `.codeflow/testing/run-coverage.sh` |
| Python coverage only | `.codeflow/testing/run-coverage.sh python` |
| Shell coverage only | `.codeflow/testing/run-coverage.sh shell` |
| CI mode (strict) | `.codeflow/testing/run-coverage.sh --strict` |
| View HTML report | `open .codeflow/testing/coverage_reports/python_html/index.html` |

### Discovery and Debugging

| Goal | Command |
|------|---------|
| List all test files | `codeflow test --list` |
| List files for mode | `codeflow test --list --mode full` |
| Preview what would run | `.codeflow/testing/run-all-tests.sh --dry-run` |
| Stop on first failure | `.codeflow/testing/run-all-tests.sh --stop-on-fail` |

### Script-to-Test Coverage Validation

| Goal | Command |
|------|---------|
| Validate all scripts have tests | `.codeflow/testing/lib/test-coverage.sh` |
| Audit mode (report only) | `.codeflow/testing/lib/test-coverage.sh --audit` |
| Fail mode (CI enforcement) | `.codeflow/testing/lib/test-coverage.sh --fail` |
| Check staged files only | `.codeflow/testing/lib/test-coverage.sh --staged` |
| Find orphaned test files | `.codeflow/testing/lib/test-coverage.sh --orphaned` |

---

## Concepts

Understanding these concepts helps you choose the right testing approach for your situation.

### Test Modes

Tests are organized into **modes** that determine which priority levels run. Choose a mode based on how thorough you need to be:

| Mode | Priorities Included | When to Use |
|------|---------------------|-------------|
| `essential` | CRITICAL only | Quick validation, pre-commit hooks |
| `standard` | CRITICAL + HIGH | Default for development (recommended) |
| `full` | All priorities | Before creating a PR, CI pipelines |

**Examples:**

```bash
codeflow test --mode essential   # Fast check (~20s)
codeflow test                    # Standard mode (~30s with parallel)
codeflow test --mode full        # Comprehensive (~40s with parallel)
```

### Parallel Execution

By default, `run-all-tests.sh` runs test categories in parallel using up to 6 concurrent jobs. This typically reduces wall-clock time by 2-3x compared to sequential execution.

| Flag | Effect |
|------|--------|
| *(default)* | Parallel with 6 jobs |
| `--jobs N` | Override max parallel jobs |
| `--sequential` | Force sequential execution (useful for debugging) |

Single-category runs (`--category`) always execute sequentially since there is only one unit of work.

The default job count is configured in `test-config.json`:

```json
"parallel": {
    "max_jobs": 6
}
```

### Priority Levels

Each test file is assigned a priority based on what it tests:

| Priority | What It Tests | Examples |
|----------|---------------|----------|
| **CRITICAL** | Core data integrity | Schema validation, CRDT operations, data consistency |
| **HIGH** | Key functionality | Memory management, security enforcement, workflows |
| **MEDIUM** | Supporting features | Library utilities, hooks, git operations |
| **LOW** | Peripheral features | Logging, error formatting, support utilities |

Priorities are defined in `test-config.json` under the `priorities` section.

### Test Categories

Tests are grouped into **22 categories** for targeted testing. Use categories when you want to test a specific subsystem without running the full suite.

**Core Categories:**

| Category | Tests |
|----------|-------|
| `scripts-db` | Database schema tests |
| `scripts-memory` | Memory management (pending Go CLI) |
| `scripts-coordination` | CRDT and coordination |
| `scripts-codeflow-py-lib` | Python shared library |
| `scripts-shell-lib` | Shell shared library |
| `scripts-state` | State management |

**Security Categories:**

| Category | Tests |
|----------|-------|
| `scripts-security` | Root security tests |
| `scripts-security-enforcement` | Enforcement modules |
| `scripts-security-lib` | Security library |
| `scripts-security-protection` | OS-level protection |
| `scripts-security-sentinel` | Sentinel system |

**Infrastructure Categories:**

| Category | Tests |
|----------|-------|
| `scripts-settings` | Settings management |
| `scripts-git-hooks` | Git hook tests |
| `consistency` | Cross-cutting consistency checks |

**Claude Hook Categories:**

| Category | Tests |
|----------|-------|
| `claude-hooks-pre-tool-use` | Pre-tool-use hooks |
| `claude-hooks-post-tool-use` | Post-tool-use hooks |
| `claude-hooks-session-start` | Session start hooks |
| `claude-hooks-session-end` | Session end hooks |
| `claude-hooks-stop` | Stop hooks |
| `claude-hooks-user-prompt-submit` | User prompt submit hooks |

**Example:**

```bash
# Run only security tests
.codeflow/testing/run-all-tests.sh --category scripts-security

# Run only database tests with verbose output
.codeflow/testing/run-all-tests.sh --category scripts-db --verbose
```

---

## CLI Tools

Three CLI tools are available, each suited for different use cases.

### `codeflow test` (Primary CLI)

The **recommended** way to run tests. This is the project's main CLI entry point.

```bash
codeflow test [options] [file]
```

**Options:**

| Flag | Description |
|------|-------------|
| `--mode, -m <mode>` | Test mode: `essential`, `standard`, `full` (default: standard) |
| `--coverage, -c` | Enable coverage enforcement (85% threshold) |
| `--verbose, -v` | Show detailed output |
| `--list, -l` | List test files without running them |
| `--file, -f <path>` | Run specific test file (alternative to positional arg) |
| `--per-file-check` | Enforce per-file coverage threshold (default: enabled) |
| `--no-per-file-check` | Disable per-file coverage check |

**Examples:**

```bash
codeflow test                              # Standard mode
codeflow test --mode full --coverage       # Full suite with coverage
codeflow test scripts/db/test_schema.py    # Single file
codeflow test --list --mode full           # List all test files
```

### `run-all-tests.sh` (Shell Runner)

Alternative shell-based runner with **category support** and additional features like dry-run and stop-on-fail.

```bash
.codeflow/testing/run-all-tests.sh [options]
```

**Options:**

| Flag | Description |
|------|-------------|
| `--mode <mode>` | Test mode: `essential`, `standard`, `full` |
| `--category <name>` | Run only tests in specified category |
| `--verbose, -v` | Show detailed output |
| `--stop-on-fail` | Stop execution on first test failure |
| `--dry-run` | Show what would run without executing |
| `--coverage` | Enable coverage enforcement |
| `--report` | Generate JSON and text reports |
| `--validate-coverage` | Validate test coverage mapping before running |
| `--jobs N` | Max parallel category jobs (default: 6) |
| `--sequential` | Force sequential execution (for debugging) |

**Examples:**

```bash
.codeflow/testing/run-all-tests.sh --category scripts-security
.codeflow/testing/run-all-tests.sh --dry-run --mode full
.codeflow/testing/run-all-tests.sh --stop-on-fail --verbose
```

### `run-coverage.sh` (Coverage Runner)

Dedicated tool for **generating coverage reports**. Use this for detailed coverage analysis or CI enforcement.

```bash
.codeflow/testing/run-coverage.sh [command] [options]
```

**Commands:**

| Command | Description |
|---------|-------------|
| *(none)* | Run all tests and generate combined coverage report |
| `python` | Run Python tests only with coverage |
| `shell` | Run shell tests only with coverage (requires kcov) |
| `report` | Generate combined report from existing data |
| `--strict` | Enforce coverage thresholds (for CI) |

**Examples:**

```bash
.codeflow/testing/run-coverage.sh              # Full coverage run
.codeflow/testing/run-coverage.sh python       # Python only
.codeflow/testing/run-coverage.sh --strict     # Fail if below threshold
```

---

## Script-to-Test Coverage Validation

The testing framework automatically validates that scripts have corresponding test files and that those tests are registered in `test-config.json`.

### What It Checks

| Check | Description |
|-------|-------------|
| **Missing tests** | Scripts without any test file |
| **Misplaced tests** | Test files in wrong directory |
| **Unregistered tests** | Test files not in `test-config.json` priorities |
| **Orphaned tests** | Test files without matching scripts |

### Validation Modes

| Mode | Behavior |
|------|----------|
| `audit` | Report issues, always return success |
| `warn` | Report issues as warnings, return success (default) |
| `fail` | Report issues and fail if any found |

### Usage

```bash
# Default validation (uses mode from config)
.codeflow/testing/lib/test-coverage.sh

# Force specific mode
.codeflow/testing/lib/test-coverage.sh --audit
.codeflow/testing/lib/test-coverage.sh --fail

# Check only staged files (for pre-commit)
.codeflow/testing/lib/test-coverage.sh --staged

# Find orphaned tests (tests without scripts)
.codeflow/testing/lib/test-coverage.sh --orphaned
```

### Naming Conventions

The validator derives expected test paths from script paths:

| Script Path | Expected Test Path |
|-------------|-------------------|
| `.codeflow/scripts/{domain}/{name}.sh` | `.codeflow/testing/scripts/{domain}/test-{name}.sh` |
| `.codeflow/scripts/{domain}/{name}.py` | `.codeflow/testing/scripts/{domain}/test_{name}.py` |
| `.claude/hooks/codeflow/{category}/{name}.sh` | `.codeflow/testing/claude-hooks/{category}/test-{name}.sh` |

### Exceptions

Scripts can be excepted from coverage requirements via `test-config.json`:

```json
{
  "coverage_enforcement": {
    "exceptions": {
      "no_test_required": [
        { "pattern": "lib/test-*.sh", "reason": "Test framework files" }
      ],
      "integration_tested": [
        { "pattern": ".codeflow/scripts/security/enforcement/*",
          "tested_by": "scripts/security/enforcement/test-cf-*.sh",
          "reason": "Has dedicated tests" }
      ]
    }
  }
}
```

---

## Coverage Policy

Coverage enforcement ensures code changes are properly tested.

### Python Coverage

Python tests use `coverage.py` with `pytest-cov` for line coverage measurement.

| Setting | Value |
|---------|-------|
| Enforcement | Line coverage |
| Threshold | 85% fail_under |
| Per-file check | Enabled by default |
| Tool | coverage.py + pytest-cov |

### Shell Coverage

Shell scripts use a "tests must pass" policy. Line coverage is tracked informationally using kcov but not enforced.

| Setting | Value |
|---------|-------|
| Enforcement | Tests must pass |
| Tracking | Informational (kcov) |
| Limitation | kcov cannot accurately track sourced files |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | All checks passed |
| `1` | Python coverage below threshold |
| `2` | Shell tests failed |
| `3` | Both Python coverage and shell tests failed |

---

## Configuration

Test behavior is controlled by `test-config.json`. This file defines priorities, modes, categories, and coverage settings.

### Configuration Structure

```json
{
  "version": "2.0.0",
  "priorities": {
    "CRITICAL": { "files": ["scripts/db/test_schema.py", "..."] },
    "HIGH": { "files": ["scripts/security/test_enforcement.py", "..."] },
    "MEDIUM": { "files": ["scripts/shell-lib/test-config.sh", "..."] },
    "LOW": { "files": ["scripts/support/test-logging.sh", "..."] }
  },
  "modes": {
    "essential": { "priorities": ["CRITICAL"] },
    "standard": { "priorities": ["CRITICAL", "HIGH"] },
    "full": { "priorities": ["CRITICAL", "HIGH", "MEDIUM", "LOW"] }
  },
  "categories": {
    "scripts-db": { "directory": "scripts/db" },
    "scripts-security": { "directory": "scripts/security" }
  },
  "parallel": {
      "max_jobs": 6,
      "progress_style": "compact"
  },
  "coverage_enforcement": {
    "python": { "fail_under": 85, "enforcement": "line_coverage" },
    "shell": { "enforcement": "tests_pass" }
  }
}
```

### Key Configuration Sections

| Section | Purpose |
|---------|---------|
| `priorities` | Maps test files to priority levels |
| `modes` | Defines which priorities each mode includes |
| `categories` | Maps category names to test directories |
| `coverage_enforcement` | Sets coverage thresholds and enforcement mode |
| `parallel` | Controls parallel category execution |

---

## Writing Tests

### Python Tests

Python tests use pytest. Place them in the appropriate `scripts/` subdirectory.

```python
# scripts/codeflow_py_lib/test_example.py
import pytest

def test_example_function():
    """Test that example_function returns expected value."""
    result = example_function()
    assert result == expected_value

@pytest.mark.critical  # Mark as CRITICAL priority
def test_critical_feature():
    """Critical tests run in all modes."""
    pass
```

**Conventions:**

- File names: `test_*.py` or `*_test.py`
- Function names: `test_*`
- Use `@pytest.mark.critical` for CRITICAL priority tests

### Shell Tests

Shell tests use the test framework helpers. Place them in the appropriate `scripts/` subdirectory.

```bash
#!/usr/bin/env bash
# scripts/example/test-example.sh
set -euo pipefail

# Source the test framework
source "$(dirname "$0")/../../lib/test-helpers.sh"

test_section "Example Tests"

# Test 1: Description
TESTS_RUN=$((TESTS_RUN + 1))
if some_condition; then
    test_pass "Condition was true"
else
    test_fail "Condition was false"
fi

# Print summary and exit
print_test_summary
[[ $TEST_FAIL_COUNT -eq 0 ]] || exit 1
```

**Conventions:**

- File names: `test-*.sh`
- Source `lib/test-helpers.sh` for test utilities
- Use `test_pass` and `test_fail` for reporting
- Always call `print_test_summary` at the end

### Registering New Tests

After creating a test file, add it to `test-config.json`:

1. Add the file path to the appropriate priority level in `priorities`
2. Ensure the category exists in `categories` if it's a new directory

---

## Directory Structure

```text
.codeflow/testing/
├── test                      # Main Python test CLI
├── run-all-tests.sh          # Shell-based test runner
├── run-coverage.sh           # Coverage report generator
├── test-config.json          # Test configuration
├── pytest.ini                # Pytest configuration
├── .coveragerc               # Coverage.py configuration
├── README.md                 # This documentation
│
├── lib/                      # Test framework libraries
│   ├── test-common.sh        # Common utilities (logging, colors)
│   ├── test-config.sh        # Config loading from JSON
│   ├── test-coverage.sh      # Coverage utilities
│   ├── test-discovery.sh     # Test file discovery
│   ├── test-helpers.sh       # Test assertions (pass/fail)
│   ├── test-reporting.sh     # Report generation
│   ├── test-runner.sh        # Test execution engine
│   └── test-parallel.sh      # Parallel execution engine
│
├── consistency/              # Cross-cutting consistency tests
│   └── test-settings-sync.sh # Settings template consistency
│
├── scripts/                  # Script tests (mirrors .codeflow/scripts/)
│   ├── db/                   # Database tests
│   ├── memory/               # Memory management tests
│   ├── coordination/         # CRDT tests
│   ├── codeflow_py_lib/      # Python library tests
│   ├── shell-lib/            # Shell library tests
│   ├── state/                # State management tests
│   ├── security/             # Security tests
│   │   ├── enforcement/      # Enforcement module tests
│   │   ├── lib/              # Security library tests
│   │   ├── protection/       # OS protection tests
│   │   └── sentinel/         # Sentinel tests
│   ├── settings/             # Settings tests
│   └── git-hooks/            # Git hook tests
│
├── claude-hooks/             # Claude hook tests
│   ├── pre-tool-use/
│   ├── post-tool-use/
│   ├── session-start/
│   ├── session-end/
│   ├── stop/
│   └── user-prompt-submit/
│
└── coverage_reports/         # Generated reports (gitignored)
    ├── python_html/          # HTML coverage reports
    └── coverage.json         # JSON coverage data
```

---

## Git Integration

Tests integrate with git hooks for automatic validation.

### Pre-commit Hook

The pre-commit hook runs the settings consistency check before each commit:

```bash
# Runs automatically on git commit
# Executes: .codeflow/testing/consistency/test-settings-sync.sh
```

This ensures settings templates stay synchronized.

### Configuration

Git integration is configured in `test-config.json`:

```json
{
  "pre_commit": {
    "default_mode": "essential",
    "consistency_check": {
      "enabled": true,
      "script": "consistency/test-settings-sync.sh"
    }
  }
}
```

### CI Integration

For CI pipelines, use strict mode to enforce coverage thresholds:

```bash
# In CI pipeline
codeflow test --mode full --coverage
.codeflow/testing/run-coverage.sh --strict
```

---

## Troubleshooting

### Python venv not found

If you see errors about missing Python dependencies:

```bash
cd .codeflow/testing
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

### Coverage below threshold

To investigate coverage failures:

```bash
# Run with verbose to see per-file coverage
codeflow test --coverage --verbose

# View detailed HTML report
open .codeflow/testing/coverage_reports/python_html/index.html
```

### Test discovery issues

If tests aren't being found:

```bash
# List what the framework sees
codeflow test --list --mode full

# Preview execution without running
.codeflow/testing/run-all-tests.sh --dry-run

# Check test-config.json for missing file registrations
```

### Shell test failures

For debugging shell test failures:

```bash
# Run specific category with verbose output
.codeflow/testing/run-all-tests.sh --category scripts-security --verbose

# Run with stop-on-fail to isolate the first failure
.codeflow/testing/run-all-tests.sh --stop-on-fail
```
