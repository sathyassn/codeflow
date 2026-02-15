# Shell Script Essentials

**Purpose:** Quick reference for shell script standards when creating or reviewing .sh files.

## File Structure

```bash
#!/usr/bin/env bash
# Purpose:   <one-line description>
# Usage:     <script-name> [options] <args>
# Author:    <name>
# Created:   <YYYY-MM-DD>

set -euo pipefail

# === Constants ===
readonly SCRIPT_NAME="${0##*/}"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# === Configuration ===
VERBOSE=false
DRY_RUN=false

# === Functions ===
usage() {
  cat <<EOF
Usage: $SCRIPT_NAME [options]

Options:
  -h, --help     Show this help message
  -V, --version  Show version
  -v, --verbose  Enable verbose output
  -n, --dry-run  Show what would be done
EOF
}

main() {
  # Parse arguments
  # Main logic
}

# === Entry Point ===
main "$@"
```

## Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Files | kebab-case.sh | `deploy-app.sh` |
| Variables | snake_case | `file_path` |
| Constants | SCREAMING_SNAKE | `MAX_RETRIES` |
| Functions | snake_case | `process_file()` |

## Required Flags

**MANDATORY for ALL executable scripts:**

| Flag | Purpose |
|------|---------|
| `-h, --help` | Show usage information |
| `-V, --version` | Show script version |

**FORBIDDEN:** Committing scripts without `-h` and `-V` flags

**Recommended (context-dependent):**

| Flag | When to Use |
|------|-------------|
| `-v, --verbose` | Scripts with operational output |
| `-n, --dry-run` | Scripts with destructive operations |
| `-q, --quiet` | Scripts used in automation |
| `--no-color` | Scripts with colored output |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Resource not found |

## Error Handling

```bash
# Errors to stderr
echo "Error: File not found" >&2
exit 1

# Conditional exit
cd "$TARGET_DIR" || { echo "Error: Cannot cd to $TARGET_DIR" >&2; exit 1; }

# Cleanup trap
cleanup() { rm -f "$TEMP_FILE"; }
trap cleanup EXIT
```

## Best Practices

- Always quote variables: `"$var"` not `$var`
- Use `[[` instead of `[` for conditionals
- Declare and assign separately: `local var; var=$(cmd)`
- Check command existence: `command -v git &> /dev/null`

## Common ShellCheck Errors

### SC2086: Double quote to prevent globbing

```bash
# Problem
echo $PATH           # Unquoted variable

# Fix
echo "$PATH"         # Quote the variable
```

### SC2034: Variable appears unused

```bash
# Problem
readonly CONFIG_FILE="config.json"  # Never used

# Fix options:
# - Use the variable
# - Export if needed: export CONFIG_FILE
# - Disable if intentional: # shellcheck disable=SC2034
```

### SC2155: Declare and assign separately

```bash
# Problem
local output=$(command)  # Masks exit code

# Fix
local output
output=$(command)        # Exit code preserved
```

### SC2164: Use cd ... || exit

```bash
# Problem
cd "$directory"          # Continues if cd fails

# Fix
cd "$directory" || exit 1
```

### SC2129: Consider using braces

```bash
# Problem
echo "line1" >> file
echo "line2" >> file

# Fix
{
  echo "line1"
  echo "line2"
} >> file
```

## OS-Agnostic Patterns

**MANDATORY for ALL scripts** (all scripts assumed to run on any OS):

```bash
# Detect OS for platform-specific commands
if [[ "$OSTYPE" == "darwin"* ]]; then
    SED_INPLACE="sed -i ''"
    ROOT_GROUP="wheel"
else
    SED_INPLACE="sed -i"
    ROOT_GROUP="root"
fi
```

| Operation | macOS | Linux |
|-----------|-------|-------|
| In-place sed | `sed -i ''` | `sed -i` |
| Root group | `wheel` | `root` |
| File permissions | `stat -f '%A'` | `stat -c '%a'` |

## Library Modularization

**MANDATORY when scripts exceed 200 lines:**

```bash
# Main script sources libraries
readonly LIB_DIR="${SCRIPT_DIR}/lib"
source "${LIB_DIR}/common.sh"
```

**Direct execution prevention (REQUIRED for lib/*.sh):**

```bash
# At top of library files
[[ "${BASH_SOURCE[0]}" == "${0}" ]] && {
    echo "Error: This script must be sourced" >&2
    exit 1
}
```

## Terminal-Aware Colors

**MANDATORY when using colors:**

```bash
# Detect terminal and set colors
if [[ -t 1 ]]; then
    RED='\033[0;31m' GREEN='\033[0;32m' NC='\033[0m'
else
    RED='' GREEN='' NC=''
fi

# Respect NO_COLOR environment variable
[[ -n "${NO_COLOR:-}" ]] && RED='' GREEN='' NC=''
```

## Inline Suppression

```bash
# Single rule
# shellcheck disable=SC2086
echo $unquoted_intentionally

# Multiple rules
# shellcheck disable=SC2034,SC2155
local unused=$(cmd)
```

## Quick Checklist

Before committing a shell script:

- [ ] `set -euo pipefail` at top
- [ ] `-h/--help` flag implemented
- [ ] `-V/--version` flag implemented
- [ ] `chmod +x` applied
- [ ] `shellcheck` passes
- [ ] OS-agnostic (if cross-platform)
- [ ] Modularized (if >200 lines)
