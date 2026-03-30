# Worktree Path Resolution Architecture

## Problem

Claude Code hooks need to resolve the correct worktree path during autorun sessions, but environment variables set via tmux `send-keys` do not propagate to hook processes. Hooks are spawned as child processes of Claude Code, not of the tmux session shell.

## Why Environment Variables Don't Propagate

There are three environment propagation mechanisms, none of which solve hook visibility:

1. **`CLAUDE_ENV_FILE`** -- Only affects Bash tool calls, not hooks. Hooks are spawned by Claude Code's process manager, which reads `{"env":{}}` from settings (undocumented and unreliable).

2. **tmux `send-keys export`** -- Sets variables in the tmux session's shell, but hooks are not spawned from that shell. They are spawned by Claude Code's internal process, which inherits the Claude Code process's environment, not the tmux shell's.

3. **`{"env":{}}` in settings.json** -- Undocumented. Even if it worked, it would be static at session start, before the worktree path is known.

## Per-PID File Solution

The solution uses per-PID environment files written to the worktree's `.state/runtime/` directory:

```text
.state/runtime/codeflow-env-{PID}.sh
```

### How It Works

1. **Worker creates the file** (`worker.rs`): Before invoking Claude, the worker writes `codeflow-env.sh` with `CODEFLOW_WORKTREE_PATH`, `CODEFLOW_SESSION_ID`, and `CF_PROJECT_ROOT`.

2. **SessionStart writes per-PID env file** (`session_start.rs`): When Claude Code starts inside the worktree, the SessionStart hook writes a fresh per-PID file via `fs::write()` at `codeflow-env-{PID}.sh` containing the session exports. The PID is the Claude Code process PID (stable for the session lifetime).

3. **Hooks resolve via PID** (`helpers.rs`): Each hook call reads the per-PID file to get the correct worktree path, then resolves session ID and project root from the worktree's env file. This is deterministic -- no env var propagation needed.

### Resolution Chain (`detect_project_dir()` at `helpers.rs:20`)

```text
Step 1: CODEFLOW_WORKTREE_PATH env var     (helpers.rs:22)
    |-- Set? + valid dir? --> return it
    |-- Not set or invalid --> fall through
    v
Step 2: CF_PROJECT_ROOT env var            (helpers.rs:37)
    |-- Set? + valid dir? --> return it
    |-- Not set --> fall through
    v
Step 3: CWD walk (find .claude/ or .codeflow/)  (helpers.rs:45)
    |-- Found project root --> use as base for Step 4
    |-- Not found --> use CWD as base
    v
Step 4: Per-PID env file lookup            (helpers.rs:61)
    |-- read_worktree_path_from_pid_file()
    |   |-- Try codeflow-env-{PID}.sh     (helpers.rs:80)
    |   |-- Fallback: shared codeflow-env.sh  (helpers.rs:84)
    |-- Valid worktree dir? --> return it
    |-- No match --> return project_root from Step 3
```

### Why PID Is Stable

| Scenario | PID behavior | Per-PID file impact |
|----------|-------------|---------------------|
| Normal session start | PID assigned at process start, stable until exit | File written once, read many times |
| Context overflow (compaction) | Same process continues, PID unchanged | File persists, no re-write needed |
| Resume (new conversation) | Same process, PID unchanged | File persists |
| Teammate spawn (tmux) | New process, new PID | New file written by teammate's SessionStart |
| Teammate spawn (in-process) | Same process, same PID | Shares lead's file (same worktree) |
| Session end | Process exits, PID released | File cleaned with worktree destruction |
| Crash (no SessionEnd) | PID dead, file persists | Cleaned by `clean_stale_pid_env_files()` on next startup (`session/env.rs:180`) |

PID is stable for the lifetime of a Claude Code process. It only changes when the process exits and a new one starts.

## Session Pointer

The main `codeflow-env.sh` file serves as a session-level pointer that:

1. Provides the canonical session ID for the worktree
2. Provides the worktree path for non-PID-aware consumers
3. Is created by the worker before Claude starts, ensuring it exists before any hooks fire

## Scenario Matrix

