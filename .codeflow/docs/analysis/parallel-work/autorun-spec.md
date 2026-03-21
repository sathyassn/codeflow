---
title: "Autorun System Specification"
version: "1.0.0"
status: proposed
epic: INF-EPC-023
created_at: "2026-03-21"
updated_at: "2026-03-21"
---

# Autorun System Specification

Authoritative specification for the `codeflow autorun` subsystem. Covers process architecture, command reference, cleanup semantics, data recording, Claude invocation, blocker handling, concurrent batches, schema additions, config additions, and gap trace.

## 1. Process Architecture

```text
┌─────────────────────────────────────────────────────────────────────┐
│                        User Terminal                                │
│  $ codeflow autorun run --batch tasks.yaml                         │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     Orchestrator (Tokio)                            │
│  codeflow-cli/core/src/autorun/orchestrator.rs                     │
│                                                                     │
│  1. Parse batch YAML -> topological sort                           │
│  2. Load parallel-work-config.json                                 │
│  3. Register autorun_session in DB (Tier 1)                        │
│  4. Emit batch_started to autorun-events.jsonl (Tier 0)            │
│  5. Semaphore-bounded dispatch loop (max_workers)                  │
│  6. SIGINT handler -> graceful abort                               │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ Worker Pool (tokio::spawn, semaphore-bounded)               │   │
│  │                                                              │   │
│  │ Worker 1                Worker 2              Worker N       │   │
│  │ ┌──────────────┐       ┌──────────────┐     ┌────────────┐  │   │
│  │ │ Worktree     │       │ Worktree     │     │ Worktree   │  │   │
│  │ │ .git-wt/w-1/ │       │ .git-wt/w-2/ │     │ .git-wt/wN │  │   │
│  │ │              │       │              │     │            │  │   │
│  │ │ tmux session │       │ tmux session │     │ tmux sess  │  │   │
│  │ │ cf-{sid}-w1  │       │ cf-{sid}-w2  │     │ cf-{sid}-N │  │   │
│  │ │              │       │              │     │            │  │   │
│  │ │ Claude Code  │       │ Claude Code  │     │ Claude     │  │   │
│  │ │ --dang-skip  │       │ --dang-skip  │     │ --dang-sk  │  │   │
│  │ └──────┬───────┘       └──────┬───────┘     └─────┬──────┘  │   │
│  │        │                      │                   │          │   │
│  │        ▼                      ▼                   ▼          │   │
│  │  ┌─────────────────────────────────────────────────────┐     │   │
│  │  │              Shared State (symlinked)               │     │   │
│  │  │                                                     │     │   │
│  │  │  .state/db/codeflow.db        (SurrealDB)          │     │   │
│  │  │  .state/coordination/state.loro (Loro CRDT)        │     │   │
│  │  │  .state/ledger/*.jsonl         (append-only)       │     │   │
│  │  │  .state/logs/                  (shared logs)       │     │   │
│  │  └─────────────────────────────────────────────────────┘     │   │
│  │                                                              │   │
│  │  ┌─────────────────────────────────────────────────────┐     │   │
│  │  │            Local State (per-worktree)               │     │   │
│  │  │                                                     │     │   │
│  │  │  .state/runtime/     (active-task.json, env)       │     │   │
│  │  │  .state/session/     (pathflow status)             │     │   │
│  │  │  .state/sentinels/   (pathflow phase/stage)        │     │   │
│  │  └─────────────────────────────────────────────────────┘     │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

**Symlink layout per worktree:**

```text
.git-worktrees/worktree-{SID}/
├── .state/
│   ├── db/             -> ../../.state/db/              (shared)
│   ├── ledger/         -> ../../.state/ledger/           (shared)
│   ├── coordination/   -> ../../.state/coordination/     (shared)
│   ├── logs/           -> ../../.state/logs/             (shared)
│   ├── registry/       -> ../../.state/registry/         (shared)
│   ├── backups/        -> ../../.state/backups/          (shared)
│   ├── runtime/                                         (LOCAL)
│   ├── session/                                         (LOCAL)
│   └── sentinels/                                       (LOCAL)
└── (full working copy of repo)
```

## 2. Command Reference

### 2.1 `codeflow autorun run [--batch <path>]`

Execute a batch of tasks in parallel worktrees.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--batch <path>` | `.codeflow/config/autorun/batch.yaml` | Path to YAML batch file |

