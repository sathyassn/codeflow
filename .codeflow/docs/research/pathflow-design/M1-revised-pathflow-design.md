# PathFlow Revised Design: First-Principles Rethink

> Comprehensive revision based on M1 feedback. Simplifies architecture, corrects conflations, rethinks teammate model, and grounds every decision in first principles.
> Date: 2026-02-06

---

## Part 1: Feedback Analysis and First-Principles Reasoning

### 1.1 Teammates: Functions vs Roles

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

Why NOT cf-security as a persistent function teammate?
- Most security enforcement is in hooks (automatic, no teammate needed)
- sandbox-check is a single lightweight operation, not worth a dedicated teammate
- Protected resource staging can be handled by whoever is doing the edit, with hook enforcement
- **Decision**: Security stays in the hook system. No dedicated security teammate.

Why NOT cf-documentation-standards / cf-script-standards as persistent function teammates?
- These are quality checks applied TO work products, not standalone operations
- They're more naturally quality gates in the relevant role teammate's workflow
- The developer runs linting as part of completing their task, the documenter follows doc standards
- **Decision**: Quality standards are embedded in role teammate blueprints, not separate teammates.

### 1.2 Delegate Mode

**Feedback**: If it can't be done via settings, just instruct.

**First-principles analysis**: The team lead needs to orchestrate, not implement. We don't need a perfect enforcement mechanism — we need clear instructions at spawn time and advisory reminders in the PathFlow task descriptions.

**Decision**: Team lead instructions in CLAUDE.md / spawn prompt state: "You are the orchestrator. You do NOT write code, edit files, or run commands directly. You create tasks, assign them to teammates, and manage the workflow." Advisory hooks can warn if the lead tries to use Edit/Write.

### 1.3 Context Monitoring

**Feedback**: Don't go deep unless Claude Code provides native support.

**Decision**: Drop custom context monitoring entirely. If teammates degrade, they'll naturally produce worse output and the lead will notice. The practical mitigation is: keep on-demand teammates on-demand (fresh context per task), and for persistent teammates (gitops, knowledge), their context accumulation is bounded (SOPs + session operations, not entire codebases).

### 1.4 Settings Configuration

**Feedback**: settings.local.json has the env var. Settings templates can be updated when we implement.

**Decision**: No settings changes needed now. `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` is in settings.local.json.

### 1.5 Can We Differentiate Team Lead vs Teammate in Hooks?

**Feedback**: Need to figure out if the read-delegation hook can differentiate team-lead from teammate, similar to main-agent vs sub-agent.

**Analysis**: From our test findings, env vars `CLAUDE_CODE_AGENT_NAME`, `CLAUDE_CODE_AGENT_TYPE`, `CLAUDE_CODE_TEAM_NAME` are NOT set in the teammate environment despite being documented. This means hooks currently CANNOT differentiate lead from teammate.

However, there may be other signals:
- The `CLAUDE_CODE_SSE_PORT` or other internal vars might differ
- The hook could check `~/.claude/teams/*/config.json` to see if a team is active
- The process tree might differ

**Decision**: This needs implementation-time investigation. For now, read-delegation applies uniformly. If we can detect team context, we adjust the delegation target (suggest messaging the appropriate teammate instead of spawning a sub-agent).

### 1.6 Naming: Not Outer Shell / Inner Path

**Feedback**: Consider main path / sub-paths, or something else. Use codes like PF-MN-1, PF-SB-1.

**First-principles analysis**: PathFlow is ONE flow, not two nested flows. The session moves through logical phases. Within the "work execution" phase, the work follows a stage-based progression (dev → review → QA). These aren't "inner" pathways plugged into an "outer" shell — they're stages within the execution phase.

**Revised naming**:
- **PathFlow**: The overall session flow
- **Phases**: Major logical milestones (PF-1 through PF-N)
- **Work Stages**: Within the execution phase, the progression of work (dev, review, QA)
- **Tasks**: Actual work items between phases (Claude Task system, ephemeral)

Phase codes: `PF-1`, `PF-2`, ... (simple, clear)
Work stage codes: `WS-DEV`, `WS-REV`, `WS-QA`, `WS-DEPLOY`

### 1.7 PathFlow as Logical Construct, Not Concrete Tasks

**Feedback**: PathFlow is a series of major nodes representing logical aspects along the pathway. Between major nodes, actual tasks exist. Both use Claude's Task system with dependency ordering.