| Scenario | Worktree | PID file | Resolution step |
|----------|----------|----------|-----------------|
| Interactive, no worktree | Main repo | Not needed | Step 3 (CWD walk) |
| Interactive, worktree | Worktree | Created by SessionStart | Step 4 (per-PID file) |
| Autorun worker | Worktree | Created by worker + SessionStart | Step 1 (CODEFLOW_WORKTREE_PATH env var) |
| Autorun worker teammate | Worktree | Per-PID for teammate's PID | Step 4 (per-PID file) |
| Parallel interactive | Worktrees | Separate per-PID files | Step 4 (no conflict) |
| Context overflow | Same worktree | Same PID, file persists | Same step as before overflow |
| Session end cleanup | Worktree destroyed | Files cleaned with worktree | Automatic via worktree removal |
| Crash recovery | Stale PID files | Cleaned on next startup | `clean_stale_pid_env_files()` |

---

## Scenario Workflows

### Scenario 1: Single Session, No Worktree (Main Repo)

```text
User starts Claude Code in project root
    |
    v
Claude Code process (PID=1234)
    |-- CWD = /path/to/project
    |
    v
SessionStart hook fires
    |-- No worktree detected
    |-- Writes codeflow-env.sh (session ID, project root)
    |-- No per-PID file needed (CWD walk sufficient)
    |
    v
Hook calls detect_project_dir()
    |-- Step 1: CODEFLOW_WORKTREE_PATH not set --> skip
    |-- Step 2: CF_PROJECT_ROOT not set --> skip
    |-- Step 3: CWD walk finds .claude/ --> /path/to/project  <-- RESOLVES HERE
    |-- Step 4: No per-PID file, no shared env pointing elsewhere --> return project_root
    |
    v
Result: /path/to/project (main repo)
```

**Why it works:** The CWD walk at `helpers.rs:45` finds the `.claude/` marker directory. No worktree indirection needed. Both in-process and tmux teammates share the same CWD, so they resolve identically.

**CRDT claims:** Not applicable -- single session, no parallel workers, no claim contention.

**Files written:** `.state/runtime/codeflow-env.sh` (session ID only, no worktree path)
**Files read:** None for path resolution (CWD walk is self-contained)

### Scenario 2: Single Session, WITH Worktree, In-Process Teammates

```text
User starts Claude Code --> worktree created
    |
    v
Claude Code process (PID=1234)
    |-- CWD = /project/.git-worktrees/worktree-ses-xxx/
    |
    v
SessionStart hook fires (PID=1234)
    |-- Detects worktree (codeflow-env.sh pre-created by WorktreeManager)
    |-- Writes codeflow-env-1234.sh with CODEFLOW_WORKTREE_PATH
    |-- Exports CLAUDE_ENV_FILE for Bash tool calls
    |
    v
Hook calls detect_project_dir() (PID=1234)
    |-- Step 1: CODEFLOW_WORKTREE_PATH not in process env --> skip
    |-- Step 2: CF_PROJECT_ROOT not set --> skip
    |-- Step 3: CWD walk finds .claude/ --> /project (MAIN repo, not worktree)
    |-- Step 4: read_pid_env_file(runtime_dir, 1234) --> worktree path  <-- RESOLVES HERE
    |
    v
Result: /project/.git-worktrees/worktree-ses-xxx/

In-process teammate (same PID=1234)
    |-- detect_project_dir() reads same codeflow-env-1234.sh
    |-- Same worktree path returned
```

**Why it works:** The per-PID file at `session/env.rs:139` is keyed to PID 1234. In-process teammates share the same PID, so they read the same file and resolve to the same worktree. The CWD walk (Step 3) finds the main repo root, but Step 4 overrides it with the worktree path from the per-PID file.

**CRDT claims:** Interactive sessions use `scope_policy=permissive` (no claims) or `scope_policy=soft` with empty scope (reactive per-edit claims via `try_acquire_claim` at `pre_tool_use.rs`).

**Files written:** `codeflow-env-1234.sh` (by SessionStart), `codeflow-env.sh` (by WorktreeManager)
**Files read:** `codeflow-env-1234.sh` (by detect_project_dir Step 4)

### Scenario 3: Single Session, WITH Worktree, Tmux Teammates

