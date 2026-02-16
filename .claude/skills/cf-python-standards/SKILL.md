---
name: cf-python-standards
description: Python scripting standards and validation reference. Covers script structure, naming, type hints, common patterns, and lint rules for .py files.
---

# Python Standards Skill

## Type

**Procedural** - On-demand reference for Python scripting conventions and validation.

## Purpose

**Quick reference for Python script structure, naming, type hints, lint rules, and common patterns used in CodeFlow.**

## Responsibilities

- Define standard script structure, naming, and type hint conventions
- Document common patterns (argparse, JSON, error handling, logging)
- Specify lint configuration and rules
- NOT: Linting execution (cf-development SOPs run `ruff`/`flake8`)
- NOT: Test writing (cf-quality-assurance handles test implementation)

## Decision Tree

```text
Working with .py file:
├── New script? → 🔧 apply-structure
├── Editing existing? → Check conventions
│   ├── Naming/imports? → 🔧 apply-conventions
│   └── Logic/patterns? → 🔧 apply-patterns
└── Before commit? → 🔧 validate-script
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-structure | ENF-L3 Advisory | Script skeleton and required elements |
| 2 | apply-conventions | ENF-L3 Advisory | Naming, type hints, imports, docstrings |
| 3 | apply-patterns | ENF-L3 Advisory | Common patterns and anti-patterns |
| 4 | validate-script | ENF-L3 Advisory | Lint rules and pre-commit checks |

## Operation Details

### 🔧 apply-structure

```text
When: Creating a new Python script or reviewing script skeleton
Purpose: Ensure correct CLI script template, required elements, and import order
Enforcement: ENF-L3 Advisory

CLI Script Template:

  #!/usr/bin/env python3
  """
  Purpose: <one-line description>
  Usage: <script-name> [options] <args>
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
      logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO)
      sys.exit(main(args))

Required Elements:

  | Element          | Rule                                                         |
  |------------------|--------------------------------------------------------------|
  | Shebang          | #!/usr/bin/env python3 for CLI scripts; omit for lib modules |
  | Future import    | from __future__ import annotations (first import)            |
  | Module docstring | Purpose + Usage lines                                        |
  | Main guard       | if __name__ == "__main__":                                   |
  | Exit code        | sys.exit(main(...)) for CLI scripts                          |

Import Order:

  1. from __future__ import annotations
  2. Standard library (sys, pathlib, json, logging)
  3. Third-party packages
  4. Local/project imports
  5. TYPE_CHECKING block (type-only imports)

  Separate each group with a blank line.

Procedure:
  1. Create file with shebang and module docstring
  2. Add future import as first import line
  3. Add imports in correct group order
  4. Define functions with type hints
  5. Add main guard with sys.exit(main(...))
  6. Verify all required elements are present

Output: Python script with correct skeleton, required elements, and import order
```

### 🔧 apply-conventions

```text
When: Writing or editing Python code -- naming, type hints, imports, or docstrings
Purpose: Ensure consistent naming, required type hints, and Google-style docstrings
Enforcement: ENF-L3 Advisory

Naming Conventions:

  | Element          | Convention          | Example              |
  |------------------|---------------------|----------------------|
  | Files (CodeFlow) | cf_ prefix + snake  | cf_process_data.py   |
  | Files (general)  | snake_case.py       | process_data.py      |
  | Variables        | snake_case          | file_path            |
  | Constants        | UPPER_SNAKE         | MAX_RETRIES          |
  | Functions        | snake_case          | process_file()       |
  | Classes          | PascalCase          | DataProcessor        |
  | Private          | _leading_underscore | _helper()            |

Type Hints (Required):

  Required on all function signatures.
  Use Python 3.10+ union syntax and 3.9+ lowercase generics.

  def greet(name: str, times: int = 1) -> str: ...
  def process(items: list[str]) -> dict[str, int]: ...
  def find(user_id: int) -> User | None: ...

  Use TYPE_CHECKING guard for imports only needed by type checkers.

Docstrings (Google Style):

  def fetch_data(url: str, timeout: int = 30) -> dict:
      """Fetch data from a URL.

      Args:
          url: The URL to fetch from.
          timeout: Request timeout in seconds.

      Returns:
          Parsed JSON response as dictionary.

      Raises:
          ConnectionError: If connection fails.
      """

  Required on all public functions and classes.
  One-liners acceptable for trivial helpers.

Procedure:
  1. Apply naming convention from table for the element type
  2. Add type hints to all function signatures
  3. Add Google-style docstrings to public functions and classes
  4. Use TYPE_CHECKING guard for type-only imports

Output: Code following consistent naming, type hint, and docstring conventions
```

### 🔧 apply-patterns

```text
When: Implementing logic -- CLI args, file I/O, error handling, logging, paths
Purpose: Use correct patterns and avoid known anti-patterns
Enforcement: ENF-L3 Advisory

Common Patterns:

  | Pattern        | Rule                                                              |
  |----------------|-------------------------------------------------------------------|
  | CLI args       | argparse for all entry points (see template)                      |
  | JSON I/O       | json.load()/json.dump() with pathlib.Path.open()                  |
  | Error handling | Specific exceptions only; never bare except:                      |
  | Logging        | logging.getLogger(__name__) with %-style formatting; never print()|
  | Paths          | pathlib.Path over os.path; use / operator for joining             |
  | File I/O       | Always use with-statement context managers                        |
  | Exit codes     | 0=success, 1=error, 2=bad args, 3=not found                      |

  Expanded examples: resources/python-essentials.md

Anti-Patterns:

  | Anti-Pattern                     | Correct Alternative                        |
  |----------------------------------|--------------------------------------------|
  | os.path.join(a, b)               | Path(a) / b                                |
  | print("Error: ...")              | logger.error(...) or print(..., file=stderr)|
  | except: (bare)                   | except SpecificError as e:                 |
  | f.read() without with            | with open(...) as f: f.read()              |
  | type(x) == str                   | isinstance(x, str)                         |
  | Missing type hints               | Add to all function signatures             |
  | import *                         | Explicit named imports                     |
  | Mutable default args (def f(x=[]))| def f(x: list | None = None)              |
  | Global mutable state             | Constants (UPPER_SNAKE) or function params |

Procedure:
  1. Check implementation against common patterns table
  2. Replace any anti-patterns with correct alternatives
  3. Verify error handling uses specific exceptions
  4. Verify logging uses getLogger, not print()
  5. Verify paths use pathlib.Path

Output: Code using correct patterns with no anti-patterns present
```

### 🔧 validate-script

```text
When: Before committing Python scripts or during pre-commit checks
Purpose: Run lint checks and verify inline suppression rules
Enforcement: ENF-L3 Advisory

Lint / Format Rules:

  Linter: ruff (preferred) or flake8 (fallback). Max line length: 100.

  | Command                              | Usage            |
  |--------------------------------------|------------------|
  | ruff check {script}                  | Primary linter   |
  | flake8 --max-line-length=100 {script}| Fallback         |

  Inline suppression: # noqa: F401 -- always include a reason comment.

  Full lint rule reference: resources/python-essentials.md

Procedure:
  1. Run ruff check on the script (or flake8 as fallback)
  2. Review any reported issues
  3. Fix all lint errors (do not suppress without justification)
  4. If suppression is necessary, add reason comment after # noqa: CODE
  5. Re-run linter until output is clean
  6. Verify max line length does not exceed 100 characters

Output: Clean lint output with no errors or justified suppressions only
```
