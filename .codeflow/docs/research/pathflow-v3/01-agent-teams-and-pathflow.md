# 01: Agent Teams and PathFlow

> Why Claude Code Agent Teams is the catalyst that makes PathFlow possible, and what it enables that the V3 sub-agent model could not.

---

## Table of Contents

- [1.1 The Problem: Why V3 Sub-Agents Were Not Enough](#11-the-problem-why-v3-sub-agents-were-not-enough)
- [1.2 What Agent Teams Provides](#12-what-agent-teams-provides)
- [1.3 The Five Capabilities That Enable PathFlow](#13-the-five-capabilities-that-enable-pathflow)
- [1.4 What Agent Teams Does Not Solve](#14-what-agent-teams-does-not-solve)
- [1.5 V3 Sub-Agents vs Agent Teams: When to Use Which](#15-v3-sub-agents-vs-agent-teams-when-to-use-which)
- [1.6 Validated Platform Behaviors](#16-validated-platform-behaviors)

---

## 1.1 The Problem: Why V3 Sub-Agents Were Not Enough

CodeFlow V3 specifies an agent architecture with 8 sub-agents (cf-planner, cf-developer, cf-reviewer, cf-qa, cf-ops, cf-documenter, cf-support, cf-general-purpose) operating in a strict hub-and-spoke model. The main agent mediates all interactions.

This architecture has fundamental limitations that prevent the kind of session lifecycle management PathFlow requires:

```
V3 Sub-Agent Model (Hub-and-Spoke)
====================================

                    +-----------+
                    | Main Agent|
                    | (mediator)|
                    +-----+-----+
                     /    |    \
                    /     |     \
          +--------+ +--------+ +--------+
          |cf-dev  | |cf-rev  | |cf-qa   |
          |(dies)  | |(dies)  | |(dies)  |
          +--------+ +--------+ +--------+
              ^           ^          ^
              |           |          |
          No peer     No peer    No peer
          comms       comms      comms
```

**Limitation 1: No peer communication.** When cf-developer finishes implementing code, it cannot tell cf-reviewer to start reviewing. The main agent must relay all information, consuming its context window and creating a sequential bottleneck.

**Limitation 2: No persistence.** Sub-agents are ephemeral -- they execute a task, return results, and die. Their context is discarded. If cf-reviewer finds issues and cf-developer needs to rework, a NEW cf-developer sub-agent must be spawned, re-reading the entire codebase from scratch.

**Limitation 3: Context bottleneck on main agent.** Because the main agent mediates everything, its context window accumulates the full conversation history of every sub-agent interaction. For a complex feature involving planning, development, review, and testing, the main agent's context fills rapidly.

**Limitation 4: No shared task visibility.** Sub-agents cannot see what other sub-agents are working on. There is no shared task list. The main agent is the only entity that knows the full state of work.

**Limitation 5: Sequential execution only.** A developer and a reviewer cannot work simultaneously. The main agent must finish one sub-agent interaction before starting another.

These limitations make it impossible to implement PathFlow's core vision: a session that progresses through logical phases with specialized teammates handling different aspects of the work.

---

## 1.2 What Agent Teams Provides

Agent Teams is a Claude Code feature (currently experimental, enabled via `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`) that provides native multi-agent coordination within a single session.

```
Agent Teams Model (Direct Communication)
==========================================

                    +-----------+
                    | Team Lead |
                    |(orchestr.)|
                    +-----+-----+
                     /    |    \
                    /     |     \
          +--------+ +--------+ +--------+
          |cf-dev  | |cf-rev  | |cf-qa   |
          |(alive) | |(alive) | |(alive) |
          +---+----+ +---+----+ +---+----+
              |           |          |
              +-----------+----------+
              Direct peer messaging
              Shared task list
```

The key architectural components:

| Component | Description |
|-----------|-------------|
| **Team Lead** | The main session agent. Creates the team, spawns teammates, orchestrates workflow. |
| **Teammates** | Independent Claude Code instances with their own context windows. Persist until explicitly shut down. |
| **Shared Task List** | File-based task coordination. All teammates can read task status and dependencies. |
| **Mailbox System** | Per-agent message inboxes for direct peer-to-peer communication. |
| **Task Dependencies** | Tasks can block/be-blocked-by other tasks, creating workflow graphs. |

---

## 1.3 The Five Capabilities That Enable PathFlow

### Capability 1: Direct Peer Communication

Teammates can message each other directly using `SendMessage`. This is the foundational capability that makes PathFlow's distributed work model possible.

**What this enables in PathFlow:**
- cf-developer sends commit requests directly to cf-gitops
- cf-developer notifies cf-knowledge-layer when a work stage completes
- cf-reviewer sends feedback directly to cf-developer during rework loops
- cf-qa reports test results to the lead without going through intermediaries

**Without this capability**, every message would route through the team lead, recreating the V3 hub-and-spoke bottleneck with additional overhead.

### Capability 2: Persistent Teammates

Teammates persist across idle periods and retain their full conversation context. They can accept multiple sequential tasks without re-initialization.

**What this enables in PathFlow:**
- cf-gitops loads git-workflow SOPs once and handles all git operations for the entire session
- cf-knowledge-layer loads memory/task/DB SOPs once and manages all WorkGraph interactions
- Function teammates accumulate useful context (what branches exist, what's been committed, current task status)
- Role teammates can be kept alive during rework loops (reviewer requests changes, developer reworks, reviewer re-reviews -- same instances throughout)

**Without this capability**, every work stage transition would require spawning a fresh agent that re-reads all SOPs and re-establishes context.

### Capability 3: Shared Task List with Dependencies

Claude's Task system provides a shared task list that all teammates can see, with dependency ordering between tasks.

**What this enables in PathFlow:**
- Phase markers (PF-1, PF-2, ...) are visible to all teammates as tasks in the shared list
- Work tasks are blocked by the phase markers that precede them
- Dynamic task insertion: new tasks can be wired into the graph mid-flow
- Every teammate can check `TaskList` to see overall session progress
- Checkpoint re-opening: a completed phase marker can be re-opened, re-blocking dependents (enables quality gates)

**Without this capability**, the team lead would need to manually track and communicate session state to every teammate.

### Capability 4: On-Demand Team Scaling

Teammates can be spawned at any time during the team's lifetime. The team lead can start small and scale up as the work progresses.

**What this enables in PathFlow:**
- cf-knowledge-layer spawned at PF-2 (needed for context awareness)
- cf-gitops spawned at PF-3 (needed once development work begins)
- cf-developer spawned at PF-4/WS-DEV (needed for implementation)
- cf-reviewer spawned at PF-4/WS-REV (needed for review)
- cf-qa spawned at PF-4/WS-QA (needed for testing)
- For simple Q&A sessions, no teammates spawned at all

**Without this capability**, the team lead would need to decide the full team composition at session start, wasting resources on teammates that may never be needed.

### Capability 5: Individual Lifecycle Control

Individual teammates can be shut down without affecting the rest of the team. The team lead has full lifecycle control.

**What this enables in PathFlow:**
- Role teammates are shut down after their work stage completes, freeing resources
- Function teammates persist for the full session but can be recycled (shutdown + respawn) if context fills up
- The team scales down as work completes: cf-developer shut down after WS-DEV, cf-reviewer shut down after WS-REV
- If a teammate degrades, it can be replaced without disrupting others

**Without this capability**, the team would be all-or-nothing: full team or no team.

---

## 1.4 What Agent Teams Does Not Solve

Agent Teams provides the intra-session coordination layer. These concerns remain CodeFlow's responsibility:

| Concern | Why Agent Teams Doesn't Address It | CodeFlow Solution |
|---------|-----------------------------------|-------------------|
| Cross-session persistence | Teams exist within a single session | CodeFlow WorkGraph (SurrealDB/JSONL) |
| Batch orchestration | No YAML batch parsing, no Go CLI | Autorun system |
| Git worktree management | No built-in worktree isolation | cf-gitops teammate + worktree scripts |
| Acceptance criteria verification | No automated verification | Stop hooks + PCV |
| Workflow enforcement | Task dependencies are advisory only | Hooks + agent instructions |
| Security enforcement | No built-in access control | PreToolUse hooks, sentinels |
| Three-tier data persistence | No JSONL/SurrealDB/Markdown management | cf-knowledge-layer teammate |

The key insight about **workflow enforcement**: Claude's Task dependency system provides excellent graph structure (define, modify, re-open, delete, multi-predecessor, parallel branches) but zero execution enforcement. A teammate CAN start a blocked task. CodeFlow must add enforcement through hooks and agent instructions. See [Sentinel Model](08-enforcement-model.md) for details.

---

## 1.5 V3 Sub-Agents vs Agent Teams: When to Use Which

Agent Teams does not replace sub-agents. Both serve different purposes:

```
Task arrives at team lead
         |
         +-- Single skill operation?  -------> Sub-Agent (Explore type)
         |   ("find all auth files")            Quick, isolated, disposable
         |
         +-- Single-file quick fix?  --------> Direct work (lead handles it)
         |   ("fix typo in README")
         |
         +-- Multi-step, needs SOP?  --------> Function Teammate (persistent)
         |   ("commit these changes")           cf-gitops, cf-knowledge-layer
         |
         +-- Multi-step, variable work? -----> Role Teammate (on-demand)
         |   ("implement this feature")         cf-developer, cf-reviewer, cf-qa
         |
         +-- Batch of independent tasks? ----> Autorun (+ Agent Teams within)
             ("implement these 5 features")
```

**Sub-agents remain valuable for:**
- Quick read-only codebase exploration (Explore type)
- Lightweight skill operations that don't need peer communication
- Operations where context isolation is the goal, not an obstacle

**Agent Teams teammates are used for:**
- Work that requires communication with other agents
- Persistent operations repeated throughout the session (function teammates)
- Multi-step tasks that benefit from retained context (role teammates)
- Any work that is part of the PathFlow stage progression

---

## 1.6 Validated Platform Behaviors

The following Agent Teams behaviors have been validated through direct testing (see `.codeflow/docs/research/agent-teams-test-findings.md`):

| Behavior | Status | Notes |
|----------|--------|-------|
| Agent definition loading via spawn prompt | Validated | Include "read your agent def at .claude/agents/{type}.md" in spawn prompt |
| Teammate persistence across idle cycles | Validated | Full context retained, multiple sequential tasks work |
| On-demand teammate spawning | Validated | New teammates can join an active team at any time |
| Multiple instances of same agent type | Validated | Differentiated by name parameter, not agent type |
| Individual shutdown without team disruption | Validated | Remaining teammates continue normally |
| Task dependency graph structure | Validated | Define, modify, re-open, delete all work |
| Checkpoint re-opening re-blocks dependents | Validated | completed -> pending re-blocks downstream tasks |
| Dynamic mid-flow task insertion | Validated | New tasks can be wired into existing graph |

| Behavior | Status | Notes |
|----------|--------|-------|
| `subagent_type` auto-injects agent definition | NOT supported | Must use explicit instruction in spawn prompt |
| Agent env vars (AGENT_NAME, AGENT_TYPE, TEAM_NAME) | NOT available | Not set despite documentation |
| Self-service context clearing | NOT possible | Only auto-compaction at ~95% capacity |
| Task dependency enforcement (prevent out-of-order) | NOT enforced | Dependencies are advisory only |
| Message delivery to shut-down teammates | Silently lost | System accepts but never delivers |

These findings directly informed PathFlow's design decisions, particularly around spawn-time instruction loading (recommended over post-spawn messages) and the need for hook-based enforcement alongside advisory task dependencies.
