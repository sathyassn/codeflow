# Shell Essentials Reference

> Companion resource for the cf-shell-standards skill operations.
> Primary operations expanded here: **apply-structure**, **apply-conventions**, **apply-patterns**, **validate-script**.

**Purpose:** Expanded examples and full lint rules for shell scripting.

## File Structure Template (apply-structure)

```bash
#!/usr/bin/env bash
# Purpose:   <one-line description>
# Usage:     <script-name> [options] <args>
# Platform:  macOS/Linux

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
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -h|--help) usage; exit 0 ;;
      -V|--version) echo "$SCRIPT_NAME v1.0.0"; exit 0 ;;
      -v|--verbose) VERBOSE=true; shift ;;
      -n|--dry-run) DRY_RUN=true; shift ;;
      --) shift; break ;;
      -*) echo "Unknown option: $1" >&2; exit 2 ;;
      *) break ;;
    esac
  done
  # Main logic here
}

# === Entry Point ===
main "$@"
```

## Library File Template (apply-structure)

```bash
#!/usr/bin/env bash
# Purpose:   <library description>
# Usage:     source this file; do not execute directly

# Source guard -- prevent double-loading
[[ -n "${_MY_LIB_LOADED:-}" ]] && return 0
readonly _MY_LIB_LOADED=1

# Direct execution guard
[[ "${BASH_SOURCE[0]}" == "${0}" ]] && {
  echo "Error: This script must be sourced, not executed directly" >&2
  exit 1
}

# === Functions ===
my_lib_function() {
  local arg="$1"
  # implementation
}
```

## Naming Conventions (apply-conventions)

| Element | Convention | Example |
|---------|------------|---------|
| Script files | kebab-case.sh | `deploy-app.sh` |
| CodeFlow scripts | cf- prefix | `cf-pre-tool-use-gate.sh` |
| Variables | snake_case | `file_path` |
| Constants | SCREAMING_SNAKE_CASE | `MAX_RETRIES` |
| Functions | snake_case | `process_file()` |
| Source guard vars | `_LOADED_` prefix | `_MY_LIB_LOADED` |

## Required Flags (apply-structure)

**Mandatory for all executable scripts:**

| Flag | Purpose |
|------|---------|
| `-h, --help` | Show usage information |

**Recommended (context-dependent):**

| Flag | When to Use |
|------|-------------|
| `-V, --version` | Versioned scripts |
| `-v, --verbose` | Scripts with operational output |
| `-n, --dry-run` | Scripts with destructive operations |
| `-q, --quiet` | Scripts used in automation |
| `--no-color` | Scripts with colored output |

## Exit Codes (apply-structure)

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Resource not found |

## Error Handling Patterns (apply-patterns)

### Basic Error Output

```bash
echo "Error: File not found: $file" >&2
exit 1
```

### Conditional with Error Message

```bash
cd "$TARGET_DIR" || { echo "Error: Cannot cd to $TARGET_DIR" >&2; exit 1; }
```

### Cleanup Trap

```bash
TEMP_FILE="$(mktemp)"
TEMP_DIR="$(mktemp -d)"
cleanup() {
  rm -f "$TEMP_FILE"
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT
```

### Safe Arithmetic Under set -e

```bash
# Problem: ((count++)) returns 1 (false) when count is 0, triggering set -e
((count++)) || true

# Alternative: use let with || true
let "count += 1" || true
```

### Command Existence Check

```bash
command -v git &> /dev/null || { echo "Error: git not found" >&2; exit 1; }
```

## Variable Handling (apply-patterns)

### Declare and Assign Separately

```bash
# Wrong -- masks exit code
local output=$(some_command)

# Correct -- exit code preserved
local output
output=$(some_command)
```

### Readonly After Assignment

```bash
# Wrong in Bash 3.2 -- masks exit code
readonly result=$(some_command)

# Correct
local result
result=$(some_command)
readonly result
```

### Default Values

```bash
local config="${1:-default_value}"
local home="${CODEFLOW_HOME:-$HOME/.codeflow}"
```

### Array Safety

```bash
# Safe empty array expansion under set -u
local -a items=()
if [[ ${#items[@]} -gt 0 ]]; then
  for item in "${items[@]}"; do
    echo "$item"
  done
fi
```

## JSON Output Patterns (apply-patterns)

### Simple JSON with jq

```bash
jq -c -n \
  --arg status "ok" \
  --arg file "$file_path" \
  '{ status: $status, file: $file }'
```

### JSON with Numeric Values

