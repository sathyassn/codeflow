# Project Context

**CRITICAL:** All AI agents MUST review and apply this file before working in this repository.
This provides universal project context for all AI models.

## 1. Project Identity

**Repository:** codeflow
**Type:** AI-native development framework template
**Purpose:** Structured multi-agent workflow for Claude Code with enforcement, memory, and coordination
**Architecture:** Agent Teams (Claude Code native) with PathFlow session lifecycle
**Version:** See `.codeflow/VERSION`

## 2. Purpose

**Problem:** AI-assisted development lacks structured workflows, leading to:

- Inconsistent work patterns across sessions
- Lost context between conversations
- No enforcement of git discipline
- Ad-hoc memory management
- No coordination for parallel work -- SOLVED by INF-EPC-023 (parallel execution)

**Solution:** CodeFlow provides:

- Multi-agent team with specialized teammates (8 agents + team lead)
- PathFlow session lifecycle (7 phases: PF1-INIT through PF7-END)
- Persistent memory via three-tier data model (JSONL + SurrealDB + Markdown)
- Hook-enforced PR-only development (19 hooks across 6 lifecycle events)
- Work type pipelines with independent review and quality gates
- Parallel execution via git worktree isolation, CRDT-based coordination, file-level claims, and serialized merge queue

## 3. Core Concepts

**CRITICAL:** Understand these terms before reading the Architecture section.

| Term | What It Is | Where It Lives |
|------|------------|----------------|
| **Team Lead** | Claude Code with CLAUDE.md instructions; orchestrates all work, delegates to teammates | `.claude/CLAUDE.md` |
| **Teammate** | Specialized agent spawned via Agent Teams; operates within defined constraints | `.claude/agents/cf-*.md` |
| **Skill** | Behavioral procedure with operations; loaded at session start | `.claude/skills/cf-*/SKILL.md` |
| **Command** | User entry point that triggers teammate spawning | `.claude/commands/cf-*.md` |
| **Hook** | Automated enforcement at lifecycle events (PreToolUse, Stop, etc.) | `.claude/hooks/codeflow/` |
| **PathFlow** | 7-phase session lifecycle from init to cleanup | CLAUDE.md Section 4 |
| **WorkGraph** | Task tracking system (JSONL source of truth + SurrealDB query layer) | `.state/ledger/`, `.state/db/` |

**Relationship Summary:**

- **User** invokes **Commands** (e.g., `/cf-develop`) to start workflows
- **Team Lead** classifies work and spawns **Teammates** at the appropriate PathFlow phase
- **Teammates** execute specialized work within their defined constraints (SOPs)
- **Teammates** communicate directly for routine operations (peer-to-peer messaging)
- **Hooks** enforce workflow compliance at the tool-call level
- **WorkGraph** persists task state and session context across sessions

## 4. Architecture

### Agent Teams

