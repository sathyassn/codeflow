# PathFlow v3 Documentation

> The session lifecycle framework for CodeFlow, enabled by Claude Code Agent Teams.

**Status**: Design Complete (Milestone 1 Revised)
**Date**: 2026-02-07

---

## Table of Contents

### Core Documentation (This Writer)

| # | Document | Description |
|---|----------|-------------|
| 00 | **README** (this file) | Index, executive summary, terminology glossary |
| 01 | [Agent Teams and PathFlow](01-agent-teams-and-pathflow.md) | Why Agent Teams enables PathFlow; improvements over V3 sub-agents |
| 02 | [System Overview](02-system-overview.md) | Architecture, three-mechanism model, component map |
| 03 | [Teammate Model](03-teammate-model.md) | Function vs role teammates, persistence criteria, spawn timing |
| 04 | [Skills and Agent Definitions](04-skills-and-agent-definitions.md) | Three-tier model, decision trees, embed criteria |
| 05 | [Communication Model](05-communication-model.md) | Direct peer messaging, routing, lead role |

### Mechanics Documentation (Other Writer)

| # | Document | Description |
|---|----------|-------------|
| 06 | [PathFlow Phases](06-progressive-orchestration.md) | PF-1 through PF-7 in detail |
| 07 | [Work Stages](07-work-stages.md) | WS-DEV, WS-REV, WS-QA, WS-WORK; conditional routing |
| 08 | [Sentinel Model](08-enforcement-model.md) | PathFlow sentinels, skill sentinels, defense-in-depth |
| 09 | [Knowledge Layer Integration](09-knowledge-layer-integration.md) | Claude Tasks vs CodeFlow WorkGraph, sync protocol |
| 10 | [Session Lifecycle](10-session-lifecycle.md) | Team lifecycle, tracked vs untracked, interactive vs autorun |
| 11 | [Use Cases and Scenarios](11-use-cases.md) | Concrete workflows for features, bugs, refactors, docs |
| 12 | [Changes from M1](12-changes-and-claude-components.md) | Side-by-side comparison, what changed and why |

---

## Executive Summary

PathFlow is a **logical progression framework** that guides Claude Code sessions through a series of phases, from session start through work execution to session end. It is not a task manager -- it uses Claude's Task system as its implementation mechanism, with phase markers and actual tasks coexisting in the same task list.

PathFlow is made possible by **Claude Code Agent Teams**, which provides capabilities that the V3 sub-agent model could not: persistent teammates, direct peer communication, shared task lists, and message-based coordination. These capabilities transform what was a hub-and-spoke orchestration bottleneck into a distributed team that can self-coordinate.

### Core Architecture

```
PF-1: Session Start
  |
PF-2: Context Awareness       cf-knowledge-layer spawned here
  |
PF-3: Work Classification     cf-gitops spawned here
  |
PF-4: Work Execution          Role teammates spawned on-demand
  |     +-- WS-DEV  (cf-developer)
  |     +-- WS-REV  (cf-reviewer)
  |     +-- WS-QA   (cf-qa)
  |
PF-5: Work Verification
  |
PF-6: Work Completion
  |
PF-7: Session End
```

### Three-Mechanism Model

PathFlow orchestrates sessions through three complementary mechanisms:

| Mechanism | What It Does | Implementation |
|-----------|-------------|----------------|
| **Instructions** | Drive the flow logic | Agent definitions, CLAUDE.md, spawn prompts |
| **Tasks** | Provide visibility into progress | Claude Task system (phase markers + work tasks) |
| **Hooks** | Enforce ordering and safety | PreToolUse/PostToolUse hooks, sentinels |

### Teammate Model

| Category | Persistence | Examples | Purpose |
|----------|:-----------:|---------|---------|
| **Function** | Persistent | cf-gitops, cf-knowledge-layer | Follow SOPs for a specific operational domain |
| **Role** | On-demand | cf-developer, cf-reviewer, cf-qa, cf-planner, cf-documenter, cf-ops | Variable-context work on specific tasks |
| **Ad-hoc** | Varies | security-auditor, perf-analyst, general-purpose | Situational work that predefined roles don't cover |
| **Sub-agents** | Ephemeral | Explore type | Quick read-only lookups |

