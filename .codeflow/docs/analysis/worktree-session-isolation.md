---
type: analysis
title: "Worktree Session Isolation Design"
area: infrastructure
status: draft
created_at: "2026-04-06"
author: cf-documentation
---

# Worktree Session Isolation Design

## Table of Contents

- [1. Problem Statement](#1-problem-statement)
  - [1.1 What Happened](#11-what-happened)
  - [1.2 Root Cause: Shared codeflow-env.sh Singleton](#12-root-cause-shared-codeflow-envsh-singleton)
  - [1.3 Why PID-Based Env Files Do Not Solve It](#13-why-pid-based-env-files-do-not-solve-it)
  - [1.4 Why Teammate Detection Is Unreliable Without Env Var Linkage](#14-why-teammate-detection-is-unreliable-without-env-var-linkage)
- [2. Design Principles](#2-design-principles)
- [3. Solution: codeflow interactive Command](#3-solution-codeflow-interactive-command)
  - [3.1 Command Interface](#31-command-interface)
  - [3.2 CLI Phase (Complete Chain)](#32-cli-phase-complete-chain)
  - [3.3 Shared Code with Autorun](#33-shared-code-with-autorun)
- [4. SessionStart Hook Changes](#4-sessionstart-hook-changes)
  - [4.1 Current Flow (Before)](#41-current-flow-before)
  - [4.2 New Flow (After)](#42-new-flow-after)
  - [4.3 CODEFLOW_MANAGED Path](#43-codeflow_managed-path)
  - [4.4 Unmanaged Path (Plain claude)](#44-unmanaged-path-plain-claude)
  - [4.5 Session ID Lifecycle](#45-session-id-lifecycle)
- [5. Hook Audit: What Changes Per Hook](#5-hook-audit-what-changes-per-hook)
  - [5.1 SessionStart (Major Changes)](#51-sessionstart-major-changes)
  - [5.2 SessionEnd (Remove PID Cleanup)](#52-sessionend-remove-pid-cleanup)
  - [5.3 PreToolUse (No Changes)](#53-pretooluse-no-changes)
  - [5.4 PostToolUse (Add Branch Update at pf-3)](#54-posttooluse-add-branch-update-at-pf-3)
  - [5.5 TaskCompleted (Add Branch Update at pf-3)](#55-taskcompleted-add-branch-update-at-pf-3)
  - [5.6 Other Hooks (No Changes)](#56-other-hooks-no-changes)
- [6. Eliminated Components](#6-eliminated-components)
  - [6.1 PID-Based Env Files (Removed)](#61-pid-based-env-files-removed)
  - [6.2 Shared codeflow-env.sh in Worktree Mode (Removed)](#62-shared-codeflow-envsh-in-worktree-mode-removed)
  - [6.3 detect_project_dir() Simplification](#63-detect_project_dir-simplification)
  - [6.4 Teammate Detection via pending_tmux_count (Removed)](#64-teammate-detection-via-pending_tmux_count-removed)
- [7. Scenario Analysis](#7-scenario-analysis)
  - [7.1 Non-Worktree Interactive (Plain claude)](#71-non-worktree-interactive-plain-claude)
  - [7.2 Single codeflow -i Session](#72-single-codeflow--i-session)
  - [7.3 Two Parallel codeflow -i Sessions](#73-two-parallel-codeflow--i-sessions)
  - [7.4 In-Process Teammates](#74-in-process-teammates)
  - [7.5 tmux Teammates (CWD Inheritance)](#75-tmux-teammates-cwd-inheritance)
  - [7.6 Autorun Workers](#76-autorun-workers)
  - [7.7 Context Overflow Recovery](#77-context-overflow-recovery)
  - [7.8 plain claude While codeflow -i Running](#78-plain-claude-while-codeflow--i-running)
  - [7.9 Max Worktrees Reached](#79-max-worktrees-reached)
- [8. Race Condition Analysis](#8-race-condition-analysis)
  - [8.1 Parallel Worktree Registration](#81-parallel-worktree-registration)
  - [8.2 tmux CWD Inheritance](#82-tmux-cwd-inheritance)
  - [8.3 Context Overflow Env Persistence](#83-context-overflow-env-persistence)
  - [8.4 Stale Session Cleanup](#84-stale-session-cleanup)
- [9. PID/Heartbeat Liveness: What Remains](#9-pidheartbeat-liveness-what-remains)
- [10. File Change Map](#10-file-change-map)
  - [10.1 New Files](#101-new-files)
  - [10.2 Modified Files](#102-modified-files)
  - [10.3 Deleted Code](#103-deleted-code)
- [11. Implementation Plan](#11-implementation-plan)
- [12. Related](#12-related)

---

## 1. Problem Statement

### 1.1 What Happened

Two parallel interactive Claude Code sessions running simultaneously caused state corruption. Both sessions shared the same `codeflow-env.sh` file at `.state/runtime/codeflow-env.sh`. When the second session started, it overwrote that file with its own `CODEFLOW_SESSION_ID`. Any hook call from the first session then read the wrong session ID — sentinel files, phase checkpoints, and WorkGraph events were attributed to the second session.

Concrete failure sequence:

```
Session A starts  →  writes CODEFLOW_SESSION_ID=ses-AAA to codeflow-env.sh
Session B starts  →  overwrites with CODEFLOW_SESSION_ID=ses-BBB

Session A hook fires  →  reads codeflow-env.sh  →  gets ses-BBB
Session A creates sentinel  →  stored under ses-BBB/
Session B checks sentinel  →  finds its own sentinel already created
Session A phase gate check  →  looks under ses-AAA/  →  sentinel missing  →  BLOCKED
```

Result: session A's PathFlow is permanently corrupted. Phase gates block operations that should be allowed.

### 1.2 Root Cause: Shared codeflow-env.sh Singleton

`codeflow-env.sh` at `.state/runtime/codeflow-env.sh` was designed as the "single source of truth" for the current session. That design assumes at most one active interactive session at a time. With worktree-isolated parallel sessions this assumption no longer holds.

Every `SessionStart` hook unconditionally overwrites the file:

```rust
// session_start.rs — current behavior (broken for parallel sessions)
write_env_file_with_worktree(project_dir, session_id, worktree_path)?;
// ^ always writes to .state/runtime/codeflow-env.sh
```

Every hook resolves the project directory by reading that same file:

```rust
// helpers.rs detect_project_dir() — current steps 1-4
// Step 4: read codeflow-env-{pid}.sh (per-PID attempt)
// Falls back to shared codeflow-env.sh
```

The shared file is a global mutable singleton. Two writers guarantee corruption.

### 1.3 Why PID-Based Env Files Do Not Solve It

The current codebase has a partial mitigation: per-PID env files at
`.state/runtime/codeflow-env-{pid}.sh`. The idea is that each Claude Code process writes to its own PID-keyed file, so reads are isolated.

This approach fails for several reasons:

**Problem 1: PID is not known at hook-spawn time.**
Hooks are spawned as child processes of Claude Code. The child process cannot reliably determine its Claude Code parent's PID in all environments. On macOS, `ppid` is sometimes the shell pid, not Claude Code's pid. The existing `get_claude_code_pid()` uses heuristics that are fragile.

**Problem 2: PID files accumulate and are never cleaned up reliably.**
A crashed session leaves its PID file. The next session may read it during fallback and get the old worktree path. The cleanup logic (`session_end.rs`) only runs when the session ends cleanly.

**Problem 3: The shared fallback remains.**
When the per-PID file is not found, the code falls back to reading the shared `codeflow-env.sh`. This fallback reintroduces the corruption vector. The fallback cannot be removed without breaking backward compatibility for sessions started before per-PID files were introduced.

**Problem 4: Teammate detection depends on PID.**
The current teammate detection logic checks `pending_tmux_count` and PID comparisons to determine whether a `SessionStart` firing is from the lead or a teammate. This logic is intertwined with PID file writing, making the whole system fragile when PIDs are unreliable.

**Root insight:** PID-based isolation is a workaround for the wrong problem. The real fix is to ensure each Claude Code process has its env var set **before it starts**, not discovered after.

### 1.4 Why Teammate Detection Is Unreliable Without Env Var Linkage

Teammate detection today relies on comparing the `lead_pid` from `pathflow-team.json` against the current process's perceived PID. This works when:

- tmux panes have deterministic PID assignment
- The PID written by `handle_team_create` matches what the teammate sees as its parent

It fails when:

- Shell wrappers interpose between Claude Code and the hook (PID is the shell, not Claude)
- Context overflow creates a new Claude Code process with a different PID
- In-process subagent teammates do not have a separate PID at all

The reliable alternative is env var inheritance: a teammate spawned inside a worktree inherits `CODEFLOW_SESSION_ID` and `CODEFLOW_WORKTREE_PATH` from its parent process. No PID comparison needed. The hook checks `std::env::var("CODEFLOW_MANAGED")` — if set, this process was correctly set up by `codeflow interactive` and all env vars are trustworthy.

→ Back to [Table of Contents](#table-of-contents)

---

## 2. Design Principles

| Principle | Rationale |
|-----------|-----------|
| **Worktree mode ON → `codeflow-env.sh` EMPTY** | The shared file is the source of corruption. In worktree mode, nothing writes to it. Each worktree has its own local env file. |
| **Each worktree is self-contained** | A worktree has its own `.state/runtime/`, `.state/session/`, `.state/sentinels/`, and `.state/ledger/`. Hooks that read state always resolve to the worktree-local path when `CODEFLOW_WORKTREE_PATH` is set. |
| **CLI sets up the worktree BEFORE Claude starts** | `codeflow interactive` creates the worktree, writes the local env file, and sets `CODEFLOW_SESSION_ID` and `CODEFLOW_WORKTREE_PATH` in the shell environment before executing `claude`. No discovery needed at hook time. |
| **Env var is the primary resolution mechanism** | `CODEFLOW_WORKTREE_PATH` from the process environment is the single source of truth. `detect_project_dir()` checks it first and trusts it unconditionally. No fallback to shared files in worktree mode. |
| **CODEFLOW_MANAGED marks CLI-managed sessions** | When `codeflow interactive` sets `CODEFLOW_MANAGED=true`, hooks skip all worktree-creation and env-file-writing logic. The env is already correct. |
| **Backward compatibility via unmanaged path** | Sessions started with plain `claude` (no `codeflow interactive`) continue to work as before. The unmanaged path writes the shared `codeflow-env.sh` as it always has. |

→ Back to [Table of Contents](#table-of-contents)

---

## 3. Solution: `codeflow interactive` Command

### 3.1 Command Interface

```
codeflow interactive    # long form
codeflow -i             # short alias
```

The command reads `parallel-work-config.json` from `.codeflow/config/parallel-work/parallel-work-config.json` to determine whether worktree isolation is enabled.

| Config `worktree.mode` | Behavior |
|------------------------|----------|
| `"always"` | Create isolated worktree, set env vars, exec claude inside worktree |
| `"disabled"` | Pass-through: exec claude directly (no worktree setup) |

When `mode = "always"`, the command:

1. Generates a new `session_id` in the format `ses-{ulid}`
2. Calls `setup_interactive_worktree()` — a shared function extracted from the autorun worker
3. Sets four environment variables before exec-ing claude
4. Changes directory to the worktree root
5. Replaces the current process with `claude` via `exec`

The `exec` (not spawn) is important: the resulting Claude Code process inherits the environment set by `codeflow interactive`. No inter-process communication is needed.

### 3.2 CLI Phase (Complete Chain)

```
USER
  |
  v
codeflow -i
  |
  +-- Read .codeflow/config/parallel-work/parallel-work-config.json
  |   └── worktree.mode = ?
  |
  +-- IF mode = "disabled"
  |     └── exec claude (pass-through, no changes)
  |
  +-- IF mode = "always"
        |
        +-- Generate session_id: ses-{ulid}
        |
        +-- setup_interactive_worktree(project_dir, session_id)
        |     |
        |     +-- create_detached_worktree(project_dir, session_id)
        |     |     worktree/setup.rs:145
        |     |     git worktree add .git-worktrees/worktree-{sid} --detach
        |     |
        |     +-- setup_shared_symlinks(project_dir, wt_state_dir)
        |     |     worktree/setup.rs:316
        |     |     Links: .state/db, .state/coordination, .state/logs,
        |     |            .state/registry, .state/backups → main repo
        |     |
        |     +-- create_local_dirs(wt_state_dir)
        |     |     worktree/setup.rs:494
        |     |     Creates: .state/ledger/, .state/runtime/,
        |     |              .state/session/, .state/sentinels/
        |     |
        |     +-- setup_runtime_subdirs(project_dir, wt_state_dir)
        |     |     worktree/setup.rs:107 (approx)
        |     |     Creates runtime subdirectories for session state
        |     |
        |     +-- write_env_file_with_worktree(wt_path, session_id, project_dir)
        |     |     session/env.rs:75
        |     |     Writes to: {worktree}/.state/runtime/codeflow-env.sh
        |     |     Content: CODEFLOW_SESSION_ID, CF_PROJECT_ROOT,
        |     |              CODEFLOW_WORKTREE_PATH
        |     |
        |     +-- locked_register_with_limit(registry_path, entry, max=5)
        |           worktree/registry.rs:380
        |           Registers worktree in .state/worktrees/worktrees.yaml
        |           Enforces max 5 concurrent worktrees
        |
        +-- Set env vars in current process:
        |     CODEFLOW_SESSION_ID   = ses-{ulid}
        |     CODEFLOW_WORKTREE_PATH = {worktree_path}
        |     CF_PROJECT_ROOT        = {main_repo_path}
        |     CODEFLOW_MANAGED       = true
        |
        +-- cd {worktree_path}
        |
        +-- exec claude
              |
              v
        Claude Code starts
        (inherits all 4 env vars)
              |
              v
        SessionStart hook fires
              |
              +-- Reads CODEFLOW_MANAGED=true
              +-- SKIP: worktree creation (already done)
              +-- SKIP: shared env write (already done)
              +-- SKIP: stale worktree cleanup
              +-- SKIP: PID file write
              +-- DO: resolve session_id from CODEFLOW_SESSION_ID
              +-- DO: source local codeflow-env.sh for verification
              +-- DO: init pathflow state files
              +-- DO: output env JSON for hooks
```

### 3.3 Shared Code with Autorun

The worktree setup logic currently lives in `autorun/worker.rs`. The `codeflow interactive` command extracts a shared function `setup_interactive_worktree()` from that module, which both paths call.

**What autorun does vs what interactive does:**

| Step | Autorun Worker | codeflow interactive |
|------|---------------|---------------------|
| Session ID source | `AUTORUN_SESSION_ID` env var (pre-assigned by orchestrator) | Generated fresh via ULID |
| Worktree creation | `setup_detached_worktree()` in worker startup | `setup_interactive_worktree()` — same underlying calls |
| Env var propagation | Set before worker thread spawns Claude | Set before `exec claude` |
| Task assignment | Pre-assigned via `AUTORUN_TASK_ID` | None — user drives via PathFlow |
| Branch | Pre-assigned via `AUTORUN_TARGET` | Detached HEAD; branch set at PF3-CLASSIFY |
| Claims | `acquire_batch()` at startup from `file_scope` | None at startup; claimed per operation |
| Merge queue | Enqueued for auto-merge if configured | Not used |
| `CODEFLOW_MANAGED` | Always true | Always true |

**Shared function signature (new):**

```rust
// worktree/setup.rs
pub fn setup_interactive_worktree(
    project_dir: &Path,
    session_id: &str,
) -> Result<WorktreePaths, WorktreeError>
```

This function consolidates the five setup steps: `create_detached_worktree`, `setup_shared_symlinks`, `create_local_dirs`, `setup_runtime_subdirs`, `write_env_file_with_worktree`, and `locked_register_with_limit`.

The autorun worker refactors to call `setup_interactive_worktree()` instead of calling the individual functions directly.

→ Back to [Table of Contents](#table-of-contents)

---

## 4. SessionStart Hook Changes

### 4.1 Current Flow (Before)

```
SessionStart fires (source=startup)
  |
  +-- Parse stdin JSON for session_id
  |
  +-- detect_project_dir()
  |     Step 1: CODEFLOW_WORKTREE_PATH env var
  |     Step 2: CF_PROJECT_ROOT env var
  |     Step 3: Walk CWD for .claude/.codeflow
  |     Step 4: Per-PID env file lookup      <-- fragile
  |     Step 5: Shared codeflow-env.sh read  <-- corruption source
  |
  +-- IF source = startup:
  |     +-- Write shared codeflow-env.sh    <-- OVERWRITES parallel session
  |     +-- Write per-PID env file          <-- partial mitigation
  |     +-- Run stale worktree cleanup      <-- may touch other sessions
  |     +-- Count pending_tmux_panes        <-- fragile teammate detection
  |     +-- IF not teammate: create worktree (if worktree mode)
  |
  +-- Init pathflow state
  +-- Output env JSON
```

### 4.2 New Flow (After)

```
SessionStart fires (source=startup)
  |
  +-- Check CODEFLOW_MANAGED env var
  |
  +-- IF CODEFLOW_MANAGED = true  (managed path)
  |     |
  |     +-- Trust CODEFLOW_SESSION_ID from env (skip stdin session_id)
  |     +-- Trust CODEFLOW_WORKTREE_PATH from env
  |     +-- SKIP: worktree creation (done by CLI)
  |     +-- SKIP: shared env write (worktrees don't touch shared file)
  |     +-- SKIP: per-PID file write (not needed)
  |     +-- SKIP: stale cleanup (CLI handles lifecycle)
  |     +-- SKIP: pending_tmux_count check (not needed)
  |     +-- DO: source local codeflow-env.sh for verification
  |     +-- DO: init pathflow state files (session-local paths)
  |     +-- DO: output env JSON
  |
  +-- IF CODEFLOW_MANAGED not set  (unmanaged path)
        |
        +-- detect_project_dir() (3 steps only — steps 4+5 removed)
        +-- Write shared codeflow-env.sh (safe: single session)
        +-- Init pathflow state (project-root paths)
        +-- Output env JSON
```

### 4.3 CODEFLOW_MANAGED Path

When `CODEFLOW_MANAGED=true`, the SessionStart hook can make these guarantees:

- `CODEFLOW_SESSION_ID` is correct — set by the CLI before exec
- `CODEFLOW_WORKTREE_PATH` is correct — set by the CLI before exec
- The worktree exists and is fully initialized — CLI completed setup
- The local `codeflow-env.sh` at `{worktree}/.state/runtime/codeflow-env.sh` is already written

The hook reads the local env file for verification (to catch any env var stripping by shell wrappers) but does not rewrite it.

Teammates spawned inside this session inherit all env vars. Their `SessionStart` also fires with `CODEFLOW_MANAGED=true`. The hook treats them identically to the lead — no teammate detection needed because the env vars are already correct.

### 4.4 Unmanaged Path (Plain `claude`)

When `CODEFLOW_MANAGED` is not set, the session is started by `claude` directly (no CLI wrapper). This is the legacy path. Behavior is unchanged from today except that PID file logic is removed:

- `detect_project_dir()` uses steps 1-3 only (env vars + CWD walk)
- The shared `codeflow-env.sh` is written (safe: only one unmanaged session can be active)
- No worktree is created
- Teammate detection via `lead_pid` comparison remains (no env var to check in this path)

The unmanaged path is a backward-compatibility provision. Users who run `claude` directly without `codeflow interactive` continue to get the existing single-session behavior.

### 4.5 Session ID Lifecycle

```
MANAGED PATH:
  codeflow -i
    └── generates ses-{ulid}
    └── sets CODEFLOW_SESSION_ID in process env
    └── writes to {worktree}/.state/runtime/codeflow-env.sh
        └── Claude Code starts with this env
            └── SessionStart reads CODEFLOW_SESSION_ID from env
                └── ALL hooks use this value for the entire session
                └── Context overflow: new process re-reads same env var
                    (inherited from parent shell that ran codeflow -i)

UNMANAGED PATH:
  claude
    └── SessionStart parses session_id from stdin JSON
    └── writes to .state/runtime/codeflow-env.sh
        └── hooks read this file
            └── PROBLEM: second session overwrites file
            └── Mitigated: only one unmanaged session should run at a time
```

Context overflow (compaction) in managed path is safe because env vars are inherited from the shell that ran `codeflow -i`. The new Claude Code process re-inherits `CODEFLOW_SESSION_ID` and `CODEFLOW_WORKTREE_PATH` without reading any file.

→ Back to [Table of Contents](#table-of-contents)

---

## 5. Hook Audit: What Changes Per Hook

### 5.1 SessionStart (Major Changes)

**File:** `codeflow-cli/core/src/hooks/session_start.rs`

**Changes:**

| Change | Before | After |
|--------|--------|-------|
| Managed detection | None | Check `CODEFLOW_MANAGED` env var at top |
| Session ID resolution | Parse stdin, fallback to env file | Managed: trust `CODEFLOW_SESSION_ID` env; Unmanaged: parse stdin |
| Shared env write | Always write `.state/runtime/codeflow-env.sh` | Managed: skip; Unmanaged: write (unchanged) |
| Per-PID env file write | Write `codeflow-env-{pid}.sh` | Remove entirely |
| Worktree creation | Create worktree if mode=always and not teammate | Managed: skip (already done); Unmanaged: unchanged |
| Stale cleanup | Run on every startup | Managed: skip; Unmanaged: run (unchanged) |
| Teammate detection | `pending_tmux_count` + `lead_pid` comparison | Managed: skip (not needed — env vars are correct); Unmanaged: unchanged |
| Local env write | None | Managed: source local env for verification (read-only) |

**New code structure:**

```rust
pub fn handle_session_start(payload: &SessionStartPayload) -> Result<()> {
    if std::env::var("CODEFLOW_MANAGED").is_ok() {
        return handle_managed_session_start(payload);
    }
    handle_unmanaged_session_start(payload)
}

fn handle_managed_session_start(payload: &SessionStartPayload) -> Result<()> {
    let session_id = std::env::var("CODEFLOW_SESSION_ID")?;
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH")?;
    // Verify local env file is present (defensive check)
    // Init pathflow state files at worktree-local paths
    // Output env JSON
    Ok(())
}

fn handle_unmanaged_session_start(payload: &SessionStartPayload) -> Result<()> {
    // Existing logic, minus PID file writes
}
```

### 5.2 SessionEnd (Remove PID Cleanup)

**File:** `codeflow-cli/core/src/hooks/session_end.rs`

**Changes:**

- Remove: cleanup of per-PID env file (`codeflow-env-{pid}.sh`)
- No other changes

The worktree cleanup for managed sessions is handled by the CLI's cleanup command (`codeflow cleanup`) or the `WorktreeRegistry` TTL, not by the SessionEnd hook.

### 5.3 PreToolUse (No Changes)

**File:** `codeflow-cli/core/src/hooks/pre_tool_use.rs`

`detect_project_dir()` already checks `CODEFLOW_WORKTREE_PATH` first (step 1). With the managed path, this env var is always set and correct. No hook logic changes are needed.

The gate-check sentinel path uses the session-local `.state/sentinels/` directory, which resolves correctly via `WorktreePaths` when `CODEFLOW_WORKTREE_PATH` is set.

### 5.4 PostToolUse (Add Branch Update at pf-3)

**File:** `codeflow-cli/core/src/hooks/post_tool_use.rs`

**New behavior:** When the checkpoint system creates the `pathflow-pf-3` sentinel (indicating PF3-CLASSIFY is complete and a feature branch has been created), PostToolUse calls `update_branch_from_current()` to record the current git branch in the worktree registry entry.

**Why:** In the managed path, the worktree starts on a detached HEAD. At PF3-CLASSIFY, `cf-git-operations` creates and switches to the feature branch. The registry entry's `branch` field should be updated to reflect the actual branch name for visibility in `codeflow worktree list`.

```rust
// In checkpoint-register PostToolUse handler
if sentinel_just_created == "pathflow-pf-3" {
    if let Ok(wt_path) = std::env::var("CODEFLOW_WORKTREE_PATH") {
        let _ = update_branch_from_current(&wt_path, &registry_path);
    }
}
```

### 5.5 TaskCompleted (Add Branch Update at pf-3)

**File:** `codeflow-cli/core/src/hooks/task_completed.rs`

Same branch update as PostToolUse: when `checkpoint-complete` creates the `pathflow-pf-3` sentinel, call `update_branch_from_current()`. This covers the case where the sentinel is created by the TaskCompleted hook rather than PostToolUse.

### 5.6 Other Hooks (No Changes)

| Hook | Status | Reason |
|------|--------|--------|
| `UserPromptSubmit` | No changes | Reads project dir via `detect_project_dir()` — already correct with env var |
| `Stop` / `SubagentStop` | No changes | Logging only; no state writes |
| `security` PreToolUse | No changes | Path-based checks; unaffected by worktree isolation |
| `webfetch-guard` PreToolUse | No changes | URL-based; unaffected |
| `settings-validate` PostToolUse | No changes | Settings path resolution already uses project dir correctly |

→ Back to [Table of Contents](#table-of-contents)

---

## 6. Eliminated Components

### 6.1 PID-Based Env Files (Removed)

**Current files:** `.state/runtime/codeflow-env-{pid}.sh`

**Written by:** `session_start.rs` (every startup)
**Read by:** `helpers.rs` `detect_project_dir()` step 4
**Cleaned by:** `session_end.rs`

**Why removed:** In the managed path, `CODEFLOW_WORKTREE_PATH` is set as an env var before Claude starts. `detect_project_dir()` reads it at step 1 and returns immediately — it never reaches step 4. The per-PID files add complexity and cleanup burden without providing value in the managed path.

In the unmanaged path, per-PID files are also removed because the single-session constraint means the shared `codeflow-env.sh` is reliable. The fragile PID detection (`get_claude_code_pid()`) is no longer called.

**Removal scope:**

- `session/env.rs`: remove `write_pid_env_file()`, `read_worktree_path_from_pid_file()`
- `session_start.rs`: remove per-PID write call
- `session_end.rs`: remove per-PID cleanup call
- `helpers.rs`: remove step 4 from `detect_project_dir()`

### 6.2 Shared codeflow-env.sh in Worktree Mode (Removed)

**File:** `.state/runtime/codeflow-env.sh` (in the main repo)

In worktree mode (managed path), this file is never written. Each worktree has its own copy at `{worktree}/.state/runtime/codeflow-env.sh`. The shared file retains its role only for the unmanaged path (single non-worktree sessions).

**Effect on `detect_project_dir()`:** Step 3 (CWD walk) returns the main repo root for unmanaged sessions. The shared env file is only read as a last-resort fallback that is no longer needed in any active path.

### 6.3 detect_project_dir() Simplification

**File:** `codeflow-cli/cli/src/helpers.rs:20`

Current implementation has 4+ steps. New implementation has 3:

```
CURRENT (4 steps, fragile):
  Step 1: CODEFLOW_WORKTREE_PATH env var  → return if valid dir
  Step 2: CF_PROJECT_ROOT env var         → return if valid dir
  Step 3: Walk CWD for .claude/.codeflow  → project root
  Step 4: Per-PID env file lookup         → REMOVED
  (implicit Step 5: shared env fallback)  → REMOVED

NEW (3 steps, reliable):
  Step 1: CODEFLOW_WORKTREE_PATH env var  → return if valid dir
  Step 2: CF_PROJECT_ROOT env var         → return if valid dir
  Step 3: Walk CWD for .claude/.codeflow  → project root (fallback)
```

In the managed path, step 1 always succeeds — the function returns after one env var check. Steps 2 and 3 are never reached for hook calls inside a `codeflow interactive` session.

In the unmanaged path, step 1 fails (no `CODEFLOW_WORKTREE_PATH`), step 2 may succeed (if `CF_PROJECT_ROOT` is set), otherwise step 3 finds the root via CWD walk.

### 6.4 Teammate Detection via pending_tmux_count (Removed)

**Current mechanism:** `SessionStart` counts the number of pending tmux panes to determine whether the current process is the lead or a teammate. If `pending_tmux_count > 0`, the session is a teammate.

**Why removed for managed path:** In the managed path, every process that fires `SessionStart` is running inside the worktree with the correct env vars already set. There is no need to distinguish lead from teammate at the hook level — both are legitimate processes sharing the same session context. The env vars are sufficient.

Teammate identification (for `pathflow-team.json` population) can be determined by checking whether `pathflow-session-status.json` already exists for this session ID. If it does, the firing process is a teammate; if not, it is the lead.

**Retention for unmanaged path:** The `lead_pid` / `pending_tmux_count` logic is retained for unmanaged sessions where tmux-based teammates are used and env var linkage is not available.

→ Back to [Table of Contents](#table-of-contents)

---

## 7. Scenario Analysis

### 7.1 Non-Worktree Interactive (Plain claude)

User runs `claude` directly without `codeflow interactive`. Single session.

```
USER
  └── claude (from project root)
        |
        SessionStart fires
        |
        +-- CODEFLOW_MANAGED not set → unmanaged path
        +-- detect_project_dir(): Step 3 → /path/to/project
        +-- Write .state/runtime/codeflow-env.sh
        |     CODEFLOW_SESSION_ID=ses-ABC
        |     CF_PROJECT_ROOT=/path/to/project
        |
        +-- Init pathflow at /path/to/project/.state/
        |
        Session runs normally
        All hooks read codeflow-env.sh → ses-ABC (safe: only one session)
        |
        SessionEnd
        └── (no PID file cleanup needed)

RESULT: Works exactly as before. No regression.
```

### 7.2 Single codeflow -i Session

```
USER
  └── codeflow -i (from project root)
        |
        +-- Read parallel-work-config.json → mode=always
        +-- Generate session_id = ses-AAA
        +-- setup_interactive_worktree()
        |     → .git-worktrees/worktree-ses-AAA/ created
        |     → .state/runtime/codeflow-env.sh written at WORKTREE local path
        |     → registered in .state/worktrees/worktrees.yaml
        |
        +-- Set env: CODEFLOW_SESSION_ID=ses-AAA
        |            CODEFLOW_WORKTREE_PATH=.../.git-worktrees/worktree-ses-AAA
        |            CF_PROJECT_ROOT=/path/to/project
        |            CODEFLOW_MANAGED=true
        |
        +-- cd .git-worktrees/worktree-ses-AAA
        +-- exec claude
              |
              SessionStart fires
              |
              +-- CODEFLOW_MANAGED=true → managed path
              +-- session_id = ses-AAA (from env)
              +-- worktree = .../.git-worktrees/worktree-ses-AAA (from env)
              +-- SKIP all worktree setup
              +-- Init pathflow at worktree-ses-AAA/.state/
              |
              All hooks:
                detect_project_dir() → Step 1 → worktree-ses-AAA/ ✓
                Sentinels at: worktree-ses-AAA/.state/sentinels/ses-AAA/
                |
              PF3-CLASSIFY: branch created, PostToolUse updates registry
              |
              Session continues normally
              |
              SessionEnd → worktree cleanup via codeflow cleanup

RESULT: Fully isolated. No shared state written.
```

### 7.3 Two Parallel codeflow -i Sessions

```
USER TERMINAL 1                    USER TERMINAL 2
  └── codeflow -i                    └── codeflow -i
        ses-AAA                             ses-BBB
        worktree-ses-AAA                    worktree-ses-BBB
        CODEFLOW_SESSION_ID=ses-AAA         CODEFLOW_SESSION_ID=ses-BBB
        CODEFLOW_WORKTREE_PATH=.../AAA      CODEFLOW_WORKTREE_PATH=.../BBB
        CODEFLOW_MANAGED=true               CODEFLOW_MANAGED=true
        |                                   |
        exec claude                         exec claude
        |                                   |
        SessionStart → managed path         SessionStart → managed path
        Init at .../AAA/.state/             Init at .../BBB/.state/
        |                                   |
        Hook fires in AAA:                  Hook fires in BBB:
          detect_project_dir()                detect_project_dir()
          → CODEFLOW_WORKTREE_PATH            → CODEFLOW_WORKTREE_PATH
          → .../AAA ✓                         → .../BBB ✓
        |                                   |
        Sentinels: .../AAA/.state/          Sentinels: .../BBB/.state/
          sentinels/ses-AAA/                  sentinels/ses-BBB/
        |                                   |
        WorkGraph: ses-AAA events           WorkGraph: ses-BBB events
        JSONL: .../AAA/.state/ledger/       JSONL: .../BBB/.state/ledger/
        |                                   |
        ZERO SHARED MUTABLE STATE           ZERO SHARED MUTABLE STATE
        (db/, coordination/ symlinked       (same — shared but read-optimized
         but SurrealDB RetryConfig           via RetryConfig)
         handles parallel access)

RESULT: Complete isolation. No corruption possible.
        Sessions are independent; shared DB access serialized by RetryConfig.
```

### 7.4 In-Process Teammates

Claude Code spawns subagents as in-process threads (not separate OS processes). In-process subagents share the parent process's environment.

```
codeflow -i → ses-AAA → exec claude (lead)
                |
                +-- CODEFLOW_SESSION_ID=ses-AAA  (inherited by all children)
                +-- CODEFLOW_WORKTREE_PATH=.../AAA
                |
                Lead spawns Task(subagent_type=general-purpose)
                  |
                  SubagentStart fires (NOT SessionStart — in-process agents
                  fire SubagentStart, not SessionStart)
                  |
                  Hook reads env vars → ses-AAA, .../AAA ✓
                  No separate worktree setup needed
                  |
                  Subagent inherits all env vars from parent process
                  detect_project_dir() → Step 1 → .../AAA ✓

RESULT: In-process teammates automatically inherit correct env vars.
        No teammate detection needed. SubagentStart hook unchanged.
```

### 7.5 tmux Teammates (CWD Inheritance)

When teammates are spawned in tmux panes, the pane inherits the CWD from `exec claude`'s working directory, which is the worktree root.

```
codeflow -i
  +-- cd .git-worktrees/worktree-ses-AAA
  +-- exec claude
        |
        Lead spawns Task with tmux backend
        tmux pane opens with:
          CWD = .git-worktrees/worktree-ses-AAA (inherited from parent)
          ENV = CODEFLOW_SESSION_ID=ses-AAA     (inherited from parent)
               CODEFLOW_WORKTREE_PATH=.../AAA   (inherited from parent)
               CODEFLOW_MANAGED=true            (inherited from parent)
        |
        SessionStart fires in tmux pane
        |
        +-- CODEFLOW_MANAGED=true → managed path
        +-- session_id = ses-AAA (from inherited env var)
        +-- worktree = .../AAA (from inherited env var)
        +-- No new worktree created
        +-- Pathflow state at .../AAA/.state/ (correct session-local paths)
        |
        detect_project_dir() in tmux pane:
          Step 1: CODEFLOW_WORKTREE_PATH=.../AAA → return .../AAA ✓

RACE CONDITION NOTE: tmux may not inherit env vars if the pane is opened
in a fresh shell. The managed path REQUIRES that the tmux pane runs a
command that inherits the parent environment (e.g., via `env` passthrough
or explicit var-setting in the spawn command).
See Section 8.2 for the CWD inheritance race.
```

### 7.6 Autorun Workers

Autorun already sets `CODEFLOW_SESSION_ID`, `CODEFLOW_WORKTREE_PATH`, and `CODEFLOW_MANAGED=true` before spawning the worker. After this change, autorun workers additionally benefit from the simplified SessionStart managed path.

```
codeflow autorun (orchestrator)
  |
  +-- Generate session_id per worker = ses-W1, ses-W2, ...
  +-- setup_interactive_worktree() (refactored shared fn)
  |     → worktree-ses-W1/ created
  +-- Set env: CODEFLOW_SESSION_ID=ses-W1
  |            CODEFLOW_WORKTREE_PATH=.../worktree-ses-W1
  |            CODEFLOW_MANAGED=true
  |            AUTORUN_SESSION_ID=ses-W1
  |            AUTORUN_TASK_ID={task_id}
  |
  +-- Worker spawns Claude for ses-W1
        |
        SessionStart fires
        |
        +-- CODEFLOW_MANAGED=true → managed path (same as interactive)
        +-- session_id from CODEFLOW_SESSION_ID
        +-- No duplicate worktree setup
        |
        Worker runs PathFlow autonomously
        Claims acquired via acquire_batch() at worker startup
        Merge queue serializes PR merges

RESULT: Autorun and interactive share the same managed SessionStart path.
        Code duplication eliminated.
```

### 7.7 Context Overflow Recovery

Context overflow (compaction) creates a new conversation context with a fresh Claude Code process (or a new context window in the same process, depending on backend).

```
MANAGED PATH context overflow:

  codeflow -i (shell process, still alive)
    CODEFLOW_SESSION_ID=ses-AAA (still in shell env)
    CODEFLOW_WORKTREE_PATH=.../AAA (still in shell env)
    CODEFLOW_MANAGED=true (still in shell env)
    |
    Claude Code context overflows
    |
    New conversation context begins
    |
    +-- IF same OS process: env vars still in process memory → ses-AAA ✓
    +-- IF new subprocess: inherits env from shell (codeflow -i is still
        running as parent) → ses-AAA ✓
    |
    SessionStart fires with source=compact
    |
    +-- CODEFLOW_MANAGED=true → managed path
    +-- session_id = ses-AAA (from env, NOT from stdin)
    +-- worktree = .../AAA (from env)
    +-- SKIP all setup (worktree already exists and is registered)
    +-- Source local env file (verification)
    +-- Init pathflow state (session already exists — resume, not init)

RESULT: Session ID is stable across context overflow.
        No new worktree created. Existing sentinels preserved.
        Recovery procedure: verify teammates alive, re-orient, continue.
```

### 7.8 plain `claude` While `codeflow -i` Running

A user runs `claude` directly while a `codeflow -i` session is already active. This is the "mixed mode" case.

```
TERMINAL 1: codeflow -i → ses-AAA → worktree-ses-AAA
            CODEFLOW_MANAGED=true (in terminal 1's env only)

TERMINAL 2: claude (from project root)
            CODEFLOW_MANAGED not set
            |
            SessionStart fires → unmanaged path
            detect_project_dir(): Step 3 → project root
            Writes: .state/runtime/codeflow-env.sh  ← DANGER ZONE
              CODEFLOW_SESSION_ID=ses-BBB
              CF_PROJECT_ROOT=/path/to/project
            |
            Session runs on project root (no worktree isolation)

ISOLATION ANALYSIS:
  Terminal 1 (managed):
    All hooks use CODEFLOW_WORKTREE_PATH env var (step 1 in detect)
    Never reads shared codeflow-env.sh
    Fully isolated from terminal 2

  Terminal 2 (unmanaged):
    Reads/writes shared codeflow-env.sh
    Not isolated from other unmanaged sessions
    But isolated FROM managed sessions (they don't touch shared file)

VERDICT: Safe. The managed session (terminal 1) is completely unaffected
by the unmanaged session (terminal 2). The unmanaged session's shared
env file write does not corrupt terminal 1 because terminal 1's hooks
never read that file.

CAVEAT: Two simultaneous unmanaged sessions (two plain `claude` in
different terminals) would still corrupt each other. Users who need
parallelism should use `codeflow -i` exclusively.
```

### 7.9 Max Worktrees Reached

`locked_register_with_limit()` enforces a maximum of 5 concurrent worktrees.

```
codeflow -i (6th attempt)
  |
  +-- setup_interactive_worktree()
        |
        +-- create_detached_worktree() ← succeeds (git level)
        +-- setup_shared_symlinks()    ← succeeds
        +-- create_local_dirs()        ← succeeds
        |
        +-- locked_register_with_limit(max=5)
              |
              Registry has 5 active entries
              |
              Returns WorktreeError::LimitReached(5)
              |
        ← setup_interactive_worktree() propagates error
  |
  codeflow -i prints:
    "Error: Maximum concurrent sessions reached (5).
     Run 'codeflow worktree list' to see active sessions.
     Run 'codeflow cleanup' to remove completed sessions."
  |
  Partial worktree (git-level) needs cleanup:
    codeflow interactive performs git worktree remove on error path

USER CORRECTIVE ACTION:
  codeflow worktree list          → see all 5 active sessions
  codeflow cleanup --session {id} → remove a completed session
  codeflow -i                     → retry (now succeeds with 4 active)
```

→ Back to [Table of Contents](#table-of-contents)

---

## 8. Race Condition Analysis

### 8.1 Parallel Worktree Registration

**Scenario:** Two `codeflow -i` commands run simultaneously (two terminal windows launched at the exact same instant).

```
T=0: codeflow -i (A) and codeflow -i (B) both start
T=1: Both call locked_register_with_limit()
     └── Uses file locking (flock) on .state/worktrees/worktrees.yaml
         One wins the lock, registers, releases
         The other waits, acquires lock, registers
T=2: Both registrations succeed (assuming count < 5)
     Each has distinct session_id (ULID generation is collision-resistant)
T=3: Both exec claude with different CODEFLOW_SESSION_ID values
```

**Verdict: Safe.** `locked_register_with_limit` uses OS-level file locking. The ULID-based session ID generation provides sufficient uniqueness (48-bit timestamp + 80-bit randomness).

### 8.2 tmux CWD Inheritance

**Scenario:** Lead spawns a teammate in a new tmux pane. The pane opens with a fresh login shell that clears the environment.

```
Lead spawns Task(tmux pane) with command:
  "Read agent def and do work"

tmux opens pane:
  Shell type = login shell (/bin/bash --login)
  Login shells source .bashrc/.zshrc → may clear CODEFLOW_* vars

Result: teammate pane has NO env vars
  detect_project_dir() → step 1 fails (no CODEFLOW_WORKTREE_PATH)
  step 3: CWD walk → finds project root (not worktree)
  SessionStart → unmanaged path → writes shared codeflow-env.sh ← CORRUPTION
```

**Mitigation:** The spawn command for tmux teammates must explicitly set env vars:

```rust
// In tmux spawn logic (not yet implemented):
format!("CODEFLOW_SESSION_ID={} CODEFLOW_WORKTREE_PATH={} \
         CODEFLOW_MANAGED=true CF_PROJECT_ROOT={} claude",
        session_id, worktree_path, project_root)
```

Alternatively, use `tmux new-window -e "CODEFLOW_SESSION_ID=..."` to pass env vars explicitly to tmux.

**Current state:** This race condition exists today. The `codeflow interactive` design resolves it if the CLI-spawned teammates explicitly pass env vars in the tmux command.

### 8.3 Context Overflow Env Persistence

**Scenario:** Context overflow in a managed session. The compacted context starts fresh. Are env vars still available?

This depends on the Claude Code backend:

| Backend | Env persistence after overflow | Safe? |
|---------|-------------------------------|-------|
| Same OS process, new context window | Env vars in process memory | Yes |
| New subprocess (fork/exec) | Child inherits parent env | Yes |
| Remote execution (cloud backend) | Env vars may not persist | Potentially No |

For local execution (the only current mode), env vars persist across context overflow. The `codeflow interactive` shell process remains alive and holds the env in its process table. Any subprocess Claude Code spawns inherits from it.

**Mitigation for remote execution (future):** The local env file at `{worktree}/.state/runtime/codeflow-env.sh` is the fallback. SessionStart's managed path already sources this file for verification. In remote mode, the hook should read from this file if env vars are not set.

### 8.4 Stale Session Cleanup

**Scenario:** A session crashes (power loss, kill -9). The worktree remains registered but the Claude Code process is gone.

```
Crash: worktree-ses-AAA registered, ses-AAA process dead
  |
  Next codeflow -i starts
  |
  +-- locked_register_with_limit()
        Registry shows 1 active entry (ses-AAA, stale)
        Limit = 5 → succeeds (count=2 after new registration)
  |
  Session runs normally

PROBLEM: stale entry counts against the limit.
If 5 stale entries exist, new sessions are blocked.

MITIGATION: PID/heartbeat liveness (Section 9) detects dead sessions.
codeflow cleanup --prune-stale removes entries whose lead process is dead.
SessionStart hook in unmanaged path runs stale cleanup (retained behavior).
For managed path: stale cleanup runs on codeflow worktree cleanup command.
```

→ Back to [Table of Contents](#table-of-contents)

---

## 9. PID/Heartbeat Liveness: What Remains

With teammate detection removed from the managed path, the only remaining use of PID-based liveness is **stale session cleanup**.

| Use Case | Before | After |
|----------|--------|-------|
| Teammate detection | PID comparison + tmux count | Removed (use CODEFLOW_MANAGED env var instead) |
| Stale worktree detection | PID file existence | `session/liveness.rs::is_session_alive(pid)` |
| SessionEnd PID cleanup | Clean up `codeflow-env-{pid}.sh` | Removed (no PID files) |
| Dead worker claim release | Heartbeat TTL + `cleanup_dead_workers()` | Retained (autorun only) |

**What `session/liveness.rs` does after this change:**

The `liveness.rs` module (`codeflow-cli/core/src/session/liveness.rs`) is retained but simplified:

```rust
// Retained: check if a session's lead process is still alive
pub fn is_session_alive(lead_pid: u32) -> bool {
    // kill(pid, 0) → process exists
    // Used by: codeflow worktree cleanup --prune-stale
    //          codeflow doctor --worktrees
}
```

The `heartbeat.rs` module (`codeflow-cli/core/src/session/heartbeat.rs`) is retained for autorun workers (they use the heartbeat TTL for claim release via `cleanup_dead_workers()`). For interactive sessions, the heartbeat is not written — liveness is determined by PID check only when cleanup is requested.

**Summary:** PID/liveness is a cleanup tool, not a routing mechanism. It is no longer called in the hot path (hooks). It is called only from administrative commands (`cleanup`, `doctor`).

→ Back to [Table of Contents](#table-of-contents)

---

## 10. File Change Map

### 10.1 New Files

| File | Description |
|------|-------------|
| `codeflow-cli/cli/src/cmd/interactive.rs` | `codeflow interactive` / `codeflow -i` command. Reads config, calls `setup_interactive_worktree()`, sets env vars, execs claude. |

### 10.2 Modified Files

| File | Change Type | Details |
|------|------------|---------|
| `codeflow-cli/cli/src/cmd/mod.rs` | Modify | Register `interactive` subcommand; add `-i` alias |
| `codeflow-cli/core/src/worktree/setup.rs` | Modify | Extract `pub fn setup_interactive_worktree()` that wraps the 5-step setup chain; called by both `interactive.rs` and refactored `autorun/worker.rs` |
| `codeflow-cli/core/src/hooks/session_start.rs` | Modify (major) | Add `CODEFLOW_MANAGED` branch; split into `handle_managed_session_start()` and `handle_unmanaged_session_start()`; remove PID file writes; remove stale cleanup from managed path; remove `pending_tmux_count` check from managed path |
| `codeflow-cli/core/src/hooks/session_end.rs` | Modify | Remove per-PID env file cleanup |
| `codeflow-cli/core/src/hooks/post_tool_use.rs` | Modify | Add `update_branch_from_current()` call when `pathflow-pf-3` sentinel is created |
| `codeflow-cli/core/src/hooks/task_completed.rs` | Modify | Add `update_branch_from_current()` call when `pathflow-pf-3` sentinel is created (via `checkpoint-complete` handler) |
| `codeflow-cli/core/src/session/env.rs` | Modify | Remove `write_pid_env_file()` and `read_worktree_path_from_pid_file()` |
| `codeflow-cli/cli/src/helpers.rs` | Modify | Simplify `detect_project_dir()` from 4 steps to 3; remove step 4 (per-PID file lookup); remove `read_worktree_path_from_pid_file()` call |
| `codeflow-cli/core/src/autorun/worker.rs` | Modify | Refactor worktree setup to call shared `setup_interactive_worktree()` instead of inline setup steps |

### 10.3 Deleted Code

| Location | Code Removed | Reason |
|----------|-------------|--------|
| `session/env.rs` | `write_pid_env_file()` function | PID files eliminated |
| `session/env.rs` | `read_worktree_path_from_pid_file()` function | PID files eliminated |
| `session/process.rs` | `get_claude_code_pid()` function (or demoted to test-only) | No longer called in production path |
| `hooks/session_start.rs` | PID file write call | PID files eliminated |
| `hooks/session_start.rs` | `pending_tmux_count` detection logic (managed path only) | Replaced by env var check |
| `hooks/session_start.rs` | Stale worktree cleanup (managed path only) | Managed by CLI commands |
| `hooks/session_end.rs` | PID file cleanup | PID files eliminated |
| `helpers.rs` | `detect_project_dir()` step 4 | PID files eliminated |
| `helpers.rs` | `read_worktree_path_from_pid_file()` call | PID files eliminated |

→ Back to [Table of Contents](#table-of-contents)

---

## 11. Implementation Plan

### Priority Order

| Priority | Change | Depends On | Risk |
|----------|--------|-----------|------|
| P0 | `setup_interactive_worktree()` shared fn in `worktree/setup.rs` | Nothing | Low — extraction only |
| P0 | `session_start.rs` CODEFLOW_MANAGED branch | Nothing | Medium — core hook change |
| P1 | `cli/src/cmd/interactive.rs` new command | `setup_interactive_worktree()` | Low — new code |
| P1 | `cli/src/cmd/mod.rs` register subcommand | `interactive.rs` | Low |
| P2 | Remove PID file functions from `session/env.rs` | Managed path tested and working | Medium — removes existing code |
| P2 | Simplify `detect_project_dir()` (remove step 4) | PID files removed | Medium |
| P2 | `session_end.rs` remove PID cleanup | PID files removed | Low |
| P3 | `autorun/worker.rs` refactor to shared fn | `setup_interactive_worktree()` | Low |
| P3 | `post_tool_use.rs` branch update at pf-3 | pf-3 sentinel logic | Low |
| P3 | `task_completed.rs` branch update at pf-3 | pf-3 sentinel logic | Low |

### Dependencies Between Changes

```
setup_interactive_worktree()  ←─── interactive.rs
       │                      ←─── autorun/worker.rs (refactor)
       │
       ▼
session_start.rs MANAGED path ←─── All managed-path scenarios

       ├─── session/env.rs PID removal (after managed path is stable)
       │         │
       │         ▼
       │    detect_project_dir() step 4 removal
       │    session_end.rs cleanup removal
       │
       └─── post_tool_use.rs branch update (independent)
            task_completed.rs branch update (independent)
```

### Required Test Cases

| Test | File | Covers |
|------|------|--------|
| `test_managed_session_start_uses_env_vars` | `session_start.rs` tests | CODEFLOW_MANAGED path reads from env |
| `test_unmanaged_session_start_unchanged` | `session_start.rs` tests | Unmanaged path unchanged |
| `test_detect_project_dir_3_steps_only` | `helpers.rs` tests | No step 4 |
| `test_setup_interactive_worktree_creates_structure` | `worktree/setup.rs` tests | Full worktree setup |
| `test_setup_interactive_worktree_limit_enforced` | `worktree/setup.rs` tests | Max 5 rejected |
| `test_interactive_cmd_exec_args` | `cmd/interactive.rs` tests | Correct env vars passed to exec |
| `test_two_parallel_sessions_no_shared_state` | Integration test | Core correctness guarantee |
| `test_context_overflow_env_persistence` | Integration test | Session ID stable after compaction |

→ Back to [Table of Contents](#table-of-contents)

---

## 12. Related

- [PID detection audit](../../memory/project_pid_detection_audit.md) — 7 writers (6 broken), 11 readers, centralized liveness fix; this design supersedes the per-PID file mitigation approach
- [Session hook lifecycle](../../memory/project_session_hook_lifecycle.md) — SessionStart/SessionEnd fire only for lead; teammates get SubagentStart/SubagentStop
- [Worktree path resolution feedback](../../memory/feedback_worktree_path_resolution.md) — `detect_project_dir()` is the single path resolution function for ALL hooks; changes here affect every hook
- [Staging worktree targeting feedback](../../memory/feedback_staging_worktree_targeting.md) — staging cp must target `$CODEFLOW_WORKTREE_PATH` in worktree mode
- [Autorun integration branch](../../memory/project_autorun_integration_branch.md) — PR #255; autorun workers already use CODEFLOW_MANAGED pattern; this design unifies interactive and autorun

→ Back to [Table of Contents](#table-of-contents)
