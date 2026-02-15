---
name: cf-script-standards
description: Provides script quality enforcement through linting and templates. Validates shell scripts with ShellCheck and Python with flake8/ruff. Use when creating or editing .sh or .py files.
context: fork
agent: cf-general-purpose
---

# Script Standards Skill

## Type

**Procedural** - Provides step-by-step script validation and templating.

## Purpose

**Ensures shell and Python scripts meet project standards through automated linting and template application.**

## Responsibilities

- Lint shell scripts with ShellCheck
- Lint Python scripts with flake8/ruff
- Apply standard templates for new scripts
- Ensure test coverage for critical scripts
- NOT: Script execution (that's Bash tool)
- NOT: Script logic review (that's code review)

## Decision Tree

```text
Creating new shell script?
├── Check OS compatibility → 🔧 apply-shell-standards (with OS detection)
├── Script > 200 lines? → Consider modularization
└── After creation → 🔧 lint-shell

Creating new Python script?
├── 🔧 apply-python-standards
├── Script > 200 lines? → Consider modularization
└── After creation → 🔧 lint-python

Editing shell script?
└── 🔧 lint-shell (after edit)

Editing Python script?
└── 🔧 lint-python (after edit)

Critical script modified?
├── 🔧 ensure-test-coverage
└── 🔧 register-test (add to test-config.json)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | lint-shell | ENF-L1 Sentinel | ShellCheck validation |
| 2 | lint-python | ENF-L1 Sentinel | flake8/ruff validation |
| 3 | apply-shell-standards | ENF-L1 Sentinel | New shell script template |
| 4 | apply-python-standards | ENF-L1 Sentinel | New Python script template |
| 5 | ensure-test-coverage | ENF-L3 Advisory | Verify test exists |
| 6 | register-test | ENF-L3 Advisory | Add test to test-config.json |
| 7 | check-modularization | ENF-L3 Advisory | Verify script size limits |

## Operation Details

### 🔧 lint-shell

```text
When: After creating/editing shell scripts
Enforcement: ENF-L1 Sentinel

Procedure:
  1. Run ShellCheck on script:
     shellcheck -x -s bash {script}

  2. Parse results by severity:
     | Code | Severity | Action |
     |------|----------|--------|
     | SC1xxx | Error | Must fix |
     | SC2xxx | Warning | Should fix |
     | SC3xxx | Info | Consider |

  3. Report issues with fix suggestions

Output:
  passed: true | false
  errors: [{code, line, message, fix}...]
  warnings: [{code, line, message}...]

On Failure:
  - If errors present: BLOCK with fix guidance
  - If warnings only: WARN but allow

📚 Resource: [shell-essentials.md](resources/shell-essentials.md)
   Load when: Understanding ShellCheck error codes or fixing shell issues
```

### 🔧 lint-python

```text
When: After creating/editing Python scripts
Enforcement: ENF-L1 Sentinel

Procedure:
  1. Run linter (prefer ruff, fallback flake8):
     ruff check {script} || flake8 {script}

  2. Parse results by category:
     | Category | Examples |
     |----------|----------|
     | Error | E501 (line length), E302 (blank lines) |
     | Warning | W503 (line break before operator) |
     | Complexity | C901 (too complex) |

  3. Report with line numbers

Output:
  passed: true | false
  issues: [{code, line, message}...]
  complexity_warnings: [{function, complexity}...]

📚 Resource: [python-essentials.md](resources/python-essentials.md)
   Load when: Understanding flake8/ruff error codes or fixing Python issues
```

### 🔧 apply-shell-standards

```text
When: Creating new shell script
Enforcement: ENF-L1 Sentinel

OS Compatibility Requirements:
  | OS | Detection | Notes |
  |----|-----------|-------|
  | macOS | [[ "$OSTYPE" == "darwin"* ]] | BSD tools differ |
  | Linux | [[ "$OSTYPE" == "linux-gnu"* ]] | GNU tools |
  | WSL | [[ -n "${WSL_DISTRO_NAME:-}" ]] | Windows paths |

Template:
  #!/usr/bin/env bash
  # Purpose:   {description}
  # Usage:     {script-name} [options]
  # Platform:  macOS/Linux
  set -euo pipefail

  # --- Constants ---
  readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  readonly SCRIPT_NAME="$(basename "$0")"

  # --- OS Detection ---
  detect_os() {
    case "$OSTYPE" in
      darwin*)  echo "macos" ;;
      linux*)   echo "linux" ;;
      msys*|cygwin*) echo "windows" ;;
      *)        echo "unknown" ;;
    esac
  }

  # --- Functions ---
  usage() {
    cat <<EOF
  Usage: $SCRIPT_NAME [options]

  {description}

  Options:
    -h, --help    Show this help
    -v, --verbose Enable verbose output
  EOF
  }

  main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
      case "$1" in
        -h|--help) usage; exit 0 ;;
        -v|--verbose) VERBOSE=1; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
      esac
    done

    # Main logic
  }

  # --- Entry Point ---
  main "$@"