```bash
jq -c -n \
  --arg name "$name" \
  --argjson count "$count" \
  --argjson passed true \
  '{ name: $name, count: $count, passed: $passed }'
```

### Building JSON Arrays

```bash
local json_array="[]"
for item in "${items[@]}"; do
  json_array=$(echo "$json_array" | jq -c --arg i "$item" '. + [$i]')
done
```

## OS-Agnostic Patterns (apply-patterns)

### OS Detection

```bash
case "$OSTYPE" in
  darwin*)
    SED_INPLACE=(sed -i '')
    STAT_PERMS=(stat -f '%A')
    ROOT_GROUP="wheel"
    ;;
  linux*)
    SED_INPLACE=(sed -i)
    STAT_PERMS=(stat -c '%a')
    ROOT_GROUP="root"
    ;;
  *)
    echo "Unsupported OS: $OSTYPE" >&2
    exit 1
    ;;
esac
```

### Platform-Specific Commands

| Operation | macOS (BSD) | Linux (GNU) |
|-----------|-------------|-------------|
| In-place sed | `sed -i ''` | `sed -i` |
| Root group | `wheel` | `root` |
| File permissions | `stat -f '%A'` | `stat -c '%a'` |
| Date (epoch) | `date -r $epoch` | `date -d @$epoch` |
| Base64 decode | `base64 -D` | `base64 -d` |
| Readlink (absolute) | `readlink` (no -f) | `readlink -f` |

## Terminal-Aware Colors (apply-patterns)

```bash
if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[0;33m'
  NC='\033[0m'
else
  RED='' GREEN='' YELLOW='' NC=''
fi

# Respect NO_COLOR standard
[[ -n "${NO_COLOR:-}" ]] && RED='' GREEN='' YELLOW='' NC=''

# Usage
echo -e "${RED}Error:${NC} something failed"
echo -e "${GREEN}OK:${NC} tests passed"
```

## Bash 3.2 Compatibility Guide (apply-conventions)

macOS ships Bash 3.2. All scripts must avoid Bash 4+ features.

### Associative Arrays (Bash 4.0+) -- AVOID

```bash
# WRONG -- Bash 4+ only
declare -A config
config[host]="localhost"
config[port]="8080"

# CORRECT -- use case statement or parallel arrays
get_config() {
  case "$1" in
    host) echo "localhost" ;;
    port) echo "8080" ;;
  esac
}

# CORRECT -- parallel indexed arrays
config_keys=(host port)
config_vals=(localhost 8080)
```

### mapfile / readarray (Bash 4.0+) -- AVOID

```bash
# WRONG
mapfile -t lines < "$file"

# CORRECT
local -a lines=()
while IFS= read -r line; do
  lines+=("$line")
done < "$file"
```

### Case Modification (Bash 4.0+) -- AVOID

```bash
# WRONG
lower="${var,,}"
upper="${var^^}"

# CORRECT
lower=$(echo "$var" | tr '[:upper:]' '[:lower:]')
upper=$(echo "$var" | tr '[:lower:]' '[:upper:]')
```

### Regex with Stored Pattern (Bash 3.2 bug)

```bash
# WRONG -- unreliable in Bash 3.2
pattern='^[0-9]+$'
[[ "$input" =~ $pattern ]]

# CORRECT -- inline the regex
[[ "$input" =~ ^[0-9]+$ ]]
```

### Pipe Stderr (Bash 4.0+) -- AVOID

```bash
# WRONG
command |& grep "error"

# CORRECT
command 2>&1 | grep "error"
```

## ShellCheck Reference (validate-script)

### Running ShellCheck

```bash
# Standard invocation
shellcheck -x -s bash script.sh

# Check all scripts in a directory
find .codeflow/scripts -name "*.sh" -exec shellcheck -x -s bash {} +
```

### Severity Levels

| Prefix | Severity | Action |
|--------|----------|--------|
| SC1xxx | Error (parse/syntax) | Must fix -- blocks commit |
| SC2xxx | Warning (semantic) | Should fix |
| SC3xxx | Info (style) | Consider |

### Common Errors and Fixes

#### SC2086: Double quote to prevent globbing and word splitting

```bash
# Problem
echo $PATH
rm $file

# Fix
echo "$PATH"
rm "$file"
```

#### SC2155: Declare and assign separately to avoid masking return values

```bash
# Problem
local output=$(command)

# Fix
local output
output=$(command)
```

#### SC2164: Use cd ... || exit in case cd fails

```bash
# Problem
cd "$directory"

# Fix
cd "$directory" || exit 1
```

