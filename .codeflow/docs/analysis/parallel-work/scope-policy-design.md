---
title: "Scope Policy Design"
type: analysis
status: active
author: cf-documentation
created_at: "2026-03-21"
updated_at: "2026-03-21"
parent: "parallel-work/README.md"
---

# Scope Policy Design

[← Back to Overview](README.md)

> **Status:** Design specification. Implementation tracked by INF-TSK-023-024 through INF-TSK-023-027. Referenced fields and behaviors describe the target design, not the current codebase state.

## Table of Contents

- [1. Purpose](#1-purpose)
- [2. Scope Policy Modes](#2-scope-policy-modes)
- [3. Determination Criteria](#3-determination-criteria)
- [4. File Scope Best Practices](#4-file-scope-best-practices)
- [5. Integration with Loro CRDT](#5-integration-with-loro-crdt)
- [6. Coordination Events](#6-coordination-events)

---

## 1. Purpose

`file_scope` and `scope_policy` work together with Loro CRDT to manage parallel worker file access. They define both the declared territory of a worker and the rules governing what happens when a worker needs to edit files outside that territory.

- **`file_scope`** — the set of file patterns a worker declares at task creation. Example: `["codeflow-cli/core/src/coordination/**"]`.
- **`scope_policy`** — the enforcement mode governing out-of-scope access. One of `soft`, `hard`, or `permissive`.

File scope is declared, not enforced by directory permissions. The CRDT claim system provides the enforcement. In the target design, a worker that edits only within its declared scope never needs to interact with the claim system during execution — all in-scope files are pre-claimed at startup via `acquire_batch()` (implemented by INF-TSK-023-027).

---

## 2. Scope Policy Modes

### 2.1 soft (default for autorun)

Workers declare `file_scope` at task creation. All `file_scope` entries are pre-claimed at startup via `acquire_batch()`. Edits within scope proceed immediately with no coordination overhead.

For out-of-scope edits, the worker attempts a Loro claim:

- **No conflict (file unclaimed):** Claim acquired. Edit allowed. `ScopeExpansion` event logged.
- **Conflict (file held by another worker):** BLOCKED (exit 2). `ClaimConflict` event recorded. Full context captured: requesting session, task, holding session, holding task, fencing token.

Natural limit: other workers' claims prevent runaway expansion. No artificial numeric cap (see [Decision #29](decisions.md#29-no-max_expansion_files-limit)).

**Best for:** Most parallel work — safe with coordinated flexibility.

### 2.2 hard

Workers declare `file_scope` at task creation. All `file_scope` entries are pre-claimed at startup.

Out-of-scope edits are blocked immediately (exit 2) without attempting a claim. The worker does not interact with the Loro claim system for out-of-scope files.

**Best for:** Critical paths where strict isolation is required:

- Database schema migrations (concurrent schema modifications must not overlap)
- Security-sensitive files (no expansion into auth, crypto, or permission code)
- Single-file hotfixes (tight scope, no expansion needed or desired)

**Warning:** If two hard-mode tasks in the same batch declare overlapping `file_scope` patterns, they will both try to pre-claim the same files. The second worker's `acquire_batch()` will produce a `ClaimConflict` and the task will be queued for a later batch. Avoid overlapping `file_scope` between hard-mode sibling tasks.

### 2.3 permissive

No scope checking. No claim acquisition. Workers edit freely.

**Best for:** Interactive (human-directed) sessions where parallel coordination is not needed.

**FORBIDDEN for autorun tasks.** Validated in code at `validate/mod.rs` — a batch containing any autorun-eligible task with `scope_policy = "permissive"` produces a validation error before any workers spawn. This is a correctness constraint, not a configurable preference (see [Decision #30](decisions.md#30-validation-in-code-not-config)).

---

## 3. Determination Criteria

`file_scope` and `scope_policy` are set at task creation time (see [Decision #31](decisions.md#31-file-scope-and-scope-policy-determination)).

### 3.1 Who Sets Them

| Task Origin | Who Sets scope | When |
|-------------|---------------|------|
| Planned task | cf-planning | During WS-PLAN task decomposition |
| Adhoc task | cf-knowledge-layer | During PF4-TSK-01 (ensure-work-registered) |

### 3.2 Scope Policy Selection

When cf-planning decomposes tasks, use this table:

| Condition | Recommended scope_policy | Rationale |
|-----------|-------------------------|-----------|
| Standard feature/fix work | `soft` | Safe expansion with CRDT coordination |
| Database schema changes | `hard` | Strict isolation prevents concurrent schema modifications |
| Security-sensitive files | `hard` | No expansion into security-critical paths |
| Configuration changes | `soft` | Usually isolated but may need related files |
| Documentation | `soft` | Minimal conflict risk, expansion useful |
| Test implementation | `soft` | Tests may need fixtures in other directories |
| Single-file hotfix | `hard` | Tight scope, no expansion needed |
| Interactive session | `permissive` | Human-directed; no parallel coordination needed |

### 3.3 Default Behavior

- If `scope_policy` is not specified in the task definition, it defaults to `"soft"` (from `parallel-work-config.json` field `claims.default_scope_policy`).
- If `file_scope` is empty for an autorun task, `validate/mod.rs` produces a validation error. All autorun tasks must have a non-empty `file_scope`.

---

## 4. File Scope Best Practices

1. **Use directory-level glob patterns for features:**
   `"codeflow-cli/core/src/coordination/**"` covers all files in the coordination module including subdirectories.

2. **Use file-level patterns for hotfixes:**
   `"codeflow-cli/core/src/hooks/pre_tool_use.rs"` limits scope to a single file.

3. **Include test files alongside source:**
   `"codeflow-cli/core/src/hooks/**"` covers both `pre_tool_use.rs` and any `tests/` subdirectory under hooks.

4. **Avoid overlapping patterns between sibling tasks in the same batch:**
   If Task A declares `"src/db/**"` and Task B declares `"src/db/schema.rs"`, Task B's pre-claim will conflict with Task A's. Redesign scopes to be non-overlapping or make the tasks sequentially dependent.

5. **If two tasks need the same file, make them sequentially dependent — not parallel:**
   Tasks that share a file cannot safely run in parallel regardless of scope policy. Use task dependencies to sequence them.

6. **Prefer precise patterns over catch-all patterns:**
   `"codeflow-cli/core/src/coordination/**"` is better than `"codeflow-cli/**"`. Overly broad scopes cause unnecessary conflicts with unrelated tasks.

---

## 5. Integration with Loro CRDT

Claims are stored in the Loro Map CRDT (`state.loro`). Each claim entry uses the file path as the map key.

```text
file_scope defines worker's DECLARED territory
    |
    v
At startup: worker pre-claims ALL file_scope entries via acquire_batch()
    -> Each file -> LoroMap.insert(file_path, ClaimValue { session, task, worktree, token, ttl, ... })
    -- Note: target Claim struct; current struct has owner/token/ttl_secs/acquired_at only
    -- task_id and worktree_id fields added by INF-TSK-023-027
    -> All pre-claims use the same fencing token batch
    -> ClaimAcquired events logged for each successful pre-claim
    |
    v
During execution:
    |
    +-- Edit within file_scope?
    |       -> Already claimed at startup
    |       -> Proceed immediately (no claim system interaction)
    |
    +-- Edit outside file_scope? (soft mode)
    |       |
    |       v
    |       Attempt Loro claim: LoroMap.insert(file_path, ClaimValue)
    |       Sync + merge with other workers' state
    |           |
    |           +-- No conflict (file unclaimed)?
    |           |       -> Claim acquired
    |           |       -> Edit allowed
    |           |       -> ScopeExpansion event logged
    |           |
    |           +-- Conflict (file held by another worker)?
    |                   -> BLOCKED (exit 2)
    |                   -> ClaimConflict event recorded with full context
    |
    +-- Edit outside file_scope? (hard mode)
            -> BLOCKED immediately (exit 2)
            -> No claim attempt
    |
    v
On completion: claims::release_all(session_id)
    -> Removes all entries for this session from LoroMap
    -> ClaimReleased event logged (file_count, release_type=normal)
```

**TTL:** Claims carry a `ttl_expires_at` timestamp (target default: `now + 4200s`; current code default: 300s in `loro.rs:31`, to be changed by INF-TSK-023-024). If the sync daemon detects a dead session (PID liveness check fails), it releases that session's claims within 5–10 seconds. TTL is the fallback for cases where the daemon itself has crashed.

**Fencing tokens:** Each claim entry carries a monotonically increasing `token` (`u64`). When two workers concurrently claim the same file, Loro's last-writer-wins semantics determine which claim survives. The loser detects the conflict by comparing its token against the current value in the merged state.

---

## 6. Coordination Events

All coordination activity is recorded in `coordination-events.jsonl` (append-only) and the `coordination_event` SurrealDB table. `capture_events` can be disabled in `parallel-work-config.json` for performance-sensitive environments, but defaults to `true`.

| Event | When | Key Data |
|-------|------|----------|
| `ClaimAcquired` | Successful pre-claim or expansion claim | file, session, task, worktree, token, was_expansion |
| `ClaimConflict` | Blocked by another holder | file, requesting session/task, holding session/task, token, scope_policy |
| `ClaimReleased` | Release on session end | session, task, file_count, release_type (normal/crash/timeout) |
| `ScopeExpansion` | Soft mode allowed out-of-scope claim | session, task, original_scope patterns, expanded_file, token |
| `MergeConflictDetected` | Git merge conflict found before PR creation | session, task, conflicting files |
| `MergeRebaseAttempted` | Rebase retry initiated | session, task, attempt_number, max_attempts (from `merge.max_rebase_attempts`, default 3) |

These events enable operational intelligence: identifying conflict hotspots, validating scope decomposition quality, and analyzing team parallelism patterns over time.

---

[← Back to Overview](README.md)