**Mechanics:**

1. Pre-flight checks: tmux available, git status clean, target branch exists
2. Parse batch YAML via `batch::parse_batch_file()` (validates structure, deps, protected branches)
3. Load `parallel-work-config.json` via `config::load_config()`
4. Generate batch session ID via `session::generate_session_id()`
5. Write `autorun_session` record to DB with status `running`, pid field set to current process PID
6. Emit `batch_started` event to `autorun-events.jsonl`
7. Start orchestrator dispatch loop (semaphore = min(max_workers, max_concurrent))
8. For each ready task: spawn worker via `TmuxWorker::run()` (worktree setup, tmux, Claude, cleanup)
9. On completion: write `autorun_session` with final status, emit `batch_completed`
10. Generate batch report to `project-management/tracking/autorun/{batch_name}-{date}.md`

**DB queries:** `CREATE autorun_session`, `UPDATE autorun_session SET status = ...`

### 2.2 `codeflow autorun status [--batch <session_id>]`

Show status of active and recent autorun batches.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--batch <session_id>` | (all active) | Filter to specific batch |

**Mechanics:**

1. Query DB: `SELECT * FROM autorun_session WHERE status = 'running' OR status = 'aborting'`
2. For each session, query workers: `SELECT * FROM autorun_worker WHERE session_id = ...`
3. Verify tmux session liveness: `tmux has-session -t {name}` for each worker
4. Detect orphan workers: tmux session exists but no DB record (or vice versa)
5. Display table: batch name, total/completed/failed/running/pending tasks, elapsed time

**Output format:**

```text
BATCH              STATUS    TASKS   DONE  FAIL  RUN   ELAPSED
my-refactor-batch  running   12      5     1     3     04:23
```

### 2.3 `codeflow autorun attach <task_id>`

Attach to a running worker's tmux session for interactive inspection.

**Mechanics:**

1. Query DB: `SELECT tmux_session FROM autorun_worker WHERE task_id = $task_id AND status = 'running'`
2. Verify tmux session exists: `tmux has-session -t {name}`
3. Execute: `tmux attach-session -t {name}` (replaces current process via exec)

### 2.4 `codeflow autorun logs <task_id> [--follow]`

Show logs from a worker's tmux session.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--follow` | false | Stream output (tmux capture-pane -p -t {name} in a loop) |

**Mechanics:**

1. Look up tmux session name from DB
2. `tmux capture-pane -p -t {name} -S -` (capture full scrollback)
3. If `--follow`: poll every 500ms and display new output

### 2.5 `codeflow autorun cancel <task_id>`

Cancel a single running worker.

**Mechanics (cleanup sequence):**

1. Look up worker PID and tmux session from DB
2. Send SIGTERM to Claude process inside tmux session
3. Wait 10 seconds for graceful shutdown
4. Send SIGKILL if still alive
5. Discard uncommitted changes in worktree: `git checkout .` + `git clean -fd`
6. Release all claims for this worker's session via `claims::release_all()`
7. Kill tmux session: `tmux kill-session -t {name}`
8. Cleanup worktree: `WorktreeManager::cleanup_worktree()`
9. Deregister from worktree registry
10. Close PR if open: `gh pr close {pr_number} --delete-branch` (if pr_number set)
11. Delete remote branch if pushed: `git push origin --delete {branch}` (if branch pushed)
12. Update DB: `UPDATE autorun_worker SET status = 'cancelled', completed_at = ...`
13. Update `autorun_task_run` with status `cancelled`

### 2.6 `codeflow autorun abort [--batch <session_id>]`

Abort an entire batch (cancel all running + skip pending).

**Mechanics:**

1. Write abort marker file: `{project_dir}/.state/runtime/abort-{session_id}`
2. Update `autorun_session.status` to `aborting`
3. Stop dispatching new tasks (orchestrator checks abort marker before dispatch)
4. Cancel all currently running workers (same sequence as cancel command)
5. Mark all pending tasks as `skipped` with reason `batch_aborted`
6. Update batch status to `aborted` with `completed_at` timestamp
7. Emit `batch_aborted` event to `autorun-events.jsonl`

### 2.7 `codeflow autorun results [--batch <session_id>]`

Display results from a completed or in-progress batch.

