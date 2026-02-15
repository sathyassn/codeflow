---
name: cf-shell-standards
description: Shell scripting standards, patterns, and validation rules for Bash scripts. On-demand reference loaded by cf-development when creating or editing .sh files.
context: on-demand
---

# Shell Standards Skill

## Type

**Procedural** - On-demand reference for shell scripting conventions and validation.

## Purpose

**Quick reference for Bash script structure, naming, compatibility, and lint rules. Loaded by cf-development for `.sh` file work.**

## Script Structure

Every executable script follows this skeleton:

```bash
#!/usr/bin/env bash
# Purpose:   <one-line description>
# Usage:     <script-name> [options]
set -euo pipefail

readonly SCRIPT_NAME="${0##*/}"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() { cat <<EOF
Usage: $SCRIPT_NAME [options]
  -h, --help     Show this help
  -v, --verbose  Enable verbose output
EOF
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -h|--help) usage; exit 0 ;;
      -v|--verbose) VERBOSE=1; shift ;;
      *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
  done
  # Main logic here
}

main "$@"
```

**Library files** (sourced, not executed) use a source guard instead of the entry point:

```bash
[[ -n "${_MY_LIB_LOADED:-}" ]] && return 0
readonly _MY_LIB_LOADED=1
```

Or the direct-execution guard:

```bash
[[ "${BASH_SOURCE[0]}" == "${0}" ]] && { echo "Error: Must be sourced" >&2; exit 1; }
```

## Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| Files (codeflow infra) | `cf-` prefix, kebab-case | `cf-deploy-hooks.sh` |
| Files (general) | kebab-case | `run-tests.sh` |
| Functions | snake_case | `process_file()` |
| Constants / readonly | SCREAMING_SNAKE | `MAX_RETRIES` |
| Local variables | snake_case with `local` | `local file_path` |
| Environment variables | SCREAMING_SNAKE | `CODEFLOW_HOME` |

## Bash 3.2 Compatibility

macOS ships Bash 3.2. All scripts must be compatible.

**Avoid (Bash 4+ only):**

| Feature | Bash Version | Replacement |
|---------|-------------|-------------|
| Associative arrays (`declare -A`) | 4.0+ | Use multiple indexed arrays or `case` statements |
| `mapfile` / `readarray` | 4.0+ | `while IFS= read -r line` loop |
| `${var,,}` / `${var^^}` (case) | 4.0+ | `tr '[:upper:]' '[:lower:]'` |
| `[[ $x =~ $pattern ]]` with stored pattern | 3.2 bug | Inline the regex literal: `[[ $x =~ ^[0-9]+$ ]]` |
| `|&` (pipe stderr) | 4.0+ | `2>&1 |` |
| `coproc` | 4.0+ | Named pipes or temp files |

## Common Patterns

### Temp File Handling

```bash
TEMP_FILE="$(mktemp)"
trap 'rm -f "$TEMP_FILE"' EXIT
```

### Error Handling

```bash
# Errors to stderr
echo "Error: file not found" >&2; exit 1

# Conditional with message
cd "$dir" || { echo "Cannot cd to $dir" >&2; exit 1; }

# Increment from 0 safely under set -e
((count++)) || true
```

### JSON Output with jq

```bash
jq -c -n \
  --arg status "ok" \
  --arg file "$file_path" \
  '{ status: $status, file: $file }'
```

### OS Detection

```bash
case "$OSTYPE" in
  darwin*)  SED_CMD=(sed -i ''); ROOT_GROUP="wheel" ;;
  linux*)   SED_CMD=(sed -i);    ROOT_GROUP="root" ;;
  *)        echo "Unsupported OS" >&2; exit 1 ;;
esac
```

## Lint Rules

Run ShellCheck on every script before commit:

```bash
shellcheck -x -s bash {script}
```

| Severity | Codes | Action |
|----------|-------|--------|
| Error | SC1xxx | Must fix -- blocks commit |
| Warning | SC2xxx | Should fix |
| Info | SC3xxx | Consider fixing |

**Top 5 fixes:** SC2086 (quote variables), SC2155 (declare/assign separately), SC2164 (`cd || exit`), SC2034 (unused variable), SC2129 (group redirects with braces).

**Suppression:** `# shellcheck disable=SC2086` on the line above.

Full ShellCheck reference and examples: [shell-essentials.md](resources/shell-essentials.md)

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| `\!` in scripts | Breaks all hooks via security-lib.sh | Use `!` directly (no backslash) |
| Unquoted `$var` | Word splitting, globbing | Always `"$var"` |
| `local x=$(cmd)` | Masks exit code of `cmd` | `local x; x=$(cmd)` |
| `[ -f $file ]` | Single bracket, unquoted | `[[ -f "$file" ]]` |
| Backticks `` `cmd` `` | Not nestable, unclear | `$(cmd)` |
| `cd dir` without guard | Continues if cd fails | `cd dir \|\| exit 1` |
| `echo $PATH` (unquoted) | Globbing on path components | `echo "$PATH"` |
| Missing `set -euo pipefail` | Silent failures | Always first executable line |
| `readonly x=$(cmd)` | Masks exit code (Bash 3.2) | `local x; x=$(cmd); readonly x` |
| Hardcoded `/tmp` | Ignores TMPDIR, sandbox issues | `"${TMPDIR:-/tmp}"` |

## Quick Checklist

Before requesting commit of any `.sh` file:

- [ ] `set -euo pipefail` present
- [ ] `shellcheck -x -s bash` passes (zero SC1xxx errors)
- [ ] All variables quoted
- [ ] `local` used in functions
- [ ] `readonly` used for constants
- [ ] Bash 3.2 compatible (no associative arrays, no mapfile)
- [ ] No `\!` anywhere in the script
- [ ] Temp files cleaned via `trap ... EXIT`
- [ ] `chmod +x` applied
