# CodeFlow

AI-native development framework template for Claude Code.

## Overview

CodeFlow provides structured workflows, memory management, and git enforcement for AI-assisted development. It transforms Claude Code into a disciplined development partner with:

- **Structured Agents** - Specialized sub-agents for planning, development, review, QA, and documentation
- **Persistent Memory** - Domain-based memory organization that persists across sessions
- **Git Enforcement** - PR-only workflow with branch protection and commit validation
- **Skill Procedures** - Validated operations that ensure consistent, quality work
- **Coordination** - CRDT-based state management for parallel agent work

## Quick Start

```bash
# Initialize CodeFlow in your project
codeflow init

# Check system health
codeflow doctor

# Start Claude Code
claude

# Use commands
/cf-plan "Add user authentication"
/cf-develop INF-TSK-008-001
/cf-review
```

## Key Commands

| Command | Purpose |
|---------|---------|
| `/cf-resume` | Resume previous work with context |
| `/cf-plan` | Create epics and tasks |
| `/cf-develop` | Implement features/fixes |
| `/cf-review` | Code review |
| `/cf-test` | Run tests |
| `/cf-ship` | Prepare for merge |

## Project Structure

```text
.claude/           # Claude Code configuration
  agents/          # Specialized sub-agents
  commands/        # Slash commands
  skills/          # Validated procedures
  hooks/           # Lifecycle automation
  memory/          # Persistent context

.codeflow/         # CodeFlow infrastructure
  scripts/         # Shell and Python utilities
  testing/         # Test framework
  config/          # Configuration

.state/            # Runtime state
  db/              # SQLite database
  ledger/          # JSONL event logs
```

## Documentation

- [PROJECT.md](PROJECT.md) - Full project context
- [AGENTS.md](AGENTS.md) - External AI instructions
- [CONTRIBUTING.md](CONTRIBUTING.md) - Contribution guide

## Requirements

- Claude Code CLI
- Git 2.25+
- Bash 4.0+
- Python 3.8+ (optional, for Python scripts)
- jq (for JSON processing)

## License

MIT
