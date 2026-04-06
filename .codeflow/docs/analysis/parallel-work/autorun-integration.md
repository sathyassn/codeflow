---
title: "Autorun Integration Branch & Serialized Merge"
type: analysis
status: active
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-04-05"
parent: "parallel-work/README.md"
---

# Autorun Integration Branch & Serialized Merge

[<- Back to Overview](README.md)

## Table of Contents

- [1. Problem Statement](#1-problem-statement)
- [2. Solution Architecture](#2-solution-architecture)
- [3. Configuration Design](#3-configuration-design)
- [4. Integration Branch Lifecycle](#4-integration-branch-lifecycle)
- [5. Branch Prefix Registration](#5-branch-prefix-registration)
- [6. Worker Claude Session Changes](#6-worker-claude-session-changes)
- [7. Serialized Merge (Rust Worker Layer)](#7-serialized-merge-rust-worker-layer)
- [8. Orchestrator Post-Processing](#8-orchestrator-post-processing)
- [9. Rust Implementation Details](#9-rust-implementation-details)
- [10. Failure Handling](#10-failure-handling)
- [11. Parallel Batch Safety](#11-parallel-batch-safety)
- [12. Testing Requirements](#12-testing-requirements)
- [13. File Change Inventory](#13-file-change-inventory)
- [14. Backward Compatibility](#14-backward-compatibility)
- [15. Assumptions](#15-assumptions)

---

## 1. Problem Statement

The current autorun system has four structural gaps when running parallel worker batches:

1. **Workers PR directly to main.** Each worker creates a PR targeting `main`. If some workers fail, partial state is merged into `main` -- there is no isolation boundary for the batch. Successfully-merged workers cannot be rolled back without reverting individual PRs.

2. **Epic markdown conflicts between parallel workers.** When workers complete, the `complete-work` operation in cf-knowledge-layer updates the epic markdown file (task status row, epic rollup). Multiple workers finishing concurrently edit the same epic file, producing merge conflicts that block the later workers' PRs.

3. **Reports remain uncommitted.** The orchestrator generates batch reports (`project-management/tracking/autorun/`) after workers complete, but these exist only on the local filesystem. They are never committed to any branch, so they are lost if the machine is reset.

4. **No final consolidated PR.** After all workers complete and their individual PRs merge, there is no single PR from the batch's combined work to `main` for human review. The changes trickle into `main` one worker at a time.

**Root cause:** The batch lacks a staging area (integration branch) that collects worker output before promoting to `main`.

---

## 2. Solution Architecture

```text
origin/main
    |
    +-- autorun/{batch-name}-{8char} (integration branch)
    |       |
    |       +-- feat/task-A  (worker 1 PR -> integration)
    |       +-- fix/task-B   (worker 2 PR -> integration)
    |       +-- feat/task-C  (worker 3 PR -> integration)
    |       |
    |       +-- [orchestrator commits: epic updates, reports]
    |       |
    |       +-- Final PR: integration -> main (human review)
    |
    v
origin/main (after merge)
```

Four coordinated changes:

| Change | Component | Purpose |
|--------|-----------|---------|
| Integration branch | Orchestrator | Isolates batch work from `main` until all workers complete |
| Serialized merge | Worker + CRDT queue | Workers enqueue in the Loro merge queue; the Rust worker layer merges PRs one at a time into the integration branch, rebasing as needed |
| Orchestrator epic update | Orchestrator post-batch | Single writer updates epic markdown after all workers are done, eliminating concurrent edit conflicts |
| Final PR | Orchestrator post-batch | Creates a single PR from the integration branch to `main` for human review |

---

## 3. Configuration Design

### 3.1 Batch File Schema (YAML)

Current `BatchFile` struct fields:

```yaml
name: "my-batch"
max_workers: 3
auto_merge: true        # currently: bool
target: "main"          # currently: String
tasks:
  - id: "INF-TSK-008-001"
    depends_on: []
    file_scope: ["src/foo.rs"]
    scope_policy: "soft"
```

**New/changed fields:**

| Field | Current Type | New Type | Default | Description |
|-------|-------------|----------|---------|-------------|
| `auto_merge` | `bool` | `Option<bool>` | `None` | `None` triggers inference (see 3.2). Explicit `true`/`false` overrides inference. |
| `final_pr` | (new) | `Option<bool>` | `None` | Whether to create a final PR from integration to `final_pr_base`. `None` triggers inference (see 3.2). |
| `final_pr_base` | (new) | `String` | `""` (empty) | Base branch for the final PR. Empty defaults to `"main"`. |
| `target` | `String` | `String` | `""` (empty) | Target/integration branch. Empty triggers auto-generation (see 3.2). |

The `name`, `max_workers`, and `tasks` fields are unchanged.

### 3.2 Default Resolution Chain

When optional fields are omitted or empty, they resolve via this deterministic chain:

**`target` resolution:**

```text
target field in YAML
    |
    +-- non-empty string --> use as-is (user-provided branch)
    |
    +-- empty / omitted --> "autorun/{name}-{session_id_last_8_chars}"
                            where name = batch name, session_id_last_8_chars = last 8 chars of the batch session ID
```

The auto-generated name uses the batch-level session ID (not a worker session ID) to ensure uniqueness across batch runs with the same name. The `autorun/` prefix is a new branch namespace.

**`auto_merge` resolution:**

```text
auto_merge field in YAML
    |
    +-- Some(true)  --> true (explicit)
    +-- Some(false) --> false (explicit)
    |
    +-- None / omitted --> inferred:
            is_protected_branch(resolved_target)?
                YES --> false (protected branches cannot be auto-merged)
                NO  --> true  (non-protected targets get auto-merge)
```

Protected branch check uses `enforcement-policy.json` `merge_protection.protected_branches` (main, master, release/*, production).

**`final_pr` resolution:**

```text
final_pr field in YAML
    |
    +-- Some(true)  --> true (explicit)
    +-- Some(false) --> false (explicit)
    |
    +-- None / omitted --> inferred:
            resolved_target is "main" or "master"?
                YES --> false (already targeting main; no second PR)
                NO  --> true  (integration branch needs a PR to main)
```

**`final_pr_base` resolution:**

```text
final_pr_base field in YAML
    |
    +-- non-empty string --> use as-is
    |
    +-- empty / omitted --> "main"
```

### 3.3 Global Config Additions (parallel-work-config.json)

Three new fields in the `merge` and `autorun` sections:

```json
{
  "merge": {
    "auto_rebase": true,
    "queue_enabled": true,
    "max_rebase_attempts": 3,
    "queue_enforcing": true,
    "queue_timeout_secs": 600
  },
  "autorun": {
    "worker_timeout_secs": 3600,
    "blocked_behavior": "skip_and_continue",
    "report_dir": "project-management/tracking/autorun",
    "max_concurrent_batches": 5,
    "heartbeat_interval_secs": 30,
    "stale_threshold_secs": 90,
    "epic_update": "orchestrator"
  }
}
```

| Field | Section | Type | Default | Description |
|-------|---------|------|---------|-------------|
| `queue_enforcing` | `merge` | `bool` | `true` | When true, the Rust worker layer enforces merge queue ordering (wait for position 0 before merging). When false, the queue is advisory only (current behavior). |
| `queue_timeout_secs` | `merge` | `u64` | `600` | Maximum seconds a worker waits for position 0 in the merge queue before timing out and marking failed. |
| `epic_update` | `autorun` | `String` | `"orchestrator"` | Who updates epic markdown: `"orchestrator"` (post-batch, single writer) or `"worker"` (each worker, current behavior). |

### 3.4 New Environment Variables

Three new env vars propagated to workers via `autorun-worker-env.sh`:

| Variable | Value Source | Purpose |
|----------|-------------|---------|
| `AUTORUN_TARGET` | Resolved `target` from batch file | The branch workers create PRs against. Used by Claude at PF3 (branch creation) and PF6-TSK-05 (PR base). |
| `AUTORUN_AUTO_MERGE` | Resolved `auto_merge` from batch file | `"true"` or `"false"`. Controls whether Claude attempts merge at PF6-TSK-07. |
| `AUTORUN_EPIC_UPDATE` | `autorun.epic_update` from config | `"orchestrator"` or `"worker"`. Controls whether cf-knowledge-layer skips epic markdown writes during `complete-work`. |

These are also written to `active-task.json` so hooks can read them:

| Field | Type | In `ActiveTask` |
|-------|------|----------------|
| `target_branch` | `Option<String>` | New field |
| `auto_merge` | `Option<bool>` | New field |
| `epic_update` | `Option<String>` | New field |

---

## 4. Integration Branch Lifecycle

### 4.1 Naming Convention

Auto-generated: `autorun/{batch-name}-{last-8-chars-of-batch-session-id}`

Examples:
- Batch "infra-tasks", session `ses-01knfj4bynh7fh754dhbvrm3qw` --> `autorun/infra-tasks-vrm3qw00`
- Batch "fix-batch", session `ses-01knx12345678abcd` --> `autorun/fix-batch-78abcd00`

User-provided: any valid branch name (e.g., `integration/sprint-42`).

### 4.2 Orchestrator Branch Creation

The orchestrator creates the integration branch before spawning workers:

```text
Orchestrator start
    |
    v
1. Resolve target branch name (Section 3.2)
2. git fetch origin
3. Does origin/{target} exist?
    |
    +-- YES: Does it have open PRs targeting it?
    |   |
    |   +-- YES (auto-generated target): ERROR. Another batch is using this branch.
    |   +-- YES (user-provided target): WARN. User explicitly chose this branch.
    |   +-- NO open PRs: Delete and recreate from origin/{final_pr_base}
    |
    +-- NO: Create from origin/{final_pr_base}
        git checkout -b {target} origin/{final_pr_base}
        git push -u origin {target}
    |
    v
4. Record target in autorun session state
5. Spawn workers with AUTORUN_TARGET={target}
```

### 4.3 Collision Handling

| Scenario | Auto-Generated Target | User-Provided Target |
|----------|----------------------|---------------------|
| Branch exists, has open PRs | ERROR: abort batch | WARN: proceed (user's responsibility) |
| Branch exists, no open PRs | Delete + recreate from final_pr_base | WARN: reuse as-is |
| Branch does not exist | Create from final_pr_base | Create from final_pr_base |

---

## 5. Branch Prefix Registration

The `autorun/` prefix must be registered in five locations to avoid enforcement violations:

| Location | File | Change | Current Value |
|----------|------|--------|---------------|
| `branch_types` array | `.codeflow/config/enforcement/enforcement-policy.json` | Add `"autorun"` | 20 entries, last is `"refine"` |
| `branch_prefixes` array | `.codeflow/config/enforcement/enforcement-policy.json` | Add `"autorun/"` | 20 entries, last is `"refine/"` |
| `default_policy()` | `codeflow-cli/core/src/autorun/config.rs` | No change needed -- config.rs does not hardcode branch types | N/A |
| `PREFIX_MAP` in `pipeline.rs` | `codeflow-cli/core/src/hooks/pipeline.rs` | Add `("autorun/", "AUTORUN")` | 10 entries, last is `("hotfix/", "HTFX")` |
| cf-git-operations branch access | `.claude/agents/cf-git-operations.md` | Add `autorun/*` to allowed branch prefixes | Currently lists feat/*, fix/*, plan/*, etc. |
| CLAUDE.md feature branch list | `.claude/CLAUDE.md` Section 7 | Add `autorun/*` to the feature branches list | Currently lists feat/*, fix/*, plan/*, docs/*, refactor/*, test/*, chore/*, cicd/*, spike/*, hotfix/* |

**Not changed:**
- `commit_types` in enforcement-policy.json -- `autorun` is a branch prefix, not a commit type. Commits on autorun branches use the standard types (feat, fix, etc.).
- CI workflow triggers -- CI runs on PR events, not branch name patterns. No change needed.

---

## 6. Worker Claude Session Changes

### 6.1 Branch Creation (PF3-CLASSIFY)

Currently, cf-git-operations creates the feature branch off `origin/main`. In autorun mode, it must branch off the integration branch instead.

**Change:** When `AUTORUN_TARGET` is set in the environment (or in `active-task.json`), cf-git-operations reads it and uses it as the base:

```text
PF3-TSK-03 (create feature branch):
    base = env(AUTORUN_TARGET) or active-task.json.target_branch
    if base is set and non-empty:
        git checkout -b {prefix}/{task-name} origin/{base}
    else:
        git checkout -b {prefix}/{task-name} origin/main  (current behavior)
```

This change is in the agent definition instructions (cf-git-operations.md), not in Rust code. The Rust CLI does not create feature branches -- Claude does.

### 6.2 PR Creation (PF6-TSK-05)

Currently, `gh pr create` uses `--base main`. In autorun mode, it must target the integration branch.

**Change:** cf-git-operations reads `AUTORUN_TARGET` and passes it to `--base`:

```text
PF6-TSK-05 (create PR):
    base = env(AUTORUN_TARGET) or active-task.json.target_branch
    gh pr create --base {base} --head {feature-branch} ...
```

### 6.3 PR Merge (PF6-TSK-07)

Currently, when `auto_merge=true` and the target is not protected, Claude runs `gh pr merge --merge --delete-branch`. With serialized merge, the merge is handled by the Rust worker layer AFTER Claude exits, not by Claude itself.

**Change:**

| `auto_merge` | Behavior Inside Claude (PF6-TSK-07) |
|-------------|--------------------------------------|
| `true` | Do NOT merge. PR is created but left open. The Rust worker layer handles serialized merge after Claude exits. Claude reports `pr_created` and proceeds to PF7. |
| `false` | PR left open for human review (current behavior). |

The key behavioral change: **Claude never runs `gh pr merge` in autorun mode when `auto_merge=true`.** The merge is deferred to the Rust worker layer to enforce serialization via the merge queue.

### 6.4 Epic Update (complete-work)

Currently, `complete-work` in cf-knowledge-layer updates the epic markdown (step 6: update task row status) and performs epic rollup (step 11: check if all sibling tasks complete).

**Change:** When `AUTORUN_EPIC_UPDATE=orchestrator`:

- **Skip step 6** (task row status update in epic markdown) -- the orchestrator will do this post-batch.
- **Skip step 11** (epic rollup) -- the orchestrator will do this post-batch.
- All other `complete-work` steps proceed normally (DB update, ledger event, task markdown sync, etc.).

This eliminates the concurrent-write conflict on the epic markdown file.

---

## 7. Serialized Merge (Rust Worker Layer)

After Claude exits successfully (exit code 0) and `auto_merge=true`, the Rust worker layer in `worker.rs` performs a serialized merge using the CRDT merge queue.

### 7.1 Merge Sequence

```text
Claude exits (exit code 0, PR created)
    |
    v
1. Extract PR number from InvokeResult
2. Enqueue in Loro CRDT merge queue:
   locked_enqueue(state_path, MergeQueueEntry {
       session_id: worker_session_id,
       task_id,
       branch: feature_branch,
       pr_ready_at: now_iso8601(),
       target_branch: AUTORUN_TARGET,  // NEW field
   })
    |
    v
3. Wait for position 0 in queue (scoped by target_branch):
   loop {
       pos = locked_position_for_target(state_path, worker_sid, target)?;
       if pos == Some(0) { break; }
       sleep(5 seconds);
       if elapsed > queue_timeout_secs { timeout error; break; }
   }
    |
    v
4. git fetch origin
5. git rebase origin/{AUTORUN_TARGET}
    |
    +-- Success: git push --force-with-lease
    |           gh pr merge {pr_number} --merge --delete-branch
    |           Dequeue from merge queue
    |
    +-- Conflict: retry rebase up to max_rebase_attempts
    |   |
    |   +-- All retries fail: mark worker failed, dequeue, log conflict files
    |
    v
6. Worker complete
```

### 7.2 Queue Timeout

If the worker waits longer than `queue_timeout_secs` (default 600s) for position 0:

1. Call `remove_by_session(state_path, worker_sid)` to remove the worker's entry from the queue.
2. Mark the worker as `failed` with reason `"merge_queue_timeout"`.
3. The PR remains open (not merged) for human inspection.

### 7.3 auto_merge=false Path

When `auto_merge=false`, the worker skips the entire merge queue flow. The PR is created and left open. No enqueue, no wait, no merge.

---

## 8. Orchestrator Post-Processing

After all workers complete (success or failure), the orchestrator runs three post-processing steps.

### 8.1 Epic File Update

The orchestrator is the single writer for epic markdown updates, eliminating concurrent conflicts.

```text
1. Create a temporary worktree on the target (integration) branch:
   git worktree add /tmp/epic-update-{batch_id} {target}

2. For each completed task:
   a. Find the epic markdown file (from task metadata)
   b. Find the task row in the epic's task table
   c. Replace the status cell from "in_progress" to "completed"

3. Check epic rollup:
   If ALL sibling tasks in the epic are now "completed":
       Update epic status to "completed"

4. git add + git commit + git push
5. Remove temp worktree: git worktree remove /tmp/epic-update-{batch_id}
```

**Markdown table parsing:** Epic task tables may have 4 or 5 columns:

| Format | Columns | Example |
|--------|---------|---------|
| 4-col | ID \| Title \| Status \| Notes | `\| INF-TSK-008-001 \| Add feature \| in_progress \| \|` |
| 5-col | ID \| Title \| Assignee \| Status \| Notes | `\| INF-TSK-008-001 \| Add feature \| cf-dev \| in_progress \| \|` |

The `update_task_row_status()` function must handle both formats by scanning for the task ID in the first column, then finding the status cell by matching known status values (`in_progress`, `pending`, `blocked`, etc.) rather than relying on a fixed column index.

### 8.2 Report Commit

The orchestrator generates a batch completion report and commits it to the integration branch:

```text
1. Generate report at {report_dir}/{batch_name}-{timestamp}.md
2. Commit to integration branch:
   git add {report_path}
   git commit -m "chore: add autorun batch report for {batch_name}"
   git push origin {target}
```

### 8.3 Final PR

If `final_pr=true` (resolved per Section 3.2) AND `completed_count > 0`:

```text
gh pr create \
    --base {final_pr_base} \
    --head {target} \
    --title "autorun: {batch_name} ({completed}/{total} tasks)" \
    --body "## Summary\n- Completed: {completed}\n- Failed: {failed}\n- Skipped: {skipped}\n\n## Tasks\n{task_summary_table}\n\n## Report\n{report_link}"
```

If `completed_count == 0`, skip the final PR (nothing to merge).

Record the final PR URL in the autorun session state for reporting.

---

## 9. Rust Implementation Details

### 9.1 batch.rs Changes

**File:** `codeflow-cli/core/src/autorun/batch.rs`

```rust
// BatchFile struct changes:
pub struct BatchFile {
    // ... existing fields unchanged ...
    #[serde(default)]
    pub auto_merge: Option<bool>,   // WAS: bool
    #[serde(default)]
    pub final_pr: Option<bool>,     // NEW
    #[serde(default)]
    pub final_pr_base: String,      // NEW
}

// ParsedBatch struct changes:
pub struct ParsedBatch {
    // ... existing fields unchanged ...
    pub auto_merge: bool,           // resolved from Option<bool>
    pub final_pr: bool,             // resolved from Option<bool>
    pub final_pr_base: String,      // resolved, defaults to "main"
    pub target_is_auto: bool,       // true if target was auto-generated
}
```

**New functions:**

- `resolve_target(name: &str, session_id: &str) -> String` -- generates `autorun/{name}-{last_8_chars}` when target is empty.
- `resolve_auto_merge(explicit: Option<bool>, target: &str, protected: &[String]) -> bool` -- implements the inference chain.
- `resolve_final_pr(explicit: Option<bool>, target: &str) -> bool` -- implements the inference chain.
- `resolve_final_pr_base(explicit: &str) -> String` -- returns `"main"` when empty.

**Validation changes:**

- Auto-generated targets skip the `is_protected_branch()` check (they use the `autorun/` prefix which is never protected).
- `auto_merge: Option<bool>` is YAML-compatible with existing `auto_merge: true` (deserialization of `true` into `Option<bool>` yields `Some(true)`).

### 9.2 worker.rs Changes

**File:** `codeflow-cli/core/src/autorun/worker.rs`

**New async function `serialized_merge()`:**

```rust
async fn serialized_merge(
    state_path: &Path,
    worker_sid: &SessionId,
    target_branch: &str,
    pr_number: i64,
    feature_branch: &str,
    worktree_path: &Path,
    config: &MergeConfig,
) -> Result<(), AutorunError> {
    // 1. Enqueue with target_branch
    // 2. Wait for position 0 (target-scoped)
    // 3. git fetch + rebase onto target
    // 4. git push --force-with-lease
    // 5. gh pr merge --merge --delete-branch
    // 6. Dequeue
}
```

**Cleanup fix:** Pre-existing issue -- if a worker fails mid-merge (between enqueue and dequeue), its merge queue entry leaks. The worker's cleanup path must call `remove_by_session()` on all exit paths (success, failure, timeout, panic).

**InvokeConfig additions:** None needed -- `target` and `auto_merge` are already on `InvokeConfig`. The Rust worker layer reads these from `WorkerConfig`.

**WorkerConfig additions:**

```rust
pub struct WorkerConfig {
    // ... existing fields ...
    pub target: String,         // already exists
    pub auto_merge: bool,       // already exists
    // NEW:
    pub epic_update: String,    // "orchestrator" or "worker"
}
```

**active-task.json additions:**

```rust
// ActiveTask struct additions (session/active_task.rs):
pub struct ActiveTask {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,    // NEW
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_merge: Option<bool>,         // NEW
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic_update: Option<String>,      // NEW
}
```

### 9.3 epic_update.rs (NEW Module)

**File:** `codeflow-cli/core/src/autorun/epic_update.rs` (to be created)

**Functions:**

```rust
/// Update task statuses in epic markdown files after a batch completes.
///
/// For each completed task, finds its row in the epic table and replaces
/// the status cell with "completed".
pub fn update_epic_statuses(
    worktree_path: &Path,
    completed_tasks: &[(String, String)],  // (task_format_id, epic_format_id)
) -> Result<usize, AutorunError>

/// Find a task row in a markdown table and replace its status cell.
///
/// Handles both 4-column and 5-column table formats by scanning for
/// the task ID in column 0 and finding the status cell by matching
/// known status values.
fn update_task_row_status(
    content: &str,
    task_format_id: &str,
    new_status: &str,
) -> Option<String>
```

### 9.4 merge_queue.rs Changes

**File:** `codeflow-cli/core/src/coordination/merge_queue.rs`

**MergeQueueEntry addition:**

```rust
pub struct MergeQueueEntry {
    pub session_id: SessionId,
    pub task_id: String,
    pub branch: String,
    pub pr_ready_at: String,
    #[serde(default)]
    pub target_branch: String,   // NEW: scopes queue operations
}
```

**New functions:**

```rust
/// Get queue position for a session, scoped to entries with matching target_branch.
///
/// Returns the position among entries sharing the same target_branch,
/// not the global queue position.
pub fn position_for_target(
    coordinator: &LoroCoordinator,
    session_id: &SessionId,
    target_branch: &str,
) -> Result<Option<usize>, CoordinationError>

/// Dequeue the front entry for a target branch if it belongs to the session.
///
/// Scans the queue for the first entry matching target_branch, then
/// checks if it belongs to session_id.
pub fn dequeue_for_target(
    coordinator: &LoroCoordinator,
    session_id: &SessionId,
    target_branch: &str,
) -> Result<Option<MergeQueueEntry>, CoordinationError>

/// Remove all entries for a given session ID (for cleanup on failure/timeout).
pub fn remove_by_session(
    coordinator: &LoroCoordinator,
    session_id: &SessionId,
) -> Result<usize, CoordinationError>

/// Locked variants for disk-backed state:
pub fn locked_position_for_target(
    state_path: &Path,
    session_id: &SessionId,
    target_branch: &str,
) -> Result<Option<usize>, CoordinationError>

pub fn locked_dequeue_for_target(
    state_path: &Path,
    session_id: &SessionId,
    target_branch: &str,
) -> Result<Option<MergeQueueEntry>, CoordinationError>

pub fn locked_remove_by_session(
    state_path: &Path,
    session_id: &SessionId,
) -> Result<usize, CoordinationError>
```

The existing `enqueue`, `dequeue`, `position`, and `locked_*` variants remain unchanged for backward compatibility. The new `*_for_target` variants add target scoping.

### 9.5 Other Changes

**orchestrator.rs** (`codeflow-cli/core/src/autorun/orchestrator.rs`):
- Add `epic_update`, `final_pr`, `final_pr_base`, `target_is_auto` to `WorkerConfig` propagation
- Add post-batch processing: call `epic_update::update_epic_statuses()`, commit reports, create final PR
- Add integration branch creation before worker spawn loop
- Add integration branch collision detection

**config.rs** (`codeflow-cli/core/src/autorun/config.rs`):
- Add `epic_update: String` to `AutorunConfig` (default: `"orchestrator"`)
- Add `queue_enforcing: bool` to `MergeConfig` (default: `true`)
- Add `queue_timeout_secs: u64` to `MergeConfig` (default: `600`)

**pipeline.rs** (`codeflow-cli/core/src/hooks/pipeline.rs`):
- Add `("autorun/", "AUTORUN")` to `PREFIX_MAP` at line 149 (after `("hotfix/", "HTFX")`)

**models/autorun.rs** (`codeflow-cli/core/src/models/autorun.rs`):
- Add `target_branch: Option<String>` to `AutorunSession`
- Add `final_pr_url: Option<String>` to `AutorunSession`

**active_task.rs** (`codeflow-cli/core/src/session/active_task.rs`):
- Add `target_branch: Option<String>`, `auto_merge: Option<bool>`, `epic_update: Option<String>` to `ActiveTask` struct

**autorun.rs** (CLI, `codeflow-cli/cli/src/cmd/autorun.rs`):
- Add `AUTORUN_TARGET`, `AUTORUN_AUTO_MERGE`, `AUTORUN_EPIC_UPDATE` to env file generation
- Add target, auto_merge, epic_update to active-task.json writes
- Add integration branch creation call before worker spawn
- Add post-batch processing calls (epic update, report commit, final PR)
- Add `gh auth status` preflight check (for `gh pr merge` and `gh pr create`)

---

## 10. Failure Handling

### 10.1 Failed Workers

| Scenario | Merge Queue | Epic Update | Final PR |
|----------|-------------|-------------|----------|
| Worker fails (exit code != 0) | Never enqueued (merge is post-Claude) | Skipped for this task | Still created if other tasks completed |
| Worker times out | Never enqueued | Skipped | Still created if other tasks completed |
| Worker blocked (claim conflict) | Never enqueued | Skipped | Still created if other tasks completed |

Failed workers' dependents are cascade-skipped by the orchestrator's existing dependency logic.

### 10.2 Rebase Failure

If `git rebase origin/{target}` fails after the worker reaches position 0:

1. Abort rebase: `git rebase --abort`
2. Retry up to `max_rebase_attempts` (default: 3) with a fresh `git fetch` before each attempt
3. If all retries fail:
   - Call `remove_by_session()` to dequeue and unblock the next worker
   - Mark the worker as `failed` with reason `"rebase_conflict"` and list of conflict files
   - The PR remains open for human resolution

### 10.3 Queue Timeout

If the worker waits longer than `queue_timeout_secs` for position 0:

1. Call `remove_by_session()` to remove the entry from the queue
2. Mark the worker as `failed` with reason `"merge_queue_timeout"`
3. Log the queue state at timeout for debugging

### 10.4 All Workers Fail

If `completed_count == 0`:
- No epic updates (nothing to update)
- No final PR (nothing to merge)
- Report is still generated (documents failures)
- Integration branch is left as-is (no cleanup) for investigation

### 10.5 Mixed Results

If some workers succeed and some fail:
- Epic updates only for completed tasks
- Final PR includes only the successfully-merged worker branches
- Report documents both successes and failures
- Failed tasks can be retried with `codeflow autorun resume`

---

## 11. Parallel Batch Safety

### 11.1 Cross-Batch Isolation

The merge queue is scoped by `target_branch`:

```text
Batch A: target = autorun/batch-a-abc12345
Batch B: target = autorun/batch-b-def67890

Queue entries:
  [ses-a1, target=autorun/batch-a-abc12345]  -- Batch A worker 1
  [ses-b1, target=autorun/batch-b-def67890]  -- Batch B worker 1
  [ses-a2, target=autorun/batch-a-abc12345]  -- Batch A worker 2
```

`position_for_target("autorun/batch-a-abc12345")` returns 0 for ses-a1 and 1 for ses-a2.
`position_for_target("autorun/batch-b-def67890")` returns 0 for ses-b1.

Batches A and B do not block each other.

### 11.2 Claims System

The existing CRDT claims system prevents overlapping file edits between workers. This is unchanged. The integration branch feature does not affect claims -- claims are file-level, not branch-level.

### 11.3 Stale Detection

Stale detection is session-scoped (via heartbeat files). Each worker has its own heartbeat file in its worktree. The integration branch does not affect stale detection.

### 11.4 Branch Name Uniqueness

Auto-generated integration branch names include the last 8 characters of the batch session ID. Since session IDs are ULIDs (monotonically increasing, globally unique), the branch names are unique across batch runs.

---

## 12. Testing Requirements

### 12.1 Unit Tests

| Test | Module | What to Assert |
|------|--------|---------------|
| `test_resolve_target_empty_generates_autorun_prefix` | batch.rs | `resolve_target("mybatch", "ses-01knfj4bynh7fh754dhbvrm3qw")` starts with `"autorun/mybatch-"` |
| `test_resolve_target_nonempty_passthrough` | batch.rs | `resolve_target("x", "ses-y")` with explicit target returns the explicit value unchanged |
| `test_resolve_auto_merge_none_protected_returns_false` | batch.rs | `resolve_auto_merge(None, "main", &["main"])` returns `false` |
| `test_resolve_auto_merge_none_nonprotected_returns_true` | batch.rs | `resolve_auto_merge(None, "autorun/x", &["main"])` returns `true` |
| `test_resolve_auto_merge_explicit_overrides` | batch.rs | `resolve_auto_merge(Some(false), "autorun/x", &["main"])` returns `false` |
| `test_resolve_final_pr_none_main_returns_false` | batch.rs | `resolve_final_pr(None, "main")` returns `false` |
| `test_resolve_final_pr_none_integration_returns_true` | batch.rs | `resolve_final_pr(None, "autorun/x")` returns `true` |
| `test_option_bool_yaml_compat` | batch.rs | YAML `auto_merge: true` deserializes to `Some(true)`; omitted deserializes to `None` |
| `test_update_task_row_status_4col` | epic_update.rs | Given a 4-column markdown table, updates the status cell correctly |
| `test_update_task_row_status_5col` | epic_update.rs | Given a 5-column markdown table, updates the status cell correctly |
| `test_update_task_row_status_missing_task` | epic_update.rs | Returns `None` when the task ID is not found |
| `test_position_for_target_scoped` | merge_queue.rs | Entries with different `target_branch` do not affect each other's positions |
| `test_dequeue_for_target_scoped` | merge_queue.rs | Only dequeues entries matching the specified target |
| `test_remove_by_session_cleans_all` | merge_queue.rs | Removes all entries for a given session regardless of position |
| `test_remove_by_session_empty_queue` | merge_queue.rs | Returns 0 on empty queue |
| `test_pipeline_prefix_autorun` | pipeline.rs | `infer_work_type_from_branch("autorun/mybatch")` returns `"AUTORUN"` |

### 12.2 Integration Tests

| Test | Scope | What to Assert |
|------|-------|---------------|
| Full batch flow with integration branch | orchestrator + worker | Workers create PRs targeting integration branch, serialized merge succeeds, final PR created |
| Batch with mixed success/failure | orchestrator | Epic updates only for completed tasks, final PR excludes failed workers |
| Resume after partial failure | orchestrator | `codeflow autorun resume` re-runs failed tasks against same integration branch |

### 12.3 Race Condition Tests

| Test | What to Assert |
|------|---------------|
| Concurrent queue access (2 workers same target) | Exactly one worker at position 0 at any time |
| Worker crash mid-merge (between enqueue and dequeue) | `remove_by_session()` cleans up, next worker proceeds |
| Queue timeout during wait | Worker marks failed, entry removed, next worker unblocked |

---

## 13. File Change Inventory

### 13.1 Rust Core (`codeflow-cli/core/src/`)

| File | Action | Description |
|------|--------|-------------|
| `autorun/batch.rs` | MODIFY | `auto_merge: bool` -> `Option<bool>`, add `final_pr`, `final_pr_base`, `target_is_auto`, add resolution functions |
| `autorun/worker.rs` | MODIFY | Add `serialized_merge()`, cleanup merge queue on all exit paths |
| `autorun/orchestrator.rs` | MODIFY | Add integration branch creation, post-batch processing, propagate new config fields |
| `autorun/config.rs` | MODIFY | Add `epic_update` to `AutorunConfig`, add `queue_enforcing`/`queue_timeout_secs` to `MergeConfig` |
| `autorun/epic_update.rs` | CREATE | New module for epic markdown status updates |
| `autorun/mod.rs` | MODIFY | Add `pub mod epic_update;` |
| `coordination/merge_queue.rs` | MODIFY | Add `target_branch` to `MergeQueueEntry`, add `*_for_target` and `remove_by_session` functions |
| `hooks/pipeline.rs` | MODIFY | Add `("autorun/", "AUTORUN")` to `PREFIX_MAP` |
| `models/autorun.rs` | MODIFY | Add `target_branch`, `final_pr_url` to `AutorunSession` |
| `session/active_task.rs` | MODIFY | Add `target_branch`, `auto_merge`, `epic_update` to `ActiveTask` |

### 13.2 Rust CLI (`codeflow-cli/cli/src/`)

| File | Action | Description |
|------|--------|-------------|
| `cmd/autorun.rs` | MODIFY | Add env vars, integration branch creation, post-batch processing, final PR, gh auth preflight |

### 13.3 Config Files

| File | Action | Description |
|------|--------|-------------|
| `.codeflow/config/enforcement/enforcement-policy.json` | MODIFY | Add `"autorun"` to `branch_types`, `"autorun/"` to `branch_prefixes` |
| `.codeflow/config/parallel-work/parallel-work-config.json` | MODIFY | Add `queue_enforcing`, `queue_timeout_secs` to `merge`; add `epic_update` to `autorun` |

### 13.4 Agent Definitions

| File | Action | Description |
|------|--------|-------------|
| `.claude/agents/cf-git-operations.md` | MODIFY | Add `autorun/*` to branch access, document `AUTORUN_TARGET` for PF3/PF6 |
| `.claude/agents/cf-knowledge-layer.md` | MODIFY | Document `AUTORUN_EPIC_UPDATE` skip behavior in `complete-work` |

### 13.5 CLAUDE.md

| File | Action | Description |
|------|--------|-------------|
| `.claude/CLAUDE.md` | MODIFY | Add `autorun/*` to feature branch list (Section 7), add `AUTORUN_TARGET`/`AUTORUN_AUTO_MERGE`/`AUTORUN_EPIC_UPDATE` to env var table (Section 4) |

### 13.6 Batch File Examples

No new files. Existing batch files remain valid (backward compatible).

---

## 14. Backward Compatibility

### 14.1 Batch File Compatibility

| Scenario | Before | After |
|----------|--------|-------|
| `auto_merge: true` (explicit bool) | Parsed as `bool` | Parsed as `Option<bool>` -> `Some(true)`. serde_yaml deserializes `true` into `Option<bool>` as `Some(true)`. |
| `auto_merge:` omitted | Default `false` | Default `None` -> inferred based on target branch |
| `target: "main"` (explicit) | Workers PR to main | Workers PR to main (unchanged) |
| `target:` omitted | Default `""` (workers PR to main) | Default `""` -> auto-generated integration branch |

**Breaking change analysis:** The only behavioral change for existing batch files is when `target` is omitted. Currently, omitted `target` means workers PR to main. After this change, omitted `target` generates an integration branch. However, since `auto_merge` also changes from `false` (default bool) to inferred, the net effect for an existing batch file with both fields omitted is:

- Before: target=main, auto_merge=false -> workers PR to main, PRs left open
- After: target=autorun/{name}-{8chars}, auto_merge=true (inferred, not protected) -> workers PR to integration, serialized merge, final PR to main

This is a behavioral change but an improvement. Users who want the old behavior can set `target: "main"` and `auto_merge: false` explicitly.

### 14.2 Merge Queue Compatibility

The `target_branch` field on `MergeQueueEntry` uses `#[serde(default)]` which defaults to an empty string. Existing queue entries (without `target_branch`) will deserialize with `target_branch: ""`. The `*_for_target` functions treat empty `target_branch` as matching any target (wildcard), preserving backward compatibility for any queue entries created before the migration.

### 14.3 Config Compatibility

All new config fields have defaults via `#[serde(default)]` or `Default` implementations. Existing `parallel-work-config.json` files without the new fields work without modification.

---

## 15. Assumptions

| # | Assumption | Verified? | Evidence |
|---|-----------|-----------|----------|
| 1 | `BatchFile.auto_merge` is currently `bool` (not `Option<bool>`) | YES | Read `batch.rs:29`: `pub auto_merge: bool` |
| 2 | `MergeQueueEntry` does not have a `target_branch` field | YES | Read `merge_queue.rs:26-35`: 4 fields (session_id, task_id, branch, pr_ready_at) |
| 3 | `PREFIX_MAP` in `pipeline.rs` has 10 entries, last is `("hotfix/", "HTFX")` | YES | Read `pipeline.rs:139-150`: confirmed 10 entries |
| 4 | `enforcement-policy.json` has `branch_types` and `branch_prefixes` arrays | YES | Read `enforcement-policy.json:108-109`: both arrays with 20 entries each |
| 5 | `ActiveTask` does not have `target_branch`, `auto_merge`, or `epic_update` fields | YES | Read `active_task.rs:30-76`: fields end at `file_scope`, no target/auto_merge/epic_update |
| 6 | `AutorunSession` does not have `target_branch` or `final_pr_url` fields | YES | Read `models/autorun.rs:7-31`: no such fields |
| 7 | `MergeConfig` has `auto_rebase`, `queue_enabled`, `max_rebase_attempts` (no queue_enforcing or timeout) | YES | Read `config.rs:163-180`: confirmed 3 fields only |
| 8 | `AutorunConfig` does not have `epic_update` field | YES | Read `config.rs:42-58`: no epic_update field |
| 9 | `WorkerConfig` already has `target` and `auto_merge` fields | YES | Read `orchestrator.rs:36-54`: confirmed both fields present |
| 10 | `InvokeConfig` already has `target` and `auto_merge` fields | YES | Read `worker.rs:106-121`: confirmed both fields present |
| 11 | `git2` crate is already a dependency (used in `conflict.rs`) | YES | Read `git/conflict.rs:44`: `git2::Repository::open` |
| 12 | The `locked_binary_rmw` pattern is used for all CRDT operations | YES | Read `merge_queue.rs:158-201`: `locked_enqueue` and `locked_dequeue` both use it |
| 13 | `serde_yaml` deserializes YAML `true` into `Option<bool>` as `Some(true)` | YES | serde_yaml standard behavior for `Option<T>` fields with `#[serde(default)]` |

---

[<- Back to Overview](README.md)
