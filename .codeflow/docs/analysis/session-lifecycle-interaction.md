---
title: "SessionStart and SessionEnd Lifecycle Interaction"
type: analysis
status: superseded
date: 2026-03-13
updated_at: "2026-03-24"
area: infrastructure
scope:
  - codeflow-cli/core/src/hooks/session_start.rs
  - codeflow-cli/core/src/hooks/session_end.rs
  - codeflow-cli/core/src/hooks/post_tool_use.rs
---

> **SUPERSEDED by [session-lifecycle-unified.md](session-lifecycle-unified.md)**
> — This document was written against the Go CLI implementation.
> The Go CLI has been retired. The unified session lifecycle document covers
> the current Rust CLI implementation. This document is retained for historical
> context only.

# SessionStart and SessionEnd lifecycle interaction

This document describes how the SessionStart and SessionEnd hooks interact
across the full session lifecycle, with `pathflow-session-status.json` as the
shared state between them. It shows how intermediate hooks update the file
between start and end, how the three SessionStart exit paths map to
SessionEnd's `shouldSkipCleanup` decisions, and how the two hooks coordinate
to prevent mid-session cleanup.

## Table of Contents

- [1. The shared state: pathflow-session-status.json](#1-the-shared-state-pathflow-session-statusjson)
- [2. Full lifecycle timeline](#2-full-lifecycle-timeline)
- [3. How SessionStart creates the state](#3-how-sessionstart-creates-the-state)
- [4. How intermediate hooks update the state](#4-how-intermediate-hooks-update-the-state)
- [5. How SessionEnd reads the state](#5-how-sessionend-reads-the-state)
- [6. Exit path to cleanup decision mapping](#6-exit-path-to-cleanup-decision-mapping)
- [7. Coordination invariants](#7-coordination-invariants)
- [8. Interaction with the stale session sweep](#8-interaction-with-the-stale-session-sweep)
- [9. Related files](#9-related-files)

---

## 1. The shared state: pathflow-session-status.json

`pathflow-session-status.json` is the single file that both SessionStart and
SessionEnd read and write. It lives at:

```text
.state/session/{SID}/pathflow/pathflow-session-status.json
```

It is the canonical source of truth for whether a session is currently active
and how far through the PathFlow lifecycle it has progressed.

**Schema:**

```json
{
  "session_id": "ses-01kp...",
  "team_name": "my-team",
  "status": "pf-in-progress",
  "last_completed_phase": "pf-4",
  "last_completed_stage": "ws-dev",
  "created_at": "2026-03-13T10:00:00.000Z",
  "updated_at": "2026-03-13T11:30:00.000Z"
}
```

**Who writes it:**

| Writer | When | What changes |
|--------|------|-------------|
| `createSessionStatus` (`start.go`) | SessionStart, startup, lead | Creates file with `status="created"` |
| `HandleTeamCreate` (`sentinel/stage.go`) | PostToolUse, TeamCreate | Sets `status="pf-started"`, `team_name` |
| checkpoint-complete hook | TaskCompleted, any phase | Sets `status="pf-in-progress"`, `last_completed_phase` |
| sentinel-write hook | PostToolUse, STAGE-COMPLETE | Sets `last_completed_stage` |
| `HandlePostTeamDelete` (`team/guard.go`) | PostToolUse, TeamDelete | Sets `status="pf-complete"` |

**Who reads it:**

| Reader | When | What it checks |
|--------|------|---------------|
| `shouldSkipCleanup` (`end.go`) | SessionEnd, every agent exit | `status`, `team_name`, config existence |
| `handleStaleCleanup` (`start.go`) | SessionStart STEP 3 | `status`, `team_name`, config existence |
| `sweepAllStaleSessions` (`start.go`) | SessionStart STEP 4 | `status`, `updated_at` for age |
| `detectCompactRecovery` (`start.go`) | SessionStart STEP 11 | `last_completed_phase`, `last_completed_stage` |
| `create_pathflow_flag` (Rust, transition) | SessionStart STEP 7 | Presence check (is-pathflow-active) |

**Key invariant:** The file is created by SessionStart and deleted by
SessionEnd's `cleanSessionState`. It is never deleted between creation and the
point where `cleanSessionState` runs. All intermediate hook updates are
atomic writes (write to `.tmp` then rename). This eliminates the race
condition in the old implementation where `config.json` could be deleted
before `shouldSkipCleanup` finished reading it.

---

## 2. Full lifecycle timeline

The following diagram shows every hook event across a complete PathFlow session,
the state of `pathflow-session-status.json` at each point, and which SessionEnd
calls would be blocked or allowed.

```text
FULL SESSION LIFECYCLE: start → end
Status file state shown at each checkpoint.

  ┌─────────────────────────────────────────────────────────────────────────┐
  │  SessionStart fires (source=startup, lead)                              │
  │  → STEP 7: createSessionStatus()                                        │
  │  Status file: { status:"created", session_id:"ses-01kp..." }            │
  │  shouldSkipCleanup if any agent exited now: ALLOW (status="created")    │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  TeamCreate PostToolUse fires                                           │
  │  → HandleTeamCreate() creates pathflow-team.json                        │
  │  → (planned) UpdatePathflowSessionStatus: status="pf-started"           │
  │  Status file: { status:"pf-started", team_name:"my-team" }             │
  │  shouldSkipCleanup if teammate exited now: SKIP (active session)        │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  PF1-TSK-01, PF1-TSK-02 TaskCompleted → checkpoint-complete fires      │
  │  → (planned) UpdatePathflowSessionStatus:                               │
  │      status="pf-in-progress", last_completed_phase="pf-1"              │
  │  Status file: { status:"pf-in-progress", last_completed_phase:"pf-1" } │
  │  shouldSkipCleanup if teammate exited now: SKIP (active session)        │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  PF2, PF3 phases complete (similar checkpoint-complete events)          │
  │  Status file: { last_completed_phase:"pf-3" }                          │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  PF4-EXECUTE: cf-development exits after WS-DEV work                   │
  │  SessionEnd fires for cf-development                                    │
  │  shouldSkipCleanup reads: status="pf-in-progress", team="my-team",     │
  │    config.json exists, last_completed_phase="pf-3" (not pf-7)          │
  │  Decision: SKIP cleanup (teammate shutdown, session active)             │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  SendMessage: "STAGE-COMPLETE: WS-DEV" → sentinel-write fires          │
  │  → (planned) UpdatePathflowSessionStatus:                               │
  │      last_completed_stage="ws-dev"                                      │
  │  Status file: { last_completed_stage:"ws-dev" }                        │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  PF5, PF6 phases complete. PF7-END reached.                            │
  │  Status file: { last_completed_phase:"pf-6" }                          │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  TeamDelete PostToolUse fires                                           │
  │  → HandlePostTeamDelete():                                              │
  │      1. UpdatePathflowSessionStatus: status="pf-complete"               │
  │      2. os.RemoveAll(.state/sentinels/pathflow/{SID}/)  ← GONE         │
  │      3. os.Remove(pathflow-team.json)                                   │
  │      4. Checkpoint reset                                                │
  │  is-pathflow-active flag: removed by PostToolUse sentinel hook          │
  │  Status file: { status:"pf-complete", last_completed_phase:"pf-6" }    │
  └─────────────────────────────────────────────────────────────────────────┘
                                  │
  ┌─────────────────────────────────────────────────────────────────────────┐
  │  Lead exits → SessionEnd fires for lead                                 │
  │  shouldSkipCleanup reads: status="pf-complete"                          │
  │  Decision: ALLOW cleanup (session finished)                             │
  │  → Section 9: cleanTeamArtifacts (backstop, team already gone)         │
  │  → Section 10: cleanSessionState removes .state/session/{SID}/         │
  │      STATUS FILE DELETED as part of directory removal.                  │
  │  → Section 11-13: runtime cleanup, temp dir, ledger event              │
  └─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. How SessionStart creates the state

SessionStart writes the status file at STEP 7 (`createSessionStatus` in Go,
`create_pathflow_flag` in Rust — Rust pending migration).

The write occurs ONLY for:

- `source=startup` or `source=unknown` (full lead path, EXIT C)
- When the file does not already exist (prevents overwrite on resume)

SessionStart does NOT write the status file for:

- `source=compact`, `source=clear`, `source=resume` (EXIT A, minimal path)
- Teammate mode (EXIT B, early return before STEP 7)

This means the status file always belongs to the lead's initialization path.
Teammates and compact events never create a new status file.

**Resume behavior:** If `createSessionStatus` finds the file already exists, it
returns `isResume=true` and skips creation. This preserves the status written by
a previous initialization, allowing the session to pick up where it left off.

---

## 4. How intermediate hooks update the state

Between SessionStart and SessionEnd, four categories of hooks update the file:

**TeamCreate PostToolUse** (`HandleTeamCreate` in `sentinel/stage.go`):

Currently creates `pathflow-team.json` but does not update the status file.
The planned update sets `status="pf-started"` and `team_name`. Until this is
implemented, the status remains `"created"` even after TeamCreate, which means
`shouldSkipCleanup` cannot protect the session in the `pf-started` window
(sub-agent exits between TeamCreate and first phase completion would allow
cleanup incorrectly).

**Checkpoint-complete TaskCompleted** (`sentinel/checkpoint.go`):

Fires when all tasks in a phase complete and creates the phase sentinel. The
planned update sets `status="pf-in-progress"` on the first phase completion
and updates `last_completed_phase` on each subsequent completion. Until this
is implemented, the status does not advance past `"created"`.

**Sentinel-write PostToolUse** (`sentinel/stage.go` `HandleStageSentinel`):

Fires on `SendMessage` containing `STAGE-COMPLETE: WS-{STAGE}`. Creates the
stage sentinel file. The planned update sets `last_completed_stage`. This
provides coarse-grained recovery context (which stage the session was in when
a compact occurred).

**HandlePostTeamDelete** (`team/guard.go`):

Fires after TeamDelete succeeds. Updates status to `"pf-complete"` as step 1
of its cleanup sequence, before removing the sentinel directory. This is the
most critical update: it signals to SessionEnd that the session completed and
cleanup may proceed. This update IS currently implemented in Go (`guard.go`
line 136-141). Rust does not yet implement this update.

---

## 5. How SessionEnd reads the state

SessionEnd's `shouldSkipCleanup` function (`end.go`) implements the following
decision tree, reading the status file as its primary signal:

```text
shouldSkipCleanup(sessionStateDir)
│
├─ Read pathflow-session-status.json
│   Parse error → ALLOW cleanup (corrupted state)
│   File missing → check old is-pathflow-active flag (backward compat)
│       Flag missing → ALLOW cleanup (no active session)
│       Flag exists  → shouldSkipCleanupLegacy() (team config check)
│
├─ status == "created"
│   → ALLOW cleanup (no team yet, nothing to protect)
│
├─ status == "pf-complete"
│   → ALLOW cleanup (TeamDelete succeeded, session finished)
│
├─ status == "pf-started" or "pf-in-progress"
│   ├─ team_name == "" → ALLOW cleanup (no team registered)
│   ├─ ~/.claude/teams/{team_name}/config.json missing
│   │   → ALLOW cleanup (team already dissolved)
│   ├─ last_completed_phase == "PF7"
│   │   → ALLOW cleanup (PF7 complete, guard against stale status)
│   └─ All checks pass → SKIP cleanup (active session, teammate shutdown)
│
└─ unknown status → ALLOW cleanup (defensive default)
```

The key distinction from the old implementation:

- Old: reads `tmuxPaneId` from `config.json`, calls `IsPaneAlive`. Returns
  false (allow cleanup) for all in-process agents.
- New: reads `status` from `pathflow-session-status.json`. Returns true (skip
  cleanup) for any `pf-started` or `pf-in-progress` session with an
  existing team config, regardless of agent backend.

---

## 6. Exit path to cleanup decision mapping

SessionStart has three exit paths (A, B, C). Each maps to a specific cleanup
behavior when SessionEnd subsequently fires for that agent.

### Exit A: compact/resume/clear (minimal path)

The agent that took EXIT A (compact, resume, or clear) did not modify the
status file. The status file reflects whatever the session was doing before the
compact.

When SessionEnd fires for this agent (after the compact session ends):

```text
Status file state at EXIT A SessionEnd:
  status = "pf-in-progress" (or "pf-started")
  team_name = "my-team"
  config.json = exists (if session still active)

shouldSkipCleanup result: SKIP (session is still active)
```

This is correct: a compact is not a session end. The lead (or surviving
teammates) continue in the same session.

### Exit B: teammate early return

The agent that took EXIT B is a teammate joining an active session. When
this teammate exits and SessionEnd fires:

```text
Status file state at EXIT B SessionEnd:
  status = "pf-in-progress"
  team_name = "my-team"
  config.json = exists (lead is still running)

shouldSkipCleanup result: SKIP (active session)
```

This is the primary bug fix: in-process teammates now correctly skip cleanup
because the signal is the status file, not tmux pane liveness.

### Exit C: new lead (full initialization)

The agent that took EXIT C is the lead. It wrote `status="created"`. After
running TeamCreate and phase work, the status advances to `pf-started` and
`pf-in-progress`. When the lead eventually reaches PF7-END and calls
TeamDelete, HandlePostTeamDelete sets `status="pf-complete"`. Then SessionEnd
for the lead fires:

```text
Status file state at EXIT C SessionEnd (after PF7):
  status = "pf-complete"

shouldSkipCleanup result: ALLOW (session finished, clean up)
```

The lead's final SessionEnd always sees `pf-complete` and proceeds with
full cleanup.

---

## 7. Coordination invariants

The following invariants must hold for the interaction to work correctly:

**Invariant 1: Status file is created before TeamCreate**

`createSessionStatus` (STEP 7 of SessionStart) runs before any PathFlow
activity. TeamCreate PostToolUse fires after the lead is already running.
The status file is guaranteed to exist before the `pf-started` update.

**Invariant 2: Status file is NOT deleted between SessionStart and TeamDelete**

No hook deletes the status file except `cleanSessionState`. `cleanSessionState`
is gated by `shouldSkipCleanup` returning false. `shouldSkipCleanup` returns
false only when status is `"created"`, `"pf-complete"`, or the file is absent.
During an active session (status = `pf-started` or `pf-in-progress`),
`shouldSkipCleanup` returns true, blocking `cleanSessionState`. The file is safe.

**Invariant 3: `pf-complete` is written BEFORE sentinel directory removal**

`HandlePostTeamDelete` updates the status to `"pf-complete"` as step 1,
before removing the sentinel directory in step 2. This ordering ensures
SessionEnd can read `pf-complete` even though the sentinel directory (and
thus `pathflow-pf-7`) is already gone by the time the lead's SessionEnd fires.

**Invariant 4: Session lock serializes concurrent SessionStart calls**

The session.lock flock in STEP 0 prevents a teammate from reading a partial
state where codeflow-env.sh is written but the status file is not yet created.
The lock is held from STEP 0 until after STEP 5 (env file write). The status
file is written in STEP 7, after the lock is released. Teammates reading the
env file after STEP 5 will see the correct SID; the status file may not exist
yet, but STEP 3 checks the status file only if the env SID matches the
env var — and the env var is only set in the lead's environment, not the
teammate's, until the lead explicitly sets it via the spawn prompt.

**Invariant 5: Status file is written atomically**

`WritePathflowSessionStatus` writes to a `.tmp` file then renames. Readers
always see a complete, consistent JSON document. Partial writes from concurrent
updates produce a valid file or no file (not a half-written file).

---

## 8. Interaction with the stale session sweep

`sweepAllStaleSessions` (STEP 4, runs on `startup/unknown` only) reads the
status file for each stale-candidate session to make cleanup decisions. Its
decisions are:

```text
sweepAllStaleSessions decision for each old session dir:

  Read pathflow-session-status.json for old-SID:
    File missing:
      age > 48h → CLEAN (pre-redesign orphan or abandoned startup)
      age < 48h → SKIP  (may be mid-initialization)

    status == "pf-complete":
      → CLEAN unconditionally (session completed normally)

    status == "created":
      age > 1h  → CLEAN (stuck at startup, no TeamCreate arrived)
      age < 1h  → SKIP  (may still be initializing)

    status == "pf-started" or "pf-in-progress":
      team_name == "":
        age > 1h → CLEAN
        else     → SKIP
      config.json missing → CLEAN (team dissolved, session orphaned)
      config.json exists, age > 24h → CLEAN (crashed long ago, team stale)
      config.json exists, age < 24h → SKIP  (may still be active)
```

This interaction is important: the sweep runs at the START of the new lead's
session. It may clean old sessions that belonged to a previous lead that
crashed or exited without reaching PF7. The status file age prevents premature
cleanup of sessions that are still active.

When SessionEnd's `cleanSessionState` later runs for the new session's lead,
it removes the new session's status file. The old session's status file was
already removed by the sweep.

---

## 9. Related files

**Go implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-cli/internal/hooks/session/start.go` | `createSessionStatus`, `sweepAllStaleSessions`, `detectCompactRecovery`, `IsPathflowActive` |
| `codeflow-cli/internal/hooks/session/end.go` | `shouldSkipCleanup`, `shouldSkipCleanupLegacy`, `cleanSessionState`, 13-section flow |
| `codeflow-cli/internal/hooks/sentinel/stage.go` | `HandleTeamCreate` (planned status update) |
| `codeflow-cli/internal/hooks/team/guard.go` | `HandlePostTeamDelete` (writes `pf-complete`) |
| `codeflow-cli/internal/hooks/sentinel/checkpoint.go` | checkpoint-complete (planned `pf-in-progress` update) |

**Rust implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-rs/codeflow-core/src/hooks/session_start.rs` | `create_pathflow_flag` (old approach; pending migration to status file) |
| `codeflow-rs/codeflow-core/src/hooks/session_end.rs` | `should_skip_cleanup`, `run` |
| `codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs` | `handle_team_create`, `handle_team_delete` (planned status updates) |

**Related analysis documents:**

| File | Relevance |
|------|-----------|
| `.codeflow/docs/analysis/session-start-redesign.md` | SessionStart unified flow, scenario walk-throughs, dead code removal |
| `.codeflow/docs/analysis/session-end-cleanup-redesign.md` | SessionEnd shouldSkipCleanup redesign, scenario analysis, decision matrix |
| `.codeflow/docs/analysis/session-startup-cleanup-redesign.md` | Earlier session startup bug fixes applied to Go and Rust |
