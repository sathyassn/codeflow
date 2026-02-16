---
name: cf-shell-standards
description: Shell scripting standards, patterns, and validation rules for Bash scripts. On-demand reference loaded by cf-development when creating or editing .sh files.
---

# Shell Standards Skill

## Type

**Procedural** - On-demand reference for shell scripting conventions and validation.

## Purpose

**Quick reference for Bash script structure, naming, compatibility, and lint rules. Loaded by cf-development for `.sh` file work.**

## Responsibilities

- Define standard script structure and library patterns
- Document naming conventions and Bash 3.2 compatibility rules
- Specify common patterns (temp files, error handling, JSON, OS detection)
- Document ShellCheck lint rules and suppression conventions
- NOT: Linting execution (cf-development SOPs run ShellCheck)
- NOT: Test writing (cf-quality-assurance handles test implementation)

## Decision Tree

```text
Working with .sh file:
├── New executable script? → 🔧 apply-structure (executable skeleton)
├── New library file? → 🔧 apply-structure (library skeleton)
├── Editing existing?
│   ├── Check naming/variables? → 🔧 apply-conventions
│   └── Add logic/patterns? → 🔧 apply-patterns
└── Before commit? → 🔧 validate-script
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-structure | ENF-L3 Advisory | Script/library skeleton and required elements |
| 2 | apply-conventions | ENF-L3 Advisory | Naming rules and Bash 3.2 compatibility |
| 3 | apply-patterns | ENF-L3 Advisory | Common patterns and anti-patterns |
| 4 | validate-script | ENF-L3 Advisory | ShellCheck rules and pre-commit checklist |

## Operation Details

### 🔧 apply-structure

```text
When: Creating a new executable script or library file
Purpose: Apply the correct skeleton with all required elements
Enforcement: ENF-L3 Advisory

Executable Script Skeleton:

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

Library File Skeleton (sourced, not executed):

  Source guard (prevents double-loading):
    [[ -n "${_MY_LIB_LOADED:-}" ]] && return 0
    readonly _MY_LIB_LOADED=1

  Or direct-execution guard:
    [[ "${BASH_SOURCE[0]}" == "${0}" ]] && { echo "Error: Must be sourced" >&2; exit 1; }

Procedure:
  1. Choose skeleton type (executable or library)
  2. Fill in header comments (Purpose, Usage)
  3. Add set -euo pipefail (executables)
  4. Add source guard or execution guard (libraries)
  5. Implement functions and main logic
  6. Run chmod +x on executable scripts

Output: Complete script file with standard structure
```

### 🔧 apply-conventions

```text
When: Naming new elements or reviewing variable/function naming in existing scripts
Purpose: Apply consistent naming rules and ensure Bash 3.2 compatibility
Enforcement: ENF-L3 Advisory

Naming Conventions:

  | Element                    | Convention       | Example            |
  |----------------------------|------------------|--------------------|
  | Files (codeflow infra)     | cf- prefix, kebab-case | cf-deploy-hooks.sh |
  | Files (general)            | kebab-case       | run-tests.sh       |
  | Functions                  | snake_case       | process_file()     |
  | Constants / readonly       | SCREAMING_SNAKE  | MAX_RETRIES        |
  | Local variables            | snake_case with local | local file_path |
  | Environment variables      | SCREAMING_SNAKE  | CODEFLOW_HOME      |

Bash 3.2 Compatibility (macOS ships Bash 3.2 -- all scripts must be compatible):

  Avoid these Bash 4+ features:

  | Feature                           | Bash Version | Replacement                                    |
  |-----------------------------------|-------------|------------------------------------------------|
  | Associative arrays (declare -A)   | 4.0+        | Use multiple indexed arrays or case statements  |
  | mapfile / readarray               | 4.0+        | while IFS= read -r line loop                   |
  | ${var,,} / ${var^^} (case)        | 4.0+        | tr '[:upper:]' '[:lower:]'                     |
  | [[ $x =~ $pattern ]] with stored  | 3.2 bug     | Inline the regex literal: [[ $x =~ ^[0-9]+$ ]] |
  | |& (pipe stderr)                  | 4.0+        | 2>&1 |                                         |
  | coproc                            | 4.0+        | Named pipes or temp files                       |

Procedure:
  1. Check file name follows kebab-case (with cf- prefix for codeflow infra)
  2. Verify functions use snake_case
  3. Verify constants use SCREAMING_SNAKE with readonly
  4. Verify local variables use local keyword
  5. Check for Bash 4+ features and replace with compatible alternatives

