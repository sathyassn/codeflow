---
type: analysis
title: "Worktree Session Isolation Design"
area: infrastructure
status: draft
created_at: "2026-04-06"
updated_at: "2026-04-06"
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
- [13. Autorun Field Renaming](#13-autorun-field-renaming)
  - [13.1 Rationale](#131-rationale)
  - [13.2 Field Mapping Table](#132-field-mapping-table)
  - [13.3 Blast Radius](#133-blast-radius)
  - [13.4 Rust Struct Changes](#134-rust-struct-changes)
  - [13.5 Environment Variable Changes](#135-environment-variable-changes)
  - [13.6 YAML Schema Changes](#136-yaml-schema-changes)
  - [13.7 Test Updates](#137-test-updates)
  - [13.8 CLAUDE.md Sections Affected](#138-claudemd-sections-affected)
- [14. Interactive Session Management](#14-interactive-session-management)
  - [14.1 InteractiveSession DB Model](#141-interactivesession-db-model)
  - [14.2 New Subcommands](#142-new-subcommands)
  - [14.3 Liveness Detection](#143-liveness-detection)
  - [14.4 State Directory Layout](#144-state-directory-layout)
  - [14.5 Common Code with Autorun](#145-common-code-with-autorun)
- [15. Autorun Batch README and Examples Update](#15-autorun-batch-readme-and-examples-update)

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
| **CLI generates session ID — always** | `codeflow -i` generates `ses-{ulid}` via CodeFlow CLI, registers in SurrealDB, writes `codeflow-env.sh`, and passes `CODEFLOW_SESSION_ID` via env var to Claude. The SessionStart hook in managed mode reads from env var — never generates a new one. |
| **Worktree mode ON → shared `codeflow-env.sh` EMPTY** | The shared file is the source of corruption. In worktree mode, nothing writes to it. Each worktree has its own local env file. |
| **Each worktree is self-contained** | A worktree has its own `.state/runtime/`, `.state/session/`, `.state/sentinels/`, and `.state/ledger/`. Hooks that read state always resolve to the worktree-local path when `CODEFLOW_WORKTREE_PATH` is set. |
| **CLI sets up the worktree BEFORE Claude starts** | `codeflow interactive` creates the worktree, writes the local env file, and sets `CODEFLOW_SESSION_ID` and `CODEFLOW_WORKTREE_PATH` in the shell environment before executing `claude`. No discovery needed at hook time. |
| **Env var is the primary resolution mechanism** | `CODEFLOW_WORKTREE_PATH` from the process environment is the single source of truth. `detect_project_dir()` checks it first and trusts it unconditionally. No fallback to shared files in worktree mode. |
| **CODEFLOW_MANAGED marks CLI-managed sessions** | When `codeflow interactive` sets `CODEFLOW_MANAGED=true`, hooks skip all worktree-creation and env-file-writing logic. The env is already correct. |
| **Backward compatibility via unmanaged path** | Sessions started with plain `claude` (no `codeflow interactive`) continue to work as before. The unmanaged path uses SessionStart-generated session IDs and writes the shared `codeflow-env.sh` as it always has. |

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
| `"always"` | Create isolated worktree, register session in DB, set env vars, exec claude inside worktree |
| `"disabled"` | Pass-through: exec claude directly (no worktree setup) |

When `mode = "always"`, the command:

1. Generates a new `session_id` in the format `ses-{ulid}`
2. Registers an `InteractiveSession` record in SurrealDB (→ see [Section 14](#14-interactive-session-management))
3. Saves the CLI process PID to the session record
4. Calls `setup_interactive_worktree()` — a shared function extracted from the autorun worker
5. Sets four environment variables before exec-ing claude
6. Changes directory to the worktree root
7. Replaces the current process with `claude` via `exec`

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
        +-- Register InteractiveSession in SurrealDB
        |     models/interactive_session.rs (new)
        |     Fields: session_id, pid=current_pid, status="starting",
        |             worktree_path="", branch="", work_type="",
        |             team_name="", created_at=now
        |
        +-- setup_interactive_worktree(project_dir, session_id)
        |     worktree/setup.rs (new pub fn)
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
        |     |     worktree/setup.rs:511 (fn definition)
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
        +-- Update InteractiveSession: worktree_path={wt_path}, status="active"
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
              +-- Reads CODEFLOW_SESSION_ID from env (NOT stdin session_id)
              +-- SKIP: session ID generation (already done by CLI)
              +-- SKIP: worktree creation (already done by CLI)
              +-- SKIP: shared env write (worktrees don't touch shared file)
              +-- SKIP: stale worktree cleanup
              +-- SKIP: PID file write
              +-- DO: source local codeflow-env.sh for verification
              +-- DO: init pathflow state files
              +-- DO: output env JSON for hooks
```

### 3.3 Shared Code with Autorun

The worktree setup logic currently lives inside the autorun worker implementation. The `codeflow interactive` command extracts a shared function `setup_interactive_worktree()` from `worktree/setup.rs`, which both paths call.

**What autorun does vs what interactive does:**

| Step | Autorun Worker | codeflow interactive |
|------|---------------|---------------------|
| Session ID source | `AUTORUN_SESSION_ID` env var (pre-assigned by orchestrator) | Generated fresh via ULID by CLI |
| DB registration | `AutorunSession` record in SurrealDB | `InteractiveSession` record in SurrealDB |
| PID saved | `AutorunSession.pid` | `InteractiveSession.pid` |
| Worktree creation | `setup_interactive_worktree()` (after rename) | `setup_interactive_worktree()` (same fn) |
| Env var propagation | Set before worker thread spawns Claude | Set before `exec claude` |
| Task assignment | Pre-assigned via `AUTORUN_TASK_ID` | None — user drives via PathFlow |
| Branch | Pre-assigned via `AUTORUN_INTEGRATION_BRANCH` | Detached HEAD; branch set at PF3-CLASSIFY |
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
  +-- Parse stdin JSON for session_id   ← Claude's internal ID (not ses-{ulid})
  |
  +-- detect_project_dir()
  |     Step 1: CODEFLOW_WORKTREE_PATH env var
  |     Step 2: CF_PROJECT_ROOT env var
  |     Step 3: Walk CWD for .claude/.codeflow
  |     Step 4: Per-PID env file lookup      <-- fragile
  |     Step 5: Shared codeflow-env.sh read  <-- corruption source
  |
  +-- IF source = startup:
  |     +-- Generate/accept session_id
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
  |     +-- Read CODEFLOW_SESSION_ID from env  (CLI-generated ses-{ulid})
  |     +-- Read CODEFLOW_WORKTREE_PATH from env
  |     +-- SKIP: session ID generation (CLI already did this)
  |     +-- SKIP: worktree creation (CLI already did this)
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
        +-- Parse session_id from stdin JSON (existing behavior)
        +-- Write shared codeflow-env.sh (safe: single session at a time)
        +-- Init pathflow state (project-root paths)
        +-- Output env JSON
```

### 4.3 CODEFLOW_MANAGED Path

When `CODEFLOW_MANAGED=true`, the SessionStart hook can make these guarantees:

- `CODEFLOW_SESSION_ID` is a CodeFlow-format session ID (`ses-{ulid}`) — set by the CLI before exec
- `CODEFLOW_WORKTREE_PATH` is correct — set by the CLI before exec
- The worktree exists and is fully initialized — CLI completed `setup_interactive_worktree()`
- The local `codeflow-env.sh` at `{worktree}/.state/runtime/codeflow-env.sh` is already written
- An `InteractiveSession` record exists in SurrealDB for this session ID

The hook reads the local env file for verification (to catch any env var stripping by shell wrappers) but does not rewrite it.

Teammates spawned inside this session inherit all env vars. Their `SessionStart` also fires with `CODEFLOW_MANAGED=true`. The hook treats them identically to the lead — no teammate detection needed because the env vars are already correct.

### 4.4 Unmanaged Path (Plain `claude`)

When `CODEFLOW_MANAGED` is not set, the session is started by `claude` directly (no CLI wrapper). This is the legacy path. Behavior is unchanged from today except that PID file logic is removed:

- `detect_project_dir()` uses steps 1-3 only (env vars + CWD walk)
- SessionStart generates session ID from stdin JSON (existing behavior — no regression)
- The shared `codeflow-env.sh` is written (safe: only one unmanaged session should run at a time)
- No worktree is created
- Teammate detection via `lead_pid` comparison remains (no env var to check in this path)

The unmanaged path is a backward-compatibility provision. Users who run `claude` directly without `codeflow interactive` continue to get the existing single-session behavior.

### 4.5 Session ID Lifecycle

```
MANAGED PATH:
  codeflow -i
    └── CLI generates ses-{ulid}                 ← ALWAYS generated by CLI
    └── CLI registers InteractiveSession in DB
    └── CLI writes {worktree}/.state/runtime/codeflow-env.sh
    └── CLI sets CODEFLOW_SESSION_ID in process env
        └── exec claude (inherits CODEFLOW_SESSION_ID)
            └── SessionStart:
                  reads CODEFLOW_SESSION_ID from env var
                  NEVER generates a new session ID
                  ALL hooks use this value for the entire session
            └── Context overflow:
                  new process re-reads CODEFLOW_SESSION_ID from env
                  (inherited from parent shell that ran codeflow -i)
                  SessionStart managed path: reads env var, not stdin
                  Session ID is STABLE across context overflow

UNMANAGED PATH:
  claude
    └── SessionStart parses session_id from stdin JSON
    └── SessionStart writes to .state/runtime/codeflow-env.sh
        └── hooks read this file
            └── PROBLEM: second session overwrites file
            └── Mitigated: only one unmanaged session should run at a time
```

Context overflow in the managed path is safe because env vars are inherited from the shell that ran `codeflow -i`. The new Claude Code process re-inherits `CODEFLOW_SESSION_ID` and `CODEFLOW_WORKTREE_PATH` without reading any file.

→ Back to [Table of Contents](#table-of-contents)

---

## 5. Hook Audit: What Changes Per Hook

### 5.1 SessionStart (Major Changes)

**File:** `codeflow-cli/core/src/hooks/session_start.rs`

**Changes:**

| Change | Before | After |
|--------|--------|-------|
| Managed detection | None | Check `CODEFLOW_MANAGED` env var at top |
| Session ID resolution | Parse stdin JSON | Managed: read `CODEFLOW_SESSION_ID` from env; Unmanaged: parse stdin (unchanged) |
| Shared env write | Always write `.state/runtime/codeflow-env.sh` | Managed: skip; Unmanaged: write (unchanged) |
| Per-PID env file write | Write `codeflow-env-{pid}.sh` | Remove entirely |
| Worktree creation | Create worktree if mode=always and not teammate | Managed: skip (done by CLI); Unmanaged: unchanged |
| Stale cleanup | Run on every startup | Managed: skip; Unmanaged: run (unchanged) |
| Teammate detection | `pending_tmux_count` + `lead_pid` comparison | Managed: skip (env vars are correct); Unmanaged: unchanged |
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
    // Read session ID from env — never from stdin
    let session_id = std::env::var("CODEFLOW_SESSION_ID")?;
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH")?;
    // Verify local env file is present (defensive check)
    // Init pathflow state files at worktree-local paths
    // Output env JSON
    Ok(())
}

fn handle_unmanaged_session_start(payload: &SessionStartPayload) -> Result<()> {
    // Existing logic, minus PID file writes
    // session_id still comes from payload (stdin JSON)
}
```

### 5.2 SessionEnd (Remove PID Cleanup)

**File:** `codeflow-cli/core/src/hooks/session_end.rs`

**Changes:**

- Remove: cleanup of per-PID env file (`codeflow-env-{pid}.sh`)
- No other changes

The worktree cleanup for managed sessions is handled by `codeflow interactive cleanup` or the `WorktreeRegistry` TTL, not by the SessionEnd hook.

### 5.3 PreToolUse (No Changes)

**File:** `codeflow-cli/core/src/hooks/pre_tool_use.rs`

`detect_project_dir()` already checks `CODEFLOW_WORKTREE_PATH` first (step 1). With the managed path, this env var is always set and correct. No hook logic changes are needed.

The gate-check sentinel path uses the session-local `.state/sentinels/` directory, which resolves correctly via `WorktreePaths` when `CODEFLOW_WORKTREE_PATH` is set.

### 5.4 PostToolUse (Add Branch Update at pf-3)

**File:** `codeflow-cli/core/src/hooks/post_tool_use.rs`

**New behavior:** When the checkpoint system creates the `pathflow-pf-3` sentinel (indicating PF3-CLASSIFY is complete and a feature branch has been created), PostToolUse calls `update_branch_from_current()` to record the current git branch in the worktree registry entry.

**Why:** In the managed path, the worktree starts on a detached HEAD. At PF3-CLASSIFY, `cf-git-operations` creates and switches to the feature branch. The registry entry's `branch` field should be updated to reflect the actual branch name for visibility in `codeflow worktree list`. The `InteractiveSession` record's `branch` field should also be updated.

```rust
// In checkpoint-register PostToolUse handler
if sentinel_just_created == "pathflow-pf-3" {
    if let Ok(wt_path) = std::env::var("CODEFLOW_WORKTREE_PATH") {
        let _ = update_branch_from_current(&wt_path, &registry_path);
        // Also update InteractiveSession.branch in DB
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

- `session/env.rs`: remove `write_pid_env_file()`
- `cli/helpers.rs`: remove `read_worktree_path_from_pid_file()` (defined at `helpers.rs:76`)
- `session_start.rs`: remove per-PID write call
- `session_end.rs`: remove per-PID cleanup call
- `helpers.rs`: remove step 4 from `detect_project_dir()`

### 6.2 Shared codeflow-env.sh in Worktree Mode (Removed)

**File:** `.state/runtime/codeflow-env.sh` (in the main repo)

In worktree mode (managed path), this file is never written. Each worktree has its own copy at `{worktree}/.state/runtime/codeflow-env.sh`. The shared file retains its role only for the unmanaged path (single non-worktree sessions).

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

### 6.4 Teammate Detection via pending_tmux_count (Removed)

**Current mechanism:** `SessionStart` counts the number of pending tmux panes to determine whether the current process is the lead or a teammate. If `pending_tmux_count > 0`, the session is a teammate.

**Why removed for managed path:** In the managed path, every process that fires `SessionStart` is running inside the worktree with the correct env vars already set. Teammate identification can be determined by checking whether `pathflow-session-status.json` already exists for this session ID. If it does, the firing process is a teammate; if not, it is the lead.

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
        +-- Parse session_id from stdin JSON
        +-- Write .state/runtime/codeflow-env.sh
        |     CODEFLOW_SESSION_ID=ses-ABC   (from stdin JSON)
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
        NOTE: In this path, session ID is generated by SessionStart hook
        from Claude's stdin JSON — not by the CLI.
```

### 7.2 Single codeflow -i Session

```
USER
  └── codeflow -i (from project root)
        |
        +-- Read parallel-work-config.json → mode=always
        +-- Generate session_id = ses-AAA       ← CLI generates ID
        +-- Register InteractiveSession in DB
        |     { session_id: "ses-AAA", pid: 12345, status: "starting" }
        +-- setup_interactive_worktree()
        |     → .git-worktrees/worktree-ses-AAA/ created
        |     → .state/runtime/codeflow-env.sh written (worktree-local)
        |     → registered in .state/worktrees/worktrees.yaml
        +-- Update InteractiveSession: status="active", worktree_path=...
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
              +-- session_id = ses-AAA (from CODEFLOW_SESSION_ID env var)
              +-- NEVER regenerates session ID from stdin
              +-- worktree = .../.git-worktrees/worktree-ses-AAA (from env)
              +-- SKIP all worktree setup
              +-- Init pathflow at worktree-ses-AAA/.state/
              |
              All hooks:
                detect_project_dir() → Step 1 → worktree-ses-AAA/ ✓
                Sentinels at: worktree-ses-AAA/.state/sentinels/ses-AAA/
                |
              PF3-CLASSIFY: branch created
                PostToolUse updates registry entry + InteractiveSession.branch
              |
              Session continues normally
              |
              SessionEnd → worktree cleanup via codeflow interactive cleanup

RESULT: Fully isolated. Session ID stable. No shared state written.
```

### 7.3 Two Parallel codeflow -i Sessions

```
USER TERMINAL 1                    USER TERMINAL 2
  └── codeflow -i                    └── codeflow -i
        ses-AAA (CLI generated)             ses-BBB (CLI generated)
        worktree-ses-AAA                    worktree-ses-BBB
        CODEFLOW_SESSION_ID=ses-AAA         CODEFLOW_SESSION_ID=ses-BBB
        CODEFLOW_WORKTREE_PATH=.../AAA      CODEFLOW_WORKTREE_PATH=.../BBB
        CODEFLOW_MANAGED=true               CODEFLOW_MANAGED=true
        |                                   |
        exec claude                         exec claude
        |                                   |
        SessionStart → managed path         SessionStart → managed path
        session_id from env: ses-AAA        session_id from env: ses-BBB
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
        InteractiveSession ses-AAA          InteractiveSession ses-BBB
        |                                   |
        ZERO SHARED MUTABLE STATE           ZERO SHARED MUTABLE STATE
        (db/, coordination/ symlinked       (same — shared but read-optimized
         but SurrealDB RetryConfig           via RetryConfig)
         handles parallel access)

RESULT: Complete isolation. No corruption possible.
        Sessions are independent; shared DB access serialized by RetryConfig.
        Both InteractiveSessions coexist in DB with distinct session IDs.
```

### 7.4 In-Process Teammates

Claude Code spawns subagents as in-process threads (not separate OS processes). In-process subagents share the parent process's environment.

```
codeflow -i → ses-AAA (CLI generated) → exec claude (lead)
                |
                +-- CODEFLOW_SESSION_ID=ses-AAA  (inherited by all children)
                +-- CODEFLOW_WORKTREE_PATH=.../AAA
                +-- CODEFLOW_MANAGED=true
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
        +-- session_id = ses-AAA (from inherited env var — not stdin)
        +-- worktree = .../AAA (from inherited env var)
        +-- No new worktree created; no new session ID generated
        +-- Pathflow state at .../AAA/.state/ (correct session-local paths)
        |
        detect_project_dir() in tmux pane:
          Step 1: CODEFLOW_WORKTREE_PATH=.../AAA → return .../AAA ✓

RACE CONDITION NOTE: tmux may not inherit env vars if the pane is opened
in a fresh shell. See Section 8.2 for the CWD inheritance race.
```

### 7.6 Autorun Workers

Autorun already sets `CODEFLOW_SESSION_ID`, `CODEFLOW_WORKTREE_PATH`, and `CODEFLOW_MANAGED=true` before spawning the worker. After this change, autorun workers benefit from the simplified SessionStart managed path, and also use the renamed env vars (→ see [Section 13](#13-autorun-field-renaming)).

```
codeflow autorun (orchestrator)
  |
  +-- Generate session_id per worker = ses-W1, ses-W2, ...
  +-- setup_interactive_worktree() (shared fn — same as interactive)
  |     → worktree-ses-W1/ created
  +-- Write autorun-worker-env.sh (autorun.rs:2624)
  |     export AUTORUN_SESSION_ID='ses-W1'
  |     export AUTORUN_BATCH_ID='{batch_session_id}'
  |     export AUTORUN_TASK_ID='{task_id}'
  |     export AUTORUN_ACCEPTANCE='{b64}'
  |     export AUTORUN_INTEGRATION_BRANCH='{target}'     ← renamed
  |     export AUTORUN_INTEGRATION_AUTO_MERGE='{bool}'   ← renamed
  |     export AUTORUN_EPIC_UPDATE='{strategy}'
  |     export CODEFLOW_WORKTREE_PATH='{wdir}'
  +-- Set CODEFLOW_MANAGED=true
  |
  +-- Worker spawns Claude for ses-W1
        |
        SessionStart fires
        |
        +-- CODEFLOW_MANAGED=true → managed path (same as interactive)
        +-- session_id from CODEFLOW_SESSION_ID env var
        +-- No duplicate worktree setup; no new session ID generated
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
    +-- IF new subprocess: inherits env from shell (codeflow -i still alive)
        → ses-AAA ✓
    |
    SessionStart fires with source=compact
    |
    +-- CODEFLOW_MANAGED=true → managed path
    +-- session_id = ses-AAA (from CODEFLOW_SESSION_ID env var)
    +-- NEVER generates a new session ID
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
            session_id from stdin JSON (not CLI-generated)
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

VERDICT: Safe. The managed session is completely unaffected by the
unmanaged session. The unmanaged session's shared env file write does
not corrupt terminal 1 because terminal 1's hooks never read that file.

CAVEAT: Two simultaneous unmanaged sessions still corrupt each other.
Users who need parallelism should use `codeflow -i` exclusively.
```

### 7.9 Max Worktrees Reached

`locked_register_with_limit()` enforces a maximum of 5 concurrent worktrees.

```
codeflow -i (6th attempt)
  |
  +-- Generate session_id = ses-FFF  (CLI always generates ID first)
  +-- Register InteractiveSession in DB: { status: "starting" }
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
  Update InteractiveSession: status="failed", error="LimitReached"
  |
  codeflow -i prints:
    "Error: Maximum concurrent sessions reached (5).
     Run 'codeflow interactive list' to see active sessions.
     Run 'codeflow interactive cleanup' to remove completed sessions."
  |
  Partial worktree (git-level) needs cleanup:
    codeflow interactive performs git worktree remove on error path

USER CORRECTIVE ACTION:
  codeflow interactive list           → see all sessions with status
  codeflow interactive cleanup {id}   → remove a completed session
  codeflow -i                         → retry (now succeeds with 4 active)
```

→ Back to [Table of Contents](#table-of-contents)

---

## 8. Race Condition Analysis

### 8.1 Parallel Worktree Registration

**Scenario:** Two `codeflow -i` commands run simultaneously (two terminal windows launched at the exact same instant).

```
T=0: codeflow -i (A) and codeflow -i (B) both start
T=1: Both generate distinct session IDs (ULID: collision-resistant)
     ses-AAA (48-bit timestamp + 80-bit random) vs ses-BBB
T=2: Both call locked_register_with_limit()
     └── Uses file locking (flock) on .state/worktrees/worktrees.yaml
         One wins the lock, registers, releases
         The other waits, acquires lock, registers
T=3: Both registrations succeed (assuming count < 5)
T=4: Both exec claude with different CODEFLOW_SESSION_ID values
```

**Verdict: Safe.** `locked_register_with_limit` uses OS-level file locking. The ULID-based session ID generation provides sufficient uniqueness.

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
| New subprocess (fork/exec) | Child inherits parent env from `codeflow -i` shell | Yes |
| Remote execution (cloud backend) | Env vars may not persist | Potentially No |

For local execution (the only current mode), env vars persist across context overflow. The `codeflow interactive` shell process remains alive and holds the env in its process table.

**Mitigation for remote execution (future):** The local env file at `{worktree}/.state/runtime/codeflow-env.sh` is the fallback. SessionStart's managed path already sources this file for verification. In remote mode, the hook should read from this file if env vars are not set.

### 8.4 Stale Session Cleanup

**Scenario:** A session crashes (power loss, kill -9). The worktree remains registered but the `codeflow -i` process is gone.

```
Crash: worktree-ses-AAA registered, InteractiveSession status="active"
       codeflow -i process dead (pid 12345 no longer exists)
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

MITIGATION: codeflow interactive list checks PID liveness via
liveness.rs::is_session_alive(pid). Stale sessions (pid dead) are
shown with status "stale" and can be removed:
  codeflow interactive cleanup --prune-stale
```

→ Back to [Table of Contents](#table-of-contents)

---

## 9. PID/Heartbeat Liveness: What Remains

With teammate detection removed from the managed path, the only remaining use of PID-based liveness is **stale session cleanup** — called from administrative commands, never from the hook hot path.

| Use Case | Before | After |
|----------|--------|-------|
| Teammate detection | PID comparison + tmux count | Removed (use CODEFLOW_MANAGED env var instead) |
| Stale worktree detection | PID file existence | `session/liveness.rs::is_session_alive(pid)` from `InteractiveSession.pid` + heartbeat at `.state/interactive/heartbeat-{SID}` |
| SessionEnd PID cleanup | Clean up `codeflow-env-{pid}.sh` | Removed (no PID files) |
| Dead worker claim release | Heartbeat TTL + `cleanup_dead_workers()` | Retained (autorun workers) |
| Interactive session liveness | PID-only | Heartbeat file at `.state/interactive/heartbeat-{SID}` written by SessionStart; enables `codeflow interactive cleanup` to detect stale sessions |

**What `session/liveness.rs` does after this change:**

The `liveness.rs` module (`codeflow-cli/core/src/session/liveness.rs`) is retained but simplified:

```rust
// Retained: check if a session's lead process is still alive
pub fn is_session_alive(lead_pid: u32) -> bool {
    // kill(pid, 0) — POSIX liveness check
    // Used by: codeflow interactive cleanup --prune-stale
    //          codeflow interactive list (stale status display)
    //          codeflow doctor --interactive
}
```

Autorun uses `stale.rs::check_pid_alive(pid: i64)` (verified at `stale.rs:50`) for the same check via the `StaleSessionInfo` pattern. The interactive session cleanup reuses this function.

The `heartbeat.rs` module (`codeflow-cli/core/src/session/heartbeat.rs`) is retained for both autorun workers and interactive sessions. The SessionStart hook writes a heartbeat file at `.state/interactive/heartbeat-{SID}` for managed interactive sessions. This heartbeat enables `codeflow interactive cleanup` to detect stale sessions reliably — it complements PID liveness (which requires the process to still exist) with a time-bounded file that survives brief process pauses. Autorun workers use heartbeat TTL for claim release via `cleanup_dead_workers()`.

**Summary:** PID/liveness and heartbeat are cleanup tools, not routing mechanisms. They are no longer called in the hook hot path. They are called only from administrative commands (`interactive cleanup`, `interactive list`, `doctor`). The heartbeat file at `.state/interactive/heartbeat-{SID}` is written by SessionStart for managed interactive sessions and provides time-bounded liveness detection alongside PID checks.

→ Back to [Table of Contents](#table-of-contents)

---

## 10. File Change Map

### 10.1 New Files

| File | Description |
|------|-------------|
| `codeflow-cli/cli/src/cmd/interactive.rs` | `codeflow interactive` / `codeflow -i` command (~200 lines). Subcommands: run (exec claude), status, cleanup, list. |
| `codeflow-cli/core/src/models/interactive_session.rs` | `InteractiveSession` DB model (~60 lines). Fields: session_id, pid, status, worktree_path, branch, work_type, team_name, created_at, started_at, completed_at. |

### 10.2 Modified Files

| File | Approx Lines | Change Type | Details |
|------|-------------|-------------|---------|
| `codeflow-cli/cli/src/cmd/mod.rs` | — | Add | Register `interactive` subcommand; add `-i` alias |
| `codeflow-cli/core/src/worktree/setup.rs` | — | Add fn | Extract `pub fn setup_interactive_worktree()` wrapping 5-step setup chain; called by `interactive.rs` and refactored `autorun.rs` |
| `codeflow-cli/core/src/hooks/session_start.rs` | — | Major modify | Add `CODEFLOW_MANAGED` branch; split into `handle_managed_session_start()` and `handle_unmanaged_session_start()`; remove PID file writes; remove stale cleanup from managed path; remove `pending_tmux_count` from managed path; managed path reads session ID from env (not stdin) |
| `codeflow-cli/core/src/hooks/session_end.rs` | — | Modify | Remove per-PID env file cleanup |
| `codeflow-cli/core/src/hooks/post_tool_use.rs` | — | Modify | Add `update_branch_from_current()` + `InteractiveSession.branch` update when `pathflow-pf-3` sentinel created |
| `codeflow-cli/core/src/hooks/task_completed.rs` | — | Modify | Add same branch update when `pathflow-pf-3` sentinel created via checkpoint-complete |
| `codeflow-cli/core/src/session/env.rs` | — | Remove fn | Remove `write_pid_env_file()` |
| `codeflow-cli/cli/src/helpers.rs` | — | Simplify | Remove step 4 from `detect_project_dir()` (per-PID file lookup); remove `read_worktree_path_from_pid_file()` (defined at `helpers.rs:76`) |
| `codeflow-cli/core/src/models/mod.rs` | — | Add | Export `InteractiveSession` |
| **Autorun field rename — see [Section 13.3](#133-blast-radius) for full table** | | | |
| `codeflow-cli/core/src/autorun/batch.rs` | 1942 | Rename fields | `BatchFile`: `target`→`integration_branch`, `auto_merge`→`integration_auto_merge`, `final_pr_base`→`final_pr_target`; `ParsedBatch`: same + `target_is_auto`→`integration_branch_is_auto`; ~80 lines of field references across struct, parse, resolve_target, tests |
| `codeflow-cli/core/src/autorun/orchestrator.rs` | 2146 | Rename fields | `WorkerConfig`: `target`→`integration_branch`, `auto_merge`→`integration_auto_merge`; ~60 field references in execute, dispatch, worker spawn logic |
| `codeflow-cli/core/src/autorun/worker.rs` | 3031 | Rename fields | `InvokeConfig`: `target`→`integration_branch`, `auto_merge`→`integration_auto_merge`; env file writer uses new names; ~40 field references |
| `codeflow-cli/cli/src/cmd/autorun.rs` | 6091 | Rename fields + env vars | `auto_merge`→`integration_auto_merge` in struct constructors and test fixtures (~100 occurrences); env var writes at line 2629-2630 renamed; `check_target_branch` → `check_integration_branch`; ~150 total changes |
| `codeflow-cli/core/src/models/autorun.rs` | 333 | Rename fields | `AutorunSession.target_branch`→`integration_branch`; add `final_pr_target` field; ~15 changes |
| `.codeflow/config/autorun/README.md` | — | Update | Inference table, autonomous defaults section (already corrected in rework) |
| `.codeflow/config/autorun/examples/minimal-autonomous.yaml` | — | Update | `final_pr` comment corrected (already applied); field name references if any |
| `.codeflow/config/autorun/examples/simple-sequential.yaml` | — | Update | `final_pr` comment corrected (already applied); field name references if any |
| `.codeflow/config/autorun/examples/custom-integration.yaml` | — | Update | `target`→`integration_branch`, `auto_merge`→`integration_auto_merge` in YAML keys and comments |
| `.codeflow/config/autorun/examples/complex-dependencies.yaml` | — | Update | No batch-level keys to rename; update comments referencing `target` |
| `.codeflow/config/autorun/examples/direct-to-main.yaml` | — | Update | `target`→`integration_branch` YAML key |
| `.codeflow/config/autorun/examples/manual-review.yaml` | — | Update | `auto_merge`→`integration_auto_merge` YAML key |
| `.codeflow/config/autorun/batches/inf-epc-024-phase1-audits.yaml` | — | Update | Comments referencing `target`/`auto_merge` |
| `.codeflow/config/autorun/batches/inf-epc-024-retention-policies.yaml` | — | Update | Comments referencing `target`/`auto_merge` |
| `.codeflow/config/autorun/local/inf-epc-024-phase1-audits.yaml` | — | Update | Comments referencing `target`/`auto_merge` |
| `.claude/CLAUDE.md` | — | Update | Section 4.4 env var table: `AUTORUN_TARGET`→`AUTORUN_INTEGRATION_BRANCH`, `AUTORUN_AUTO_MERGE`→`AUTORUN_INTEGRATION_AUTO_MERGE`; Section 6.7 PF6-COMPLETE autorun description |
| `.claude/agents/cf-git-operations.md` | — | Update | Line 389: `AUTORUN_AUTO_MERGE` → `AUTORUN_INTEGRATION_AUTO_MERGE` |

### 10.3 Deleted Code

| Location | Code Removed | Reason |
|----------|-------------|--------|
| `session/env.rs` | `write_pid_env_file()` function | PID files eliminated |
| `cli/helpers.rs` | `read_worktree_path_from_pid_file()` function (at `helpers.rs:76`) | PID files eliminated; function was only in helpers.rs, not session/env.rs |
| `session/process.rs` | `get_claude_code_pid()` function (or demoted to test-only) | No longer called in production path |
| `hooks/session_start.rs` | PID file write call | PID files eliminated |
| `hooks/session_start.rs` | `pending_tmux_count` detection logic (managed path only) | Replaced by env var check |
| `hooks/session_start.rs` | Stale worktree cleanup (managed path only) | Managed by CLI commands |
| `hooks/session_end.rs` | PID file cleanup | PID files eliminated |
| `helpers.rs` | `detect_project_dir()` step 4 | PID files eliminated |

→ Back to [Table of Contents](#table-of-contents)

---

## 11. Implementation Plan

### Priority Order

| Priority | Change | Depends On | Risk |
|----------|--------|-----------|------|
| P0 | `setup_interactive_worktree()` shared fn in `worktree/setup.rs` | Nothing | Low — extraction only |
| P0 | `InteractiveSession` model in `models/interactive_session.rs` | Nothing | Low — new code |
| P0 | `session_start.rs` CODEFLOW_MANAGED branch | Nothing | Medium — core hook change |
| P1 | `cli/src/cmd/interactive.rs` new command | `setup_interactive_worktree()`, `InteractiveSession` | Low — new code |
| P1 | `cli/src/cmd/mod.rs` register subcommand | `interactive.rs` | Low |
| P2 | Remove PID file functions from `session/env.rs` | Managed path tested and working | Medium — removes existing code |
| P2 | Simplify `detect_project_dir()` (remove step 4) | PID files removed | Medium |
| P2 | `session_end.rs` remove PID cleanup | PID files removed | Low |
| P3 | Autorun field rename (all files in Section 13.3) | P0 complete | Medium — wide blast radius |
| P3 | `autorun.rs` refactor to shared `setup_interactive_worktree()` | `setup_interactive_worktree()` | Low |
| P3 | `post_tool_use.rs` branch update at pf-3 | pf-3 sentinel logic | Low |
| P3 | `task_completed.rs` branch update at pf-3 | pf-3 sentinel logic | Low |

### Dependencies Between Changes

```
InteractiveSession model    ←── interactive.rs (DB registration)
setup_interactive_worktree() ←── interactive.rs
       │                      ←── autorun/worker.rs (refactor)
       │
       ▼
session_start.rs MANAGED path ←── All managed-path scenarios

       ├─── session/env.rs PID removal (after managed path is stable)
       │         │
       │         ▼
       │    detect_project_dir() step 4 removal
       │    session_end.rs cleanup removal
       │
       ├─── post_tool_use.rs branch update (independent)
       │    task_completed.rs branch update (independent)
       │
       └─── Autorun field rename (independent — can be done in parallel)
```

### Required Test Cases

| Test | File | Covers |
|------|------|--------|
| `test_managed_session_start_uses_env_vars` | `session_start.rs` tests | CODEFLOW_MANAGED path reads session ID from env, not stdin |
| `test_managed_session_start_never_generates_id` | `session_start.rs` tests | No new session ID generated in managed path |
| `test_unmanaged_session_start_unchanged` | `session_start.rs` tests | Unmanaged path: session ID from stdin JSON, shared env written |
| `test_detect_project_dir_3_steps_only` | `helpers.rs` tests | No step 4 |
| `test_setup_interactive_worktree_creates_structure` | `worktree/setup.rs` tests | Full worktree setup |
| `test_setup_interactive_worktree_limit_enforced` | `worktree/setup.rs` tests | Max 5 rejected |
| `test_interactive_cmd_registers_session` | `cmd/interactive.rs` tests | InteractiveSession written to DB before exec |
| `test_interactive_cmd_exec_args` | `cmd/interactive.rs` tests | Correct env vars passed to exec |
| `test_two_parallel_sessions_no_shared_state` | Integration test | Core correctness guarantee |
| `test_context_overflow_env_persistence` | Integration test | Session ID stable after compaction (env var survives) |
| `test_autorun_field_rename_parse` | `batch.rs` tests | `integration_branch` and `integration_auto_merge` parse correctly |
| `test_autorun_env_var_names` | `autorun.rs` tests | `AUTORUN_INTEGRATION_BRANCH` and `AUTORUN_INTEGRATION_AUTO_MERGE` written to env file |

→ Back to [Table of Contents](#table-of-contents)

---

## 12. Related

- [PID detection audit](../../memory/project_pid_detection_audit.md) — 7 writers (6 broken), 11 readers, centralized liveness fix; this design supersedes the per-PID file mitigation approach
- [Session hook lifecycle](../../memory/project_session_hook_lifecycle.md) — SessionStart/SessionEnd fire only for lead; teammates get SubagentStart/SubagentStop
- [Worktree path resolution feedback](../../memory/feedback_worktree_path_resolution.md) — `detect_project_dir()` is the single path resolution function for ALL hooks; changes here affect every hook
- [Staging worktree targeting feedback](../../memory/feedback_staging_worktree_targeting.md) — staging cp must target `$CODEFLOW_WORKTREE_PATH` in worktree mode
- [Autorun integration branch](../../memory/project_autorun_integration_branch.md) — PR #255; autorun workers already use CODEFLOW_MANAGED pattern; this design unifies interactive and autorun

→ Back to [Table of Contents](#table-of-contents)

---

## 13. Autorun Field Renaming

### 13.1 Rationale

The current autorun batch schema uses `target` and `auto_merge` as top-level batch fields. These names are ambiguous:

- `target` — does this mean the branch workers target for their PRs, or the final merge destination?
- `auto_merge` — auto-merge of worker PRs, or auto-merge of the final PR?

With the introduction of `final_pr`, `final_pr_base`, and `target_is_auto`, the confusion compounds. Renaming makes the relationship explicit:

| Old Name | New Name | Meaning (explicit) |
|----------|----------|-------------------|
| `target` | `integration_branch` | The branch worker PRs merge into (auto-generated or explicit) |
| `auto_merge` | `integration_auto_merge` | Whether worker PRs auto-merge to the integration branch |
| `final_pr_base` | `final_pr_target` | The branch the final PR merges into (always `main` or a release branch) |
| `target_is_auto` | `integration_branch_is_auto` | Whether `integration_branch` was auto-generated (internal flag) |

YAML-level benefit: a batch file now reads clearly:

```yaml
integration_branch: autorun/sprint-42   # workers merge here
integration_auto_merge: true            # automatically after CI
final_pr: true                          # then a summary PR to:
final_pr_target: main                   # main
```

### 13.2 Field Mapping Table

| Scope | Old | New | Env Var (old) | Env Var (new) |
|-------|-----|-----|--------------|--------------|
| `BatchFile` struct | `target: String` | `integration_branch: String` | N/A | N/A |
| `BatchFile` struct | `auto_merge: Option<bool>` | `integration_auto_merge: Option<bool>` | N/A | N/A |
| `BatchFile` struct | `final_pr_base: String` | `final_pr_target: String` | N/A | N/A |
| `ParsedBatch` struct | `target: String` | `integration_branch: String` | N/A | N/A |
| `ParsedBatch` struct | `auto_merge: bool` | `integration_auto_merge: bool` | N/A | N/A |
| `ParsedBatch` struct | `final_pr_base: String` | `final_pr_target: String` | N/A | N/A |
| `ParsedBatch` struct | `target_is_auto: bool` | `integration_branch_is_auto: bool` | N/A | N/A |
| `WorkerConfig` struct | `target: String` | `integration_branch: String` | N/A | N/A |
| `WorkerConfig` struct | `auto_merge: bool` | `integration_auto_merge: bool` | N/A | N/A |
| `InvokeConfig` struct | `target: String` | `integration_branch: String` | N/A | N/A |
| `InvokeConfig` struct | `auto_merge: bool` | `integration_auto_merge: bool` | N/A | N/A |
| `AutorunSession` model | `target_branch: Option<String>` | `integration_branch: Option<String>` | N/A | N/A |
| Worker env file | exported as `AUTORUN_TARGET` | exported as `AUTORUN_INTEGRATION_BRANCH` | `AUTORUN_TARGET` | `AUTORUN_INTEGRATION_BRANCH` |
| Worker env file | exported as `AUTORUN_AUTO_MERGE` | exported as `AUTORUN_INTEGRATION_AUTO_MERGE` | `AUTORUN_AUTO_MERGE` | `AUTORUN_INTEGRATION_AUTO_MERGE` |
| YAML batch schema | `target:` key | `integration_branch:` key | N/A | N/A |
| YAML batch schema | `auto_merge:` key | `integration_auto_merge:` key | N/A | N/A |
| YAML batch schema | `final_pr_base:` key | `final_pr_target:` key | N/A | N/A |

### 13.3 Blast Radius

All files requiring changes due to the field rename. Approximate line counts are the number of occurrences (not file line count):

| File | Occurrences | Change Type |
|------|-------------|-------------|
| `codeflow-cli/core/src/autorun/batch.rs` | ~80 | Struct field renames in `BatchFile`, `ParsedBatch`; field references in `parse_batch_data_with_project_dir()`, `resolve_target()`, `validate_batch()`, `build_resume_batch()`, and test fixtures |
| `codeflow-cli/core/src/autorun/orchestrator.rs` | ~60 | `WorkerConfig` struct field renames; field references in `execute_with_batch_file()`, dispatch loop, worker spawn, and test fixtures |
| `codeflow-cli/core/src/autorun/worker.rs` | ~40 | `InvokeConfig` struct field renames; env file writer at line 2629-2630 analog; all trait impl methods referencing `target`/`auto_merge` |
| `codeflow-cli/cli/src/cmd/autorun.rs` | ~150 | Env var writes (line 2629-2630 → `AUTORUN_INTEGRATION_BRANCH`, `AUTORUN_INTEGRATION_AUTO_MERGE`); struct constructors for `WorkerConfig`/`InvokeConfig`; `check_target_branch` → `check_integration_branch`; all test fixtures (lines ~3218, 3257, 3285, 3494, 3578, 3611, 3655, 3728, 6034, 6060) |
| `codeflow-cli/core/src/models/autorun.rs` | ~15 | `AutorunSession.target_branch` → `integration_branch`; add `final_pr_target` field; serde annotations |
| `codeflow-cli/core/src/autorun/epic_update.rs` | ~5 | Any references to `target` or `auto_merge` fields from `ParsedBatch`/`WorkerConfig` |
| `codeflow-cli/core/src/autorun/config.rs` | ~5 | Any `target` or `auto_merge` field references in config loading |
| `codeflow-cli/core/src/autorun/stale.rs` | ~5 | `AutorunSession.target_branch` → `integration_branch` in stale session display |
| `.codeflow/config/autorun/README.md` | ~15 | Inference table (already partially corrected); autonomous defaults section; all YAML examples in README body |
| `.codeflow/config/autorun/examples/minimal-autonomous.yaml` | ~3 | Comment updated (applied); no YAML keys to rename (omitted in this file) |
| `.codeflow/config/autorun/examples/simple-sequential.yaml` | ~3 | Comment updated (applied); no YAML keys to rename (omitted in this file) |
| `.codeflow/config/autorun/examples/custom-integration.yaml` | ~4 | `target: autorun/sprint-42` → `integration_branch: autorun/sprint-42`; `auto_merge: true` → `integration_auto_merge: true`; update comments |
| `.codeflow/config/autorun/examples/complex-dependencies.yaml` | ~2 | Update comments referencing `target`; no YAML keys (omitted) |
| `.codeflow/config/autorun/examples/direct-to-main.yaml` | ~3 | `target: main` → `integration_branch: main`; update comments |
| `.codeflow/config/autorun/examples/manual-review.yaml` | ~3 | `auto_merge: false` → `integration_auto_merge: false`; update comments |
| `.codeflow/config/autorun/batches/inf-epc-024-phase1-audits.yaml` | ~2 | Update comments referencing `target`/`auto_merge` |
| `.codeflow/config/autorun/batches/inf-epc-024-retention-policies.yaml` | ~2 | Update comments referencing `target`/`auto_merge` |
| `.codeflow/config/autorun/local/inf-epc-024-phase1-audits.yaml` | ~2 | Update comments referencing `target`/`auto_merge` |
| `.claude/CLAUDE.md` | ~6 | Env var table (Section 4.4): `AUTORUN_TARGET`→`AUTORUN_INTEGRATION_BRANCH`, `AUTORUN_AUTO_MERGE`→`AUTORUN_INTEGRATION_AUTO_MERGE`; Section 6.7 PF6-COMPLETE autorun description; Section 4.4 autorun env var descriptions |
| `.claude/agents/cf-git-operations.md` | ~2 | Line 389: `AUTORUN_AUTO_MERGE`→`AUTORUN_INTEGRATION_AUTO_MERGE`; any `AUTORUN_TARGET` references |
| `.codeflow/docs/analysis/parallel-work/autorun-integration.md` | ~20 | All `AUTORUN_TARGET`/`AUTORUN_AUTO_MERGE` references in analysis document |
| `project-management/epics/INF/INF-EPC-022/tasks/INF-TSK-022-032.md` | ~8 | Acceptance criteria and DEV/REV report referencing `AUTORUN_TARGET`/`AUTO_MERGE` |

**Total estimated occurrences: ~450+**

### 13.4 Rust Struct Changes

**`BatchFile` (batch.rs, deserialized from YAML):**

```rust
// BEFORE
pub struct BatchFile {
    pub auto_merge: Option<bool>,
    pub target: String,
    pub final_pr_base: String,
    // ...
}

// AFTER
pub struct BatchFile {
    #[serde(default)]
    pub integration_auto_merge: Option<bool>,
    #[serde(default)]
    pub integration_branch: String,
    #[serde(default)]
    pub final_pr_target: String,
    // ...
}
```

**`ParsedBatch` (batch.rs, validated struct):**

```rust
// BEFORE
pub struct ParsedBatch {
    pub auto_merge: bool,
    pub target: String,
    pub target_is_auto: bool,
    pub final_pr_base: String,
    // ...
}

// AFTER
pub struct ParsedBatch {
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    pub integration_branch_is_auto: bool,
    pub final_pr_target: String,
    // ...
}
```

**`WorkerConfig` (orchestrator.rs):**

```rust
// BEFORE
pub struct WorkerConfig {
    pub auto_merge: bool,
    pub target: String,
    // ...
}

// AFTER
pub struct WorkerConfig {
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    // ...
}
```

**`InvokeConfig` (worker.rs):**

```rust
// BEFORE
pub struct InvokeConfig {
    pub auto_merge: bool,
    pub target: String,
    // ...
}

// AFTER
pub struct InvokeConfig {
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    // ...
}
```

**`AutorunSession` (models/autorun.rs):**

```rust
// BEFORE
pub target_branch: Option<String>,

// AFTER
pub integration_branch: Option<String>,
pub final_pr_target: Option<String>,  // new field
```

### 13.5 Environment Variable Changes

The worker env file written at `autorun.rs:2624` changes as follows:

```rust
// BEFORE (autorun.rs:2629-2630)
export AUTORUN_TARGET='{target}'
export AUTORUN_AUTO_MERGE='{auto_merge}'

// AFTER
export AUTORUN_INTEGRATION_BRANCH='{integration_branch}'
export AUTORUN_INTEGRATION_AUTO_MERGE='{integration_auto_merge}'
```

All consumers of these env vars (hook scripts, agent definitions, CLAUDE.md) must be updated in the same change.

**CLAUDE.md env var table update (Section 4.4):**

| Old Variable | New Variable | Description |
|-------------|-------------|-------------|
| `AUTORUN_TARGET` | `AUTORUN_INTEGRATION_BRANCH` | Branch worker PRs target; auto-generated or explicit |
| `AUTORUN_AUTO_MERGE` | `AUTORUN_INTEGRATION_AUTO_MERGE` | Whether worker PRs auto-merge after CI passes |

### 13.6 YAML Schema Changes

The batch file YAML schema changes affect all example files and production batch files. Users upgrading must rename their batch file keys:

```yaml
# BEFORE
name: my-batch
target: autorun/my-branch
auto_merge: true
final_pr: true
final_pr_base: main

# AFTER
name: my-batch
integration_branch: autorun/my-branch
integration_auto_merge: true
final_pr: true
final_pr_target: main
```

The `final_pr` key itself is unchanged. Only `target`→`integration_branch`, `auto_merge`→`integration_auto_merge`, and `final_pr_base`→`final_pr_target` change.

**Serde backward compatibility:** During the transition, `BatchFile` can use `#[serde(alias = "target")]` to accept both old and new YAML keys. This avoids breaking existing batch files in the field. The alias can be removed after all batch files are migrated.

### 13.7 Test Updates

All test fixtures in `autorun.rs` that construct `WorkerConfig` or `InvokeConfig` directly must be updated. Key test locations (verified from source):

| File | Line | What to Update |
|------|------|----------------|
| `autorun.rs` | 3218 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3257 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3285 | `auto_merge: true` → `integration_auto_merge: true` |
| `autorun.rs` | 3299 | `deserialized.auto_merge` → `deserialized.integration_auto_merge` |
| `autorun.rs` | 3494 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3578 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3611 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3655 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 3728 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 6034 | `auto_merge: false` → `integration_auto_merge: false` |
| `autorun.rs` | 6060 | `auto_merge: false` → `integration_auto_merge: false` |
| `batch.rs` | (all test fixtures) | `target` → `integration_branch`, `auto_merge` → `integration_auto_merge` |
| `orchestrator.rs` | (all test fixtures) | `target` → `integration_branch`, `auto_merge` → `integration_auto_merge` |

New tests to add:

- `test_batch_parses_integration_branch_key` — verify new YAML key name accepted
- `test_batch_parses_legacy_target_alias` — verify old `target:` key still works via serde alias
- `test_env_file_uses_integration_branch_var` — verify `AUTORUN_INTEGRATION_BRANCH` in written env file

### 13.8 CLAUDE.md Sections Affected

| Section | Location | Change Required |
|---------|----------|----------------|
| Section 4.4 (Autorun Mode env vars) | Table row `AUTORUN_TARGET` | Rename to `AUTORUN_INTEGRATION_BRANCH`; update description |
| Section 4.4 (Autorun Mode env vars) | Table row `AUTORUN_AUTO_MERGE` | Rename to `AUTORUN_INTEGRATION_AUTO_MERGE`; update description |
| Section 4.4 (per-phase autorun behavior) | PF6-COMPLETE row | `AUTORUN_AUTO_MERGE=true` → `AUTORUN_INTEGRATION_AUTO_MERGE=true` |
| Section 6.7 (PR Workflow) | PF6-TSK-07 description | `AUTORUN_AUTO_MERGE` → `AUTORUN_INTEGRATION_AUTO_MERGE` |

→ Back to [Table of Contents](#table-of-contents)

---

## 14. Interactive Session Management

### 14.1 InteractiveSession DB Model

A new SurrealDB model tracks interactive sessions, parallel to `AutorunSession` for autorun batches. This enables `codeflow interactive list/status/cleanup` subcommands and provides liveness detection without PID files.

**File:** `codeflow-cli/core/src/models/interactive_session.rs` (new, ~60 lines)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveSession {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    /// CodeFlow session ID (ses-{ulid}) — generated by CLI before exec.
    pub session_id: String,
    /// PID of the codeflow -i process that exec'd claude.
    pub pid: i64,
    /// Session lifecycle status.
    pub status: InteractiveSessionStatus,
    /// Absolute path to the git worktree root.
    pub worktree_path: Option<String>,
    /// Feature branch name (set at PF3-CLASSIFY via post_tool_use update).
    pub branch: Option<String>,
    /// Work type (FEAT, FIX, DOCS, etc.) — set at PF3-CLASSIFY.
    pub work_type: Option<String>,
    /// Team name from TeamCreate — set during PF1-INIT.
    pub team_name: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

pub enum InteractiveSessionStatus {
    Starting,   // CLI has generated ID, setup in progress
    Active,     // Worktree created, Claude running
    Completing, // PF6-COMPLETE in progress
    Completed,  // PF7-END reached
    Failed,     // Setup failed (e.g., LimitReached)
    Stale,      // PID dead, session never completed cleanly
}
```

The `AutorunSession` model at `codeflow-cli/core/src/models/autorun.rs` follows the same pattern — `InteractiveSession` mirrors its lifecycle fields.

### 14.2 New Subcommands

**File:** `codeflow-cli/cli/src/cmd/interactive.rs` (new)

```
codeflow interactive [run]     # default: exec claude in managed worktree
codeflow -i                    # short alias for codeflow interactive run

codeflow interactive list      # list all sessions (active, completed, stale)
codeflow interactive status    # status of current session (from CODEFLOW_SESSION_ID)
codeflow interactive cleanup [--session <id>] [--prune-stale] [--all-completed]
```

**`codeflow interactive list` output:**

```
SESSION              STATUS     BRANCH                    WORKTREE             AGE
ses-01knhh42ax6tz4y  active     feat/session-isolation    worktree-ses-01kn... 2h
ses-01knhg0f6dm359g  active     docs/template-alignment   worktree-ses-01kn... 3h
ses-01knXXXXXXXXXX  stale      (unknown)                 worktree-ses-01kn... 1d
```

**`codeflow interactive cleanup` behavior:**

- `--session <id>`: remove worktree, update DB record to `Completed`, release claims
- `--prune-stale`: check PID liveness for all active sessions; mark dead ones `Stale`, remove worktrees
- `--all-completed`: remove all worktrees for sessions with status `Completed`

### 14.3 Liveness Detection

Interactive session liveness uses the same `check_pid_alive()` function from `autorun/stale.rs:50`:

```rust
// Reuse from stale.rs
pub fn check_pid_alive(pid: i64) -> bool {
    let pid_i32 = pid as i32;
    if pid_i32 <= 0 { return false; }
    let ret = unsafe { libc::kill(pid_i32, 0) };
    ret == 0
}
```

For interactive sessions, the PID stored in `InteractiveSession.pid` is the PID of the `codeflow -i` process (the shell that ran exec). When Claude Code terminates (normal or crash), `codeflow -i` also terminates — the shell process dies. So `pid_alive = false` means both the shell and Claude are gone.

**No heartbeat for interactive sessions.** Autorun workers write heartbeat files for the CRDT claim TTL mechanism. Interactive sessions do not hold CRDT claims (they use permissive scope or per-operation claims), so no heartbeat is needed.

### 14.4 State Directory Layout

```text
.state/
├── interactive/                 (LOCAL per-worktree — NOT in main repo)
│   └── heartbeat-{SID}         (NOT used — interactive sessions use PID-only liveness)
│
└── db/codeflow.db              (SHARED — symlinked in worktrees)
    └── interactive_session     (SurrealDB table — shared across all sessions)
```

The `InteractiveSession` records live in the shared SurrealDB (`codeflow.db`), accessible from any worktree or the main repo. The `.state/interactive/` directory is reserved for future use (e.g., a heartbeat if the PID-only approach proves insufficient).

### 14.5 Common Code with Autorun

The interactive session management shares significant code with autorun session management:

| Component | Autorun Source | Interactive Reuse |
|-----------|---------------|------------------|
| PID liveness | `stale.rs::check_pid_alive()` | Same function, called from `interactive.rs` |
| Worktree setup | `setup_interactive_worktree()` (after extraction) | Same function |
| Worktree cleanup | `WorktreeManager::cleanup_worktree()` | Same function |
| Claims release | `claims::release_all(coordinator, session_id)` | Same function |
| DB session model | `AutorunSession` | `InteractiveSession` (parallel model) |
| Status enum | `AutorunSessionStatus` | `InteractiveSessionStatus` (mirrors lifecycle) |

The CLI subcommand structure mirrors autorun:

```
codeflow autorun run/status/list/cleanup/abort/cancel/...
codeflow interactive run/status/list/cleanup
```

This consistency makes the CLI predictable and the implementation maintainable.

→ Back to [Table of Contents](#table-of-contents)

---

## 15. Autorun Batch README and Examples Update

Every file in `.codeflow/config/autorun/` requires updates for the field rename. The following table details exactly what changes in each file.

### README.md

**File:** `.codeflow/config/autorun/README.md`

| Location | Change |
|----------|--------|
| Autonomous defaults table (line 66) | `target` column → `integration_branch`; `auto_merge` column → `integration_auto_merge` (already partially corrected in rework) |
| Autonomous defaults section (line 60) | Sentence about `final_pr` already corrected |
| YAML schema reference anywhere in README | `target:` → `integration_branch:`, `auto_merge:` → `integration_auto_merge:`, `final_pr_base:` → `final_pr_target:` |
| auto_merge Rules section | Update field name references |
| final_pr section | Update `final_pr_base` → `final_pr_target` |

### examples/minimal-autonomous.yaml

**Changes:** Comment-only (YAML keys omitted — no batch-level keys in this file).

| Line | Before | After |
|------|--------|-------|
| 11-13 | `final_pr: NOT auto-inferred…` | Already corrected in rework: `final_pr: true — resolve_target() sets…` |

No YAML key renames needed (file omits `target`, `auto_merge`, `final_pr_base`).

### examples/simple-sequential.yaml

**Changes:** Comment-only (YAML keys omitted).

| Lines | Before | After |
|-------|--------|-------|
| 13-17 | `final_pr is NOT auto-inferred — it defaults to false…` | Already corrected in rework: `resolve_target() sets final_pr to true…` |

No YAML key renames needed.

### examples/custom-integration.yaml

**Changes:** YAML keys and comments.

| Location | Before | After |
|----------|--------|-------|
| Line 43 | `target: autorun/sprint-42` | `integration_branch: autorun/sprint-42` |
| Line 44 | `auto_merge: true` | `integration_auto_merge: true` |
| Comments (lines 8, 14-15) | References to `target`/`auto_merge` field names | Update to `integration_branch`/`integration_auto_merge` |

### examples/complex-dependencies.yaml

**Changes:** Comments only (file omits batch-level `target`/`auto_merge` keys).

| Location | Before | After |
|----------|--------|-------|
| Line 29 | `target branch is auto-generated…` | Update wording to `integration_branch is auto-generated…` |
| Line 30 | `auto_merge is inferred from target` | `integration_auto_merge is inferred from integration_branch` |

### examples/direct-to-main.yaml

**Changes:** YAML key and comments.

| Location | Before | After |
|----------|--------|-------|
| Line 41 | `target: main` | `integration_branch: main` |
| Comments (lines 9-17) | References to `target` | Update to `integration_branch` |

### examples/manual-review.yaml

**Changes:** YAML key and comments.

| Location | Before | After |
|----------|--------|-------|
| Line 31 | `auto_merge: false` | `integration_auto_merge: false` |
| Comments (lines 3-17) | References to `auto_merge` | Update to `integration_auto_merge` |

### batches/inf-epc-024-phase1-audits.yaml

**Changes:** Comments only (file omits batch-level keys other than `name`, `max_workers`, `tasks`).

| Location | Before | After |
|----------|--------|-------|
| Lines 7-9 | `target branch is auto-generated… auto_merge is inferred from target` | Update to `integration_branch`/`integration_auto_merge` |

### batches/inf-epc-024-retention-policies.yaml

**Changes:** Comments only.

| Location | Before | After |
|----------|--------|-------|
| Lines 7-9 | Same pattern as phase1-audits | Same correction |

### batches/local/inf-epc-024-phase1-audits.yaml

**Changes:** Comments only.

| Location | Before | After |
|----------|--------|-------|
| Lines 7-9 | Same pattern | Same correction |

→ Back to [Table of Contents](#table-of-contents)
