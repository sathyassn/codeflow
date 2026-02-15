# Python Essentials Reference

**Purpose:** Expanded examples and full lint rules reference for Python scripts. Companion to the [cf-python-standards SKILL.md](../SKILL.md).

---

## Full Script Template (Annotated)

```python
#!/usr/bin/env python3
"""
Purpose: <one-line description>
Usage: <script-name> [options] <args>

Extended description if needed (what the script does, why it exists).
"""
from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from argparse import Namespace

# === Constants ===
SCRIPT_DIR = Path(__file__).parent.resolve()
DEFAULT_TIMEOUT = 30
EXIT_SUCCESS = 0
EXIT_ERROR = 1
EXIT_BAD_ARGS = 2
EXIT_NOT_FOUND = 3

logger = logging.getLogger(__name__)


# === Functions ===
def process_file(file_path: Path, *, verbose: bool = False) -> bool:
    """Process a single file.

    Args:
        file_path: Path to the file to process.
        verbose: Enable verbose output.

    Returns:
        True if successful, False otherwise.

    Raises:
        FileNotFoundError: If file does not exist.
    """
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")
    logger.info("Processing %s", file_path)
    # ... implementation ...
    return True


def main(args: Namespace) -> int:
    """Main entry point.

    Returns:
        Exit code (0 for success, non-zero for error).
    """
    try:
        target = Path(args.target)
        if not target.exists():
            logger.error("Target not found: %s", target)
            return EXIT_NOT_FOUND
        process_file(target, verbose=args.verbose)
    except Exception:
        logger.exception("Unexpected error")
        return EXIT_ERROR
    return EXIT_SUCCESS


def parse_args() -> Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("target", help="File to process")
    parser.add_argument("-v", "--verbose", action="store_true",
                        help="Enable verbose output")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT,
                        help=f"Timeout in seconds (default: {DEFAULT_TIMEOUT})")
    return parser.parse_args()


# === Entry Point ===
if __name__ == "__main__":
    parsed = parse_args()
    logging.basicConfig(
        level=logging.DEBUG if parsed.verbose else logging.INFO,
        format="%(levelname)s: %(message)s",
    )
    sys.exit(main(parsed))
```

---

## Library Module Template

Library modules (imported, not executed directly) omit the shebang and argparse:

```python
"""
<Module description>

Provides utility functions for <domain>.
"""
from __future__ import annotations

import logging
from pathlib import Path

logger = logging.getLogger(__name__)


# === Public API ===
def helper_function(value: str) -> str:
    """Transform value according to project rules.

    Args:
        value: Input string to transform.

    Returns:
        Transformed string.
    """
    return value.strip().lower()


class DataProcessor:
    """Process data records according to project schema.

    Attributes:
        config_path: Path to the configuration file.
    """

    def __init__(self, config_path: Path) -> None:
        self.config_path = config_path
        self._cache: dict[str, str] = {}

    def process(self, record: dict) -> dict:
        """Process a single record.

        Args:
            record: Input record dictionary.

        Returns:
            Processed record dictionary.
        """
        return record
```

---

## Type Hint Patterns

### Basic Signatures

```python
def greet(name: str, times: int = 1) -> str:
    return name * times

def log_message(msg: str) -> None:
    logger.info(msg)
```

### Collections

```python
# Python 3.9+ lowercase generics (preferred)
def process_items(items: list[str]) -> dict[str, int]:
    return {item: len(item) for item in items}

def get_pairs(data: dict[str, list[int]]) -> list[tuple[str, int]]:
    return [(k, v) for k, vs in data.items() for v in vs]
```

### Optional and Union

```python
# Python 3.10+ union syntax (preferred)
def find_user(user_id: int) -> User | None:
    ...

# Fallback for older Python
from typing import Optional, Union
def find_user(user_id: int) -> Optional[User]:
    ...
```

### Callable and Complex Types

```python
from typing import Callable

def apply_transform(
    data: list[str],
    transform: Callable[[str], str],
) -> list[str]:
    return [transform(item) for item in data]
```

### TYPE_CHECKING Guard

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from argparse import Namespace
    from pathlib import Path as PathType
```

Use `TYPE_CHECKING` for imports only needed by type checkers, avoiding runtime import overhead.

---

## Docstring Style (Google)

### Function Docstring

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
        TimeoutError: If request times out.
    """
```

### Class Docstring

