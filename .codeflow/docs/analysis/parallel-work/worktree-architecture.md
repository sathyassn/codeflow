---
title: "Worktree Architecture"
type: analysis
status: draft
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# Worktree Architecture

[← Back to Overview](README.md)

## Table of Contents

- [1. Session-Scoped State Inventory](#1-session-scoped-state-inventory)
- [2. Existing Implementation](#2-existing-implementation)
- [3. Bugs in Existing Implementation](#3-bugs-in-existing-implementation)
- [4. Gaps in Existing Implementation](#4-gaps-in-existing-implementation)
- [5. Worktree at SessionStart](#5-worktree-at-sessionstart)
- [6. .claude/ Directory Strategy](#6-claude-directory-strategy)
- [7. Detached HEAD Worktree Pattern](#7-detached-head-worktree-pattern)
- [8. Worktree Cleanup at SessionEnd](#8-worktree-cleanup-at-sessionend)
- [9. Edge Cases](#9-edge-cases)
- [Appendix: Temp Path Convention](#appendix-temp-path-convention)

---

## 1. Session-Scoped State Inventory

This section inventories every piece of state written during a PathFlow session, classifying each by its conflict severity when two sessions run concurrently.

### 1.1 Conflict Severity Rating

| Severity | Meaning | Parallel Impact |
|----------|---------|----------------|
| CRITICAL | Direct data loss or corruption | Blocks parallelism entirely |
| HIGH | Incorrect behavior, wrong session context | Causes subtle bugs and wrong state |
| MEDIUM | Potential race condition | May cause intermittent failures |
| LOW | Cosmetic or recoverable | Acceptable with minor fixes |
| SAFE | Already session-scoped | No conflict |

### 1.2 State File Inventory

#### CRITICAL Severity

| File | Location | Written By | Conflict Behavior |
|------|----------|-----------|-------------------|
| `codeflow-env.sh` | `.state/runtime/codeflow-env.sh` | `session.WriteEnvFile()` at `session.go:301-321` | Last writer wins. Session B overwrites Session A's ID. `Current()` (`session.go:225-241`) returns wrong session ID for Session A. All subsequent operations (end, commit, checkpoint) reference the wrong session. |
| `/tmp/claude/{project}/` | System temp directory | `createProjectTempDir()` at `start.go:830-850` | `os.RemoveAll(tmpDir)` (line 846) destroys ALL contents -- including files from a concurrent session. No session scoping. |
| `active-task.json` | `.state/runtime/active-task.json` | `SetActiveTask()` at `activetask.go:39-63` | Last writer wins. Hooks reading this file (`GetActiveTask()` at `activetask.go:67-84`) get the wrong task context. Stage routing, sentinel naming, and checkpoint registration all use the wrong task. **Note:** active-task.json is NOT created at session start or PF1. It is created at PF4-TSK-02 (begin-work) by cf-knowledge-layer. StartInit only cleans up stale active-task.json (`start.go:276`, `cleanupActiveTask()`). |

#### HIGH Severity

| File | Location | Written By | Conflict Behavior |
|------|----------|-----------|-------------------|
| Team config | `~/.claude/teams/{name}/config.json` | Claude Code Agent Teams runtime | If both sessions use the same team name, config collides. Teammate pane references become invalid. |
| `state.json` | `.state/coordination/state.json` | `claim.Manager.saveDoc()` at `claim.go:110-133` | Non-atomic read-modify-write. Two sessions calling `Acquire()` concurrently can lose claims via TOCTOU race. Resolved by migrating to `state.loro` via Loro Map CRDT (sole coordination mechanism). |

#### MEDIUM Severity

| File | Location | Written By | Conflict Behavior |
|------|----------|-----------|-------------------|
| `pathflow-events.jsonl` | `.state/logs/pathflow-events.jsonl` | Session hooks | Log interleaving. Events from both sessions appear in one file. Readable but confusing for debugging. File-level flock protects against partial writes (I/O safety, not coordination). |
| Session metadata | `.state/logs/sessions/session-{SID}.meta` | `writeSessionMetadata()` at `start.go:687` | Inherently session-scoped by filename. SAFE for data integrity, but concurrent directory listings may see stale metadata from peer sessions. |

#### SAFE (Already Session-Scoped)

| File | Location | Written By | Notes |
|------|----------|-----------|-------|
| PathFlow flag | `.state/session/{SID}/pathflow/is-pathflow-active` | `createPathFlowFlag()` at `start.go:640-671` | Session ID in path prevents collision |
| Checkpoint | `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` | `initCheckpoint()` at `start.go:674-684` | Session ID in path prevents collision |
| Sentinels | `.state/sentinels/pathflow/{SID}/*` | PostToolUse hooks | Session ID in path prevents collision |
| Team JSON | `.state/session/{SID}/pathflow/pathflow-team.json` | Team initialization | Session ID in path prevents collision |

### 1.3 Stale Session Cleanup Conflict

`handleStaleCleanup()` at `start.go:334-387` performs PID-based stale detection. On startup, if a prior session's lead PID is dead, it calls `cleanupStaleSession()` (line 380) which removes the env file and optionally destroys the team.

**Parallel conflict:** If Session A starts while Session B is alive, the cleanup logic correctly detects Session B's PID as alive and enters teammate mode (line 372-374). However, if Session B dies and Session C starts, `cleanupStaleSession()` destroys Session B's leftover state, which is correct. The risk is a narrow race: Session B dies, Session C starts cleanup, but Session A (which was referencing Session B's env file) has not yet re-read it. Session A's `Current()` call returns a now-invalid session ID.

---

## 2. Existing Implementation

The worktree manager at `worktree.go:62-79` (Manager struct) and `worktree.go:110-191` (Setup method) already implements the core worktree creation flow:

1. **Git worktree creation:** `git worktree add {path} -b {branch}` at `worktree.go:126`
2. **Config file copying:** `.gitignore` copied to worktree at `worktree.go:135-143`
3. **Shared state symlinks:** `db`, `ledger`, `registry`, `backups`, `coordination`, `logs` symlinked from main `.state/` to worktree `.state/` at `worktree.go:153-164`
4. **Local state directories:** `runtime`, `session`, `sentinels` created fresh (not symlinked) at `worktree.go:167-173`
5. **Registry tracking:** Worktree registered in `.state/worktrees.yaml` at `worktree.go:175-187`

**Base directory:** `.git-worktrees/` (to be changed from current `.claude/worktrees/` -- see Section 3)
**Registry:** `.state/worktrees.yaml` (`worktree.go:90-94`)

### 2.1 State Split Assessment

The shared/local split at `worktree.go:153-167` is well-designed for parallel work:

| Shared (symlinked) | Why Shared | Parallel Safety |
|--------------------|-----------|----------------|
| `db/` | Single SQLite database | WAL mode + busy_timeout handle concurrent access |
| `ledger/` | Append-only JSONL files | flock for file I/O append safety (not coordination) |
| `coordination/` | Claims state | Loro Map CRDT (sole coordination mechanism, native crate dependency in Rust CLI). Replaces state.json (see [Data Layer Protection](data-layer-protection.md) and [CRDT Coordination](crdt-coordination.md)). |
| `logs/` | Event logs | Append-only, flock for file I/O append safety (not coordination) |
| `registry/` | Worktree tracking | Single writer expected |
| `backups/` | DB backups | Timestamped filenames prevent collision |

| Local (per-worktree) | Why Local | Parallel Safety |
|---------------------|----------|----------------|
| `runtime/` | Session files (env, active-task) | Each worktree gets its own -- eliminates CRITICAL conflicts |
| `session/` | Session-scoped state | Each worktree gets its own |
| `sentinels/` | Phase/stage sentinels | Each worktree gets its own |

---

## 3. Bugs in Existing Implementation

### Worktree Base Directory Mismatch

**Bug:** The worktree manager default base directory is `.claude/worktrees/` (`worktree.go:98-102`), but `.gitignore` line 67 ignores `.git-worktrees/`. This mismatch means worktree directories could be accidentally committed to git.

**Fix:** Change the code default from `.claude/worktrees/` to `.git-worktrees/` at `worktree.go:102`. The `.gitignore` entry is already correct.

**Rationale for `.git-worktrees/` over `.claude/worktrees/`:**
- `.gitignore:67` already excludes `.git-worktrees/` -- the code is the mismatch, not the gitignore
- `.claude/` is Claude Code configuration space (agents, settings, skills, memory) -- worktrees are runtime artifacts, not configuration
- `.git-worktrees/` at project root uses git-adjacent naming (like `.gitignore`, `.gitattributes`), clearly communicating purpose
- No conflict with `.git/worktrees/` (git's internal metadata for worktree tracking) -- completely different paths
- Claude Code SDK's `EnterWorktree` tool uses `.claude/worktrees/`, but CodeFlow has its own worktree manager with a separate base directory

**Recommendation:** Change code to use `.git-worktrees/` and leave `.gitignore` as-is. Add this as Task #1 in Phase A (see [Decisions](decisions.md#15-worktree-base-directory)).

---

## 4. Gaps in Existing Implementation

| Gap | Description | Severity |
|-----|-------------|----------|
| No PathFlow integration | `Setup()` is not called from any PathFlow phase or hook | Blocking |
| Branch name required | `Setup()` requires `branch` parameter (line 114-116) -- no detached HEAD support | Minor (solved by `--detach` pattern, see Section 7) |
| No `.claude/` handling | Worktree does not copy or symlink `.claude/` directory (agents, settings, hooks) | Needs analysis (see Section 6) |
| No autorun integration | `worker.go:109` uses `WorkDir: "."` -- never calls worktree Setup | Blocking (see [Autorun Integration](autorun-integration.md)) |
| No project temp dir scoping | `createProjectTempDir()` uses project name, not session/worktree name | Critical |
| No `.codeflow/` handling | Test suite and config directories not addressed | Medium |

---

## 5. Worktree at SessionStart (NOT PF1-INIT)

Worktree creation MUST happen in the SessionStart hook (`start.go:StartInit()`), BEFORE any PathFlow phase begins. If worktree creation happens at PF1-INIT, then the pathflow-phase-tasks checkpoint file, pathflow-active flag, and PF1 sentinels would all be written to the main project's `.state/` directories and later need migration into the worktree -- creating unnecessary complexity and race conditions.

By creating the worktree at SessionStart:
- The session ID is generated first (StartInit Section 2, `start.go:220-261`)
- The worktree is created using the session ID as the name
- `codeflow-env.sh` is written INSIDE the worktree's `.state/runtime/`
- The pathflow-active flag (`start.go:640-671`) is created inside the worktree's `.state/session/{SID}/`
- The checkpoint file (`start.go:674-684`) is initialized inside the worktree's `.state/session/{SID}/`
- All subsequent PathFlow phases (PF1 through PF7) execute within the worktree from the start

**Updated initialization flow:**

```text
SessionStart hook (start.go:StartInit)
    |
    v
Section 1: Parse stdin JSON
    |
    v
Section 1b: PID-based stale cleanup
    |
    v
Section 2: Session ID generation (SID)
    |
    v
NEW Section 2b: Worktree creation
    1. Manager.SetupDetached("worktree-{SID}")
       -> git worktree add --detach .git-worktrees/worktree-{SID}
       -> .state/ created with shared symlinks + local dirs
    2. Switch working context to worktree path
    3. Set CODEFLOW_WORKTREE_PATH in result.EnvVars
    |
    v
Section 2c: Write codeflow-env.sh INSIDE worktree
    -> .git-worktrees/worktree-{SID}/.state/runtime/codeflow-env.sh
    -> Contains CODEFLOW_SESSION_ID, CF_PROJECT_ROOT, CODEFLOW_WORKTREE_PATH
    |
    v
Section 3-12: All remaining init steps run in worktree context
    -> Directories created in worktree .state/
    -> PathFlow flag in worktree .state/session/{SID}/
    -> Checkpoint in worktree .state/session/{SID}/
    -> Temp dir: /tmp/claude/{project}/{worktree-name}/managed/protected-edits/
    |
    v
All PathFlow phases (PF1 through PF7) execute in worktree
    |
    v
PF3-CLASSIFY: git checkout -b {prefix}/{name} (inside worktree)
    -> Attaches a branch to the previously detached HEAD
    |
    v
PF7-END: Teammate shutdown, team deletion (does NOT remove worktree)
    |
    v
SessionEnd hook (end.go:EndCleanup)
    -> Worktree cleanup: Manager.Cleanup("worktree-{SID}")
    -> Removes worktree directory and deregisters from worktrees.yaml
    -> Handles abnormal termination (crash without PF7)
```

---

## 6. `.claude/` Directory Strategy

The `.claude/` directory contains files with varying write characteristics:

| Path | Write Behavior | Protection | Worktree Strategy |
|------|---------------|------------|-------------------|
| `agents/*.md` | Written by cf-documentation during DOCS pipeline | No denyWrite rule | Shared via git worktree (tracked in git) |
| `commands/*.md` | Rarely modified | No denyWrite rule | Shared via git worktree |
| `skills/*/SKILL.md` | Rarely modified | No denyWrite rule | Shared via git worktree |
| `settings.json` | Modified by Claude Code IDE settings | Has denyWithinAllow sandbox rule | Shared via git worktree |
| `settings.local.json` | Modified by user | Has denyWithinAllow sandbox rule | Shared via git worktree |
| `memory/*.md` | Written during PF6-COMPLETE by cf-knowledge-layer | No denyWrite rule | Per-worktree copy via git worktree (tracked in git). Git merge handles reconciliation. |
| (worktrees moved to `.git-worktrees/`) | Managed by worktree.Manager | Gitignored (`.gitignore:67`) | N/A -- no longer under `.claude/` (see Section 3, [Decision #15](decisions.md#15-worktree-base-directory)) |
| `hooks/project/` | Project-specific hook scripts | No denyWrite rule | Shared via git worktree |
| `CLAUDE.md` | Team lead instructions | Has denyWithinAllow sandbox rule | Shared via git worktree |

Since `.claude/` is tracked in git, `git worktree add` automatically makes all tracked files available in the worktree. No explicit copying or symlinking is needed for tracked content.

---

## 7. Detached HEAD Worktree Pattern

**Problem:** The current `Manager.Setup()` at `worktree.go:126` hardcodes `git worktree add {path} -b {branch}`, requiring a branch name at creation time. In the PathFlow lifecycle, the branch name is not known until PF3-CLASSIFY (work type determines the branch prefix). Worktree creation needs to happen at SessionStart for proper state isolation from the start.

**Solution:** `git worktree add --detach <path>` creates a worktree with a detached HEAD at the current HEAD commit. No branch is needed at creation time. Later, inside the worktree, `git checkout -b <branch>` creates and attaches a branch.

**Implementation:** Add a new `SetupDetached(name string)` method to the Manager:

```go
func (m *Manager) SetupDetached(name string) (*Worktree, error) {
    // git worktree add --detach {path}
    // Same shared/local symlink logic as Setup()
    // Branch field left empty in registry, updated at PF3
}
```

**Detached HEAD and commits before PF3:** Between SessionStart and PF3-CLASSIFY, no code changes should be committed (Edit/Write is gated on the pf-3 sentinel). However, state files (pathflow flag, checkpoint, active-task) are written to the worktree's local `.state/` directories, which are not committed to git. The detached HEAD state is safe because:
- State files are in `.state/` (gitignored, not committed)
- No source code modifications happen before PF3
- The pf-3 Edit/Write gate prevents premature code changes
- At PF3, `git checkout -b {prefix}/{name}` creates the branch and all subsequent commits go to that branch

**Environment variable lifecycle:**

| Event | CODEFLOW_WORKTREE_PATH |
|-------|----------------------|
| SessionStart (startup) | Set to worktree path, written to codeflow-env.sh |
| Compact/Resume | Re-read from codeflow-env.sh, re-exported |
| Clear | Reset (new session will create new worktree) |
| SessionEnd | Used to locate worktree for cleanup, then removed |

**codeflow-env.sh with worktree support:**

```bash
export CODEFLOW_SESSION_ID='ses-01kjxabc123'
export CF_PROJECT_ROOT='codeflow'
export CODEFLOW_WORKTREE_PATH='/path/to/.git-worktrees/worktree-ses-01kjxabc123'
```

---

## 8. Worktree Cleanup at SessionEnd

Worktree cleanup happens in the SessionEnd hook (`end.go:EndCleanup()`), NOT at PF7-END. This separation ensures:

1. **Normal flow:** PF7-END handles teammate shutdown and team deletion. SessionEnd hook (which fires after PF7) handles worktree removal.
2. **Abnormal termination:** If the session crashes without reaching PF7, the SessionEnd hook still fires and cleans up the worktree.
3. **No orphan worktrees:** The SessionEnd hook always runs (Claude Code guarantees this), so worktrees are always cleaned up.

**Implementation:** Add to `end.go:EndCleanup()` after Section 12 (project temp cleanup):

```text
Section 13: Worktree cleanup
    1. Read CODEFLOW_WORKTREE_PATH from codeflow-env.sh
    2. If present, call Manager.Cleanup(worktree-name, CleanupOpts{Force: true})
    3. git worktree prune (clean up stale references)
```

---

## 9. Edge Cases

### 9.1 Compact / Resume Events

When Claude Code compacts or resumes a session, the SessionStart hook fires again with `source=compact` or `source=resume`. The worktree already exists from the initial `source=startup` call.

**Expected behavior:** `handleStaleCleanup()` at `start.go:334-387` detects the existing session via `codeflow-env.sh`, finds the lead PID alive (line 372), and returns the existing session ID without cleanup. The worktree path is re-read from `codeflow-env.sh` and re-exported via `result.EnvVars`.

**Risk:** None. The worktree is already set up. Compact/resume do not recreate or modify it.

**Implementation requirement:** The new worktree creation code in Section 2b of `StartInit()` MUST check `source` before creating a worktree. Only `source=startup` creates a new worktree. Compact/resume must detect the existing worktree path from `codeflow-env.sh` and reuse it.

### 9.2 Abnormal Termination (Crash Without PF7)

If the session crashes without reaching PF7-END (e.g., machine sleep, network loss, OOM kill):

1. **Worktree persists on disk** at `.git-worktrees/worktree-{SID}/`
2. **No branch cleanup** -- the worktree's branch (if created at PF3) remains in `git branch` output
3. **Stale env file** -- `codeflow-env.sh` still references the dead session

**Recovery path:** On the next `source=startup` session:
- `handleStaleCleanup()` reads the old session's env file
- Finds the lead PID dead (line 372 check fails)
- Calls `cleanupStaleSession()` at line 380, which removes the env file
- **Gap:** `cleanupStaleSession()` currently does NOT clean up the worktree directory. This must be added: after removing the env file, check for and remove the stale worktree.

**Implementation requirement:** Extend `cleanupStaleSession()` to:
1. Read `CODEFLOW_WORKTREE_PATH` from the env file before deleting it
2. If present, call `git worktree remove --force {path}`
3. Call `git worktree prune` to clean stale internal references

### 9.3 Two Sessions Starting Simultaneously

If two `source=startup` sessions start at the same time on the same project:

1. **Session ID collision:** Impossible. Session IDs are ULID-based (`session.generateID()` at `session.go:40-42`), which encode millisecond timestamps plus random entropy. Two ULIDs generated in the same millisecond differ in the random suffix.
2. **Worktree name collision:** Impossible. Worktree names include the session ID (`worktree-{SID}`), so unique IDs produce unique worktree paths.
3. **`codeflow-env.sh` race:** CRITICAL. Both sessions write to the same `.state/runtime/codeflow-env.sh`. Session B overwrites Session A's ID. This is the exact CRITICAL conflict from Section 1.2. **Resolution:** With worktrees, each session writes `codeflow-env.sh` to its own worktree's `.state/runtime/`, not the shared location. This eliminates the race entirely.
4. **SQLite contention:** Both worktrees share the same SQLite database via symlink. WAL mode with `busy_timeout=5000` (connection.go:98) handles concurrent writes. `SetMaxOpenConns(1)` (connection.go:92) serializes per-process. Cross-process contention is handled by SQLite's file-level locking. Expected behavior: one session waits up to 5 seconds for the other's write to complete.
5. **JSONL contention:** Both worktrees share the same ledger directory via symlink. `writer.go:99` uses `flock(LOCK_EX)` for file I/O append safety (this is file-level locking, not coordination). Expected behavior: one session blocks briefly while the other holds the lock. No data loss.
6. **Claims contention:** Both worktrees share `coordination/` via symlink. With Loro Map CRDT (Phase A), claims are coordinated atomically -- concurrent `Acquire()` calls produce deterministic merge via Loro's conflict resolution. The current state.json TOCTOU race is eliminated by the migration to `state.loro` (see [Data Layer Protection](data-layer-protection.md) Section 3).

### 9.4 Worktree Cleanup Race at SessionEnd

If Session A's `EndCleanup()` runs while Session B is actively using shared state (ledger, database):

**SQLite:** Safe. Removing Session A's worktree removes the symlink to `.state/db/`, but does NOT affect the actual database file or Session B's connection to it. SQLite connections hold file descriptors to the actual file, not to the symlink.

**JSONL:** Safe. Same reasoning -- removing the symlink does not affect Session B's open file descriptors to the actual ledger files.

**git worktree remove:** Safe. `git worktree remove` only removes the worktree directory and its entry in `.git/worktrees/`. It does not affect other worktrees or the main repository.

### 9.5 Loro Feature Availability

With the pure Rust CLI (Epic 0), Loro is a compiled-in crate dependency -- there is no separate library to load or fail. The Loro CRDT engine is always available when the `codeflow` binary runs. This eliminates the "library not found" failure mode that existed in the earlier Rust shared library + CGo FFI architecture.

**Single-session without parallel features:** If the Loro coordination module encounters an unexpected error (corrupt `state.loro`, disk full), the CLI should:
- Emit a warning to stderr: `"codeflow: coordination state unavailable, parallel work disabled"`
- Allow single-session operation (SQLite WAL and JSONL file-level append locking continue to work for single-session use)
- Block parallel session creation (claims require functional Loro state)

**Implementation requirement:** Both "coordination healthy" and "coordination degraded" paths need integration tests.

---

## Appendix: Temp Path Convention

Parallel sessions use session-scoped temp directories:

```text
/tmp/claude/{project-folder-name}/{worktree-name}/managed/protected-edits/
```

Example: `/tmp/claude/codeflow/worktree-ses-01kjxabc123/managed/protected-edits/`

The worktree name includes the session ID, making the path inherently unique across concurrent sessions. The `os.RemoveAll()` in `createProjectTempDir()` must be changed to only clean the session's own temp subdirectory, not the project-level directory.

---

[← Back to Overview](README.md)
