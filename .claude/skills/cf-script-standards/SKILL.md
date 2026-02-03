---
name: cf-script-standards
description: Shell and Python linting (shellcheck, flake8) and templates.
context: fork
---

# Script Standards Skill

## Type

**Procedural** - Step-by-step procedures for script linting and standards.

## Purpose

**Ensure shell and Python scripts meet project standards through automated linting.**

## Prerequisites

- shellcheck v0.9.0+ (`brew install shellcheck`)
- flake8 v7.0+ (`pip install flake8`)

## Responsibilities

- Validate shell scripts against ShellCheck rules
- Validate Python scripts against PEP 8 via flake8
- Apply standard templates for new scripts

## Decision Tree

```text
START: Script file operation?
    │
    ├─ Creating NEW shell script (.sh)?
    │   └─ 🔧 apply-shell-standards → 🔧 lint-shell
    │
    ├─ Creating NEW Python script (.py)?
    │   └─ 🔧 apply-python-standards → 🔧 lint-python
    │
    ├─ Editing shell script?
    │   └─ 🔧 lint-shell (after edit)
    │
    └─ Editing Python script?
        └─ 🔧 lint-python (after edit)
```

## Operations

### 🔧 lint-shell

**When:** After creating or editing .sh files

**Purpose:** Validate against ShellCheck rules

**Procedure:**

```bash
shellcheck <file-path>
```

- Exit 0 = clean
- Exit 1 = issues found, fix and re-run

### 🔧 lint-python

**When:** After creating or editing .py files

**Purpose:** Validate against PEP 8 via flake8

**Procedure:**

```bash
flake8 <file-path>
```

- No output = clean
- Issues listed = fix and re-run

### 🔧 apply-shell-standards

**When:** Creating new shell script

**Purpose:** Apply standard template

**Template:**

```bash
#!/usr/bin/env bash
# Purpose:   <description>
# Usage:     <script-name> [options]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Options:
    -h, --help     Show this help
    -V, --version  Show version
EOF
}

main() {
    # Implementation
    :
}

main "$@"
```

**After creation:** `chmod +x <script>` then 🔧 lint-shell

### 🔧 apply-python-standards

**When:** Creating new Python script

**Purpose:** Apply standard template

**Template:**

```python
#!/usr/bin/env python3
"""
Purpose: <description>
Usage: <script-name> [options]
"""

import sys
from typing import Optional


def main() -> int:
    """Main entry point."""
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

**After creation:** 🔧 lint-python

## Key Conventions

**Shell:**
- Always `set -euo pipefail`
- Use `[[ ]]` for conditionals
- Quote variables: `"$var"`
- Functions: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

**Python:**
- Type hints on all functions
- Google-style docstrings
- Exit codes: 0=success, 1=error, 2=invalid args
- Import order: stdlib, third-party, local

## Resources

For shell patterns: `resources/shell-essentials.md`
For Python patterns: `resources/python-essentials.md`
