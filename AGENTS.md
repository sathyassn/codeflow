# External AI Instructions

This file provides instructions for AI tools other than Claude Code (e.g., Copilot, Cursor, other assistants).

## Project Overview

**CodeFlow** is an AI-native development framework template for Claude Code. It provides structured workflows, memory management, and git enforcement for AI-assisted development.

## Tech Stack

- **Shell Scripts:** Bash (shellcheck compliant)
- **Python Scripts:** Python 3.8+ (flake8 compliant)
- **Configuration:** JSON, YAML
- **Database:** SurrealDB (for state management)
- **Version Control:** Git with PR-only workflow

## Key Directories

| Directory | Purpose |
|-----------|---------|
| `.claude/` | Claude Code configuration (agents, commands, skills, hooks) |
| `.codeflow/` | CodeFlow infrastructure (scripts, config, testing) |
| `.state/` | Runtime state (database, logs, coordination) |
| `project/` | Project knowledge base |
| `project-management/` | Project management (epics, tracking) |

## Coding Standards

### Shell Scripts

- Use `#!/usr/bin/env bash`
- Include `set -euo pipefail`
- Source shared library: `.codeflow/scripts/shell-lib/index.sh`
- Follow shellcheck recommendations

### Python Scripts

- Use type hints
- Import from `codeflow_py_lib` for shared utilities
- Follow PEP 8 style guide
- Include docstrings for functions

### Markdown

- Use ATX-style headers (`#`)
- Include frontmatter where appropriate
- Keep lines under 100 characters
- Use fenced code blocks with language identifiers

## Git Workflow

- **Branch naming:** `feat/`, `fix/`, `docs/`, `plan/`, `experiment/` prefixes
- **Commits:** Conventional commit format (`type: description`)
- **No direct commits to main** - use pull requests

## Testing

Test configuration lives at `.codeflow/config/testing/test-config.json`. Test
bodies live under `.codeflow/testing/`. Run the full suite with:

```bash
codeflow test --mode full
```

## Important Files

| File | Purpose |
|------|---------|
| `PROJECT.md` | Full project context |
| `.claude/settings.json` | Permissions and hooks |
| `.codeflow/VERSION` | CodeFlow version |
| `codeflow` | CLI entry point |