**This is a fundamental reframing**. Let me think through what this means:

The Claude Code Task system (TaskCreate, TaskList, TaskUpdate) becomes a DUAL-PURPOSE tool:
1. **Logical phase markers** — represent PathFlow milestones (PF-1, PF-2, etc.)
2. **Actual tasks** — represent real work to be done between milestones

Both coexist in the same task list. The dependency system (blockedBy) enforces ordering:
- Each phase marker blocks the next phase marker
- Actual tasks are blocked by the phase marker that precedes them
- The next phase marker is blocked by ALL actual tasks that precede it
- Dynamically inserted tasks automatically block the next phase marker

Example task graph:
```
[PF-1: Session Start]  ← phase marker
  Task: Initialize session record  ← actual task, blocked by PF-1
  Task: Clean expired state  ← actual task, blocked by PF-1
[PF-2: Context Awareness]  ← phase marker, blocked by both tasks above
  Task: Detect active work  ← actual task, blocked by PF-2
  Task: Load work context  ← actual task, blocked by detect-active-work
[PF-3: Work Classification]  ← phase marker, blocked by load-work-context
  ...
```

This is elegant because:
- Phase markers are visible in TaskList (everyone can see where the session is)
- Dependency enforcement provides ordering (advisory, but visible)
- Dynamic tasks can be inserted between any two markers
- The team lead manages the flow by completing phase markers when appropriate

### 1.8 Claude Task System vs CodeFlow WorkGraph — CRITICAL DISTINCTION

**Feedback**: These got confused. CodeFlow's WorkGraph is persistent project management (like Linear/Jira). Claude's Task system is ephemeral session orchestration.

**Let me be very clear**:

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

### 1.9 Multi-Pathway Sessions

**Feedback**: A task should move through dev → review → QA within one session. Important for Autorun.

**First-principles analysis**: In real software development, a ticket doesn't just get "developed" — it gets developed, then reviewed, then tested. These are different stages handled by different people/roles.

In PathFlow, this means the "Work Execution" phase (PF-6) is not a single stage — it's a multi-stage progression:

```
PF-6: Work Execution
  ├── WS-DEV: Development stage  (cf-developer)
  │     Tasks: implement, self-test, commit
  ├── WS-REV: Review stage  (cf-reviewer)
  │     Tasks: analyze code, document findings, verdict
  ├── WS-QA: QA stage  (cf-qa)
  │     Tasks: write tests, run tests, report coverage
  └── (optional) WS-DEPLOY: Deploy stage  (cf-gitops for PR/merge)
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

Plus a `stage_history` JSONL field or separate table for tracking the progression:
```json
[
  {"stage": "dev", "status": "complete", "started_at": "...", "completed_at": "...", "verdict": "pass"},
  {"stage": "review", "status": "complete", "started_at": "...", "completed_at": "...", "verdict": "changes_requested", "reason": "missing error handling"},
  {"stage": "dev", "status": "complete", "started_at": "...", "completed_at": "...", "verdict": "pass"},
  {"stage": "review", "status": "complete", "started_at": "...", "completed_at": "...", "verdict": "approved"},
  {"stage": "qa", "status": "in_progress", "started_at": "..."}
]
```

**DB Schema change needed**: Add `stage`, `stage_status`, `stage_history` to the `tasks` table. This is a V3 spec modification, not a fundamental redesign.

### 1.10 Simplify Configurability

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

### 1.11 Sentinels and TTLs

**Feedback**: With persistent function teammates following SOPs, TTLs may not make sense. Can we use task-based messaging instead?

**First-principles analysis**:

Why do sentinels exist? To prevent operations from happening in the wrong order or without prerequisites. Example: prevent `git commit` without first invoking `complete-work`.

With PathFlow and function teammates:
- cf-knowledge completes work → updates WorkGraph status → creates some signal
- cf-gitops receives "please commit" message → should verify that work completion happened first

**The question is: how does cf-gitops verify that cf-knowledge completed its work?**

Options:
1. **Sentinel files (existing)**: cf-knowledge creates sentinel file, PreToolUse hook on Bash (git commit) checks for it. Works, proven.
2. **Claude Task status**: cf-gitops checks if the "complete-work" Claude Task is marked `completed` in the task list. Requires TeamTask to call TaskGet/TaskList.
3. **Direct messaging**: cf-knowledge messages cf-gitops: "Work completed, you may commit." cf-gitops proceeds only after receiving this message.
4. **Task dependency**: The "create-commit" Claude Task is blocked by the "complete-work" Claude Task. When complete-work finishes, commit unblocks.

Option 4 (task dependency) handles the ORDERING. But we know dependencies are advisory-only — a teammate could ignore them.

Option 1 (sentinels) provides HARD enforcement via hooks. Even if a teammate tries to commit out of order, the hook blocks it.

**The key insight**: Sentinels in hooks provide defense-in-depth. Even if a teammate ignores task ordering, the hook catches it. This is valuable.

**But do sentinels need TTLs in PathFlow?**

With V3 skills, TTL made sense: the skill was invoked, sentinel created, and you wanted the downstream operation to happen SOON (within 600s). Otherwise the sentinel was "stale."

With PathFlow, staleness isn't the concern — ordering is. If cf-knowledge completes work at T+5min, and cf-gitops commits at T+25min (after the developer finished some additional cleanup), the sentinel should still be valid.

**Decision**: PathFlow sentinels have NO TTL (session-scoped). They persist for the session and are cleaned up at session end. The ordering enforcement comes from the task graph; the sentinel provides backup enforcement in hooks. Existing skill sentinels (with TTLs) remain for non-team sessions (backward compatibility).

### 1.12 Direct Peer Communication

**Feedback**: Hub-and-spoke through the lead defeats the purpose of Agent Teams.

**First-principles analysis**: The whole POINT of Agent Teams over sub-agents is that teammates can communicate directly. If we funnel everything through the lead, we've recreated the sub-agent model with extra overhead.

**Revised communication model**:

| Communication Type | How | Example |
|-------------------|-----|---------|
| Task assignment | Lead → Teammate (via Claude Task system + SendMessage) | Lead assigns "implement login" to cf-developer |
| Work handoff | Teammate → Teammate (direct SendMessage) | cf-developer → cf-gitops: "Please commit with message X" |
| Status updates | Teammate → Lead (SendMessage) | cf-qa → Lead: "All tests pass" |
| Escalation | Teammate → Lead (SendMessage) | cf-developer → Lead: "Blocked — need clarification on requirements" |
| WorkGraph updates | Any teammate → cf-knowledge (SendMessage) | cf-developer → cf-knowledge: "Task FRT-TSK-042 dev stage complete" |
| Progress recording | Any teammate → cf-knowledge (SendMessage) | cf-reviewer → cf-knowledge: "Record decision: using strategy pattern" |

**Key pattern**: cf-knowledge and cf-gitops are "service teammates" that other teammates call on directly. The lead orchestrates the overall flow but doesn't mediate every interaction.

**The lead's role**:
- Creates the PathFlow task graph (Claude Tasks with phases and actual work)
- Assigns initial tasks to appropriate teammates
- Manages phase transitions (marks phase markers as completed)
- Handles escalations and decisions
- Monitors overall progress via TaskList
- Does NOT relay messages between teammates

### 1.13 Skills → Agent Definitions

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

**Skills are NOT deprecated**. They coexist with agent definitions. In team mode, the agent .md is loaded by the teammate. In non-team mode (backward compat), the skill is invoked normally.

### 1.14 Impact on V3 Spec Components

**What changes**:

| V3 Component | Impact | Details |
|-------------|--------|---------|
| WorkGraph (epics/tasks) | MODIFIED | Add `stage`, `stage_status`, `stage_history` to tasks table |
| Three-tier data model | UNCHANGED | JSONL → SurrealDB → Markdown still applies |
| CRDT/Claims | ENHANCED | Claims now associated with teammate names, not just agent names. Teammates create claims for their file scope. |
| Hooks | MODIFIED | PathFlow-aware hooks need to handle team context. New pathflow-gate hook possible. |
| Skills | PRESERVED | Skills remain as SOPs and non-team-mode operation. Agent .md files created alongside. |
| Commands | MODIFIED | Commands like `/cf-develop` now route to team lead which assigns to teammates, rather than forking to sub-agent. |
| Sub-agents | COEXIST | Sub-agents still used for quick read-only lookups (Explore type). Teammates replace forked skill execution. |
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

---

## Part 2: Revised PathFlow Architecture

### 2.1 What PathFlow IS

PathFlow is a **logical progression framework** that guides Claude Code sessions through a series of phases. It is NOT a task manager — it uses Claude's Task system as its implementation mechanism, with phase markers and actual tasks coexisting in the same task list.

**PathFlow is to a session what a flowchart is to a process**: it defines the logical steps, their order, and the decision points. The actual work happens between the steps.

### 2.2 PathFlow Phases

```
PF-1: Session Start
  │
