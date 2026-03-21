---
title: "Autorun Full Chain Trace"
version: "1.0.0"
status: proposed
epic: INF-EPC-023
created_at: "2026-03-21"
updated_at: "2026-03-21"
---

# Autorun Full Chain Trace

End-to-end chain trace for all autorun scenarios. At each node, components are annotated with `[FILE:LINE]` for implemented code, or `[MISSING]` for gaps requiring implementation.

## Scenario A: Single Interactive Session (Baseline)

A single developer running `claude` in an interactive session with PathFlow.

```text
User prompt -> SessionStart hook
    [core/src/hooks/session_start.rs:1-300]
    |
    v
WorktreeManager::setup_detached() (if worktree mode)
    [core/src/worktree/setup.rs:1-150]
    |
    v
PF1-INIT -> PF2-CONTEXT -> PF3-CLASSIFY (branch created)
    [gate-check enforced: core/src/hooks/pre_tool_use.rs:1-300]
    |
    v
PF4-EXECUTE: cf-development (WS-DEV)
    Edit/Write gated on pf-3 sentinel
    scope_policy enforcement via try_acquire_claim()
    [core/src/hooks/pre_tool_use.rs: try_acquire_claim()]
    |
    v
WS-REV -> WS-QA -> PF5-VERIFY -> PF6-COMPLETE
    PR created via cf-git-operations
    Merge conflict check: [core/src/git/conflict.rs:40-95]
    |
    v
PF7-END -> SessionEnd cleanup
    [core/src/hooks/session_end.rs]
    Worktree destroyed, claims released
```

**Chain status:** All links fully implemented. This is the reference path.

## Scenario B: Autorun Batch Happy Path

A batch of tasks executed via `codeflow autorun run --batch tasks.yaml`.

```text
$ codeflow autorun run --batch tasks.yaml
    [cli/src/cmd/autorun.rs:12-15]
    |
    v
Pre-flight checks
    tmux available: [MISSING - Gap 11]
    git status clean: [MISSING - Gap 11]
    target branch exists: [MISSING - Gap 11]
    |
    v
Parse batch YAML
    [core/src/autorun/batch.rs:73-116]
    validate_batch(): structure, deps, cycles, protected branches
    autorun_eligible check: [MISSING - Gap 6]
    file_scope overlap detection: [MISSING - Gap 7]
    target_branch validation: [MISSING - Gap 9]
    |
    v
Load parallel-work config
    [core/src/autorun/config.rs:152-168]
    autorun section: [MISSING - Gap 16]
    |
    v
Generate session ID
    [core/src/session/mod.rs: generate_session_id()]
    |
    v
Record batch start
    DB autorun_session: [MISSING - Gap 8]
    JSONL batch_started: [MISSING - Gap 15]
    |
    v
Orchestrator dispatch loop
    [core/src/autorun/orchestrator.rs:91-131]
    Semaphore bounded: min(max_workers, max_concurrent)
    SIGINT handler: [MISSING - Gap 17]
    Abort marker check: [MISSING - Gap 17]
    |
    v
For each ready task -> spawn worker
    [core/src/autorun/orchestrator.rs:199-218]
    tmux_prefix: "codeflow-worker" (not batch-scoped) [Gap 18]
    |
    v
Worker::run()
    [core/src/autorun/worker.rs:160-356]
    |
    +-- Worktree setup
    |   [core/src/autorun/worker.rs:179]
    |   RealWorktreeProvider -> WorktreeManager::setup_detached()
    |
    +-- Write codeflow-env.sh
    |   [core/src/autorun/worker.rs:184-194]
    |
    +-- Pre-acquire claims
    |   [core/src/autorun/worker.rs:196-219]
    |   Claim conflict: warn-only [Gap 4]
    |
    +-- Write active-task.json
    |   [core/src/autorun/worker.rs:221-233]
    |
    +-- Create tmux session
    |   [core/src/autorun/worker.rs:236]
    |
    +-- Invoke Claude
    |   [core/src/autorun/worker.rs:239-252]
    |   RealClaude::invoke: NO-OP STUB [Gap 1, Gap 13]
    |
    +-- Check merge conflicts (warn-only)
    |   [core/src/autorun/worker.rs:254-270]
    |   attempt_rebase: [MISSING - Gap 3]
    |
    +-- Enqueue merge queue
    |   [core/src/autorun/worker.rs:273-287]
    |   Dequeue: blind front-pop [Gap 2]
    |
    +-- Cleanup: release claims, kill tmux, remove worktree
        [core/src/autorun/worker.rs:289-318]
        maybe_auto_stop_daemon: [MISSING - Gap 5]
    |
    v
Report results
    [cli/src/cmd/autorun.rs:69-96]
    DB recording: [MISSING - Gap 8]
    Batch report: [MISSING - Gap 10]
```