```text
┌───────────────────────────────────────────────────────────────────────┐
│                              USER                                     │
│                                │                                      │
│                         ┌──────▼──────┐                               │
│                         │  Team Lead  │  CLAUDE.md (orchestrator)      │
│                         │  (router)   │  Delegates, never codes        │
│                         └──────┬──────┘                               │
│                                │                                      │
│            ┌───────────────────┼───────────────────┐                  │
│            │ PERSISTENT (PF1-PF7)                  │                  │
│   ┌────────▼────────┐  ┌──────▼──────┐  ┌─────────▼────────┐         │
│   │  cf-security    │  │cf-knowledge │  │   cf-git-operations      │         │
│   │  (PF1-INIT)     │  │  -layer     │  │  (PF3-CLASSIFY)  │         │
│   │  sandbox, perms  │  │(PF2-CONTEXT)│  │  branch, commit  │         │
│   └─────────────────┘  │ memory, DB  │  │  PR, sync        │         │
│                         └─────────────┘  └──────────────────┘         │
│                                                                       │
│            ┌───────────────────┼───────────────────┐                  │
│            │ ON-DEMAND (PF4-EXECUTE only)           │                  │
│   ┌────────▼────────┐  ┌──────▼──────┐  ┌─────────▼────────┐         │
│   │  cf-development   │  │ cf-planning  │  │ cf-documentation    │         │
│   │  WS-DEV (code)  │  │ WS-PLAN     │  │ WS-DOCS          │         │
│   └─────────────────┘  └─────────────┘  └──────────────────┘         │
│   ┌─────────────────┐  ┌─────────────┐                               │
│   │  cf-review    │  │   cf-quality-assurance     │                               │
│   │  WS-REV (4mode) │  │ WS-QA/TEST │                               │
│   └─────────────────┘  └─────────────┘                               │
│                                                                       │
│   ┌───────────────────────────────────────────────────────────┐       │
│   │                    ENFORCEMENT LAYER                       │       │
│   │  20 hooks: pathflow-gate, team-guard, edit-write,         │       │
│   │  protected-resource, read-delegation, security, webfetch  │       │
│   └───────────────────────────────────────────────────────────┘       │
│                                                                       │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐       │
│   │   WorkGraph   │  │    Git       │  │      Memory          │       │
│   │JSONL+SurrealDB│  │  PR-only    │  │  3-tier persistent   │       │
│   └──────────────┘  └──────────────┘  └──────────────────────┘       │
│                                                                       │
│   ┌───────────────────────────────────────────────────────────┐       │
│   │               PARALLEL EXECUTION LAYER                    │       │
│   │  Worktree isolation, CRDT coordination (Loro),            │       │
│   │  file-level claims, fencing tokens, merge queue           │       │
│   └───────────────────────────────────────────────────────────┘       │
└───────────────────────────────────────────────────────────────────────┘
```

### PathFlow Session Lifecycle

```text
PF1-INIT ──▶ PF2-CONTEXT ──▶ PF3-CLASSIFY ──▶ PF4-EXECUTE ──▶ PF5-VERIFY ──▶ PF6-COMPLETE ──▶ PF7-END
  │              │                │                 │                              │
  │              │                │                 │                              │
  spawn          spawn            spawn          spawn on-demand              create PR
  cf-security    cf-knowledge     cf-git-operations      per work stage              shutdown
                 -layer           create branch                              teammates
```

### Work Type Pipelines (PF4-EXECUTE)

| Work Type | Pipeline | Primary Teammate |
|-----------|----------|------------------|
| FEAT, FIX, RFCT, CICD | WS-DEV --> WS-REV --> WS-QA | cf-development |
| HTFX, CHOR | WS-DEV --> WS-REV | cf-development |
| DOCS | WS-DOCS --> WS-REV | cf-documentation |
| TEST | WS-TEST --> WS-REV | cf-quality-assurance |
| PLAN, SPKE | WS-PLAN --> WS-REV | cf-planning |

## 5. Workflow

**Typical Tracked Session:**

```text
1. PF1-INIT: Register session, spawn cf-security
   └─▶ SessionStart hook fires, team infrastructure created

2. PF2-CONTEXT: Spawn cf-knowledge-layer, check active work
   └─▶ Resume previous work OR start fresh

3. PF3-CLASSIFY: Classify work type/area, spawn cf-git-operations, create branch
   └─▶ Task registered in WorkGraph, pathflow sentinel created

4. PF4-EXECUTE: Run work pipeline (stage sequence per work type)
   └─▶ On-demand teammates spawned per stage (WS-DEV, WS-REV, WS-QA, etc.)
   └─▶ Teammates communicate peer-to-peer for routine ops
   └─▶ Rework loops: WS-REV can send back to primary stage (max 3 iterations)

5. PF5-VERIFY: Confirm all stages passed, acceptance criteria met

6. PF6-COMPLETE: cf-git-operations creates PR, cf-knowledge-layer marks task complete

7. PF7-END: Shutdown all teammates, write session summary, TeamDelete
```