```text
Lead Claude Code (PID=1234, tmux pane %1)
    |
    v
SessionStart writes codeflow-env-1234.sh
    |-- Contains: CODEFLOW_WORKTREE_PATH=/project/.git-worktrees/worktree-ses-xxx/
    |
    v
Lead spawns teammate via tmux (new pane %2)
    |
    v
Teammate Claude Code (PID=5678, tmux pane %2)
    |-- CWD = /project (main repo, not worktree)
    |
    v
Teammate SessionStart fires (PID=5678)
    |-- Reads session-pointer.json from main repo
    |-- Discovers worktree path: /project/.git-worktrees/worktree-ses-xxx/
    |-- Writes codeflow-env-5678.sh with same worktree path
    |
    v
Teammate hook calls detect_project_dir() (PID=5678)
    |-- Step 3: CWD walk --> /project (main repo)
    |-- Step 4: read_pid_env_file(runtime_dir, 5678) --> worktree path  <-- RESOLVES HERE
    |
    v
Result: both PID 1234 and PID 5678 resolve to same worktree
```

**Why it works:** Each tmux-spawned process gets a unique PID. The teammate's SessionStart reads the session pointer from the main repo (which the lead's SessionStart or WorktreeManager created), discovers the worktree path, and writes its own per-PID file. Both processes resolve to the same worktree via different per-PID files.

**CRDT claims:** Same as Scenario 2 -- teammates in the same session do not contend. Claims are session-scoped, not PID-scoped.

**Files written:** `codeflow-env-1234.sh` (lead), `codeflow-env-5678.sh` (teammate)
**Files read:** `session-pointer.json` (teammate discovers worktree), `codeflow-env-{PID}.sh` (each process reads its own)

### Scenario 4: Parallel Interactive Sessions (Two Users)

```text
Session A: Claude Code (PID=1000)        Session B: Claude Code (PID=2000)
    |                                         |
    v                                         v
worktree-ses-aaa/                        worktree-ses-bbb/
    |                                         |
    v                                         v
SessionStart writes                      SessionStart writes
codeflow-env-1000.sh                     codeflow-env-2000.sh
    |                                         |
    v                                         v
Hook (PID=1000):                         Hook (PID=2000):
  Step 4 reads env-1000.sh               Step 4 reads env-2000.sh
  --> worktree-ses-aaa/                   --> worktree-ses-bbb/

Shared codeflow-env.sh: LAST WRITER WINS (backward compat)
Per-PID files: NO CONFLICT (different PIDs = different files)
```

**Why it works:** Different PIDs produce different per-PID filenames. No overwrite conflict. The shared `codeflow-env.sh` is still written for backward compatibility, but per-PID files are checked FIRST (Step 4 before shared fallback at `helpers.rs:80-84`). This is the scenario that PR #230's shared-only approach broke -- two sessions would overwrite each other's shared env file.

**CRDT claims:** Each session has its own `worker_sid`. Claims acquired by Session A (via `acquire_batch` at `worker.rs:314` or `try_acquire_claim` at `pre_tool_use.rs`) use Session A's ID. Session B uses Session B's ID. Conflicting file edits are blocked by the Loro CRDT coordinator.

**Files written:** `codeflow-env-1000.sh`, `codeflow-env-2000.sh`, `codeflow-env.sh` (last writer wins)
**Files read:** Each session reads only its own per-PID file

### Scenario 5: Autorun Parallel Workers (Tmux Only)

```text
Orchestrator (main repo, PID=100)
    |-- Creates tmux sessions for each worker
    |
    +---> Worker A (tmux cf-ar-task-a, PID=3001)
    |         |
    |         v
    |     worker.rs creates worktree-ses-worker-a/
    |     worker.rs writes codeflow-env.sh (worker_sid, worktree path)
    |     tmux send-keys: export CODEFLOW_WORKTREE_PATH=...
    |     tmux send-keys: export AUTORUN_SESSION_ID={worker_sid}  <-- NOT batch session_id
    |     tmux send-keys: export AUTORUN_BATCH_ID={batch_session_id}
    |     tmux send-keys: claude --dangerously-skip-permissions '...'
    |         |
    |         v
    |     Claude Code starts (PID=3001, inherits tmux shell env)
    |         |
    |         v
    |     detect_project_dir() (PID=3001):
    |       Step 1: CODEFLOW_WORKTREE_PATH IS SET (from tmux export)  <-- RESOLVES HERE
    |       --> worktree-ses-worker-a/
    |
    +---> Worker B (tmux cf-ar-task-b, PID=3002)
              |
              v
          (Same flow, different worktree, different worker_sid)
```