## Scenario C: Claim Conflict

Worker B attempts to claim a file already held by Worker A.

```text
Worker A starts, acquires claims for file_scope
    acquire_batch(coordinator, ["src/foo.rs"], worker_a_sid)
    [core/src/autorun/worker.rs:196-219]
    [core/src/coordination/claims.rs: acquire_batch()]
    |
    v
Worker B starts, attempts same file
    acquire_batch(coordinator, ["src/foo.rs"], worker_b_sid)
    |
    v
Coordinator::acquire("src/foo.rs", worker_b_sid)
    [core/src/coordination/loro.rs: acquire()]
    |
    +-- LoroMap lookup: claims["src/foo.rs"] exists, held by worker_a_sid
    +-- TTL check: claim not expired
    +-- Result: Err(CoordinationError::ClaimConflict { ... })
    |
    v
Current behavior: eprintln warning, continue [Gap 4]
    [core/src/autorun/worker.rs:215-217]
    |
    v
Target behavior: [MISSING]
    Read blocked_behavior from autorun config
    If "skip_and_continue": skip task, mark as blocked
    If "fail": return error, mark as failed
    Record blocked_reason and claim_conflicts in autorun_task_run
    Emit worker_blocked event to autorun-events.jsonl
```

## Scenario D: Worker Failure

Claude exits with non-zero exit code (test failure, review rejection after max iterations).

```text
Worker::run() invokes Claude
    [core/src/autorun/worker.rs:239-252]
    |
    v
Claude exits with exit_code != 0
    Currently: InvokeResult always has exit_code=0 (stub) [Gap 1]
    Target: real exit code from marker file
    |
    v
Worker builds WorkerResult with status="failed"
    [core/src/autorun/worker.rs:320-338]
    |
    v
Cleanup runs regardless
    [core/src/autorun/worker.rs:289-318]
    Release claims, kill tmux, remove worktree
    |
    v
Orchestrator marks task as failed
    [core/src/autorun/orchestrator.rs:246-250]
    Dependent tasks auto-skipped
    [core/src/autorun/orchestrator.rs:167-181]
    |
    v
DB recording: [MISSING - Gap 8]
JSONL worker_failed event: [MISSING - Gap 15]
```

## Scenario E: Worker Timeout

Worker exceeds `worker_timeout_secs` (default 3600s).

```text
Worker::run() wraps Claude invocation in tokio::time::timeout
    [core/src/autorun/worker.rs:239-252]
    timeout = DEFAULT_WORKER_TIMEOUT (60 min)
    Config-driven timeout: [MISSING - Gap 16]
    |
    v
Timeout fires
    |
    v
Worker builds WorkerResult with status="timeout", exit_code=124
    [core/src/autorun/worker.rs:343-354]
    |
    v
Cleanup: claims released, tmux killed, worktree removed
    [core/src/autorun/worker.rs:289-318]
    SIGTERM -> 10s -> SIGKILL: [MISSING] (current: just kill_session)
    |
    v
DB recording: [MISSING - Gap 8]
JSONL worker_timeout event: [MISSING - Gap 15]
```

