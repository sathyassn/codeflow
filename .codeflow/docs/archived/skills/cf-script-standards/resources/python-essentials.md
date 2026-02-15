# Python Script Essentials

**Purpose:** Quick reference for Python script standards when creating or reviewing .py files.

## File Structure

```python
#!/usr/bin/env python3
"""
Purpose: <one-line description>
Usage: <script-name> [options] <args>

Extended description if needed.
"""

import sys
from pathlib import Path

# === Constants ===
SCRIPT_DIR = Path(__file__).parent.resolve()
DEFAULT_TIMEOUT = 30

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
    pass

def main() -> int:
    """Main entry point.

    Returns:
        Exit code (0 for success, non-zero for error).
    """
    return 0

# === Entry Point ===
if __name__ == "__main__":
    sys.exit(main())
```

## Naming Conventions (PEP 8)

| Element | Convention | Example |
|---------|------------|---------|
| Files | snake_case.py | `process_data.py` |
| Variables | snake_case | `file_path` |
| Constants | SCREAMING_SNAKE | `MAX_RETRIES` |
| Functions | snake_case | `process_file()` |
| Classes | PascalCase | `DataProcessor` |
| Private | _leading_underscore | `_helper()` |

## Type Hints (Required)

```python
# Function signatures
def greet(name: str, times: int = 1) -> str:
    pass

# With None return
def log_message(msg: str) -> None:
    pass

# Collections
def process_items(items: list[str]) -> dict[str, int]:
    pass

# Optional
from typing import Optional
def find_user(user_id: int) -> Optional[User]:
    pass
```

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
        TimeoutError: If request times out.
    """
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Resource not found |

## Error Handling

```python
# Specific exceptions, not bare except
try:
    result = process()
except FileNotFoundError as e:
    print(f"Error: {e}", file=sys.stderr)
    return 1
except PermissionError as e:
    print(f"Error: {e}", file=sys.stderr)
    return 1

# Never use bare except:
# except:  # BAD - catches everything including KeyboardInterrupt
```

## Best Practices

- Use `pathlib.Path` instead of `os.path`
- Use f-strings for formatting: `f"Hello {name}"`
- Use `argparse` for CLI argument parsing
- Use context managers: `with open(...) as f:`
- Errors to stderr: `print(..., file=sys.stderr)`

## Common flake8 Errors

### E501: Line too long

```python
# Problem
result = some_very_long_function_name(argument1, argument2, argument3, argument4)

# Fix
result = some_very_long_function_name(
    argument1,
    argument2,
    argument3,
    argument4,
)
```

**Note:** Project uses 100 char limit. Run: `flake8 --max-line-length=100`

### F401: Module imported but unused

```python
# Problem
import os
import sys  # Only sys is used

# Fix
import sys  # Remove unused import
```

### E302: Expected 2 blank lines

```python
# Problem
def first():
    pass
def second():  # Missing blank line
    pass

# Fix
def first():
    pass


def second():  # Two blank lines before top-level definitions
    pass
```

### E303: Too many blank lines

```python
# Problem (3 blank lines)
def first():
    pass



def second():
    pass

# Fix (exactly 2 blank lines)
def first():
    pass


def second():
    pass
```

### W291/W293: Trailing whitespace

Remove trailing spaces. Most editors can do this automatically on save.

### E402: Module import not at top

```python
# Problem
print("Starting...")
import sys  # Import after code

# Fix
import sys

print("Starting...")
```

## Inline Suppression

```python
# Single rule
import unused_but_needed  # noqa: F401

# Long line that cannot be split
long_line = "..."  # noqa: E501
```
