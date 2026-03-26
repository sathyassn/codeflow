---
title: "Worktree Cleanup Safety"
type: analysis
status: active
author: cf-documentation
created_at: "2026-03-22"
updated_at: "2026-03-22"
parent: "parallel-work/README.md"
related: "worktree-architecture.md"
---

# Worktree Cleanup Safety

[← Back to Overview](README.md)

## Table of Contents

- [1. Background](#1-background)
- [2. Cleanup Chain Gaps (Before Fix)](#2-cleanup-chain-gaps-before-fix)
- [3. Rescue-Before-Delete Design](#3-rescue-before-delete-design)
- [4. Orphan Detection](#4-orphan-detection)
- [5. Checkpoint Path Resolution Bug](#5-checkpoint-path-resolution-bug)
- [6. Staging Path Isolation](#6-staging-path-isolation)
- [7. Worktree Path Protection (Pending)](#7-worktree-path-protection-pending)
- [8. Lock File Cleanup](#8-lock-file-cleanup)
- [9. Registry Dedup](#9-registry-dedup)
- [10. Before/After Cleanup Flow](#10-beforeafter-cleanup-flow)
- [11. Heartbeat-Based Liveness Detection](#11-heartbeat-based-liveness-detection)
- [12. Implementation Status Summary](#12-implementation-status-summary)

---

## 1. Background

When CodeFlow runs sessions in isolated git worktrees (`.git-worktrees/worktree-{SID}/`),
each worktree holds live work: uncommitted code, PathFlow state files, and staged
protected-edits. A session crash — machine sleep, OOM kill, or network loss — leaves the
worktree on disk with no cleanup path unless the SessionEnd hook fires.

This document records the gaps found in the original cleanup implementation, the fixes
applied in this session (branch `fix/worktree-cleanup-safety`), and one remaining gap
that requires a follow-on task.

Source modules referenced throughout this document:

| Module | Path |
|--------|------|
| Worktree cleanup | `codeflow-cli/core/src/worktree/cleanup.rs` |
| Worktree setup | `codeflow-cli/core/src/worktree/setup.rs` |
| Session start hook | `codeflow-cli/core/src/hooks/session_start.rs` |
| Session end hook | `codeflow-cli/core/src/hooks/session_end.rs` |
| Worktree registry | `codeflow-cli/core/src/worktree/registry.rs` |
| Session env file | `codeflow-cli/core/src/session/env.rs` |

---

## 2. Cleanup Chain Gaps (Before Fix)

### 2.1 SessionEnd Depends on Env Var, Silently Skips

**Problem:** `EndCleanup` in `session_end.rs` reads `CODEFLOW_WORKTREE_PATH` from the
process environment to determine which worktree to clean up. When a hook process is
spawned for the SessionEnd event, the environment variable is populated from
`result.env_vars` in the SessionStart response. However, if the session crashed and
Claude Code spawns a new process to handle the SessionEnd hook, the env var is not set.
The cleanup call silently returns early without removing the worktree.

**Impact:** Stale worktrees accumulate on disk. On a machine with many crashes, the
`.git-worktrees/` directory fills with abandoned directories.

### 2.2 `clean_stale_worktrees()` Only Checks Registry Entries

**Problem:** `clean_stale_worktrees()` in `session_start.rs` iterates over
`worktrees.yaml` and force-cleans any entry whose `lead_pid` is dead. However, it only
processes entries that appear in the registry. Worktrees that were created but never
registered — or whose registry entries were removed without cleaning the directory — are
completely invisible to this function.

**Example scenario:**

1. Session starts, creates worktree directory.
2. `locked_register_with_limit()` call is interrupted before the registry write completes.
3. The directory exists on disk but no registry entry exists.
4. `clean_stale_worktrees()` never visits it. It persists indefinitely.

### 2.3 `sweep_all_stale_sessions()` Removes Session State But Not Worktrees

**Problem:** `sweep_all_stale_sessions()` in `session_start.rs` removes stale session
state (PathFlow flags, checkpoint files, env file) when it finds dead `lead_pid` values
in `pathflow-session-status.json`. It correctly reclaims `.state/session/{SID}/` state.
However, it does not look up or clean the corresponding worktree directory.

**Impact:** Session state is removed (preventing stale context leaks) but the
git worktree — including all working tree files, branch references, and `.state/` local
symlinks — remains on disk.

### 2.4 No Work-Safety Check Before Force Deletion

**Problem:** Force cleanup (`CleanupOpts { force: true }`) previously called
`fs::remove_dir_all()` directly on the worktree path with no check for uncommitted or
unpushed work. In the normal flow this is safe (PF7-END commits and pushes before
cleanup). In crash scenarios, in-progress code is destroyed without any save attempt.

**Impact:** Work written by the agent but not yet committed is lost when a crash triggers
stale cleanup on the next startup.

---

## 3. Rescue-Before-Delete Design

### 3.1 `rescue_uncommitted_work()` Function

`rescue_uncommitted_work()` in `cleanup.rs` (line 88) runs as the first step of force
cleanup, before `fs::remove_dir_all()` is called. The function:

1. Checks whether the worktree path contains a git repo (`.git` file presence).
2. If yes, runs `git status --porcelain` to detect dirty files.
3. If dirty: stages all files with `git add -A` and creates a WIP commit:
   `"wip: auto-save from crashed session {session_hint}"`.
4. Checks `git log origin/{branch}..HEAD` to detect unpushed commits.
5. If unpushed: attempts `git push origin {branch}`.
6. If push succeeds: returns `Ok(())` — safe to delete.
7. If push fails: returns `Err(WorktreeError::UnpushedWork(...))`, blocking deletion.

### 3.2 `WorktreeError::UnpushedWork` Blocks Deletion

When `rescue_uncommitted_work()` returns `Err(WorktreeError::UnpushedWork)`, the
`cleanup_worktree()` function propagates that error immediately. The force-remove path
(`fs::remove_dir_all()`) is never reached.

This preserves crash-safety equivalent to non-worktree mode: if the network is
unavailable when cleanup runs, the worktree survives until push becomes possible.

### 3.3 Detached HEAD Handling

When the worktree is on a detached HEAD (SessionStart ran but PF3-CLASSIFY did not create
a branch), `git rev-parse --abbrev-ref HEAD` returns `"HEAD"`. In this state:

- If the worktree has dirty files (uncommitted WIP), `rescue_uncommitted_work()` returns
  `Err(WorktreeError::UnpushedWork)` because there is no branch to push to.
- The worktree is preserved with its uncommitted state intact.
- The user must manually inspect and clean up the worktree in this case.

### 3.4 Cleanup Flow with Rescue

```text
cleanup_worktree(name, force=true)
    │
    ├─ PathFlow guard (skip if force=true)
    │
    ├─ rescue_uncommitted_work(wt_path, session_hint)
    │    ├─ Not a git repo? → Ok(()) → proceed to delete
    │    ├─ Clean? → Ok(()) → proceed to delete
    │    ├─ Dirty files → stage + WIP commit
    │    │    ├─ On a branch → attempt push
    │    │    │    ├─ Push ok → Ok(()) → proceed to delete
    │    │    │    └─ Push fail → Err(UnpushedWork) → BLOCK DELETE
    │    │    └─ Detached HEAD → Err(UnpushedWork) → BLOCK DELETE
    │    └─ No dirty, unpushed commits → attempt push (same flow)
    │
    ├─ remove_via_git2(name, force=true)
    └─ fs::remove_dir_all(wt_path) if still exists
```

---

## 4. Orphan Detection

### 4.1 What Is an Orphan Worktree

An orphan worktree is a directory under `.git-worktrees/` that:

- Exists on disk (was created by `create_detached_worktree()` or `create_worktree()`).
- Has no corresponding entry in `.state/worktrees.yaml`, OR has an entry with
  `status != "active"`.

Orphans arise when:

1. `locked_register_with_limit()` was interrupted after `repo.worktree()` but before
   the YAML write completed (partial creation).
2. A registry entry was manually removed or corrupted.
3. The worktree was cloned or copied outside of `WorktreeManager`.

### 4.2 Orphan Detection Implementation

`clean_orphaned_worktrees()` in `session_start.rs` (line 576) runs at the end of
`clean_stale_worktrees()`. It:

1. Collects the set of registered directory names from the registry (active entries only).
2. Scans `.git-worktrees/` for `worktree-*` named directories not in the registered set.
3. For each unregistered directory:
   a. Checks the file creation/modification time — skips if younger than 5 minutes
      (grace period for in-progress initialization).
   b. Reads `{wt_path}/.state/session/*/pathflow/pathflow-session-status.json` to check
      `lead_pid` liveness.
   c. If `lead_pid` is dead (or the status file is missing): force-cleans the worktree
      via `WorktreeManager::cleanup(name, CleanupOpts { force: true })`.
   d. If `lead_pid` is alive: leaves the worktree intact (session is initializing).

### 4.3 Grace Period Rationale

A 5-minute grace period protects worktrees that are being created concurrently by another
session. Between `repo.worktree()` and `locked_register_with_limit()`, the worktree
exists on disk but is not yet in the registry. Without the grace period, a concurrent
startup could detect and destroy the nascent worktree.

The 5-minute window is conservative: `create_detached_worktree()` completes in under a
second in practice. Any worktree older than 5 minutes without a registry entry is
definitively an orphan.

---

## 5. Checkpoint Path Resolution Bug

### 5.1 Root Cause

`CODEFLOW_WORKTREE_PATH` is written to two locations during SessionStart:

1. `result.env_vars` HashMap — exported to Claude Code as environment variables for the
   session process.
2. `{worktree}/.state/runtime/codeflow-env.sh` — written by
   `session::write_env_file_with_worktree()` (line 334 in `session_start.rs`).

Hook processes spawned by Claude Code (PreToolUse, PostToolUse, TaskCompleted) inherit
the environment from `result.env_vars`. However, when hooks read the env file to
determine the project directory, they call `session::current_session_id()` which reads
from `{project_dir}/.state/runtime/codeflow-env.sh` — the main project path, not the
worktree.

The consequence: PostToolUse hooks that use `project_dir` to resolve sentinel and
checkpoint paths fall back to the main project's `.state/` rather than the worktree's
local `.state/`. This creates split-brain state:

| State type | Written to |
|------------|-----------|
| Worktree env file | `{worktree}/.state/runtime/codeflow-env.sh` |
| Sentinel files | `{worktree}/.state/sentinels/` (local, correct) |
| Checkpoint file | `{worktree}/.state/session/{SID}/pathflow/` (local, correct) |
| PostToolUse sentinel-write | Falls back to `{main_repo}/.state/sentinels/` (wrong) |
| PostToolUse checkpoint-register | Falls back to `{main_repo}/.state/session/` (wrong) |

### 5.2 Fix: `detect_project_dir()` Reads `codeflow-env.sh` as Fallback

The fix adds a fallback in the project directory resolution path used by PostToolUse
hooks. When `CODEFLOW_WORKTREE_PATH` is set in the process environment (which it is,
because `result.env_vars` propagates to hook processes), the hook resolves its working
paths relative to the worktree root rather than CWD.

This ensures sentinel files and checkpoint updates land in the worktree's local `.state/`
directories — consistent with where SessionStart created them.

---

## 6. Staging Path Isolation

### 6.1 The Collision Problem

Protected-edits staging uses the path:

```text
/tmp/claude/{project}/managed/protected-edits/
```

In single-session mode, this path is unique per project. With parallel sessions in
worktrees, multiple sessions share the same project name, causing the staging directories
to collide. Session A's staged file is visible to Session B; if both sessions stage the
same filename, they overwrite each other's work.

### 6.2 Fix: `WorktreePaths::temp_dir()` Scopes by Worktree Name

`WorktreePaths::temp_dir()` (verified in `session_start.rs` test
`test_create_project_temp_dir_with_worktree`) constructs the temp path as:

```text
/tmp/claude/{project}/{worktree-name}/managed/
```

Example: `/tmp/claude/codeflow/worktree-ses-01kmca6xw1j375mnkstb93vksq/managed/`

Since worktree names embed the session ID (e.g., `worktree-ses-01kmca6xw1j375mnkstb93vksq`),
the path is unique per session. Sessions cannot observe or overwrite each other's staged
files.

The worktree-scoped path is used when `worktree_paths` is `Some` in
`create_project_temp_dir()` (line 1098 in `session_start.rs`). When no worktree is
active (single-session mode), the original project-level path is used, preserving
backward compatibility.

**SessionEnd cleanup:** `SessionEndCleanup` removes only the session-scoped subdirectory
(e.g., `worktree-ses-222`), not the entire project temp dir. Other sessions' directories
(e.g., `worktree-ses-111`, `worktree-ses-333`) survive, as verified by the
`test_cleanup_project_temp_dir_with_worktree` test in `session_end.rs`.

---

## 7. Worktree Path Protection (Pending)

### 7.1 Problem: Relative Patterns Miss Worktree Paths

The `enforcement-policy.json` protection patterns are written as relative paths:

```json
"critical": [
  ".state/session/**",
  ".state/sentinels/**",
  ...
]
```

When a session runs in main-repo mode, these patterns match correctly against the project
root. When a session runs in worktree mode (`project_dir = main repo`, but working files
are under `.git-worktrees/worktree-{SID}/`), the actual file paths written during work
normalize to `.git-worktrees/worktree-{SID}/.state/session/**` — which does not match
the relative pattern `.state/session/**`.

**Impact:** The protection-guard hook may allow writes to worktree session state files
that it would block in main-repo mode.

### 7.2 Proposed Fix: `worktree_protection` Section in `enforcement-policy.json`

The fix adds a new configurable `worktree_protection` section to `enforcement-policy.json`
with an `expansion_prefix` field:

```json
"worktree_protection": {
  "expansion_prefix": ".git-worktrees/worktree-{SID}",
  "apply_to_tiers": ["critical", "high"]
}
```

When `CODEFLOW_WORKTREE_PATH` is set, the protection-guard hook expands each pattern with
the worktree-relative prefix before matching, ensuring coverage is equivalent to
main-repo mode.

**Status:** This fix is tracked as task #21 (Priority 2e) and has NOT yet been
implemented in this session. The current `enforcement-policy.json` does not contain a
`worktree_protection` section.

---

## 8. Lock File Cleanup

### 8.1 Problem: Orphaned `.lock` Files Block Subsequent Sessions

`acquire_session_lock()` creates `session.lock` in `.state/runtime/` using
`flock(LOCK_EX)`. The lock is held for the duration of session initialization and
released via `drop(lock_file)`. If the process crashes while holding the lock, the
`.lock` file remains on disk.

On subsequent startups, the new process acquires a new exclusive lock (because the old
lock is released when the old file descriptor is closed by OS cleanup). However, orphaned
`.lock` files accumulate and clutter the directory.

The same issue exists for `.lock` files in session pathflow directories:
- `.state/session/{SID}/pathflow/pathflow-phase-tasks.lock`
- `.state/session/{SID}/pathflow/pathflow-session-status.lock`
- `.state/session/{SID}/pathflow/pathflow-team.lock`

### 8.2 Fix: `clean_lock_files_in_dir()` in SessionStart

`clean_lock_files_in_dir()` (line 32 in `session_start.rs`) scans a directory and
removes any file with a `.lock` extension. It is called during `clean_stale_worktrees()`
to clean lock files from session pathflow directories before force-cleaning the worktree.

```rust
fn clean_lock_files_in_dir(dir: &Path) {
    if !dir.exists() { return; }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lock") {
                let _ = fs::remove_file(&path);
            }
        }
    }
}
```

The function is intentionally silent on errors — lock file cleanup is best-effort and
must not block the session startup flow.

---

## 9. Registry Dedup

### 9.1 Problem: Duplicate Entries in `worktrees.yaml`

`locked_register_with_limit()` uses a locked read-modify-write pattern on `worktrees.yaml`.
Before this fix, a race between two concurrent calls registering the same worktree name
could produce duplicate entries: both calls read the same version of the file (zero
entries for this name), both add an entry, and one write overwrites the other.

**Impact:** Two entries with `name == "worktree-{SID}"` appear in the registry. The
limit check (max 3 concurrent) counts both, over-reporting active worktrees and blocking
new session creation when capacity is not actually exhausted.

### 9.2 Fix: Dedup Check in `locked_register_with_limit()`

The fix (line 246 in `registry.rs`) checks for an existing active entry with the same
name before appending:

```rust
// Check for existing entry with same name (dedup).
if let Some(existing) = reg.worktrees.iter_mut()
    .find(|e| e.name == entry_owned.name && e.status == "active")
{
    // Update existing entry fields instead of appending.
    *existing = entry_owned.clone();
    return Ok(());
}
```

When a duplicate name is detected, the function updates the existing entry in-place
rather than appending a new one. This is idempotent: registering the same worktree twice
produces exactly one active entry.

---

## 10. Before/After Cleanup Flow

### 10.1 Before: Unsafe Cleanup Chain

```text
SessionEnd fires
    │
    ├─ Read CODEFLOW_WORKTREE_PATH from env
    │    └─ If not set (crash recovery) → SILENT SKIP, worktree left on disk
    │
    ├─ If set: call WorktreeManager::cleanup(name, force=true)
    │    └─ Force: fs::remove_dir_all() immediately
    │         └─ Uncommitted work DESTROYED with no save attempt
    │
Next startup (clean_stale_worktrees):
    │
    ├─ Iterate worktrees.yaml entries
    │    └─ Force-clean entries with dead lead_pid
    │         └─ Orphans (not in registry) INVISIBLE, persist forever
    │
    └─ No orphan scan, no lock cleanup, no registry dedup
```

### 10.2 After: Safe Cleanup Chain

```text
SessionEnd fires
    │
    ├─ Read CODEFLOW_WORKTREE_PATH from env file (codeflow-env.sh) as fallback
    │    └─ If still not found → log warning, attempt deferred cleanup at next startup
    │
    ├─ If found: call WorktreeManager::cleanup(name, force=true)
    │    ├─ rescue_uncommitted_work() FIRST:
    │    │    ├─ Dirty files? → stage + WIP commit
    │    │    ├─ Unpushed? → attempt push
    │    │    │    ├─ Push ok → safe to delete
    │    │    │    └─ Push fail → BLOCK delete, return UnpushedWork error
    │    │    └─ Clean → safe to delete
    │    └─ fs::remove_dir_all() only if rescue_uncommitted_work() returned Ok
    │
Next startup (clean_stale_worktrees):
    │
    ├─ Iterate worktrees.yaml entries
    │    ├─ Force-clean entries with dead lead_pid (same as before)
    │    └─ clean_lock_files_in_dir() for session pathflow dirs
    │
    ├─ clean_orphaned_worktrees() — NEW
    │    ├─ Scan .git-worktrees/ for worktree-* dirs not in registry
    │    ├─ Skip dirs younger than 5 minutes (grace period)
    │    ├─ Check lead_pid liveness in pathflow-session-status.json
    │    └─ Force-clean confirmed-dead orphans
    │
    ├─ git worktree prune (stale git refs)
    │
PostToolUse hooks (checkpoint-register, sentinel-write):
    │
    ├─ BEFORE: fall back to main-repo .state/ when worktree not in CWD
    └─ AFTER: read CODEFLOW_WORKTREE_PATH from codeflow-env.sh fallback
              → resolve sentinel/checkpoint paths relative to worktree

Staging (protected-edits):
    ├─ BEFORE: /tmp/claude/{project}/managed/protected-edits/ (shared across sessions)
    └─ AFTER: /tmp/claude/{project}/{worktree-name}/managed/ (per-session isolated)

Registry writes (locked_register_with_limit):
    ├─ BEFORE: duplicate entries possible under race conditions
    └─ AFTER: dedup check prevents duplicates, idempotent registration
```

---

## 11. Heartbeat-Based Liveness Detection

### Problem: lead_pid is Unreliable

The `lead_pid` stored in `pathflow-session-status.json` is obtained via `parent_id()` in the
hook process. The hook process hierarchy is:

```text
Claude Code (grandparent) → sh -c (parent) → codeflow hooks ... (us)
```

`parent_id()` returns the `sh -c` shell PID, which exits immediately after the hook completes.
This means `lead_pid` is always dead by the time `clean_stale_worktrees()` checks it, causing
every active session to appear stale.

### Solution: Heartbeat File

A heartbeat file at `.state/runtime/heartbeat` is updated on every hook invocation (throttled
to once per 5 seconds). The file contains JSON with `session_id`, `timestamp`, and `source`.

| Module | Path |
|--------|------|
| Heartbeat API | `codeflow-cli/core/src/session/heartbeat.rs` |
| Process utilities | `codeflow-cli/core/src/session/process.rs` |
| Hook injection | `codeflow-cli/cli/src/helpers.rs` (`touch_heartbeat()`) |

**Heartbeat lifecycle:**

1. **Created:** On first hook invocation after session start (SessionStart init handler)
2. **Updated:** On every subsequent hook invocation (throttled: skip if mtime < 5s ago)
3. **Removed:** By SessionEnd cleanup handler (`heartbeat::remove()`)

**Liveness check pattern (applied at all stale sweep sites):**

```rust
// Primary: heartbeat-based liveness
if heartbeat::is_alive(&project_dir, threshold_secs) {
    // Session definitely alive
} else if session::process::is_process_alive(lead_pid) {
    // Secondary: PID still alive (defense-in-depth)
} else {
    // Session is stale — safe to clean
}
```

### 3-State Worktree Registry

The worktree registry now supports three status values:

| Status | Meaning | Set By |
|--------|---------|--------|
| `active` | Session owns this worktree | `locked_register_with_limit()` at creation |
| `completing` | PathFlow done, session may still be alive briefly | `handle_team_delete()` via `mark_completing()` |
| `removed` | Worktree cleaned up | Cleanup operations |

Stale sweep logic:
1. If heartbeat exists AND timestamp <= threshold: ALIVE, skip
2. If heartbeat missing: clean exit (SessionEnd deleted it), safe to clean
3. If heartbeat exists AND timestamp > threshold: crashed/abandoned, safe to clean
4. Secondary: also check `lead_pid` as defense-in-depth

### Process Tree Walking

`get_claude_code_pid()` walks the process tree to find the grandparent PID (the actual Claude
Code process) instead of using `parent_id()` which returns the ephemeral `sh -c` shell.
This is used in `handle_team_create()` and CLI session start/end handlers for accurate
`lead_pid` recording.

### Configuration

`stale_heartbeat_threshold_secs` in `.codeflow/config/parallel-work/parallel-work-config.json`
(autorun section) controls the heartbeat staleness threshold. Default: 86400 seconds (24 hours).

---

## 12. Implementation Status Summary

| Gap | Fix | Status |
|-----|-----|--------|
| SessionEnd silently skips cleanup when env var not set | Read `codeflow-env.sh` as fallback | Implemented |
| `clean_stale_worktrees()` misses orphan worktrees | `clean_orphaned_worktrees()` added | Implemented |
| `sweep_all_stale_sessions()` does not clean worktrees | Now calls `WorktreeManager::cleanup()` | Implemented |
| No safety check before force deletion | `rescue_uncommitted_work()` runs first | Implemented |
| `WorktreeError::UnpushedWork` blocks deletion when network unavailable | Error propagated, deletion blocked | Implemented |
| Checkpoint/sentinel hooks fall back to main repo under worktree | `detect_project_dir()` reads `codeflow-env.sh` fallback | Implemented |
| Staging tmp dir shared across parallel sessions | `WorktreePaths::temp_dir()` scopes by worktree name | Implemented |
| Lock files accumulate after crashes | `clean_lock_files_in_dir()` cleans `.lock` files at startup | Implemented |
| Registry duplicate entries under concurrent registration | Dedup check in `locked_register_with_limit()` | Implemented |
| Protection patterns miss worktree-relative paths | `worktree_protection` section in `enforcement-policy.json` | **Pending — task #21** |
| `lead_pid` stores ephemeral shell PID, always appears dead | Heartbeat file + `get_claude_code_pid()` process tree walk | Implemented |
| Stale sweep uses PID-only liveness, false positives | Heartbeat primary + PID secondary liveness check | Implemented |
| Registry has only active/removed, no transitional state | Added `completing` status via `mark_completing()` | Implemented |

---

[← Back to Overview](README.md)