## Scenario F: Merge Conflict

Worker completes successfully but branch conflicts with target.

```text
Claude exits with exit_code=0
    [core/src/autorun/worker.rs:254-270]
    |
    v
check_merge_conflicts(worktree_path, target_branch)
    [core/src/git/conflict.rs:40-95]
    ConflictResult { has_conflicts: true, conflicting_files: [...] }
    |
    v
Current behavior: eprintln warning, continue [Gap 3]
    [core/src/autorun/worker.rs:258-262]
    |
    v
Target behavior: [MISSING]
    Read auto_rebase from merge config
    [core/src/autorun/config.rs:102-108]
    If auto_rebase=true:
        attempt_rebase(worktree_path, target_branch)
        [core/src/git/conflict.rs:148-180]
        Retry up to max_rebase_attempts (default 3)
        On success: continue to merge queue
        On ConflictAborted: set status="merge_conflict"
    If auto_rebase=false:
        Set status="merge_conflict" immediately
    |
    v
Record merge_conflict in autorun_task_run: [MISSING - Gap 8]
Emit merge_conflict_detected event: [MISSING - Gap 15]
```

## Scenario G: Parallel Interactive Sessions

Two interactive sessions running simultaneously in separate worktrees.

```text
Session A starts -> SessionStart hook
    WorktreeManager::setup_detached() -> worktree-{SID_A}
    [core/src/worktree/setup.rs]
    WorktreeRegistry::locked_register_with_limit(max=3)
    [core/src/worktree/registry.rs]
    |
    v
Session B starts -> SessionStart hook
    WorktreeManager::setup_detached() -> worktree-{SID_B}
    WorktreeRegistry: 2 of 3 slots used
    |
    v
Both sessions edit different files
    scope_policy=permissive (interactive default)
    No claim enforcement
    [core/src/hooks/pre_tool_use.rs: try_acquire_claim()]
    |
    v
Session A creates PR
    check_merge_conflicts() before PR creation
    [core/src/git/conflict.rs:40-95]
    merge_queue::enqueue()
    [core/src/coordination/merge_queue.rs:46-54]
    |
    v
Session B creates PR
    merge_queue::enqueue() (position 2 in FIFO)
    Sync daemon propagates CRDT state between worktrees
    [core/src/coordination/sync.rs]
    Daemon auto-started when worktree count > 1
```

**Chain status:** Fully implemented. Claims are permissive in interactive mode.

## Scenario H: Protected Files in Scope

A batch task has `file_scope` entries that include protected paths.

```text
Parse batch YAML
    [core/src/autorun/batch.rs:73-116]
    |
    v
Validate task file_scope against enforcement-policy.json
    Protected paths: .claude/CLAUDE.md, .claude/settings.json, etc.
    Current: [MISSING - Gap 6]
    |
    v
Target behavior:
    Read enforcement-policy.json at parse time
    [.codeflow/config/enforcement/enforcement-policy.json]
    For each task file_scope entry:
        Check against protection-guard protected paths
        If protected path found: reject task with ERROR
    |
    v
auto_merge + protected target_branch
    Current: checks PROTECTED_BRANCHES constant
    [core/src/autorun/batch.rs:156-174]
    Already implemented (main, master, production, release/*)
```

## Scenario I: Daemon Lifecycle

Sync daemon starts when worktree count > 1, stops when <= 1.