```python
class TaskRunner:
    """Execute tasks according to dependency graph.

    Manages task scheduling, execution order, and failure recovery
    for the CodeFlow pipeline.

    Attributes:
        max_retries: Maximum retry attempts per task.
        timeout: Default timeout in seconds.
    """

    def __init__(self, max_retries: int = 3, timeout: int = 60) -> None:
        self.max_retries = max_retries
        self.timeout = timeout
```

### One-liner (Trivial Helpers)

```python
def is_empty(value: str) -> bool:
    """Return True if value is empty or whitespace-only."""
    return not value.strip()
```

---

## Error Handling Patterns

### Specific Exceptions

```python
try:
    result = process()
except FileNotFoundError as e:
    logger.error("File not found: %s", e)
    return EXIT_NOT_FOUND
except PermissionError as e:
    logger.error("Permission denied: %s", e)
    return EXIT_ERROR
except json.JSONDecodeError as e:
    logger.error("Invalid JSON at line %d: %s", e.lineno, e.msg)
    return EXIT_ERROR
```

### Re-raising After Logging

```python
try:
    risky_operation()
except ValueError:
    logger.exception("Operation failed with bad value")
    raise  # Re-raise after logging
```

### Custom Exceptions

```python
class CodeFlowError(Exception):
    """Base exception for CodeFlow errors."""

class ConfigError(CodeFlowError):
    """Configuration file error."""

class ValidationError(CodeFlowError):
    """Input validation error."""
```

### Context Managers

```python
# File I/O -- always use with-statement
with Path("data.json").open() as f:
    data = json.load(f)

# Multiple context managers
with (
    Path("input.txt").open() as src,
    Path("output.txt").open("w") as dst,
):
    dst.write(src.read())
```

---

## JSON Handling Patterns

### Load and Save

```python
import json
from pathlib import Path

def load_json(path: Path) -> dict:
    """Load JSON file with error handling."""
    try:
        with path.open() as f:
            return json.load(f)
    except json.JSONDecodeError as e:
        logger.error("Invalid JSON in %s at line %d: %s", path, e.lineno, e.msg)
        raise

def save_json(path: Path, data: dict, *, indent: int = 2) -> None:
    """Save data to JSON file with trailing newline."""
    with path.open("w") as f:
        json.dump(data, f, indent=indent)
        f.write("\n")
```

### JSONL (Append-Only Logs)

```python
def append_jsonl(path: Path, record: dict) -> None:
    """Append a single JSON record to a JSONL file."""
    with path.open("a") as f:
        f.write(json.dumps(record, separators=(",", ":")) + "\n")

def read_jsonl(path: Path) -> list[dict]:
    """Read all records from a JSONL file."""
    records = []
    with path.open() as f:
        for line_num, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as e:
                logger.warning("Skipping invalid JSON at line %d: %s", line_num, e.msg)
    return records
```

---

## Argument Parsing Patterns

### Subcommands

```python
def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="CodeFlow CLI")
    sub = parser.add_subparsers(dest="command", required=True)

    # 'test' subcommand
    test_parser = sub.add_parser("test", help="Run test suite")
    test_parser.add_argument("--coverage", action="store_true")

    # 'doctor' subcommand
    sub.add_parser("doctor", help="Diagnose infrastructure")

    return parser
```

### Mutually Exclusive Options

```python
group = parser.add_mutually_exclusive_group()
group.add_argument("--json", action="store_true", help="JSON output")
group.add_argument("--csv", action="store_true", help="CSV output")
```

### File Arguments with Path Validation

```python
parser.add_argument(
    "input_file",
    type=Path,
    help="Input file path",
)
# Validate after parsing
args = parser.parse_args()
if not args.input_file.exists():
    parser.error(f"File not found: {args.input_file}")
```

---

## Logging Configuration

### Basic Setup

```python
logging.basicConfig(
    level=logging.DEBUG if args.verbose else logging.INFO,
    format="%(levelname)s: %(message)s",
)
```

### Structured Format (for Production Scripts)

```python
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
    datefmt="%Y-%m-%dT%H:%M:%S",
)
```

### Logging Best Practices

```python
# Use %-style formatting (lazy evaluation -- string not built if level filtered)
logger.info("Processing %s (%d items)", file_path, count)

# AVOID f-strings in logger calls (always evaluated)
# BAD: logger.info(f"Processing {file_path} ({count} items)")

# Use logger.exception() in except blocks (auto-includes traceback)
try:
    risky()
except Exception:
    logger.exception("Failed during processing")
    raise
```

---

## Full Lint Rules Reference

### ruff / flake8 Error Codes

