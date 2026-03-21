---
title: "Autorun Integration"
type: analysis
status: active
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-21"
parent: "parallel-work/README.md"
---

# Autorun Integration

[← Back to Overview](README.md)

## Table of Contents

- [1. Current State](#1-current-state)
- [2. Worktree-Integrated Autorun](#2-worktree-integrated-autorun)
- [3. Implementation Changes](#3-implementation-changes)
- [4. Worker Lifecycle](#4-worker-lifecycle)
- [5. Parallel Work Configuration](#5-parallel-work-configuration)
- [6. Scope Policy in Workers](#6-scope-policy-in-workers)

---

## 1. Current State

The autorun orchestrator in the Rust CLI (`codeflow-cli/core/src/autorun/worker.rs`) invokes Claude Code via the `WorktreeProvider` trait. Each worker receives its own worktree path. Workers share the project root only for symlinked shared state (db, ledger, coordination, logs).

```rust
// worker.rs — Rust CLI (current implementation)
let result = self.provider.invoke(InvokeConfig {
    worktree_path: worktree.path.clone(),   // Per-worker isolation
    prompt: format!("Execute autorun task {}", cfg.task_id),
    session_id: cfg.session_id.clone(),
    task_id: cfg.task_id.clone(),
    auto_merge: cfg.auto_merge,
    target: cfg.target.clone(),
    tmux_session: tmux_name,
}).await?;
```

---

## 2. Worktree-Integrated Autorun

Each autorun worker needs its own worktree for filesystem isolation:

```text
Autorun Orchestrator
    |
    +-- Worker 1: worktree-{SID-1}
    |   WorkDir: .git-worktrees/worktree-{SID-1}
    |   Branch: feat/task-A (created at PF3 inside worktree)
    |   Task: INF-TSK-008-001
    |
    +-- Worker 2: worktree-{SID-2}
    |   WorkDir: .git-worktrees/worktree-{SID-2}
    |   Branch: fix/task-B (created at PF3 inside worktree)
    |   Task: INF-TSK-008-002
    |
    +-- Worker 3: worktree-{SID-3}
        WorkDir: .git-worktrees/worktree-{SID-3}
        Branch: feat/task-C (created at PF3 inside worktree)
        Task: INF-TSK-008-003
```

---

## 3. Implementation Changes

| Component | Change | File |
|-----------|--------|------|
| `InvokeConfig` | `worktree_path` field set per-worker | `autorun/worker.rs` |
| `WorktreeProvider` trait | `setup_detached()` before invoke, `cleanup()` on completion | `autorun/worker.rs` |
| `TmuxWorker::run()` cleanup | After worker completes (success or failure), cleanup worktree | `autorun/worker.rs` |
| Batch orchestrator | Validate batch size against `max_concurrent` from `parallel-work-config.json` | `autorun/orchestrator.rs` |
| Claims coordination | Workers call `claims::acquire_batch()` on `file_scope` entries before starting. `ClaimConflict` = queue task for later. | `autorun/worker.rs` + `coordination/claims.rs` |
| Config integration | Workers read `parallel-work-config.json` for `max_concurrent`, `default_scope_policy`, `ttl_secs` | `autorun/worker.rs` |
| Scope policy | Each task carries a `scope_policy` field. Workers enforce it during execution. | `autorun/worker.rs` |
| Coordination events | Workers emit `ClaimAcquired` and `ClaimReleased` events via `LoroCoordinator` | `autorun/worker.rs` + `coordination/loro.rs` |
| Autorun validation | Reject `permissive` scope_policy for autorun tasks at config parse time | `validate/mod.rs` |

---

## 4. Worker Lifecycle

```text
Worker spawned by orchestrator (autorun/orchestrator.rs)
    |
    v
1. Generate worker session ID (ULID format)
2. Read scope_policy and file_scope from task definition
3. WorktreeManager::setup_detached("worktree-{SID}")
    -> Creates worktree with shared/local state
    -> Shared (symlinked): db/, ledger/, coordination/, logs/, registry/, backups/
    -> Local (per-worktree): runtime/, session/, sentinels/
    |
    v
4. claims::acquire_batch(coordinator, file_scope, session_id)
    -> Pre-claim all declared file_scope entries
    -> ClaimConflict on any entry -> queue task for later batch
    -> ClaimAcquired events logged to coordination-events.jsonl
    |
    v
5. Write codeflow-env.sh in worktree
    -> Contains CODEFLOW_SESSION_ID, CF_PROJECT_ROOT, CODEFLOW_WORKTREE_PATH
    |
    v
6. provider.invoke(InvokeConfig { worktree_path, ... })
    -> Claude session starts inside worktree
    -> SessionStart hook detects existing worktree (skip creation)
    -> PathFlow runs PF1-PF7 inside worktree
    |
    v
7. Worker completes (success/failure/timeout)
    |
    v
8. claims::release_all(session_id)
    -> ClaimReleased event logged
    |
    v
9. WorktreeManager::cleanup("worktree-{SID}")
    -> git worktree remove
    -> Deregister from worktrees.yaml
```

**Validation invariants (enforced in `validate/mod.rs`):**

- `file_scope` MUST be non-empty for autorun tasks — validation error if empty
- `scope_policy` MUST NOT be `permissive` for autorun tasks — validation error
- Batch size validated against `max_concurrent` from `parallel-work-config.json` before any workers spawn

**Note:** Step 6 requires the SessionStart hook to detect that a worktree already exists (env file present, worktree path set) and skip worktree creation. This "pre-created worktree" mode is needed for the autorun integration where the orchestrator creates the worktree before invoking Claude.

---

## 5. Parallel Work Configuration

Workers read configuration from `.codeflow/config/parallel-work/parallel-work-config.json` at startup. The orchestrator reads it before spawning workers to validate batch constraints.

Key fields used by the autorun system:

| Field | Location in Config | Purpose |
|-------|-------------------|---------|
| `max_concurrent` | `worktree.max_concurrent` | Maximum parallel workers (default: 3) |
| `default_scope_policy` | `claims.default_scope_policy` | Scope policy when task doesn't specify one (default: "soft") |
| `ttl_secs` | `claims.ttl_secs` | Claim TTL safety net (default: 4200s) |
| `capture_events` | `claims.capture_events` | Whether to persist coordination events (default: true) |
| `interval_secs` | `sync.interval_secs` | Sync daemon interval (default: 30s) |

See [parallel-work-config-spec.md](parallel-work-config-spec.md) for the full schema.

---

## 6. Scope Policy in Workers

Each autorun task carries a `scope_policy` field set at task creation time (by cf-planning for planned tasks, by cf-knowledge-layer for adhoc tasks). Workers enforce this policy throughout execution.

| scope_policy | Worker Behavior |
|-------------|-----------------|
| `soft` | Pre-claim `file_scope` at startup. Out-of-scope edits attempt a Loro claim. `ClaimConflict` blocks the edit if held. `ScopeExpansion` logged if unclaimed. |
| `hard` | Pre-claim `file_scope` at startup. Out-of-scope edits blocked immediately (exit 2) — no claim attempt. |
| `permissive` | FORBIDDEN for autorun. Rejected at config parse time in `validate/mod.rs`. |

Default: `soft`. Workers that do not have a `scope_policy` in their task definition inherit `default_scope_policy` from `parallel-work-config.json`.

---

[← Back to Overview](README.md)