```text
First worktree created (count=1)
    WorktreeRegistry::locked_register_with_limit()
    [core/src/worktree/registry.rs]
    Daemon check: count <= 1, no daemon needed
    |
    v
Second worktree created (count=2)
    Daemon auto-start (if sync.auto_start=true)
    start_daemon(project_dir, interval_secs)
    [core/src/coordination/sync.rs: start_daemon()]
    Writes PID file at .state/coordination/sync-daemon.pid
    |
    v
Daemon runs sync cycles every interval_secs
    run_sync_cycle(): compaction + TTL claim cleanup
    [core/src/coordination/sync.rs: run_sync_cycle()]
    |
    v
Worktree removed (count=1)
    Worker cleanup calls worktree.cleanup()
    [core/src/autorun/worker.rs:316-318]
    maybe_auto_stop_daemon(): [NOT CALLED - Gap 5]
    [core/src/worktree/registry.rs:350-380]
    |
    v
Target: worker cleanup calls maybe_auto_stop_daemon()
    Reads registry, counts active worktrees
    If count <= 1: stop_daemon()
    [core/src/coordination/sync.rs: stop_daemon()]
```

## Scenario J: Batch Validation

Complete validation flow for a YAML batch file.

```text
Read batch YAML
    [core/src/autorun/batch.rs:73-77]
    |
    v
Deserialize to BatchFile struct
    [core/src/autorun/batch.rs:85-86]
    |
    v
validate_batch()
    [core/src/autorun/batch.rs:119-177]
    |
    +-- Empty tasks check [IMPLEMENTED: batch.rs:121]
    +-- Empty task ID check [IMPLEMENTED: batch.rs:127]
    +-- Duplicate task ID check [IMPLEMENTED: batch.rs:129-134]
    +-- Dependency reference check [IMPLEMENTED: batch.rs:138-153]
    +-- Self-dependency check [IMPLEMENTED: batch.rs:146-151]
    +-- Protected branch + auto_merge check [IMPLEMENTED: batch.rs:156-174]
    |
    +-- autorun_eligible validation [MISSING - Gap 6]
    +-- acceptance non-empty validation [MISSING - Gap 6]
    +-- scope_policy != permissive validation [MISSING - Gap 6]
    +-- file_scope overlap detection [MISSING - Gap 7]
    +-- target_branch existence check [MISSING - Gap 9]
    +-- max_workers vs max_concurrent reconciliation [MISSING - Gap 6]
    |
    v
Topological sort (Kahn's algorithm)
    [core/src/autorun/batch.rs:184-230]
    |
    v
Build ParsedBatch
    [core/src/autorun/batch.rs:107-115]
```

## Chain Link Table

Every component connection in the autorun system.

| From | To | Link Type | Status | File:Line |
|------|----|-----------|--------|-----------|
| CLI `autorun` command | `batch::parse_batch_file()` | Function call | OK | `cli/src/cmd/autorun.rs:28` |
| CLI `autorun` command | `config::load_config()` | Function call | OK | `cli/src/cmd/autorun.rs:32` |
| CLI `autorun` command | `Orchestrator::execute()` | Function call | OK | `cli/src/cmd/autorun.rs:60-63` |
| Orchestrator | `WorkerRunner::run()` | Trait dispatch | OK | `orchestrator.rs:238` |
| TmuxWorker | `WorktreeProvider::setup()` | Trait dispatch | OK | `worker.rs:179` |
| TmuxWorker | `ClaudeInvoker::invoke()` | Trait dispatch | STUB | `worker.rs:241` |
| TmuxWorker | `TmuxRunner::create_session()` | Trait dispatch | OK | `worker.rs:236` |
| TmuxWorker | `claims::acquire_batch()` | Function call | PARTIAL (warn-only) | `worker.rs:210` |
| TmuxWorker | `check_merge_conflicts()` | Function call | PARTIAL (warn-only) | `worker.rs:257` |
| TmuxWorker | `merge_queue::locked_enqueue()` | Function call | OK | `worker.rs:282` |
| TmuxWorker | `merge_queue::locked_dequeue()` | Function call | PARTIAL (blind pop) | `worker.rs:311` |
| TmuxWorker | `claims::release_all()` | Function call | OK | `worker.rs:300` |
| TmuxWorker | `WorktreeProvider::cleanup()` | Trait dispatch | OK | `worker.rs:316` |
| TmuxWorker | `maybe_auto_stop_daemon()` | Function call | MISSING | `worker.rs:316-318` |
| Orchestrator | DB autorun_session | Write | MISSING | `orchestrator.rs:91` |
| Orchestrator | JSONL autorun-events | Write | MISSING | `orchestrator.rs:91` |
| Worker | DB autorun_worker | Write | MISSING | `worker.rs:160` |
| Worker | DB autorun_task_run | Write | MISSING | `worker.rs:160` |
| Config | AutorunConfig | Deserialization | MISSING | `config.rs:17-28` |
| CLI | Pre-flight checks | Validation | MISSING | `autorun.rs:17` |
| CLI | Subcommands (status, etc.) | Command dispatch | MISSING | `autorun.rs:12` |
| Orchestrator | SIGINT handler | Signal | MISSING | `orchestrator.rs:91` |
| Batch validation | Task markdown | File read | MISSING | `batch.rs:119` |
| Batch validation | enforcement-policy.json | File read | MISSING | `batch.rs:119` |
| RealClaude | tmux + file-marker | Process mgmt | MISSING | `autorun.rs:156` |

