# CodeFlow Agent Instructions

This file provides core instructions for AI agents working within CodeFlow.

## Overview

CodeFlow is an AI-native development framework that provides:

- Structured workflow management
- Memory persistence across sessions
- Multi-agent coordination
- Automated quality enforcement

## Key Principles

1. **Follow the Protocol**: Use cf-working-protocol for cognitive procedures
2. **Git Workflow**: All changes through PRs, never commit directly to main
3. **Documentation**: Update docs alongside code changes
4. **Testing**: Verify changes with appropriate tests

## Skills

Skills are loaded from `.claude/skills/` and provide specialized capabilities:

- **cf-working-protocol**: Cognitive procedures (think-and-act, decide, verify)
- **cf-git-workflow**: Git operations and branching conventions
- **cf-script-standards**: Shell and Python scripting conventions
- **cf-documentation-standards**: Markdown and documentation standards

## Directory Structure

```text
.claude/           # Claude-specific configuration
.codeflow/         # CodeFlow infrastructure
.state/            # Runtime state (gitignored)
project/           # Project documentation
epics/             # Epic tracking
```

## Getting Started

1. Read PROJECT.md for project context
2. Review relevant skills for your task
3. Follow git-workflow for all changes
4. Verify work before marking complete

---

*This file will be expanded as CodeFlow infrastructure is built.*
