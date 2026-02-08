# Part 6: Progressive Orchestration

> How PathFlow phases (PF-1 through PF-7) organize a session using progressive graph creation, phase markers, and the Claude Task system.

---

## Table of Contents

- [6.1 What Progressive Orchestration Means](#61-what-progressive-orchestration-means)
- [6.2 PathFlow Phases Overview](#62-pathflow-phases-overview)
- [6.3 Phase Marker Mechanics](#63-phase-marker-mechanics)
- [6.4 Progressive Graph Creation](#64-progressive-graph-creation)
- [6.5 Ad-Hoc Task Insertion](#65-ad-hoc-task-insertion)
- [6.6 Task Graph Example: Feature Development](#66-task-graph-example-feature-development)
- [6.7 Task Graph Example: Documentation Session](#67-task-graph-example-documentation-session)
- [6.8 Phase Completion and Sentinel Creation](#68-phase-completion-and-sentinel-creation)
- [6.9 Relationship to Work Stages](#69-relationship-to-work-stages)
- [6.10 Team Persistence: Critical Invariant](#610-team-persistence-critical-invariant)

---

## 6.1 What Progressive Orchestration Means

PathFlow uses **progressive orchestration**, not upfront choreography. The team lead does not create the entire task graph at session start. Instead, each phase's tasks are created only when the previous phase completes and the lead has enough context to plan the next step.

```
Upfront choreography (NOT how PathFlow works):
  Session start -> Create ALL 30 tasks -> Execute them

Progressive orchestration (how PathFlow works):
  PF-1 created -> PF-1 tasks run -> PF-2 created -> PF-2 tasks run -> ...
```

**Why progressive?**

1. **Context-dependent decisions**: What tasks PF-4 needs depends on what PF-3 discovered (work type, stage requirements). You cannot plan PF-4 tasks before PF-3 runs.
2. **Reduced waste**: Tasks never created are tasks that never need deletion or rework.
3. **Adaptive response**: If a review fails in PF-4, the lead creates rework tasks on the fly rather than having pre-planned a "happy path only" graph.
4. **Bounded complexity**: At any point, only the current phase's tasks plus the next phase marker exist in the task list.

---

## 6.2 PathFlow Phases Overview

PathFlow defines 7 phases. Each phase is a logical milestone in the session lifecycle.

```
PF-1  Session Start
 |
PF-2  Context Awareness
 |
PF-3  Work Classification
 |
PF-4  Work Execution         <-- Work stages live here (WS-DEV, WS-REV, WS-QA)
 |
PF-5  Work Verification
 |
PF-6  Work Completion
 |
PF-7  Session End
```

| Phase | Purpose | Key Actions |
|-------|---------|-------------|
| PF-1 | Boot the session | Clean state, create session record, spawn persistent function teammates |
| PF-2 | Load context | Detect active work, load resumption context, assess situation |
| PF-3 | Classify work | Analyze user request, register in WorkGraph, determine work stages |
| PF-4 | Execute work | Run through work stages (DEV, REVIEW, QA) with role teammates |
| PF-5 | Verify work | Confirm acceptance criteria met, artifacts complete |
| PF-6 | Finalize | Mark WorkGraph complete, create PR, shut down role teammates |
| PF-7 | Close session | Shut down persistent teammates, clean up team, finalize session |

**Simplified from M1**: The original design had 9 nodes. "Intent Identification" and "Work Path Selection" merged into PF-3 (Work Classification). "Post-Work Finalize" merged into PF-6. "Session Close" merged into PF-7. "Team Init" became a task within PF-1, not a separate phase. See [12-changes-and-claude-components.md](12-changes-and-claude-components.md) for the full delta.

---

## 6.3 Phase Marker Mechanics

Phase markers are Claude Tasks created with a `[PF-N]` naming convention. They serve as visible milestones in the task list that all teammates can see.

**Creation rules**:

- Phase markers are created with `TaskCreate` using subject format `[PF-N] Phase Name`
- They are created with status `pending`
- The team lead marks them `completed` when all tasks for that phase are done
- Each phase marker blocks the next phase marker via `blockedBy`

**Example**:

```
TaskCreate: subject="[PF-1] Session Start"
  -> Task #1 created, status=pending

TaskCreate: subject="[PF-2] Context Awareness", blockedBy=[#1]
  -> Task #4 created, status=pending, blocked by #1

... actual tasks #2, #3 created between them ...

Lead marks #1 completed
  -> #4 becomes unblocked
```

**What a phase marker represents**: A phase marker is NOT a task to be "worked on." It is a checkpoint. The lead creates it, actual work tasks are blocked by it, and the lead completes it when the phase's work is done. Completing a phase marker:

1. Unblocks the next phase's tasks
2. Triggers sentinel creation (via PostToolUse hook)
3. Signals to all teammates that the session has advanced

**Work stage markers** follow the same pattern within PF-4: `[WS-DEV]`, `[WS-REV]`, `[WS-QA]`, plus completion markers `[WS-DEV-DONE]`, `[WS-REV-DONE]`, `[WS-QA-DONE]`.

---

## 6.4 Progressive Graph Creation

The lead creates tasks phase-by-phase, not all at once. Here is the progression:

```
Time -->

T0: Lead creates [PF-1] + PF-1 tasks + [PF-2] marker
    Task list: #1 [PF-1], #2 init-session, #3 spawn-gitops, #4 [PF-2]

T1: PF-1 tasks complete. Lead marks #1 done. Creates PF-2 tasks + [PF-3] marker.
    Task list: ... #5 detect-work, #6 load-context, #7 [PF-3]

T2: PF-2 tasks complete. Lead marks #4 done. Creates PF-3 tasks + [PF-4] marker.
    Task list: ... #8 classify-work, #9 register-work, #10 [PF-4]

T3: PF-3 tasks complete. Lead marks #7 done. Creates PF-4 work stage tasks.
    (This is where the graph gets complex -- see Section 6.6)

T4: PF-4 stages complete. Lead marks #10 done. Creates PF-5 tasks + [PF-6].
    ...and so on through PF-7.
```

**Key property**: At any point in time, the task list contains:

- All completed phase markers and tasks (historical)
- The current phase marker (just completed)
- The current phase's in-progress tasks
- The next phase marker (pending, blocked)
- Nothing beyond that

This bounds the task list size and keeps it comprehensible for all teammates.

> **Critical dependency**: The progressive graph described here depends on the team's task list persisting throughout the session. If the team is dissolved mid-session, the entire graph is destroyed with no recovery path. See [Section 6.10](#610-team-persistence-critical-invariant) for the team persistence invariant.

---

## 6.5 Ad-Hoc Task Insertion

The progressive model supports dynamic task insertion at any point. The lead can add tasks between any two existing markers without restructuring the graph.

**Insertion mechanics**:

```
Before insertion:
  [PF-3] -> #8 classify -> #9 register -> [PF-4]

Lead discovers: security review needed before execution.

After insertion:
  [PF-3] -> #8 classify -> #9 register -> #10 security-review -> [PF-4]
  (with #10 blockedBy #9, and [PF-4] blockedBy #10)
```

**Wiring rules for inserted tasks**:

1. New task is blocked by the task it should follow
2. The next phase marker adds the new task to its `blockedBy` list
3. Existing tasks are NOT modified (no re-wiring needed for upstream tasks)

**From test findings**: Dynamic mid-flow insertion works immediately. New tasks appear in TaskList, dependency wiring is reflected, and the graph updates correctly. See [agent-teams-test-findings.md](../agent-teams-test-findings.md), Test F.

---

## 6.6 Task Graph Example: Feature Development

A full feature development session working on `FRT-TSK-FEAT-AUTH-042`:

```
PHASE          TASK                                              ASSIGNED TO        BLOCKED BY
------         ----                                              -----------        ----------
[PF-1]  #1    Session Start                                      (marker)           -
        #2    Initialize session record                          cf-knowledge-layer #1
        #3    Spawn cf-gitops                                    lead               #1

[PF-2]  #4    Context Awareness                                  (marker)           #2, #3
        #5    Detect active work                                 cf-knowledge-layer #4
        #6    Load work context for FRT-TSK-042                  cf-knowledge-layer #5

[PF-3]  #7    Work Classification                                (marker)           #6
        #8    Register active work, select stages DEV+REV+QA     cf-knowledge-layer #7
        #9    Create branch feat/frt-auth-validation             cf-gitops          #8

[PF-4]  #10   Work Execution                                     (marker)           #9
        #11   [WS-DEV] Development Stage                         (sub-marker)       #10
        #12   Implement login form validation                    cf-developer       #11
        #13   Commit implementation                              cf-gitops          #12
        #14   [WS-DEV-DONE]                                      (sub-marker)       #13
        #15   [WS-REV] Review Stage                              (sub-marker)       #14
        #16   Review login form implementation                   cf-reviewer        #15
        #17   [WS-REV-DONE]                                      (sub-marker)       #16
        #18   [WS-QA] QA Stage                                   (sub-marker)       #17
        #19   Write and run tests                                cf-qa              #18
        #20   Commit test files                                  cf-gitops          #19
        #21   [WS-QA-DONE]                                       (sub-marker)       #20

[PF-5]  #22   Work Verification                                  (marker)           #21
        #23   Verify acceptance criteria met                     lead               #22

[PF-6]  #24   Work Completion                                    (marker)           #23
        #25   Update WorkGraph task to complete                  cf-knowledge-layer #24
        #26   Create PR                                          cf-gitops          #25

[PF-7]  #27   Session End                                        (marker)           #26
```

**Note**: Tasks #11-#21 (the PF-4 work stages) are created by the lead only AFTER PF-3 completes and the lead knows which stages are needed. If PF-3 determined this was a docs task, only WS-DEV (with cf-documenter) and WS-REV would be created.

---

## 6.7 Task Graph Example: Documentation Session

A documentation task that uses only WORK and REVIEW stages (non-code path):

```
PHASE          TASK                                              ASSIGNED TO        BLOCKED BY
------         ----                                              -----------        ----------
[PF-1]  #1    Session Start                                      (marker)           -
        #2    Initialize session record                          cf-knowledge-layer #1

[PF-2]  #3    Context Awareness                                  (marker)           #2
        #4    Load context for DOC-TSK-DOCS-GENL-015             cf-knowledge-layer #3

[PF-3]  #5    Work Classification                                (marker)           #4
        #6    Register active work, select stages WORK+REVIEW    cf-knowledge-layer #5

[PF-4]  #7    Work Execution                                     (marker)           #6
        #8    [WS-WORK] Documentation Stage                      (sub-marker)       #7
        #9    Write API reference documentation                  cf-documenter      #8
        #10   [WS-WORK-DONE]                                     (sub-marker)       #9
        #11   [WS-REV] Review Stage                              (sub-marker)       #10
        #12   Review documentation accuracy                      cf-reviewer        #11
        #13   [WS-REV-DONE]                                      (sub-marker)       #12

[PF-5]  #14   Work Verification                                  (marker)           #13
        #15   Verify documentation completeness                  lead               #14

[PF-6]  #16   Work Completion                                    (marker)           #15
        #17   Update WorkGraph                                   cf-knowledge-layer #16
        #18   Commit and create PR                               cf-gitops          #17

[PF-7]  #19   Session End                                        (marker)           #18
```

**Key difference from feature development**: No cf-developer, no QA stage. The cf-documenter fills the WS-WORK stage, and review verifies accuracy rather than code correctness.

---

## 6.8 Phase Completion and Sentinel Creation

When the lead marks a phase marker as completed via TaskUpdate, a PostToolUse hook fires and creates a PathFlow sentinel:

```
Lead calls: TaskUpdate(#7, status="completed")
  -> PostToolUse hook detects: task subject starts with "[PF-"
  -> Hook creates sentinel: .state/sentinels/pathflow:pf-3
  -> Sentinel file contains: { phase: "pf-3", timestamp: "...", session_id: "..." }
```

This sentinel is then checked by the `cf-pre-tool-use-pathflow-gate.sh` hook before allowing operations that require a specific phase to have completed. See [08-enforcement-model.md](08-enforcement-model.md) for full sentinel mechanics.

**Work stage sub-markers** follow the same pattern:

```
TaskUpdate(#14, status="completed")  -- [WS-DEV-DONE]
  -> Sentinel: .state/sentinels/pathflow:ws-dev-done
```

---

## 6.9 Relationship to Work Stages

Progressive orchestration creates the container (phases). Work stages fill the PF-4 container with actual development pipeline steps. The two concepts are layered:

```
Progressive Orchestration (outer):
  PF-1 -> PF-2 -> PF-3 -> PF-4 -> PF-5 -> PF-6 -> PF-7

Work Stages (inside PF-4):
  PF-4 contains: WS-DEV -> WS-REV -> WS-QA
  (or WS-WORK -> WS-REV for non-code)
```

The lead determines which work stages are needed at PF-3 (Work Classification), then creates the stage tasks inside PF-4 progressively. Work stages are documented in detail in [07-work-stages.md](07-work-stages.md).

---

## 6.10 Team Persistence: Critical Invariant

The entire progressive orchestration model described in this document depends on a single critical invariant: **the team must persist from creation until PF-7 completes**.

### The Rule

Once a team is created (typically at PF-1 or PF-3), it MUST persist until PF-7 (Session End) completes. The `Teammate(operation="cleanup")` call is ONLY permitted during PF-7, after all shutdown procedures are complete.

### Why This Matters

The Claude Task system stores its task list per-team at `~/.claude/tasks/{team-name}/`. The entire PathFlow task graph -- phase markers, work stages, dependencies, assignments -- lives in that team's task list. Calling `Teammate(operation="cleanup")` deletes the team directory and its task list, which:

- **Destroys the progressive orchestration graph mid-session**: All phase markers (PF-1 through PF-7) described in [Section 6.3](#63-phase-marker-mechanics) are lost
- **Removes all sentinel creation triggers**: PostToolUse hooks cannot detect phase completions if the tasks no longer exist (see [Section 6.8](#68-phase-completion-and-sentinel-creation))
- **Loses all work stage history and dependency ordering**: The progressive graph described in [Section 6.4](#64-progressive-graph-creation) is erased entirely
- **Destroys teammate task assignments and progress**: All in-progress work becomes unrecoverable
- **Eliminates the complete audit trail**: Session progression history is gone with no trace
- **Effectively resets the session to zero with no recovery path**

### Enforcement

Three layers protect this invariant using defense-in-depth: instructions tell the lead not to do it, tasks make session state visible, and hooks hard-block if the first two fail.

1. **Instruction-level**: The team lead's instructions (CLAUDE.md / spawn-time instructions) explicitly forbid calling `Teammate(operation="cleanup")` until PF-7. This is the primary enforcement mechanism.
2. **Hook-level (mandatory)**: A PreToolUse hook (`cf-pre-tool-use-team-guard.sh`) intercepts `Teammate` tool calls with `operation="cleanup"` and blocks them (exit 2) if the `pathflow-active` flag (`/tmp/claude/managed/state/pathflow-active`) exists. This is a hard guardrail, not an advisory -- if the flag is present, the cleanup call is rejected regardless of intent.
3. **Role restriction**: Teammates should NOT have the ability to dissolve the team. Only the team lead should call cleanup, and only during PF-7.

### Teammate Shutdown vs Team Cleanup

These are different operations with very different consequences:

| Operation | What it does | Effect on task list | Safe mid-session? |
|-----------|-------------|--------------------|--------------------|
| `SendMessage(type="shutdown_request")` | Shuts down one teammate | None -- task list is preserved | Yes |
| `Teammate(operation="cleanup")` | Dissolves the team entirely | **Deletes the task list** | **No** |

Shutting down individual teammates is safe and expected. A teammate shutdown removes that agent process but does NOT affect the team's task list. The team and its task list persist even when all teammates have shut down. Only `Teammate(operation="cleanup")` destroys the team and its tasks.

### Recovery

If a team is accidentally dissolved mid-session, there is **no recovery path** for the task graph. The session should be considered broken. The lead should:

1. Log the incident as a session failure
2. Attempt to recreate the team and reconstruct minimal state from surviving sentinels (in `.state/sentinels/`) and WorkGraph data
3. Acknowledge that full session state -- including dependency ordering, assignment history, and phase progression -- is permanently lost

This is a catastrophic failure mode. Prevention through the enforcement layers above is the only reliable strategy.

---

## Related Documents

- [02-system-overview.md](02-system-overview.md) -- Full architecture with phase details
- [07-work-stages.md](07-work-stages.md) -- Work stage routing and conditional logic
- [08-enforcement-model.md](08-enforcement-model.md) -- Sentinel creation on phase completion
- [10-session-lifecycle.md](10-session-lifecycle.md) -- Session boundary and autorun integration
- [11-use-cases.md](11-use-cases.md) -- Full walkthrough examples
