# Claude Agent Teams Integration into CodeFlow

## Proposal: Adapting CodeFlow Specification V3 for Native Agent Teams Support

**Date**: 2026-02-05
**Status**: Draft Proposal
**Scope**: CodeFlow Specification V3 + Implementation
**Author**: Research Team (team-lead, web-researcher, spec-reviewer, workflow-reviewer, impl-reviewer)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Current State Analysis](#2-current-state-analysis)
3. [Claude Agent Teams Capabilities](#3-claude-agent-teams-capabilities)
4. [Gap Analysis: Current CodeFlow vs Agent Teams](#4-gap-analysis-current-codeflow-vs-agent-teams)
5. [Integration Architecture](#5-integration-architecture)
6. [Scenario Analysis](#6-scenario-analysis)
7. [Autorun + Agent Teams Synergy](#7-autorun--agent-teams-synergy)
8. [Knowledge Layer Integration](#8-knowledge-layer-integration)
9. [Migration Path from Current Architecture](#9-migration-path-from-current-architecture)
10. [Specification Changes Required](#10-specification-changes-required)
11. [Risks and Mitigations](#11-risks-and-mitigations)
12. [Recommendations](#12-recommendations)
13. [Appendix: Research Sources](#13-appendix-research-sources)

---

## 1. Executive Summary

Claude Code's Agent Teams feature provides **native multi-agent coordination** capabilities that directly address several architectural limitations in CodeFlow V3's current design. This proposal recommends integrating Agent Teams as a **first-class orchestration layer** within CodeFlow, replacing the hub-and-spoke sub-agent model with a team-based approach for complex work, while preserving sub-agents for lightweight forked-context operations.

### Key Findings

1. **Agent Teams solve CodeFlow's biggest limitation**: The current hub-and-spoke architecture where sub-agents cannot communicate peer-to-peer is directly addressed by Agent Teams' built-in messaging system.

2. **The main-agent-as-orchestrator goal aligns perfectly**: Agent Teams' team-lead role with delegate mode mirrors CodeFlow's design intent of constraining the main agent to coordination.

3. **Autorun and Agent Teams are complementary, not competing**: Autorun handles cross-session batch orchestration (Go CLI + tmux sessions); Agent Teams handles intra-session multi-agent coordination. The most powerful pattern is **Autorun spawning sessions that internally use Agent Teams**.

4. **The Knowledge Layer needs adaptation, not replacement**: The work graph, CRDT claims, and worktree management remain valuable infrastructure that Agent Teams can leverage for persistent coordination beyond single sessions.

5. **Sub-agents still have a role**: Lightweight, forked-context skill execution (the `cf-general-purpose` pattern) remains more efficient than spawning full team members for simple operations.

---

## 2. Current State Analysis

### 2.1 CodeFlow V3 Architecture (Specification)

CodeFlow V3 is a four-layer architecture:

| Layer | Components | Role |
|-------|-----------|------|
| L4: User Interface | 14 slash commands + Go CLI (`codeflow`) | User interaction |
| L3: Claude Layer | Main agent + 8 sub-agents + 11 skills + 26 hooks | Intelligence + orchestration |
| L2: State Layer | SQLite + JSONL + CRDT/Loro | Persistence + coordination |
| L1: Git Layer | Git hooks + rendered markdown | Version control |

**Orchestration Model**: Strict hub-and-spoke. The main agent mediates all sub-agent interactions. Sub-agents cannot:
- Spawn other sub-agents
- Communicate with peer sub-agents
- Persist state beyond their execution
- Access the main agent's conversation history

**Agent Roles** (specified but not yet implemented):

| Agent | Purpose | Branch Access |
|-------|---------|---------------|
| cf-planner | Epic/task creation | plan/* |
| cf-developer | Implementation | feat/*, fix/*, refactor/* |
| cf-reviewer | Code review | read-only |
| cf-qa | Testing | test/*, feat/* (tests only) |
| cf-ops | CI/CD, deployment | ops/*, deploy/* |
| cf-documenter | Documentation | docs/* |
| cf-support | Help and status | read-only |
| cf-general-purpose | Forked skill execution | delegated |

**Skills System**: 11 skills with fork-first context strategy. Only `cf-working-protocol` runs inline; all others fork to `cf-general-purpose` sub-agent. This protects main agent context but creates communication overhead.

**Autorun System**: Go CLI orchestrator that:
- Parses YAML batch files defining tasks with acceptance criteria
- Spawns isolated tmux sessions with git worktrees
- Uses Haiku LLM as stop-hook verifier
- Manages concurrency limits (default: 3 workers/session, 2 sessions, 6 total)
- Workers are fully isolated with NO inter-worker communication

### 2.2 Workflow Repo (Predecessor)

The Workflow repo pioneered many CodeFlow patterns but revealed key limitations:
- **Document bloat**: Agent specs (20-30KB each), skills (45-86KB), CLAUDE.md (28KB) consumed excessive context
- **Memory accumulation**: 30+ topic folders with files up to 230KB
- **Read delegation friction**: 50-line threshold for delegating reads was too aggressive
- **Sub-agent isolation**: One-way context bundles with no peer communication
- **Over-prescription**: Mandatory emoji signals, PAC-5 checkpoints, and TIER indicators in every response created compliance overhead

### 2.3 Current CodeFlow Implementation

Implementation is at Phase 3 (Hooks) of 9 planned phases:

| Component | Status |
|-----------|--------|
| Directory structure | Complete |
| Skills (10 defined) | Complete |
| Hooks (6 lifecycle points) | In progress |
| Security enforcement | In progress |
| Settings templates (4 profiles) | Complete |
| Shell library | Complete |
| Testing framework | Complete |
| Agent definitions | **Not started** (empty `.claude/agents/`) |
| Slash commands | **Not started** (empty `.claude/commands/`) |
| Database/state layer | **Not started** |
| Go CLI / Autorun | **Not started** |

The fact that agent definitions are not yet implemented presents an **ideal opportunity** to design them with Agent Teams in mind from the start.

---

## 3. Claude Agent Teams Capabilities

### 3.1 Architecture Overview

Agent Teams is an experimental feature (enabled via `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`) that provides:

| Component | Purpose |
|-----------|---------|
| **Team Lead** | Main session; creates team, spawns teammates, coordinates |
| **Teammates** | Independent Claude Code instances working on assigned tasks |
| **Shared Task List** | File-based task coordination at `~/.claude/tasks/{team-name}/` |
| **Mailbox System** | Per-agent message inboxes at `~/.claude/teams/{team-name}/inboxes/` |

### 3.2 Key Capabilities

| Capability | Details |
|------------|---------|
| **Peer-to-peer messaging** | Direct messages between any teammates (not just lead) |
| **Broadcast messaging** | Send to all teammates simultaneously |
| **Task dependencies** | Tasks can block/be blocked by other tasks |
| **Delegate mode** | Locks lead to coordination-only tools (Shift+Tab) |
| **Plan approval workflow** | Teammates submit plans for lead approval before implementing |
| **Idle state management** | Automatic idle notifications; messages wake idle teammates |
| **Custom agent types** | `.claude/agents/` definitions usable as `subagent_type` |
| **Display modes** | In-process (any terminal) or split-pane (tmux/iTerm2) |
| **Self-organizing swarm** | Teammates can poll TaskList and self-claim available work |

### 3.3 Limitations

| Limitation | Impact on CodeFlow |
|------------|-------------------|
| No nested teams | A teammate cannot spawn its own team |
| No session resumption | In-process teammates lost on `/resume` |
| One team per session | Must cleanup before starting new team |
| No inter-team communication | Teams are isolated silos |
| Lead is fixed | Cannot promote teammate to lead |
| Higher token cost | Each teammate has separate context window |
| Experimental status | API may change |

---

## 4. Gap Analysis: Current CodeFlow vs Agent Teams

### 4.1 Problems Agent Teams Solve

| CodeFlow Limitation | Agent Teams Solution |
|--------------------|---------------------|
| Sub-agents can't communicate peer-to-peer | Native peer messaging via SendMessage |
| Main agent mediates ALL transitions | Teammates self-coordinate via shared task list |
| No real-time Autorun worker coordination | In-session teammates can share findings live |
| Hub-and-spoke creates sequential bottleneck | Parallel independent work with async messaging |
| Sub-agent context discarded after return | Teammates persist until explicit shutdown |
| Main agent context window is bottleneck | Work distributed across multiple context windows |
| No review-during-development | Reviewer teammate can provide continuous feedback |
| cf-model-orchestrator is speculative | Agent Teams provides proven multi-agent coordination |

### 4.2 What Agent Teams Do NOT Solve

| Need | Why Agent Teams Doesn't Address It |
|------|-----------------------------------|
| Cross-session persistence | Teams exist within a single session; no resumption |
| Batch task orchestration | No YAML batch file parsing; no Go CLI management |
| Git worktree management | No built-in worktree creation/isolation |
| Acceptance criteria verification | No Haiku-based stop-hook verifier |
| Multi-machine coordination | Teams are local to one machine |
| CRDT-based claims | Teams use file-based task locking, not CRDT |
| Three-tier data persistence | No JSONL/SQLite/Markdown data management |

### 4.3 Overlap Analysis

```
                    ┌─────────────────────────────────┐
                    │         AGENT TEAMS              │
                    │  - Peer messaging                │
                    │  - Shared task list               │
                    │  - Team lead delegation           │
                    │  - Plan approval workflow         │
    ┌───────────────┼──────────────────┐               │
    │   CODEFLOW    │    OVERLAP       │               │
    │               │  - Task tracking │               │
    │  - Batch      │  - Agent roles   │               │
    │    orchestr.  │  - Orchestration │               │
    │  - Git        │  - Work coord.   │               │
    │    worktrees  │                  │               │
    │  - CRDT       │                  │               │
    │  - JSONL/SQL  │                  │               │
    │  - Go CLI     │                  │               │
    │  - Stop hook  │                  │               │
    │  - Security   │                  │               │
    └───────────────┼──────────────────┘               │
                    └─────────────────────────────────┘
```

**Overlap areas** (task tracking, agent roles, orchestration, work coordination) are where integration design must carefully delineate responsibilities.

---

## 5. Integration Architecture

### 5.1 Proposed Layered Model

```
┌──────────────────────────────────────────────────────────────┐
│                    L5: AUTORUN (Cross-Session)                │
│  Go CLI -> Spawns tmux sessions -> Each session = L4 team    │
│  Batch YAML -> Task assignment -> Acceptance verification     │
└──────────────────────────┬───────────────────────────────────┘
                           │ spawns
┌──────────────────────────▼───────────────────────────────────┐
│               L4: AGENT TEAM (Intra-Session)                  │
│  Team Lead (main agent) -> Spawns teammates via Task tool     │
│  SendMessage for coordination -> Shared task list              │
│  Delegate mode -> Team lead stays orchestration-only           │
└──────────────────────────┬───────────────────────────────────┘
                           │ uses
┌──────────────────────────▼───────────────────────────────────┐
│              L3: CLAUDE LAYER (Skills + Hooks)                │
│  Skills loaded per-teammate -> Hooks enforce per-teammate     │
│  Sub-agents for lightweight forked skill execution             │
│  Security enforcement via PreToolUse hooks                     │
└──────────────────────────┬───────────────────────────────────┘
                           │ persists to
┌──────────────────────────▼───────────────────────────────────┐
│              L2: STATE LAYER (Knowledge Layer)                │
│  SQLite + JSONL + CRDT -> Shared across teammates via git     │
│  Work graph -> Task/epic management                            │
│  Active work claims -> CRDT-based resource locking             │
└──────────────────────────┬───────────────────────────────────┘
                           │ version-controlled by
┌──────────────────────────▼───────────────────────────────────┐
│              L1: GIT LAYER                                    │
│  Git hooks -> Branch protection -> Worktrees                   │
│  Each teammate operates on shared repo or dedicated worktree   │
└──────────────────────────────────────────────────────────────┘
```

### 5.2 Two-Tier Agent Model

Replace the current flat sub-agent model with a two-tier approach:

#### Tier 1: Team Members (Agent Teams)
For substantial, persistent work requiring coordination:

| Role | Maps From | When to Use |
|------|-----------|-------------|
| `cf-planner` | Current spec agent | Planning epics, tasks, requirements analysis |
| `cf-developer` | Current spec agent | Implementation work (feature, fix, refactor) |
| `cf-reviewer` | Current spec agent | Code review, PR review |
| `cf-qa` | Current spec agent | Test creation, test execution, quality gates |
| `cf-ops` | Current spec agent | CI/CD, deployment, release management |
| `cf-documenter` | Current spec agent | Documentation, ADRs, guides |
| `cf-researcher` | NEW | Web research, competitive analysis, spike investigations |

These are defined as `.claude/agents/*.md` files and used as `subagent_type` when spawning teammates.

#### Tier 2: Sub-Agents (Traditional)
For lightweight, ephemeral operations:

| Role | Purpose | When to Use |
|------|---------|-------------|
| `cf-general-purpose` | Forked skill execution | Quick skill operations needing isolated context |
| `Explore` (built-in) | Fast codebase search | File discovery, symbol lookup |
| `Plan` (built-in) | Architecture design | Quick design sketches |

**Decision rule**: If the task requires communication with other agents, persistence across multiple steps, or coordination with shared state -> use Team Member. If it's a quick, isolated operation -> use Sub-Agent.

### 5.3 Main Agent as Team Lead

The main agent should adopt the team lead role with these behaviors:

1. **Delegate mode by default**: When a team is active, the main agent should operate in coordination-only mode (equivalent to pressing Shift+Tab)
2. **Skill routing through teammates**: Instead of forking skills to `cf-general-purpose`, route skill-heavy work to the appropriate team member
3. **Plan approval gating**: Team members with `plan_mode_required` must get lead approval before implementation, enabling review checkpoints
4. **Context preservation**: The lead maintains high-level state while teammates handle detailed work, preventing context window exhaustion

### 5.4 Team Lifecycle in CodeFlow

```
Session Start
    │
    ├── Simple task? ──────────────> Direct work (no team needed)
    │
    ├── Complex task? ─────────────> Spawn team
    │   │
    │   ├── Assess work scope
    │   ├── Create CodeFlow tasks (work graph)
    │   ├── Map tasks to agent roles
    │   ├── spawnTeam("cf-{epic-id}")
    │   ├── Spawn teammates with role-specific agents
    │   ├── Assign tasks via TaskUpdate
    │   ├── Enter delegate mode
    │   │
    │   ├── COORDINATION LOOP:
    │   │   ├── Receive teammate messages
    │   │   ├── Approve/reject plans
    │   │   ├── Reassign blocked tasks
    │   │   ├── Handle escalations
    │   │   └── Monitor progress
    │   │
    │   ├── COMPLETION:
    │   │   ├── All tasks completed
    │   │   ├── Send shutdown_request to all teammates
    │   │   ├── Await confirmations
    │   │   ├── Cleanup team
    │   │   └── Update work graph status
    │   │
    │   └── Post-team: verify-work, commit, PR
    │
    └── Autorun context? ─────────> Team within Autorun worker
        (see Section 7)
```

---

## 6. Scenario Analysis

### 6.1 Scenario: Feature Implementation (Plan + Develop + Review + Test)

**Current approach (hub-and-spoke)**:
1. User invokes `/cf-plan` -> Main agent spawns cf-planner sub-agent
2. Planner returns plan -> Main agent receives, stores in memory
3. User invokes `/cf-develop` -> Main agent spawns cf-developer with plan context
4. Developer returns code -> Main agent receives
5. Main agent spawns cf-reviewer for review
6. Reviewer returns feedback -> Main agent relays to user
7. User decides on fixes -> Main agent re-spawns cf-developer
8. Repeat review cycle
9. Main agent spawns cf-qa for testing
10. Sequential, single-threaded, context-heavy on main agent

**Proposed approach (Agent Teams)**:
1. User describes feature -> Lead creates work graph tasks
2. Lead spawns team: cf-planner, cf-developer, cf-reviewer, cf-qa
3. cf-planner gets planning task, submits plan for lead approval
4. Lead approves -> cf-developer gets implementation task (depends on planning)
5. cf-reviewer monitors developer progress via peer messaging, provides early feedback
6. cf-developer completes -> cf-qa auto-unblocks, begins testing
7. cf-reviewer reviews final code, sends feedback directly to cf-developer
8. Parallel, multi-threaded, context distributed across teammates

**Advantages**: 3-4x faster for complex features; reviewer can catch issues early; QA starts immediately after development; main agent context stays clean.

### 6.2 Scenario: Bug Investigation and Fix

**Current approach**:
1. Main agent investigates (consuming context)
2. Spawns cf-developer for fix
3. Developer has no investigation context (must re-investigate)
4. Spawns cf-qa for testing
5. QA has no fix context

**Proposed approach (Agent Teams)**:
1. Lead spawns cf-researcher to investigate
2. cf-researcher finds root cause, messages cf-developer directly with findings
3. cf-developer implements fix, messages cf-qa with test suggestions
4. cf-qa writes and runs tests, sends results to cf-reviewer
5. cf-reviewer approves or sends feedback to cf-developer

**Advantage**: Context flows between teammates without going through the lead. Investigation findings go directly to the developer who needs them.

### 6.3 Scenario: Documentation Update

**Current approach**: Main agent spawns cf-documenter, who cannot access developer's recent changes without going through main agent.

**Proposed approach**: cf-documenter is a persistent team member who receives direct messages from cf-developer about API changes, automatically updating docs as features are built.

### 6.4 Scenario: Code Review (External PR)

**Current approach**: Main agent spawns cf-reviewer as sub-agent. Reviewer has limited context, returns a single review. Follow-up requires re-spawning.

**Proposed approach**: cf-reviewer is a persistent team member. Can have back-and-forth discussion with cf-developer about specific findings. Can request cf-qa to write tests for edge cases found during review. Multi-round review without lead mediation.

### 6.5 Scenario: Large-Scale Refactoring

**Current approach**: Not well-supported. Main agent would need to coordinate multiple sequential sub-agent invocations, likely losing context.

**Proposed approach**:
1. Lead creates tasks for each refactoring component
2. Spawns multiple cf-developer teammates (cf-dev-api, cf-dev-ui, cf-dev-db)
3. Each developer works on their area independently
4. Developers message each other about interface changes
5. cf-reviewer provides ongoing review
6. cf-qa runs integration tests as components complete

**Key consideration**: Each developer should work on **separate files/modules** to avoid merge conflicts. Use CRDT claims to enforce this.

### 6.6 Scenario: Research and Spike Investigation

**Current approach**: Main agent spawns general-purpose sub-agent for web research, consuming multiple forked contexts.

**Proposed approach**: cf-researcher team member conducts thorough research with persistent context. Can spawn Explore sub-agents for codebase search. Sends structured findings to cf-planner for incorporation into plans.

---

## 7. Autorun + Agent Teams Synergy

### 7.1 Relationship: Complementary, Not Competing

```
┌─────────────────────────────────────────────────────────┐
│                     AUTORUN                              │
│  Scope: Cross-session batch orchestration                │
│  Tool: Go CLI (codeflow autorun start batch.yaml)        │
│  Isolation: Separate tmux sessions + git worktrees       │
│  Communication: NONE between workers                     │
│  Verification: Haiku stop-hook per worker                │
│  Best for: Independent, well-defined tasks               │
└─────────────────────────┬───────────────────────────────┘
                          │
                    SPAWNS SESSIONS
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                    AGENT TEAMS                           │
│  Scope: Intra-session multi-agent coordination           │
│  Tool: Native Claude Code Team/Task/Message tools        │
│  Isolation: Shared repo, separate context windows        │
│  Communication: Peer-to-peer messaging                   │
│  Verification: Lead approval + plan mode                 │
│  Best for: Complex tasks needing coordination            │
└─────────────────────────────────────────────────────────┘
```

**Key insight**: Autorun is the **outer orchestrator** (batch → sessions); Agent Teams is the **inner orchestrator** (session → teammates). They operate at different scopes and complement each other.

### 7.2 Autorun Workers Using Agent Teams Internally

When Autorun spawns a Claude session for a complex task, that session's prompt should instruct Claude to act as a team lead and internally use Agent Teams:

```yaml
# batch.yaml (enhanced)
tasks:
  - id: T-FEAT-001
    title: "Implement user authentication"
    acceptance_criteria:
      - JWT-based auth with refresh tokens
      - Login/logout endpoints
      - Middleware for protected routes
      - Unit tests with >80% coverage
      - API documentation updated
    team_strategy: "full"    # NEW FIELD
    team_roles:              # NEW FIELD
      - cf-developer
      - cf-reviewer
      - cf-qa
```

**Worker session prompt** (generated by Go CLI):

```
You are working on task T-FEAT-001: "Implement user authentication".

Acceptance criteria:
- JWT-based auth with refresh tokens
- Login/logout endpoints
- Middleware for protected routes
- Unit tests with >80% coverage
- API documentation updated

INSTRUCTIONS:
1. You are the team lead. Spawn an Agent Team for this task.
2. Create team members: cf-developer, cf-reviewer, cf-qa
3. Break the acceptance criteria into team tasks
4. Assign tasks to appropriate team members
5. Coordinate development, review, and testing
6. Ensure all acceptance criteria are met before creating PR
7. Enter delegate mode to focus on orchestration
```

### 7.3 Team Strategy Options

New batch file field `team_strategy` to control how each Autorun worker uses Agent Teams:

| Strategy | Description | When to Use |
|----------|-------------|-------------|
| `solo` | No team; worker operates alone (current behavior) | Simple, well-defined tasks |
| `pair` | Worker + one reviewer teammate | Medium complexity with quality gate |
| `full` | Worker as lead + role-specific teammates | Complex features needing coordination |
| `swarm` | Worker as lead + N general-purpose teammates | Large tasks with many independent subtasks |

### 7.4 Cross-Worker Coordination via Knowledge Layer

While Autorun workers can't communicate directly (different sessions), Agent Teams within each worker CAN use the shared Knowledge Layer:

```
Autorun Worker A (Session 1)          Autorun Worker B (Session 2)
├── Team Lead                          ├── Team Lead
├── cf-developer                       ├── cf-developer
├── cf-reviewer                        ├── cf-qa
│                                      │
└── Writes to Knowledge Layer ────────>└── Reads from Knowledge Layer
    (CRDT claims, JSONL events)            (discovers A's API contracts)
```

This requires:
1. Workers share the same `.state/` directory (symlinked from worktrees)
2. CRDT claims prevent conflicting file edits across workers
3. JSONL event log provides an append-only communication channel
4. Workers poll for relevant events from other workers

### 7.5 Autorun Stop Hook Enhancement

The existing Haiku-based stop hook should be enhanced to verify team coordination:

```
Enhanced verification checklist:
- [ ] All acceptance criteria met
- [ ] All team members completed their tasks
- [ ] Code review completed (if team_strategy != solo)
- [ ] Tests pass (if cf-qa was on the team)
- [ ] PR created with proper description
- [ ] No unresolved team member messages
```

---

## 8. Knowledge Layer Integration

### 8.1 Work Graph + Agent Teams Task List

Two task systems must coexist:

| System | Scope | Persistence | Use |
|--------|-------|-------------|-----|
| **CodeFlow Work Graph** | Cross-session, cross-team | SQLite + JSONL + Markdown | Epic/task lifecycle management |
| **Agent Teams Task List** | Single session, single team | File-based (~/.claude/tasks/) | Intra-team coordination |

**Integration approach**: The CodeFlow work graph is the **source of truth** for task definitions. Agent Teams task list is a **runtime projection** of work graph tasks assigned to the current team.

```
CodeFlow Work Graph (persistent)
    │
    ├── Epic: E-AUTH-001 "User Authentication"
    │   ├── Task: T-AUTH-001 "Design JWT scheme" (cf-planner)
    │   ├── Task: T-AUTH-002 "Implement endpoints" (cf-developer)
    │   ├── Task: T-AUTH-003 "Write tests" (cf-qa)
    │   └── Task: T-AUTH-004 "Review implementation" (cf-reviewer)
    │
    └── Session starts, team spawned
        │
        ▼
Agent Teams Task List (ephemeral)
    ├── Task #1: "Design JWT scheme" -> assigned to cf-planner
    ├── Task #2: "Implement endpoints" -> blocked by #1, assigned to cf-developer
    ├── Task #3: "Write tests" -> blocked by #2, assigned to cf-qa
    └── Task #4: "Review implementation" -> blocked by #2, assigned to cf-reviewer
```

**Sync protocol**:
1. Team lead reads CodeFlow work graph for current epic/tasks
2. Team lead creates Agent Teams tasks mirroring the work graph
3. As teammates complete Agent Teams tasks, lead updates CodeFlow work graph
4. On team cleanup, final work graph state is committed

### 8.2 CRDT Claims + Team Member File Ownership

CRDT claims should integrate with team member assignments:

```python
# When spawning a team member, auto-create claims:
claim = {
    "agent": "cf-developer",
    "pattern": "src/auth/**",      # Files this teammate owns
    "mode": "exclusive",
    "team": "cf-E-AUTH-001",
    "fencing_token": next_token()
}
```

**Benefits**:
- Prevents two teammates from editing the same files
- Integrates with PreToolUse hooks for enforcement
- Persists beyond team lifetime via CRDT (for cross-session coordination)
- Pattern-based claims allow flexible file ownership

### 8.3 Memory Events from Teammates

Each teammate should write memory events to their role's domain:

| Teammate | Memory Domain | Event Types |
|----------|--------------|-------------|
| cf-planner | planning/ | decisions, requirements, trade-offs |
| cf-developer | development/ | implementation notes, API contracts |
| cf-reviewer | review/ | findings, suggestions, approvals |
| cf-qa | qa/ | test results, coverage reports |
| cf-documenter | documentation/ | doc updates, API changes |

**Integration**: Teammates write to the shared `.state/memory/` JSONL via the existing `cf-memory-management` skill. The lead's post-team verification includes checking that all memory events are properly persisted.

### 8.4 Worktrees + Agent Teams

Two integration patterns:

#### Pattern A: Shared Worktree (Default)
All teammates work in the same worktree/branch. Use CRDT claims to prevent file conflicts.
- **Pros**: Simple, teammates can see each other's changes immediately
- **Cons**: Risk of conflicts, requires careful claim management

#### Pattern B: Per-Teammate Worktrees (Complex Features)
Each teammate gets their own worktree/branch, merging back to the feature branch.
- **Pros**: Complete isolation, no conflict risk
- **Cons**: Complex merge management, teammates can't see each other's changes
- **Best for**: Large refactoring where components are truly independent

**Recommendation**: Use Pattern A by default with CRDT claims. Use Pattern B only for large-scale parallel refactoring where file ownership is clearly separable.

---

## 9. Migration Path from Current Architecture

### 9.1 Phase-Aligned Integration

Since CodeFlow implementation is at Phase 3 (Hooks) with agent definitions not yet started, integration should align with the existing phase plan:

| Phase | Current Plan | Agent Teams Integration |
|-------|-------------|------------------------|
| 3 (Hooks) | Hook system | Add team-aware hooks (see 9.2) |
| 4 (Agents) | Define 8 agents | Design agents as BOTH sub-agents AND team members (see 9.3) |
| 5 (Commands) | 14 slash commands | Add `/cf-team` command family (see 9.4) |
| 6 (Integration) | End-to-end flows | Agent Teams as primary orchestration for complex work |
| 7 (CLI) | Go CLI binary | Autorun team_strategy support |
| 8 (Monorepo) | Template extraction | Agent Teams config as template component |

### 9.2 Hook Enhancements for Agent Teams

New hooks needed:

| Hook Point | Script | Purpose |
|------------|--------|---------|
| SessionStart | `cf-session-start-team-detect.sh` | Detect if session is an Autorun worker that should spawn a team |
| PreToolUse (Task) | `cf-pre-tool-use-team-task.sh` | Validate team task operations against work graph |
| PostToolUse (SendMessage) | `cf-post-tool-use-team-msg.sh` | Log team messages to audit trail |
| Stop | `cf-stop-team-verify.sh` | Verify team shutdown before session end |

### 9.3 Dual-Purpose Agent Definitions

Agent `.md` files should be designed to work in both contexts:

```markdown
---
name: cf-developer
description: Implementation agent for CodeFlow
# Sub-agent mode (forked context)
subagent_context: fork
subagent_agent: cf-general-purpose
# Team member mode
team_role: developer
team_file_patterns:
  - "src/**"
  - "lib/**"
team_branch_access:
  - "feat/*"
  - "fix/*"
  - "refactor/*"
team_plan_mode_required: true
---

# cf-developer

## Role
...
```

When used as a sub-agent (lightweight skill execution), the agent runs in forked context as before. When used as a team member, it gets the full agent definition with team-specific configuration.

### 9.4 New Commands

| Command | Purpose |
|---------|---------|
| `/cf-team-start` | Spawn a team for current work |
| `/cf-team-status` | View team status, task progress |
| `/cf-team-stop` | Gracefully shut down team |
| `/cf-team-msg` | Send message to specific teammate |

---

## 10. Specification Changes Required

### 10.1 New Specification Documents

| Document | Location | Content |
|----------|----------|---------|
| `05-claude-components/teams/README.md` | New section | Agent Teams architecture overview |
| `05-claude-components/teams/team-lifecycle.md` | New | Team creation, coordination, shutdown |
| `05-claude-components/teams/team-communication.md` | New | Messaging patterns and protocols |
| `05-claude-components/teams/team-task-sync.md` | New | Work graph ↔ team task list sync |
| `09-autorun/team-strategy.md` | New | Autorun worker team strategies |
| `06-flows/team-orchestration-flow.md` | New | End-to-end team workflow |

### 10.2 Modified Specification Documents

| Document | Changes |
|----------|---------|
| `03-architecture/system-overview.md` | Add L5 (Autorun) layer; update L3 for teams |
| `05-claude-components/agents/README.md` | Add dual-mode agent definitions |
| `05-claude-components/main-agent/01-claude-md-spec.md` | Add team lead behavior, delegate mode |
| `05-claude-components/skills/README.md` | Clarify sub-agent vs team member skill loading |
| `04-knowledge-layer/03-active-work.md` | Add team-aware claims |
| `06-flows/command-routing.md` | Add team context routing |
| `09-autorun/architecture.md` | Add team_strategy option |
| `09-autorun/batch-files.md` | Add team_strategy and team_roles fields |
| `09-autorun/worker-lifecycle.md` | Add team lifecycle within worker |

### 10.3 Settings Changes

```json
// settings.json additions
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  },
  "teammateMode": "in-process",

  // CodeFlow-specific team config
  "_team_config": {
    "default_strategy": "solo",
    "auto_delegate_mode": true,
    "max_teammates": 4,
    "plan_approval_required": ["cf-developer", "cf-ops"],
    "auto_claim_enabled": true
  }
}
```

---

## 11. Risks and Mitigations

### 11.1 Technical Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent Teams is experimental; API may change | HIGH | Abstract team operations behind CodeFlow skill layer; isolate direct API usage |
| Token cost scales linearly with teammates | MEDIUM | Default to `solo` strategy; only spawn teams for complex work; use budget limits |
| Teammates may not follow CodeFlow protocols | MEDIUM | Skills and hooks loaded per-teammate; security enforcement applies to all |
| File conflicts between teammates | MEDIUM | CRDT claims enforce exclusive file ownership; PreToolUse hooks validate |
| Team state lost on session crash | MEDIUM | Knowledge Layer persists work graph state independent of team lifetime |
| Orphaned tmux sessions | LOW | Autorun cleanup handles; session-end hook checks for orphans |

### 11.2 Architectural Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Over-engineering: teams for simple tasks | HIGH | Decision rule: only spawn teams for multi-file, multi-role work |
| Two task systems (work graph + team tasks) create confusion | MEDIUM | Clear ownership: work graph = persistent; team tasks = ephemeral projection |
| Delegate mode prevents lead from quick fixes | LOW | Lead can exit delegate mode temporarily for minor interventions |
| Team communication generates noise | MEDIUM | Structured message templates; broadcast restrictions |

### 11.3 Process Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Specification scope creep from team integration | HIGH | Phase the integration; don't redesign everything at once |
| Agent definitions become overly complex for dual mode | MEDIUM | Keep team-specific config in frontmatter; share core behavior |
| Testing complexity increases significantly | MEDIUM | Test sub-agent mode and team mode independently |

---

## 12. Recommendations

### 12.1 Immediate Actions (Phase 3-4)

1. **Enable Agent Teams in settings**: Add `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` to all settings templates now, even before full integration.

2. **Design agent definitions for dual mode**: When building Phase 4 (Agents), design `.claude/agents/*.md` files that work as both sub-agents and team members from the start.

3. **Add team-aware hooks**: In the current Phase 3 hooks work, add team detection and team message logging hooks.

4. **Create `cf-team-management` skill**: A new skill that encapsulates team lifecycle operations, providing a CodeFlow abstraction over the raw Agent Teams API.

### 12.2 Medium-Term Actions (Phase 5-7)

5. **Implement work graph ↔ team task sync**: Build the synchronization protocol between CodeFlow's persistent work graph and Agent Teams' ephemeral task list.

6. **Add team_strategy to Autorun**: Enhance the Go CLI and batch file format to support team strategies within Autorun workers.

7. **Build `/cf-team-*` commands**: User-facing commands for team management.

8. **Implement CRDT claim integration**: Auto-create CRDT claims when team members are assigned file patterns.

### 12.3 Long-Term Actions (Phase 8-9)

9. **Cross-worker event bus**: Use JSONL append-only logs as a lightweight event bus for Autorun workers to discover each other's progress.

10. **Team templates**: Pre-defined team configurations for common workflows (feature development, bug fix, documentation sprint, release management).

11. **Metrics and observability**: Track team performance, token usage, task completion rates, communication patterns.

### 12.4 What to Keep vs. Replace vs. Enhance

| Component | Action | Rationale |
|-----------|--------|-----------|
| Hub-and-spoke sub-agent orchestration | **Enhance** | Keep for lightweight ops; add team-based for complex work |
| cf-general-purpose forked execution | **Keep** | Still best for quick, isolated skill operations |
| Autorun batch orchestration | **Enhance** | Add team_strategy for intra-worker coordination |
| Autorun worker isolation | **Keep** | Cross-session isolation remains valuable for safety |
| CRDT claims | **Enhance** | Integrate with team member file ownership |
| Work graph (epics/tasks) | **Enhance** | Add sync protocol with Agent Teams task list |
| Memory domain isolation | **Keep** | Teammates write to their role's domain |
| Security enforcement | **Keep** | Hooks apply to all teammates equally |
| cf-model-orchestrator | **Reconsider** | Agent Teams may subsume some of its functionality; evaluate overlap |
| Read delegation (aggressive) | **Replace** | Team members handle their own file reading; delegate mode on lead is sufficient |
| Sentinel system | **Keep** | Skill-first enforcement applies to teammates too |

### 12.5 Decision: Sub-Agents vs Agent Teams

```
Task arrives
    │
    ├── Single skill operation? ──────> Sub-Agent (forked context)
    │   (e.g., "validate this JSON")
    │
    ├── Single-file change? ──────────> Direct work (main agent)
    │   (e.g., "fix typo in README")
    │
    ├── Multi-step, single role? ─────> Sub-Agent (persistent)
    │   (e.g., "refactor this function")
    │
    ├── Multi-role coordination? ─────> Agent Team
    │   (e.g., "implement auth feature")
    │
    └── Batch of independent tasks? ──> Autorun (+ Agent Teams within)
        (e.g., "implement these 5 tasks")
```

---

## 13. Appendix: Research Sources

### Official Documentation
- [Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams) - Primary feature documentation
- [Claude Code Sub-agents](https://code.claude.com/docs/en/sub-agents) - Sub-agent comparison

### Case Studies and Analysis
- [Anthropic: Building a C Compiler](https://www.anthropic.com/engineering/building-c-compiler) - 16 parallel agents, 100K lines, 2B tokens
- [VentureBeat: Opus 4.6 Launch](https://venturebeat.com/technology/anthropics-claude-opus-4-6-brings-1m-token-context-and-agent-teams-to-take) - Feature announcement context
- [Paddo.dev: Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/) - Internal architecture analysis
- [Swarm Orchestration Guide (Gist)](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea) - Community patterns
- [Marc0.dev: Setup Guide](https://www.marc0.dev/en/blog/claude-code-agent-teams-multiple-ai-agents-working-in-parallel-setup-guide-1770317684454) - Practical setup reference

### Internal References
- CodeFlow Specification V3: `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v3/`
- Workflow Repository: `/Volumes/DATA/Local/software-workspace/projects/workflow/`
- CodeFlow Implementation: `/Volumes/DATA/Local/software-workspace/projects/codeflow/`
