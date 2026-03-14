---
title: "SessionEnd Cleanup Redesign: Status Tracker Approach"
type: analysis
status: proposed
date: 2026-03-13
area: infrastructure
scope:
  - codeflow-cli/internal/hooks/session/end.go
  - codeflow-cli/internal/hooks/session/start.go
  - codeflow-cli/internal/hooks/team/guard.go
  - codeflow-rs/codeflow-core/src/hooks/session_end.rs
  - codeflow-rs/codeflow-core/src/hooks/session_start.rs
  - codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs
---

# SessionEnd cleanup redesign: status tracker approach

This document records the analysis, root cause diagnosis, and proposed solution
for the recurring `cleanTeamArtifacts` bug in the SessionEnd hook. The bug
causes team config deletion mid-session when a sub-agent exits and the tmux
liveness check fails. The solution replaces the tmux-based `shouldSkipCleanup`
guard with a structured `pathflow-session-status.json` status tracker.

## Table of Contents

- [1. Problem statement](#1-problem-statement)
- [2. The 13-section SessionEnd cleanup flow](#2-the-13-section-sessionend-cleanup-flow)
- [2a. Unified SessionEnd flow diagram](#2a-unified-sessionend-flow-diagram)
- [3. Current broken implementation](#3-current-broken-implementation)
- [4. PF7 shutdown sequence](#4-pf7-shutdown-sequence)
- [5. Why option D (pf-7 sentinel) was rejected](#5-why-option-d-pf-7-sentinel-was-rejected)
- [6. Scenario analysis](#6-scenario-analysis)
  - [Scenario A: sub-agent exits mid-session (the bug)](#scenario-a-sub-agent-exits-mid-session-the-bug)
  - [Scenario B: normal PF7 shutdown (teammate exits before TeamDelete)](#scenario-b-normal-pf7-shutdown-teammate-exits-before-teamdelete)
  - [Scenario C: lead exits after TeamDelete](#scenario-c-lead-exits-after-teamdelete)
  - [Scenario D: crash / orphaned session](#scenario-d-crash--orphaned-session)
  - [Scenario E: non-PathFlow session](#scenario-e-non-pathflow-session)
  - [Scenario F: new PF flow after completed one](#scenario-f-new-pf-flow-after-completed-one)
  - [Scenario G: status stuck but PF7 done (edge case)](#scenario-g-status-stuck-but-pf7-done-edge-case)
- [7. The solution: pathflow-session-status.json](#7-the-solution-pathflow-session-statusjson)
  - [7.1 File structure](#71-file-structure)
  - [7.2 Status values and transitions](#72-status-values-and-transitions)
  - [7.3 File lifecycle](#73-file-lifecycle)
  - [7.4 Hook update timeline](#74-hook-update-timeline)
- [8. New shouldSkipCleanup decision logic](#8-new-shouldskipcleanup-decision-logic)
- [9. Decision matrix](#9-decision-matrix)
- [10. Files changed](#10-files-changed)
- [11. Comparison: old vs new](#11-comparison-old-vs-new)
- [12. Backwards compatibility](#12-backwards-compatibility)
- [13. Related files](#13-related-files)

---

## 1. Problem statement

The SessionEnd hook's `shouldSkipCleanup()` function uses tmux pane liveness to
decide whether to skip team cleanup. This is broken for in-process agents.

When a sub-agent exits, Claude Code fires `SessionEnd` for that sub-agent's
session. The `shouldSkipCleanup()` guard reads `is-pathflow-active`, then reads
`pathflow-team.json`, then reads `~/.claude/teams/{name}/config.json` to extract
tmux pane IDs, then calls `IsPaneAlive()` for each pane.

In-process agents do not use tmux panes. Their `tmuxPaneId` in `config.json` is
either absent or refers to a dead pane. The liveness check returns `false`.
`shouldSkipCleanup()` returns `false`. SessionEnd proceeds to run
`cleanTeamArtifacts()`, which calls `os.RemoveAll` on `~/.claude/teams/{name}/`
and `~/.claude/tasks/{name}/`. The team config is deleted mid-session.

With the team config gone, new teammates cannot be spawned. The session is
effectively broken. This bug has recurred multiple times across sessions and is
tracked in `.claude/memory/bug_team_config_deletion.md`.

The root cause is that **tmux pane liveness is not a reliable signal for
in-process agent liveness**. A different guard signal is required.

---

## 2. The 13-section SessionEnd cleanup flow

The `EndCleanup()` function in Go (`end.go`) and `run()` in Rust
(`session_end.rs`) both follow the same 13-section structure. Section 3 is the
critical gate that determines whether sections 9 and 10 run.

```text
EndCleanup() / run()
│
├─ Section 1:  Parse stdin JSON (session_id, transcript_path)
│
├─ Section 2:  Resolve session ID
│               └─ session.Current() → reads CODEFLOW_SESSION_ID env or codeflow-env.sh
│               └─ If empty or "unknown" → return early (no cleanup)
│
├─ Section 3:  PathFlow guard ← THE CRITICAL GATE
│               └─ shouldSkipCleanup() checks whether cleanup should run
│               └─ If true → return early (skip all destructive sections)
│
├─ Section 4:  PF7 diagnostic validation
│               └─ Check pathflow-pf-7 sentinel existence
│               └─ Log warnings if missing
│
├─ Section 5:  Clean PathFlow sentinels
│               └─ os.RemoveAll(.state/sentinels/pathflow/{SID}/)
│
├─ Section 6:  Handle active task preservation
│               └─ Preserve active-task.json if status == "in_progress"
│
├─ Section 7:  (reserved — no-op in current implementation)
│
├─ Section 8:  Read team name from pathflow-team.json
│               └─ Extracts team_name for sections 9 and 10
│
├─ Section 9:  cleanTeamArtifacts() ← THE DESTRUCTIVE ACTION
│               └─ os.RemoveAll(~/.claude/teams/{name}/)
│               └─ os.RemoveAll(~/.claude/tasks/{name}/)
│
├─ Section 10: cleanSessionState() ← ALSO DESTRUCTIVE
│               └─ os.RemoveAll(.state/session/{SID}/)
│               └─ Removes is-pathflow-active, pathflow-team.json, checkpoint, etc.
│
├─ Section 11: Clean runtime files
│               └─ session.CleanRuntimeFiles(.state/runtime/)
│
├─ Section 12: Clean project temp directory
│               └─ os.RemoveAll($TMPDIR/claude/{projectName}/)
│
└─ Section 13: Write session_end ledger event
                └─ Appends to .state/ledger/sessions.jsonl
```

The invariant is: **if Section 3 allows cleanup to proceed, Sections 9 and 10
will always run**. There is no per-section guard. The entire cleanup sequence is
a single critical section gated by `shouldSkipCleanup()`.

---

## 2a. Unified SessionEnd flow diagram

The following diagram shows `shouldSkipCleanup` behavior for the three caller
types (lead, tmux teammate, in-process sub-agent) using the new status-based
logic. Each caller type produces identical inputs to the function — only the
session status and team config presence differentiate them.

```text
SessionEnd fires (any agent: lead, tmux teammate, in-process sub-agent)
│
├─ Section 2: Resolve session ID
│   session.Current(runtimeDir)
│   → reads CODEFLOW_SESSION_ID env var, falls back to codeflow-env.sh
│   No SID found or SID == "unknown" → return early (no cleanup)
│
└─ Section 3: shouldSkipCleanup(sessionStateDir)
    │
    ├─ Read pathflow-session-status.json
    │   Parse error → ALLOW cleanup (proceed to Sections 4-13)
    │   File missing → check old is-pathflow-active flag
    │       Flag missing → ALLOW cleanup
    │       Flag exists  → shouldSkipCleanupLegacy() (team config existence)
    │
    ├─ status == "created"
    │   All callers: ALLOW cleanup
    │   Rationale: no team created yet, nothing to protect
    │
    ├─ status == "pf-complete"
    │   All callers: ALLOW cleanup
    │   Rationale: TeamDelete succeeded, session finished cleanly
    │
    ├─ status == "pf-started" or "pf-in-progress"
    │   ├─ team_name == "" → ALLOW cleanup
    │   ├─ config.json missing → ALLOW cleanup (team already dissolved)
    │   ├─ last_completed_phase == "PF7" → ALLOW cleanup (guard)
    │   └─ All checks pass → SKIP cleanup
    │       Lead: SKIP (should not happen — lead reaches pf-complete before exit)
    │       tmux teammate: SKIP (session active, lead still running)
    │       in-process sub-agent: SKIP (BUG FIXED: was ALLOW with old tmux check)
    │
    └─ unknown status → ALLOW cleanup

Caller-specific notes:

  Lead (EXIT C from SessionStart):
    After TeamDelete: status="pf-complete" → ALLOW
    After crash (status="pf-in-progress"): SKIP → stale sweep handles cleanup

  tmux teammate (EXIT B from SessionStart):
    status="pf-in-progress", config exists → SKIP (session active)
    Same behavior as before, but now status-driven instead of tmux-driven

  In-process sub-agent (EXIT B or EXIT A from SessionStart):
    status="pf-in-progress", config exists → SKIP (BUG FIX)
    Old behavior: all tmux panes dead → ALLOW (destructive, wrong)
    New behavior: status file consulted → SKIP (correct)
```

**Backward compatibility path** (`shouldSkipCleanupLegacy`):

Sessions created before `pathflow-session-status.json` was introduced have only
the old `is-pathflow-active` empty flag. When the new code finds the flag but
no status file, it calls `shouldSkipCleanupLegacy`, which checks whether
`~/.claude/teams/{team_name}/config.json` exists. This replaces the old tmux
check with a config existence check, which works for in-process agents.

---

## 3. Current broken implementation

The current `shouldSkipCleanup()` implementation follows this decision chain:

```text
shouldSkipCleanup(sessionStateDir)
│
├─ Check is-pathflow-active flag
│   └─ Does NOT exist → return false (allow cleanup — no PathFlow session)
│   └─ EXISTS → continue
│
├─ Read pathflow-team.json
│   └─ File not found → return false ("no team file — allow cleanup")
│   └─ Parse error → return false ("unreadable — allow cleanup")
│   └─ team_name is empty → return false ("no team name — allow cleanup")
│
├─ Read ~/.claude/teams/{team_name}/config.json
│   └─ File not found → return false ("config missing — allow cleanup")
│   └─ Parse error → return false ("config unreadable — allow cleanup")
│
├─ Extract members[].tmuxPaneId from config.json
│   └─ No members → fall through
│
└─ For each tmuxPaneId:
    └─ Call TmuxChecker.IsPaneAlive(paneId)
    └─ Any alive → return true (skip cleanup — session is active)
    └─ None alive → return false (allow cleanup)
```

**The bug is in the final step.** For in-process agents, `tmuxPaneId` is empty
or refers to a dead pane. `IsPaneAlive()` returns `false` for all members.
The function returns `false`, and cleanup runs destructively mid-session.

Every fallthrough path at every step of this chain also allows cleanup. The
design defaults to "run cleanup when uncertain," which is wrong. The safe
default should be "skip cleanup when uncertain."

---

## 4. PF7 shutdown sequence

Understanding the normal PF7-END sequence is essential context for why the pf-7
sentinel cannot be used as a guard signal (explained in [Section 5](#5-why-option-d-pf-7-sentinel-was-rejected)).

```text
PF7-END (normal session close)
│
├─ Lead sends shutdown_request to on-demand teammates
│   └─ cf-development, cf-review, cf-quality-assurance (any order)
│   └─ Each teammate exits → SessionEnd fires for each
│       └─ shouldSkipCleanup() must return true (session still active)
│
├─ Lead sends shutdown_request to persistent teammates
│   └─ cf-git-operations, cf-knowledge-layer, cf-security (any order)
│   └─ Each teammate exits → SessionEnd fires for each
│       └─ shouldSkipCleanup() must return true (session still active)
│
├─ Lead calls TaskUpdate(PF7-TSK-03, completed)
│   └─ TaskCompleted hook fires
│   └─ checkpoint-complete hook creates pathflow-pf-7 sentinel
│
├─ Lead calls TeamDelete
│   └─ PreToolUse team-guard allows (pf-6 sentinel exists)
│   └─ TeamDelete executes
│   └─ PostToolUse sentinel-write / handle_team_delete fires:
│       └─ os.RemoveAll(.state/sentinels/pathflow/{SID}/)  ← SENTINEL DIR GONE
│       └─ Remove pathflow-team.json
│       └─ Reset pathflow-phase-tasks.json
│   └─ PostToolUse removes is-pathflow-active flag
│
└─ Lead exits → SessionEnd fires for lead
    └─ shouldSkipCleanup() runs
    └─ is-pathflow-active flag: GONE (removed after TeamDelete)
    └─ Returns false → cleanup proceeds correctly
```

The critical observation: **TeamDelete's PostToolUse hook destroys the sentinel
directory before any subsequent SessionEnd fires**. The pf-7 sentinel exists
only briefly between TaskUpdate completion and TeamDelete. After TeamDelete, the
entire sentinel directory is gone.

---

## 5. Why option D (pf-7 sentinel) was rejected

The intuitive fix is to gate cleanup on the pf-7 sentinel: "if pf-7 exists, the
session completed cleanly, allow cleanup." This approach is fundamentally broken.

The sequence of events at PF7-END is:

```text
TaskUpdate(PF7-TSK-03) → pathflow-pf-7 sentinel created
     ↓
TeamDelete called
     ↓
HandlePostTeamDelete fires (PostToolUse)
     ↓
os.RemoveAll(.state/sentinels/pathflow/{SID}/)
     ↓
pathflow-pf-7 is GONE
     ↓
Lead exits → SessionEnd fires
     ↓
Check for pf-7 sentinel → NOT FOUND → "no clean PF7" → would block cleanup
```

The `HandlePostTeamDelete` function in Go (`team/guard.go:132-160`) and its Rust
mirror (`post_tool_use.rs:300-332`) both call `os.RemoveAll` on the entire
sentinel directory as step 1. The pf-7 sentinel is deleted before the lead's
SessionEnd fires.

Any solution based on checking the pf-7 sentinel in `shouldSkipCleanup()` is
a race against `HandlePostTeamDelete`. The sentinel cannot be the guard signal.

---

## 6. Scenario analysis

The following scenarios cover all meaningful runtime states for the guard
decision. Each diagram shows the state of key files and the expected decision.

### Scenario A: sub-agent exits mid-session (the bug)

```text
Timeline:
  SessionStart (lead)           → pathflow-active created
  TeamCreate                    → config.json created
  SessionStart (sub-agent)      → (reads same session via env)
  [work in progress]
  Sub-agent exits               → SessionEnd fires for sub-agent

State at SessionEnd (sub-agent):
  is-pathflow-active:           EXISTS (session active)
  pathflow-team.json:           EXISTS (team_name = "inf-tsk-022-024")
  config.json members:          in-process agents, tmuxPaneId = "" or dead
  tmux liveness check:          ALL DEAD (in-process agents, no tmux)

Current decision:               allow cleanup (BUG — session still active)
Correct decision:               SKIP cleanup
```

### Scenario B: normal PF7 shutdown (teammate exits before TeamDelete)

```text
Timeline:
  Lead sends shutdown_request to cf-development
  cf-development exits          → SessionEnd fires

State at SessionEnd (cf-development):
  is-pathflow-active:           EXISTS
  pathflow-team.json:           EXISTS
  config.json members:          lead + other teammates (some alive)
  tmux liveness:                some panes alive (lead is running)

Current decision:               skip cleanup (correct — if lead uses tmux)
With in-process lead:           BUG — all panes dead, allow cleanup
Correct decision:               SKIP cleanup
```

### Scenario C: lead exits after TeamDelete

```text
Timeline:
  TeamDelete succeeds
  PostToolUse handler:
    - Removes .state/sentinels/pathflow/{SID}/
    - Removes pathflow-team.json
    - Resets checkpoint
  PostToolUse hook removes is-pathflow-active
  Lead exits                    → SessionEnd fires

State at SessionEnd (lead):
  is-pathflow-active:           GONE (removed by PostToolUse after TeamDelete)
  session state dir:            exists but no flag

Current decision:               allow cleanup (correct — flag gone)
Correct decision:               allow cleanup
```

### Scenario D: crash / orphaned session

```text
Timeline:
  Session running
  Process dies unexpectedly (crash, kill, OOM)
  No cleanup hooks run
  Next SessionStart detects orphan via sweepAllStaleSessions

State at next cleanup opportunity:
  is-pathflow-active:           EXISTS (never cleaned up)
  pathflow-team.json:           EXISTS
  config.json:                  may be present or absent
  tmux liveness:                all dead (session long ended)

Current decision:               allow cleanup (works, but only for tmux agents)
For in-process agents:          may incorrectly skip (new behavior needed)
Correct decision:               eventually clean via sweepAllStaleSessions
```

### Scenario E: non-PathFlow session

```text
State:
  is-pathflow-active:           DOES NOT EXIST
  (no PathFlow team, no sentinels)

Current decision:               allow cleanup (correct — no PathFlow)
Correct decision:               allow cleanup
```

### Scenario F: new PF flow after completed one

```text
Timeline:
  Session 1 completes PF7 cleanly
  Session 2 starts for same session ID (compact/resume)
  Session 2 runs TeamCreate
  New pathflow-active flag created

State:
  is-pathflow-active:           EXISTS (session 2 active)
  pathflow-team.json:           EXISTS (session 2 team)

Current decision:               depends on tmux (same bug applies)
Correct decision:               SKIP cleanup (session 2 is active)
```

### Scenario G: status stuck but PF7 done (edge case)

```text
Scenario:
  PF7 completes (TeamDelete ran, pathflow-active removed)
  But pathflow-session-status.json was not updated to "pf-complete"
  (e.g., hook failure during transition)

State:
  is-pathflow-active:           GONE
  pathflow-session-status.json: status = "pf-in-progress"

With new logic (Section 8):
  is-pathflow-active check:     flag gone → return false (allow cleanup)
  (status file not consulted — flag absence is the primary signal)

Decision:                       allow cleanup (correct)
```

---

## 7. The solution: pathflow-session-status.json

The replacement for the tmux liveness check is a structured status file that
tracks PathFlow session state. It is written only by hooks, read only by
`shouldSkipCleanup()`, and deleted only by `cleanSessionState()`.

### 7.1 File structure

```json
{
  "session_id": "ses-01kkk3zsxnp79v9s88amqztzsm",
  "team_name": "inf-tsk-022-024",
  "status": "pf-in-progress",
  "last_completed_phase": "pf-3",
  "last_completed_stage": "ws-dev",
  "created_at": "2026-03-13T10:00:00Z",
  "updated_at": "2026-03-13T11:30:00Z"
}
```

**Location:** `.state/session/{SID}/pathflow/pathflow-session-status.json`

**Fields:**

| Field | Type | Purpose |
|-------|------|---------|
| `session_id` | string | The `CODEFLOW_SESSION_ID` for this session |
| `team_name` | string | Team name from TeamCreate (for recovery diagnostics) |
| `status` | string | Current PathFlow lifecycle status (see Section 7.2) |
| `last_completed_phase` | string | Last pf-N phase sentinel created |
| `last_completed_stage` | string | Last ws-* stage sentinel created |
| `created_at` | RFC3339 | When this file was created |
| `updated_at` | RFC3339 | Last update timestamp |

### 7.2 Status values and transitions

```text
(session start)
      │
      ▼
  "created"         ← written by SessionStart hook (init step)
      │
      │ TeamCreate PostToolUse
      ▼
  "pf-started"      ← written when TeamCreate tool completes
      │
      │ first phase sentinel (pf-1) created
      ▼
  "pf-in-progress"  ← written by checkpoint-complete on any phase completion
      │
      │ HandlePostTeamDelete (TeamDelete PostToolUse)
      ▼
  "pf-complete"     ← written when TeamDelete succeeds
      │
      │ SessionEnd cleanSessionState
      ▼
  (file deleted)    ← .state/session/{SID}/ removed by cleanSessionState()
```

| Status | Set by | Meaning |
|--------|--------|---------|
| `created` | SessionStart hook | Session started, PathFlow not yet initialized |
| `pf-started` | PostToolUse (TeamCreate) | Team created, PathFlow active |
| `pf-in-progress` | PostToolUse (checkpoint-complete) | At least one phase completed |
| `pf-complete` | PostToolUse (HandlePostTeamDelete) | TeamDelete completed cleanly |

### 7.3 File lifecycle

```text
Created:  SessionStart hook
           └─ Writes status = "created"
           └─ Does NOT overwrite if file already exists (resume case)

Updated:  Multiple PostToolUse handlers (not SessionEnd)
           ├─ TeamCreate → status = "pf-started", team_name = "{name}"
           ├─ checkpoint-complete (any pf-N) → status = "pf-in-progress",
           │   last_completed_phase = "pf-{N}"
           ├─ sentinel-write (any ws-*) → last_completed_stage = "ws-{stage}"
           └─ HandlePostTeamDelete → status = "pf-complete"

Deleted:  cleanSessionState() ONLY
           └─ os.RemoveAll(.state/session/{SID}/) removes the entire dir
           └─ This is Section 10 of EndCleanup — never called before Section 3
```

**Key invariant:** The file is NEVER deleted between SessionStart and the point
where `cleanSessionState()` runs. It cannot be deleted mid-session by any hook
except `cleanSessionState()`. This eliminates the race condition in the current
implementation, where `config.json` can be deleted before `shouldSkipCleanup()`
reads it.

### 7.4 Hook update timeline

```text
Session lifetime event timeline for pathflow-session-status.json:

  SessionStart (startup)
    → create status = "created"

  TeamCreate PostToolUse
    → update status = "pf-started", team_name = "{name}"

  TaskUpdate(PF1-TSK-01, completed) → checkpoint-complete
    → no phase done yet (not all PF1 tasks complete)

  TaskUpdate(PF1-TSK-02, completed) → checkpoint-complete
    → pf-1 phase complete
    → update status = "pf-in-progress", last_completed_phase = "pf-1"

  [... repeat for pf-2, pf-3, etc. ...]

  SendMessage "STAGE-COMPLETE: WS-DEV" → sentinel-write
    → update last_completed_stage = "ws-dev"

  TaskUpdate(PF7-TSK-03, completed) → checkpoint-complete
    → pf-7 phase complete
    → update last_completed_phase = "pf-7"

  TeamDelete PostToolUse → HandlePostTeamDelete
    → update status = "pf-complete"
    → (also removes sentinel dir and pathflow-team.json)

  Lead exits → SessionEnd
    → shouldSkipCleanup() reads status = "pf-complete" → allow cleanup
    → cleanSessionState() removes .state/session/{SID}/ (including this file)
```

---

## 8. New shouldSkipCleanup decision logic

The new implementation uses a multi-signal check. The primary signal is the
`is-pathflow-active` flag (unchanged). The secondary signal is the status file.

```text
shouldSkipCleanup(sessionStateDir)
│
├─ Check is-pathflow-active flag
│   └─ Does NOT exist → return false (no active session, allow cleanup)
│   └─ EXISTS → continue (PathFlow was started, must check status)
│
├─ Read pathflow-session-status.json
│   └─ File not found → return false (no status file, safe to clean)
│   └─ Parse error → return false (corrupt file, safe to clean)
│
├─ Check status field
│   └─ "pf-complete" → return false (TeamDelete ran, allow cleanup)
│   └─ "" or "created" → return true (early stage, skip cleanup)
│   └─ "pf-started" or "pf-in-progress" → continue to team_name check
│
├─ Check team_name field
│   └─ "" → return true (team exists but name unknown, skip to be safe)
│   └─ non-empty → continue to config existence check
│
├─ Check ~/.claude/teams/{team_name}/config.json existence
│   └─ Does NOT exist → return false (team already cleaned, allow cleanup)
│   └─ EXISTS → continue
│
└─ Check last_completed_phase
    └─ "" or "pf-1" or "pf-2" → return true (early phase, skip cleanup)
    └─ "pf-7" → return false (PF7 complete, allow cleanup — TeamDelete
    │             may have failed to update status to "pf-complete")
    └─ any other pf-N → return true (mid-session, skip cleanup)
```

**The key changes from current behavior:**

1. Status-driven decision replaces tmux liveness. In-process agents produce the
   same status updates as tmux-backed agents.
2. The default when uncertain changes from "allow cleanup" to "skip cleanup"
   for active sessions.
3. Config existence check is retained as a fallback signal (if the config is
   already gone, cleanup already ran — allow again).

---

## 9. Decision matrix

The table below shows every meaningful scenario, the state of each signal, and
the expected decision.

| Scenario | Status | Team Name | Config Exists? | Phase | Decision | Correct? |
|----------|--------|-----------|----------------|-------|----------|----------|
| Sub-agent exits mid-session (Scenario A) | `pf-in-progress` | `inf-tsk-022-024` | Yes | `pf-4` | SKIP | Yes |
| Normal PF7 teammate shutdown (Scenario B) | `pf-in-progress` | `inf-tsk-022-024` | Yes | `pf-6` | SKIP | Yes |
| Lead exits after TeamDelete (Scenario C) | (flag gone) | n/a | n/a | n/a | ALLOW | Yes |
| Crash / orphaned session (Scenario D) | `pf-in-progress` | `inf-tsk-022-024` | Maybe | `pf-4` | SKIP on SessionEnd | Yes |
| Non-PathFlow session (Scenario E) | (flag gone) | n/a | n/a | n/a | ALLOW | Yes |
| Status file missing (new session) | not found | n/a | n/a | n/a | ALLOW | Yes |
| TeamDelete ran, status = pf-complete | `pf-complete` | any | n/a | `pf-7` | ALLOW | Yes |
| Team config already deleted | `pf-in-progress` | `inf-tsk-022-024` | No | any | ALLOW | Yes |
| PF7 complete, status not updated (Scenario G) | `pf-in-progress` | any | n/a | `pf-7` | ALLOW | Yes |
| Status = created (very early, no team yet) | `created` | `` | n/a | `` | SKIP | Yes |
| Corrupt status file | (parse error) | n/a | n/a | n/a | ALLOW | Yes |
| Status = pf-started, config missing | `pf-started` | `team-x` | No | `` | ALLOW | Yes |

---

## 10. Files changed

All changes are required in both Go and Rust implementations to maintain parity.

| File | Language | Change Type | Description |
|------|----------|-------------|-------------|
| `codeflow-cli/internal/hooks/session/end.go` | Go | Modify | Replace tmux check in `shouldSkipCleanup()` with status file check |
| `codeflow-cli/internal/hooks/session/start.go` | Go | Modify | Write `pathflow-session-status.json` at session init (status = "created") |
| `codeflow-cli/internal/hooks/team/guard.go` | Go | Modify | `HandlePostTeamDelete` updates status to "pf-complete" before removing sentinel dir |
| `codeflow-rs/codeflow-core/src/hooks/session_end.rs` | Rust | Modify | Mirror Go changes: replace `should_skip_cleanup()` logic |
| `codeflow-rs/codeflow-core/src/hooks/session_start.rs` | Rust | Modify | Write status file at session init |
| `codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs` | Rust | Modify | `handle_team_delete` updates status to "pf-complete" |

The Go post_tool_use handler for checkpoint-complete and sentinel-write updates
(sections 7.2 and 7.4) are handled through the existing hook pipeline — the
checkpoint-complete and sentinel-write hooks need to update `last_completed_phase`
and `last_completed_stage` fields when they create their respective sentinels.
This may require additional changes to the checkpoint and sentinel write hooks.

---

## 11. Comparison: old vs new

| Aspect | Old (tmux liveness) | New (status tracker) |
|--------|--------------------|--------------------|
| Guard signal | tmux pane liveness via `IsPaneAlive()` | Structured JSON status file |
| In-process agents | BROKEN — all panes dead, cleanup runs | Works — status transitions are hook-driven |
| Race conditions | `config.json` can be deleted before check | Status file persists until `cleanSessionState()` |
| Default on uncertainty | Allow cleanup (dangerous) | Skip cleanup (safe) |
| Crash recovery | Unreliable — stale pane IDs remain alive | Deferred to `sweepAllStaleSessions()` at next startup |
| Complexity | ~90 lines, 3 functions, reads 3 files | Multi-signal read of 1 file, clear status enum |
| Diagnostics | None (pass/fail based on pane liveness) | Timestamps, phase, stage, team name all recorded |
| Determinism | Depends on tmux server state | All hook-driven, no external process queries |
| Failure mode | Destructive (deletes team config) | Conservative (skips cleanup, logs warning) |

---

## 12. Backwards compatibility

The existing `is-pathflow-active` empty flag file is retained. It remains the
primary gate: if absent, cleanup always proceeds. The status file is a secondary
signal consulted only when the flag exists.

Sessions without a `pathflow-session-status.json` file (created before this
redesign is deployed, or created by an older binary) fall through to the
"file not found" path in the new logic. That path returns `false` (allow cleanup),
which is identical to the old behavior for sessions where the tmux check also
returned false. There is no regression for sessions that do not use the new file.

Old empty `is-pathflow-active` flag → status file not found → treated as "no
active PathFlow state" → proceed with cleanup. This is the safe default.

---

## 13. Related files

The following files are directly referenced in this analysis.

**Go implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-cli/internal/hooks/session/end.go` | `shouldSkipCleanup()`, `shouldSkipCleanupLegacy()`, `cleanTeamArtifacts()`, full 13-section flow |
| `codeflow-cli/internal/hooks/session/start.go` | Session init, `createSessionStatus()`, `IsPathflowActive()`, `WritePathflowSessionStatus()`, `UpdatePathflowSessionStatus()` |
| `codeflow-cli/internal/hooks/team/guard.go` | `HandlePostTeamDelete()` (writes `pf-complete`), `CheckTeamDelete()` |
| `codeflow-cli/internal/hooks/sentinel/stage.go` | `HandleTeamCreate()` (planned: writes `pf-started`) |
| `codeflow-cli/internal/hooks/sentinel/checkpoint.go` | checkpoint-complete hook (planned: writes `pf-in-progress`) |

**Rust implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-rs/codeflow-core/src/hooks/session_end.rs` | Rust mirror of `end.go` — `should_skip_cleanup()`, `run()` |
| `codeflow-rs/codeflow-core/src/hooks/session_start.rs` | Rust mirror of `start.go` — `create_pathflow_flag()` (pending migration to status file) |
| `codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs` | `handle_team_delete()` (pending: add `pf-complete` update), `handle_team_create()` (pending: add `pf-started` update), `SentinelWrite` handler |

**Configuration:**

| File | Relevance |
|------|-----------|
| `.codeflow/config/pathflow/pathflow-config.json` | Phase and stage definitions used by checkpoint system |

**Memory:**

| File | Relevance |
|------|-----------|
| `.claude/memory/bug_team_config_deletion.md` | Recurring bug history and root cause record |

**Related analysis documents:**

| File | Relevance |
|------|-----------|
| `.codeflow/docs/analysis/session-start-redesign.md` | SessionStart unified flow, scenario walk-throughs, dead code removal, implementation phasing |
| `.codeflow/docs/analysis/session-lifecycle-interaction.md` | Full start-to-end lifecycle timeline showing all hook interactions with the status file |
| `.codeflow/docs/analysis/session-startup-cleanup-redesign.md` | Earlier session startup bug fixes (source-based PID branching, sweepAllStaleSessions) |
