# Part 2: Revised PathFlow Architecture

> The core architecture: 7 phases, work stages, task graph model, communication flow, sentinel model.

---

## Table of Contents

- [2.1 What PathFlow IS](#21-what-pathflow-is)
- [2.2 PathFlow Phases](#22-pathflow-phases)
- [2.3 Phase Details](#23-phase-details)
  - [PF-1: Session Start](#pf-1-session-start)
  - [PF-2: Context Awareness](#pf-2-context-awareness)
  - [PF-3: Work Classification](#pf-3-work-classification)
  - [PF-4: Work Execution](#pf-4-work-execution)
  - [PF-5: Work Verification](#pf-5-work-verification)
  - [PF-6: Work Completion](#pf-6-work-completion)
  - [PF-7: Session End](#pf-7-session-end)
- [2.4 Claude Task Graph Example](#24-claude-task-graph-example)
- [2.5 Teammate Communication Flow](#25-teammate-communication-flow)
- [2.6 Sentinel Model](#26-sentinel-model)

---

## 2.1 What PathFlow IS

PathFlow is a **logical progression framework** that guides Claude Code sessions through a series of phases. It is NOT a task manager — it uses Claude's Task system as its implementation mechanism, with phase markers and actual tasks coexisting in the same task list.

**PathFlow is to a session what a flowchart is to a process**: it defines the logical steps, their order, and the decision points. The actual work happens between the steps.

---

## 2.2 PathFlow Phases

```
PF-1: Session Start
  |
PF-2: Context Awareness
  |
PF-3: Work Classification
  |
PF-4: Work Execution  <-- contains work stages (WS-DEV, WS-REV, WS-QA)
  |
PF-5: Work Verification
  |
PF-6: Work Completion
  |
PF-7: Session End
```

**7 phases, not 9.** Simplified from the M1 design by merging redundant nodes:
- "Intent Identification" merged into "Work Classification" (PF-3) — they're the same thing
- "Work Path Selection" merged into "Work Classification" (PF-3) — selecting the path IS classifying the work
- "Team Init" is not a phase — it's a task within Session Start or Context Awareness
- "Post-Work Finalize" merged into "Work Completion" (PF-6)
- "Session Close" merged into "Session End" (PF-7)

---

## 2.3 Phase Details

### PF-1: Session Start

**Purpose**: Boot the session, establish infrastructure.

**Tasks between PF-1 and PF-2**:
- Clean expired sentinels and stale state (SessionStart hooks handle this automatically)
- Create session record in Knowledge Layer
- Spawn persistent function teammates (cf-gitops, cf-knowledge) — if team mode warranted
- Log session start

**Team action**: Lead spawns cf-knowledge (persistent) immediately — it's needed for context awareness. cf-gitops spawned once we know we'll be doing development work (can be deferred to PF-3).

### PF-2: Context Awareness

**Purpose**: Understand what work exists, what's in progress, what context is available.

**Tasks between PF-2 and PF-3**:
- cf-knowledge: Detect active work (query WorkGraph for in-progress tasks)
- cf-knowledge: Load work context if resuming (epic details, task description, recent progress)
- Lead: Assess current situation (user prompt, branch state, project state)

**Decision point**: Is there existing work to resume, or is this new work?

### PF-3: Work Classification

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

### PF-4: Work Execution

**Purpose**: The actual work happens here, progressing through work stages.

This is where multi-stage execution happens. The stages are determined by PF-3:

**Stage progression** (for a typical development task):
```
WS-DEV (Development)
  | cf-developer spawned on-demand
  | Implements, self-tests, asks cf-gitops to commit
  | Sends completion message to cf-knowledge
  | cf-knowledge updates WorkGraph: stage=dev, stage_status=complete
  |
WS-REV (Review)  <-- optional, based on task type
  | cf-reviewer spawned on-demand
  | Reviews code, documents findings
  | Verdict: approved -> proceed | changes_requested -> route back to WS-DEV
  | cf-knowledge updates WorkGraph: stage=review, stage_status=complete
  |
WS-QA (QA/Testing)  <-- optional, based on task type
  | cf-qa spawned on-demand
  | Writes tests, runs tests, reports coverage
  | Verdict: pass -> proceed | fail -> route back to WS-DEV
  | cf-knowledge updates WorkGraph: stage=qa, stage_status=complete
  |
WS-DEPLOY (optional)
  | cf-gitops handles PR creation, remote sync
  | cf-knowledge updates WorkGraph: stage=deploy, stage_status=complete
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
| Feature (FEAT) | DEV -> REV -> QA -> DEPLOY | Full pipeline |
| Bug fix (FIX) | DEV -> QA -> DEPLOY | Review optional for small fixes |
| Hotfix (HTFX) | DEV -> DEPLOY | Minimal, post-merge review flagged |
| Refactor (RFCT) | DEV -> REV -> QA | Deployment after merge |
| Docs (DOCS) | DEV (documenter) -> REV | No QA for docs |
| Test (TEST) | DEV (QA) | QA teammate IS the developer |
| Planning (SPKE/FEAT planning) | DEV (planner) | Planning-only, no code |
| Research | DEV (explorer sub-agents) | No review/QA |

### PF-5: Work Verification

**Purpose**: Verify the work meets acceptance criteria before marking complete.

**Tasks between PF-5 and PF-6**:
- Lead: Determine verification level (simple for small tasks, thorough for large ones)
- Lead: Run Post-Completion Verification (PCV) — check artifacts, verify tests pass, check acceptance criteria
- For autorun: Programmatically evaluate acceptance criteria from the WorkGraph task

**This is the existing Stop hook enforcement point** (cf-stop-verify-work.sh). It blocks session completion until PCV is done.

### PF-6: Work Completion

**Purpose**: Finalize work, update WorkGraph, ensure everything is persisted.

**Tasks between PF-6 and PF-7**:
- cf-knowledge: Mark WorkGraph task as `complete` (stage=done, status=complete)
- cf-knowledge: Record final progress event in JSONL
- cf-gitops: Final sync-remote if needed, ensure PR is created
- Lead: Shut down on-demand teammates (cf-developer, cf-reviewer, cf-qa)

### PF-7: Session End

**Purpose**: Clean up and close the session.

**Tasks at PF-7**:
- Lead: Shut down persistent teammates (cf-gitops, cf-knowledge)
- Lead: Clean up team (Teammate.cleanup())
- System: SessionEnd hooks fire (archive state, finalize log)
- System: Finalize session record

---

## 2.4 Claude Task Graph Example

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

**Phase markers**: #1, #4, #7, #10, #22, #24, #27
**Work stage markers**: #11, #14, #15, #17, #18, #21
**Actual tasks**: everything else

---

## 2.5 Teammate Communication Flow

```
Session Start:
  Lead spawns cf-knowledge (persistent)
  Lead spawns cf-gitops (persistent)

Work Execution (DEV stage):
  Lead spawns cf-developer (on-demand)
  Lead assigns task #12 to cf-developer
  cf-developer implements...
  cf-developer -> cf-gitops: "Please commit: feat(auth): add login validation"
  cf-gitops commits, messages cf-developer: "Committed as abc123"
  cf-developer -> cf-knowledge: "Dev stage complete for FRT-TSK-042"
  cf-knowledge updates WorkGraph
  cf-developer -> Lead: "Task #12 complete. Implementation done."
  Lead shuts down cf-developer

Work Execution (REV stage):
  Lead spawns cf-reviewer (on-demand)
  Lead assigns task #16 to cf-reviewer
  cf-reviewer reviews...
  cf-reviewer -> cf-knowledge: "Review complete. Verdict: approved."
  cf-reviewer -> Lead: "Task #16 complete. Code approved."
  Lead shuts down cf-reviewer

Work Execution (QA stage):
  Lead spawns cf-qa (on-demand)
  Lead assigns task #19 to cf-qa
  cf-qa writes tests, runs them...
  cf-qa -> cf-gitops: "Please commit test files: test(auth): add login validation tests"
  cf-gitops commits
  cf-qa -> cf-knowledge: "QA stage complete. 8/8 tests pass. Coverage 94%."
  cf-qa -> Lead: "Task #19 complete. All tests pass."
  Lead shuts down cf-qa

Completion:
  cf-gitops -> Lead: "PR #42 created against main"
  cf-knowledge -> Lead: "WorkGraph task FRT-TSK-042 marked complete"
  Lead shuts down cf-gitops, cf-knowledge
  Lead cleans up team
```

---

## 2.6 Sentinel Model

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
