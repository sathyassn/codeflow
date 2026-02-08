# 02: System Overview

> PathFlow architecture, the three-mechanism model, progressive orchestration, and the complete component map.

---

## Table of Contents

- [2.1 What PathFlow Is](#21-what-pathflow-is)
- [2.2 The Three-Mechanism Model](#22-the-three-mechanism-model)
- [2.3 Progressive Orchestration](#23-progressive-orchestration)
- [2.4 PathFlow Architecture Diagram](#24-pathflow-architecture-diagram)
- [2.5 Component Map](#25-component-map)
- [2.6 How the Mechanisms Work Together](#26-how-the-mechanisms-work-together)
- [2.7 The Claude Task Graph](#27-the-claude-task-graph)

---

## 2.1 What PathFlow Is

PathFlow is a **logical progression framework** that guides Claude Code sessions through a series of phases. It is to a session what a flowchart is to a process: it defines the logical steps, their order, and the decision points. The actual work happens between the steps.

PathFlow is NOT:
- A task manager (it uses Claude's Task system, but does not replace it)
- A project tracker (the CodeFlow WorkGraph handles persistent project tracking)
- A rigid pipeline (phases can be skipped, stages can loop, tasks can be inserted dynamically)

PathFlow IS:
- A session lifecycle framework with 7 phases (PF-1 through PF-7)
- A work execution model with stages (WS-DEV, WS-REV, WS-QA, WS-WORK)
- A coordination protocol between the team lead and teammates
- An enforcement layer that ensures work quality through hooks and sentinels

---

## 2.2 The Three-Mechanism Model

PathFlow orchestrates sessions through three complementary mechanisms. Each serves a distinct purpose, and all three are needed:

```
+-------------------------------------------------------------+
|                    THREE-MECHANISM MODEL                      |
+-------------------------------------------------------------+
|                                                               |
|  INSTRUCTIONS            TASKS              HOOKS             |
|  (Drive the flow)        (Show progress)    (Enforce rules)   |
|                                                               |
|  +----------------+   +----------------+   +----------------+ |
|  | Agent defs     |   | Phase markers  |   | PreToolUse     | |
|  | CLAUDE.md      |   | (PF-1..PF-7)  |   | PostToolUse    | |
|  | Spawn prompts  |   | Work tasks     |   | Stop hooks     | |
|  | Skill SOPs     |   | Dependencies   |   | Sentinels      | |
|  +----------------+   +----------------+   +----------------+ |
|                                                               |
|  "What to do and     "Where we are and    "What must be true  |
|   how to do it"       what's next"         before proceeding" |
|                                                               |
+-------------------------------------------------------------+
```

### Mechanism 1: Instructions

Instructions define what each agent should do and how to do it. They are the "brains" of PathFlow.

| Artifact | Location | Purpose |
|----------|----------|---------|
| Agent definitions | `.claude/agents/cf-*.md` | Identity, constraints, SOPs, communication patterns |
| CLAUDE.md | `.claude/CLAUDE.md` | Project-wide instructions for the team lead |
| Spawn prompts | Built at runtime | Task-specific instructions passed to teammates at creation |
| Skill SOPs | `.claude/skills/cf-*/SKILL.md` | Canonical operational procedures |

Instructions drive the flow logic: the team lead's instructions tell it to create phases, spawn teammates, and manage transitions. Each teammate's agent definition tells it how to do its specific job and who to communicate with.

### Mechanism 2: Tasks

Tasks provide visibility into session progress. They are the "dashboard" of PathFlow.

The Claude Task system (TaskCreate, TaskList, TaskUpdate) serves a dual purpose:
1. **Phase markers** -- tasks that represent PathFlow milestones (PF-1, PF-2, ...)
2. **Work tasks** -- tasks that represent actual work to be done between milestones

Both coexist in the same task list. Dependencies between tasks create an ordering graph that everyone on the team can see.

Key properties:
- Every teammate can call `TaskList` to see overall session state
- Phase markers are visible as milestones in the list
- Task dependencies show what must complete before what
- Dynamic insertion allows the graph to adapt as work progresses

### Mechanism 3: Hooks

Hooks enforce ordering and safety rules. They are the "guardrails" of PathFlow.

| Hook Type | When It Fires | What It Enforces |
|-----------|--------------|------------------|
| PreToolUse | Before a tool call | Sentinel checks, file protection, security rules |
| PostToolUse | After a tool call | Logging, state updates, sentinel creation |
| Stop | Before session ends | Work verification (PCV), artifact checks |
| SessionStart | At session beginning | State cleanup, initialization |
| SessionEnd | At session end | Cleanup, finalization |

Hooks provide defense-in-depth. Even if a teammate ignores task dependencies (which are advisory), hooks catch out-of-order operations. For example, a PreToolUse hook on `git commit` checks that the `complete-work` sentinel exists, preventing commits of unfinished work.

---

## 2.3 Progressive Orchestration

PathFlow uses **progressive orchestration**: the team lead creates the next phase only when the current phase completes. This contrasts with upfront choreography, where the entire task graph is defined at session start.

```
Upfront Choreography (NOT PathFlow)        Progressive Orchestration (PathFlow)
====================================        ====================================

Session start:                              Session start:
  Create ALL 27 tasks                         Create PF-1 + its tasks
  Wire ALL dependencies                       Complete PF-1
  Assign ALL teammates                        Create PF-2 + its tasks
                                              Complete PF-2
  Problems:                                   Create PF-3 + its tasks
  - Can't predict work scope                  ...and so on
  - Wasted tasks for skipped stages
  - No adaptation to discoveries            Advantages:
  - Rigid, brittle                          - Adapts to what's discovered
                                            - Only creates what's needed
                                            - Handles unexpected routing
                                            - Resilient to changes
```

In practice, the team lead may create a few phases ahead when the path is predictable, but the principle holds: do not create phases that depend on information you do not yet have.

**Example**: At PF-3 (Work Classification), the lead determines that this task needs DEV + REV + QA stages. Only then does it create the WS-DEV tasks and spawn cf-developer. It does not pre-create WS-REV or WS-QA tasks until WS-DEV completes, because the review findings might change what QA needs to test.

---

## 2.4 PathFlow Architecture Diagram

```
+=================================================================+
|                      PATHFLOW SESSION                            |
+=================================================================+
|                                                                   |
|  TEAM LEAD (main agent)                                          |
|  +------------------------------------------------------------+  |
|  | - Creates PathFlow task graph (progressive)                 |  |
|  | - Spawns/shuts down teammates (predefined OR ad-hoc)        |  |
|  | - Manages phase transitions                                 |  |
|  | - Handles escalations and decisions                         |  |
|  | - Monitors progress via TaskList                            |  |
|  +------------------------------------------------------------+  |
|       |              |              |              |              |
|       | spawn        | spawn        | spawn        | spawn       |
|       v              v              v              v             |
|  +---------+   +------------+   +--------+   +---------+        |
|  |cf-gitops|   |cf-knowledge|   |cf-dev  |   |cf-rev   |  ...   |
|  |FUNCTION |   |  -layer    |   |ROLE    |   |ROLE     |        |
|  |PERSISTENT   |FUNCTION    |   |ON-DEM. |   |ON-DEM.  |        |
|  +---------+   |PERSISTENT  |   +--------+   +---------+        |
|       |        +------------+       |              |             |
|       |              |              |              |             |
|  +----+--------------+--------------+--------------+--------+   |
|  |              SHARED TASK LIST                             |   |
|  |  [PF-1] -> tasks -> [PF-2] -> tasks -> [PF-3] -> ...    |   |
|  +-----------------------------------------------------------+   |
|                                                                   |
|  +-----------------------------------------------------------+   |
|  |              HOOK ENFORCEMENT LAYER                        |   |
|  |  PreToolUse | PostToolUse | Stop | Sentinels               |   |
|  +-----------------------------------------------------------+   |
|                                                                   |
+=================================================================+
|                                                                   |
|  CODEFLOW KNOWLEDGE LAYER (persistent, cross-session)            |
|  +-----------------------------------------------------------+   |
|  | WorkGraph (SQLite) | JSONL Ledger | Memory | Markdown      |   |
|  +-----------------------------------------------------------+   |
|                                                                   |
+=================================================================+
```

---

## 2.5 Component Map

### PathFlow Components

| Component | Type | Location | Purpose |
|-----------|------|----------|---------|
| Team Lead instructions | Instruction | `.claude/CLAUDE.md` | PathFlow orchestration logic |
| cf-gitops agent def | Instruction | `.claude/agents/cf-gitops.md` | Git operations SOP |
| cf-knowledge-layer agent def | Instruction | `.claude/agents/cf-knowledge-layer.md` | WorkGraph/Memory/DB SOP |
| cf-developer agent def | Instruction | `.claude/agents/cf-developer.md` | Implementation work |
| cf-reviewer agent def | Instruction | `.claude/agents/cf-reviewer.md` | Code review |
| cf-qa agent def | Instruction | `.claude/agents/cf-qa.md` | Testing and quality |
| cf-planner agent def | Instruction | `.claude/agents/cf-planner.md` | Planning and task breakdown |
| cf-documenter agent def | Instruction | `.claude/agents/cf-documenter.md` | Documentation |
| cf-ops agent def | Instruction | `.claude/agents/cf-ops.md` | DevOps and deployment |
| Ad-hoc teammate | Instruction | Spawn prompt (no agent def file) | Situational work outside predefined roles |
| Phase markers | Task | Claude Task list | PathFlow milestones (PF-1..PF-7) |
| Work stage markers | Task | Claude Task list | Stage milestones (WS-DEV, WS-REV, WS-QA) |
| Work tasks | Task | Claude Task list | Actual work items |
| PathFlow sentinels | Hook/State | `.state/sentinels/` | Phase completion markers |
| PreToolUse hooks | Hook | `.claude/hooks/codeflow/pre-tool-use/` | Enforcement before operations |
| PostToolUse hooks | Hook | `.claude/hooks/codeflow/post-tool-use/` | State updates after operations |
| Stop hooks | Hook | `.claude/hooks/codeflow/Stop/` | Work verification before session end |

### Supporting Components (CodeFlow Infrastructure)

| Component | Location | Purpose |
|-----------|----------|---------|
| WorkGraph (SQLite) | `.state/db/codeflow.db` | Persistent task/epic storage |
| JSONL Ledger | `.state/memory/*.jsonl` | Append-only event log |
| Session records | `.state/sessions/` | Session metadata and history |
| Git hooks | `.codeflow/scripts/git-hooks/` | Commit/push enforcement |
| Security enforcement | `.codeflow/scripts/security/` | Protected paths, dangerous commands |
| Shell library | `.codeflow/scripts/shell-lib/` | Shared shell functions |

### Configuration

| Component | Location | Purpose |
|-----------|----------|---------|
| Settings | `.claude/settings.json` | Claude Code configuration |
| Settings templates | `.claude/settings-templates/` | Preset configurations (strict, standard, permissive, autonomous) |
| Protected paths | `.codeflow/protected-*.list` | Files requiring elevated care |
| Stage config | PathFlow config (TBD) | Default stages per work type |

---

## 2.6 How the Mechanisms Work Together

Here is a concrete example showing all three mechanisms working together during a transition from PF-3 to PF-4:

```
1. INSTRUCTION (agent def tells lead what to do):
   "After PF-3 completes, determine required work stages.
    Spawn cf-developer for WS-DEV. Create Claude Tasks for
    the development stage."

2. TASK (lead creates tasks in Claude Task list):
   #10 [PF-4] Work Execution        (blocked by #9)
   #11 [WS-DEV] Development Stage   (blocked by #10)
   #12 Implement login validation    (blocked by #11, assigned: cf-developer)
   #13 Commit implementation         (blocked by #12, assigned: cf-gitops)

3. HOOK (enforcement during execution):
   - PreToolUse on Edit/Write: Checks sentinel "pathflow:pf-3" exists
     (proves work was classified before coding started)
   - PreToolUse on git commit: Checks sentinel "complete-work" exists
     (proves work was properly completed before committing)
   - PostToolUse on commit: Creates sentinel "pathflow:ws-dev-done"
     (proves dev stage completed, unblocking review)
```

Each mechanism reinforces the others:
- Instructions tell agents WHAT to do
- Tasks make the plan VISIBLE to everyone
- Hooks ENFORCE that the plan was followed

---

## 2.7 The Claude Task Graph

PathFlow uses Claude's Task system to create a dependency graph that represents the session's workflow. Phase markers and work tasks coexist in the same list.

### Example: Feature Development Session

```
#1  [PF-1] Session Start                    phase marker
#2  Initialize session record                task (cf-knowledge-layer)
#3  [PF-2] Context Awareness                phase marker (blocked by #2)
#4  Detect active work                       task (cf-knowledge-layer)
#5  Load work context                        task (cf-knowledge-layer, blocked by #4)
#6  [PF-3] Work Classification              phase marker (blocked by #5)
#7  Classify and register work               task (cf-knowledge-layer)
#8  Create branch                            task (cf-gitops)
#9  [PF-4] Work Execution                   phase marker (blocked by #7, #8)
#10 [WS-DEV] Development Stage              sub-phase marker
#11 Implement login validation               task (cf-developer)
#12 Commit implementation                    task (cf-gitops, blocked by #11)
#13 [WS-REV] Review Stage                   sub-phase marker (blocked by #12)
#14 Review implementation                    task (cf-reviewer)
#15 [WS-QA] QA Stage                        sub-phase marker (blocked by #14)
#16 Write and run tests                      task (cf-qa)
#17 Commit test files                        task (cf-gitops, blocked by #16)
#18 [PF-5] Work Verification                phase marker (blocked by #17)
#19 Run PCV verification                     task (lead)
#20 [PF-6] Work Completion                  phase marker (blocked by #19)
#21 Update WorkGraph to complete             task (cf-knowledge-layer)
#22 Create PR                                task (cf-gitops)
#23 [PF-7] Session End                      phase marker (blocked by #21, #22)
```

### Task Graph Properties

| Property | Behavior | Source |
|----------|----------|--------|
| Cascading unblock | Completing a task unblocks its dependents | Claude Task system |
| Dynamic insertion | New tasks can be added and wired mid-flow | Claude Task system |
| Checkpoint re-opening | Setting a completed task to pending re-blocks dependents | Claude Task system |
| Multi-predecessor blocking | A task can be blocked by multiple tasks | Claude Task system |
| Parallel branches | Multiple tasks can depend on the same predecessor | Claude Task system |
| Advisory enforcement | Blocked tasks CAN be started (not hard-enforced) | Claude Task system limitation |
| Hard enforcement | Hooks check sentinels to block out-of-order operations | CodeFlow hooks |

For detailed phase descriptions, see [PathFlow Phases](06-progressive-orchestration.md). For work stage details, see [Work Stages](07-work-stages.md).
