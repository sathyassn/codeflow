---
title: "Session Startup Cleanup Redesign"
type: analysis
status: final
date: 2026-03-12
area: infrastructure
scope:
  - codeflow-cli/internal/hooks/session/start.go
  - codeflow-cli/internal/hooks/session/start_test.go
  - codeflow-cli/internal/hooks/session/end.go
  - codeflow-cli/internal/hooks/team/guard.go
  - codeflow-rs/codeflow-core/src/hooks/session_start.rs
---

# Session Startup Cleanup Redesign

This document records the analysis and solution for three interrelated bugs in the
Go CLI session startup code, and documents the fixes applied to both Go and Rust
implementations.

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. Problem Analysis](#2-problem-analysis)
  - [Bug 1: PID Overwrite Race in handleStaleCleanup](#bug-1-pid-overwrite-race-in-handlestalecleanup)
  - [Bug 2: detectStaleSessions is Warn-Only](#bug-2-detectstalesessions-is-warn-only)
  - [Bug 3: sweepOrphanSentinels is a No-Op](#bug-3-sweeporphansentinels-is-a-no-op)
- [3. Process Model](#3-process-model)
- [4. Solution Design](#4-solution-design)
  - [Fix 1: Source-Based PID Branching in handleStaleCleanup](#fix-1-source-based-pid-branching-in-handlestalecleanup)
  - [Fix 2: New sweepAllStaleSessions Function](#fix-2-new-sweepallistalesessions-function)
  - [Fix 3: detectStaleTeams Converted to Clean](#fix-3-detectstaleteams-converted-to-clean)
- [5. SessionEnd Integration](#5-sessionend-integration)
- [6. Change Matrix](#6-change-matrix)
- [7. Rust Port Notes](#7-rust-port-notes)
- [8. Test Coverage](#8-test-coverage)

## 1. Executive Summary

Three interrelated bugs in the Go CLI session startup code (`start.go`) caused
session directory proliferation, broken teammate detection, and potential session
corruption. The PID overwrite race allowed surviving teammates to claim leadership
after a lead crash, preventing cleanup of the lead's session. Two cleanup functions
were effectively dead code: `detectStaleSessions` warned about orphans but never
removed them, and `sweepOrphanSentinels` targeted a condition (sentinel dirs
without session dirs) that `createDirectories` made impossible to trigger in
practice. The fix introduced source-based branching in `handleStaleCleanup`
(compact/clear do not update the lead PID; only resume does), replaced both dead
functions with a single `sweepAllStaleSessions` that runs on startup and actually
removes stale sessions, and converted `detectStaleTeams` from warn-only to
performing actual cleanup. The same fixes were ported to the Rust implementation in
`codeflow-rs/codeflow-core/src/hooks/session_start.rs`.

## 2. Problem Analysis

### Bug 1: PID Overwrite Race in handleStaleCleanup

**Location:** `codeflow-cli/internal/hooks/session/start.go`, `handleStaleCleanup`
function

**Symptom:** When a teammate's session compact or clear fires and the lead PID is
dead, `updateLeadPID` ran unconditionally (in the pre-fix code), allowing the
teammate to overwrite the lead PID in `pathflow-team.json` with its own process ID.

**Root cause:** The original code did not distinguish between `source=compact`,
`source=clear`, and `source=resume` when the lead PID is dead. All three were
treated identically: dead PID, not startup, so update the PID and continue.

**Impact:**

- Corrupted `pathflow-team.json`: a teammate PID was written as the lead PID
- Broken teammate detection in `SessionEnd`: `shouldSkipCleanup` reads the lead
  PID and, finding a live teammate PID, concludes the lead is alive and skips
  cleanup
- The original session's state directories are never cleaned up
- On observed systems, this accumulated 93 orphan session directories

**The race:**

```
Timeline: Lead crashes, teammates survive

t0: Lead (PID 1000) creates pathflow-team.json {lead_pid: 1000}
t1: Lead crashes (PID 1000 dies)
t2: Teammate A (PID 2000) context compacts -> source="compact"
    handleStaleCleanup (pre-fix): PID 1000 dead + source=compact
    -> updateLeadPID -> writes {lead_pid: 2000}   <- BUG
t3: Teammate B's SessionEnd: reads PID 2000, PID 2000 alive
    -> "lead alive, skip cleanup" -> session NEVER cleaned up

Result: .state/session/{old-SID}/ and all artifacts remain forever
        New sessions accumulate alongside old orphans
```

### Bug 2: detectStaleSessions is Warn-Only

**Location:** `detectStaleSessions` function (removed in the fix)

**Symptom:** The function scanned all `ses-*` directories under `.state/session/`,
identified sessions where the lead PID was dead or the team file was absent, and
appended warning strings to the result. It never removed anything.

**Root cause:** The function only implemented detection. Actual cleanup relied on
users running `/cf-cleanup --sessions` manually. The function's name (`detect`)
reflected this advisory intent, but advisory-only cleanup is insufficient for a
system that must recover automatically from lead crashes.

**Impact:** Session directory proliferation. In one observed instance, 93 orphan
directories accumulated under `.state/session/` because no automated cleanup was
running.

### Bug 3: sweepOrphanSentinels is a No-Op

**Location:** `sweepOrphanSentinels` function (removed in the fix)

**Symptom:** The function scanned `.state/sentinels/pathflow/` for sentinel
directories with no matching session directory, then removed them. In practice it
never removed anything meaningful.

**Root cause:** `createDirectories` always creates both the session directory
(`.state/session/{SID}`) and the sentinel directory (`.state/sentinels/pathflow/{SID}`)
in the same call. Because both directories are created atomically by the same
function, a sentinel directory without a matching session directory cannot arise
from normal operation.

The actual source of orphan sentinels is the absence of cleanup (Bug 2): when a
session is abandoned, both its session directory and its sentinel directory
accumulate together. Removing sentinel dirs without session dirs (as
`sweepOrphanSentinels` targeted) does not match the real failure mode where both
exist and neither is cleaned.

**Impact:** False sense of cleanup coverage. The function existed and ran but did
not address any real-world orphan scenario.

## 3. Process Model

### StartInit Execution Flow

```
SessionStart Hook -> StartInit()
|
+-- Section 1: Parse stdin JSON (session_id, source, transcript_path)
|
+-- Section 1b: handleStaleCleanup(projectDir, envFilePath, source)
|   |
|   +-- Read codeflow-env.sh -> get oldSID
|   +-- Validate oldSID format (ses-{legacy|ULID})
|   +-- Check pathflow-team.json for oldSID
|   |   |
|   |   +-- Lead PID alive?
|   |   |   YES -> return (oldSID, teamMode=true)   [teammate path]
|   |   |
|   |   +-- Lead PID dead + source=startup/unknown
|   |   |   -> cleanupStaleSession -> return ("", false)
|   |   |
|   |   +-- Lead PID dead + source=resume
|   |   |   -> updateLeadPID -> return (oldSID, false)   [user relaunched]
|   |   |
|   |   +-- Lead PID dead + source=compact/clear
|   |       -> return (oldSID, false)   [surviving teammate, no PID update]
|   |
|   +-- No team file -> handleNoTeamFile
|       +-- No flag -> remove env file -> return ("", false)
|       +-- Flag exists + startup/unknown -> cleanup -> return ("", false)
|       +-- Flag exists + compact/resume/clear -> return (oldSID, false)
|
+-- [if teamMode] -> return early with TEAMMATE MODE result
|
+-- Section 2: Session ID resolution
|   +-- existingSID from handleStaleCleanup? -> use it
|   +-- CODEFLOW_SESSION_ID env var set? -> use it
|   +-- source=startup/unknown -> SessionStarter.StartSession -> new ULID SID
|   +-- else -> read codeflow-env.sh fallback -> error if not found
|
+-- Section 3: createDirectories(projectDir, sessionID)
|   Creates: .state/logs/sessions, .state/logs/security, .state/db,
|            .state/runtime, .state/sentinels/pathflow/{SID},
|            .state/session/{SID}
|
+-- Section 4+5: sweepAllStaleSessions (startup/unknown only)
|   +-- Scan all ses-* dirs under .state/session/ (skip currentSID)
|   |   For each:
|   |   +-- No pathflow-team.json -> removeStaleSessionArtifacts
|   |   +-- Invalid JSON -> removeStaleSessionArtifacts
|   |   +-- Lead PID alive -> skip
|   |   +-- Lead PID dead or zero -> removeStaleSessionArtifacts
|   |
|   +-- Scan all ses-* dirs under .state/sentinels/pathflow/ (skip currentSID)
|       +-- No matching session dir -> remove sentinel dir
|
+-- Section 6: cleanupActiveTask
|   +-- status=completed/done -> remove active-task.json
|   +-- updated_at > 24h ago -> remove active-task.json
|   +-- otherwise -> preserve
|
+-- Section 7: createPathFlowFlag
|   +-- Flag exists -> return isResume=true
|   +-- Flag absent -> create {session_id, created_at, tracking_level:"pending"}
|
+-- Section 7c: initCheckpoint
|   -> pathflow-phase-tasks.json pre-initialized for 7 phases
|
+-- Section 8: writeSessionMetadata
|   -> .state/logs/sessions/session-{SID}.meta
|
+-- Section 9: detectStaleTeams
|   +-- Scan ~/.claude/teams/ for team configs
|   +-- All tmux panes dead? -> remove team dir + task dir
|
+-- Section 10: detectCompactRecovery
|   -> advisory messages if compact/resume/clear with active team config
|
+-- Section 11: createProjectTempDir
|   -> /tmp/claude/{project-name}/
|
+-- Section 12: autoRebuildCLI (startup only)
    -> rebuild if binary VCS revision differs from git HEAD
```

### Source-Based Behavior Table

| Source | SID Behavior | PID Update | Stale Sweep | Use Case |
|--------|-------------|------------|-------------|----------|
| `startup` | Generate new | N/A (no existing team) | YES (full sweep) | Fresh /exit -> new session |
| `compact` | Reuse existing | NO (teammate may reach this) | No | Context window compaction |
| `clear` | Reuse existing | NO (teammate may reach this) | No | User sends /clear |
| `resume` | Reuse existing | YES (user relaunched claude) | No | Session resume |
| `unknown` | Generate new | N/A | YES (full sweep) | Fallback (treated as startup) |

The `compact` and `clear` cases must NOT update the lead PID because the claude
process does not restart on compact or clear. If the lead PID is dead during compact
or clear, this session startup was triggered by a surviving teammate's context
overflow, not by a user relaunching the lead. Updating the PID would incorrectly
claim leadership.

The `resume` case SHOULD update the lead PID because `resume` means the user
explicitly typed `/resume` in a new claude process. The new process has a new PID,
and the existing session should recognize this new PID as the lead.

## 4. Solution Design

### Fix 1: Source-Based PID Branching in handleStaleCleanup

**Before (pre-fix behavior):**

The original code updated the PID for any non-startup source when the lead PID was
dead:

```go
// Pre-fix: dead PID + not startup -> always update lead PID
if source != "startup" && source != "unknown" {
    init_.updateLeadPID(teamFilePath, teamData)
    return oldSID, false, nil
}
```

This logic treated compact, clear, and resume identically. A surviving teammate's
compact or clear would overwrite the lead PID.

**After (current behavior in `start.go`):**

```go
// Lead PID is dead.
if source == "startup" || source == "unknown" {
    // Fresh startup with dead PID -- full cleanup.
    init_.cleanupStaleSession(projectDir, envFilePath, oldSID, teamInfo.TeamName)
    return "", false, nil
}

// Resume with dead PID -- update lead_pid (user relaunched claude).
if source == "resume" {
    init_.updateLeadPID(teamFilePath, teamData)
    return oldSID, false, nil
}

// Compact/clear with dead PID -- don't update PID.
// On compact/clear, the claude process does NOT restart. If the lead PID
// is dead and source is compact/clear, this caller is a surviving tmux
// teammate whose lead died. Updating the PID would incorrectly claim
// leadership.
return oldSID, false, nil
```

The branching logic is now explicit for each source value:

- `startup`/`unknown`: full cleanup, generate new SID
- `resume`: update PID (user relaunched), reuse SID
- `compact`/`clear`: do NOT update PID, reuse SID

### Fix 2: New sweepAllStaleSessions Function

The two pre-fix functions `detectStaleSessions` (warn-only) and
`sweepOrphanSentinels` (no-op) were removed and replaced by a single
`sweepAllStaleSessions` function that actually cleans up stale state.

**What it does:**

1. Scans all `ses-*` directories under `.state/session/` (excluding `currentSID`)
2. For each directory:
   - If no `pathflow-team.json` exists: call `removeStaleSessionArtifacts`
   - If JSON is invalid: call `removeStaleSessionArtifacts`
   - If lead PID is alive: skip (active session)
   - If lead PID is dead or zero: call `removeStaleSessionArtifacts`
3. Scans all `ses-*` directories under `.state/sentinels/pathflow/` (excluding
   `currentSID`)
4. For each sentinel directory with no matching session directory: remove it

`removeStaleSessionArtifacts` removes three things: the session directory, the
corresponding sentinel directory, and (if `teamName` is set) the team config and
task list directories under `~/.claude/teams/` and `~/.claude/tasks/`.

**Only runs on startup/unknown:** The sweep is expensive (scans all session dirs)
and is only needed when starting fresh. Compact, resume, and clear callers reuse
an existing session and do not benefit from a full sweep.

**`removeStaleSessionArtifacts` helper** (in `start.go`):

```go
func (init_ *Initializer) removeStaleSessionArtifacts(projectDir, sid, teamName string) {
    _ = os.RemoveAll(filepath.Join(projectDir, ".state", "session", sid))
    _ = os.RemoveAll(filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid))
    if teamName != "" {
        _ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "teams", teamName))
        _ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
    }
}
```

### Fix 3: detectStaleTeams Converted to Clean

The pre-fix `detectStaleTeams` scanned `~/.claude/teams/` for team configs with
dead tmux panes and emitted warning strings. It never removed anything.

The fixed version now performs actual cleanup when all panes are dead:

```go
// If ALL members are dead, remove the team directory and task list.
if alive == 0 {
    _ = os.RemoveAll(filepath.Join(teamsDir, teamName))
    _ = os.RemoveAll(filepath.Join(init_.HomeDir, ".claude", "tasks", teamName))
    warnings = append(warnings,
        fmt.Sprintf("STALE TEAM CLEANED: '%s' (%d members, all panes dead)",
            teamName, len(cfg.Members)))
}
```

The condition changed from "count dead panes > 0" (any dead pane triggers cleanup)
to "all panes dead" (`alive == 0`). A team with some dead panes but at least one
alive pane may still be active. Only a team where every pane is dead is truly stale.

## 5. SessionEnd Integration

`end.go` works in coordination with the startup fixes. Its key function is
`shouldSkipCleanup` (in the `Cleaner` type), which protects teammates from running
cleanup when the lead is still alive.

**shouldSkipCleanup logic (end.go):**

1. If no `is-pathflow-active` flag exists: proceed with cleanup (no active session)
2. If `pathflow-team.json` is missing or unreadable: proceed with cleanup
3. If `lead_pid` is zero or negative: proceed with cleanup
4. If `lead_pid == PPID` (this is the lead's own SessionEnd): proceed with cleanup
5. If lead PID is alive: **skip cleanup** (teammate shutdown, lead handles it)
6. If lead PID is dead: proceed with cleanup (orphaned session)

This function is what Bug 1 corrupted. When a teammate overwrote the lead PID with
its own alive PID, check 5 returned true, blocking cleanup for the orphaned
session indefinitely.

**cleanPathflowSentinels** (end.go): removes the sentinel directory
`.state/sentinels/pathflow/{sessionID}/` for the current session only. Does not
touch other sessions' sentinels; that is `sweepAllStaleSessions`'s job on the
next startup.

**HandlePostTeamDelete** in `guard.go`: called PostToolUse after a successful
`TeamDelete`. Performs three operations in order:

1. Removes the sentinel directory for the session
2. Removes `pathflow-team.json` from the session pathflow dir
3. Resets `pathflow-phase-tasks.json` to fresh state via `ResetAllPhases`

The `is-pathflow-active` flag is intentionally NOT removed by
`HandlePostTeamDelete`. That flag is session-scoped and managed separately by the
PathFlow lifecycle.

## 6. Change Matrix

| File | Change | Function | Description |
|------|--------|----------|-------------|
| `start.go` | Modified | `handleStaleCleanup` | Source-based PID branching: compact/clear do not update lead PID; resume does |
| `start.go` | Added | `sweepAllStaleSessions` | Consolidated replacement for warn-only and no-op functions |
| `start.go` | Added | `removeStaleSessionArtifacts` | Helper: removes session dir, sentinel dir, team artifacts |
| `start.go` | Removed | `detectStaleSessions` | Replaced by `sweepAllStaleSessions` |
| `start.go` | Removed | `isSessionStale` | Logic integrated into `sweepAllStaleSessions` |
| `start.go` | Removed | `sweepOrphanSentinels` | Integrated into `sweepAllStaleSessions` |
| `start.go` | Modified | `detectStaleTeams` | Changed from warn-only to active cleanup; condition changed from "any dead pane" to "all panes dead" |
| `start.go` | Modified | `StartInit` Section 4+5 | Calls `sweepAllStaleSessions` on startup instead of removed functions |
| `start_test.go` | Modified | `TestStartInit_CompactContinuation` | Verifies PID is NOT updated on compact with dead lead |
| `start_test.go` | Added | `TestStartInit_ResumeContinuation` | Verifies PID IS updated on resume with dead lead |
| `start_test.go` | Added | `TestSweepAllStaleSessions` | Comprehensive sub-tests: dead PID cleanup, alive session skip, no-team-file cleanup, orphan sentinel sweep |
| `start_test.go` | Removed | `TestDetectStaleSessions_NoStale` | Tests removed method |
| `start_test.go` | Removed | `TestDetectStaleSessions_NonSessionDir` | Tests removed method |
| `start_test.go` | Modified | `TestDetectStaleTeams` | Updated to verify actual directory removal, not just warning messages |
| `session_start.rs` | Modified | `handle_stale_cleanup` | Equivalent source-based PID branching |
| `session_start.rs` | Added | `sweep_all_stale_sessions` | Equivalent consolidated sweep |
| `session_start.rs` | Added | `remove_stale_session_artifacts` | Equivalent cleanup helper |
| `session_start.rs` | Modified | `detect_stale_teams` | Equivalent warn-to-clean conversion |
| `session_start.rs` | Modified | `run` Section 4+5 | Calls `sweep_all_stale_sessions` on startup |

## 7. Rust Port Notes

The Rust implementation in
`codeflow-rs/codeflow-core/src/hooks/session_start.rs` mirrors the Go
implementation and received the same fixes.

**handle_stale_cleanup** applies the identical branching logic. When the lead PID
is dead and `source == "resume"`, it calls `self.update_lead_pid`. When `source`
is `compact` or `clear`, it returns `(Some(old_sid), false)` without updating the
PID:

```rust
// Compact/clear with dead PID -- don't update PID.
// The caller is a surviving tmux teammate whose lead died.
return (Some(old_sid), false);
```

**sweep_all_stale_sessions** implements the same two-pass sweep using
`fs::read_dir`. The idiomatic Rust pattern uses `.flatten()` to skip directory
entry errors rather than propagating them, matching the Go `continue` pattern:

```rust
for entry in entries.flatten() {
    if !entry.file_type().is_ok_and(|t| t.is_dir()) {
        continue;
    }
    let name = entry.file_name().to_string_lossy().to_string();
    if !name.starts_with("ses-") || name == current_sid {
        continue;
    }
    // ... same logic as Go
}
```

**Type differences:** The Rust version uses `SessionId` (a newtype wrapper) rather
than `String` for session IDs, enforcing format validation at the type level. The
`PathflowTeamInfo` struct uses snake_case field names (`lead_pid`, `team_name`)
consistent with serde_json deserialization of the JSON schema.

**Error handling:** Where Go uses `_ = os.RemoveAll(...)` (ignore errors), Rust
uses `let _ = fs::remove_dir_all(...)`. Both treat cleanup failures as non-fatal
and continue with the next item.

**TmuxChecker trait:** The Rust equivalent of `osTmuxChecker` implements the
`TmuxChecker` trait. The `detect_stale_teams` method uses the same condition:
all-panes-dead before removing the team directory.

## 8. Test Coverage

The test suite in `start_test.go` covers each fix with targeted test functions.

**Bug 1 coverage (PID branching):**

| Test | Scenario | Assertion |
|------|----------|-----------|
| `TestStartInit_CompactContinuation` | Dead lead PID + source=compact | `lead_pid` remains the original dead PID (99998), NOT updated to test PPID (99999) |
| `TestStartInit_ResumeContinuation` | Dead lead PID + source=resume | `lead_pid` is updated to test PPID (99999) |
| `TestStartInit_TeammateMode` | Alive lead PID | `IsTeammate=true`, returns early with teammate messages |
| `TestStartInit_StaleSessionCleanup` | Dead lead PID + source=startup | New SID generated, stale artifacts removed |

**Bug 2 coverage (sweep replaces warn-only):**

The `TestSweepAllStaleSessions` function uses sub-tests:

| Sub-test | Scenario | Assertion |
|----------|----------|-----------|
| `cleans_stale_session_with_dead_pid` | Stale session with dead PID | Session dir, sentinel dir, and team dir all removed |
| `skips_alive_session` | Session with alive PID | Session dir preserved |
| `cleans_session_without_team_file` | Session dir but no team file | Session dir removed |
| `sweeps_orphan_sentinels` | Sentinel dir with no matching session dir | Orphan sentinel removed; sentinel with alive-PID session preserved |

`TestSweepAllStaleSessions_CleansStaleFlagOnly` verifies a session with an
`is-pathflow-active` flag but no team file is treated as stale and removed.

**Bug 3 coverage (detectStaleTeams now cleans):**

`TestDetectStaleTeams` was updated to assert that the team directory is actually
removed when all panes are dead, not just that a warning string is returned.

**Test infrastructure:**

Tests use injectable interfaces (`ProcessChecker`, `TmuxChecker`,
`SessionStarter`) via `newTestInitializer`, which sets all dependencies to mock
implementations with deterministic behavior. This allows tests to control PID
liveness, pane liveness, and session ID generation without hitting the real OS.
The test PPID is fixed at 99999, allowing assertions about whether `lead_pid` was
updated to the caller's process ID.