| Code | Category | Description | Action |
|------|----------|-------------|--------|
| **E1xx** | Indentation | | |
| E101 | | Indentation contains mixed spaces and tabs | Must fix |
| E111 | | Indentation is not a multiple of four | Must fix |
| **E2xx** | Whitespace | | |
| E201 | | Whitespace after `(` | Must fix |
| E202 | | Whitespace before `)` | Must fix |
| E211 | | Whitespace before `(` or `[` | Must fix |
| E225 | | Missing whitespace around operator | Must fix |
| E231 | | Missing whitespace after `,` | Must fix |
| E251 | | Unexpected spaces around keyword / parameter default | Must fix |
| E261 | | At least two spaces before inline comment | Must fix |
| E262 | | Inline comment should start with `#` | Must fix |
| E271 | | Multiple spaces after keyword | Must fix |
| **E3xx** | Blank Lines | | |
| E301 | | Expected 1 blank line before function definition | Must fix |
| E302 | | Expected 2 blank lines before top-level definition | Must fix |
| E303 | | Too many blank lines | Must fix |
| **E4xx** | Imports | | |
| E401 | | Multiple imports on one line | Must fix |
| E402 | | Module level import not at top of file | Must fix |
| **E5xx** | Line Length | | |
| E501 | | Line too long (> 100 chars for this project) | Must fix |
| **E7xx** | Statement | | |
| E711 | | Comparison to `None` (use `is` / `is not`) | Must fix |
| E712 | | Comparison to `True`/`False` (use `if x:`) | Must fix |
| E721 | | Do not compare types, use `isinstance()` | Must fix |
| E722 | | Do not use bare `except` | Must fix |
| E731 | | Do not assign a `lambda` expression (use `def`) | Must fix |
| **W** | Warnings | | |
| W291 | | Trailing whitespace | Should fix |
| W292 | | No newline at end of file | Should fix |
| W293 | | Whitespace before a comment | Should fix |
| W503 | | Line break before binary operator | Should fix |
| W605 | | Invalid escape sequence | Must fix |
| **F** | PyFlakes | | |
| F401 | | Module imported but unused | Must fix |
| F403 | | `from module import *` used | Must fix |
| F405 | | Name may be undefined (from `import *`) | Must fix |
| F811 | | Redefinition of unused name | Must fix |
| F821 | | Undefined name | Must fix |
| F841 | | Local variable assigned but never used | Must fix |
| **C** | Complexity | | |
| C901 | | Function is too complex (cyclomatic > 10) | Refactor |

### Fixing Common Errors

#### E501: Line Too Long

```python
# Problem
result = some_very_long_function_name(argument1, argument2, argument3, argument4)

# Fix: break at parentheses
result = some_very_long_function_name(
    argument1,
    argument2,
    argument3,
    argument4,
)

# Fix: break long strings
message = (
    "This is a very long message that would exceed "
    "the line length limit if written on one line."
)
```

#### F401: Module Imported but Unused

```python
# Problem
import os       # unused
import sys

# Fix: remove unused import
import sys

# Exception: intentional re-export
from module import helper  # noqa: F401
```

#### E302: Expected 2 Blank Lines

```python
# Problem
def first():
    pass
def second():
    pass

# Fix
def first():
    pass


def second():
    pass
```

#### E711/E712: Comparison Style

```python
# Problem
if x == None:       # E711
if flag == True:    # E712

# Fix
if x is None:
if flag:
```

#### E722: Bare Except

```python
# Problem
try:
    risky()
except:
    pass

# Fix
try:
    risky()
except Exception:
    logger.exception("Operation failed")
    raise
```

#### C901: Too Complex

Split into smaller functions. Each function should do one thing. Extract branches into helper functions.

### Inline Suppression

```python
# Suppress single rule with reason
import unused_but_needed  # noqa: F401  -- needed for side effect

# Suppress line length (use sparingly)
long_url = "https://very-long-url..."  # noqa: E501

# Suppress entire file (rare, document why at top)
# ruff: noqa: E501
```

### Project ruff Configuration

Recommended `pyproject.toml` section:

```toml
[tool.ruff]
line-length = 100
target-version = "py311"

[tool.ruff.lint]
select = ["E", "F", "W", "C901"]
ignore = ["W503"]

[tool.ruff.lint.mccabe]
max-complexity = 10
```

### Project flake8 Configuration

Fallback `.flake8`:

```ini
[flake8]
max-line-length = 100
max-complexity = 10
ignore = W503
```
