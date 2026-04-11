# Part 1: Feedback Analysis and First-Principles Reasoning

> Systematic analysis of each M1 feedback point, reasoned from first principles.

---

## Table of Contents

- [1.1 Teammates: Functions vs Roles](#11-teammates-functions-vs-roles)
- [1.2 Delegate Mode](#12-delegate-mode)
- [1.3 Context Monitoring](#13-context-monitoring)
- [1.4 Settings Configuration](#14-settings-configuration)
- [1.5 Lead vs Teammate Differentiation in Hooks](#15-lead-vs-teammate-differentiation-in-hooks)
- [1.6 Naming: Not Outer Shell / Inner Path](#16-naming-not-outer-shell--inner-path)
- [1.7 PathFlow as Logical Construct](#17-pathflow-as-logical-construct)
- [1.8 Claude Tasks vs CodeFlow WorkGraph](#18-claude-tasks-vs-codeflow-workgraph)
- [1.9 Multi-Pathway Sessions](#19-multi-pathway-sessions)
- [1.10 Simplify Configurability](#110-simplify-configurability)
- [1.11 Sentinels and TTLs](#111-sentinels-and-ttls)
- [1.12 Direct Peer Communication](#112-direct-peer-communication)
- [1.13 Skills to Agent Definitions](#113-skills-to-agent-definitions)
- [1.14 Impact on V3 Spec Components](#114-impact-on-v3-spec-components)

---

## 1.1 Teammates: Functions vs Roles

**Feedback**: Skills define standard operating procedures (SOPs) for specific functions (git, memory, etc.). A persistent function-based teammate (like cf-gitops) that loads SOPs once and handles all git operations throughout a session is different from a role-based teammate (like cf-developer) that does variable context work.

**First-principles analysis**:

Why do skills exist? They encode *how to do things correctly* — procedures, conventions, validation rules. They are SOPs.

Why do sub-agents exist? They provide *isolated context* so the main agent's context window isn't polluted by operational details.

In Agent Teams, why would we want persistent teammates for SOPs?
- The SOP is loaded ONCE into the teammate's context
- The teammate handles ALL operations of that type throughout the session
- No re-loading of SOP per operation (saves context across the session)
- The teammate develops expertise by accumulating context about what it's done (e.g., what branches exist, what commits were made)

Why would variable-context work be on-demand?
- Each task brings new, different context (different files, different requirements)
- Context accumulation is COUNTERPRODUCTIVE — stale context from task A confuses task B
- Fresh context per task is better for quality

**Revised teammate taxonomy**:

| Category | Purpose | Persistence | Examples |
|----------|---------|:-----------:|---------|
| **Function teammates** | Follow SOPs for a specific operational domain | PERSISTENT | cf-gitops, cf-knowledge |
| **Role teammates** | Do variable-context work on specific tasks | ON-DEMAND | cf-developer, cf-planner, cf-reviewer, cf-qa, cf-documenter |
| **Sub-agents** | Quick read-only lookups, disposable | EPHEMERAL | Explore type, quick checks |

**Function teammates in detail**:

**cf-gitops** (persistent, session lifetime):
- Loads cf-git-workflow SOPs once at spawn
- Handles: branch creation, commits, PRs, sync, merge, review-changes, rebase
- Follows naming conventions, commit message formats, PR templates
- Other teammates message it directly when they need git operations done
- Accumulates useful context: what branches exist, what's been committed, PR status

**cf-knowledge** (persistent, session lifetime):
- Loads cf-memory-management + cf-task-management + cf-db-operations SOPs
- Handles: work detection, context loading, begin-work, record-progress, complete-work, WorkGraph CRUD (create/update epics and tasks), session records
- Is the interface between the session and the persistent Knowledge Layer (DB, JSONL)
- Accumulates useful context: what work is active, what progress has been made, current task status

**Why NOT cf-security as a persistent function teammate?**
- Most security enforcement is in hooks (automatic, no teammate needed)
- sandbox-check is a single lightweight operation, not worth a dedicated teammate
- Protected resource staging can be handled by whoever is doing the edit, with hook enforcement
- **Decision**: Security stays in the hook system. No dedicated security teammate.

**Why NOT cf-documentation-standards / cf-script-standards as persistent function teammates?**
- These are quality checks applied TO work products, not standalone operations
- They're more naturally quality gates in the relevant role teammate's workflow
- The developer runs linting as part of completing their task, the documenter follows doc standards
- **Decision**: Quality standards are embedded in role teammate blueprints, not separate teammates.

---

## 1.2 Delegate Mode

**Feedback**: If it can't be done via settings, just instruct.

**First-principles analysis**: The team lead needs to orchestrate, not implement. We don't need a perfect enforcement mechanism — we need clear instructions at spawn time and advisory reminders in the PathFlow task descriptions.

**Decision**: Team lead instructions in CLAUDE.md / spawn prompt state: "You are the orchestrator. You do NOT write code, edit files, or run commands directly. You create tasks, assign them to teammates, and manage the workflow." Advisory hooks can warn if the lead tries to use Edit/Write.

---

## 1.3 Context Monitoring

**Feedback**: Don't go deep unless Claude Code provides native support.

**Decision**: Drop custom context monitoring entirely. If teammates degrade, they'll naturally produce worse output and the lead will notice. The practical mitigation is: keep on-demand teammates on-demand (fresh context per task), and for persistent teammates (gitops, knowledge), their context accumulation is bounded (SOPs + session operations, not entire codebases).

---

## 1.4 Settings Configuration

**Feedback**: settings.local.json has the env var. Settings templates can be updated when we implement.

**Decision**: No settings changes needed now. `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` is in settings.local.json.

---

## 1.5 Lead vs Teammate Differentiation in Hooks

**Feedback**: Need to figure out if the read-delegation hook can differentiate team-lead from teammate, similar to main-agent vs sub-agent.

**Analysis**: From our test findings, env vars `CLAUDE_CODE_AGENT_NAME`, `CLAUDE_CODE_AGENT_TYPE`, `CLAUDE_CODE_TEAM_NAME` are NOT set in the teammate environment despite being documented. This means hooks currently CANNOT differentiate lead from teammate.

However, there may be other signals:
- The `CLAUDE_CODE_SSE_PORT` or other internal vars might differ
- The hook could check `~/.claude/teams/*/config.json` to see if a team is active
- The process tree might differ

**Decision**: This needs implementation-time investigation. For now, read-delegation applies uniformly. If we can detect team context, we adjust the delegation target (suggest messaging the appropriate teammate instead of spawning a sub-agent).

---

## 1.6 Naming: Not Outer Shell / Inner Path

**Feedback**: Consider main path / sub-paths, or something else. Use codes like PF-MN-1, PF-SB-1.

**First-principles analysis**: PathFlow is ONE flow, not two nested flows. The session moves through logical phases. Within the "work execution" phase, the work follows a stage-based progression (dev -> review -> QA). These aren't "inner" pathways plugged into an "outer" shell — they're stages within the execution phase.

**Revised naming**:
- **PathFlow**: The overall session flow
- **Phases**: Major logical milestones (PF-1 through PF-N)
- **Work Stages**: Within the execution phase, the progression of work (dev, review, QA)
- **Tasks**: Actual work items between phases (Claude Task system, ephemeral)

Phase codes: `PF-1`, `PF-2`, ... (simple, clear)
Work stage codes: `WS-DEV`, `WS-REV`, `WS-QA`, `WS-DEPLOY`

---

## 1.7 PathFlow as Logical Construct

**Feedback**: PathFlow is a series of major nodes representing logical aspects along the pathway. Between major nodes, actual tasks exist. Both use Claude's Task system with dependency ordering.

**This is a fundamental reframing.** The Claude Code Task system (TaskCreate, TaskList, TaskUpdate) becomes a DUAL-PURPOSE tool:
1. **Logical phase markers** — represent PathFlow milestones (PF-1, PF-2, etc.)
2. **Actual tasks** — represent real work to be done between milestones

Both coexist in the same task list. The dependency system (blockedBy) enforces ordering:
- Each phase marker blocks the next phase marker
- Actual tasks are blocked by the phase marker that precedes them
- The next phase marker is blocked by ALL actual tasks that precede it
- Dynamically inserted tasks automatically block the next phase marker

Example task graph:
```
[PF-1: Session Start]  <-- phase marker
  Task: Initialize session record  <-- actual task, blocked by PF-1
  Task: Clean expired state  <-- actual task, blocked by PF-1
[PF-2: Context Awareness]  <-- phase marker, blocked by both tasks above
  Task: Detect active work  <-- actual task, blocked by PF-2
  Task: Load work context  <-- actual task, blocked by detect-active-work
[PF-3: Work Classification]  <-- phase marker, blocked by load-work-context
  ...
```

This is elegant because:
- Phase markers are visible in TaskList (everyone can see where the session is)
- Dependency enforcement provides ordering (advisory, but visible)
- Dynamic tasks can be inserted between any two markers
- The team lead manages the flow by completing phase markers when appropriate

---

## 1.8 Claude Tasks vs CodeFlow WorkGraph

**CRITICAL DISTINCTION**

**Feedback**: These got confused in M1. CodeFlow's WorkGraph is persistent project management (like Linear/Jira). Claude's Task system is ephemeral session orchestration.

| Aspect | Claude Task System | CodeFlow WorkGraph |
|--------|-------------------|-------------------|
| **What** | TaskCreate, TaskList, TaskUpdate, TaskGet | epics, tasks in SurrealDB/JSONL/Markdown |
| **Scope** | Single session, ephemeral | Cross-session, permanent |
| **Purpose** | Orchestrate PathFlow nodes and teammate assignments | Track project work items, status, planning |
| **Managed by** | Team lead + PathFlow | cf-knowledge teammate (formerly cf-task-management) |
| **Analogy** | A Kanban board for today's sprint standup (thrown away after) | Jira/Linear project board (persists forever) |
| **Task IDs** | #1, #2, #3 (sequential, reset per session) | FRT-TSK-FEAT-AUTH-042 (structured, permanent) |

**How they interact**: When working on WorkGraph task `FRT-TSK-FEAT-AUTH-042`, PathFlow creates Claude Tasks for the session's workflow (PF-1, PF-2, ..., actual work tasks). The cf-knowledge teammate updates the WorkGraph task's status as the work progresses through stages.

**cf-task-management is NOT replaced**. Its operations (classify-work, ensure-work-registered, create-epic, create-task, update-task, query-tasks) are still needed — they operate on the PERSISTENT WorkGraph, not on Claude's ephemeral task list. These operations become part of cf-knowledge teammate's SOP.

---

## 1.9 Multi-Pathway Sessions

**Feedback**: A task should move through dev -> review -> QA within one session. Important for Autorun.

**First-principles analysis**: In real software development, a ticket doesn't just get "developed" — it gets developed, then reviewed, then tested. These are different stages handled by different people/roles.

In PathFlow, this means the Work Execution phase (PF-4) is not a single stage — it's a multi-stage progression:

```
PF-4: Work Execution
  +-- WS-DEV: Development stage  (cf-developer)
  |     Tasks: implement, self-test, commit
  +-- WS-REV: Review stage  (cf-reviewer)
  |     Tasks: analyze code, document findings, verdict
  +-- WS-QA: QA stage  (cf-qa)
  |     Tasks: write tests, run tests, report coverage
  +-- (optional) WS-DEPLOY: Deploy stage  (cf-gitops for PR/merge)
```

**Conditional routing**: If review finds issues:
1. cf-reviewer sends findings to cf-developer (direct communication)
2. cf-knowledge updates WorkGraph task status: `review_complete` with verdict `changes_requested`
3. PathFlow routes BACK to WS-DEV (re-open development stage)
4. Developer addresses review comments
5. Back to WS-REV for re-review

This requires **WorkGraph task status expansion**:

Current: `todo | blocked | in_progress | complete`

Proposed:
```
status: todo | in_progress | complete | blocked | cancelled
stage: null | dev | review | qa | deploy
stage_status: null | pending | in_progress | complete | failed
```

Plus a `stage_history` JSON field for tracking the progression (see [04-workgraph-schema.md](04-workgraph-schema.md)).

**Stage configuration per work type**:

| Work Type | Stages | Notes |
|-----------|--------|-------|
| Feature (FEAT) | DEV -> REV -> QA -> DEPLOY | Full pipeline |
| Bug fix (FIX) | DEV -> QA -> DEPLOY | Review optional for small fixes |
| Hotfix (HTFX) | DEV -> DEPLOY | Minimal, post-merge review flagged |
| Refactor (RFCT) | DEV -> REV -> QA | Deployment after merge |
| Docs (DOCS) | DEV (documenter) -> REV | No QA for docs |
| Test (TEST) | DEV (QA) | QA teammate IS the developer |
| Planning (SPKE/FEAT planning) | DEV (planner) | Planning-only, no code |
| Research | DEV (explorer sub-agents) | No review/QA |

---

## 1.10 Simplify Configurability

**Feedback**: Question every option. Drop things that are made up.

**First-principles questioning**:

*Do we need enforcement policies (strict/standard/permissive)?*
- Q: When would someone want LESS enforcement?
- A: During quick ad-hoc fixes where ceremony slows things down.
- Q: But does disabling enforcement actually help? Or does it just create risk?
- A: The enforcement is mainly about work tracking (ensure task_id exists) and quality (verify work before stopping). Disabling these means untracked, unverified work.
- Q: Is there a GENUINE use case where you want untracked work?
- A: Yes — pure exploration, research, or when you're just asking Claude a question. But in those cases, you probably aren't writing code or committing.
- **Decision**: TWO modes only. **Tracked** (enforcement on: work must be registered, verified) and **Untracked** (no enforcement: exploration, questions, quick lookups). The mode is determined by whether the user is DOING work (editing files, committing) or just ASKING/EXPLORING.

*Do we need session types (interactive/autorun/structured/unstructured)?*
- Q: What genuinely differs between these?
- A: Interactive vs Autorun is real — user present vs not. Structured vs unstructured is about whether an epic/task pre-exists.
- Q: Do these need separate configurations? Or are they just properties?
- **Decision**: TWO session properties: `interactive` vs `autorun` (user presence), and `has_existing_task` (boolean). Everything else derives from these. No separate "profiles."

*Do we need solo mode?*
- Q: When would you NOT want any team?
- A: When asking a simple question, or when the task is trivially simple (one file edit).
- Q: But spawning a team has overhead. Is it always worth it?
- A: No. For quick tasks, the overhead of team creation exceeds the benefit.
- **Decision**: Team creation is OPTIONAL and triggered only when the work warrants it. Not a "mode" — a pragmatic choice by the lead. The lead always exists; teammates are spawned only when beneficial.

---

## 1.11 Sentinels and TTLs

**Feedback**: With persistent function teammates following SOPs, TTLs may not make sense. Can we use task-based messaging instead?

**First-principles analysis**:

Why do sentinels exist? To prevent operations from happening in the wrong order or without prerequisites. Example: prevent `git commit` without first invoking `complete-work`.

With PathFlow and function teammates:
- cf-knowledge completes work -> updates WorkGraph status -> creates some signal
- cf-gitops receives "please commit" message -> should verify that work completion happened first

**The question is: how does cf-gitops verify that cf-knowledge completed its work?**

Options:
1. **Sentinel files (existing)**: cf-knowledge creates sentinel file, PreToolUse hook on Bash (git commit) checks for it. Works, proven.
2. **Claude Task status**: cf-gitops checks if the "complete-work" Claude Task is marked `completed` in the task list. Requires teammate to call TaskGet/TaskList.
3. **Direct messaging**: cf-knowledge messages cf-gitops: "Work completed, you may commit." cf-gitops proceeds only after receiving this message.
4. **Task dependency**: The "create-commit" Claude Task is blocked by the "complete-work" Claude Task. When complete-work finishes, commit unblocks.

Option 4 (task dependency) handles the ORDERING. But dependencies are advisory-only — a teammate could ignore them.

Option 1 (sentinels) provides HARD enforcement via hooks. Even if a teammate tries to commit out of order, the hook catches it.

**The key insight**: Sentinels in hooks provide defense-in-depth. Even if a teammate ignores task ordering, the hook catches it.

**But do sentinels need TTLs in PathFlow?**

With V3 skills, TTL made sense: the skill was invoked, sentinel created, and you wanted the downstream operation to happen SOON (within 600s). Otherwise the sentinel was "stale."

With PathFlow, staleness isn't the concern — ordering is. If cf-knowledge completes work at T+5min, and cf-gitops commits at T+25min (after the developer finished some additional cleanup), the sentinel should still be valid.

**Decision**: PathFlow sentinels have NO TTL (session-scoped). They persist for the session and are cleaned up at session end. The ordering enforcement comes from the task graph; the sentinel provides backup enforcement in hooks. Existing skill sentinels (with TTLs) remain for non-team sessions (backward compatibility).

---

## 1.12 Direct Peer Communication

**Feedback**: Hub-and-spoke through the lead defeats the purpose of Agent Teams.

**First-principles analysis**: The whole POINT of Agent Teams over sub-agents is that teammates can communicate directly. If we funnel everything through the lead, we've recreated the sub-agent model with extra overhead.

**Revised communication model**:

| Communication Type | How | Example |
|-------------------|-----|---------|
| Task assignment | Lead -> Teammate (via Claude Task system + SendMessage) | Lead assigns "implement login" to cf-developer |
| Work handoff | Teammate -> Teammate (direct SendMessage) | cf-developer -> cf-gitops: "Please commit with message X" |
| Status updates | Teammate -> Lead (SendMessage) | cf-qa -> Lead: "All tests pass" |
| Escalation | Teammate -> Lead (SendMessage) | cf-developer -> Lead: "Blocked — need clarification on requirements" |
| WorkGraph updates | Any teammate -> cf-knowledge (SendMessage) | cf-developer -> cf-knowledge: "Task FRT-TSK-042 dev stage complete" |
| Progress recording | Any teammate -> cf-knowledge (SendMessage) | cf-reviewer -> cf-knowledge: "Record decision: using strategy pattern" |

**Key pattern**: cf-knowledge and cf-gitops are "service teammates" that other teammates call on directly. The lead orchestrates the overall flow but doesn't mediate every interaction.

**The lead's role**:
- Creates the PathFlow task graph (Claude Tasks with phases and actual work)
- Assigns initial tasks to appropriate teammates
- Manages phase transitions (marks phase markers as completed)
- Handles escalations and decisions
- Monitors overall progress via TaskList
- Does NOT relay messages between teammates

---

## 1.13 Skills to Agent Definitions

**Feedback**: Should skills become agent definitions or remain as skills used as blueprints?

**First-principles analysis**:

What are the artifacts today?
- `.claude/skills/cf-*/SKILL.md` — Procedural instructions with operations, enforcement, resources
- `.claude/agents/` — Empty (not implemented yet)

What do teammates need?
- Identity and constraints (who am I, what can I do)
- Operational procedures / SOPs (how to do things correctly)
- Communication protocol (how to interact with peers)

The SKILL.md format has good structure (operations table, decision tree, enforcement levels, resources). The agent .md format is freeform.

**Proposal**: Create agent definitions in `.claude/agents/cf-*.md` that EMBED the relevant skill operations. The skill SKILL.md files remain as the canonical reference, but the agent .md files are what teammates actually load.

For function teammates (cf-gitops, cf-knowledge): The agent .md embeds ALL operations from the relevant skill(s). This is their complete SOP.

For role teammates (cf-developer, cf-planner, etc.): The agent .md embeds the role identity, constraints, and references to which function teammates to call for specific operations.

**The skills themselves remain** as:
1. Documentation of standard operating procedures
2. Source of truth for operations
3. Invokable by the lead (or non-team sessions) via the Skill tool
4. Used as "curriculum" for building agent .md files

**Skills are NOT deprecated.** They coexist with agent definitions. In team mode, the agent .md is loaded by the teammate. In non-team mode (backward compat), the skill is invoked normally.

---

## 1.14 Impact on V3 Spec Components

**What changes**:

| V3 Component | Impact | Details |
|-------------|--------|---------|
| WorkGraph (epics/tasks) | MODIFIED | Add `stage`, `stage_status`, `stage_history` to tasks table |
| Three-tier data model | UNCHANGED | JSONL -> SurrealDB -> Markdown still applies |
| CRDT/Claims | ENHANCED | Claims now associated with teammate names, not just agent names |
| Hooks | MODIFIED | PathFlow-aware hooks need to handle team context. New pathflow-gate hook. |
| Skills | PRESERVED | Skills remain as SOPs and non-team-mode operation. Agent .md files created alongside. |
| Commands | MODIFIED | Commands like `/cf-develop` now route to team lead which assigns to teammates. |
| Sub-agents | COEXIST | Sub-agents still used for quick read-only lookups. Teammates replace forked skill execution. |
| Sentinel system | EXTENDED | PathFlow sentinels (session-scoped, no TTL) added alongside skill sentinels (TTL-based). |
| Session management | ENHANCED | Sessions now include team lifecycle. active_work table gets stage tracking. |

**What does NOT change**:
- Three-tier data model (JSONL, SurrealDB, Markdown)
- JSONL ledger format
- Epic/task ID conventions
- Branch naming conventions
- Commit message format
- Protected resource system
- File scope enforcement
- Memory event recording
