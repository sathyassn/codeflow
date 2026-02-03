# Contributing to CodeFlow

## Getting Started

1. Clone the repository
2. Run `./codeflow doctor` to verify setup
3. Create a feature branch: `git checkout -b feat/your-feature`

## Development Workflow

### Branch Naming

Use these prefixes:

- `feat/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation changes
- `refactor/` - Code refactoring
- `test/` - Test additions
- `chore/` - Maintenance tasks

### Commit Messages

Follow conventional commit format:

```
type: description (max 50 chars)

- Optional bullet points (max 3)
- Each bullet max 72 chars
```

Valid types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`, `build`, `ci`

### Pull Requests

1. Push your branch: `git push -u origin feat/your-feature`
2. Create PR with:
   - Clear title (`type: description`)
   - `## Summary` section
   - `## Testing` section

## Code Standards

### Shell Scripts

- Use `#!/usr/bin/env bash`
- Include `set -euo pipefail`
- Pass shellcheck
- Source `.codeflow/scripts/shell-lib/index.sh` for utilities

### Python Scripts

- Python 3.8+
- Use type hints
- Pass flake8
- Import from `codeflow_py_lib` for shared utilities

### Documentation

- Use ATX headers (`#`, `##`, etc.)
- Include code examples where helpful
- Keep lines under 100 characters

## Testing

Run the test suite:

```bash
./.codeflow/testing/run-all-tests.sh
```

## Questions?

Open an issue or start a discussion.
