---
title: "Parallel Work Configuration Specification"
type: analysis
status: active
author: cf-documentation
created_at: "2026-03-21"
updated_at: "2026-03-21"
parent: "parallel-work/README.md"
---

# Parallel Work Configuration Specification

[← Back to Overview](README.md)

> **Status:** Design specification. The config file `.codeflow/config/parallel-work/parallel-work-config.json` does not yet exist — it is to be created by INF-TSK-023-024. Source files do not yet read this config (target: `session_start.rs`, `registry.rs`, `sync.rs`, `loro.rs`, `worker.rs`). All defaults and field descriptions reflect the target design.

## Table of Contents

- [1. File Location](#1-file-location)
- [2. Purpose](#2-purpose)
- [3. Schema](#3-schema)
- [4. Example Configuration](#4-example-configuration)
- [5. Notes](#5-notes)

---

## 1. File Location

`.codeflow/config/parallel-work/parallel-work-config.json`

---

## 2. Purpose

Single configuration source for all parallel execution settings. Read at startup or on first use by:

- `codeflow-cli/core/src/hooks/session_start.rs` — worktree setup, max_concurrent enforcement
- `codeflow-cli/core/src/worktree/registry.rs` — `locked_register_with_limit()` reads `worktree.max_concurrent`
- `codeflow-cli/core/src/coordination/sync.rs` — sync daemon reads `sync.interval_secs`, `sync.auto_start`
- `codeflow-cli/core/src/coordination/loro.rs` — claim TTL reads `claims.ttl_secs`, `claims.capture_events`
- `codeflow-cli/core/src/autorun/worker.rs` — workers read `claims.default_scope_policy`, `worktree.max_concurrent` (target: `WorkerConfig` gains `scope_policy` field via INF-TSK-023-024)

All values are configurable once INF-TSK-023-024 is implemented. Currently, several source files still use hardcoded defaults.

---

## 3. Schema

### 3.1 worktree

Controls when and how git worktrees are created for session isolation.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | string | `"autorun"` | When to create worktrees. `"autorun"` — only for autorun sessions. `"always"` — every session including interactive. `"disabled"` — never create worktrees. |
| `max_concurrent` | integer | `3` | Maximum parallel worktrees. Enforced by `locked_register_with_limit()` in `worktree/registry.rs`. Attempts to create a worktree beyond this limit produce an error. |
| `base_dir` | string | `".git-worktrees"` | Directory for worktree creation. Relative to project root. Must be gitignored (`.gitignore` line for `.git-worktrees/` is required). |

### 3.2 sync

Controls the CRDT sync daemon that propagates state between worktrees (same-machine) and machines (multi-machine).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `interval_secs` | integer | `5` (target; current code default: 30s `DEFAULT_SYNC_INTERVAL_SECS`, to be changed by INF-TSK-023-026) | Sync daemon cycle interval in seconds. At each interval, the daemon exports new Loro deltas, pushes to its per-peer git ref, and imports all peer refs. Balances responsiveness (faster = sooner conflict detection) vs I/O overhead (slower = fewer filesystem operations). |
| `auto_start` | boolean | `true` | Auto-start the sync daemon when `count_active() > 1` (more than one worktree active). Set `false` for debugging or when running the daemon manually. |

### 3.3 merge

Controls how merge conflicts are handled when workers create PRs.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `auto_rebase` | boolean | `true` | Attempt `git rebase` on merge conflicts before PR creation. When `true`, `check_merge_conflicts()` in `git/conflict.rs` triggers a rebase attempt if conflicts are detected. |
| `queue_enabled` | boolean | `true` | Use the Loro-backed FIFO merge queue (`coordination/merge_queue.rs`) for PR ordering across concurrent workers. When `true`, workers enqueue before PR creation and dequeue after merge. |
| `max_rebase_attempts` | integer | `3` | Maximum rebase retries before giving up. Between attempts, the sync daemon propagates latest state from other workers. If all attempts fail, the worker emits a `MergeConflictDetected` event and escalates to the team lead. |

### 3.4 claims

Controls the Loro CRDT claim system for parallel file access coordination.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_scope_policy` | string | `"soft"` | Default `scope_policy` for tasks that do not specify one explicitly. One of `"soft"`, `"hard"`, `"permissive"`. Note: `"permissive"` as a default is valid only if all tasks are interactive; autorun tasks always require a non-permissive policy. |
| `ttl_secs` | integer | `4200` (target; current code default: 300s in `loro.rs:31`, to be changed by INF-TSK-023-024) | Safety-net claim TTL in seconds. Claims expire after this duration regardless of session state. Must exceed the worker timeout (default 3600s) to prevent "locked out of own file" scenarios. The sync daemon's PID liveness detection is the primary crash cleanup mechanism (5–10s); TTL is the nuclear fallback. |
| `capture_events` | boolean | `true` | Persist all coordination events (`ClaimAcquired`, `ClaimConflict`, `ClaimReleased`, `ScopeExpansion`, `MergeConflictDetected`, `MergeRebaseAttempted`) to `coordination-events.jsonl` and the `coordination_event` SurrealDB table. Set `false` to disable event capture in performance-sensitive environments. |

---

## 4. Example Configuration

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
  }
}
```

---

## 5. Notes

- **No "validation" section.** Autorun validation rules (require non-empty `file_scope`, reject `permissive` for autorun tasks) are correctness constraints enforced in `validate/mod.rs` (INF-TSK-023-027). They are not configurable because they are not preferences — they are requirements for safe parallel execution.

- **No "conflict" section.** CRDT claim conflicts are handled by the claims subsystem. Git merge conflicts are handled by the merge subsystem. These are distinct failure modes with distinct handling paths.

- **No `max_expansion_files`.** Loro CRDT provides natural limits on scope expansion via other workers' claims. Artificial numeric limits would constrain the soft policy mechanism without adding safety. See [Decision #29](decisions.md#29-no-max_expansion_files-limit).

- **All values are configurable; none are hardcoded in source.** Code reads from this config at startup or on first use. Default values are defined in code only as the fallback when the config file is absent or a field is omitted.

- **Config file is optional.** If absent, all defaults apply. The system works out-of-the-box without creating this file. Users only need to create it when they want to override defaults.

---

[← Back to Overview](README.md)