PF-2: Context Awareness
  │
PF-3: Work Classification
  │
PF-4: Work Execution  ← contains work stages (WS-DEV, WS-REV, WS-QA)
  │
PF-5: Work Verification
  │
PF-6: Work Completion
  │
PF-7: Session End
```

**7 phases, not 9.** Simplified from the M1 design by merging redundant nodes:
- "Intent Identification" merged into "Work Classification" (PF-3) — they're the same thing
- "Work Path Selection" merged into "Work Classification" (PF-3) — selecting the path IS classifying the work
- "Team Init" is not a phase — it's a task within Session Start or Context Awareness
- "Post-Work Finalize" merged into "Work Completion" (PF-6)
- "Session Close" merged into "Session End" (PF-7)

### 2.3 Phase Details

#### PF-1: Session Start

**Purpose**: Boot the session, establish infrastructure.

**Tasks between PF-1 and PF-2**:
- Clean expired sentinels and stale state (SessionStart hooks handle this automatically)
- Create session record in Knowledge Layer
- Spawn persistent function teammates (cf-gitops, cf-knowledge) — if team mode warranted
- Log session start

**Team action**: Lead spawns cf-knowledge (persistent) immediately — it's needed for context awareness. cf-gitops spawned once we know we'll be doing development work (can be deferred to PF-3).

#### PF-2: Context Awareness

**Purpose**: Understand what work exists, what's in progress, what context is available.

**Tasks between PF-2 and PF-3**:
- cf-knowledge: Detect active work (query WorkGraph for in-progress tasks)
- cf-knowledge: Load work context if resuming (epic details, task description, recent progress)
- Lead: Assess current situation (user prompt, branch state, project state)

**Decision point**: Is there existing work to resume, or is this new work?

#### PF-3: Work Classification

**Purpose**: Understand what the user wants, classify it, register it in WorkGraph, determine which work stages are needed.

**Tasks between PF-3 and PF-4**:
- Lead: Analyze user request (understand what they're asking for)
- cf-knowledge: Classify work (area, type, domain) — using cf-task-management SOPs
- cf-knowledge: Ensure work is registered (task_id NOT NULL) — create task in ongoing epic if needed
- cf-knowledge: Register active_work entry with task_id
- Lead: Determine work stages needed (DEV only? DEV + REV? DEV + REV + QA?)
- Lead: Spawn cf-gitops (persistent) if not already spawned
- Lead: Create the Claude Task graph for PF-4 with appropriate work stages

**This is where the WorkGraph task gets linked**: If the user says "work on FRT-TSK-FEAT-AUTH-042", cf-knowledge looks it up. If they say "fix the login bug", cf-knowledge classifies and creates a new task.

#### PF-4: Work Execution

**Purpose**: The actual work happens here, progressing through work stages.

This is where multi-stage execution happens. The stages are determined by PF-3:

**Stage progression** (for a typical development task):
```
WS-DEV (Development)
  │ cf-developer spawned on-demand
  │ Implements, self-tests, asks cf-gitops to commit
  │ Sends completion message to cf-knowledge
  │ cf-knowledge updates WorkGraph: stage=dev, stage_status=complete
  │
WS-REV (Review)  ← optional, based on task type
  │ cf-reviewer spawned on-demand
  │ Reviews code, documents findings
  │ Verdict: approved → proceed | changes_requested → route back to WS-DEV
  │ cf-knowledge updates WorkGraph: stage=review, stage_status=complete
  │
WS-QA (QA/Testing)  ← optional, based on task type
  │ cf-qa spawned on-demand
  │ Writes tests, runs tests, reports coverage
  │ Verdict: pass → proceed | fail → route back to WS-DEV
  │ cf-knowledge updates WorkGraph: stage=qa, stage_status=complete
  │
WS-DEPLOY (optional)
  │ cf-gitops handles PR creation, remote sync
  │ cf-knowledge updates WorkGraph: stage=deploy, stage_status=complete
