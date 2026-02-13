# Project Context

**CRITICAL:** All AI agents MUST review and apply this file before working in this repository.
This provides universal project context for all AI models.

## 1. Project Identity

**Repository:** codeflow
**Type:** AI-native development framework template
**Purpose:** Structured workflow for Claude Code with enforcement, memory, and coordination
**Version:** See `.codeflow/VERSION`

## 2. Purpose

**Problem:** AI-assisted development lacks structured workflows, leading to:

- Inconsistent work patterns across sessions
- Lost context between conversations
- No enforcement of git discipline
- Ad-hoc memory management
- No coordination for parallel work

**Solution:** CodeFlow provides:

- Structured agent-based workflows
- Persistent memory organization by domain
- Git-enforced PR-only development
- Validated skill procedures
- CRDT-based coordination for parallel agents

## 3. Core Concepts

**CRITICAL:** Understand these terms before reading the Architecture section.

| Term | What It Is | Where It Lives |
|------|------------|----------------|
| **Main Agent** | Claude Code with CLAUDE.md instructions; orchestrates all work | `.claude/CLAUDE.md` |
| **Sub-Agent** | Specialized executor for specific domain; invoked by main agent | `.claude/agents/cf-*.md` |
| **Skill** | Validated procedure with operations; agent INVOKES before acting | `.claude/skills/cf-*/SKILL.md` |
| **Command** | User entry point that triggers workflow | `.claude/commands/cf-*.md` |
| **Hook** | Automated behavior at lifecycle events | `.claude/hooks/codeflow/` |
| **Memory Domain** | Persistent work area for specific agent | `.claude/memory/{domain}/` |

**Relationship Summary:**

- **User** invokes **Commands** to start workflows
- **Main Agent** loads command specs and orchestrates work
- **Main Agent** invokes **Sub-Agents** for specialized tasks
- **Sub-Agents** use **Skills** for validated operations
- **Hooks** enforce behavior at key points
- **Memory** persists state across sessions

## 4. Architecture

**Component Relationships:**

```text
┌─────────────────────────────────────────────────────────────────┐
│                         USER                                    │
│                           │                                     │
│                    ┌──────▼──────┐                              │
│                    │   Command   │  /cf-plan, /cf-develop       │
│                    │   (entry)   │  /cf-review, /cf-ship        │
│                    └──────┬──────┘                              │
│                           │                                     │
│                    ┌──────▼──────┐                              │
│                    │ Main Agent  │  Claude Code (CLAUDE.md)     │
│                    │ (orchestr.) │                              │
│                    └──────┬──────┘                              │
│                           │                                     │
│         ┌─────────────────┼─────────────────┐                   │
│         │                 │                 │                   │
│  ┌──────▼──────┐   ┌──────▼──────┐   ┌──────▼──────┐           │
│  │ cf-planner  │   │cf-developer │   │cf-documenter│           │
│  │  (planning) │   │   (code)    │   │   (docs)    │           │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘           │
│         │                 │                 │                   │
│         └─────────────────┼─────────────────┘                   │
│                           │                                     │
│                    ┌──────▼──────┐                              │
│                    │   Skills    │  Validated procedures        │
│                    └──────┬──────┘                              │
│                           │                                     │
│              ┌────────────┼────────────┐                        │
│              │            │            │                        │
│       ┌──────▼──────┐ ┌───▼───┐ ┌──────▼──────┐                │
│       │   Memory    │ │  Git  │ │    Hooks    │                │
│       │ (persistent)│ │(truth)│ │ (enforce)   │                │
│       └─────────────┘ └───────┘ └─────────────┘                │
└─────────────────────────────────────────────────────────────────┘
```

## 5. Workflow

**Typical Work Session:**

```text
1. User starts session
   └─→ SessionStart hook fires
   └─→ Main agent loads CLAUDE.md (imports PROJECT.md)

2. User invokes command: /cf-plan "Add authentication"
   └─→ Main agent loads command spec
   └─→ Main agent validates arguments
   └─→ Main agent creates work-agreement

3. Main agent invokes sub-agent
   └─→ Constructs context bundle
   └─→ Sub-agent executes using skills
   └─→ Sub-agent writes to memory domain

4. Sub-agent completes work
   └─→ Returns results to main agent

5. Main agent presents results to user
```

## 6. Standards

**Git Patterns:**

| Type | Pattern | Example |
|------|---------|---------|
| Planning | `plan/*` | `plan/auth-feature` |
| Features | `feat/*` | `feat/websocket-support` |
| Fixes | `fix/*` | `fix/memory-leak` |
| Documentation | `docs/*` | `docs/api-guide` |
| Experiments | `experiment/*` | `experiment/new-approach` |

**Commit Format:** Conventional commits (`type: description`)

**Script Standards:**

- Shell: shellcheck compliant, uses `.codeflow/scripts/shell-lib/`
- Python: flake8/ruff compliant, uses `.codeflow/scripts/codeflow_py_lib/`

## 7. Security Constraints

**NEVER commit:**

- .env files or secrets
- API keys, passwords, tokens
- Credentials of any kind
- Private keys

**FORBIDDEN operations:**

- `git commit --no-verify` (bypasses hooks)
- `git push --force` to main/master
- `sudo` commands
- Commands modifying system files outside project

## 8. File Organization

```text
codeflow/
├── .claude/                    # Claude Code configuration
│   ├── agents/                 # Sub-agent specifications (cf-*.md)
│   ├── commands/               # Slash command definitions (cf-*.md)
│   ├── hooks/codeflow/         # Lifecycle hooks
│   ├── skills/                 # Validated procedures (cf-*/SKILL.md)
│   ├── memory/                 # Persistent memory by domain
│   └── settings.json           # Permissions and hooks config
├── .codeflow/                  # CodeFlow infrastructure
│   ├── scripts/                # Automation scripts
│   │   ├── codeflow_py_lib/    # Python shared library
│   │   └── shell-lib/          # Shell shared library
│   ├── config/                 # Configuration files
│   ├── testing/                # Test framework
│   └── VERSION                 # CodeFlow version
├── .state/                     # Runtime state
│   ├── db/                     # SQLite database
│   ├── ledger/                 # JSONL event logs
│   ├── coordination/           # CRDT state
│   └── logs/                   # Operation logs
├── project/                    # Project knowledge base
├── project-management/         # Project management
│   ├── epics/                  # Epic/task hierarchy (by area)
│   └── tracking/               # Auto-generated tracking views
├── PROJECT.md                  # This file
├── AGENTS.md                   # External AI instructions
└── codeflow                    # CLI entry point
```

## 9. Available Skills

**CRITICAL:** INVOKE the skill BEFORE performing operations. Do NOT work from memory.

| Skill | Purpose | Location |
|-------|---------|----------|
| cf-working-protocol | Cognitive procedures (7 operations) | `.claude/skills/cf-working-protocol/` |
| cf-git-workflow | Git operations (branching, commits, PRs) | `.claude/skills/cf-git-workflow/` |
| cf-script-standards | Shell and Python conventions | `.claude/skills/cf-script-standards/` |
| cf-documentation-standards | Markdown quality | `.claude/skills/cf-documentation-standards/` |
