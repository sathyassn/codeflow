# System Dependencies

This document lists external system tools required by CodeFlow.

## Required Dependencies

### Shell Tools

| Tool | Purpose | Install (macOS) | Install (Linux) |
|------|---------|-----------------|-----------------|
| bash | Shell execution (v4+) | `brew install bash` | Built-in |
| jq | JSON processing | `brew install jq` | `apt install jq` |
| shellcheck | Shell linting | `brew install shellcheck` | `apt install shellcheck` |
| sqlite3 | Schema tests only (Go CLI is sole DB authority) | Built-in | `apt install sqlite3` |

### Python Tools

| Tool | Purpose | Install |
|------|---------|---------|
| python3 | Python 3.10+ | `brew install python@3.12` or system |
| pip | Package management | Included with Python |
| venv | Virtual environments | Included with Python |

## Optional Dependencies

### Testing & Coverage

| Tool | Purpose | Install (macOS) | Install (Linux) |
|------|---------|-----------------|-----------------|
| kcov | Shell script coverage | `brew install kcov` | `apt install kcov` |
| pytest-cov | Python coverage | `pip install pytest-cov` | Same |

### Development Tools

| Tool | Purpose | Install (macOS) | Install (Linux) |
|------|---------|-----------------|-----------------|
| ruff | Python linting | `pip install ruff` | Same |
| flake8 | Python linting | `pip install flake8` | Same |
| gh | GitHub CLI | `brew install gh` | `apt install gh` |

## kcov Notes

### macOS SIP (System Integrity Protection) Issue

On macOS, kcov cannot instrument SIP-protected system binaries like `/bin/bash`.
This causes coverage to fail for scripts using the system bash.

### Solution: Use Homebrew Bash

**Install Homebrew bash:**

```bash
brew install bash
```

**Update script shebangs to use Homebrew bash:**

```bash
#!/opt/homebrew/bin/bash    # Apple Silicon
#!/usr/local/bin/bash       # Intel Mac
```

Or use `#!/usr/bin/env bash` with PATH configured to find Homebrew bash first.

**Why this works:**

- Homebrew binaries are installed in `/opt/homebrew/` (not SIP-protected)
- kcov can instrument non-SIP binaries freely
- Sourced scripts are tracked when the parent uses Homebrew bash

### Verified Working Configuration

```bash
# Script with Homebrew bash shebang
#!/opt/homebrew/bin/bash
source ./lib/common.sh  # This WILL be tracked!

# Run kcov
kcov --include-path=.codeflow/scripts \
     /tmp/coverage-output \
     ./test-script.sh
```

**Results:** Both main script AND sourced scripts get coverage tracked.

### Alternative: bashcov

[bashcov](https://github.com/infertux/bashcov) is a Ruby-based alternative:

- Requires Ruby 3.0+ (macOS ships with 2.6)
- Install: `gem install bashcov` (after installing Ruby 3.0+)
- Uses SimpleCov for HTML reports

### CI Environment

For CI (GitHub Actions, etc.), Linux runners work without these workarounds
since Linux doesn't have SIP restrictions.

### Usage Examples

```bash
# Single test with coverage
kcov --include-path=.codeflow/scripts \
     /tmp/coverage-output \
     ./test-script.sh

# Multiple tests merged
kcov --include-path=.codeflow/scripts /tmp/cov1 ./test1.sh
kcov --include-path=.codeflow/scripts /tmp/cov2 ./test2.sh
kcov --merge /tmp/merged /tmp/cov1 /tmp/cov2

# Debug method (alternative to PS4)
kcov --bash-method=DEBUG \
     --include-path=.codeflow/scripts \
     /tmp/coverage-output \
     ./test-script.sh
```

## Version Requirements

| Tool | Minimum Version | Recommended |
|------|-----------------|-------------|
| bash | 4.0 | 5.0+ |
| python | 3.10 | 3.12+ |
| shellcheck | 0.8 | 0.9+ |
| kcov | 40 | 43+ |
| jq | 1.6 | 1.7+ |

## Installation Script

```bash
#!/bin/bash
# Install all dependencies on macOS
brew install bash jq shellcheck kcov gh

# Create Python venv for testing
cd .codeflow/testing
python3 -m venv .venv
.venv/bin/pip install -r requirements-test.txt
```

## Verification

Run the following to verify installations:

```bash
# Check versions
bash --version | head -1
python3 --version
shellcheck --version | head -1
kcov --version
jq --version
```