**Untracked sessions** (questions, exploration) skip PF3-PF6 and go directly to PF7-END.

## 6. Standards

**Git Patterns:**

| Type | Branch Prefix | Example |
|------|---------------|---------|
| Features | `feat/*` | `feat/websocket-support` |
| Fixes | `fix/*` | `fix/memory-leak` |
| Planning | `plan/*` | `plan/auth-feature` |
| Documentation | `docs/*` | `docs/api-guide` |
| Experiments | `experiment/*` | `experiment/new-workflow` |
| Tests | `test/*` | `test/add-commit-validation` |
| CI/CD | `ci/*` | `ci/add-github-actions` |
| Hotfixes | `hotfix/*` | `hotfix/security-patch` |
| Refactoring | `refactor/*` | `refactor/extract-lib` |
| Chores | `chore/*` | `chore/update-deps` |

**Commit Format:** Conventional commits (`type(scope): description`), enforced by cf-git-operations.

**Script Standards:**

- Shell: shellcheck compliant, `set -euo pipefail`, uses `.codeflow/scripts/security/lib/`
- Python: ruff/flake8 compliant, uses `.codeflow/scripts/codeflow_py_lib/`

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
- Direct commits to main (all changes through PRs)

**Enforcement:** 19 hooks provide defense-in-depth. Protected resources (CLAUDE.md, settings.json) are routed through a staging workflow requiring user approval.

## 8. File Organization

```text
codeflow/
├── .claude/                          # Claude Code configuration
│   ├── CLAUDE.md                     # Team lead instructions (608 lines)
│   ├── agents/                       # 8 teammate definitions (cf-*.md)
│   │   ├── cf-security.md            #   Persistent: sandbox, permissions
│   │   ├── cf-knowledge-layer.md     #   Persistent: WorkGraph, memory, DB
│   │   ├── cf-git-operations.md              #   Persistent: git operations
│   │   ├── cf-development.md           #   On-demand: code implementation
│   │   ├── cf-planning.md             #   On-demand: design, architecture
│   │   ├── cf-documentation.md          #   On-demand: documentation
│   │   ├── cf-review.md            #   On-demand: independent review
│   │   └── cf-quality-assurance.md                  #   On-demand: QA gate / test writer
│   ├── skills/                       # 1 active skill
│   │   └── cf-working-protocol/      #   Team lead cognitive procedures
│   ├── hooks/codeflow/               # 19 hook scripts by event type
│   │   ├── session-start/            #   3 scripts (cleanup, instructions, logging)
│   │   ├── user-prompt-submit/       #   2 scripts (validation, logging)
│   │   ├── pre-tool-use/             #   7 scripts (pathflow-gate, team-guard, etc.)
│   │   ├── post-tool-use/            #   3 scripts (logging, settings, tmp-workflow)
│   │   ├── stop/                     #   2 scripts (pathflow-gate, logging)
│   │   └── session-end/              #   2 scripts (cleanup, logging)
│   ├── commands/                     # Slash command definitions
│   ├── memory/                       # Persistent memory by domain
│   └── settings.json                 # Permissions, hooks, PathFlow config
├── .codeflow/                        # CodeFlow infrastructure
│   ├── config/                       # Enforcement policies
│   │   ├── enforcement/              #   enforcement-policy.json
│   │   └── pathflow/                 #   pathflow-config.json (phases, stages, pipelines, teammates, rework)
│   ├── scripts/security/lib/         # Security libraries (security-lib.sh, context-lib.sh)
│   ├── scripts/codeflow_py_lib/      # Python shared library
│   ├── testing/                      # Test suite (1,555+ tests)
│   ├── docs/archived/skills/         # 9 archived skills (reference only)
│   └── VERSION                       # CodeFlow version
├── .git-worktrees/                   # Git worktrees for parallel sessions
├── .state/                           # Runtime state (partially gitignored)
│   ├── db/codeflow.db                # Tier 1: SurrealDB (query interface)
│   ├── coordination/                 # CRDT state (state.loro) for claims + merge queue
│   ├── ledger/                       # Tier 0: JSONL event logs (rebuild authority)
│   ├── runtime/                      # Active task, session ID
│   ├── sentinels/                    # PathFlow sentinels
│   └── worktrees.yaml                # Worktree registry (active/removed entries)
├── project/                          # Project knowledge base
├── project-management/               # Tier 2: Human-readable work tracking
│   ├── epics/                        # Epic markdown files
│   └── tracking/                     # Progress tracking
├── PROJECT.md                        # This file (project context)
├── AGENTS.md                         # External AI instructions (non-Claude)
└── codeflow                          # CLI entry point
```