Requirements:
  - set -euo pipefail
  - Readonly constants
  - Usage function
  - Main function
  - Proper quoting
  - OS detection for cross-platform scripts

📚 Resource: [shell-essentials.md](resources/shell-essentials.md)
   Load when: Applying shell template or understanding shell best practices
```

### 🔧 apply-python-standards

```text
When: Creating new Python script
Enforcement: ENF-L1 Sentinel

Template:
  #!/usr/bin/env python3
  """
  {Description}

  Usage:
      python {script-name}.py [options]
  """
  from __future__ import annotations

  import argparse
  import logging
  import sys
  from pathlib import Path
  from typing import TYPE_CHECKING

  if TYPE_CHECKING:
      from argparse import Namespace

  logger = logging.getLogger(__name__)


  def main(args: Namespace) -> int:
      """Main entry point."""
      return 0


  def parse_args() -> Namespace:
      """Parse command line arguments."""
      parser = argparse.ArgumentParser(description=__doc__)
      parser.add_argument("-v", "--verbose", action="store_true")
      return parser.parse_args()


  if __name__ == "__main__":
      args = parse_args()
      logging.basicConfig(
          level=logging.DEBUG if args.verbose else logging.INFO
      )
      sys.exit(main(args))

Requirements:
  - Type hints
  - Docstrings
  - Argparse for CLI
  - Proper logging
  - Main guard

📚 Resource: [python-essentials.md](resources/python-essentials.md)
   Load when: Applying Python template or understanding Python best practices
```

### 🔧 ensure-test-coverage

```text
When: After writing critical script
Enforcement: ENF-L3 Advisory

Procedure:
  1. Check if test file exists:
     | Script | Expected Test |
     |--------|---------------|
     | scripts/foo.sh | tests/test_foo.sh |
     | scripts/foo.py | tests/test_foo.py |

  2. If test missing:
     - Generate test stub
     - Remind to add tests

  3. Register test in test-config.json

Output:
  test_exists: true | false
  test_path: {expected path}
  stub_generated: true | false
```

### 🔧 register-test

```text
When: After creating test file for script
Enforcement: ENF-L3 Advisory

Procedure:
  1. Verify test file exists
  2. Update .codeflow/config/test-config.json:
     {
       "tests": {
         "{script_path}": {
           "test_file": "{test_path}",
           "type": "shell" | "python",
           "critical": true | false
         }
       }
     }
  3. Verify test is runnable

Output:
  registered: true | false
  config_path: .codeflow/config/test-config.json
```

### 🔧 check-modularization

```text
When: After creating/editing script
Enforcement: ENF-L3 Advisory

Modularization Rules:
  | Metric | Threshold | Action |
  |--------|-----------|--------|
  | Lines | > 200 | Split into modules |
  | Functions | > 10 | Extract to library |
  | Nesting | > 4 levels | Refactor |

Procedure:
  1. Count script lines (excluding comments/blanks)
  2. Count function definitions
  3. Check maximum nesting depth
  4. If thresholds exceeded:
     - Suggest modularization
     - Identify extraction candidates

Output:
  lines: {count}
  functions: {count}
  max_nesting: {depth}
  needs_modularization: true | false
  suggestions: [{extraction candidates}]
```

## Shell Best Practices

| Practice | Reason |
|----------|--------|
| `set -euo pipefail` | Fail on errors, undefined vars, pipe failures |
| Quote variables | Prevent word splitting and globbing |
| Use `[[` not `[` | More features, safer |
| Use `$()` not backticks | Nestable, clearer |
| Declare readonly constants | Prevent accidental modification |
| Use local in functions | Prevent variable leakage |

## Python Best Practices

| Practice | Reason |
|----------|--------|
| Type hints | IDE support, documentation |
| Docstrings | Self-documenting code |
| Logging over print | Configurable, structured |
| argparse for CLI | Standard, full-featured |
| Path over os.path | Object-oriented, cleaner |
| main guard | Allow import without execution |

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [shell-essentials.md](resources/shell-essentials.md) | ShellCheck rules and shell best practices | When fixing shell issues |
| [python-essentials.md](resources/python-essentials.md) | flake8/ruff rules and Python best practices | When fixing Python issues |