```

**Conditional routing**: If WS-REV verdict is `changes_requested`:
1. cf-reviewer messages cf-developer directly with findings
2. cf-knowledge updates WorkGraph stage_history with the review failure
3. Lead creates new Claude Tasks for the rework + re-review
4. New WS-DEV tasks are blocked by the "review findings" task
5. New WS-REV tasks are blocked by the rework tasks

**Stage configuration per work type**:

| Work Type | Stages | Notes |
|-----------|--------|-------|
| Feature (FEAT) | DEV → REV → QA → DEPLOY | Full pipeline |
| Bug fix (FIX) | DEV → QA → DEPLOY | Review optional for small fixes |
| Hotfix (HTFX) | DEV → DEPLOY | Minimal, post-merge review flagged |
| Refactor (RFCT) | DEV → REV → QA | Deployment after merge |
| Docs (DOCS) | DEV (documenter) → REV | No QA for docs |
| Test (TEST) | DEV (QA) | QA teammate IS the developer |
| Planning (SPKE/FEAT planning) | DEV (planner) | Planning-only, no code |
| Research | DEV (explorer sub-agents) | No review/QA |

#### PF-5: Work Verification

**Purpose**: Verify the work meets acceptance criteria before marking complete.

**Tasks between PF-5 and PF-6**:
- Lead: Determine verification level (simple for small tasks, thorough for large ones)
- Lead: Run Post-Completion Verification (PCV) — check artifacts, verify tests pass, check acceptance criteria
- For autorun: Programmatically evaluate acceptance criteria from the WorkGraph task

**This is the existing Stop hook enforcement point** (cf-stop-verify-work.sh). It blocks session completion until PCV is done.

#### PF-6: Work Completion

**Purpose**: Finalize work, update WorkGraph, ensure everything is persisted.

**Tasks between PF-6 and PF-7**:
- cf-knowledge: Mark WorkGraph task as `complete` (stage=done, status=complete)
- cf-knowledge: Record final progress event in JSONL
- cf-gitops: Final sync-remote if needed, ensure PR is created
- Lead: Shut down on-demand teammates (cf-developer, cf-reviewer, cf-qa)

#### PF-7: Session End

**Purpose**: Clean up and close the session.

**Tasks at PF-7**:
- Lead: Shut down persistent teammates (cf-gitops, cf-knowledge)
- Lead: Clean up team (Teammate.cleanup())
- System: SessionEnd hooks fire (archive state, finalize log)
- System: Finalize session record

### 2.4 How the Claude Task Graph Looks

For a typical feature development session working on `FRT-TSK-FEAT-AUTH-042`:

```
#1  [PF-1] Session Start                    (phase marker)
#2  Initialize session record                (blocked by #1, assigned: cf-knowledge)
#3  Spawn cf-gitops                          (blocked by #1, assigned: lead)
#4  [PF-2] Context Awareness                (blocked by #2, #3)
#5  Detect active work                       (blocked by #4, assigned: cf-knowledge)
#6  Load work context for FRT-TSK-042        (blocked by #5, assigned: cf-knowledge)
#7  [PF-3] Work Classification              (blocked by #6)
#8  Register active work, select stages      (blocked by #7, assigned: cf-knowledge)
#9  Create branch feat/frt-auth-validation   (blocked by #8, assigned: cf-gitops)
#10 [PF-4] Work Execution                   (blocked by #9)
#11 [WS-DEV] Development Stage              (blocked by #10, sub-phase marker)
#12 Implement login form validation          (blocked by #11, assigned: cf-developer)
#13 Commit implementation                    (blocked by #12, assigned: cf-gitops)
#14 [WS-DEV-DONE]                           (blocked by #13)
#15 [WS-REV] Review Stage                   (blocked by #14, sub-phase marker)
#16 Review login form implementation         (blocked by #15, assigned: cf-reviewer)
#17 [WS-REV-DONE]                           (blocked by #16)
#18 [WS-QA] QA Stage                        (blocked by #17, sub-phase marker)
#19 Write and run tests                      (blocked by #18, assigned: cf-qa)
#20 Commit test files                        (blocked by #19, assigned: cf-gitops)
#21 [WS-QA-DONE]                            (blocked by #20)
#22 [PF-5] Work Verification                (blocked by #21)
#23 Run PCV verification                     (blocked by #22, assigned: lead)
#24 [PF-6] Work Completion                  (blocked by #23)
#25 Update WorkGraph task to complete        (blocked by #24, assigned: cf-knowledge)
#26 Create PR                                (blocked by #25, assigned: cf-gitops)
#27 [PF-7] Session End                      (blocked by #26)
```

Phase markers: #1, #4, #7, #10, #22, #24, #27
Work stage markers: #11, #14, #15, #17, #18, #21
Actual tasks: everything else

### 2.5 Teammate Communication Flow

```
Session Start:
  Lead spawns cf-knowledge (persistent)
  Lead spawns cf-gitops (persistent)

