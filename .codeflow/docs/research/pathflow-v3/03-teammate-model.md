# 03: Teammate Model

> Function teammates vs role teammates: persistence criteria, the complete roster, spawn timing, and lifecycle management.

---

## Table of Contents

- [3.1 Teammate Categories](#31-teammate-categories)
- [3.2 Function Teammates](#32-function-teammates)
- [3.3 Role Teammates](#33-role-teammates)
- [3.4 Complete Teammate Roster](#34-complete-teammate-roster)
- [3.5 Persistence Criteria](#35-persistence-criteria)
- [3.6 Spawn Timing](#36-spawn-timing)
- [3.7 Lifecycle Management](#37-lifecycle-management)
- [3.8 Why NOT Other Function Teammates](#38-why-not-other-function-teammates)
- [3.9 Ad-Hoc Teammate Flexibility](#39-ad-hoc-teammate-flexibility)

---

## 3.1 Teammate Categories

PathFlow uses three categories of agents, each with a different persistence model and purpose. The predefined teammates listed below are **optimized defaults for common workflows**, not a hard constraint. The team lead retains full ability to spawn ad-hoc teammates of any type when the situation warrants it (see [3.9 Ad-Hoc Teammate Flexibility](#39-ad-hoc-teammate-flexibility)).

```
+-------------------------------------------------------------------+
|                     TEAMMATE CATEGORIES                            |
+-------------------------------------------------------------------+
|                                                                     |
|  FUNCTION TEAMMATES    ROLE TEAMMATES       AD-HOC       SUB-AGENTS|
|  (Persistent SOPs)     (On-Demand Work)   TEAMMATES     (Ephemeral)|
|                                           (Situational)            |
|  +----------------+  +----------------+  +------------+ +---------+|
|  | cf-gitops      |  | cf-developer   |  | (any type) | | Explore ||
|  | cf-knowledge-  |  | cf-reviewer    |  | security-  | |(built-in|
|  |   layer        |  | cf-qa          |  |   auditor  | +---------+|
|  +----------------+  | cf-planner     |  | perf-      |           |
|                      | cf-documenter  |  |   analyst  |           |
|  Loaded: once        | cf-ops         |  | general-   |           |
|  Lives: full session +----------------+  |   purpose  |           |
|  Context: accumulates                    +------------+           |
|  Purpose: SOPs       Loaded: per task                             |
|                      Lives: one stage    Loaded: as needed        |
|                      Context: fresh      Lives: varies            |
|                      Purpose: var. work  Purpose: situational     |
|                                                                     |
+-------------------------------------------------------------------+
```

The key distinction: **function teammates** benefit from context accumulation (knowing what branches exist, what's been committed, current task status), while **role teammates** benefit from fresh context (no stale assumptions from previous tasks). **Ad-hoc teammates** are spawned when the predefined roster does not cover the need.

---

## 3.2 Function Teammates

Function teammates are persistent agents that follow Standard Operating Procedures (SOPs) for a specific operational domain. They are spawned once and live for the entire session.

### cf-gitops

**Domain**: All git operations.

**Source Skills**: cf-git-workflow (full SOP embedded in agent definition).

**Operations**: Branch creation, commits, PRs, sync, merge, rebase, review-changes.

**Why persistent?**
- Git operations are procedural and convention-heavy (branch naming, commit format, PR templates)
- Git SOPs are loaded once and reused for every operation
- Accumulated context is valuable: what branches exist, what's been committed, PR status
- Other teammates call on cf-gitops directly for git work -- it must be available throughout

**Communication pattern**:
- Receives commit requests from: cf-developer, cf-qa, cf-documenter
- Receives PR requests from: team lead, cf-ops
- Reports to team lead: branch created, PR status, sync status
- Updates cf-knowledge-layer: after significant git events

### cf-knowledge-layer

**Domain**: The Knowledge Layer (WorkGraph, Memory, DB).

**Source Skills**: cf-memory-management, cf-task-management, cf-db-operations (all SOPs embedded).

**Operations**: Work detection, context loading, begin-work, record-progress, complete-work, WorkGraph CRUD (create/update epics and tasks), session records.

**Why persistent?**
- Knowledge Layer operations happen throughout the entire session (from PF-2 through PF-7)
- Three different skill SOPs are composited into one agent -- loading once saves significant context
- Accumulated context is valuable: what work is active, what progress has been recorded, current WorkGraph state
- It is the single interface between the session and persistent storage -- centralizing this prevents every teammate from needing to understand DB operations

**Communication pattern**:
- Receives stage-completion updates from: cf-developer, cf-reviewer, cf-qa
- Receives progress recording requests from: any teammate
- Reports to team lead: work detected, context loaded, WorkGraph updates
- Does not communicate with: cf-gitops (no direct interaction needed)

---

## 3.3 Role Teammates

Role teammates are on-demand agents spawned for specific tasks. They get fresh context per assignment and are shut down after task completion.

| Role | Purpose | Typical Stage | Source Skills |
|------|---------|---------------|---------------|
| **cf-developer** | Implementation work | WS-DEV | cf-script-standards (Reference on Demand) |
| **cf-reviewer** | Code review | WS-REV | cf-code-exploration, cf-script-standards (Reference on Demand) |
| **cf-qa** | Testing and quality | WS-QA | cf-testing-workflow, cf-script-standards (Reference on Demand) |
| **cf-planner** | Planning and task breakdown | PF-3 (pre-execution) | cf-documentation-standards (Reference on Demand) |
| **cf-documenter** | Technical documentation | WS-DEV (docs tasks) | cf-documentation-standards (Reference on Demand) |
| **cf-ops** | DevOps, CI/CD, deployment | PF-6 | (deployment procedures) |

**Why on-demand?**
- Each task brings new, different context (different files, different requirements)
- Context accumulation from previous tasks is counterproductive -- stale context from task A may confuse task B
- Fresh context per task produces higher quality work
- Resource-efficient: only alive when needed

**cf-ops is for DevOps**, not git operations. cf-ops handles deployment pipelines, CI/CD configuration, and infrastructure. Git operations (branching, commits, PRs) are cf-gitops territory.

---

## 3.4 Complete Teammate Roster

```
+-------------------------------------------------------------------+
|  TEAM LEAD                                                         |
|  Role: Orchestrator                                                |
|  Persistence: PERSISTENT (is the session itself)                   |
|  Source: cf-working-protocol (embedded in CLAUDE.md)               |
|  Purpose: PathFlow management, task assignment, decisions          |
+-------------------------------------------------------------------+
     |
     |--- FUNCTION TEAMMATES (persistent, session lifetime)
     |    |
     |    +-- cf-gitops
     |    |   Source: cf-git-workflow
     |    |   Purpose: All git operations following conventions
     |    |
     |    +-- cf-knowledge-layer
     |        Source: cf-memory-management + cf-task-management
     |                + cf-db-operations
     |        Purpose: WorkGraph CRUD, memory ops, session tracking
     |
     |--- ROLE TEAMMATES (on-demand, per task/stage)
     |    |
     |    +-- cf-developer
     |    |   Purpose: Implementation work
     |    |
     |    +-- cf-reviewer
     |    |   Purpose: Code review
     |    |
     |    +-- cf-qa
     |    |   Purpose: Test writing and execution
     |    |
     |    +-- cf-planner
     |    |   Purpose: Planning, epic/task breakdown
     |    |
     |    +-- cf-documenter
     |    |   Purpose: Technical documentation
     |    |
     |    +-- cf-ops
     |        Purpose: DevOps, CI/CD, deployment
     |
     |--- AD-HOC TEAMMATES (spawned when predefined roles don't fit)
          |
          +-- (any general-purpose teammate)
          +-- (any specialized one-off: security-auditor, perf-analyst, etc.)
          +-- (Explore-type sub-agents for quick lookups)
```

**Design principle**: The roster above represents the recommended defaults for common development workflows. The team lead is **never limited to predefined roles** and can spawn any teammate type supported by Claude Code's Agent Teams when the situation warrants it. See [3.9 Ad-Hoc Teammate Flexibility](#39-ad-hoc-teammate-flexibility) for details.

---

## 3.5 Persistence Criteria

A teammate should be **persistent (function)** when ALL FOUR of these conditions are met:

| # | Condition | Rationale |
|---|-----------|-----------|
| 1 | Operations are **procedural** (follow SOPs exactly) | The value is in consistent execution of defined procedures |
| 2 | Operations are **repeated** (many times per session) | Loading SOPs once and reusing saves context across the session |
| 3 | Context accumulation is **beneficial** (knowing history helps) | The teammate makes better decisions by knowing what it has already done |
| 4 | Multiple teammates **depend on it** (service role) | Other teammates need to call on it throughout the session |

Applying these criteria to validate the current roster:

| Candidate | Procedural? | Repeated? | Accumulation Beneficial? | Depended On? | Verdict |
|-----------|:-----------:|:---------:|:------------------------:|:------------:|---------|
| cf-gitops | Yes (branch naming, commit format) | Yes (many commits/session) | Yes (knows branches, PR state) | Yes (devs, QA commit through it) | **FUNCTION** |
| cf-knowledge-layer | Yes (DB operations, JSONL format) | Yes (updates throughout session) | Yes (knows active work, progress) | Yes (all teammates report to it) | **FUNCTION** |
| cf-developer | No (variable implementation) | Sometimes | No (stale context hurts) | No (works independently) | **ROLE** |
| cf-reviewer | Partially (review checklist) | Sometimes | No (each review is fresh) | No | **ROLE** |
| cf-qa | Partially (test patterns) | Sometimes | No (each test suite is fresh) | No | **ROLE** |
| cf-security | Yes (hook-based) | Yes | N/A | N/A | **NOT A TEAMMATE** (hooks handle it) |

---

## 3.6 Spawn Timing

Teammates are spawned **progressively** as the session advances through phases. Not all teammates are needed from the start.

```
SPAWN TIMELINE
==============

PF-1: Session Start
  |
  +-- (no teammates needed yet for simple sessions)
  |
PF-2: Context Awareness
  |
  +-- SPAWN cf-knowledge-layer (persistent)
  |   Needed for: work detection, context loading
  |   Stays alive until: PF-7
  |
PF-3: Work Classification
  |
  +-- SPAWN cf-gitops (persistent)
  |   Needed for: branch creation (development work confirmed)
  |   Stays alive until: PF-7
  |
  +-- SPAWN cf-planner (on-demand, if planning is needed)
  |   Shut down after: plan delivered
  |
PF-4: Work Execution
  |
  +-- WS-DEV:
  |   SPAWN cf-developer (on-demand)
  |   Shut down after: WS-DEV completes (or kept alive for rework loops)
  |
  +-- WS-REV:
  |   SPAWN cf-reviewer (on-demand)
  |   Shut down after: WS-REV completes
  |
  +-- WS-QA:
  |   SPAWN cf-qa (on-demand)
  |   Shut down after: WS-QA completes
  |
  +-- PF-6: cf-gitops creates PR (already alive)
  |
PF-5-6: Verification and Completion
  |
  +-- cf-knowledge-layer and cf-gitops handle remaining work
  |
PF-7: Session End
  |
  +-- SHUTDOWN cf-gitops
  +-- SHUTDOWN cf-knowledge-layer
  +-- CLEANUP team
```

**Deferred spawning rationale**:
- cf-knowledge-layer is deferred to PF-2 (not PF-1) because PF-1 is infrastructure bootstrapping that hooks handle automatically
- cf-gitops is deferred to PF-3 because there may be no development work (simple Q&A sessions skip it entirely)
- Role teammates are deferred to their respective work stages because spawning them earlier wastes resources
- For simple sessions (no file editing), no teammates may be spawned at all

---

## 3.7 Lifecycle Management

### Spawn Protocol

When the team lead spawns a teammate, it follows this protocol:

1. **Determine need**: Based on current phase and work requirements
2. **Build spawn prompt**: Include task-specific instructions and "Read your agent definition at `.claude/agents/{type}.md`"
3. **Spawn via Task tool**: Using the appropriate `subagent_type` and unique `name`
4. **Assign initial task**: Via TaskUpdate (set owner) + SendMessage (provide context)

The spawn prompt must include the instruction to read the agent definition file because `subagent_type` alone does NOT auto-inject the definition into the teammate's context (validated behavior -- see [Agent Teams and PathFlow](01-agent-teams-and-pathflow.md#16-validated-platform-behaviors)).

### Shutdown Protocol

1. **Lead sends shutdown_request** to the teammate via SendMessage
2. **Teammate confirms** via shutdown_response (approve: true)
3. **Teammate is removed** from the team config
4. If no confirmation within a reasonable time, lead sends a follow-up reminder

**Shutdown timing for role teammates**: Shut down after their work stage completes. Exception: during rework loops (review requests changes), keep the developer alive rather than shutting down and respawning.

**Shutdown timing for function teammates**: Shut down at PF-7 (Session End), after all work is finalized.

### Recycling (Context Recovery)

If a persistent teammate's context fills up (auto-compaction at ~95% is the only relief), the lead can recycle it:

1. Shut down the teammate
2. Respawn with the same name and agent type
3. The new instance starts with fresh context
4. Re-send any necessary state via messages

This should be rare for function teammates whose context is bounded (SOPs + session operations).

---

## 3.8 Why NOT Other Function Teammates

Several candidates were evaluated and rejected for function teammate status:

| Candidate | Verdict | Reasoning |
|-----------|---------|-----------|
| **cf-security** | Not a teammate | Security enforcement lives in hooks (automatic, no teammate needed). Sandbox-check is a single lightweight operation. Protected resource staging is handled by the editing teammate with hook enforcement. |
| **cf-documentation-standards** | Not a teammate | Quality standards are checks applied TO work products, not standalone operations. The documenter and planner follow doc standards as part of their role. Embedded in relevant role teammate blueprints. |
| **cf-script-standards** | Not a teammate | Same reasoning as cf-documentation-standards. Developers and QA follow script standards as part of their role. Embedded in cf-developer and cf-qa blueprints. |
| **cf-model-orchestrator** | Deferred (future) | External model delegation could warrant a persistent function teammate if used heavily. Not needed for MVP. Revisit post-MVP. |

The general principle: if an operation is **automatic** (hooks), **embedded in other roles** (quality standards), or **not yet proven necessary** (model orchestration), it does not warrant a dedicated persistent teammate.

---

## 3.9 Ad-Hoc Teammate Flexibility

**Design principle**: PathFlow's predefined teammates are optimized defaults for common workflows. The team lead is never limited to predefined roles and can spawn any teammate type supported by Claude Code's Agent Teams when the situation warrants it.

### What the Lead Can Always Do

| Capability | Example | When |
|-----------|---------|------|
| Spawn **general-purpose** teammates | A teammate with custom instructions for a one-off task | Task does not fit any predefined role |
| Spawn **specialized one-off** teammates | security-auditor, performance-analyst, migration-helper | Domain expertise needed that no predefined role covers |
| Spawn **Explore-type sub-agents** | Quick codebase search, symbol lookup | Lightweight read-only lookups that do not need a full teammate |
| Spawn **custom teammates with custom instructions** | Any combination of skills, constraints, and communication patterns | Novel workflow not anticipated by the predefined roster |
| Spawn **multiple instances** of the same role | developer-frontend, developer-backend (both cf-developer type) | Parallel work on independent modules |

### How Ad-Hoc Teammates Work

Ad-hoc teammates follow the same lifecycle as role teammates:

1. **Spawn**: Lead creates the teammate with a descriptive name and custom spawn prompt
2. **Instructions**: The spawn prompt contains all necessary context (no agent definition file needed, though one can be referenced if available)
3. **Communication**: The teammate communicates using the same SendMessage protocol as predefined teammates
4. **Shutdown**: The lead shuts down the teammate when its task is complete

The key difference from predefined teammates: ad-hoc teammates do not have a pre-built agent definition in `.claude/agents/`. Their instructions come entirely from the spawn prompt. This makes them fast to create but less standardized.

### When to Use Ad-Hoc vs Predefined

```
Task arrives at team lead
         |
         +-- Fits a predefined role?  -------> Use predefined teammate
         |   (development, review, QA,          (has agent def, SOPs, quality
         |    planning, docs, ops, git)          checklist, communication patterns)
         |
         +-- Doesn't fit, but needs            Spawn ad-hoc teammate
         |   a full teammate context?  -------> (custom spawn prompt, lead monitors,
         |   (security audit, perf              standard lifecycle)
         |    analysis, data migration)
         |
         +-- Quick lookup, no coordination     Use Explore sub-agent
             needed?  -----------------------> (ephemeral, no peer comms)
```

### CLAUDE.md Instruction for the Lead

The team lead's instructions (CLAUDE.md) should include guidance like:

> You have a predefined roster of teammates (cf-gitops, cf-knowledge-layer, cf-developer, cf-reviewer, cf-qa, cf-planner, cf-documenter, cf-ops) optimized for standard development workflows. However, you are NOT limited to these roles. When a task requires expertise or context that no predefined role covers, spawn an ad-hoc teammate with appropriate custom instructions. Use Explore-type sub-agents for quick read-only lookups that do not require peer communication.

This ensures the lead knows it has full flexibility while defaulting to the well-tested predefined roles for common workflows.