The predefined roster above represents optimized defaults. The team lead is never limited to predefined roles and can spawn any teammate type supported by Agent Teams. See [Teammate Model](03-teammate-model.md#39-ad-hoc-teammate-flexibility) for details.

### Key Distinction: Claude Tasks vs CodeFlow WorkGraph

| Aspect | Claude Task System | CodeFlow WorkGraph |
|--------|-------------------|-------------------|
| **What** | TaskCreate, TaskList, TaskUpdate | Epics and tasks in SQLite/JSONL/Markdown |
| **Scope** | Single session, ephemeral | Cross-session, permanent |
| **Purpose** | Orchestrate PathFlow phases and teammate assignments | Track project work items, status, planning |
| **Analogy** | Sprint standup board (discarded after) | Jira/Linear project board (persists forever) |

---

## Terminology Glossary

| Term | Definition |
|------|-----------|
| **PathFlow** | The session lifecycle framework. Guides sessions through logical phases (PF-1 through PF-7). |
| **Phase** | A logical milestone in the PathFlow progression. Represented as phase markers in the Claude Task list. Codes: PF-1 through PF-7. |
| **Work Stage** | A step within the Work Execution phase (PF-4). Represents a development pipeline stage. Codes: WS-DEV, WS-REV, WS-QA, WS-WORK. |
| **Phase Marker** | A Claude Task that represents a PathFlow phase. Used for visibility and dependency gating in the task list. |
| **Function Teammate** | A persistent teammate that follows SOPs for a specific operational domain (e.g., cf-gitops for git, cf-knowledge-layer for WorkGraph/Memory/DB). Spawned once, lives for the session. |
| **Role Teammate** | An on-demand teammate spawned for a specific task. Fresh context per assignment. Shut down after task completion. Examples: cf-developer, cf-reviewer, cf-qa. |
| **Team Lead** | The main Claude Code agent that orchestrates the session. Creates the PathFlow task graph, assigns tasks, manages phase transitions, handles escalations. |
| **Claude Task System** | The ephemeral task system provided by Claude Code (TaskCreate, TaskList, TaskUpdate, TaskGet). Used by PathFlow for session orchestration. Tasks are discarded when the session ends. |
| **CodeFlow WorkGraph** | The persistent project management system in CodeFlow. Stores epics and tasks in SQLite/JSONL/Markdown. Survives across sessions. Managed by cf-knowledge-layer. |
| **cf-gitops** | Persistent function teammate for all git operations. Loads cf-git-workflow SOPs. Handles branching, commits, PRs, sync. |
| **cf-knowledge-layer** | Persistent function teammate for the Knowledge Layer. Manages WorkGraph CRUD, memory operations, DB operations, session tracking. Loads cf-memory-management, cf-task-management, and cf-db-operations SOPs. |
| **Progressive Orchestration** | The pattern of creating the next PathFlow phase only when the current phase completes, rather than defining the entire task graph upfront. Enables dynamic adaptation. |
| **Three-Mechanism Model** | PathFlow's implementation approach: Instructions (drive flow logic) + Tasks (provide visibility) + Hooks (enforce ordering and safety). |
| **Tracked Mode** | Session mode where enforcement is active: work must be registered in WorkGraph, verified before completion. Used when editing files or committing code. |
| **Untracked Mode** | Session mode where enforcement is off. Used for exploration, questions, and quick lookups. |
| **Agent Definition** | A markdown file in `.claude/agents/` that defines a teammate's identity, constraints, SOPs, communication patterns, and quality checklist. Loaded at spawn time. |
| **Sentinel** | A state marker that proves a prerequisite action has been completed. Checked by hooks to enforce ordering. PathFlow sentinels are session-scoped (no TTL). |
| **Direct Peer Communication** | The communication model where teammates message each other directly (e.g., cf-developer messages cf-gitops for commits) rather than routing through the team lead. |
| **Conditional Routing** | When a work stage verdict triggers a loop back to a previous stage (e.g., review requests changes, routing back to WS-DEV). |
| **Ad-hoc Teammate** | A teammate spawned for a task that does not fit any predefined role. Created with custom spawn-prompt instructions rather than a pre-built agent definition. The team lead can spawn ad-hoc teammates of any type at any time. |

---

## Design Lineage

This documentation supersedes the M1 PathFlow design (`.codeflow/docs/research/pathflow-design/`). Key changes from M1 are documented in [Changes from M1](12-changes-and-claude-components.md).

The revised design research that informed this documentation is in `.codeflow/docs/research/pathflow-revised/`.

Agent Teams test findings that validated platform capabilities are in `.codeflow/docs/research/agent-teams-test-findings.md`.