Work Execution (DEV stage):
  Lead spawns cf-developer (on-demand)
  Lead assigns task #12 to cf-developer
  cf-developer implements...
  cf-developer → cf-gitops: "Please commit: feat(auth): add login validation"
  cf-gitops commits, messages cf-developer: "Committed as abc123"
  cf-developer → cf-knowledge: "Dev stage complete for FRT-TSK-042"
  cf-knowledge updates WorkGraph
  cf-developer → Lead: "Task #12 complete. Implementation done."
  Lead shuts down cf-developer

Work Execution (REV stage):
  Lead spawns cf-reviewer (on-demand)
  Lead assigns task #16 to cf-reviewer
  cf-reviewer reviews...
  cf-reviewer → cf-knowledge: "Review complete. Verdict: approved."
  cf-reviewer → Lead: "Task #16 complete. Code approved."
  Lead shuts down cf-reviewer

Work Execution (QA stage):
  Lead spawns cf-qa (on-demand)
  Lead assigns task #19 to cf-qa
  cf-qa writes tests, runs them...
  cf-qa → cf-gitops: "Please commit test files: test(auth): add login validation tests"
  cf-gitops commits
  cf-qa → cf-knowledge: "QA stage complete. 8/8 tests pass. Coverage 94%."
  cf-qa → Lead: "Task #19 complete. All tests pass."
  Lead shuts down cf-qa

Completion:
  cf-gitops → Lead: "PR #42 created against main"
  cf-knowledge → Lead: "WorkGraph task FRT-TSK-042 marked complete"
  Lead shuts down cf-gitops, cf-knowledge
  Lead cleans up team
```

### 2.6 Sentinel Model (Simplified)

**Two types of sentinels coexist**:

| Type | TTL | Purpose | When Used |
|------|-----|---------|-----------|
| **Skill sentinels** | 600s (existing) | Ensure skill was invoked recently before downstream operation | Non-team sessions (backward compat) |
| **PathFlow sentinels** | Session-scoped (no expiry) | Ensure PathFlow phase completed before downstream work | Team sessions |

**PathFlow sentinels**:
- Created when a phase marker is completed
- Named: `pathflow:{phase}` (e.g., `pathflow:pf-3`, `pathflow:ws-dev-done`)
- Checked by: `cf-pre-tool-use-pathflow-gate.sh` (new hook)
- No TTL — valid for entire session, cleaned up at session end
- Provides defense-in-depth alongside task graph ordering

**Which sentinels do we actually need?**

| Sentinel | Purpose | Still Needed? |
|----------|---------|:------------:|
| complete-work before commit | Prevent committing unfinished work | Yes — cf-gitops checks before committing |
| sandbox-check before push | Prevent unauthorized network access | Yes — hook enforcement for network ops |
| ensure-work-registered before Edit/Write | Prevent untracked file modifications | Yes — work must be in WorkGraph |
| pathflow:pf-3 before Edit/Write | Prevent coding before work is classified | Yes — ensures PathFlow was followed |

**The simplification**: PathFlow sentinels replace the need for MANY individual skill sentinels in team mode. If `pathflow:pf-3` exists (work classified and registered), that implicitly means work IS registered. The granular `cf-task-management:ensure-work-registered` sentinel becomes redundant because PathFlow PF-3 encompasses it.

**But** — for backward compatibility in non-team sessions, skill sentinels remain unchanged.

---

## Part 3: Revised Teammate Model

### 3.1 Complete Teammate Roster

| Teammate | Category | Persistence | Source Skills | Purpose |
|----------|----------|:-----------:|:-------------|---------|
| **Team Lead** | Orchestrator | PERSISTENT | cf-working-protocol (embedded) | PathFlow management, task assignment, decisions |
| **cf-gitops** | Function | PERSISTENT | cf-git-workflow | All git operations following conventions |
| **cf-knowledge** | Function | PERSISTENT | cf-memory-management, cf-task-management, cf-db-operations | WorkGraph CRUD, memory ops, session tracking |
| **cf-developer** | Role | ON-DEMAND | (code exploration tips, script standards) | Implementation work |
| **cf-planner** | Role | ON-DEMAND | cf-documentation-standards (for plan docs) | Planning, epic/task breakdown |
| **cf-reviewer** | Role | ON-DEMAND | (code exploration tips) | Code review |
| **cf-qa** | Role | ON-DEMAND | cf-testing-workflow, cf-script-standards (for test quality) | Test writing and execution |
| **cf-documenter** | Role | ON-DEMAND | cf-documentation-standards | Technical documentation |
| **cf-ops** | Role | ON-DEMAND | (deployment procedures) | DevOps, CI/CD, deployment |

**cf-ops IS for DevOps** (not git operations). cf-ops handles deployment pipelines, CI/CD configuration, infrastructure. Git operations (branching, commits, PRs) are cf-gitops territory.

### 3.2 Agent Definition Format

Agent definitions live at `.claude/agents/cf-*.md`. They are loaded by teammates at spawn time via "Read your agent definition at `.claude/agents/cf-{role}.md`".

**Structure**:
```markdown
---
name: cf-{role}
description: {What this teammate does}. {When the lead should spawn it}.
---

