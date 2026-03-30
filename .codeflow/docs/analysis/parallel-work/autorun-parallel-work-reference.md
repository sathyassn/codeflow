# Autorun and Parallel Work Architecture

Comprehensive reference for the CodeFlow autorun subsystem: batch execution of tasks across parallel worktrees with CRDT-based coordination.

## Table of Contents

- [1. Overview](#1-overview)
- [2. Workflow Diagrams](#2-workflow-diagrams)
  - [2.1 Batch Execution (Happy Path)](#21-batch-execution-happy-path)
  - [2.2 Worker Lifecycle (Per-Task)](#22-worker-lifecycle-per-task)
  - [2.3 Claim Enforcement During Claude Execution](#23-claim-enforcement-during-claude-execution)
  - [2.4 Concurrent Workers -- Coordination](#24-concurrent-workers----coordination)
  - [2.5 Cancel/Abort Flow](#25-cancelabort-flow)
  - [2.6 Sync Daemon Lifecycle](#26-sync-daemon-lifecycle)
- [3. Component Reference](#3-component-reference)
  - [3.1 Batch Parser](#31-batch-parser)
  - [3.2 Orchestrator](#32-orchestrator)
  - [3.3 Worker](#33-worker)
  - [3.4 Config](#34-config)
  - [3.5 CRDT Coordination](#35-crdt-coordination)
  - [3.6 Worktree Management](#36-worktree-management)
  - [3.7 CLI Commands](#37-cli-commands)
  - [3.8 Models](#38-models)
  - [3.9 Git Conflict Handling](#39-git-conflict-handling)
  - [3.10 Event System](#310-event-system)
- [4. Data Model](#4-data-model)
- [5. Configuration Reference](#5-configuration-reference)
- [6. Environment Variables](#6-environment-variables)
- [7. Chain Completeness Matrix](#7-chain-completeness-matrix)
- [8. Cross-References](#8-cross-references)

## 1. Overview

The autorun subsystem enables autonomous, parallel execution of multiple CodeFlow tasks. Each task runs in its own isolated git worktree with a dedicated Claude Code session executing the full PathFlow pipeline (PF1 through PF7) without human intervention.

**Key characteristics:**

- **Batch-driven**: Tasks are defined in YAML batch files with dependency ordering
- **Worktree-isolated**: Each worker gets its own git worktree for filesystem safety
- **CRDT-coordinated**: Loro-based claims prevent concurrent edits to the same file
- **Dependency-aware**: Topological sort ensures tasks execute in dependency order
- **Fault-tolerant**: Failed tasks skip dependents; timeouts trigger cleanup

**Architecture layers:**

| Layer | Component | Purpose |
|-------|-----------|---------|
| CLI | `codeflow autorun <subcommand>` | User-facing commands |
| Orchestration | `orchestrator.rs` | Semaphore-bounded dispatch loop |
| Execution | `worker.rs` (`TmuxWorker`) | Per-task worktree + tmux + Claude |
| Coordination | `coordination/` (Loro CRDT) | Claims, merge queue, sync daemon |
| Storage | SurrealDB + JSONL | Tier 0/1 recording |

**Position in CodeFlow:**

Autorun operates at the pre-PF1 level. The `codeflow autorun run` command creates an orchestrator that spawns independent workers, each of which runs its own complete PathFlow session (PF1 through PF7). The lead never interacts with workers directly; the CLI orchestrator manages their lifecycle.

## 2. Workflow Diagrams

### 2.1 Batch Execution (Happy Path)

```text
USER                        CLI (autorun.rs)              ORCHESTRATOR              WORKER(s)
  |                              |                              |                       |
  |  codeflow autorun run        |                              |                       |
  |  --batch tasks.yaml          |                              |                       |
  |----------------------------->|                              |                       |
  |                              | 1. check_tmux_available()   |                       |
  |                              | 2. check_git_clean()        |                       |
  |                              | 3. parse_batch_file()       |                       |
  |                              |    ├─ YAML deserialize      |                       |
  |                              |    ├─ validate_batch()      |                       |
  |                              |    │  ├─ no dupes           |                       |
  |                              |    │  ├─ deps exist         |                       |
  |                              |    │  ├─ no self-deps       |                       |
  |                              |    │  └─ merge protection   |                       |
  |                              |    ├─ validate_batch_extended()                      |
  |                              |    │  ├─ autorun_eligible   |                       |
  |                              |    │  ├─ file_scope valid   |                       |
  |                              |    │  ├─ scope_policy valid |                       |
  |                              |    │  └─ overlap detection  |                       |
  |                              |    └─ topological_sort()    |                       |
  |                              | 4. load_config()            |                       |
  |                              | 5. create providers         |                       |
  |                              | 6. Orchestrator::new()      |                       |
  |                              |----------------------------->|                       |
  |                              |              7. execute()    |                       |
  |                              |              ├─ DB: AutorunSession(Running)          |
  |                              |              ├─ JSONL: BatchStarted                  |
  |                              |              ├─ cap workers to min(batch, config)    |
  |                              |              │  ┌─────── DISPATCH LOOP ──────┐       |
  |                              |              │  │ for task in topo_order:    │       |
  |                              |              │  │   skip if done/running     │       |
  |                              |              │  │   skip if dep failed       │       |
  |                              |              │  │   acquire semaphore permit │       |
  |                              |              │  │   build WorkerConfig       │       |
  |                              |              │  │   tokio::spawn worker      │       |
  |                              |              │  │   sleep 100ms (stagger)    │       |
  |                              |              │  └────────────────────────────┘       |
  |                              |              │<── WorkerResult(s)                    |
  |                              |              │  classify: completed/failed/blocked   |
  |                              |              | 8. DB: AutorunSession(Completed)      |
  |                              |              ├─ JSONL: BatchCompleted                |
  |                              |<-------------|                                       |
  |                              | 9. generate_batch_report()                          |
  |  ← report + exit code       |                                                      |
```

### 2.2 Worker Lifecycle (Per-Task)

```text
ORCHESTRATOR                          WORKER (TmuxWorker::run)
  |                                        |
  | tokio::spawn(runner.run(cfg))          |
  |--------------------------------------->|
  |                                        | 1. Generate worker session ID (ULID)
  |                                        | 2. worktree.setup(wt_name)
  |                                        |    └─ git worktree add --detach
  |                                        | 3. Write codeflow-env.sh to worktree
  |                                        | 4. DB: CREATE autorun_worker (Running)
  |                                        | 5. JSONL: WorkerStarted
  |                                        | 6. tmux.create_session(name)
  |                                        | 7. Set env vars in tmux:
  |                                        |    ├─ AUTORUN_SESSION_ID
  |                                        |    ├─ AUTORUN_TASK_ID
  |                                        |    ├─ AUTORUN_ACCEPTANCE (base64)
  |                                        |    └─ CODEFLOW_WORKTREE_PATH
  |                                        | 8. claude.invoke(config)
  |                                        |    └─ claude -p "{prompt}"
  |                                        |       --dangerously-skip-permissions
  |                                        |       --output-format json
  |                                        | 9. Poll for worker-exit-code file
  |                                        | 10. Parse InvokeResult
  |                                        | 11. resolve_merge_conflicts()
  |                                        |     ├─ check_merge_conflicts()
  |                                        |     └─ attempt_rebase() (if needed)
  |                                        | 12. Merge queue: enqueue → dequeue
  |                                        | 13. DB: UPDATE autorun_worker (final)
  |                                        | 14. JSONL: WorkerCompleted/Failed
  |                                        | 15. Cleanup:
  |                                        |     ├─ tmux.kill_session()
  |                                        |     └─ worktree.cleanup()
  |<---------------------------------------|
  | WorkerResult { status, pr_number, .. } |
```

### 2.3 Claim Enforcement During Claude Execution

```text
Claude Edit/Write tool call
    |
    v
PreToolUse hook: gate-check
    |
    v
Read scope_policy from active-task.json
    |
    ├── PERMISSIVE ──> Allow (no claim check)
    |                  NOTE: Forbidden for autorun_eligible tasks
    |
    ├── HARD ──> File in file_scope?
    |            ├── YES ──> Allow (claim pre-acquired)
    |            └── NO  ──> BLOCK (exit 2, no claim attempt)
    |
    └── SOFT ──> File in file_scope?
                 ├── YES ──> Allow (claim pre-acquired at startup)
                 └── NO  ──> Coordinator::acquire(path, session_id)
                              ├── Ok(claim)  ──> Allow
                              │   └─ Emit ScopeExpansion event
                              └── Err(conflict) ──> BLOCK (exit 2)
                                  └─ Emit ClaimConflict event
```

### 2.4 Concurrent Workers -- Coordination

```text
                    ┌─────────────────────────────────┐
                    │      Orchestrator (Tokio)        │
                    │  Semaphore = min(batch, config)  │
                    └──────┬──────────┬───────────┬────┘
                           │          │           │
                    ┌──────▼───┐ ┌────▼─────┐ ┌───▼──────┐
                    │ Worker 1 │ │ Worker 2 │ │ Worker 3 │
                    │ wt-{s1}  │ │ wt-{s2}  │ │ wt-{s3}  │
                    └──────┬───┘ └────┬─────┘ └───┬──────┘
                           │          │           │
                    ┌──────▼──────────▼───────────▼──────┐
                    │        Loro CRDT (state.loro)       │
                    │                                     │
                    │  Claims Map:                        │
                    │    src/main.rs → Worker 1 (token 7) │
                    │    src/lib.rs  → Worker 2 (token 8) │
                    │    tests/*.rs  → Worker 3 (token 9) │
                    │                                     │
                    │  Merge Queue (FIFO):                │
                    │    [Worker 1, Worker 3]              │
                    │                                     │
                    │  Fencing Tokens:                    │
                    │    Monotonic counter (AtomicU64)     │
                    └─────────────────────────────────────┘
                                     │
                    ┌────────────────▼────────────────────┐
                    │       Shared State (symlinked)       │
                    │  .state/db/codeflow.db               │
                    │  .state/ledger/autorun-events.jsonl   │
                    │  .state/coordination/state.loro      │
                    └─────────────────────────────────────┘
```

### 2.5 Cancel/Abort Flow

```text
codeflow autorun cancel <task_id>
    |
    v
1. Query DB: autorun_worker WHERE task_id
    |
    v
2. Look up worker PID + tmux session
    |
    v
3. SIGTERM → wait 10s → SIGKILL (if still alive)
    |
    v
4. git checkout . && git clean -fd (discard changes)
    |
    v
5. claims::release_all(coordinator, worker_sid)
    |
    v
6. tmux kill-session -t {name}
    |
    v
7. WorktreeManager::cleanup_worktree(name)
    |
    v
8. Deregister from worktree registry
    |
    v
9. Close PR if open: gh pr close {pr_number} --delete-branch
    |
    v
10. Delete remote branch if pushed: git push origin --delete {branch}
    |
    v
11. UPDATE autorun_worker SET status = 'cancelled'
    |
    v
12. UPDATE autorun_task_run SET status = 'cancelled'
    |
    v
13. End state: main repo untouched, worktree removed, claims released


codeflow autorun abort [--batch <session_id>]
    |
    v
1. Write abort marker: .state/runtime/abort-{session_id}
    |
    v
2. UPDATE autorun_session SET status = 'aborting'
    |
    v
3. Orchestrator checks abort marker → stops dispatching new tasks
    |
    v
4. For each running worker: cancel sequence (steps 1-13 above)
    |
    v
5. For each pending task: mark skipped (reason: batch_aborted)
    |
    v
6. UPDATE autorun_session SET status = 'aborted', completed_at = now()
    |
    v
7. JSONL: BatchAborted event
    |
    v
8. End state: all worktrees removed, all claims released, DB updated
```

### 2.6 Sync Daemon Lifecycle

```text
Worktree count changes
    |
    v
WorktreeRegistry::locked_register_with_limit()
    |
    v
Count active worktrees
    |
    ├── count > 1 AND daemon not running
    │       |
    │       v
    │   sync::start_daemon(project_dir, interval_secs)
    │       |
    │       v
    │   ┌──── SYNC LOOP ────────────────────────────┐
    │   │  loop {                                    │
    │   │    sleep(interval_secs)                    │
    │   │    run_sync_cycle(config, peer_id)         │
    │   │      ├─ Read local state.loro              │
    │   │      ├─ Read peer states via git refs      │
    │   │      ├─ Merge CRDT deltas                  │
    │   │      ├─ Write merged state                 │
    │   │      └─ Push updated ref to peers          │
    │   │    cleanup_dead_workers(config)             │
    │   │      ├─ Check worker PIDs (kill(pid, 0))   │
    │   │      └─ Release claims for dead workers    │
    │   │  }                                         │
    │   └────────────────────────────────────────────┘
    │
    └── count <= 1 AND daemon running
            |
            v
        sync::stop_daemon(project_dir)
            |
            v
        Kill daemon process, remove PID file
```

## 3. Component Reference

### 3.1 Batch Parser

**File:** `codeflow-cli/core/src/autorun/batch.rs`

Parses YAML batch files, validates structure, and produces topologically sorted task order.

**Key structs:**

| Struct | Fields | Purpose |
|--------|--------|---------|
| `BatchFile` | `name`, `max_workers`, `auto_merge`, `target`, `tasks: Vec<TaskSpec>` | Raw YAML deserialization target |
| `TaskSpec` | `id`, `depends_on: Vec<String>`, `file_scope: Vec<String>`, `scope_policy: Option<String>` | Per-task specification in batch. `file_scope` and `scope_policy` are optional overrides — task markdown is the source of truth. |
| `ParsedBatch` | `name`, `file_path`, `max_workers`, `auto_merge`, `target`, `tasks`, `order: Vec<String>` | Validated batch with topo-sorted order |

**Key functions:**

| Function | Purpose |
|----------|---------|
| `parse_batch_file(path)` | Read YAML from disk, delegate to `parse_batch_data` |
| `parse_batch_data(data, file_path)` | Parse + validate + topo-sort |
| `parse_batch_data_with_project_dir(data, file_path, project_dir)` | Same, with enforcement policy lookup for protected branches |
| `validate_batch(batch)` | Check no dupes, deps exist, no self-deps, no cycles, merge protection |
| `validate_batch_extended(batch)` | Check autorun_eligible, file_scope, scope_policy, overlap detection |
| `topological_sort(tasks)` | Kahn's algorithm for dependency ordering |
| `read_task_scope(task_id, project_dir)` | Reads file_scope and scope_policy from task markdown frontmatter |

**Source of truth design:** The task markdown file (`project-management/epics/{area}/{epic}/tasks/{task}.md`) is the authoritative source for `file_scope`, `scope_policy`, and `acceptance` criteria. The batch file specifies only task IDs, execution order, and batch-level settings. Optional `file_scope` and `scope_policy` fields in `TaskSpec` are overrides that can narrow (but not widen) the task markdown values.

**Constants:**

- `DEFAULT_MAX_WORKERS`: 3
- `DEFAULT_PROTECTED_BRANCHES`: `["main", "master", "release/*", "production"]`
- `ENFORCEMENT_POLICY_PATH`: `.codeflow/config/enforcement/enforcement-policy.json`

**Examples:** See `.codeflow/config/autorun/examples/` for annotated batch files covering sequential, parallel, auto-merge, and complex dependency patterns.

### 3.2 Orchestrator

**File:** `codeflow-cli/core/src/autorun/orchestrator.rs`

Manages dependency-aware concurrent task execution using tokio semaphore.

**Key structs:**

| Struct | Fields | Purpose |
|--------|--------|---------|
| `WorkerConfig` | `session_id`, `worker_id`, `worker_num`, `task_id`, `batch_name`, `auto_merge`, `target`, `tmux_prefix`, `file_scope`, `scope_policy`, `blocked_behavior` | Configuration passed to each worker |
| `WorkerResult` | `worker_id`, `task_id`, `status`, `exit_code`, `pr_number`, `pr_url`, `error`, `branch_name`, `duration_sec` | Result returned from each worker |
| `Orchestrator<R, S>` | `runner: Arc<R>`, `store: Arc<S>` | Generic over `WorkerRunner` and `DataStore` |
| `ExecutionState` | `completed`, `failed`, `running`, `results`, `semaphore`, `abort`, `handles`, `tmux_sessions` | Shared mutable state during batch execution |

**Key methods:**

| Method | Purpose |
|--------|---------|
| `Orchestrator::new(runner, store)` | Create orchestrator with DI |
| `Orchestrator::execute(session_id, batch, project_dir, shutdown)` | Run batch; races dispatch loop against shutdown signal |
| `abort_cleanup(state, task_order)` | Kill tmux sessions, drain handles (10s deadline), mark pending as skipped |
| `drain_handles(handles, grace_period)` | Wait for JoinHandles with timeout, abort remaining |
| `dispatch_ready_tasks(...)` | Scan topo order, skip done/running/dep-failed, acquire semaphore, spawn |

**Graceful shutdown:** The `execute` method takes a `shutdown: Future` parameter. Production uses `tokio::signal::ctrl_c()`. On signal, the abort flag is set atomically; the dispatch loop stops; `abort_cleanup` kills tmux sessions and drains handles.

### 3.3 Worker

**File:** `codeflow-cli/core/src/autorun/worker.rs`

DI traits and `TmuxWorker` implementation for per-task execution.

**Traits:**

| Trait | Methods | Purpose |
|-------|---------|---------|
| `TmuxRunner` | `create_session`, `send_command`, `kill_session`, `has_session` | Tmux operations (mockable) |
| `ClaudeInvoker` | `invoke(InvokeConfig) -> InvokeResult` | Claude Code execution (mockable) |
| `WorktreeProvider` | `setup(name) -> WorktreeInfo`, `cleanup(name)` | Worktree lifecycle (mockable) |
| `WorkerRunner` | `run(WorkerConfig) -> WorkerResult` | Top-level worker execution |

**Key structs:**

| Struct | Fields | Purpose |
|--------|--------|---------|
| `TmuxWorker<T, C, W, S>` | `tmux`, `claude`, `worktree`, `timeout`, `project_dir`, `store` | Worker implementation with DI |
| `InvokeConfig` | `work_dir`, `prompt`, `session_id`, `task_id`, `auto_merge`, `target`, `tmux_session`, `acceptance_criteria` | Claude invocation parameters |
| `InvokeResult` | `exit_code`, `pr_number`, `pr_url`, `branch_name`, `output` | Claude invocation result |
| `WorktreeInfo` | `path: PathBuf` | Created worktree location |

**Merge conflict resolution:**

| Enum | Variants | Purpose |
|------|----------|---------|
| `MergeConflictAction` | `Continue`, `RebasedSuccessfully`, `MergeConflict { error }` | Outcome of conflict resolution |

The `resolve_merge_conflicts` function takes closures for `check_merge_conflicts` and `attempt_rebase` for testability. It retries rebase up to `max_rebase_attempts` with 2-second sleep between attempts.

**Default timeout:** `DEFAULT_WORKER_TIMEOUT` = 60 minutes (3600 seconds).

### 3.4 Config

**File:** `codeflow-cli/core/src/autorun/config.rs`

Loads and validates `.codeflow/config/parallel-work/parallel-work-config.json`.

**Top-level struct:** `ParallelWorkConfig`

| Section | Struct | Key Fields |
|---------|--------|------------|
| `worktree` | `WorktreeConfig` | `mode: WorktreeMode`, `max_concurrent: usize`, `base_dir: String` |
| `sync` | `SyncConfig` | `interval_secs: u64`, `auto_start: bool` |
| `merge` | `MergeConfig` | `auto_rebase: bool`, `queue_enabled: bool`, `max_rebase_attempts: usize` |
| `claims` | `ClaimsConfig` | `default_scope_policy: String`, `ttl_secs: u64`, `capture_events: bool` |
| `autorun` | `AutorunConfig` | `worker_timeout_secs: u64`, `blocked_behavior: String`, `report_dir: String` |

**WorktreeMode enum:** `Autorun` (default), `Always`, `Disabled`

**Validation constraints:**

| Field | Constraint |
|-------|-----------|
| `worktree.max_concurrent` | 1..=10 |
| `claims.ttl_secs` | >= 60 |
| `merge.max_rebase_attempts` | >= 1 |
| `autorun.worker_timeout_secs` | >= 60 |
| `autorun.blocked_behavior` | `"skip_and_continue"` or `"fail"` |

**Loading:** `load_config(project_dir)` returns defaults if config file is missing. All fields use `#[serde(default)]` so the config file is optional and partial configs work.

### 3.5 CRDT Coordination

**Directory:** `codeflow-cli/core/src/coordination/`

CRDT-based coordination for concurrent workers using Loro.

**Key modules:**

| Module | Purpose |
|--------|---------|
| `coordination/mod.rs` | Coordinator struct, acquire/release API |
| `coordination/loro.rs` | Loro CRDT document operations |
| `coordination/claims.rs` | Batch claim acquisition (`acquire_batch`) |
| `coordination/sync.rs` | Sync daemon (start, stop, run_sync_cycle, cleanup_dead_workers) |
| `coordination/merge_queue.rs` | FIFO merge queue (enqueue, dequeue, queue_len) |
| `coordination/types/events.rs` | Event type definitions |

**Key APIs:**

| Function | Signature | Purpose |
|----------|-----------|---------|
| `Coordinator::acquire` | `(path, session_id) -> Result<Claim>` | Acquire file claim |
| `Coordinator::release` | `(path, session_id) -> Result<()>` | Release file claim |
| `claims::acquire_batch` | `(coordinator, paths, session_id) -> Result<Vec<Claim>>` | Batch claim acquisition |
| `merge_queue::enqueue` | `(coordinator, entry) -> Result<()>` | Add to merge queue |
| `merge_queue::dequeue` | `(coordinator) -> Result<Option<Entry>>` | Remove from merge queue |
| `sync::start_daemon` | `(project_dir, interval_secs) -> Result<()>` | Start sync daemon |
| `sync::stop_daemon` | `(project_dir) -> Result<()>` | Stop sync daemon |
| `sync::daemon_status` | `(project_dir) -> DaemonStatus` | Health check |
| `sync::cleanup_dead_workers` | `(config) -> Result<()>` | Release claims for dead workers |

**State file:** `.state/coordination/state.loro` (binary Loro CRDT document, symlinked across worktrees)

### 3.6 Worktree Management

**Directory:** `codeflow-cli/core/src/worktree/`

Git worktree lifecycle management.

**Key modules:**

| Module | Purpose |
|--------|---------|
| `worktree/mod.rs` | `WorktreeManager` struct |
| `worktree/paths.rs` | `WorktreePaths` for state file resolution |
| `worktree/registry.rs` | `WorktreeRegistry` with `locked_register_with_limit` |
| `worktree/setup.rs` | `setup_detached()` for worktree creation |
| `worktree/cleanup.rs` | `cleanup_worktree()` for removal |

**Worktree layout:**

```text
.git-worktrees/worktree-{SID}/
├── .state/
│   ├── db/             -> ../../.state/db/              (symlink, shared)
│   ├── ledger/         -> ../../.state/ledger/           (symlink, shared)
│   ├── coordination/   -> ../../.state/coordination/     (symlink, shared)
│   ├── logs/           -> ../../.state/logs/             (symlink, shared)
│   ├── registry/       -> ../../.state/registry/         (symlink, shared)
│   ├── backups/        -> ../../.state/backups/          (symlink, shared)
│   ├── runtime/                                         (LOCAL per-worktree)
│   ├── session/                                         (LOCAL per-worktree)
│   └── sentinels/                                       (LOCAL per-worktree)
└── (full working copy)
```

**Registry:** `.state/worktrees.yaml` stores `WorktreeEntry` records (session_id, path, branch, status). Max 3 concurrent worktrees enforced by `locked_register_with_limit`.

### 3.7 CLI Commands

**File:** `codeflow-cli/cli/src/cmd/autorun.rs`

Clap-based subcommand definitions for `codeflow autorun`.

**Subcommands:**

| Subcommand | Arguments/Flags | Purpose |
|------------|-----------------|---------|
| `run` | `--batch <path>` (default: `.codeflow/config/autorun/batch.yaml`) | Execute batch |
| `status` | `--batch <session_id>` (optional) | Show active batch status |
| `attach` | `<task_id>` | Attach to worker's tmux session |
| `logs` | `<task_id>`, `--follow` | View worker tmux logs |
| `cancel` | `<task_id>` | Cancel single worker |
| `abort` | `--batch <session_id>` (optional) | Abort entire batch |
| `results` | `--batch <session_id>` (optional) | Display batch results |
| `history` | `--limit`, `--since`, `--status`, `--batch-name`, `--all` | Show historical batches |

**Pre-flight checks (before batch parsing):**

| Check | Function | Failure |
|-------|----------|---------|
| tmux available | `check_tmux_available()` | `which tmux` fails |
| Git clean | `check_git_clean(project_dir)` | `git status --porcelain` non-empty |
| Target branch exists | `check_target_branch(project_dir, target)` | Neither local nor remote ref found |

### 3.8 Models

**File:** `codeflow-cli/core/src/models/autorun.rs`

SurrealDB model structs for autorun state.

**Structs:**

| Struct | Key Fields | Purpose |
|--------|------------|---------|
| `AutorunSession` | `id`, `batch_file`, `batch_name`, `status`, `max_session_workers`, `total_tasks`, `completed_tasks`, `failed_tasks`, `pid`, `skipped_tasks`, `created_at`, `completed_at` | Batch execution record |
| `AutorunWorker` | `id`, `session_id`, `worker_num`, `task_id`, `status`, `tmux_session`, `worktree_path`, `file_scope`, `scope_policy`, `worker_session_id`, `pr_number`, `started_at`, `completed_at` | Per-worker record |
| `AutorunTaskRun` | `id`, `worker_id`, `task_id`, `session_id`, `status`, `branch_name`, `worktree_path`, `pr_number`, `pr_url`, `blocked_reason`, `claim_conflicts`, `merge_conflicts`, `started_at`, `completed_at`, `duration_seconds`, `exit_code`, `error_message`, `verification_result`, `created_at` | Per-task execution record |

### 3.9 Git Conflict Handling

**File:** `codeflow-cli/core/src/git/conflict.rs`

In-memory merge conflict detection using git2.

**Key types:**

| Type | Fields | Purpose |
|------|--------|---------|
| `ConflictResult` | `has_conflicts: bool`, `conflicting_files: Vec<String>`, `target_branch: String` | Result of conflict check |
| `RebaseResult` | `Success`, `ConflictAborted { conflicting_files }` | Outcome of rebase attempt |

**Key functions:**

| Function | Purpose |
|----------|---------|
| `check_merge_conflicts(repo_path, target_branch)` | In-memory tree merge to detect conflicts without modifying working tree |
| `attempt_rebase(repo_path, target_branch)` | Auto-rebase with `RebaseResult` |

The conflict check computes the merge base between HEAD and the target branch, then performs an in-memory three-way merge to detect conflicting files. This runs before PR creation (PF6-TSK-05) to fail fast on merge issues.

### 3.10 Event System

**File:** `codeflow-cli/core/src/coordination/types/events.rs`

Event types for parallel execution tracking.

**Coordination events** (written to `coordination-events.jsonl`):

| Event | Key Fields | When |
|-------|------------|------|
| `ClaimAcquired` | `session_id`, `path`, `task_id` | File claim acquired |
| `ClaimConflict` | `session_id`, `path`, `held_by` | Claim blocked by another worker |
| `ClaimReleased` | `session_id`, `path` | Claim released |
| `ScopeExpansion` | `session_id`, `path`, `original_scope` | Out-of-scope file claimed (soft policy) |
| `MergeConflictDetected` | `session_id`, `branch`, `target_branch` | Merge conflicts found |
| `MergeRebaseAttempted` | `session_id`, `branch`, `target_branch`, `success` | Rebase attempt result |

**Autorun events** (written to `autorun-events.jsonl`):

| Event | Key Fields | When |
|-------|------------|------|
| `BatchStarted` | `session_id`, `batch_name`, `total_tasks` | Batch execution begins |
| `BatchCompleted` | `session_id`, `completed_tasks`, `failed_tasks`, `skipped_tasks` | Batch finishes |
| `BatchAborted` | `session_id`, `reason` | Batch abort requested |
| `WorkerStarted` | `session_id`, `worker_id`, `task_id` | Worker spawned |
| `WorkerCompleted` | `session_id`, `worker_id`, `task_id`, `pr_number` | Worker succeeds |
| `WorkerFailed` | `session_id`, `worker_id`, `task_id`, `error` | Worker fails |
| `WorkerTimeout` | `session_id`, `worker_id`, `task_id` | Worker times out |
| `WorkerBlocked` | `session_id`, `worker_id`, `task_id`, `reason` | Worker blocked (claims) |
| `WorkerCancelled` | `session_id`, `worker_id`, `task_id`, `reason` | Worker cancelled |

## 4. Data Model

### SurrealDB tables

**`autorun_session`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Session identifier |
| `batch_file` | `string` | Path to batch YAML |
| `batch_name` | `option<string>` | Human-readable batch name |
| `status` | `AutorunSessionStatus` | `Pending`, `Running`, `Completed`, `Failed`, `Aborted`, `Aborting` |
| `max_session_workers` | `i32` | Configured max concurrent workers |
| `total_tasks` | `i32` | Total tasks in batch |
| `completed_tasks` | `i32` | Successfully completed tasks |
| `failed_tasks` | `i32` | Failed tasks |
| `skipped_tasks` | `i32` | Skipped tasks (dep failure or abort) |
| `pid` | `option<i64>` | Orchestrator PID (for orphan detection) |
| `created_at` | `string` | ISO 8601 creation time |
| `completed_at` | `option<string>` | ISO 8601 completion time |

**`autorun_worker`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Worker identifier |
| `session_id` | `string` | Parent session ID |
| `worker_num` | `i32` | Worker number in batch |
| `task_id` | `string` | Assigned task ID |
| `status` | `AutorunWorkerStatus` | `Queued`, `Running`, `Completed`, `Failed`, `Cancelled`, `Timeout` |
| `tmux_session` | `option<string>` | Tmux session name |
| `worktree_path` | `option<string>` | Worktree directory path |
| `file_scope` | `array<string>` | File patterns claimed |
| `scope_policy` | `string` | `soft` (default), `hard`, or `permissive` |
| `worker_session_id` | `option<string>` | Claude session ID within worker |
| `pr_number` | `option<i64>` | PR number if created |
| `started_at` | `option<string>` | ISO 8601 start time |
| `completed_at` | `option<string>` | ISO 8601 completion time |

**`autorun_task_run`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Task run identifier |
| `worker_id` | `string` | Parent worker ID |
| `task_id` | `string` | Task being executed |
| `session_id` | `string` | Parent session ID |
| `status` | `AutorunTaskRunStatus` | `Pending`, `Running`, `Completed`, `Failed`, `Blocked`, `Skipped`, `Cancelled`, `MergeConflict`, `Timeout` |
| `branch_name` | `option<string>` | Feature branch name |
| `worktree_path` | `option<string>` | Worktree directory path |
| `pr_number` | `option<i64>` | PR number if created |
| `pr_url` | `option<string>` | PR URL |
| `blocked_reason` | `option<string>` | Why task was blocked |
| `claim_conflicts` | `option<array<string>>` | Paths with claim conflicts |
| `merge_conflicts` | `option<array<string>>` | Paths with merge conflicts |
| `started_at` | `option<string>` | ISO 8601 start time |
| `completed_at` | `option<string>` | ISO 8601 completion time |
| `duration_seconds` | `option<i64>` | Execution time in seconds |
| `exit_code` | `option<i64>` | Claude exit code |
| `error_message` | `option<string>` | Error details |
| `verification_result` | `option<string>` | Acceptance criteria verification |
| `created_at` | `string` | ISO 8601 creation time |

### CRDT state

**Location:** `.state/coordination/state.loro`

| Data Structure | Purpose |
|---------------|---------|
| Claims map | `path -> { session_id, fencing_token, timestamp }` |
| Merge queue | FIFO list of `{ session_id, branch, pr_number, timestamp }` |
| Fencing tokens | Monotonic counter ensuring claim validity across crashes |

## 5. Configuration Reference

### parallel-work-config.json

**Location:** `.codeflow/config/parallel-work/parallel-work-config.json`

All fields have defaults via `#[serde(default)]`. The config file is optional -- defaults apply when absent.

```json
{
  "worktree": {
    "mode": "autorun",
    "max_concurrent": 3,
    "base_dir": ".git-worktrees"
  },
  "sync": {
    "interval_secs": 5,
    "auto_start": true
  },
  "merge": {
    "auto_rebase": true,
    "queue_enabled": true,
    "max_rebase_attempts": 3
  },
  "claims": {
    "default_scope_policy": "soft",
    "ttl_secs": 4200,
    "capture_events": true
  },
  "autorun": {
    "worker_timeout_secs": 3600,
    "blocked_behavior": "skip_and_continue",
    "report_dir": "project-management/tracking/autorun"
  }
}
```

**Field reference:**

| Section | Field | Type | Default | Description |
|---------|-------|------|---------|-------------|
| `worktree` | `mode` | `string` | `"autorun"` | When to create worktrees: `autorun`, `always`, `disabled` |
| `worktree` | `max_concurrent` | `int` | `3` | Max concurrent worktrees (1-10) |
| `worktree` | `base_dir` | `string` | `".git-worktrees"` | Worktree base directory |
| `sync` | `interval_secs` | `int` | `30` | CRDT sync interval in seconds |
| `sync` | `auto_start` | `bool` | `true` | Auto-start daemon when worktree count > 1 |
| `merge` | `auto_rebase` | `bool` | `true` | Attempt rebase on merge conflicts |
| `merge` | `queue_enabled` | `bool` | `true` | Use FIFO merge queue for PR ordering |
| `merge` | `max_rebase_attempts` | `int` | `3` | Max rebase retries (>= 1) |
| `claims` | `default_scope_policy` | `string` | `"soft"` | Default scope policy for tasks |
| `claims` | `ttl_secs` | `int` | `4200` | Claim TTL in seconds (>= 60) |
| `claims` | `capture_events` | `bool` | `true` | Log coordination events to JSONL |
| `autorun` | `worker_timeout_secs` | `int` | `3600` | Max worker execution time (>= 60) |
| `autorun` | `blocked_behavior` | `string` | `"skip_and_continue"` | On claim conflict: `skip_and_continue` or `fail` |
| `autorun` | `report_dir` | `string` | `"project-management/tracking/autorun"` | Batch report output directory |

### pathflow-config.json (relevant fields)

**Location:** `.codeflow/config/pathflow/pathflow-config.json`

| Field | Relevance |
|-------|-----------|
| `stages[].max_parallel` | Max parallel instances per work stage |
| `stages[].batch_size` | Items per batch for parallel execution |
| `rework.max_rework_iterations` | Max WS-REV rework iterations (3) |
| `rework.max_qa_retries` | Max WS-QA retries (3) |
| `rework.stage_timeout_minutes` | Per-stage timeout (60 minutes) |

### enforcement-policy.json (relevant fields)

**Location:** `.codeflow/config/enforcement/enforcement-policy.json`

| Field | Relevance |
|-------|-----------|
| `merge_protection.branches` | Protected branches: `main`, `master`, `release/*`, `production` |
| `merge_protection.rules` | `auto_merge:true` + protected target is FORBIDDEN |

## 6. Environment Variables

| Variable | Purpose | Set By | Used By |
|----------|---------|--------|---------|
| `AUTORUN_SESSION_ID` | Worker-specific session ID; matches CRDT claim identity; presence indicates autorun mode | Worker invocation (`autorun.rs:1312`) | Lead (autorun detection), hooks (claim operations) |
| `AUTORUN_BATCH_ID` | Batch-level session ID for correlation across workers | Worker invocation (`autorun.rs:1317`) | Observability, batch tracking |
| `AUTORUN_TASK_ID` | Pre-assigned task ID from the batch file | CLI orchestrator | Workers (skip PF2 active work check) |
| `AUTORUN_ACCEPTANCE` | Base64-encoded acceptance criteria from task markdown | CLI orchestrator | Workers (verification) |
| `CODEFLOW_WORKTREE_PATH` | Absolute path to worker's isolated git worktree | Worker setup (`worker.rs`) | Hooks, state file resolution |
| `CODEFLOW_SESSION_ID` | Session identifier (written to `codeflow-env.sh` and per-PID `codeflow-env-{PID}.sh`) | SessionStart hook | All hooks, state lookups |
| `CODEFLOW_CLAIM_TTL_SECS` | Override claim TTL (optional) | Config | Coordination layer |

## 7. Chain Completeness Matrix

Summary of all implementation chains and their status.

| Chain | Components | Status |
|-------|-----------|--------|
| Batch parsing | `batch.rs` (parse, validate, topo-sort) | Implemented |
| Extended validation | `batch.rs` (autorun_eligible, file_scope, scope_policy, overlap) | Implemented |
| Orchestrator dispatch | `orchestrator.rs` (semaphore, dispatch loop, dep tracking) | Implemented |
| Graceful shutdown | `orchestrator.rs` (SIGINT handler, abort cleanup, handle drain) | Implemented |
| Worker lifecycle | `worker.rs` (worktree setup, tmux, claude invoke, cleanup) | Implemented |
| Merge conflict resolution | `worker.rs` + `git/conflict.rs` (check, rebase, retry) | Implemented |
| Claim enforcement | `coordination/` (acquire, release, batch acquire, conflict) | Implemented |
| Merge queue | `coordination/merge_queue.rs` (enqueue, dequeue, FIFO) | Implemented |
| Sync daemon | `coordination/sync.rs` (start, stop, cycle, dead worker cleanup) | Implemented |
| DB recording | `models/autorun.rs` + `store` trait (session, worker, task_run) | Implemented |
| JSONL events | `coordination/types/events.rs` + `ledger/routing.rs` | Implemented |
| CLI subcommands | `cli/src/cmd/autorun.rs` (run, status, attach, logs, cancel, abort, results, history) | Implemented |
| Pre-flight checks | `cli/src/cmd/autorun.rs` (tmux, git clean, target branch) | Implemented |
| Config loading | `config.rs` (5 sections, defaults, validation) | Implemented |
| Batch reports | `orchestrator.rs` (generate_batch_report) | Implemented |

## 8. Cross-References

| Resource | Path | Relevance |
|----------|------|-----------|
| CLAUDE.md | `.claude/CLAUDE.md` | Sections 4.4 (Autorun Mode), 5 (Cross-Session Parallelism), 7 (Enforcement) |
| cf-development agent | `.claude/agents/cf-development.md` | Autorun behavior section, decision tiers, timeout handling |
| Autorun spec | `.codeflow/docs/analysis/parallel-work/autorun-spec.md` | Authoritative specification |
| Parallel work config spec | `.codeflow/docs/analysis/parallel-work/parallel-work-config-spec.md` | Config field descriptions |
| Parallel work config | `.codeflow/config/parallel-work/parallel-work-config.json` | Runtime config |
| PathFlow config | `.codeflow/config/pathflow/pathflow-config.json` | Phase/stage/pipeline definitions |
| Enforcement policy | `.codeflow/config/enforcement/enforcement-policy.json` | Protected branches, merge rules |
| cf-autorun command | `.claude/commands/cf-autorun.md` | Slash command definition |
| Batch examples | `.codeflow/config/autorun/examples/` | Annotated batch file examples (sequential, parallel, auto-merge, complex deps) |