Output: Correctly named elements with Bash 3.2 compatible syntax
```

### 🔧 apply-patterns

```text
When: Adding logic, error handling, or utility patterns to a script
Purpose: Use established patterns and avoid known anti-patterns
Enforcement: ENF-L3 Advisory

Common Patterns:

  Temp File Handling:
    TEMP_FILE="$(mktemp)"
    trap 'rm -f "$TEMP_FILE"' EXIT

  Error Handling:
    # Errors to stderr
    echo "Error: file not found" >&2; exit 1

    # Conditional with message
    cd "$dir" || { echo "Cannot cd to $dir" >&2; exit 1; }

    # Increment from 0 safely under set -e
    ((count++)) || true

  JSON Output with jq:
    jq -c -n \
      --arg status "ok" \
      --arg file "$file_path" \
      '{ status: $status, file: $file }'

  OS Detection:
    case "$OSTYPE" in
      darwin*)  SED_CMD=(sed -i ''); ROOT_GROUP="wheel" ;;
      linux*)   SED_CMD=(sed -i);    ROOT_GROUP="root" ;;
      *)        echo "Unsupported OS" >&2; exit 1 ;;
    esac

Anti-Patterns:

  | Anti-Pattern                | Problem                          | Fix                                           |
  |-----------------------------|----------------------------------|-----------------------------------------------|
  | \! in scripts               | Breaks all hooks via security-lib| Use ! directly (no backslash)                 |
  | Unquoted $var               | Word splitting, globbing         | Always "$var"                                 |
  | local x=$(cmd)              | Masks exit code of cmd           | local x; x=$(cmd)                             |
  | [ -f $file ]                | Single bracket, unquoted         | [[ -f "$file" ]]                              |
  | Backticks `cmd`             | Not nestable, unclear            | $(cmd)                                        |
  | cd dir without guard        | Continues if cd fails            | cd dir || exit 1                              |
  | echo $PATH (unquoted)       | Globbing on path components      | echo "$PATH"                                  |
  | Missing set -euo pipefail   | Silent failures                  | Always first executable line                  |
  | readonly x=$(cmd)           | Masks exit code (Bash 3.2)       | local x; x=$(cmd); readonly x                 |
  | Hardcoded /tmp              | Ignores TMPDIR, sandbox issues   | "${TMPDIR:-/tmp}"                             |

Procedure:
  1. Identify the pattern needed (temp files, error handling, JSON, OS detection)
  2. Apply the established pattern from above
  3. Check against anti-patterns table
  4. Verify no anti-patterns are introduced

Output: Script logic using established patterns, free of anti-patterns
```

### 🔧 validate-script

```text
When: Before requesting commit of any .sh file
Purpose: Run ShellCheck and verify script meets all standards
Enforcement: ENF-L3 Advisory

ShellCheck Command:
  shellcheck -x -s bash {script}

Severity Levels:

  | Severity | Codes  | Action                |
  |----------|--------|-----------------------|
  | Error    | SC1xxx | Must fix -- blocks commit |
  | Warning  | SC2xxx | Should fix            |
  | Info     | SC3xxx | Consider fixing       |

Top 5 Fixes:
  SC2086 -- quote variables
  SC2155 -- declare and assign separately
  SC2164 -- cd || exit
  SC2034 -- unused variable
  SC2129 -- group redirects with braces

Suppression:
  # shellcheck disable=SC2086
  (Place on the line above the flagged line)

For the full ShellCheck rule reference with before/after examples, load: resources/shell-essentials.md

Pre-Commit Checklist:

  - [ ] set -euo pipefail present
  - [ ] shellcheck -x -s bash passes (zero SC1xxx errors)
  - [ ] All variables quoted
  - [ ] local used in functions
  - [ ] readonly used for constants
  - [ ] Bash 3.2 compatible (no associative arrays, no mapfile)
  - [ ] No \! anywhere in the script
  - [ ] Temp files cleaned via trap ... EXIT
  - [ ] chmod +x applied

Procedure:
  1. Run shellcheck -x -s bash on the script
  2. Fix any SC1xxx errors (must fix)
  3. Fix SC2xxx warnings (should fix)
  4. Review SC3xxx info items (consider fixing)
  5. Walk through pre-commit checklist
  6. Re-run shellcheck to confirm clean

Output: Clean ShellCheck run and all checklist items satisfied
```

## Resources

Companion resources provide expanded detail beyond the operation summaries above. Load when the inline guidance is insufficient for the task at hand.

| Resource | Companion To | Contains |
|----------|-------------|----------|
| `resources/shell-essentials.md` | validate-script, apply-patterns | Full ShellCheck rule reference, expanded pattern examples, Bash 3.2 compatibility details |