**Why it works:** The autorun invoker (`autorun.rs:1309`) exports `CODEFLOW_WORKTREE_PATH` via tmux `send-keys` BEFORE starting Claude. When Claude Code starts, its process inherits the tmux shell's environment, so Step 1 of `detect_project_dir()` catches it immediately. The per-PID file is also written as belt-and-suspenders by SessionStart.

**CRDT claims:** Proactive + reactive. Worker pre-claims files via `acquire_batch()` at `worker.rs:314` using `worker_sid`. Hooks use `AUTORUN_SESSION_ID` (= `worker_sid`, fixed in this PR) for reactive per-edit claims via `try_acquire_claim`. Since both use the same ID, claim ownership is consistent.

**Session ID alignment (fixed in this PR):**

| Component | ID used | Source |
|-----------|---------|--------|
| `acquire_batch()` (worker.rs:317) | `worker_sid` | Generated per-worker at `worker.rs:282` |
| `AUTORUN_SESSION_ID` env var | `worker_sid` | Exported at `autorun.rs:1312` (was `cfg.session_id` before fix) |
| `active-task.json` session_id | `worker_sid` | Written at `worker.rs:382` (was `cfg.session_id` before fix) |
| `AUTORUN_BATCH_ID` env var | `cfg.session_id` | New export at `autorun.rs:1317` for batch-level correlation |

**Files written:** `codeflow-env.sh` (worker), `codeflow-env-{PID}.sh` (SessionStart), env vars via tmux
**Files read:** Step 1 reads env var directly (no file I/O needed)

### Scenario 6: Crash Recovery and Stale Cleanup

```text
Session crashes (no SessionEnd fires)
    |
    v
Stale files remain:
    .state/runtime/codeflow-env-{dead_PID}.sh
    .state/session/{SID}/pathflow/pathflow-session-status.json
    .state/sentinels/pathflow/{SID}/...
    |
    v
Next session starts
    |
    v
SessionStart hook fires (new PID)
    |
    v
clean_stale_pid_env_files() at session/env.rs:180
    |-- Reads all codeflow-env-*.sh files
    |-- For each: extract PID from filename
    |-- Check if PID is alive (kill(pid, 0))
    |-- Dead PID? --> delete the file
    |-- Alive PID? --> keep (active session)
    |
    v
Session pointer cleanup:
    |-- sweep_stale_sessions() checks lead_pid liveness
    |-- Dead lead? --> remove session directory
    |
    v
New session proceeds with clean state
```

**Why it works:** Per-PID files have the PID embedded in the filename, making stale detection trivial. `clean_stale_pid_env_files()` at `session/env.rs:180` iterates the runtime directory, extracts PIDs from filenames matching `codeflow-env-{N}.sh`, and checks process liveness. Dead PIDs mean crashed sessions whose files can be safely removed.

**CRDT claims:** Claims have TTL (`claims.ttl_secs`, default 4200s). After crash, claims expire naturally. For immediate release, `claims::release_all()` can be called with the crashed session's worker_sid.

**Files cleaned:** `codeflow-env-{dead_PID}.sh`, session directories, stale sentinels

---

## CRDT Claims Comparison

| Mode | Claim acquisition | Scope enforcement | When |
|------|-------------------|-------------------|------|
| Autorun | **Proactive** via `acquire_batch()` at `worker.rs:314` + **Reactive** per-edit via `try_acquire_claim` at `pre_tool_use.rs` | `scope_policy=soft` (default) or `hard` | Pre-claim file_scope at startup; per-edit claims for out-of-scope files |
| Interactive (soft) | **Reactive only** -- per-edit via `try_acquire_claim` | `scope_policy=soft` with empty scope = every edit attempts claim | Claim attempted on each Edit/Write; conflicts blocked |
| Interactive (permissive) | **No claims** | `scope_policy=permissive` -- no scope checking | Single-user sessions; forbidden for `autorun_eligible` tasks |