**Mechanics:**

1. Query DB: `SELECT * FROM autorun_task_run WHERE session_id = $sid ORDER BY created_at`
2. Display table with: task_id, status, duration, PR number, exit code, error

**Output format:**

```text
TASK                STATUS      DURATION  PR    EXIT  ERROR
INF-TSK-023-028    completed   12:34     #205  0
INF-TSK-023-029    failed      03:21     --    1     test failure
INF-TSK-023-030    skipped     --        --    -1    dependency failed
```

### 2.8 `codeflow autorun history [--limit N] [--since DATE] [--status STATUS] [--batch-name PATTERN] [--all]`

Show historical autorun batch executions.

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | 10 | Maximum number of batches to show |
| `--since` | (none) | Filter batches started after this ISO date |
| `--status` | (none) | Filter by status (completed, failed, aborted) |
| `--batch-name` | (none) | Filter by batch name pattern (substring match) |
| `--all` | false | Show all batches (overrides --limit) |

**Mechanics:**

1. Build SurrealQL query from flags: `SELECT * FROM autorun_session ORDER BY created_at DESC LIMIT $limit`
2. Apply filters: `WHERE status = $status AND created_at > $since AND batch_name CONTAINS $pattern`
3. For each session, aggregate task stats from `autorun_task_run`
4. Display summary table

## 3. Cancel/Abort Cleanup

### Cancel (single worker)

```text
cancel <task_id>
    |
    v
Lookup worker PID + tmux session from DB
    |
    v
SIGTERM -> wait 10s -> SIGKILL (if still alive)
    |
    v
git checkout . && git clean -fd (discard changes)
    |
    v
claims::release_all(coordinator, worker_sid)
    |
    v
tmux kill-session -t {name}
    |
    v
WorktreeManager::cleanup_worktree(name)
    |
    v
Deregister from worktree registry
    |
    v
Close PR if open (gh pr close {pr_number} --delete-branch)
    |
    v
Delete remote branch if pushed (git push origin --delete {branch})
    |
    v
UPDATE autorun_worker SET status = 'cancelled'
UPDATE autorun_task_run SET status = 'cancelled'
    |
    v
End state: main repo untouched, worktree removed, claims released
```

### Abort (entire batch)

```text
abort [--batch <session_id>]
    |
    v
Write abort marker file
    |
    v
UPDATE autorun_session SET status = 'aborting'
    |
    v
Stop dispatching (orchestrator checks marker)
    |
    v
For each running worker: cancel sequence (above)
    |
    v
For each pending task: mark skipped (reason: batch_aborted)
    |
    v
UPDATE autorun_session SET status = 'aborted', completed_at = now()
    |
    v
Emit batch_aborted event to autorun-events.jsonl
    |
    v
End state: all worktrees removed, all claims released, DB updated
```

## 4. Data Recording

### Tier 0: JSONL Event Log

**File:** `.state/ledger/autorun-events.jsonl` (shared, append-only, git-tracked)

Each event is a JSON line with a `session_id` field for filtering.

**Event types:**

| Event Type | When Emitted | Key Fields |
|-----------|-------------|------------|
| `batch_started` | Orchestrator begins | session_id, batch_name, batch_file, max_workers, total_tasks |
| `worker_started` | Worker spawned | session_id, worker_id, task_id, tmux_session, worktree_path |
| `worker_completed` | Worker finishes successfully | session_id, worker_id, task_id, exit_code, pr_number, pr_url, duration_sec |
| `worker_failed` | Worker exits non-zero | session_id, worker_id, task_id, exit_code, error, duration_sec |
| `worker_timeout` | Worker exceeds timeout | session_id, worker_id, task_id, timeout_secs |
| `worker_blocked` | Worker blocked by claim conflict | session_id, worker_id, task_id, blocked_reason, claim_conflicts |
| `worker_cancelled` | Worker cancelled by user | session_id, worker_id, task_id |
| `batch_completed` | All tasks done (success or failure) | session_id, batch_name, completed, failed, skipped, timed_out |
| `batch_aborted` | Batch abort requested | session_id, batch_name, reason |

**Routing:** Add event types to `ledger/routing.rs` and file constant to `ledger/files.rs`. Route all `batch_*` and `worker_*` prefixed event types to `autorun-events.jsonl`.

### Tier 1: SurrealDB Tables

