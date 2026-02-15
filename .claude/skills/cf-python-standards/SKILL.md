---
name: cf-python-standards
description: Python scripting standards and validation reference. Covers script structure, naming, type hints, common patterns, and lint rules for .py files.
context: on-demand (loaded by cf-development when creating or editing .py files)
---

# Python Standards Skill

## Type

**Procedural** - On-demand reference for Python scripting conventions and validation.

## Purpose

**Quick reference for Python script structure, naming, type hints, lint rules, and common patterns used in CodeFlow.**

## Responsibilities

- Define standard script structure, naming, type hints, and docstring style
- Document common patterns (argparse, JSON, error handling, logging) and lint config
- NOT: Linting execution (cf-development SOPs run `ruff`/`flake8`)
- NOT: Test writing (cf-development Test Writing SOP)

---

## Script Structure

### CLI Script Template

```python
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
```

### Required Elements

| Element | Rule |
|---------|------|
| Shebang | `#!/usr/bin/env python3` for CLI scripts; omit for library modules |
| Future import | `from __future__ import annotations` (first import) |
| Module docstring | Purpose + Usage lines |
| Main guard | `if __name__ == "__main__":` |
| Exit code | `sys.exit(main(...))` for CLI scripts |

### Import Order

1. `from __future__ import annotations`
2. Standard library (`sys`, `pathlib`, `json`, `logging`)
3. Third-party packages
4. Local/project imports
5. `TYPE_CHECKING` block (type-only imports)

Separate each group with a blank line.

---

## Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Files (CodeFlow) | `cf_` prefix + `snake_case.py` | `cf_process_data.py` |
| Files (general) | `snake_case.py` | `process_data.py` |
| Variables | `snake_case` | `file_path` |
| Constants | `UPPER_SNAKE` | `MAX_RETRIES` |
| Functions | `snake_case` | `process_file()` |
| Classes | `PascalCase` | `DataProcessor` |
| Private | `_leading_underscore` | `_helper()` |

---

## Type Hints (Required)

Required on all function signatures. Use Python 3.10+ union syntax and 3.9+ lowercase generics.

```python
def greet(name: str, times: int = 1) -> str: ...
def process(items: list[str]) -> dict[str, int]: ...
def find(user_id: int) -> User | None: ...
```

Use `TYPE_CHECKING` guard for imports only needed by type checkers.

---

## Docstrings (Google Style)

```python
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
```

Required on all public functions and classes. One-liners acceptable for trivial helpers.

---

## Common Patterns

| Pattern | Rule |
|---------|------|
| CLI args | `argparse` for all entry points (see template) |
| JSON I/O | `json.load()`/`json.dump()` with `pathlib.Path.open()` |
| Error handling | Specific exceptions only; never bare `except:` |
| Logging | `logging.getLogger(__name__)` with `%`-style formatting; never `print()` |
| Paths | `pathlib.Path` over `os.path`; use `/` operator for joining |
| File I/O | Always use `with`-statement context managers |
| Exit codes | 0=success, 1=error, 2=bad args, 3=not found |

Expanded examples: [python-essentials.md](resources/python-essentials.md)

---

## Lint / Format Rules

**Linter:** `ruff` (preferred) or `flake8` (fallback). Max line length: **100**.

| Command | Usage |
|---------|-------|
| `ruff check {script}` | Primary linter |
| `flake8 --max-line-length=100 {script}` | Fallback |

**Inline suppression:** `# noqa: F401` -- always include a reason comment.

Full lint rule reference: [python-essentials.md](resources/python-essentials.md)

---

## Anti-Patterns

| Anti-Pattern | Correct Alternative |
|-------------|-------------------|
| `os.path.join(a, b)` | `Path(a) / b` |
| `print("Error: ...")` | `logger.error(...)` or `print(..., file=sys.stderr)` |
| `except:` (bare) | `except SpecificError as e:` |
| `f.read()` without `with` | `with open(...) as f: f.read()` |
| `type(x) == str` | `isinstance(x, str)` |
| Missing type hints | Add to all function signatures |
| `import *` | Explicit named imports |
| Mutable default args (`def f(x=[])`) | `def f(x: list \| None = None)` |
| Global mutable state | Constants (`UPPER_SNAKE`) or function params |
