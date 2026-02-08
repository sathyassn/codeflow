# Part 10: Session Lifecycle

> Tracked vs untracked sessions, team creation timing, session boundary after PR, autorun integration, and read delegation.

---

## Table of Contents

- [10.1 Tracked vs Untracked Sessions](#101-tracked-vs-untracked-sessions)
- [10.2 How Session Mode Is Detected](#102-how-session-mode-is-detected)
- [10.3 Team Creation Timing](#103-team-creation-timing)
- [10.4 Session Boundary After PR](#104-session-boundary-after-pr)
- [10.5 Autorun Integration](#105-autorun-integration)
- [10.6 Autorun Phase Traversal](#106-autorun-phase-traversal)
- [10.7 The Haiku Review Check](#107-the-haiku-review-check)
- [10.8 Rework Limits and Stage Timeouts](#108-rework-limits-and-stage-timeouts)
- [10.9 Read Delegation in Agent-Teams Mode](#109-read-delegation-in-agent-teams-mode)
- [10.10 Session Properties Summary](#1010-session-properties-summary)
- [10.11 Team Lifecycle Within a Session](#1011-team-lifecycle-within-a-session)

---

## 10.1 Tracked vs Untracked Sessions

PathFlow recognizes two session modes. The mode determines whether enforcement is active and whether PathFlow phases are created.

```
+---------------------------+     +---------------------------+
|     TRACKED SESSION       |     |    UNTRACKED SESSION      |
+---------------------------+     +---------------------------+
| PathFlow phases active    |     | No PathFlow phases        |
| Work registered in        |     | No WorkGraph registration |
|   WorkGraph               |     | No team creation          |
| Team created when needed  |     | No enforcement            |
| Sentinels enforced        |     | Direct agent interaction  |
| Three-tier data updates   |     | Quick Q&A, exploration    |
+---------------------------+     +---------------------------+
```

| Aspect | Tracked | Untracked |
|--------|---------|-----------|
| PathFlow phases | PF-1 through PF-7 | None |
| WorkGraph | Task registered, stages tracked | No WorkGraph interaction |
| Team | Created when work warrants it | Not created |
| Sentinels | Created and enforced | Not created |
| Enforcement | Full (hooks active) | Minimal (basic safety hooks only) |
| Use cases | Feature dev, bug fix, docs, refactor | Questions, exploration, quick lookups |

**There is no configuration switch.** The mode is inferred from user intent, not selected from a menu.

---

## 10.2 How Session Mode Is Detected

The session mode is inferred during PF-2 (Context Awareness) and PF-3 (Work Classification). The lead determines the mode based on signals:

**Tracked indicators** (any of these suggest tracked mode):

- User mentions a task ID: "work on FRT-TSK-042"
- User describes work that produces artifacts: "implement the login form"
- Existing active_work entry found from a previous session
- User explicitly requests: "create a task for this"
- Branch already checked out for a feature

**Untracked indicators** (all of these suggest untracked mode):

- User asks a question: "how does the auth module work?"
- User requests exploration: "show me the test coverage"
- User asks for explanation: "explain this error"
- No file modifications expected
- No active_work entries found and no work implied

**Detection flow**:

```
PF-1: Session Start (always runs -- lightweight)
  |
PF-2: Context Awareness
  |  cf-knowledge-layer checks for active_work
  |  Lead reads user prompt
  |
  +-- Active work found? -----------> TRACKED (resume)
  |
  +-- User describes work? ---------> TRACKED (new work)
  |
  +-- User asks question/explores? -> UNTRACKED
  |
  +-- Ambiguous? -------------------> Lead asks user
```

**An untracked session can become tracked**: If the user starts with a question but then says "actually, let's fix that bug," the lead transitions to tracked mode by creating PF-3 tasks and registering work. The reverse does not happen -- once tracked, a session stays tracked.

---

## 10.3 Team Creation Timing

Team creation is deferred and progressive. The lead does NOT create a team at session start. Teams are created only when work warrants multi-agent coordination.

**Team creation decision tree**:

```
Is this a tracked session?
  |
  NO --> No team. Lead handles directly.
  |
  YES
  |
  Is the work trivial (XS, single file)?
    |
    YES --> No team. Lead handles directly.
    |       (Or spawns just cf-knowledge-layer for tracking.)
    |
    NO
    |
    Create team with:
      1. cf-knowledge-layer (persistent, immediate)
      2. cf-gitops (persistent, when code work confirmed)
      3. Role teammates (on-demand, per work stage)
```

**Progressive teammate spawning**:

| When | Who | Why |
|------|-----|-----|
| PF-1 (Session Start) | cf-knowledge-layer | Needed for context awareness in PF-2 |
| PF-3 (Work Classification) | cf-gitops | Needed once we know code work is happening |
| PF-4 (WS-DEV) | cf-developer | Spawned when dev stage starts |
| PF-4 (WS-REV) | cf-reviewer | Spawned when review stage starts |
| PF-4 (WS-QA) | cf-qa | Spawned when QA stage starts |
| Any phase | Ad-hoc teammates | Spawned when the lead identifies a need |

**Why defer?** From test findings: team creation has overhead (config files, task list setup). Spawning teammates that may not be needed wastes resources. Progressive spawning matches progressive orchestration -- create what you need, when you need it.

### Ad-Hoc Teammate Flexibility

The predefined teammate roster (cf-gitops, cf-knowledge-layer, cf-developer, cf-reviewer, cf-qa, cf-planner, cf-documenter, cf-ops) represents **optimized defaults, not constraints**. The team lead can spawn additional teammates at any time during the session:

| Ad-Hoc Type | When Useful | Example |
|-------------|-------------|---------|
| General-purpose | Task falls outside predefined roles | Spawning a teammate to analyze performance metrics |
| Explore sub-agent | Quick read-only research mid-phase | Spawning an Explore agent to search a large codebase during PF-3 |
| Custom agent | Project-specific specialist needed | Spawning a teammate with a custom `.claude/agents/cf-perf-analyst.md` definition |
| Multiple instances | Parallel work on same role | Two cf-developers working on different modules simultaneously |

**The lead decides team composition based on the work at hand.** If a security audit is needed mid-session and no predefined role fits, the lead spawns a general-purpose teammate with security-focused instructions. If a task requires deep codebase exploration before planning, the lead spawns an Explore sub-agent. The roster is a starting point, not a ceiling.

**Team persistence**: Once created, the team persists for the entire session. Individual teammates join and leave, but the team itself (and its task list) is only cleaned up during PF-7. See Section 10.11 for the team lifecycle invariant. Dissolving the team mid-session destroys the entire PathFlow task graph -- all phase markers, work stage tracking, and dependency ordering.

---

## 10.4 Session Boundary After PR

PathFlow strongly recommends starting a new session after a PR is created (PF-6 -> PF-7 boundary). This is enforced by the natural structure of PathFlow, not by a hard block.

**Why new session after PR?**

1. **Context clarity**: A new session starts with fresh context. Continuing after a PR mixes old task context with new work.
2. **WorkGraph hygiene**: Each session maps to one primary work item. Mixing work items in a session complicates tracking.
3. **Team reset**: Role teammates (cf-developer, cf-reviewer, cf-qa) accumulate task-specific context. Fresh spawns for fresh work.
4. **Sentinel cleanup**: PathFlow sentinels are session-scoped. A new session gets clean sentinel state.

**How PathFlow enforces this naturally**:

```
PF-6: Work Completion
  cf-gitops creates PR             <-- Work is "done"
  cf-knowledge-layer marks task complete  <-- WorkGraph updated
  |
PF-7: Session End
  1. Lead shuts down role teammates (shutdown_request)
  2. Lead shuts down function teammates (shutdown_request)
  3. All teammates confirm shutdown
  4. Lead finalizes session (logging, etc.)
  5. Lead calls Teammate(operation="cleanup")  <-- LAST step
  6. Session ends
  |
  (session ends)
  |
New session starts -> PF-1 -> ...
```

After PF-6 completes, the only remaining phase is PF-7 (Session End). There is no "PF-8: Start New Work." The lead's instructions say: after PR creation, proceed to PF-7 and end the session. If the user wants to do more work, they start a new session.

**Not a hard block**: If the user insists on continuing ("don't end the session, I want to fix something else"), the lead can accommodate by resetting PathFlow state. But the default path is: one PR per session.

---

## 10.5 Autorun Integration

Autorun mode runs PathFlow phases automatically without human interaction. The session traverses PF-1 through PF-7 with pre-configured task parameters.

**Autorun characteristics**:

| Property | Value |
|----------|-------|
| Human present | No |
| Input | Batch file with task IDs and parameters |
| Decision-making | Automated (lead follows defaults) |
| Work stages | Determined by task work_type (from WorkGraph) |
| Review | Automated (Haiku-class model) |
| Rework | Bounded by limits (max iterations) |
| Output | PR created (or task marked blocked if failed) |

**Autorun session flow**:

```
Autorun worker receives: task_id=FRT-TSK-FEAT-AUTH-042

PF-1: Session Start
  -> Automatic: create session, spawn cf-knowledge-layer

PF-2: Context Awareness
  -> cf-knowledge-layer loads task context from WorkGraph
  -> Task has: description, acceptance_criteria, file_scope

PF-3: Work Classification
  -> Automatic: work_type from task record determines stages
  -> Feature -> DEV + REV + QA

PF-4: Work Execution
  -> WS-DEV: cf-developer implements based on task description
  -> WS-REV: cf-reviewer checks acceptance criteria (Haiku model)
  -> WS-QA: cf-qa writes and runs tests

PF-5: Work Verification
  -> Automatic: check acceptance criteria programmatically

PF-6: Work Completion
  -> cf-gitops creates PR
  -> cf-knowledge-layer marks task complete

PF-7: Session End
  -> Cleanup, record results
```

---

## 10.6 Autorun Phase Traversal

In autorun mode, phase transitions happen automatically. The lead does not wait for user input between phases.

**Automatic phase progression**:

```
PF-1 -> PF-2: Immediate (session booted -> load context)
PF-2 -> PF-3: Immediate (context loaded -> classify work)
PF-3 -> PF-4: Immediate (work classified -> begin execution)
PF-4 stages:  Sequential (DEV done -> REV -> QA)
PF-4 -> PF-5: Immediate (all stages pass -> verify)
PF-5 -> PF-6: Immediate (verified -> complete)
PF-6 -> PF-7: Immediate (PR created -> end)
```

**The only pause points** in autorun:

1. **Rework loops**: If REV or QA fails, the lead pauses to create rework tasks before continuing
2. **Rework limit reached**: If max iterations exceeded, the lead pauses to mark task as blocked
3. **Stage timeout**: If a stage exceeds its time limit, the lead intervenes

---

## 10.7 The Haiku Review Check

In autorun mode, the WS-REV stage uses a fast, cost-effective model (Haiku-class) for automated review instead of a full Opus-class reviewer.

**How it works**:

1. Lead spawns cf-reviewer with instructions to check acceptance criteria
2. cf-reviewer uses the task's `acceptance_criteria` from WorkGraph as its checklist
3. For each criterion, cf-reviewer verifies the implementation satisfies it
4. Verdict: `approved` if all criteria met, `changes_requested` if any fail
5. Findings are specific: "Criterion 3 not met: 'API returns 401 for invalid tokens' -- no test found for this case"

**Why Haiku-class?**

- Autorun sessions may process many tasks in a batch
- Full Opus-class review for every task is expensive
- Haiku is sufficient for criteria-checking (mechanical, not creative)
- If Haiku review passes but the PR reviewer (human or CI) finds issues, that feedback loops back as a new task

**Interactive sessions use full-capability reviewers**: When a human is present (interactive mode), cf-reviewer uses the same model class as other teammates. The Haiku optimization is autorun-specific.

---

## 10.8 Rework Limits and Stage Timeouts

Autorun sessions must have bounded execution to prevent runaway loops.

**Rework limits**:

```
DEV -> REV -> [changes_requested] -> DEV -> REV -> [changes_requested] -> DEV -> REV
  |         iteration 1                |         iteration 2                |      iteration 3
  |                                    |                                    |
  |                                    |                                    +-> MAX REACHED
  |                                    |                                        Mark task BLOCKED
  |                                    |                                        Skip to PF-7
```

| Parameter | Default | Source |
|-----------|---------|--------|
| `max_rework_iterations` | 3 | Task metadata or autorun config |
| `max_qa_retries` | 2 | Task metadata or autorun config |
| `stage_timeout_minutes` | 30 | Autorun config |

**Stage timeouts**:

If any single stage (DEV, REV, QA) exceeds its timeout:

1. Lead sends shutdown request to the stuck agent
2. Lead records timeout event in WorkGraph via cf-knowledge-layer
3. Lead marks task as `blocked` with reason: "stage timeout: {stage}"
4. Session proceeds to PF-7 (Session End)

**Interactive sessions**: No automatic limits in interactive mode. The human decides when to intervene. The lead may suggest: "This is the 3rd rework cycle. Should we continue or table this task?"

---

## 10.9 Read Delegation in Agent-Teams Mode

The existing read-delegation hook (`cf-pre-tool-use-read-delegation.sh`) suggests delegating file reads to sub-agents to save main context. In agent-teams mode, this behavior needs adjustment.

**The problem**: In agent-teams mode, teammates ARE the sub-agents. If cf-developer reads a file, it should NOT be told to spawn another sub-agent to read it -- cf-developer is already a separate context from the lead.

**The solution**: Read delegation is conditionally disabled in agent-teams mode.

**Detection logic**:

```
on PreToolUse(Read, file_path):
  if pathflow_active():
    # Agent-teams mode: teammates handle their own reads
    # No delegation needed
    ALLOW (no advisory message)
  else:
    # Standalone mode: suggest delegation to sub-agent
    # (existing behavior)
    ADVISE: "Consider delegating this read to a sub-agent"
```

**Agent-teams mode detection**: The hook checks for `/tmp/claude/managed/state/pathflow-active` flag. If the flag exists, the hook assumes agent-teams mode and skips the delegation advisory.

**Why not disable entirely?** The lead itself should still consider delegation for large file reads. But in practice, the lead rarely reads files directly in agent-teams mode -- it delegates work to teammates. The conditional disabling prevents noisy advisories to teammates.

---

## 10.10 Session Properties Summary

Every session has two orthogonal properties that determine its behavior:

```
                    Interactive              Autorun
                 +-------------------+  +-------------------+
                 | Human present     |  | No human          |
    Tracked      | PathFlow active   |  | PathFlow active   |
                 | Full enforcement  |  | Auto-progression  |
                 | Lead asks user    |  | Bounded execution |
                 | for decisions     |  | Haiku review      |
                 +-------------------+  +-------------------+
                 +-------------------+  +-------------------+
                 | Human present     |  | (Not applicable)  |
    Untracked    | No PathFlow       |  | Autorun requires  |
                 | Direct Q&A        |  | a task to run --  |
                 | No enforcement    |  | always tracked    |
                 +-------------------+  +-------------------+
```

| Property | Values | Determined By |
|----------|--------|---------------|
| Mode | Tracked / Untracked | Inferred from user intent at PF-2/PF-3 |
| Interaction | Interactive / Autorun | Session invocation method (CLI vs batch runner) |

**Derived behaviors**:

| Behavior | Tracked + Interactive | Tracked + Autorun | Untracked |
|----------|----------------------|-------------------|-----------|
| PathFlow phases | Full PF-1 to PF-7 | Full PF-1 to PF-7 | None |
| Team creation | When warranted | Always (needed for stages) | Never |
| Work stages | Lead decides | From task work_type | None |
| Review model | Full capability | Haiku-class | N/A |
| Rework limits | None (human decides) | Configured | N/A |
| Session boundary | Recommended after PR | Enforced after PR | No boundary |
| Enforcement | Full | Full | Basic safety only |

---

## 10.11 Team Lifecycle Within a Session

The team lifecycle is a session invariant. Once a team is created, it must persist until PF-7 (Session End). This is a critical safety constraint -- dissolving the team mid-session destroys the PathFlow task graph.

**Team lifecycle flow**:

```
Team Created (PF-1 or PF-3)
  |
  v
Teammates join/leave (PF-3 through PF-6)  <-- SAFE: individual teammate shutdown
  |                                             doesn't affect team or task list
  v
Team Cleanup (PF-7 ONLY)                  <-- ONLY time cleanup is called
  |
  v
Session End
```

**Key invariants**:

- **Team creation happens once per session**: At PF-1 for tracked sessions that know they need a team (e.g., resuming work with an existing task), or at PF-3 when work classification confirms a team is needed.
- **Individual teammates can be shut down and respawned at any time**: This is normal and expected. Role teammates (cf-developer, cf-reviewer, cf-qa) are shut down after their stage completes. Function teammates (cf-knowledge-layer, cf-gitops) persist for the full session. Shutting down a teammate does not affect the team or its task list.
- **The team itself must NEVER be dissolved until PF-7**: The team owns the task list. The task list holds all PathFlow phase markers, work stage tracking, dependency ordering, and checkpoint state. Destroying the team mid-session destroys all of this.
- **`Teammate(operation="cleanup")` is the ONLY call that destroys the team**: This call removes the team config, the task list directory, and all associated state. It must only be called as the final step of PF-7.
- **`SendMessage(type="shutdown_request")` to individual teammates is safe**: This shuts down a single teammate process. The team and task list remain intact.

**What happens if the team is dissolved mid-session**:

| Lost State | Consequence |
|------------|-------------|
| Task list | All phase markers (PF-1 through current) destroyed |
| Work stage tracking | No record of which stages completed, which are pending |
| Dependency ordering | Remaining tasks lose their dependency graph |
| Checkpoint state | Cannot resume or recover if session is interrupted |
| Teammate roster | Lead loses track of who is active |

**This is unrecoverable.** There is no mechanism to reconstruct a dissolved task list. The session must end and the work must be restarted from scratch.

**Mandatory enforcement**: Team persistence is not enforced by instructions alone. A PreToolUse hook (`cf-pre-tool-use-team-guard.sh`) guards against accidental team dissolution:

```
on PreToolUse(Teammate, params):
  if params.operation == "cleanup" and pathflow_active():
    BLOCK: "Team cleanup is not permitted while PathFlow is active.
            Only PF-7 (Session End) may dissolve the team.
            Remove the pathflow-active flag first by completing PF-7."
  else:
    ALLOW
```

The hook matches `Teammate` tool calls and blocks any `cleanup` operation while the `/tmp/claude/managed/state/pathflow-active` flag exists. PF-7 is responsible for removing this flag before calling cleanup. This follows the three-mechanism enforcement model: instructions tell the lead not to dissolve the team, task dependencies prevent premature PF-7 execution, and the hook provides a hard block as a last line of defense. All three mechanisms must agree before team dissolution can proceed.

**Note**: In agent-teams mode, the verify-work Stop hook's PCV checking is dropped entirely -- work verification is handled by the WS-REV stage during PF-4 instead of a self-check at session end. See [08-enforcement-model.md](08-enforcement-model.md), Section 8.8.

---

## Related Documents

- [06-progressive-orchestration.md](06-progressive-orchestration.md) -- Phase progression within a session
- [07-work-stages.md](07-work-stages.md) -- Stage details for tracked sessions
- [08-enforcement-model.md](08-enforcement-model.md) -- Enforcement in tracked vs untracked
- [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) -- Session recovery from Knowledge Layer
- [11-use-cases.md](11-use-cases.md) -- Walkthrough of each session type