#### SC2034: Variable appears unused

```bash
# If intentional (exported or sourced by another script):
# shellcheck disable=SC2034
readonly CONFIG_FILE="config.json"

# Or export it:
export CONFIG_FILE="config.json"
```

#### SC2129: Consider using { cmd1; cmd2; } >> file

```bash
# Problem
echo "line1" >> file
echo "line2" >> file
echo "line3" >> file

# Fix
{
  echo "line1"
  echo "line2"
  echo "line3"
} >> file
```

#### SC2181: Check exit code directly, not via $?

```bash
# Problem
command
if [[ $? -ne 0 ]]; then
  echo "failed"
fi

# Fix
if ! command; then
  echo "failed"
fi
```

#### SC2015: Note that A && B || C is not if-then-else

```bash
# Problem -- C runs if B fails, not just if A fails
[[ -f "$file" ]] && process "$file" || echo "failed"

# Fix
if [[ -f "$file" ]]; then
  process "$file"
else
  echo "failed"
fi
```

#### SC2046: Quote to prevent word splitting on command substitution

```bash
# Problem
files=$(find . -name "*.sh")
process $files

# Fix
while IFS= read -r file; do
  process "$file"
done < <(find . -name "*.sh")
```

### Inline Suppression

```bash
# Single rule -- applies to next line only
# shellcheck disable=SC2086
echo $intentionally_unquoted

# Multiple rules
# shellcheck disable=SC2034,SC2155
local unused=$(cmd)

# File-level (top of file, after shebang)
# shellcheck disable=SC2034
```

### Source Directive

When shellcheck cannot follow source paths:

```bash
# shellcheck source=lib/common.sh
source "${LIB_DIR}/common.sh"
```

## Modularization Rules

| Metric | Threshold | Action |
|--------|-----------|--------|
| Lines (non-comment) | > 200 | Split into library modules |
| Functions per file | > 10 | Extract to dedicated library |
| Nesting depth | > 4 levels | Refactor logic |

### Library Sourcing Pattern

```bash
readonly LIB_DIR="${SCRIPT_DIR}/lib"

# Source with existence check
if [[ -f "${LIB_DIR}/common.sh" ]]; then
  # shellcheck source=lib/common.sh
  source "${LIB_DIR}/common.sh"
else
  echo "Error: Required library not found: ${LIB_DIR}/common.sh" >&2
  exit 1
fi
```

## Anti-Patterns (apply-patterns)

### Never Use `\!` in Scripts

```bash
# WRONG -- backslash-escaped ! breaks hooks via security-lib.sh
if \! command -v git &>/dev/null; then

# CORRECT
if ! command -v git &>/dev/null; then
```

This is a critical project-specific rule. The `\!` sequence causes parse failures in security-lib.sh, breaking all PreToolUse hooks.

### Never Hardcode /tmp

```bash
# WRONG -- ignores TMPDIR, breaks sandbox
TEMP="/tmp/myfile"

# CORRECT
TEMP="${TMPDIR:-/tmp}/myfile"

# BEST -- use mktemp
TEMP="$(mktemp)"
```

### Never Use eval for Dynamic Variable Names

```bash
# WRONG -- security risk
eval "value=\$$var_name"

# CORRECT -- use indirect expansion
value="${!var_name}"
```

### Avoid Subshells in Loops for Variable Capture

```bash
# WRONG -- count stays 0 (subshell)
count=0
cat file | while read -r line; do
  ((count++)) || true
done
echo "$count"  # Always 0

# CORRECT -- redirect, no subshell
count=0
while read -r line; do
  ((count++)) || true
done < file
echo "$count"  # Correct value
```

## Quick Checklist (validate-script)

Before requesting commit of any `.sh` file:

- [ ] `#!/usr/bin/env bash` shebang
- [ ] `set -euo pipefail` immediately after header
- [ ] `-h`/`--help` flag implemented
- [ ] `shellcheck -x -s bash` passes (zero SC1xxx errors)
- [ ] All variables quoted (`"$var"`)
- [ ] `local` used for function variables
- [ ] `readonly` used for constants
- [ ] Declare and assign on separate lines (`local x; x=$(cmd)`)
- [ ] Bash 3.2 compatible (no associative arrays, no mapfile, no `${var,,}`)
- [ ] No `\!` anywhere in the script
- [ ] Temp files use `mktemp` + `trap cleanup EXIT`
- [ ] `chmod +x` applied
- [ ] OS-agnostic where applicable (sed, stat, date)
