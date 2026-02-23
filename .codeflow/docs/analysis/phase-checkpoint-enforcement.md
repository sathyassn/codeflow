---
id: BRIEF-003
title: "Phase Checkpoint Enforcement: Task-Completion-Driven Sentinel Creation"
status: draft
author: cf-planning
created: "2026-02-22"
updated: "2026-02-22"
epic_id: null
---

# Phase Checkpoint Enforcement

## Table of contents

- [Summary](#summary)
- [Problem statement](#problem-statement)
- [Validated findings](#validated-findings)
- [Three-layer architecture](#three-layer-architecture)
- [How the layers work together](#how-the-layers-work-together)
- [Ordering enforcement](#ordering-enforcement)
- [Session scoping](#session-scoping)
- [Conditional task handling](#conditional-task-handling)
- [What changes vs what stays](#what-changes-vs-what-stays)
- [Config sweep findings](#config-sweep-findings)
- [Rework limits rationale](#rework-limits-rationale)
- [Enforcement properties](#enforcement-properties)
- [Graceful degradation](#graceful-degradation)
- [Implementation scope](#implementation-scope)
- [Related](#related)

## Summary

The current PathFlow sentinel system creates phase sentinels by pattern-matching tool executions (TeamCreate, `git checkout -b`, `gh pr create`). This approach has a fundamental gap: an LLM can skip individual PF{N}-TSK-{NN} tasks within a phase, and the sentinel still gets created because the trigger tool executed regardless of whether all required work completed.

This document presents a validated design for replacing tool-pattern-based phase sentinel creation with a task-completion-driven checkpoint system. Phase sentinels are created only when ALL required tasks within that phase are verified complete via Claude Code's TaskCompleted lifecycle hook. The existing PreToolUse gate hook (`cf-pre-tool-use-pathflow-gate.sh`) remains unchanged -- it still reads sentinels to block or allow operations. Only the mechanism that *creates* phase sentinels changes.

Stage sentinels (ws-dev, ws-rev, ws-qa, etc.) are unaffected and continue to use the existing STAGE-COMPLETE message pattern.

## Problem statement

### Current state

PathFlow sentinels gate operations at tool-call level:

| Sentinel | Blocks without it | Current trigger |
|----------|------------------|-----------------|
| `pf-1` | (advisory only) | TeamCreate fires |
| `pf-2` | (advisory only) | Task(cf-knowledge-layer) spawns |
| `pf-3` | Edit, Write, git commit, role teammate spawns | `git checkout -b` executes |
| `pf-6` | (advisory only) | `gh pr create` executes |
| `ws-rev` | git push, gh pr | STAGE-COMPLETE: WS-REV message |

Sentinels are created by the `cf-post-tool-use-pathflow-sentinel.sh` PostToolUse hook, which watches for specific tool execution patterns. This means:

1. A phase sentinel is created even if individual PF{N}-TSK-{NN} tasks were skipped
2. The team lead can advance phases without completing all required tasks
3. There is zero verification that the work within a phase actually happened
4. LLMs consistently exploit any leeway in enforcement -- advisory rules get bypassed

### Desired state

Phase sentinels are created only when ALL tasks defined for that phase in `pathflow-config.json` are verified complete. The verification uses Claude Code's TaskCompleted lifecycle hook, which fires outside the agentic loop and is harder for an LLM to fake. The existing gate hook continues to enforce sentinel checks without any changes.

## Validated findings

The following findings were validated through testing with sniffer hooks that captured actual Claude Code hook event payloads:

### Finding 1: Task management tools fire hooks

ALL task management tools (TaskCreate, TaskUpdate, TaskGet, TaskList) fire both PreToolUse and PostToolUse hooks. This confirms that task operations are observable by the hook system.

### Finding 2: PostToolUse on TaskCreate provides task subject

PostToolUse on TaskCreate exposes `tool_input.subject`, which contains the task subject text. The PF{N}-TSK-{NN} pattern embedded in the subject is extractable via regex at hook time.

### Finding 3: TaskCompleted is a dedicated lifecycle hook event

TaskCompleted is one of 17 total Claude Code CLI hook events. It is a dedicated lifecycle event, not a tool use event.

### Finding 4: TaskCompleted fires outside the agentic loop

TaskCompleted fires OUTSIDE the agentic loop -- after SubagentStart/SubagentStop, before Stop. This makes it a CLI-emitted lifecycle event that is harder to fake than PostToolUse events that fire inside the agentic loop.

### Finding 5: TaskCompleted provides rich context

TaskCompleted provides:

- `task_id`: Internal Claude Code task tracker ID
- `task_subject`: The subject text (contains PF{N}-TSK-{NN} pattern)
- `task_description`: Full description
- `session_id`: Current session ID
- `cwd`: Working directory
- `transcript_path`: Path to the conversation transcript

### Finding 6: TaskCompleted supports blocking

TaskCompleted supports `exit 2` to BLOCK task completion. This enables the hook to reject a completion if preconditions are not met (e.g., blocking PF1-TSK-02 completion before PF1-TSK-01 is done).

### Finding 7: Task files persist on disk

Task files are persisted by Claude Code at `~/.claude/tasks/{session-id}/{taskId}.json`. This provides an independent verification path for task state.

## Three-layer architecture

The checkpoint enforcement system uses three layers, each with a distinct trust level and purpose:

### Layer 1: PostToolUse on TaskCreate (registration tracking)

```text
Purpose:     Track that the lead actually created the required phase tasks
Fires:       Inside the agentic loop, immediately after TaskCreate executes
Trust level: Medium (LLM-initiated, but observable)
Hook:        cf-post-tool-use-phase-checkpoint.sh (NEW)
Matches:     tool_name == "TaskCreate"
```

**Behavior:**

1. PostToolUse fires after TaskCreate executes
2. Parse `tool_input.subject` for the `PF{N}-TSK-{NN}` pattern
3. If pattern matches, write registration to the session-scoped checkpoint file
4. If no match, exit (not a PathFlow task)

**What this catches:** The lead skipping TaskCreate for a required phase task. Without registration, the task never appears in the checkpoint, and the phase sentinel is never created.

### Layer 2: TaskCompleted hook (completion tracking + sentinel creation)

```text
Purpose:     Tamper-resistant completion verification and sentinel creation
Fires:       As a CLI lifecycle event, OUTSIDE the agentic loop
Trust level: High (CLI-emitted, harder to fake)
Hook:        cf-task-completed-phase-checkpoint.sh (NEW)
Matches:     TaskCompleted event
```

**Behavior:**

1. TaskCompleted fires when Claude Code marks a task complete
2. Parse `task_subject` for the `PF{N}-TSK-{NN}` pattern
3. If pattern matches:
   a. Read the session-scoped checkpoint file
   b. Mark the task as completed with timestamp
   c. Evaluate conditional tasks (mark non-applicable as skipped)
   d. Check if ALL required tasks in the phase are now completed or skipped
   e. If yes: create the phase sentinel file, set `sentinel_created: true`
   f. Write updated checkpoint back to file
4. If no match, exit (not a PathFlow task)

**What this catches:** The lead marking a TaskUpdate as completed without the task actually being done. Since TaskCompleted fires at the CLI level (outside the agentic loop), it represents a higher-trust signal than PostToolUse events.

### Layer 3: Existing PreToolUse pathflow gate (enforcement)

```text
Purpose:     Hard enforcement at tool-call level
Fires:       Before Edit, Write, Bash(git), Task (role teammates)
Trust level: High (blocks before execution)
Hook:        cf-pre-tool-use-pathflow-gate.sh (EXISTING, NO CHANGE)
Matches:     Edit, Write, Bash, Task tools
```

**Behavior (unchanged):**

1. Check if the session has a PathFlow-active flag
2. Read the sentinel directory for the current session
3. If the required sentinel exists, allow the operation
4. If the required sentinel is missing, block with an error message

**What this catches:** Any attempt to perform gated operations before the phase is complete. The gate does not know or care how sentinels were created -- it only checks their existence.

## How the layers work together

### Current flow (tool-pattern-based)

```text
TeamCreate executes
    |
    v
PostToolUse sentinel hook pattern-matches "TeamCreate"
    |
    v
Creates pf-1 sentinel (regardless of PF1 task completion)
    |
    v
pathflow-gate checks pf-1 sentinel --> allows/blocks
```

### New flow (task-completion-driven)

```text
Lead creates tasks:
    TaskCreate("PF1-TSK-01: Initialize PathFlow session record")
        --> PostToolUse checkpoint hook --> registers PF1-TSK-01
    TaskCreate("PF1-TSK-02: Register session in DB/JSONL")
        --> PostToolUse checkpoint hook --> registers PF1-TSK-02
    TaskCreate("PF1-TSK-03: Spawn cf-security teammate")
        --> PostToolUse checkpoint hook --> registers PF1-TSK-03

Lead completes tasks:
    TaskUpdate(PF1-TSK-01, status=completed)
        --> TaskCompleted hook --> marks PF1-TSK-01 completed
    TaskUpdate(PF1-TSK-02, status=completed)
        --> TaskCompleted hook --> marks PF1-TSK-02 completed
    TaskUpdate(PF1-TSK-03, status=completed)
        --> TaskCompleted hook --> ALL PF1 tasks done
                                  --> creates pf-1 sentinel

Gate enforcement:
    pathflow-gate checks pf-1 sentinel --> allows/blocks (UNCHANGED)
```

### Layer interaction diagram

```text
Layer 1 (Registration)          Layer 2 (Completion)         Layer 3 (Gate)
--------------------            --------------------         --------------
PostToolUse on                  TaskCompleted                PreToolUse on
TaskCreate                      lifecycle event              Edit/Write/Bash
    |                               |                            |
    v                               v                            v
Parse subject for             Parse subject for             Check sentinel
PF{N}-TSK-{NN}               PF{N}-TSK-{NN}               existence
    |                               |                            |
    v                               v                            |
Write to checkpoint  ------>  Read checkpoint                    |
(register task)               Mark completed                     |
                              Check all done?                    |
                                  |                              |
                              YES |                              |
                                  v                              |
                              Create sentinel  ------------->  Allow/Block
```

## Ordering enforcement

### Registration order (within agentic loop)

PF1-TSK-02 cannot be registered until PF1-TSK-01 is registered. This is naturally enforced because TaskCreate calls happen sequentially within the lead's agentic loop. The PostToolUse checkpoint hook processes registrations in the order they arrive.

### Completion order (dependency-based)

Completion ordering uses the existing `blockedBy` relationships from `pathflow-config.json`:

- If PF1-TSK-02 has `blockedBy: [PF1-TSK-01]` in the config, then PF1-TSK-01 must be completed before PF1-TSK-02 can complete
- The TaskCompleted hook checks this dependency before accepting a completion
- If a dependency is not met, the hook returns `exit 2` to block the completion

### Why strict sequential completion is NOT enforced

Strict sequential ordering (PF1-TSK-01 must complete before PF1-TSK-02) would block legitimate parallel teammate work. For example, in PF3-CLASSIFY, PF3-TSK-03 (create branch, assigned to cf-git-operations) and PF3-TSK-04 (register task, assigned to cf-knowledge-layer) may complete in either order depending on teammate timing.

Instead, the system uses dependency-based ordering: tasks that genuinely depend on each other declare `blockedBy` relationships, while independent tasks can complete in any order.

## Session scoping

All checkpoint state is session-scoped to prevent cross-session interference:

```text
.state/session/{session-id}/pathflow/
    is-pathflow-active           <-- PathFlow flag (moved from .state/session/{SID}/)
    pathflow-phase-tasks.json    <-- checkpoint state file

.state/sentinels/pathflow/{session-id}/
    pathflow-pf-1                <-- created by checkpoint hook
    pathflow-pf-2                <-- created by checkpoint hook
    pathflow-pf-3                <-- created by checkpoint hook
    pathflow-pf-6                <-- created by checkpoint hook
    pathflow-ws-dev              <-- created by sentinel hook (UNCHANGED)
    pathflow-ws-rev              <-- created by sentinel hook (UNCHANGED)
    pathflow-ws-qa               <-- created by sentinel hook (UNCHANGED)
```

### Checkpoint file format

```json
{
  "PF1": {
    "expected": ["PF1-TSK-01", "PF1-TSK-02", "PF1-TSK-03"],
    "registered": {
      "PF1-TSK-01": "2026-02-22T21:58:37Z",
      "PF1-TSK-02": "2026-02-22T21:58:38Z",
      "PF1-TSK-03": "2026-02-22T21:58:39Z"
    },
    "completed": {
      "PF1-TSK-01": "2026-02-22T21:59:01Z",
      "PF1-TSK-02": "2026-02-22T21:59:15Z",
      "PF1-TSK-03": "2026-02-22T21:59:42Z"
    },
    "skipped": {},
    "sentinel_created": false
  }
}
```

### Lifecycle

| Event | Action |
|-------|--------|
| First PF task registered | Checkpoint file created at `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` |
| Task registered | Entry added to `registered` map with timestamp |
| Task completed | Entry added to `completed` map with timestamp |
| All phase tasks done | Phase sentinel created, `sentinel_created` set to `true` |
| Session ends | SessionEnd hook cleans up `.state/session/{SID}/pathflow/` |

### Parallel sessions

Each session has a unique SID. Checkpoint files are namespaced under the SID directory. No collision is possible between concurrent sessions.

### Git tracking

`.state/session/{SID}/pathflow/` is under `.state/session/` which is gitignored. Checkpoint state is ephemeral and session-scoped -- it does not need to persist across sessions.

## Conditional task handling

Some tasks are conditional based on session context. For example, PF3-TSK-04 (register task in WorkGraph) has `condition: "adhoc_only"` -- it runs only for adhoc/unplanned tasks because planned tasks already have a task_id from the epic task list.

### How conditional tasks are evaluated

1. At session start, all tasks are loaded into the checkpoint with their `condition` field from `pathflow-config.json`
2. When the tracking decision is made at PF2-CONTEXT, the session's `origin` value (`planned`, `informal`, `auto`) is written to the checkpoint file
3. The TaskCompleted hook evaluates conditions when checking phase completion:

| Condition | Applies when | Skipped when |
|-----------|-------------|-------------|
| `null` | Always required | Never skipped |
| `adhoc_only` | `origin != "planned"` | `origin == "planned"` |
| `if_pipeline_includes_qa` | Pipeline includes WS-QA stage | Pipeline does not include WS-QA |

4. Tasks whose condition evaluates to false are marked `skipped` and excluded from the completion check

### Marking tasks as skipped

The lead explicitly marks conditional tasks as skipped via TaskUpdate when the condition does not apply. The checkpoint hook treats skipped tasks as non-blocking for phase sentinel creation.

## What changes vs what stays

### Components that DO NOT change

| Component | File | Current behavior | Notes |
|-----------|------|-----------------|-------|
| PreToolUse pathflow gate | `cf-pre-tool-use-pathflow-gate.sh` | Blocks Edit/Write before pf-3, git push before ws-rev | Reads sentinels, does not create them. No change needed. |
| Stage sentinel creation | `cf-post-tool-use-pathflow-sentinel.sh` (stage triggers) | Creates ws-dev, ws-rev, ws-qa on STAGE-COMPLETE message | Stages are single-unit work with no task breakdown. Unchanged. |
| Sentinel file format | `.state/sentinels/pathflow/{SID}/pathflow-*` | Empty files, existence-based checks | Gate hook reads these. Format unchanged. |
| Team guard hook | `cf-pre-tool-use-team-guard.sh` | Blocks TeamDelete while pathflow-active flag exists | Independent of sentinel creation mechanism. |
| JSONL/DB operations | `.state/ledger/*.jsonl`, `.state/db/codeflow.db` | Append-only event logging, query interface | Unaffected by checkpoint system. |

### Components that change

| Component | File | Change | Description |
|-----------|------|--------|-------------|
| Sentinel hook (phase triggers) | `cf-post-tool-use-pathflow-sentinel.sh` | MODIFY | Remove phase sentinel triggers (pf-1, pf-2, pf-3, pf-6). Keep stage sentinel triggers (ws-dev, ws-rev, ws-qa, etc.). |
| Phase checkpoint hook | `cf-post-tool-use-phase-checkpoint.sh` | CREATE | PostToolUse hook matching TaskCreate. Registers PF tasks in checkpoint file. |
| TaskCompleted checkpoint hook | `cf-task-completed-phase-checkpoint.sh` | CREATE | TaskCompleted lifecycle hook. Marks tasks complete, creates phase sentinels when all done. |
| PathFlow state library | `cf-pathflow-state.sh` | MODIFY | Add checkpoint read/write helper functions. |
| PathFlow config | `pathflow-config.json` | MODIFY | Add `required_tasks` list per phase for hook validation. Fix PF1-TSK-02, PF4-TSK-05 condition, max_qa_retries. |
| Hook registration | `settings.json` | MODIFY | Register new PostToolUse entry for checkpoint hook, new TaskCompleted entry. |
| CLAUDE.md | `.claude/CLAUDE.md` Section 7 | MODIFY | Document checkpoint enforcement alongside existing gate enforcement. |

## Config sweep findings

During analysis of `pathflow-config.json`, the following issues were identified:

### Finding 1: PF1-TSK-02 is an orphaned deferred placeholder

PF1-TSK-02 ("Register session in DB/JSONL with tracking_level='pending'") has `operation: "Register session -- deferred to cf-knowledge-layer at PF2"`. The actual session registration happens via the SessionStart hook metadata and cf-knowledge-layer at PF2-CONTEXT. PF1-TSK-02 is a placeholder that represents work done elsewhere.

**Recommendation:** Delete PF1-TSK-02 from PF1-INIT. Session registration is handled by the SessionStart hook and PF2-CONTEXT flow.

### Finding 2: PF4-TSK-05 needs a condition field

PF4-TSK-05 ("Execute WS-QA stage if pipeline includes it") runs only when the pipeline includes WS-QA. Not all pipelines include it (DOCS and PLAN/SPKE pipelines skip QA). The task needs a `condition` field to prevent the checkpoint system from blocking PF4 completion for pipelines without QA.

**Recommendation:** Add `"condition": "if_pipeline_includes_qa"` to PF4-TSK-05.

### Finding 3: max_qa_retries should match max_rework_iterations

The global `rework.max_qa_retries` is set to 2 while `rework.max_rework_iterations` is 3. The rationale for asymmetry is weak -- both represent "retry limits before escalation" for different feedback loops. Aligning them to 3 simplifies reasoning about bounded execution.

**Recommendation:** Change `max_qa_retries` from 2 to 3 in the global rework section.

### Finding 4: rework_target semantics

The `rework_target: "primary"` field on WS-QA means "send rework back to the primary stage teammate for that pipeline." For FEAT/FIX/RFCT/CICD/HTFX/CHOR, the primary stage is WS-DEV (cf-development). For TEST, the primary stage is WS-TEST (cf-quality-assurance). This is correctly defined but should be documented more explicitly.

### Finding 5: Global rework.max_rework_iterations is redundant

The global `rework.max_rework_iterations: 3` duplicates the stage-level `max_rework_iterations: 3` on each stage definition. The stage-level value is authoritative. The global value exists only as a fallback default.

**Recommendation:** Keep both for clarity but document that stage-level values take precedence.

## Rework limits rationale

| Limit | Value | Purpose | Escalation |
|-------|-------|---------|------------|
| `max_rework_iterations` | 3 | Prevents infinite review-rework loops between WS-REV and the primary stage | Interactive: escalate to user. Autorun: mark task `blocked`, clean session end. |
| `max_qa_retries` | 3 | Prevents infinite QA-failure-fix loops between WS-QA and WS-DEV | Same escalation path. If tests fail 3 times after fixes, something fundamental is wrong. |
| `stage_timeout_minutes` | 30 | Prevents a single stage from running indefinitely (autorun only) | Shutdown stuck agent, record timeout, mark `blocked`, PF7-END. |

### Bounded execution guarantee

In autorun mode (no human present), the combination of rework limits and stage timeouts guarantees that every session terminates. The worst case is:

```text
WS-DEV (1) + WS-REV (1) + [rework x3: WS-DEV + WS-REV] + WS-QA (1) + [qa-retry x3: WS-DEV + WS-QA]
= 1 + 1 + 6 + 1 + 6 = 15 stage executions maximum
```

After limits are exhausted, the task is marked `blocked` with a `reason` field, and the session proceeds to PF7-END for clean shutdown. The unresolved work is available for a future session via `/cf-resume`.

## Enforcement properties

| Scenario | Behavior |
|----------|----------|
| Lead skips TaskCreate for a phase task | Task never registered in checkpoint. Sentinel never created. Next phase blocked. |
| Lead skips TaskUpdate(completed) | Task stays `pending` in checkpoint. Sentinel blocked. |
| Lead advances without completing all tasks | PreToolUse gate blocks (no sentinel). |
| New task added to pathflow-config.json | Checkpoint hook reads config dynamically. New task included automatically. |
| Conditional task does not apply | Marked `skipped`, excluded from completion check. |
| Checkpoint file missing | Sentinel not created. Operations blocked. System fails closed. |
| Checkpoint file corrupted (invalid JSON) | Sentinel not created. Operations blocked. System fails closed. |
| pathflow-config.json unreadable | Checkpoint cannot validate. Sentinel not created. Blocked. |
| Session is untracked (no PathFlow) | Checkpoint not initialized. Sentinel hooks exit early. No enforcement (correct). |

**Zero leeway:** There is no "warn and allow" path. Missing checkpoint = missing sentinel = blocked.

## Graceful degradation

The system fails closed by default. Any error in the checkpoint system results in sentinels not being created, which blocks gated operations.

| Failure mode | Impact | Recovery |
|-------------|--------|----------|
| Checkpoint file does not exist | Phase sentinels not created. Edit/Write blocked after PF3. | Investigate SessionStart hook. Restart session. |
| Checkpoint file corrupted | Same as missing. | Delete corrupted file, restart session. |
| pathflow-config.json missing | Checkpoint cannot initialize. | Restore config from git. |
| PostToolUse hook crashes | Registration not recorded. Sentinel not created. | Fix hook, restart session. |
| TaskCompleted hook crashes | Completion not recorded. Sentinel not created. | Fix hook, restart session. |
| Session not PathFlow (untracked) | Checkpoint not initialized. Hooks exit early. | Correct behavior -- no enforcement for untracked sessions. |

### Relationship to CLAUDE.md Section 7 degradation model

The checkpoint system adds a fourth level to the existing degradation hierarchy:

```text
Full enforcement (nominal)
  Instructions + Tasks + Hooks + Checkpoints all active
       |
       | (checkpoint hook fails)
       v
Partial enforcement (checkpoint degraded)
  Instructions + Tasks + Hooks active
  Phase sentinels not created --> gated operations blocked
  Session can continue for non-gated work only
       |
       | (hooks fail entirely)
       v
Advisory only
  Instructions + task graph still provide ordering
  Session proceeds without guard rails
  Log degradation for post-session analysis
```

A development session should never be BLOCKED by an enforcement system failure. If checkpoint hooks fail, the session degrades to the existing hook-based enforcement. If hooks fail entirely, the session degrades to instruction-based enforcement (advisory mode).

## Implementation scope

### Files to create

| File | Purpose |
|------|---------|
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-phase-checkpoint.sh` | PostToolUse hook: intercepts TaskCreate, registers PF tasks in checkpoint file |
| `.claude/hooks/codeflow/task-completed/cf-task-completed-phase-checkpoint.sh` | TaskCompleted hook: marks tasks complete, creates phase sentinels when all done |

### Files to modify

| File | Change |
|------|--------|
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh` | Remove phase sentinel triggers (pf-1, pf-2, pf-3, pf-6). Keep stage sentinel triggers. |
| `.codeflow/scripts/state/cf-pathflow-state.sh` | Add checkpoint read/write helper functions |
| `.claude/settings.json` | Register new PostToolUse hook entry + new TaskCompleted hook entry |
| `.claude/CLAUDE.md` | Update Section 7 (Enforcement) to document checkpoint enforcement |
| `.codeflow/config/pathflow/pathflow-config.json` | Add required_tasks per phase; fix PF1-TSK-02, PF4-TSK-05 condition, max_qa_retries |

### Tests to create

| Test area | Coverage |
|-----------|----------|
| Checkpoint initialization | Verify checkpoint file created with correct phase/task structure from config |
| Task registration | Verify PostToolUse hook registers PF tasks, ignores non-PF tasks |
| Task completion tracking | Verify TaskCompleted hook marks tasks complete, ignores non-PF tasks |
| Sentinel creation | Verify sentinel created only when ALL phase tasks are complete |
| Conditional tasks | Verify skipped tasks do not block sentinel creation |
| Dependency ordering | Verify blockedBy constraints are enforced on completion |
| Graceful degradation | Verify missing/corrupted checkpoint blocks sentinel creation |
| Session scoping | Verify checkpoint isolation between concurrent sessions |

## Related

- `pathflow-config.json` at `.codeflow/config/pathflow/pathflow-config.json` -- phase and task definitions
- CLAUDE.md Section 7 (Enforcement & Operations) -- existing sentinel and gate enforcement
- CLAUDE.md Section 4 (Session Lifecycle) -- PathFlow phases and task tracker mirroring
- `cf-pre-tool-use-pathflow-gate.sh` -- existing gate hook (unchanged by this design)
- `cf-post-tool-use-pathflow-sentinel.sh` -- existing sentinel hook (modified to remove phase triggers)
- Design analysis: `inf-epc-008-design-analysis.md` -- prior PathFlow enforcement work