# {Role Name}

## Identity
Who you are, what team you're on, how to communicate.

## Constraints
Branch access, file scope, tool restrictions, memory domain.

## Standard Operating Procedures
The actual operations/procedures from the relevant skill(s).
This is the core SOP content — what to do and how to do it.

## Communication
Who to message for what. Direct peer communication patterns.
- cf-gitops for any git operations
- cf-knowledge for any WorkGraph/memory updates
- Lead for escalations and decisions

## Quality Checklist
What to verify before marking work complete.
```

### 3.3 Skills Coexistence

Skills remain in `.claude/skills/`. They serve as:
1. **Canonical SOPs** — the authoritative source for operational procedures
2. **Non-team-mode invocation** — when running without Agent Teams, skills work as before
3. **Content source** — agent .md files embed/reference skill operations
4. **User-invokable commands** — `/cf-commit`, `/cf-plan` etc. still work

In team mode, commands route to teammates: `/cf-commit` → lead tells cf-gitops to commit.
In non-team mode, commands invoke skills normally: `/cf-commit` → fork to cf-general-purpose.

---

## Part 4: WorkGraph Schema Changes

### 4.1 Tasks Table Additions

```sql
-- New columns on tasks table
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;
  -- null | dev | review | qa | deploy | done
ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL;
  -- null | pending | in_progress | complete | failed
ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]';
  -- JSON array of stage progression records