## 9. Capabilities

### Active Skill (1)

| Skill | Purpose | Location |
|-------|---------|----------|
| cf-working-protocol | Team lead cognitive procedures (5 operations: meta-awareness, think-and-act, decide, respond-organized, research-quality) | `.claude/skills/cf-working-protocol/` |

9 skills archived to `.codeflow/docs/archived/skills/`. Their operations are now embedded directly in agent definitions.

### Agent Definitions (8)

| Agent | Category | Spawned At | Purpose |
|-------|----------|-----------|---------|
| cf-security | Persistent | PF1-INIT | Security consultation, sandbox validation |
| cf-knowledge-layer | Persistent | PF2-CONTEXT | WorkGraph CRUD, memory ops, DB operations |
| cf-git-operations | Persistent | PF3-CLASSIFY | Branch, commit, PR, sync operations |
| cf-development | On-demand | PF4-EXECUTE (WS-DEV) | Code implementation + unit tests |
| cf-planning | On-demand | PF4-EXECUTE (WS-PLAN) | Design, architecture, analysis |
| cf-documentation | On-demand | PF4-EXECUTE (WS-DOCS) | Documentation writing |
| cf-review | On-demand | PF4-EXECUTE (WS-REV) | Independent review (4 modes) |
| cf-quality-assurance | On-demand | PF4-EXECUTE (WS-QA/WS-TEST) | QA gate or primary test writer |

### Three-Tier Data Model

| Tier | Location | Purpose | Git Tracked |
|------|----------|---------|-------------|
| **0 (JSONL)** | `.state/ledger/*.jsonl` | Rebuild authority -- immutable, append-only | Yes |
| **1 (SurrealDB)** | `.state/db/codeflow.db` | Query interface -- fast indexed lookups | No |
| **2 (Markdown)** | `project-management/`, `.claude/memory/` | Human-readable derived views | Yes |

### Parallel Execution

| Component | Module | Purpose |
|-----------|--------|---------|
| WorktreeManager | `codeflow-cli/core/src/worktree/mod.rs` | Creates/cleans isolated git worktrees per session |
| WorktreePaths | `codeflow-cli/core/src/worktree/paths.rs` | Canonical path resolver for worktree-scoped state |
| WorktreeRegistry | `codeflow-cli/core/src/worktree/registry.rs` | YAML-based tracking with locked concurrency control (max 3) |
| Coordinator (CRDT) | `codeflow-cli/core/src/coordination/mod.rs` | Loro CRDT-based file-level claims with fencing tokens |
| MergeQueue | `codeflow-cli/core/src/coordination/merge_queue.rs` | FIFO queue for serialized PR merges across sessions |
| ConflictDetection | `codeflow-cli/core/src/git/conflict.rs` | In-memory merge conflict check before PR creation |

### Commands (14)

```text
/cf-resume   /cf-plan       /cf-develop   /cf-review    /cf-test
/cf-ship     /cf-deploy     /cf-document  /cf-cleanup   /cf-stack
/cf-approval-mode   /cf-help   /cf-doctor   /cf-autorun
```
