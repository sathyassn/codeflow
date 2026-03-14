---
title: "SessionStart Hook Redesign: Unified Flow with Source-Based Routing"
type: analysis
status: proposed
date: 2026-03-13
area: infrastructure
scope:
  - codeflow-cli/internal/hooks/session/start.go
  - codeflow-cli/internal/hooks/session/start_test.go
  - codeflow-cli/internal/hooks/sentinel/stage.go
  - codeflow-cli/internal/hooks/team/guard.go
  - codeflow-rs/codeflow-core/src/hooks/session_start.rs
  - codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs
---

# SessionStart hook redesign: unified flow with source-based routing

This document records the analysis and proposed redesign for the SessionStart
hook. It covers the current problems in the hook, the new unified flow that
replaces the old logic, scenario walk-throughs for all meaningful runtime
states, status transition mapping, and the dead code to be removed.

## Table of Contents

- [1. Current problems](#1-current-problems)
  - [1.1 Teammate detection relies on tmux pane liveness](#11-teammate-detection-relies-on-tmux-pane-liveness)
  - [1.2 Dead PID code](#12-dead-pid-code)
  - [1.3 No source-type gating](#13-no-source-type-gating)
  - [1.4 Stale session sweep uses tmux](#14-stale-session-sweep-uses-tmux)
  - [1.5 No session state awareness on compact](#15-no-session-state-awareness-on-compact)
- [2. Redesign: unified flow](#2-redesign-unified-flow)
  - [2.1 Two differentiation signals](#21-two-differentiation-signals)
  - [2.2 Complete unified flow](#22-complete-unified-flow)
  - [2.3 Three exit paths](#23-three-exit-paths)
- [3. Scenario walk-throughs](#3-scenario-walk-throughs)
- [4. Status transitions](#4-status-transitions)
- [5. Dead code removal](#5-dead-code-removal)
- [6. Decision matrix](#6-decision-matrix)
- [7. Related files](#7-related-files)

---

## 1. Current problems

### 1.1 Teammate detection relies on tmux pane liveness

The current teammate detection in `handleStaleCleanup` (`start.go`) reads
`pathflow-team.json`, extracts member `tmuxPaneId` fields from
`~/.claude/teams/{team_name}/config.json`, and calls `TmuxChecker.IsPaneAlive`
for each member pane.

In-process agents do not use tmux panes. Their `tmuxPaneId` is empty or refers
to a dead pane. `IsPaneAlive` returns `false`. The function concludes the
session is stale and proceeds with cleanup, even though the session is active.

The same failure mode applies in `sweepAllStaleSessions`: it calls
`hasLiveTeamPanes`, which uses the same tmux check, and will incorrectly clean
an active session whose agents are all in-process.

### 1.2 Dead PID code

`getClaudePID` in `start.go` uses `ps -o ppid=` to walk up the process tree
and find the Claude process PID. This PID is stored in `PPID` on the
`Initializer` struct and passed to `updateLeadPID` when writing
`pathflow-team.json`.

The `ProcessChecker` interface (`IsAlive`) is defined on the `Initializer` and
injected into `NewInitializer`. However, `ProcessChecker.IsAlive` is no longer
called from `start.go`'s main initialization flow. Teammate detection in the
current code goes through `hasLiveTeamPanes` (tmux-based), not
`ProcessChecker` (PID-based).

Captured dead code:

- `getClaudePID`: called only during `NewInitializer` to set `PPID`
- `ProcessChecker` interface: injected but not used by `StartInit`
- `updateLeadPID`: still called on `source=resume` to update `lead_pid` in
  `pathflow-team.json`, but `lead_pid` is no longer read by `shouldSkipCleanup`
  in `end.go` (which now uses the status file)
- `lead_pid` field in `pathflow-team.json`: written by `HandleTeamCreate`
  (`sentinel/stage.go`) and `handle_team_create` (`post_tool_use.rs`), read
  nowhere in the active cleanup path

The `PPID` field on `Initializer` remains necessary as long as `updateLeadPID`
is called. The full removal requires also removing the `resume` PID update path.

### 1.3 No source-type gating

Compact, resume, and clear sources currently reach the same code paths as
startup for several operations:

- Writing the `is-pathflow-active` flag (now replaced by
  `pathflow-session-status.json`) happens on any non-resume path where the
  file does not already exist
- Checkpoint initialization runs regardless of source type
- The stale sweep runs only on `startup/unknown` (already gated), but the
  `detectStaleTeams` sweep also runs only on startup (already gated)

The remaining gap is that compact/clear sessions for in-process agents that
are not detected as teammates will proceed through the full lead initialization
path (directory creation, checkpoint init, status file creation) even though
they are surviving agents from an ongoing session.

### 1.4 Stale session sweep uses tmux

`sweepAllStaleSessions` determines whether a session is stale by calling
`hasLiveTeamPanes`, which checks tmux pane liveness via `TmuxChecker`. For
sessions where all agents are in-process (no tmux panes), every session appears
stale, and the sweep removes active sessions.

The redesign replaces the tmux-based liveness check in the sweep with a
multi-signal check using `pathflow-session-status.json`:

- status = `pf-complete` → session done, safe to clean
- status = `pf-started` or `pf-in-progress` + config exists + age < 24h → skip
- Missing status file + age > 48h → clean (abandoned before redesign)
- Missing status file + recent → skip (may be in-process session without status
  file written yet)

### 1.5 No session state awareness on compact

When a compact, clear, or resume source fires, the current code in
`detectCompactRecovery` outputs generic COMPACT RECOVERY messages if any team
config or active session exists. This is advisory-only and does not use the
session status to provide phase-aware recovery context (e.g., "session was at
PF4-EXECUTE, WS-DEV stage").

The redesign makes the compact path read `pathflow-session-status.json` to
output the last completed phase and stage, giving the lead context to orient
itself after context overflow.

---

## 2. Redesign: unified flow

### 2.1 Two differentiation signals

The SessionStart hook fires identically for the lead, teammates, and in-process
sub-agents. No stdin field distinguishes who is calling. The hook must infer the
caller's role from two signals:

**Signal 1: `CODEFLOW_SESSION_ID` environment variable**

This variable is inherited by teammate processes from the lead. When a teammate
starts, its environment already contains the lead's session ID. When the lead
starts fresh, the variable is absent (empty string).

| Condition | Implication |
|-----------|-------------|
| `CODEFLOW_SESSION_ID` is empty | Caller is likely the lead (new process) |
| `CODEFLOW_SESSION_ID` matches `existingSID` | Caller may be a teammate |
| `CODEFLOW_SESSION_ID` differs from `existingSID` | New lead, stale env file |

**Signal 2: `pathflow-session-status.json`**

This file is written by the SessionStart hook when the lead starts fresh
(status `"created"`). It is updated by PostToolUse hooks as the session
progresses. Its presence and status value provide deterministic state about the
session lifecycle without querying external processes.

| Status | Implication |
|--------|-------------|
| File absent | No session, or session predates redesign |
| `"created"` | Session started but no team yet |
| `"pf-started"` | Team created, PF1 in progress |
| `"pf-in-progress"` | At least one phase completed |
| `"pf-complete"` | TeamDelete succeeded |

Signal 2 also confirms teammate mode: if the env var matches the existing SID
AND the status is active AND team config exists, the caller is a teammate.

### 2.2 Complete unified flow

The following flow replaces the current `StartInit` logic. Section numbers
correspond to the existing section numbering in `start.go` to ease migration.

```text
STEP 0: Acquire session.lock (flock)
        Serializes concurrent SessionStart calls across lead and teammates.
        Lead acquires first; teammate waits, then reads lead's state.
        Lock is released after env file is written or before early return.

STEP 1: Read existing session ID
        Primary source:  codeflow-env.sh (CANONICAL)
        Fallback source: pathflow-session-status.json session_id field
        Result:          existingSID (may be "" if no prior session)

STEP 2: Source-based routing
        compact | resume | clear → STEP 2a  (minimal path, no destructive ops)
        startup | unknown        → STEP 3   (full lead path)

STEP 2a: Minimal path (safe for all agents on compact/resume/clear)
         Read pathflow-session-status.json for existingSID.
         Set CODEFLOW_SESSION_ID = existingSID (from env file or status file).
         If status file found: output recovery context (last_completed_phase,
           last_completed_stage, team_name) to stderr.
         RETURN.  No stale sweep, no SID generation, no checkpoint init,
                  no directory creation, no status file creation.

STEP 3: Teammate detection (multi-signal, for startup/unknown sources)

        Signal 1: CODEFLOW_SESSION_ID env var
          envSID = os.Getenv("CODEFLOW_SESSION_ID")
          Case A: envSID == "" and existingSID == ""
            → No prior session. This is a fresh lead startup. Go to STEP 4.
          Case B: envSID == "" and existingSID != ""
            → env var absent but env file exists. Prior session may be stale.
              Continue to Signal 2.
          Case C: envSID != "" and envSID != existingSID
            → env var set to a different SID (e.g., new lead with stale file).
              Treat as new lead. Go to STEP 4.
          Case D: envSID != "" and envSID == existingSID
            → env var matches existing SID. May be teammate. Continue to
              Signal 2.

        Signal 2: pathflow-session-status.json confirmation
          Read status from .state/session/{existingSID}/pathflow/
          Case: File missing, or status == "pf-complete", or no config exists
            → Session not active. Treat as new lead. Go to STEP 4.
          Case: status == "pf-started" or "pf-in-progress", and
                team_name is set, and
                ~/.claude/teams/{team_name}/config.json exists
            → Active session confirmed. Caller is teammate. Go to STEP 3a.

STEP 3a: Teammate early return
         Set is_teammate = true
         Set CODEFLOW_SESSION_ID = existingSID
         Set CF_PROJECT_ROOT = basename(projectDir)
         Output: "TEAMMATE MODE: joining session {SID}, team {team_name}"
         Release session.lock
         RETURN.  Skip ALL remaining steps.

STEP 4: Sweep stale sessions (runs BEFORE SID generation, startup/unknown only)
        For each .state/session/{old-SID}/ (skip current working SID):
          Read pathflow-session-status.json from that session's pathflow dir.
          Decision tree:
            Missing status file:
              age > 48h → clean (likely pre-redesign orphan)
              age < 48h → skip (may be very new session without status yet)
            status == "pf-complete" → clean (session finished)
            status == "created":
              age > 1h  → clean (abandoned before TeamCreate)
              age < 1h  → skip (may be starting up)
            status == "pf-started" or "pf-in-progress":
              team_name == "" → age > 1h ? clean : skip
              config.json missing → clean (team already cleaned, orphaned)
              config.json exists + age > 24h → clean (crashed long ago)
              config.json exists + age < 24h → skip (may still be active)
          "Clean" removes:
            .state/session/{old-SID}/
            .state/sentinels/pathflow/{old-SID}/
            ~/.claude/teams/{team_name}/  (if team_name set)
            ~/.claude/tasks/{team_name}/  (if team_name set)

STEP 5: Generate new session ID
        Call session.Start() → returns new ULID-based SID
        Write codeflow-env.sh with new SID (overwrites old)
        Set CODEFLOW_SESSION_ID = new SID
        Set CF_PROJECT_ROOT = basename(projectDir)
        Release session.lock

STEP 6: Create session directories
        .state/logs/sessions/
        .state/logs/security/
        .state/db/
        .state/runtime/
        .state/sentinels/pathflow/{SID}/
        .state/session/{SID}/

STEP 7: Create pathflow-session-status.json (status = "created")
        Path: .state/session/{SID}/pathflow/pathflow-session-status.json
        If file already exists (resume into existing SID): return isResume=true
        Otherwise: write { session_id, status:"created", created_at, updated_at }

STEP 8: Initialize checkpoint (pathflow-phase-tasks.json)
        Path: .state/session/{SID}/pathflow/pathflow-phase-tasks.json
        Reads pathflow-config.json to pre-initialize all 7 phases.
        Skipped if file already exists (isResume=true).

STEP 9: Write session metadata
        Path: .state/logs/sessions/session-{SID}.meta
        Fields: session_id, claude_uuid, source, started_at, git_branch,
                git_commit, user

STEP 10: Detect stale teams (startup/unknown only, tmux-based for now)
         Scans ~/.claude/teams/ for team configs where all tmux panes are dead.
         Removes stale team dirs and task list dirs.
         Note: This remains tmux-based until team configs include backend type.
         In-process-only teams with dead panes will be cleaned here.

STEP 11: Detect compact recovery context
         For compact/resume/clear: emit recovery context from status file.
         (Already handled in STEP 2a; this step is a no-op in the redesign.)

STEP 12: Create project temp directory
         /tmp/claude/{projectName}/

STEP 13: Auto-rebuild CLI (startup only)
         Compare binary VCS revision to git HEAD; rebuild if different.

STEP 14: Output env JSON and RETURN
```

### 2.3 Three exit paths

The unified flow has exactly three exit points:

```text
                     SessionStart fires
                           │
               ┌───────────┴───────────┐
               │ STEP 0: Acquire lock  │
               └───────────┬───────────┘
                           │
               ┌───────────┴───────────┐
               │ STEP 1: Read existSID │
               └───────────┬───────────┘
                           │
                    source type?
                    │              │
              compact/resume/clear  startup/unknown
                    │              │
                    ▼              ▼
               ┌─────────┐   ┌──────────────┐
               │ STEP 2a │   │ STEP 3:      │
               │Minimal  │   │ Teammate     │
               │Path     │   │ detection    │
               └────┬────┘   └──────┬───────┘
                    │               │
             EXIT A │        teammate?
                    │         │        │
                    │       YES        NO
                    │         │        │
                    │         ▼        ▼
                    │   ┌──────────┐  STEP 4: Sweep stale
                    │   │ STEP 3a  │  STEP 5: Generate SID
                    │   │Teammate  │  STEP 6: Create dirs
                    │   │early ret │  STEP 7: Create status
                    │   └────┬─────┘  STEP 8: Init checkpoint
                    │        │        STEP 9: Write metadata
                    │   EXIT B│       STEP 10-13: ...
                    │        │         │
                    │        │        EXIT C
                    └────────┴─────────┘

EXIT A: STEP 2a → RETURN (compact/resume/clear, any agent)
  - Set CODEFLOW_SESSION_ID from env file / status file
  - Output recovery context if status file readable
  - No destructive operations performed

EXIT B: STEP 3a → RETURN (teammate joining, any backend)
  - set is_teammate=true, CODEFLOW_SESSION_ID=existingSID
  - Output "TEAMMATE MODE: joining session {SID}, team {team_name}"
  - No state mutation performed

EXIT C: STEP 14 → RETURN (new lead, full initialization)
  - New SID generated (or resumed SID used)
  - Full directory structure created
  - pathflow-session-status.json written with status="created"
  - Checkpoint initialized
  - All output written
```

---

## 3. Scenario walk-throughs

### Scenario A: Lead first startup

```text
Preconditions:
  codeflow-env.sh:   absent (or contains stale SID with pf-complete status)
  CODEFLOW_SESSION_ID: "" (env var not set)

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "" (no env file)
  STEP 2: source=startup → full path
  STEP 3: envSID="" and existingSID="" → Case A → new lead, go to STEP 4
  STEP 4: No prior sessions to sweep (or only pf-complete ones)
  STEP 5: Generate new SID (e.g., ses-01kp...), write codeflow-env.sh
  STEP 6: Create .state directories
  STEP 7: Write pathflow-session-status.json { status:"created" }
  STEP 8: Initialize checkpoint
  STEP 9: Write session metadata
  STEP 10-13: Stale team check, project temp dir, auto-rebuild
  STEP 14: Output env JSON → EXIT C

Output: CODEFLOW_SESSION_ID=ses-01kp..., status file written
```

### Scenario B: Teammate spawned (tmux or in-process)

```text
Preconditions:
  codeflow-env.sh:     contains ses-01kp...
  CODEFLOW_SESSION_ID: "ses-01kp..." (inherited from lead environment)
  pathflow-session-status.json: { status:"pf-in-progress", team_name:"my-team" }
  ~/.claude/teams/my-team/config.json: exists

Flow:
  STEP 0: Lock acquired (briefly, lead already released)
  STEP 1: existingSID = "ses-01kp..."
  STEP 2: source=startup → full path
  STEP 3: Signal 1: envSID="ses-01kp..." == existingSID="ses-01kp..." → Case D
          Signal 2: status="pf-in-progress", team_name="my-team", config exists
            → teammate confirmed → go to STEP 3a
  STEP 3a: is_teammate=true, CODEFLOW_SESSION_ID="ses-01kp..."
           Output: "TEAMMATE MODE: joining session ses-01kp..., team my-team"
           Release lock
           EXIT B

Output: CODEFLOW_SESSION_ID=ses-01kp..., is_teammate=true
        No state mutation. Lead's directories and status file untouched.
```

### Scenario C: Lead context overflow (compact)

```text
Preconditions:
  source: "compact"
  codeflow-env.sh:     contains ses-01kp...
  pathflow-session-status.json: { status:"pf-in-progress",
    last_completed_phase:"pf-4", last_completed_stage:"ws-dev",
    team_name:"my-team" }

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "ses-01kp..."
  STEP 2: source=compact → STEP 2a (minimal path)
  STEP 2a: Read status file → output recovery context:
    "COMPACT RECOVERY: Session ses-01kp..., team my-team"
    "Last completed phase: pf-4, last completed stage: ws-dev"
    "MANDATORY: Verify teammate liveness before any respawn."
    Set CODEFLOW_SESSION_ID = "ses-01kp..."
    EXIT A

Output: CODEFLOW_SESSION_ID=ses-01kp..., recovery context emitted
        No stale sweep, no SID generation, no status file mutation.
```

### Scenario D: Teammate context overflow (compact)

```text
Preconditions:
  source: "compact"
  CODEFLOW_SESSION_ID: "ses-01kp..." (inherited from lead)
  codeflow-env.sh: contains ses-01kp...

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "ses-01kp..."
  STEP 2: source=compact → STEP 2a (minimal path)
  STEP 2a: Read status file → output recovery context (same as Scenario C)
    Set CODEFLOW_SESSION_ID = "ses-01kp..."
    EXIT A

Output: Identical to Scenario C. The hook cannot distinguish lead compact from
        teammate compact at STEP 2a. This is correct: both should minimize
        operations and output recovery context.
```

### Scenario E: Lead startup after crash (config.json still exists)

```text
Preconditions:
  source: "startup"
  codeflow-env.sh: contains ses-OLD (crashed session's SID)
  CODEFLOW_SESSION_ID: "" (new process, env var not set)
  pathflow-session-status.json for ses-OLD:
    { status:"pf-in-progress", team_name:"my-team", updated_at: "2h ago" }
  ~/.claude/teams/my-team/config.json: exists

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "ses-OLD"
  STEP 2: source=startup → full path
  STEP 3: Signal 1: envSID="" and existingSID="ses-OLD" → Case B
          Signal 2: status="pf-in-progress", team exists, config exists
            BUT source=startup with envSID="" → this is a fresh process,
            not a teammate inheriting the env var.
            The status is active but the caller is the lead restarting.
            → Treat as new lead (not teammate). Go to STEP 4.
  STEP 4: Sweep ses-OLD:
    status="pf-in-progress", config exists, age=2h < 24h → SKIP
    (session may still be active from surviving teammates)
    Note: If no surviving teammates, stale team sweep in STEP 10 cleans it.
  STEP 5: Generate new SID (ses-NEW), write codeflow-env.sh
  STEP 6-14: Full initialization for ses-NEW → EXIT C

Output: New SID generated. Old session preserved for now; cleaned by STEP 10
        or next startup sweep once teammates exit and panes go dead.
```

### Scenario F: Lead startup after clean PF7

```text
Preconditions:
  source: "startup"
  codeflow-env.sh: contains ses-OLD
  pathflow-session-status.json for ses-OLD: { status:"pf-complete" }
  CODEFLOW_SESSION_ID: ""

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "ses-OLD"
  STEP 2: source=startup → full path
  STEP 3: Signal 1: envSID="" → Case A or B
          Signal 2: status="pf-complete" → session done, not active
            → Treat as new lead. Go to STEP 4.
  STEP 4: Sweep ses-OLD: status="pf-complete" → CLEAN
    Removes .state/session/ses-OLD/, .state/sentinels/pathflow/ses-OLD/
  STEP 5: Generate ses-NEW, write codeflow-env.sh
  STEP 6-14: Full initialization → EXIT C

Output: Previous session directory cleaned, new session started cleanly.
```

### Scenario G: Teammate crashes mid-work (lead handles recovery)

```text
Preconditions:
  Teammate process died unexpectedly (OOM, kill signal).
  Lead is still running.
  No SessionEnd fired for the teammate (process killed, not exited cleanly).
  OR: SessionEnd fired for the teammate, shouldSkipCleanup returned true
      because status="pf-in-progress" and team config exists.

Lead recovery path (next lead action):
  Lead sends SendMessage to the dead teammate (no response within 30s).
  Lead verifies via tmux: pane is dead.
  Lead respawns teammate with same name.
  New teammate fires SessionStart (source=startup).
  → Scenario B applies: CODEFLOW_SESSION_ID inherited → teammate detected.

The lead does NOT need to restart its own session. The SessionStart hook does
not interfere with the lead's running session when a teammate restarts.
```

### Scenario H: Lead resume

```text
Preconditions:
  source: "resume" (user typed /resume or restarted claude after exit)
  codeflow-env.sh: contains ses-EXISTING
  pathflow-session-status.json: { status:"pf-in-progress", ... }
  CODEFLOW_SESSION_ID: "" (new process)

Flow:
  STEP 0: Lock acquired
  STEP 1: existingSID = "ses-EXISTING"
  STEP 2: source=resume → STEP 2a (minimal path)
  STEP 2a: Read status file → output recovery context
    Set CODEFLOW_SESSION_ID = "ses-EXISTING"
    EXIT A

Note: Unlike the old code, resume does NOT call updateLeadPID in the new
      design. The lead_pid field in pathflow-team.json is no longer used for
      cleanup decisions (status file replaced it). Removing the PID update
      eliminates the risk of a teammate overwriting the lead PID on compact.
```

### Scenario I: Lead clear

```text
Preconditions:
  source: "clear" (user sent /clear)
  Functionally identical to Scenario H (resume).

Flow:
  STEP 2: source=clear → STEP 2a (minimal path)
  STEP 2a: Read status file → output recovery context
    Set CODEFLOW_SESSION_ID from env file
    EXIT A

No distinction between resume and clear in the redesigned minimal path.
```

---

## 4. Status transitions

The `pathflow-session-status.json` file is the shared state between SessionStart,
PostToolUse hooks, and SessionEnd. The following table maps each hook event to
the handler that updates the file and the resulting status transition.

```text
Status transition timeline for pathflow-session-status.json:

  SessionStart(startup) → createSessionStatus()
    → CREATE  { status:"created", session_id, created_at }

  TeamCreate PostToolUse → handle_team_create() / HandleTeamCreate()
    → UPDATE  status = "pf-started", team_name = "{name}"

  TaskCompleted(PF1 tasks complete) → checkpoint-complete hook
    → UPDATE  status = "pf-in-progress", last_completed_phase = "pf-1"

  TaskCompleted(PF{N} tasks complete) → checkpoint-complete hook
    → UPDATE  last_completed_phase = "pf-{N}"

  SendMessage("STAGE-COMPLETE: WS-{X}") → sentinel-write hook
    → UPDATE  last_completed_stage = "ws-{x}"

  TeamDelete PostToolUse → HandlePostTeamDelete() / handle_team_delete()
    → UPDATE  status = "pf-complete"

  SessionEnd → cleanSessionState()
    → DELETE  .state/session/{SID}/ (removes status file as part of dir)
```

Hook-to-handler mapping by implementation:

| Event | Go Handler | Go Location | Rust Handler | Rust Location |
|-------|-----------|-------------|-------------|---------------|
| SessionStart(startup) | `createSessionStatus` | `start.go` | `create_pathflow_flag` (old) | `session_start.rs` |
| TeamCreate PostToolUse | `HandleTeamCreate` | `sentinel/stage.go` | `handle_team_create` | `post_tool_use.rs` |
| checkpoint-complete | (planned) | `sentinel/checkpoint.go` | (planned) | `task_completed.rs` |
| sentinel-write stage | `HandleStageSentinel` | `sentinel/stage.go` | `handle_send_message` | `post_tool_use.rs` |
| TeamDelete PostToolUse | `HandlePostTeamDelete` | `team/guard.go` | `handle_team_delete` | `post_tool_use.rs` |
| SessionEnd cleanup | `cleanSessionState` | `end.go` | `clean_session_state` | `session_end.rs` |

**Implementation state (as of 2026-03-13):**

| Handler | Go status | Rust status |
|---------|-----------|-------------|
| `createSessionStatus` (status="created") | Implemented | Not yet (uses `create_pathflow_flag`) |
| `HandleTeamCreate` → status="pf-started" | Not yet implemented | Not yet implemented |
| checkpoint-complete → status="pf-in-progress" | Not yet implemented | Not yet implemented |
| `HandlePostTeamDelete` → status="pf-complete" | Implemented | Not yet (does not update status) |
| `shouldSkipCleanup` reads status file | Implemented | Not yet |

The `pf-started` and `pf-in-progress` transitions are planned but not yet
implemented in either Go or Rust. The existing `shouldSkipCleanup` in Go
handles these statuses in its switch statement but they will only appear in the
status file once the TeamCreate and checkpoint handlers are updated.

---

## 5. Dead code removal

The following code is identified for removal as part of the full redesign. All
items are in `codeflow-cli/internal/hooks/session/start.go` unless noted.

| Symbol | File | Reason for removal |
|--------|------|--------------------|
| `getClaudePID` | `start.go` | Only used to populate `PPID` for `updateLeadPID`. Removing PID update path removes the need for this function. |
| `ProcessChecker` interface | `start.go` | No longer called in `StartInit`. Was intended for PID-based liveness; replaced by status file. |
| `osProcessChecker` type | `start.go` | Concrete implementation of `ProcessChecker`. Removed with the interface. |
| `updateLeadPID` | `start.go` | Only called on `source=resume`. In the redesign, `lead_pid` in `pathflow-team.json` is no longer used for cleanup decisions. |
| `lead_pid` field (write) | `sentinel/stage.go` `HandleTeamCreate` | Written to `pathflow-team.json` but read nowhere in the active cleanup path. The field remains in the JSON schema for backward compat but is not used. |
| `TmuxChecker` (stale sweep) | `start.go` `sweepAllStaleSessions` | `hasLiveTeamPanes` uses `TmuxChecker`; the redesign replaces this check with status file age analysis. `TmuxChecker` is still used by `detectStaleTeams` for team config cleanup — not removed, but the sweep usage is refactored. |
| `PPID` field on `Initializer` | `start.go` | Set by `getClaudePID()`. Removable when `updateLeadPID` is removed. |

**Rust equivalent removals** (`codeflow-rs/codeflow-core/src/hooks/session_start.rs`):

| Symbol | Reason |
|--------|--------|
| `update_lead_pid` | Same rationale as Go |
| `ppid` field on `SessionStartInit` | Used only by `create_pathflow_flag` for embedding in the flag file; removing PID tracking from the flag file removes this dependency |

**What stays:**

| Symbol | Why retained |
|--------|-------------|
| `TmuxChecker` interface and `osTmuxChecker` | Still used by `detectStaleTeams` for cleaning team configs with all-dead panes |
| `pathflow-team.json` `lead_pid` field | Retained in JSON schema for backward compatibility; not removed, just not consulted |
| `handleNoTeamFile` | Handles the case where env file exists but no team file — still needed for pre-PF1 cleanup |
| `session.lock` | Lock acquisition is correct and should be retained |

---

## 6. Decision matrix

The table maps source type and caller role to the expected exit path.

| Source | Caller | envSID | Status | Exit |
|--------|--------|--------|--------|------|
| `startup` | Lead (fresh) | `""` | n/a (no file) | C (new SID) |
| `startup` | Lead (after crash) | `""` | `pf-in-progress` (old SID) | C (new SID) |
| `startup` | Lead (after clean PF7) | `""` | `pf-complete` (old SID) | C (new SID, old cleaned) |
| `startup` | Teammate (tmux/in-proc) | matches | `pf-in-progress` | B (teammate) |
| `compact` | Lead | any | any | A (minimal) |
| `compact` | Teammate | any | any | A (minimal) |
| `resume` | Lead | `""` | `pf-in-progress` | A (minimal) |
| `resume` | Teammate | matches | `pf-in-progress` | A (minimal) |
| `clear` | Lead | any | any | A (minimal) |
| `clear` | Teammate | any | any | A (minimal) |
| `unknown` | Any | `""` | n/a | C (new SID, treated as startup) |

**Priority phasing:**

| Priority | Change | Rationale |
|----------|--------|-----------|
| P0 | Route compact/resume/clear to minimal path (STEP 2a) | Prevents destructive ops on teammate compact |
| P0 | Status-based teammate detection (STEP 3) | Replaces tmux liveness; works for in-process agents |
| P1 | Status-based stale sweep (STEP 4) | Replaces tmux liveness in sweep; prevents false positives |
| P1 | Update HandleTeamCreate to write `pf-started` | Populates status transitions |
| P1 | Update checkpoint-complete to write `pf-in-progress` | Populates status transitions |
| P2 | Remove dead PID code | Cleanup after P0/P1 are stable |
| P2 | Port Go changes to Rust | Rust uses old `is-pathflow-active` flag; needs full migration |

---

## 7. Related files

**Go implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-cli/internal/hooks/session/start.go` | `StartInit`, `handleStaleCleanup`, `createSessionStatus`, `sweepAllStaleSessions` |
| `codeflow-cli/internal/hooks/session/end.go` | `shouldSkipCleanup`, 13-section cleanup flow |
| `codeflow-cli/internal/hooks/sentinel/stage.go` | `HandleTeamCreate`, `HandleTeammateSpawn`, `HandleStageSentinel` |
| `codeflow-cli/internal/hooks/team/guard.go` | `HandlePostTeamDelete`, `CheckTeamDelete` |
| `codeflow-cli/internal/hooks/sentinel/checkpoint.go` | Checkpoint registration and completion hooks |

**Rust implementation:**

| File | Relevance |
|------|-----------|
| `codeflow-rs/codeflow-core/src/hooks/session_start.rs` | `SessionStartInit::run`, `handle_stale_cleanup`, `create_pathflow_flag` (to be replaced) |
| `codeflow-rs/codeflow-core/src/hooks/session_end.rs` | `should_skip_cleanup`, `run` |
| `codeflow-rs/codeflow-core/src/hooks/post_tool_use.rs` | `handle_team_create`, `handle_team_delete`, `SentinelWrite` |

**Related analysis:**

| File | Relevance |
|------|-----------|
| `.codeflow/docs/analysis/session-end-cleanup-redesign.md` | SessionEnd shouldSkipCleanup redesign using status file |
| `.codeflow/docs/analysis/session-lifecycle-interaction.md` | Full start↔end lifecycle timeline |
| `.codeflow/docs/analysis/session-startup-cleanup-redesign.md` | Earlier session startup bug fixes (source-based PID branching) |