**Tables:** `autorun_session`, `autorun_worker`, `autorun_task_run` (existing, with schema additions per Section 8)

**Writes:**

| Table | When | Operation |
|-------|------|-----------|
| `autorun_session` | Batch start | CREATE with status=running |
| `autorun_session` | Batch end | UPDATE status, completed_tasks, failed_tasks, completed_at |
| `autorun_worker` | Worker spawn | CREATE with status=running |
| `autorun_worker` | Worker end | UPDATE status, completed_at |
| `autorun_task_run` | Task start | CREATE with status=running |
| `autorun_task_run` | Task end | UPDATE status, pr_number, pr_url, completed_at |

### Tier 2: Markdown Reports

**Directory:** `project-management/tracking/autorun/` (git-tracked)

**File naming:** `{batch_name}-{YYYY-MM-DD}.md`

**Content:** Per-task table with status, duration, PR URL, error messages. Summary statistics. Generated by `codeflow autorun results` or automatically at batch completion.

## 5. Claude Invocation

### Hybrid tmux + file-marker approach

Each worker invokes Claude Code via tmux and monitors for completion using a file marker.

**Invocation sequence:**

```text
1. Create tmux session: tmux new-session -d -s codeflow-{sid_short}-w{N}

2. Set environment variables via tmux:
   tmux send-keys -t {session} 'export AUTORUN_SESSION_ID={batch_sid}' Enter
   tmux send-keys -t {session} 'export AUTORUN_TASK_ID={task_id}' Enter
   tmux send-keys -t {session} 'export AUTORUN_ACCEPTANCE={base64_criteria}' Enter
   tmux send-keys -t {session} 'export CODEFLOW_WORKTREE_PATH={worktree}' Enter

3. Send Claude command:
   tmux send-keys -t {session} \
     'cd {worktree} && claude -p "{task_prompt}" \
       --dangerously-skip-permissions \
       --output-format json \
       > .state/runtime/worker-output.json 2>&1; \
       echo $? > .state/runtime/worker-exit-code' Enter

4. Poll for exit-code marker file:
   loop {
     if exists {worktree}/.state/runtime/worker-exit-code {
       read exit_code
       read worker-output.json
       break
     }
     sleep 1s
     check timeout
   }

5. Parse results from worker-output.json:
   InvokeResult {
     exit_code: from marker file,
     branch: from git branch --show-current in worktree,
     pr_url: from Claude output or gh pr list,
     pr_number: parsed from pr_url,
   }
```

**User can attach at any time:** `codeflow autorun attach <task_id>` connects to the tmux session interactively.

**Task prompt construction:**

The task prompt is built from the task markdown file. The orchestrator reads `project-management/epics/{area}/{epic}/tasks/{task}.md`, extracts the description, approach, acceptance criteria, and file scope, and constructs a PathFlow-aware prompt that instructs Claude to:

1. Execute the task as an autorun session (no user interaction)
2. Follow PathFlow PF1-PF7 autonomously
3. Create a PR targeting the configured branch
4. Exit with code 0 on success, non-zero on failure

## 6. Blocker Handling

| Blocker Scenario | Detection Point | Action | Worker Status |
|-----------------|-----------------|--------|---------------|
| **Claim conflict** | `acquire_batch()` at worker startup | Check scope_policy: soft=retry with backoff then skip; hard=fail immediately | `blocked` (with `blocked_reason` and `claim_conflicts` in DB) |
| **Protected file in scope** | `validate_batch()` at parse time | Reject task at batch validation before any worker starts | N/A (batch parse error) |
| **Permission prompt** | Prevented by `--dangerously-skip-permissions` | Should never occur; if it does, Claude hangs until timeout | `timeout` |
| **Test failure** | Claude exits non-zero inside worker | Worker captures exit code, marks failed | `failed` |
| **Review rejection** | Claude handles rework loop internally | Claude retries up to `max_rework_iterations` (3), then exits non-zero if still rejected | `failed` |
| **Merge conflict** | `check_merge_conflicts()` after Claude exits | If `auto_rebase` is true, call `attempt_rebase()` up to `max_rebase_attempts`; on failure, mark `merge_conflict` | `merge_conflict` |
| **Timeout** | `tokio::time::timeout` wrapping Claude invocation | SIGTERM to tmux session, 10s grace, SIGKILL; cleanup worktree and claims | `timeout` |
| **Worktree limit** | `WorktreeRegistry::locked_register_with_limit()` | Orchestrator waits for semaphore permit (bounded by min(max_workers, max_concurrent)) | Queued (not started yet) |
| **Git status dirty** | Pre-flight check before batch start | Abort batch with error message | N/A (pre-flight failure) |
| **tmux not available** | Pre-flight check before batch start | Abort batch with error message | N/A (pre-flight failure) |