## Complete Gap List

All 20 gaps with evidence and implementation task mapping.

| # | Gap | Evidence | Severity | Task |
|---|-----|----------|----------|------|
| 1 | Claude invoker no-op stub | `RealClaude::invoke()` at `cli/src/cmd/autorun.rs:157-170` returns hardcoded `InvokeResult { exit_code: 0, ... }` | CRITICAL | A |
| 2 | Merge queue blind dequeue | `locked_dequeue()` at `worker.rs:311` pops front entry without verifying session_id | HIGH | B |
| 3 | Merge conflict warn-only | `check_merge_conflicts()` result at `worker.rs:258-262` only logged | HIGH | C |
| 4 | Claim conflict warn-only | `acquire_batch()` failure at `worker.rs:215-217` only logged | HIGH | B |
| 5 | Missing daemon auto-stop | Worker cleanup at `worker.rs:316-318` does not call `maybe_auto_stop_daemon()` | MEDIUM | B |
| 6 | Missing autorun_eligible validation | `validate_batch()` at `batch.rs:119-177` only checks structure | HIGH | D |
| 7 | No file_scope overlap detection | `validate_batch()` does not check for overlapping file_scope | MEDIUM | D |
| 8 | No DB/JSONL recording | `Orchestrator::execute()` at `orchestrator.rs:91-131` writes no events | HIGH | E |
| 9 | target_branch not validated for existence | `validate_batch()` at `batch.rs:156-174` only checks protected list | MEDIUM | D |
| 10 | No management/results commands | `autorun.rs:12-15` only has `run` function, no subcommands | HIGH | J, K |
| 11 | No pre-flight checks | `run_with_dir()` at `autorun.rs:17-28` goes straight to batch parse | MEDIUM | G |
| 12 | No cancel/abort support | No mechanism to stop running workers or abort batch | HIGH | J |
| 13 | Generic task prompt | Worker prompt at `worker.rs:243-244` is `"Execute autorun task {id}"` | HIGH | A |
| 14 | No structured results query | `report_results()` at `autorun.rs:69-96` prints to stdout only | MEDIUM | J |
| 15 | No autorun event routing | `routing.rs:13-63` has no `batch_*` or `worker_*` event types | HIGH | E |
| 16 | No autorun config section | `ParallelWorkConfig` at `config.rs:17-28` has 4 sections, no `autorun` | MEDIUM | F |
| 17 | No graceful shutdown | `Orchestrator::execute()` at `orchestrator.rs:91-131` has no signal handler | HIGH | H |
| 18 | Non-unique tmux names | `tmux_prefix: "codeflow-worker"` at `orchestrator.rs:209` | MEDIUM | B |
| 19 | No --batch CLI flag | Hardcoded path at `autorun.rs:18-22` | LOW | G |
| 20 | No worktree limit retry | Semaphore at `orchestrator.rs:188` not bounded by max_concurrent | MEDIUM | B |