```

### 4.2 Stage History Format

```json
[
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-06T14:00:00Z",
    "completed_at": "2026-02-06T14:25:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Implemented login validation"
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-06T14:26:00Z",
    "completed_at": "2026-02-06T14:35:00Z",
    "agent": "cf-reviewer",
    "verdict": "changes_requested",
    "notes": "Missing error handling for network timeout"
  },
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-06T14:36:00Z",
    "completed_at": "2026-02-06T14:42:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Added network timeout error handling"
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-06T14:43:00Z",
    "completed_at": "2026-02-06T14:48:00Z",
    "agent": "cf-reviewer",
    "verdict": "approved"
  }
]
```

### 4.3 Active Work Table Enhancement

```sql
ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL;
ALTER TABLE active_work ADD COLUMN team_name TEXT DEFAULT NULL;
```

This tracks which work stage is currently active and which team is handling it.

---

## Part 5: Open Questions

### Q1: V3 Modification vs V4?

Given the revised design, the changes to V3 are:
- Add `stage`, `stage_status`, `stage_history` to tasks table
- Add `current_stage`, `team_name` to active_work table
- Create agent definitions in `.claude/agents/`
- Add PathFlow-aware hooks
- Modify commands to support team routing

These are ADDITIONS to V3, not replacements. The existing V3 architecture (skills, hooks, sub-agents, WorkGraph, three-tier data) remains intact and operational.

**Revised recommendation**: MODIFY V3, don't create V4. The changes are additive, not breaking. Agent Teams mode is a layer ON TOP of V3, not a replacement.

### Q2: Stage Configuration

Where should the stage configuration (which work types get which stages) live?
- Option A: In the WorkGraph task metadata (per-task)
- Option B: In a PathFlow config file (global defaults by work type)
- Option C: Determined dynamically by the lead based on task complexity

Leaning toward: Option B with Option A overrides. A config file maps work types to default stages, individual tasks can override.

### Q3: Who Determines When to Spawn a Team?

Not every session needs a team. When the user says "what's the status of my tasks?" — no team needed. When the user says "implement this feature" — team needed.

Should the lead auto-detect? Or should the user signal? Or should it be based on whether the work involves file modifications?

### Q4: Autorun Integration

In autorun mode, the batch config specifies the WorkGraph task ID. PathFlow runs automatically. But how does the batch config specify which stages to run? Options:
- Batch config includes `stages: [dev, review, qa, deploy]`
- Batch config says `full_pipeline: true` (all stages)
- Stages determined from the task's work_type (automatic)

### Q5: Peer Communication Reliability

Our testing showed that teammates CAN message each other directly. But messages are fire-and-forget (no delivery guarantee to shut-down teammates). For the direct communication model to work reliably:
- How do we handle if cf-gitops is overloaded/slow?
- What if a message is sent but the recipient has shut down?
- Should there be a timeout + escalation pattern?

### Q6: How Does the Lead Know Which Teammate to Spawn?

In team mode, when the user says something, the lead needs to know which teammate to spawn. This routing logic needs to be documented. Should it be:
- In the lead's instructions (CLAUDE.md)
- In the PathFlow config
- Derived from the work stage (DEV stage → cf-developer, REV stage → cf-reviewer)
- This seems naturally stage-driven: the work stage determines the teammate.

---

## Part 6: Summary of Changes from M1 Design

| Aspect | M1 Design | Revised Design | Why |
|--------|-----------|---------------|-----|
| PathFlow structure | 9-node outer shell + pluggable inner pathways | 7-phase single flow with work stages inside PF-4 | Simpler, one unified flow |
| Naming | OS-1 through OS-9, inner pathway templates | PF-1 through PF-7, WS-DEV/REV/QA | Clearer, hierarchical |
| Teammate model | Role-based only | Function (persistent SOPs) + Role (on-demand variable work) | Functions like git/memory benefit from persistence |
| cf-gitops | Didn't exist (git was everyone's responsibility) | Dedicated persistent function teammate for all git ops | Consistent git procedures, loaded once |
| cf-knowledge | Didn't exist (cf-memory-management dissolved) | Dedicated persistent function teammate for Knowledge Layer | Single interface to DB/JSONL/WorkGraph |
| cf-task-management | "Dissolved into native TaskCreate" | PRESERVED as cf-knowledge's SOP for WorkGraph CRUD | WorkGraph ≠ Claude Tasks. WorkGraph is permanent. |
| Communication | Hub-and-spoke through lead | Direct peer communication + lead orchestration | Leverages Agent Teams' key advantage |
| Delegate mode | Hook-based enforcement | Instruction-based guidance | Simpler, sufficient |
| Context monitoring | Custom hook-based self-reporting | Dropped | Not reliable without native support |
| Configurability | 4 layers, 3 enforcement policies, 4 session types | Tracked vs Untracked mode, interactive vs autorun property | Radically simplified |
| Sentinels | Session-scoped TTL-free pathway sentinels | Same, but fewer needed due to PathFlow phase coverage | Simplification |
| Multi-pathway | Deferred to future | Core feature: work stages (DEV → REV → QA) with conditional routing | Essential for real development workflows |
| WorkGraph task status | No changes | Add stage, stage_status, stage_history columns | Enables multi-stage tracking |
| V3 impact | V4 recommended | V3 modification (additive) | Changes are additions, not replacements |

---

## Part 7: What I Need From You

Before proceeding to detailed design and implementation planning:

1. **Teammate roster**: Does the cf-gitops + cf-knowledge (persistent function) + role-based on-demand model make sense? Any roles missing or unnecessary?

2. **Work stages**: Is DEV → REV → QA → DEPLOY the right default progression? Should some stages be mandatory vs optional per work type?

3. **Multi-pathway routing**: When review finds issues, the revised design routes back to DEV. Is this the right pattern? Are there other routing conditions to handle?

4. **WorkGraph schema changes**: The proposed `stage`, `stage_status`, `stage_history` additions — any concerns about the schema design?

5. **V3 modification**: Agreed on modifying V3 rather than creating V4?

6. **Team spawning trigger**: What should trigger team creation vs solo work?

7. **Anything else that needs rethinking?**