### Claim conflict retry logic (scope_policy=soft)

```text
acquire_batch(coordinator, file_scope, worker_sid)
    |
    v
For each path in file_scope:
    Coordinator::acquire(path, worker_sid)
        |
        +-- Ok(claim) -> claimed successfully
        |
        +-- Err(ClaimConflict) -> path held by another session
                |
                v
            blocked_behavior config check:
                |
                +-- "skip_and_continue" -> log warning, skip this task
                |
                +-- "fail" -> return error, mark task as blocked
```

## 7. Concurrent Batches

Multiple simultaneous `codeflow autorun run` invocations are supported.

**Isolation mechanisms:**

| Mechanism | Scope | How |
|-----------|-------|-----|
| Tmux session names | Batch-scoped | `codeflow-{batch_sid_short}-w{N}` -- each batch uses its own SID prefix |
| Worktree limit | Global | `WorktreeRegistry` enforces `max_concurrent` (default 3) across ALL batches |
| Claims | Global | Loro CRDT prevents two workers (any batch) from editing the same file |
| DB records | Batch-scoped | `autorun_session.id` distinguishes batches; workers reference their session_id |
| Merge queue | Global | FIFO ordering across all batches; `merge_queue::enqueue/dequeue` |

**Status command with multiple batches:**

```text
$ codeflow autorun status

BATCH              STATUS    TASKS   DONE  FAIL  RUN   ELAPSED
refactor-hooks     running   8       3     0     2     12:34
fix-tests          running   5       1     0     2     04:56
```

**Worktree limit enforcement:**

The orchestrator semaphore is bounded by `min(max_workers, max_concurrent)`. When two batches run simultaneously, they compete for the global `max_concurrent` worktree slots enforced by `WorktreeRegistry`.

## 8. Schema Additions

### autorun_session additions

| Field | Type | Description |
|-------|------|-------------|
| `pid` | `option<int>` | OS PID of the orchestrator process (for orphan detection) |
| `skipped_tasks` | `int` | Count of tasks skipped (dependency failure or batch abort) |

### autorun_worker additions

| Field | Type | Description |
|-------|------|-------------|
| `file_scope` | `array<string>` | File patterns claimed by this worker |
| `scope_policy` | `string` | Scope policy for this worker (soft/hard/permissive) |

### autorun_task_run additions

| Field | Type | Description |
|-------|------|-------------|
| `blocked_reason` | `option<string>` | Why the task was blocked (claim conflict, dependency, etc.) |
| `claim_conflicts` | `option<array<string>>` | Paths that had claim conflicts |
| `merge_conflicts` | `option<array<string>>` | Paths with merge conflicts |

## 9. Config Addition

Add `"autorun"` section to `.codeflow/config/parallel-work/parallel-work-config.json`:

```json
{
  "worktree": { "..." : "..." },
  "sync": { "..." : "..." },
  "merge": { "..." : "..." },
  "claims": { "..." : "..." },
  "autorun": {
    "worker_timeout_secs": 3600,
    "blocked_behavior": "skip_and_continue",
    "report_dir": "project-management/tracking/autorun"
  }
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `worker_timeout_secs` | 3600 | Maximum seconds per worker before timeout |
| `blocked_behavior` | `"skip_and_continue"` | What to do on claim conflict: `"skip_and_continue"` or `"fail"` |
| `report_dir` | `"project-management/tracking/autorun"` | Directory for batch report markdown output |

**Rust struct addition in `config.rs`:**

```rust
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AutorunConfig {
    pub worker_timeout_secs: u64,
    pub blocked_behavior: String,
    pub report_dir: String,
}
```

## 10. Gap Trace

20 identified gaps mapped to implementation tasks.

| # | Gap | Current State | Target State | File:Line | Task |
|---|-----|--------------|-------------|-----------|------|
| 1 | Claude invoker is a no-op stub | `RealClaude::invoke` returns hardcoded success | Real tmux+file-marker invocation with env vars, prompt, polling | `cli/src/cmd/autorun.rs:156-170` | A |
| 2 | Merge queue dequeue is blind front-pop | `locked_dequeue` pops first entry regardless of session | Session-verified dequeue: only pop if front entry matches this session | `core/src/autorun/worker.rs:311-313` | B |
| 3 | Merge conflict handling is warn-only | `check_merge_conflicts` result logged but not acted on | Read config, call `attempt_rebase()`, set status=merge_conflict on failure | `core/src/autorun/worker.rs:254-270` | C |
| 4 | Claim conflict is warn-only | `acquire_batch` failure logged, worker continues | Check claim result, apply `blocked_behavior` (skip or fail) | `core/src/autorun/worker.rs:213-218` | B |
| 5 | Worker cleanup skips daemon auto-stop | Worktree removed but daemon not checked | Call `maybe_auto_stop_daemon()` after worktree cleanup | `core/src/autorun/worker.rs:316-318` | B |
| 6 | Batch validation lacks autorun_eligible check | Validates structure only (deps, cycles, protected branches) | Read task markdown, verify autorun_eligible=true, acceptance non-empty, scope_policy not permissive | `core/src/autorun/batch.rs:119-177` | D |
| 7 | No file_scope overlap detection | Tasks with overlapping file_scope not flagged | Detect overlapping file_scope between concurrent (non-dependent) tasks at parse time | `core/src/autorun/batch.rs:119-177` | D |
| 8 | No DB/JSONL recording | Orchestrator and workers write no events | Inject Store trait; write autorun_session, autorun_worker, autorun_task_run; emit JSONL events | `core/src/autorun/orchestrator.rs:91-131` | E |
| 9 | target_branch not validated | Batch target not checked for existence or protection | Verify target branch exists; check against `enforcement-policy.json` protected list | `core/src/autorun/batch.rs:156-174` | D |
| 10 | No session management commands | Only `codeflow autorun` (hardcoded `run` path) | status, attach, logs, cancel, abort, results, history subcommands | `cli/src/cmd/autorun.rs:12-15` | J, K |
| 11 | No pre-flight checks | Batch file path is only validation | Check tmux available, git status clean, target branch exists | `cli/src/cmd/autorun.rs:17-28` | G |
| 12 | No cancel/abort support | Workers run to completion or timeout | Cancel single worker, abort entire batch with full cleanup | (not implemented) | J |
| 13 | InvokeConfig lacks acceptance criteria | Prompt is generic string `"Execute autorun task {id}"` | Read task markdown, extract acceptance criteria, construct PathFlow-aware prompt | `core/src/autorun/worker.rs:243-244` | A |
| 14 | No results/history queries | `report_results` prints to stdout only | Query DB for structured results, support filtering and formatting | `cli/src/cmd/autorun.rs:69-96` | J |
| 15 | No autorun-events.jsonl routing | Autorun events not routed to JSONL | Add autorun event types to `ledger/routing.rs`, create `AutorunEvent` enum in events.rs | `core/src/ledger/routing.rs:13-63` | E |
| 16 | No autorun config section | Only 4 sections (worktree, sync, merge, claims) | Add `autorun` section with worker_timeout_secs, blocked_behavior, report_dir | `core/src/autorun/config.rs:17-28` | F |
| 17 | No graceful shutdown | Orchestrator runs until all tasks complete or timeout | SIGINT handler; abort marker; cancel running workers; skip pending | `core/src/autorun/orchestrator.rs:91-131` | H |
| 18 | Tmux names not batch-scoped | All workers use `codeflow-worker-{N}` prefix | Use `codeflow-{batch_sid_short}-w{N}` for cross-batch uniqueness | `core/src/autorun/orchestrator.rs:209` | B |
| 19 | No CLI `--batch` flag | Hardcoded batch path `.codeflow/config/autorun/batch.yaml` | Accept `--batch <path>` CLI argument | `cli/src/cmd/autorun.rs:18-22` | G |
| 20 | No worktree limit retry | Worker creation fails if registry full | Orchestrator bounded by `min(max_workers, max_concurrent)` via semaphore | `core/src/autorun/orchestrator.rs:188` | B |
