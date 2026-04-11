---
id: BRIEF-006
title: "PathFlow Lifecycle Reference"
status: approved
author: cf-documentation
created: "2026-02-24"
---

# PathFlow lifecycle reference

## Table of contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Artifacts](#artifacts)
- [pathflow-team.json schema](#pathflow-teamjson-schema)
- [Checkpoint system](#checkpoint-system)
- [Sentinel system](#sentinel-system)
- [Cleanup layers](#cleanup-layers)
- [SessionStart lifecycle](#sessionstart-lifecycle)
- [SessionEnd lifecycle](#sessionend-lifecycle)
- [Scenarios](#scenarios)
- [Hook interaction sequence](#hook-interaction-sequence)
- [Edge cases](#edge-cases)
- [Graceful degradation](#graceful-degradation)
- [Related files](#related-files)

---

## Overview

PathFlow is CodeFlow's session lifecycle management system. It enforces a seven-phase
workflow (PF1-INIT through PF7-END) for every tracked development session. The system
uses two complementary architectures working together:

- **Checkpoint system**: task-completion-driven phase sentinel creation (ensures all
  required tasks complete before a phase advances)
- **Cleanup system**: three-layer artifact management (ensures session-scoped state is
  properly removed at the right time)

**Key artifacts produced by a PathFlow session:**

| Artifact | Location | Purpose |
|----------|----------|---------|
| Env file | `.state/runtime/codeflow-env.sh` | Shared session ID across all processes |
| PathFlow flag | `.state/session/{SID}/pathflow/is-pathflow-active` | Guards against premature cleanup |
| Checkpoint file | `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` | Tracks phase task registration and completion |
| Team file | `.state/session/{SID}/pathflow/pathflow-team.json` | Lead PID for teammate detection |
| Phase sentinels | `.state/sentinels/pathflow/{SID}/pathflow-pf-{N}` | Gate enforcement per phase |
| Stage sentinels | `.state/sentinels/pathflow/{SID}/pathflow-ws-{stage}` | Gate enforcement per work stage |
| Team config | `~/.claude/teams/{team_name}/` | Teammate metadata (tmux pane IDs, etc.) |
| Task list | `~/.claude/tasks/{team_name}/` | Claude Code's internal task tracker state |

---

## Architecture

Two models work together to form the complete PathFlow lifecycle:

### Three-layer cleanup model

Controls when session-scoped state is removed:

```text
Layer 1: PreToolUse team-guard
  Purpose:       Gate only — checks pf-6 sentinel before TeamDelete
  Action:        Allow (exit 0) or block (exit 2)
  Side effects:  NONE — does not remove any state
                 |
                 v (TeamDelete executes)

Layer 2: PostToolUse on TeamDelete
  Purpose:       Remove pathflow-active flag after TeamDelete succeeds
  Action:        rm -f .state/session/{SID}/pathflow/is-pathflow-active
  Fires:         Only on successful TeamDelete (PostToolUse timing)
                 |
                 v (Session ends)

Layer 3: SessionEnd
  Purpose:       Comprehensive cleanup of all remaining session-scoped state
  Action:        Removes sentinels, session dir, env file, team config (backstop)
  Guard:         Skips if pathflow-active flag exists (teammate shutdown)
```

### Three-layer checkpoint model

Controls when phase sentinels are created:

```text
Layer 1: PostToolUse on TaskCreate (Registration)
  Purpose:       Track required phase tasks; block cross-phase registration
  Hook:          cf-post-tool-use-phase-checkpoint.sh
  Fires:         Inside agentic loop, after TaskCreate
  Trust level:   Medium (LLM-initiated, observable)
                 |
                 v (task completed)

Layer 2: TaskCompleted lifecycle hook (Completion + Sentinel creation)
  Purpose:       Tamper-resistant completion verification; creates phase sentinel
  Hook:          cf-task-completed-phase-checkpoint.sh
  Fires:         Outside agentic loop (CLI lifecycle event)
  Trust level:   High (harder to fake)
                 |
                 v (sentinel exists)

Layer 3: PreToolUse pathflow gate (Enforcement)
  Purpose:       Hard enforcement at tool-call level
  Hook:          cf-pre-tool-use-pathflow-gate.sh
  Fires:         Before Edit, Write, Bash(git), Task (role teammates)
  Trust level:   High (blocks before execution)
```

### How the two models interact

```text
SessionStart
    |
    v
Checkpoint pre-initialized (all phases, all expected tasks)
PathFlow flag created
    |
    v
Lead creates phase tasks (TaskCreate)
    |
    v
Layer 1 checkpoint hook registers tasks in checkpoint file
    |
    v
Lead completes phase tasks (TaskUpdate status=completed)
    |
    v
Layer 2 checkpoint hook marks tasks complete
When ALL tasks in phase done/skipped --> creates phase sentinel
    |
    v
Layer 3 gate hook checks phase sentinel before gated operations
    |
    v
... phases PF1-PF6 complete ...
    |
    v
PF7-END: lead runs TeamDelete
    |
    v
Layer 1 team-guard: checks pf-6 sentinel -> allows TeamDelete
TeamDelete executes: removes team config, task list
    |
    v
Layer 2 cleanup hook: removes pathflow-active flag
    |
    v
SessionEnd
    |
    v
Guard: flag absent -> full cleanup (sentinels, session dir, env file)
```

---

## Artifacts

Complete inventory of all session-scoped artifacts with creation and cleanup responsibility:

| Artifact | Path | Created by | Cleaned by (primary) | Cleaned by (backstop) |
|----------|------|-----------|---------------------|----------------------|
| Env file | `.state/runtime/codeflow-env.sh` | SessionStart | SessionEnd (Layer 3) | PID-based cleanup at next SessionStart |
| PathFlow flag | `.state/session/{SID}/pathflow/is-pathflow-active` | SessionStart | PostToolUse on TeamDelete (Layer 2) | SessionEnd via session dir rm |
| Checkpoint file | `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` | SessionStart (`checkpoint_init_all_phases`) | SessionEnd via session dir rm | PID-based cleanup at next SessionStart |
| Team file | `.state/session/{SID}/pathflow/pathflow-team.json` | PostToolUse on TeamCreate | SessionEnd via session dir rm | PID-based cleanup at next SessionStart |
| Session directory | `.state/session/{SID}/` | SessionStart (dir creation) | SessionEnd (Layer 3) | PID-based cleanup at next SessionStart |
| Phase sentinels | `.state/sentinels/pathflow/{SID}/pathflow-pf-{N}` | Layer 2 checkpoint hook (TaskCompleted) | SessionEnd (Layer 3) | PID-based cleanup at next SessionStart |
| Stage sentinels | `.state/sentinels/pathflow/{SID}/pathflow-ws-{stage}` | PostToolUse sentinel hook (STAGE-COMPLETE pattern) | SessionEnd (Layer 3) | PID-based cleanup at next SessionStart |
| Skill sentinels | `.state/sentinels/skill/{SID}/` | Various skills | SessionEnd (Layer 3) | PID-based cleanup at next SessionStart |
| Active task | `.state/runtime/active-task.json` | cf-knowledge-layer | SessionEnd (conditional — preserve if in_progress) | PID-based cleanup at next SessionStart |
| Session ID ref | `.state/runtime/current-session-id` | SessionStart | SessionEnd (via env file rm) | PID-based cleanup at next SessionStart |
| Temp files | `/tmp/claude/sessions/{SID}/` | Various | SessionEnd (Layer 3) | OS temp cleanup |
| Team config | `~/.claude/teams/{team_name}/` | TeamCreate | TeamDelete | SessionEnd (backstop), PID-based cleanup |
| Task list | `~/.claude/tasks/{team_name}/` | TeamCreate | TeamDelete | SessionEnd (backstop), PID-based cleanup |

### Cleanup responsibility matrix

| Artifact | PreToolUse team-guard | PostToolUse TeamDelete | SessionEnd |
|----------|----------------------|------------------------|------------|
| pathflow-active flag | (gate only, no action) | **Primary removal** | Backstop via session dir rm |
| Phase sentinels | -- | -- | **Removes** |
| Stage sentinels | -- | -- | **Removes** |
| Skill sentinels | -- | -- | **Removes** |
| Session directory | -- | -- | **Removes** |
| Env file | -- | -- | **Removes** |
| Active task | -- | -- | Conditional preserve if in_progress |
| Temp files | -- | -- | **Removes** |
| Team config | -- | **Primary** (via TeamDelete) | Backstop if still exists |
| Task list | -- | **Primary** (via TeamDelete) | Backstop if still exists |

---

## pathflow-team.json schema

**Location:** `.state/session/{SID}/pathflow/pathflow-team.json`

**Created by:** PostToolUse on TeamCreate (`cf-post-tool-use-pathflow-sentinel.sh`)

**Purpose:** Records the lead's OS process PID so teammates can distinguish themselves from stale sessions at SessionStart.

```json
{
  "team_name": "string",
  "lead_claude_uuid": "string",
  "lead_pid": 12345,
  "codeflow_session_id": "ses-...",
  "teammate_spawned": false,
  "created_at": "2026-02-24T10:00:00Z",
  "last_spawn_name": null
}
```

| Field | Type | Set by | Purpose |
|-------|------|--------|---------|
| `team_name` | string | PostToolUse on TeamCreate | Team identifier for config and task list cleanup |
| `lead_claude_uuid` | string | PostToolUse on TeamCreate | Claude Code's internal session UUID for the lead |
| `lead_pid` | integer | PostToolUse on TeamCreate | OS PID of the lead Claude Code process (`$PPID`) |
| `codeflow_session_id` | string | PostToolUse on TeamCreate | CodeFlow session ID from env file |
| `teammate_spawned` | boolean | PostToolUse on Task | Whether any teammate has been spawned |
| `created_at` | string | PostToolUse on TeamCreate | ISO 8601 timestamp |
| `last_spawn_name` | string/null | PostToolUse on Task | Name of most recently spawned teammate |

**Why it lives in the pathflow directory:**

The file lives under `.state/session/{SID}/pathflow/` because it is session-scoped,
PathFlow-specific, and co-located with the flag and checkpoint files. When the session
directory is removed, all three files (`is-pathflow-active`, `pathflow-phase-tasks.json`,
`pathflow-team.json`) are removed atomically.

**PID update on compaction:** When Claude Code auto-compacts (`source=compact`), the lead
process PID changes. The SessionStart hook updates `lead_pid` in this file via atomic
write (`tmp + mv`) so teammates do not see a stale dead PID and trigger false cleanup.

---

## Checkpoint system

### Purpose

The checkpoint system ensures phase sentinels are created only when ALL required tasks
within that phase are verified complete. This prevents the team lead from skipping
individual PF{N}-TSK-{NN} tasks and still advancing to the next phase.

### Checkpoint file format

**Location:** `.state/session/{SID}/pathflow/pathflow-phase-tasks.json`

**Pre-initialized at session start** by `checkpoint_init_all_phases()` called from the
SessionStart hook. All phases (PF1-PF7) are created with their expected tasks from
`pathflow-config.json` before any task registration hooks fire.

```json
{
  "context": {
    "origin": "informal",
    "work_type": "DOCS"
  },
  "PF1": {
    "expected": ["PF1-TSK-01", "PF1-TSK-02"],
    "registered": {
      "PF1-TSK-01": "2026-02-24T10:00:00Z",
      "PF1-TSK-02": "2026-02-24T10:00:01Z"
    },
    "completed": {
      "PF1-TSK-01": "2026-02-24T10:00:30Z",
      "PF1-TSK-02": "2026-02-24T10:00:45Z"
    },
    "skipped": {},
    "conditions": {},
    "sentinel_created": true
  }
}
```

**Context keys:**

| Key | Set during | Values | Used by |
|-----|-----------|--------|---------|
| `origin` | PF2-CONTEXT tracking decision | `planned`, `informal`, `auto` | `adhoc_only` condition |
| `work_type` | PF3-CLASSIFY work classification | `FEAT`, `FIX`, `DOCS`, etc. | `if_pipeline_includes_qa` condition |

### Task registration (Layer 1)

**Hook:** `cf-post-tool-use-phase-checkpoint.sh`

**Fires:** After every TaskCreate call inside the agentic loop.

**Behavior:**

1. Parse `tool_input.subject` for the `PF{N}-TSK-{NN}` pattern
2. If no match, exit silently (not a PathFlow task)
3. Check if previous phase sentinel exists (`pathflow-pf-{N-1}`)
4. If previous phase sentinel missing: exit 2 (BLOCKED — cross-phase dependency violated)
5. If previous phase sentinel present (or N=1): write registration timestamp to checkpoint file

**What this catches:** Lead attempting to register next-phase tasks before completing the
current phase. Without registration, the task never appears in the checkpoint and the
phase sentinel is never created.

### Task completion (Layer 2)

**Hook:** `cf-task-completed-phase-checkpoint.sh`

**Fires:** When Claude Code marks a task complete (outside the agentic loop — CLI
lifecycle event, harder to fake than PostToolUse).

**Behavior:**

1. Parse `task_subject` for the `PF{N}-TSK-{NN}` pattern
2. If no match, exit silently
3. Read the session-scoped checkpoint file
4. Mark the task completed with timestamp
5. Auto-evaluate conditional tasks against stored context
6. Check if ALL required tasks in the phase are now completed or skipped
7. If all done: create the phase sentinel file, set `sentinel_created: true`

**What this catches:** Lead marking a task complete without actually finishing the work.
Since this fires at the CLI lifecycle level (outside the agentic loop), it provides
higher-trust verification than PostToolUse.

### Conditional task handling

Some tasks are conditional based on session context. The checkpoint system auto-evaluates
conditions so the lead does not need to manually skip non-applicable tasks.

| Condition | Applies when | Auto-skipped when |
|-----------|-------------|-------------------|
| `null` | Always required | Never |
| `adhoc_only` | `origin != "planned"` | `origin == "planned"` |
| `if_pipeline_includes_qa` | Pipeline includes WS-QA | Work type is DOCS, PLAN, or SPKE |

**Fail-safe:** When the context key needed for evaluation is not yet set, the condition
is NOT evaluable and the task remains required. This prevents premature phase completion
before context is established.

**Two mechanisms for handling conditional tasks:**

| Mechanism | When used | How |
|-----------|-----------|-----|
| Auto-evaluation | During `checkpoint_is_phase_complete()` | Conditions evaluated against stored context; matching tasks treated as done |
| Explicit skip | Via `checkpoint_skip_task()` | Lead or hook explicitly marks task as skipped with timestamp; triggers phase completion check |

### Ordering enforcement

**Cross-phase (hook-enforced):** Layer 1 blocks registration of PF{N} tasks until the
PF{N-1} sentinel exists. All PF{N-1} tasks must be registered and completed first.

**Within-phase (dependency-based):** Tasks declare `blockedBy` relationships in
`pathflow-config.json`. The Layer 2 hook checks dependencies before accepting a
completion. Parallel teammate work does not require strict sequential ordering.

**Why strict sequential completion is not enforced within a phase:** Teammates may
complete tasks in either order depending on timing. Dependency-based ordering handles
genuine dependencies while allowing independent tasks to complete freely.

### Enforcement properties

| Scenario | Behavior |
|----------|----------|
| Lead skips TaskCreate for a phase task | Task never registered. Sentinel never created. Next phase blocked. |
| Lead skips TaskUpdate(completed) | Task stays pending. Sentinel blocked. |
| Lead advances without completing all tasks | PreToolUse gate blocks (no sentinel). |
| Lead registers next-phase tasks before current phase done | BLOCKED (exit 2, Layer 1). |
| New task added to pathflow-config.json | Checkpoint hook reads config dynamically. New task included automatically. |
| Conditional task does not apply | Marked skipped, excluded from completion check. |
| Checkpoint file missing | Sentinel not created. Operations blocked (fails closed). |
| Checkpoint file corrupted | Sentinel not created. Operations blocked (fails closed). |
| Session is untracked | Checkpoint not initialized. Hooks exit early. No enforcement (correct). |

---

## Sentinel system

### Phase sentinels vs stage sentinels

| Type | Examples | Created by | Consumed by |
|------|---------|-----------|-------------|
| Phase sentinels | `pathflow-pf-1` through `pathflow-pf-7` | Layer 2 checkpoint hook (TaskCompleted) | PreToolUse pathflow gate |
| Stage sentinels | `pathflow-ws-dev`, `pathflow-ws-rev`, `pathflow-ws-qa` | PostToolUse sentinel hook (STAGE-COMPLETE pattern) | PreToolUse pathflow gate |

### Phase sentinel creation flow

```text
All PF{N}-TSK-{NN} tasks created (Layer 1 registers them)
    |
    v (each task completed via TaskUpdate)
TaskCompleted hook fires for each task (Layer 2)
    |
    v
Layer 2 checks: are ALL expected tasks in PF{N} completed or skipped?
    |
    +-- NO: update checkpoint, exit (sentinel not created yet)
    |
    +-- YES: touch .state/sentinels/pathflow/{SID}/pathflow-pf-{N}
             set sentinel_created: true in checkpoint
```

### Stage sentinel creation flow

```text
Stage teammate sends completion message:
    SendMessage("STAGE-COMPLETE: WS-DEV")
    |
    v
PostToolUse sentinel hook fires (cf-post-tool-use-pathflow-sentinel.sh)
Pattern-matches content for "STAGE-COMPLETE: WS-{stage}"
(content normalized to uppercase before matching)
    |
    v
Creates .state/sentinels/pathflow/{SID}/pathflow-ws-{stage}
```

### Gate enforcement

The PreToolUse pathflow gate (`cf-pre-tool-use-pathflow-gate.sh`) checks sentinel
existence before allowing gated operations:

| Sentinel required | Operations blocked without it |
|------------------|-------------------------------|
| `pf-3` | Edit, Write, `git commit`, role teammate spawns |
| `pf-5` AND `ws-rev` | `git push`, `gh pr` commands (dual gate: both must exist) |

The git push/PR gate is a **dual sentinel check**: both `pf-5` (PF5-VERIFY complete) and
`ws-rev` (WS-REV stage complete) must exist before `git push` or `gh pr` are allowed.
Either sentinel alone is insufficient. `pf-5` transitively requires `pf-4` (PF4-EXECUTE
must be complete before PF5-VERIFY can begin), so this gate effectively enforces the full
execute-then-verify sequence before any push.

**Stage sentinel ordering:** Stage sentinels enforce ordering within PF4-EXECUTE.
The `ws-rev` sentinel is required (alongside `pf-5`) to gate `git push`/`gh pr`, which
means `ws-rev` must be created (via `STAGE-COMPLETE: WS-REV` message) before any push
or PR. This hard-blocks shipping without a completed review. Similarly, `ws-dev` (or
`ws-docs`/`ws-plan`/`ws-test`) must exist before WS-REV is meaningful -- if a
stage-complete message is sent out of order, no artifact exists to review. The sentinel
hook does not enforce ordering between stage sentinels (it creates them on pattern-match
only), but the dual gate on git push/PR prevents shipping without review and without
PF5-VERIFY completing.

**Important:** Agents must NOT create sentinels manually. Sentinel creation is automated
by hooks. If a sentinel appears missing, investigate the hook pipeline or verify the
correct session ID path at `.state/sentinels/pathflow/{session-id}/`.

---

## Cleanup layers

### Layer 1: PreToolUse team-guard (gate only)

**Hook:** `cf-pre-tool-use-team-guard.sh`

**Purpose:** Gate enforcement before TeamDelete. No side effects.

**Behavior:**

1. If tool is not Teammate (cleanup operation) or TeamDelete: allow through
2. Check if PathFlow is active (flag exists)
3. If not active: allow through
4. If active AND tool is TeamDelete AND `pathflow-pf-6` sentinel exists: allow through (PF7-END)
5. Otherwise: block with exit 2

**Critical constraint:** This hook does NOT remove the pathflow-active flag. It is a
gate only. Flag removal is deferred to PostToolUse on TeamDelete (Layer 2).

### Layer 2: PostToolUse on TeamDelete (flag removal)

**Hook:** `cf-post-tool-use-pathflow-sentinel.sh` (handles multiple PostToolUse events)

**Purpose:** Remove pathflow-active flag after TeamDelete succeeds. Single action,
single responsibility.

**Behavior:**

1. Fires only after TeamDelete succeeds (PostToolUse timing guarantees this)
2. Removes `.state/session/{SID}/pathflow/is-pathflow-active`
3. If removal fails, flag stays — correct behavior (SessionEnd handles as backstop)

**Why this timing matters:** PreToolUse fires BEFORE execution. If the flag were removed
in PreToolUse and TeamDelete then failed, the flag would be gone with no team dissolved.
PostToolUse fires only on success — atomically coupling flag removal to TeamDelete success.

### Layer 3: SessionEnd (comprehensive cleanup)

**Hook:** `cf-session-end-cleanup.sh` (v2.4.0)

**Guard condition (v2.4.0):** When the pathflow-active flag exists, the hook performs a
PID-based liveness check before deciding whether to skip or proceed:

1. Read `pathflow-team.json` to get `lead_pid`
2. If `$PPID == lead_pid`: proceed with cleanup (this IS the lead's own SessionEnd --
   `$PPID` is the Claude Code process that invoked the hook, and it matches `lead_pid`,
   so this is the lead ending, not a teammate)
3. If team file exists and `lead_pid` is alive (`kill -0` succeeds): skip cleanup
   (this is a teammate shutdown -- the lead session is still active)
4. If team file exists and `lead_pid` is dead: proceed with cleanup
   (orphaned session -- lead crashed and SessionEnd fired late or via a stray process)
5. If team file does not exist: proceed with cleanup
   (pre-TeamCreate state or already cleaned)

This replaces the v2.2.0 behavior of unconditionally skipping when the flag exists.

**Cleanup sequence when flag is absent:**

1. Log pf-7 diagnostic (is pf-7 sentinel present or absent?)
2. Remove all PathFlow sentinels: `rm -rf .state/sentinels/pathflow/{SID}/`
3. Remove expired skill sentinels: `rm -rf .state/sentinels/skill/{SID}/`
4. Read `pathflow-team.json` to get `team_name`
5. Remove team config (backstop): `rm -rf ~/.claude/teams/{team_name}/`
6. Remove task list (backstop): `rm -rf ~/.claude/tasks/{team_name}/`
7. Remove session directory: `rm -rf .state/session/{SID}/` (covers flag, checkpoint, team file)
8. Remove env file: `rm -f .state/runtime/codeflow-env.sh`
9. Remove temp files: `rm -rf /tmp/claude/sessions/{SID}/`
10. Preserve active task if `in_progress`, otherwise clean

---

## SessionStart lifecycle

### Hook execution order

SessionStart hooks are split into two matcher entries in `settings.json` to enforce
sequential execution:

1. **Matcher 1** (runs first): `cf-working-protocol` skill load + `cf-session-start-init.sh`
2. **Matcher 2** (runs after matcher 1 completes): `cf-session-start-instructions.sh` + `cf-session-start-logging.sh`

Hooks within the same matcher run in **parallel**. Hooks in different matchers run
**sequentially**. This split ensures the init hook finishes writing `codeflow-env.sh`
before the logging and instructions hooks attempt to read it, preventing orphan
UUID-based session directories.

### Decision tree

```text
SessionStart hook fires
    |
    v
Read stdin: extract session_id (UUID), source field
_SESSION_SOURCE = "startup" | "compact" | "resume" | "clear" | "unknown"
    |
    v
=== SECTION 1b: PID-based stale session cleanup ===
Does env file exist at .state/runtime/codeflow-env.sh?
    |
    +-- NO --> Fresh start, no cleanup needed --> normal init
    |
    +-- YES --> Source env file, get old CODEFLOW_SESSION_ID (_old_sid)
               |
               Does .state/session/{_old_sid}/pathflow/pathflow-team.json exist?
               |
               +-- NO --> Check pathflow-active flag
               |          |
               |          +-- FLAG MISSING --> Orphan env file --> rm env file only
               |          +-- FLAG EXISTS --> Check source
               |                             |
               |                             +-- startup/unknown --> FULL CLEANUP
               |                             |    (stale pre-TeamCreate: session dir, sentinels, env)
               |                             +-- compact/resume/clear --> proceed normally
               |                                  (lead's own session continuing pre-TeamCreate)
               |
               +-- YES --> Read lead_pid from pathflow-team.json
                          |
                          kill -0 $lead_pid
                          |
                          +-- ALIVE --> Teammate starting
                          |            _TEAMMATE_MODE=true
                          |            Skip: session ID generation, flag creation, checkpoint init
                          |            Output: "TEAMMATE MODE" message
                          |
                          +-- DEAD --> Source: startup or unknown?
                                      |
                                      +-- YES --> Stale session: FULL CLEANUP
                                      |          a. rm -rf ~/.claude/teams/{team_name}/
                                      |          b. rm -rf ~/.claude/tasks/{team_name}/
                                      |          c. rm -rf .state/session/{_old_sid}/
                                      |          d. rm -rf .state/sentinels/pathflow/{_old_sid}/
                                      |          e. rm -f .state/runtime/codeflow-env.sh
                                      |          f. rm -f .state/runtime/active-task.json
                                      |          g. rm -f .state/runtime/current-session-id
                                      |          h. unset CODEFLOW_SESSION_ID
                                      |
                                      +-- NO --> Source: compact/resume/clear
                                                 Same session continuing (compaction)
                                                 Skip cleanup
                                                 Update lead_pid in pathflow-team.json (atomic tmp+mv)
    |
    v
=== SECTIONS 2-6: Setup and directory creation ===
Source env file, load libraries
Create required directories
Stale session detection (warning-only, for non-PID-covered cases)
Expired sentinel cleanup
Orphan sentinel sweep (Section 5b — removes sentinels from sessions with no matching session dir)
Active task context expiry check
    |
    v
=== SECTION 7: PathFlow flag creation ===
If not teammate mode AND not compact/resume/clear:
    Create .state/session/{SID}/pathflow/is-pathflow-active
    |
=== SECTION 7b: Sentinel recovery ===
If compact source: check for missing sentinels (context overflow recovery)
    |
=== SECTION 7c: Checkpoint pre-initialization ===
Call checkpoint_init_all_phases() from cf-pathflow-state.sh
Creates pathflow-phase-tasks.json with all phases and expected tasks
    |
=== SECTIONS 8-9: Metadata and stale team detection ===
Write session metadata
Scan ~/.claude/teams/ for stale team configs (warning-only)
```

### Source field handling

| Source | Env file? | Team file? | Lead PID | Action |
|--------|-----------|-----------|----------|--------|
| `startup` | No | N/A | N/A | Fresh start, normal init |
| `startup` | Yes | Yes | Alive | Teammate mode, minimal init |
| `startup` | Yes | Yes | Dead | Stale cleanup (PID-based), fresh start |
| `startup` | Yes | No | N/A (flag exists) | Full cleanup (orphaned pre-TeamCreate session) |
| `startup` | Yes | No | N/A (flag absent) | Clean env file only (orphan env file) |
| `compact` | Yes | Yes | Dead | Update lead_pid in team file, continue session |
| `compact` | Yes | No | N/A | Proceed normally (lead's own pre-TeamCreate session) |
| `resume` | Yes | Yes | Dead | Update lead_pid in team file, continue session |
| `clear` | Yes | Yes | Dead | Update lead_pid in team file, continue session |
| `unknown` | Yes | Yes | Dead | Stale cleanup (PID-based), fresh start |

### PID-based detection rationale

- `$PPID` in hooks equals the Claude Code process PID (direct parent of hook subprocess)
- Hooks run outside Claude Code's sandbox — `kill -0`, `ps`, `tmux` all work
- Each Claude Code process (lead vs teammate) has a distinct OS PID
- `kill -0` is signal 0 — checks process existence only, does not send any signal
- Exit code 0 means the process exists; non-zero means it does not

---

## SessionEnd lifecycle

### Complete flow

```text
SessionEnd hook fires (cf-session-end-cleanup.sh v2.4.0)
    |
    v
Source env file to get SESSION_ID
Source security-lib for is_pathflow_active()
    |
    v
=== PATHFLOW GUARD (v2.4.0) ===
is_pathflow_active()?
    |
    +-- FALSE --> Lead session ending, flag absent
    |             Proceed with cleanup
    |
    +-- TRUE --> Flag present: perform PID check
                Read pathflow-team.json for lead_pid
                    |
                    +-- Team file MISSING --> Proceed with cleanup
                    |    (pre-TeamCreate state or already cleaned)
                    |
                    +-- PPID == lead_pid --> Proceed with cleanup
                    |    (this IS the lead's own SessionEnd — self-reference)
                    |
                    +-- lead_pid ALIVE (kill -0 succeeds) --> Skip cleanup (exit 0)
                    |    (teammate shutdown — lead session still active)
                    |
                    +-- lead_pid DEAD --> Proceed with cleanup
                         (orphaned session — lead crashed)
    |
    v
=== PF7 DIAGNOSTIC ===
Does .state/sentinels/pathflow/{SID}/pathflow-pf-7 exist?
    +-- YES: "Clean PF7 shutdown"
    +-- NO:  "Incomplete PF7 shutdown (possible crash or skip)"
    |
    v
1. Remove PathFlow sentinels
   rm -rf .state/sentinels/pathflow/{SID}/
    |
    v
2. Remove skill sentinels
   rm -rf .state/sentinels/skill/{SID}/
    |
    v
3. Check active task
   If in_progress: preserve
   Else: clear
    |
    v
4. Read pathflow-team.json to get team_name
    |
    v
5. Backstop: remove team config/task list if still present
   rm -rf ~/.claude/teams/{team_name}/
   rm -rf ~/.claude/tasks/{team_name}/
    |
    v
6. Remove session directory
   rm -rf .state/session/{SID}/
    |
    v
7. Remove env file
   rm -f .state/runtime/codeflow-env.sh
    |
    v
8. Remove temp files
   rm -rf /tmp/claude/sessions/{SID}/
```

---

## Scenarios

Step-by-step hook interactions for ten scenarios covering nominal and failure cases.

### Scenario 1: Clean PF1→PF7 (happy path)

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Session start | SessionStart init | `source=startup`, no prior env file; generate SID, write env file, create pathflow flag, init checkpoint | env file, flag, checkpoint created |
| TeamCreate | PostToolUse sentinel | Write `pathflow-team.json` with `lead_pid=$PPID` | team file written |
| Teammate spawn | PostToolUse sentinel | Update `pathflow-team.json`: `teammate_spawned=true` | team file updated |
| Phase tasks (PF1) | PostToolUse checkpoint | Register PF1-TSK-{NN} tasks in checkpoint | tasks registered |
| Tasks complete (PF1) | TaskCompleted checkpoint | Mark tasks done; all PF1 tasks done → create `pathflow-pf-1` | pf-1 sentinel created |
| Stage completion | PostToolUse sentinel | Pattern-matches `STAGE-COMPLETE: WS-*` → create stage sentinel | stage sentinel created |
| TeamDelete | PreToolUse team-guard | Checks flag + pf-6 sentinel → allows through (exit 0) | gate passed |
| TeamDelete | PostToolUse sentinel | After TeamDelete succeeds → remove pathflow-active flag | flag removed |
| Session end | SessionEnd cleanup | Flag absent → `_PATHFLOW_ACTIVE=false` → full cleanup | all artifacts removed |
| (alt) Session end without TeamDelete | SessionEnd cleanup v2.4.0 | Flag present → `$PPID == lead_pid` → proceed with cleanup (lead's own SessionEnd) | all artifacts removed |

**Result: CLEAN**

### Scenario 2: Compaction mid-session

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Session start | SessionStart | `source=startup`, no prior env → normal init, `pathflow-team.json` written with `lead_pid=PID_A` | session established |
| COMPACTION | Claude Code | Old process killed, new process starts; `source=compact` | PID changes to PID_B |
| Session start (post-compact) | SessionStart | `source=compact`, reads env file, reads `pathflow-team.json`: PID_A dead but source=compact → skip stale cleanup, update `lead_pid=PID_B` atomically | team file updated |
| Teammate spawned | Teammate SessionStart | Reads updated `pathflow-team.json`: PID_B alive → teammate mode | correct |
| PF7-END | Normal cleanup | Full cleanup proceeds | CLEAN |

**Result: CLEAN** (atomic PID update in team file prevents false stale detection)

### Scenario 3: Multiple compactions

| Step | Action | State |
|------|--------|-------|
| Compaction 1 | `source=compact`, PID_A dead → update to PID_B | chain: PID_A → PID_B |
| Compaction 2 | `source=compact`, PID_B dead → update to PID_C | chain: PID_B → PID_C |
| Teammate start (any point) | Reads latest `pathflow-team.json`, PID_C alive → teammate mode | correct |
| PF7-END | Normal cleanup | CLEAN |

Each compaction atomically updates `lead_pid` via tmp+mv. The chain is linear — each
write overwrites the previous.

**Result: CLEAN**

### Scenario 4: Lead crash (no clean shutdown)

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Lead crashes | (none) | No SessionEnd, no TeamDelete fires | all artifacts remain on disk |
| Next session start | SessionStart | `source=startup`, reads env file, finds `pathflow-team.json`, old lead PID dead → stale cleanup | removes: team config, task list, session dir (flag, checkpoint, team file), sentinels, env file, active task, session ID ref |

**Result: CLEAN** (deferred cleanup at next startup)

### Scenario 5: Teammate starts during active session

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Teammate session start | SessionStart | `source=startup`, reads env file, reads `pathflow-team.json`, `lead_pid` alive → teammate mode | correct |
| Skipped | SessionStart | Session ID generation, flag creation, checkpoint init | all skipped (lead handles these) |

Detection logic: `kill -0 "$_old_lead_pid"` at `cf-session-start-init.sh` Section 1b.

**Result: CORRECT**

### Scenario 6: Teammate starts after compaction

| Step | Action | Result |
|------|--------|--------|
| Compaction updated `pathflow-team.json` | `lead_pid=PID_B` | team file current |
| Teammate SessionStart | Reads `lead_pid=PID_B`, `kill -0 PID_B` succeeds → teammate mode | correct |

**Result: CORRECT** (PID update in Scenario 2 prevents false stale detection here)

### Scenario 7: SessionEnd without TeamDelete

Two sub-cases depending on who triggers SessionEnd:

#### 7a: Lead session exits without PF7 (user closes terminal)

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Lead session exits without PF7 | (none) | No TeamDelete, no PostToolUse flag removal | pathflow-active flag still exists |
| SessionEnd fires for lead | SessionEnd v2.3.0 | Flag present → PID check: lead_pid dead (lead just exited) → proceed with cleanup | full cleanup runs |

**Result: CLEAN** (v2.3.0 guard: dead lead PID with flag present → cleanup proceeds, not deferred)

#### 7b: Teammate session ends while lead is alive

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Teammate shuts down | (none) | Teammate session ends; lead session still active | flag still present |
| SessionEnd fires for teammate | SessionEnd v2.3.0 | Flag present → PID check: lead_pid alive (lead still running) → skip cleanup (exit 0) | lead state preserved |

**Result: CORRECT** (v2.3.0 guard: alive lead PID with flag present → skip, preserving lead's shared session state)

### Scenario 8: Pre-TeamCreate crash

Two sub-scenarios depending on the source field at next startup:

#### 8a: source=startup (new session after crash)

| Step | Action | Result |
|------|--------|--------|
| Session start completes | env file written, flag created, checkpoint initialized | normal session state |
| Lead crashes before TeamCreate | No `pathflow-team.json` written yet | env file exists, flag exists, no team file |
| Next startup (SessionStart) | `source=startup`; reads env file, old SID found; checks for `pathflow-team.json` → MISSING | falls through to pathflow-active flag check |
| Flag check | Flag exists AND no team file → full stale cleanup: session dir, sentinels, env file, active task | all stale artifacts removed |

**Result: CLEANED** (v1.6.0 Fix 1: orphan env file with flag but no team file → full cleanup on startup)

#### 8b: source=compact/resume/clear (continuation after compaction)

| Step | Action | Result |
|------|--------|--------|
| Session start completes | env file written, flag created, checkpoint initialized | normal session state |
| Compaction occurs before TeamCreate | No `pathflow-team.json` written yet; `source=compact` at next SessionStart | flag and env file still valid |
| SessionStart (post-compact) | `source=compact`; reads env file, checks team file → MISSING; source is not startup → proceed normally | lead's own pre-TeamCreate session continues |

**Result: HANDLED** (non-startup source with missing team file is treated as lead's own session continuing normally)

### Scenario 9: Corrupted env file

| Step | Action | Result |
|------|--------|--------|
| Env file exists but is corrupted | `source "$_env_file_pre"` returns non-zero or `$CODEFLOW_SESSION_ID` is empty | `_old_sid` is empty |
| Decision tree | `_old_sid` is empty → skip all stale detection | falls through to normal init |
| SessionStart continues | Generates new SID, writes fresh env file | fresh session established |

**Result: HANDLED** (empty SID skips stale detection; fresh session starts normally)

### Scenario 10: Orphaned sentinels

| Step | Action | Result |
|------|--------|--------|
| Crash leaves sentinels in `.state/sentinels/pathflow/{old_SID}/` | No SessionEnd ran; sentinels not cleaned | orphaned sentinel directory |
| Next startup | Stale PID cleanup runs (Scenario 4 pattern) | `rm -rf .state/sentinels/pathflow/{old_SID}/` included in cleanup |
| New session | Generates new SID; new sentinel directory created fresh | no contamination from old sentinels |

**Result: CLEANED** (PID-based cleanup at startup removes orphaned sentinels atomically
with the rest of the stale session)

---

## Hook interaction sequence

Complete sequence from session start to session end showing all hooks and their ordering:

```text
SESSION START
    |
    v
SessionStart: cf-session-start-init.sh
  - Read stdin (source field, claude UUID)
  - PID-based stale cleanup decision
  - Setup, directories
  - PathFlow flag creation
  - Checkpoint pre-initialization (checkpoint_init_all_phases)
    |
    v (TeamCreate)
PostToolUse: cf-post-tool-use-pathflow-sentinel.sh
  - Write pathflow-team.json with lead_pid=$PPID

    |
    v (each TaskCreate with PF{N}-TSK-{NN})
PostToolUse: cf-post-tool-use-phase-checkpoint.sh
  - Parse subject for PF{N}-TSK-{NN}
  - Check prev phase sentinel (cross-phase gate)
  - Register task in checkpoint file

    |
    v (each TaskUpdate status=completed)
TaskCompleted: cf-task-completed-phase-checkpoint.sh
  - Parse task_subject for PF{N}-TSK-{NN}
  - Mark task complete in checkpoint
  - Auto-evaluate conditional tasks
  - If all phase tasks done → touch pathflow-pf-{N} sentinel

    |
    v (before Edit/Write/git commit/role teammate spawn)
PreToolUse: cf-pre-tool-use-pathflow-gate.sh
  - Check pf-3 sentinel for Edit/Write/git commit
  - Check pf-5 AND ws-rev sentinels for git push/gh pr (dual gate, both required)
  - Allow or block

    |
    v (SendMessage with STAGE-COMPLETE: WS-{stage})
PostToolUse: cf-post-tool-use-pathflow-sentinel.sh
  - Normalize content to uppercase
  - Pattern-match STAGE-COMPLETE: WS-{stage}
  - Touch pathflow-ws-{stage} sentinel

    |
    v (Task tool for teammate spawn)
PostToolUse: cf-post-tool-use-pathflow-sentinel.sh
  - Update pathflow-team.json: teammate_spawned=true, last_spawn_name

    |
    v ... phases PF1-PF6 complete ...

    |
    v (TeamDelete)
PreToolUse: cf-pre-tool-use-team-guard.sh
  - Check pathflow-active flag
  - Check pf-6 sentinel
  - Allow TeamDelete through (PF7-END gate passed)

    |
    v (TeamDelete executes: removes team config and task list)

    |
    v
PostToolUse: cf-post-tool-use-pathflow-sentinel.sh
  - Detect TeamDelete tool name
  - Remove pathflow-active flag

SESSION END
    |
    v
SessionEnd: cf-session-end-cleanup.sh (v2.4.0)
  - Guard: check pathflow-active flag
    FLAG ABSENT:  lead session end → proceed
    FLAG PRESENT: read pathflow-team.json for lead_pid
      PPID == lead_pid:    lead's own SessionEnd → proceed
      lead_pid alive:      teammate shutdown → exit 0 (skip)
      lead_pid dead:       orphaned session → proceed
      no team file:        proceed with cleanup
  - PF7 diagnostic log
  - Remove PathFlow sentinels
  - Remove skill sentinels
  - Preserve or clear active task
  - Read team_name from pathflow-team.json
  - Backstop: remove team config/task list
  - Remove session directory (covers flag, checkpoint, team file)
  - Remove env file
  - Remove temp files
```

---

## Edge cases

| Edge case | Likelihood | Handling |
|-----------|-----------|---------|
| PID recycling (OS reuses dead lead PID for unrelated process) | Very low (PID space is large, recycling is sequential) | Acceptable risk. Window between lead death and new SessionStart is typically seconds. `kill -0` on unrelated process causes false teammate mode — session proceeds normally rather than being cleaned up. |
| Compaction and teammate spawn race | Negligible | Claude Code executes sequentially — teammate cannot spawn during compaction. |
| In-process teammates (non-tmux) | Common | Correct: same PID as lead, `kill -0` succeeds, treated as teammate. No cleanup. |
| Lead's own SessionEnd with flag present | Common (PF7 without TeamDelete) | v2.4.0: `$PPID == lead_pid` check fires first, proceeds with cleanup. Without this check, `kill -0` on own PID always succeeds, falsely treating the lead's SessionEnd as a teammate shutdown. |
| Crash within seconds of TeamCreate | Rare | `pathflow-team.json` may not exist yet. Falls to "no team file" path, checks flag instead. |
| Team config missing at cleanup time | Possible (manually deleted) | Cleanup skips gracefully, continues with other artifacts. |
| Multiple stale sessions | Possible | Only the session referenced by the env file is cleaned. Other stale sessions remain until their env file is referenced. |
| Concurrent teammate spawns | Possible | `pathflow-team.json` writes use atomic write (tmp + mv) to prevent races. |
| Pre-TeamCreate crash | Low | Handled in Scenario 8: no team file → flag check → proceed normally. |
| Corrupted env file | Low | Handled in Scenario 9: empty SID → skip stale detection → fresh start. |
| Orphaned sentinels | Crash scenario | Handled in Scenario 10: PID-based cleanup at startup removes them. |
| Concurrent sessions | Rare | Each session has a unique SID. Checkpoint and sentinel files are namespaced under SID. No collision possible. |
| Worktree isolation | Active | Worktrees use selective symlinks: `.state/runtime/`, `.state/session/`, `.state/sentinels/` are local per-worktree. Shared directories (db, ledger, logs) use symlinks to main repo. |

### Worktree selective symlink structure

```text
.state/                              (directory, not symlink)
├── db -> main/.state/db             (shared: single SurrealDB database)
├── ledger -> main/.state/ledger     (shared: single JSONL event log)
├── registry -> main/.state/registry (shared: registry data)
├── backups -> main/.state/backups   (shared: backups)
├── coordination -> main/.state/coordination  (shared: CRDT data)
├── logs -> main/.state/logs         (shared: log aggregation)
├── runtime/                         (local: env file, active task, session ID ref)
├── session/                         (local: PathFlow state, checkpoint, team file)
└── sentinels/                       (local: gate state)
```

---

## Graceful degradation

The enforcement system degrades gracefully when components fail. A session is never
BLOCKED by an enforcement system failure.

```text
Full enforcement (nominal)
  Instructions + Tasks + Hooks + Checkpoints all active
       |
       | (checkpoint hook fails to create sentinel)
       v
Partial enforcement — checkpoint degraded
  Instructions + Tasks + Hooks active
  Phase sentinels not created → gated operations blocked
  Session can continue for non-gated work only
       |
       | (hooks fail entirely)
       v
Advisory only
  Instructions + task graph still provide ordering
  Session proceeds without guard rails
  Log degradation for post-session analysis
```

**Checkpoint failure modes:**

| Failure mode | Impact | Recovery |
|-------------|--------|----------|
| Checkpoint file missing | Phase sentinels not created. Edit/Write blocked after PF3. | Investigate SessionStart hook. Restart session. |
| Checkpoint file corrupted | Same as missing. | Delete corrupted file, restart session. |
| `pathflow-config.json` missing | Checkpoint cannot initialize. | Restore config from git. |
| PostToolUse hook crashes | Registration not recorded. Sentinel not created. | Fix hook, restart session. |
| TaskCompleted hook crashes | Completion not recorded. Sentinel not created. | Fix hook, restart session. |

**Cleanup failure modes:**

| Failure mode | Impact | Recovery |
|-------------|--------|----------|
| PostToolUse on TeamDelete fails | Flag stays. SessionEnd guard skips cleanup. | PID-based cleanup at next startup. |
| SessionEnd skips cleanup (flag present) | Intentional — teammate shutdown. | Normal: lead session end runs full cleanup. |
| SessionEnd crashes mid-cleanup | Partial cleanup. Some artifacts may remain. | PID-based cleanup at next startup catches remaining. |

---

## Related files

All files involved in the PathFlow lifecycle system:

### Hook scripts

| File | Hook type | Purpose |
|------|-----------|---------|
| `.claude/hooks/codeflow/session-start/cf-session-start-init.sh` | SessionStart | Session initialization, PID-based stale cleanup, checkpoint pre-init |
| `.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh` | SessionEnd | Comprehensive artifact cleanup |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh` | PreToolUse | Phase and stage gate enforcement |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh` | PreToolUse | TeamDelete gate (Layer 1 of cleanup model) |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh` | PostToolUse | Stage sentinel creation, team file management, flag removal (Layer 2) |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-phase-checkpoint.sh` | PostToolUse | Phase task registration (Layer 1 of checkpoint model) |
| `.claude/hooks/codeflow/task-completed/cf-task-completed-phase-checkpoint.sh` | TaskCompleted | Phase task completion, phase sentinel creation (Layer 2 of checkpoint model) |

### State files

| File | Purpose |
|------|---------|
| `.state/runtime/codeflow-env.sh` | Shared session ID across all processes |
| `.state/runtime/active-task.json` | Current task context for hook use |
| `.state/runtime/current-session-id` | Current session ID reference |
| `.state/session/{SID}/pathflow/is-pathflow-active` | PathFlow flag (teammate guard) |
| `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` | Checkpoint state file |
| `.state/session/{SID}/pathflow/pathflow-team.json` | Team lead PID and metadata |
| `.state/sentinels/pathflow/{SID}/pathflow-pf-{N}` | Phase gate sentinels |
| `.state/sentinels/pathflow/{SID}/pathflow-ws-{stage}` | Stage gate sentinels |
| `.state/logs/pathflow-events.jsonl` | Phase and stage transition log |

### Configuration

| File | Purpose |
|------|---------|
| `.codeflow/config/pathflow/pathflow-config.json` | Phase and task definitions, expected tasks, conditions |
| `.codeflow/config/enforcement/enforcement-policy.json` | Merge protection, sentinel config |
| `.claude/settings.json` | Hook registration (event, matcher, command for all hooks) |

### Libraries

| File | Purpose |
|------|---------|
| `.codeflow/scripts/state/cf-pathflow-state.sh` | Checkpoint functions: `checkpoint_init_all_phases`, `checkpoint_set_context`, `checkpoint_is_phase_complete`, `checkpoint_skip_task` |
| `.codeflow/scripts/security/lib/security-lib.sh` | Security library including `is_pathflow_active()` |
| `.codeflow/scripts/security/lib/context-lib.sh` | Context library for session state |

### Superseded documents

The following documents are superseded by this reference:

- `.codeflow/docs/analysis/pathflow-cleanup-fixes.md` — three-layer cleanup model (BRIEF-005)
- `.codeflow/docs/analysis/pid-based-teammate-detection.md` — PID-based stale detection (BRIEF-004)
- `.codeflow/docs/analysis/phase-checkpoint-enforcement.md` — checkpoint architecture (BRIEF-003)
- `.codeflow/docs/analysis/stale-session-cleanup-design.md` — stale session design (no BRIEF ID)
