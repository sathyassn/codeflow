---
description: "Launch autonomous multi-task execution"
argument-hint: "<subcommand> [arguments]"
---

# /cf-autorun Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Assess autorun infrastructure and session state
- 🔧 think-and-act: Before launching workers or modifying session state (PAC-5)
- 🔧 decide: Worker count, timeout, and batch validation decisions are Tier 2
- 🔧 respond-organized: Clear status output with worker progress and session summaries

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Launch and manage autonomous multi-task execution sessions using parallel workers with isolated worktrees.

**Usage:**

```text
/cf-autorun <subcommand> [arguments]
```

**Subcommands:**

| Subcommand | Usage | Purpose |
|------------|-------|---------|
| `run` | `/cf-autorun run --batch tasks.yaml` | Execute a batch of tasks in parallel worktrees |
| `status` | `/cf-autorun status` | Show status of active autorun batches |
| `attach <task_id>` | `/cf-autorun attach INF-TSK-023-001` | Attach to a running worker's tmux session |
| `logs <task_id>` | `/cf-autorun logs INF-TSK-023-001` | View logs from a worker's tmux session |
| `cancel <task_id>` | `/cf-autorun cancel INF-TSK-023-001` | Cancel a single running worker |
| `abort` | `/cf-autorun abort --batch ses-abc123` | Abort an entire batch (cancel all running, skip pending) |
| `results` | `/cf-autorun results --batch ses-abc123` | Display results from a batch execution |
| `history` | `/cf-autorun history --limit 10` | Show historical autorun batch executions |

**Use When:**

- Multiple independent tasks need parallel execution
- Sprint work should run autonomously without human intervention
- Batch processing of planned tasks from the WorkGraph
- Overnight or background task execution

**Do Not Use When:**

- A PathFlow session is already active (workers create their own)
- Tasks have complex interdependencies requiring human judgment
- Single task execution (use `/cf-develop` directly)
- Tasks are not yet planned or registered in the WorkGraph

### Pipeline Position

```text
Phase: Pre-PF1 | Type: Autonomous execution
Prerequisite: No active PathFlow session
Launches independent workers that each run their own PF1-PF7 pipeline.
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `subcommand` | Yes | One of: `run`, `status`, `attach`, `logs`, `cancel`, `abort`, `results`, `history` |
| `--batch <path>` | For `run` | Path to batch definition YAML file (default: `.codeflow/config/autorun/batch.yaml`) |
| `<task_id>` | For `attach`, `logs`, `cancel` | Task ID of the worker to interact with |
| `--batch <session_id>` | For `status`, `abort`, `results` (optional) | Filter to specific batch session |

**Flags:**

| Flag | Short | Applies To | Description | Default |
|------|-------|------------|-------------|---------|
| `--batch <path>` | `-b` | `run` | Path to YAML batch file | `.codeflow/config/autorun/batch.yaml` |
| `--batch <session_id>` | | `status`, `abort`, `results` | Filter to specific batch session | (all active) |
| `--follow` | `-f` | `logs` | Stream output (poll at 500ms) | false |
| `--limit` | | `history` | Maximum number of batches to show | 10 |
| `--since` | | `history` | Filter batches started after ISO date | (none) |
| `--status` | | `history` | Filter by status (completed, failed, aborted) | (none) |
| `--batch-name` | | `history` | Filter by batch name pattern (substring) | (none) |
| `--all` | | `history` | Show all batches (overrides --limit) | false |

**Examples:**

```bash
# Execute a batch of tasks
/cf-autorun run --batch .state/autorun/batches/sprint-tasks.yaml

# Check overall status
/cf-autorun status

# Check specific batch
/cf-autorun status --batch ses-abc123

# Attach to a running worker
/cf-autorun attach INF-TSK-023-001

# View worker logs
/cf-autorun logs INF-TSK-023-001

# Follow worker logs in real-time
/cf-autorun logs INF-TSK-023-001 --follow

# Cancel a single worker
/cf-autorun cancel INF-TSK-023-001

# Abort an entire batch
/cf-autorun abort --batch ses-abc123

# View batch results
/cf-autorun results --batch ses-abc123

# Show recent batch history
/cf-autorun history --limit 5

# Show all completed batches since a date
/cf-autorun history --status completed --since 2026-03-01
```

---

## 3. Prerequisites

**Required State:**

- [ ] No active PathFlow session (autorun workers create their own)
- [ ] Git repository is clean (no uncommitted changes)
- [ ] Batch file exists and is valid YAML (for `start`)
- [ ] All referenced task IDs exist in the WorkGraph (for `start`)
- [ ] `codeflow` CLI is installed and accessible (global CLI)
- [ ] tmux is available for worker process management

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| `codeflow` CLI | Autorun orchestration engine |
| tmux | Worker process isolation |
| git worktrees | Parallel branch isolation |
| `.state/autorun/` | Session and batch storage |

### Pre-Start Validation

Before launching workers, the CLI validates:

```text
1. No pathflow-active flag exists
2. Working tree is clean (git status --porcelain is empty)
3. Batch file parses as valid YAML
4. All task IDs resolve in the WorkGraph
5. Target branch exists
6. Sufficient disk space for worktrees
7. If auto_merge:true, target_branch must be non-null
8. If auto_merge:true, target_branch must NOT be a protected branch (main, master, release/*, production)
```

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: Pre-PF1 | Type: Autonomous Execution

/cf-autorun start invoked
    |
    v
Parse subcommand and arguments
    |
    v
Batch file valid? ---NO---> ERROR: "Invalid batch"
    |
    YES
    |
    v
PathFlow active? ---YES---> ERROR: "Active PathFlow session"
    |
    NO
    |
    v
Git tree clean? ---NO---> ERROR: "Uncommitted changes"
    |
    YES
    |
    v
Resolve task IDs from WorkGraph                [cf-knowledge-layer]
    |
    v
--dry-run? ---YES---> Report validation results, STOP
    |
    NO
    |
    v
Create autorun session record
    |
    v
Spawn tmux workers with worktrees
    |
    v
Workers execute PathFlow PF1-PF7 (each worker independently)
    |
    v
All workers done? ---NO---> Continue monitoring
    |
    YES
    |
    v
Summarize: PRs created, failures, time
    |
    v
Next: Monitor with /cf-autorun status, cleanup with /cf-autorun cleanup --merged
```

### 4.1.2 Status Subcommand

```text
/cf-autorun status [session-id]
    |
    v
Session ID provided? ---NO---> List all active sessions
    |                              |
    YES                            v
    |                          Show summary for each
    v
Query session record                           [cf-knowledge-layer]
    |
    v
Display worker states (running, completed, failed, timed-out)
    |
    v
Show progress: {completed}/{total} tasks
```

### 4.1.3 Sessions Subcommand

```text
/cf-autorun sessions
    |
    v
Query all autorun sessions from JSONL          [cf-knowledge-layer]
    |
    v
Format session list (ID, name, status, progress)
    |
    v
Display sorted by most recent first
```

### 4.1.4 Stop Subcommand

```text
/cf-autorun stop <session-id>
    |
    v
Validate session exists and is running         [cf-knowledge-layer]
    |
    v
Signal workers to stop (graceful shutdown)
    |
    v
Wait for workers to reach safe stopping point
    |
    v
Preserve worktrees for debugging
    |
    v
Update session record: status=stopped
    |
    v
Report: {completed} completed, {stopped} stopped
```

### 4.1.5 Cleanup Subcommand

```text
/cf-autorun cleanup --merged
    |
    v
List all autorun worktrees                     [cf-git-operations]
    |
    v
Identify merged PRs (branch merged to main)
    |
    v
For each merged worktree:
    Remove worktree directory
    Delete local branch
    |
    v
Preserve unmerged worktrees
    |
    v
Report: {removed} cleaned, {preserved} preserved
```

### 4.1.6 Logs Subcommand

```text
/cf-autorun logs <session-id>
    |
    v
Validate session exists                        [cf-knowledge-layer]
    |
    v
Read session log files from .state/autorun/sessions/
    |
    v
Format and display worker logs (chronological)
    |
    v
Show per-worker status and event timeline
```

### 4.2 Execution Steps

**Step 1: Parse Subcommand**

- Identify subcommand (`run`, `status`, `attach`, `logs`, `cancel`, `abort`, `results`, `history`)
- Parse subcommand-specific arguments and flags
- Route to appropriate handler

**Step 2: Validate Prerequisites (start only)**

- Verify no `pathflow-active` flag exists
- Verify git working tree is clean
- Parse and validate batch YAML file
- Resolve all task IDs against WorkGraph

**Step 3: Create Session Record (start only)**

- Generate session ID
- Store session record in `.state/autorun/sessions/`
- Log session start event to JSONL

**Step 4: Spawn Workers (start only)**

- Create git worktree for each task (from target branch)
- Launch tmux session per worker
- Each worker runs `claude` with autorun environment variables:
  - `AUTORUN_SESSION_ID` -- Session identifier
  - `AUTORUN_TASK_ID` -- Assigned task
  - `AUTORUN_ACCEPTANCE` -- Acceptance criteria
- Workers execute full PathFlow lifecycle (PF1 through PF7) autonomously

**Step 5: Monitor Progress (start only)**

- Track worker status via session files
- LLM-based Stop hook (Sonnet) verifies completion per worker
- Workers create PRs when their PathFlow completes successfully
- Report failures and timeouts

**Step 6: Present Results**

- **start:** Summary of spawned workers and session ID
- **status:** Worker states (running, completed, failed, timed-out)
- **sessions:** List of all sessions with status
- **stop:** Confirmation of session termination
- **cleanup:** List of cleaned worktrees and branches
- **logs:** Session event log output

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Reasoning before launching workers |
| cf-knowledge-layer | query-tasks | Resolve and validate task IDs from batch |
| cf-knowledge-layer | query-events | Session history and status |

**Note:** This command is a CLI wrapper. The `codeflow autorun` CLI handles worker orchestration directly. The lead invokes the CLI and presents results.

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-autorun` invoked | Validate invocation |
| Stop (in workers) | Worker task completes | Sonnet-based completion verification |

**Worker-Level Hooks:**

Each worker runs as an independent Claude session with its own hook lifecycle. The Stop hook in each worker uses a Sonnet-class model to verify acceptance criteria are met before marking the task complete.

```text
Worker Stop Hook Flow:
  Worker reaches stop point
    ↓
  Sonnet evaluates: Are acceptance criteria met?
    ↓
  ├── Met → Worker creates PR, marks task complete
  └── Not met → Worker continues or times out
```

---

## 7. Memory Integration

**Memory usage:** Session metadata in `.state/autorun/`; task data via WorkGraph.

### Files Accessed (Read)

| File | Purpose |
|------|---------|
| `.state/autorun/batches/*.yaml` | Batch definitions |
| `.state/autorun/sessions/*.json` | Session records and status |
| `.state/db/codeflow.db` | Task resolution from WorkGraph |
| `.state/logs/pathflow-events.jsonl` | Event log (worker progress) |

### Files Created/Modified

| File | When |
|------|------|
| `.state/autorun/sessions/{session-id}.json` | Session created (`start`) |
| `.state/autorun/sessions/{session-id}.json` | Session updated (`stop`) |
| Worker worktree directories | Created per worker (`start`), removed (`cleanup`) |

### Batch File Format

Minimal batch file — task IDs and dependencies only. File scope, scope policy,
and acceptance criteria come from the task markdown (source of truth).

```yaml
name: my-batch
max_workers: 3
auto_merge: false
tasks:
  - id: FRT-TSK-001-001
  - id: FRT-TSK-001-002
  - id: FRT-TSK-001-003
    depends_on:
      - FRT-TSK-001-001
      - FRT-TSK-001-002
```

See `.codeflow/config/autorun/examples/` for annotated examples:
`simple-sequential.yaml`, `parallel-independent.yaml`,
`auto-merge-integration.yaml`, `complex-dependencies.yaml`.

**Field reference:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Human-readable batch name |
| `max_workers` | int | No | Max concurrent workers (default: 3) |
| `auto_merge` | bool | No | Auto-merge worker PRs (default: false) |
| `target` | string | When `auto_merge:true` | Target branch for PRs; must NOT be protected |
| `tasks[].id` | string | Yes | Task ID matching WorkGraph |
| `tasks[].depends_on` | list | No | Task IDs that must complete first |
| `tasks[].file_scope` | list | No | Override file scope (narrows task markdown scope) |
| `tasks[].scope_policy` | string | No | Override scope policy (tightens task markdown policy) |

**Validation Rules:**

| Rule | Condition | Result |
|------|-----------|--------|
| `auto_merge:true` requires `target` | `auto_merge:true` and `target` is empty | Validation error at batch parsing time |
| `auto_merge:true` + protected target is FORBIDDEN | `auto_merge:true` and `target` is main, master, release/\*, or production | Validation error at batch parsing time |
| `auto_merge:false` terminal state | Workers with `auto_merge:false` end in `complete` | Worker creates PR but does not merge; task is already complete |

**Deprecated fields:**

| Field | Status | Replacement |
|-------|--------|-------------|
| `auto_commit` | Deprecated | Commits are always created by workers via cf-git-operations. Field is ignored if present. |
| `target_branch` | Renamed | Use `target` instead |
| `max_session_workers` | Renamed | Use `max_workers` instead |
| `description` | Removed | Not used by the batch parser |
| `timeout` | Removed | Use `autorun.worker_timeout_secs` in `parallel-work-config.json` instead |

### Integration Branch Convention

When `auto_merge:true`, autorun uses an integration branch pattern:

```text
main
  └── autorun/{batch-name}              ← integration branch (created off main)
        ├── feat/task-001               ← worker branch (created off integration)
        ├── feat/task-002               ← worker branch (created off integration)
        └── fix/task-003                ← worker branch (created off integration)
```

- The integration branch `autorun/{batch-name}` is created off `main` before workers start
- Each worker creates its feature branch off the integration branch
- Worker PRs target the integration branch (NOT main)
- Worker PRs are merged via `gh pr merge --delete-branch` (NOT `--squash`, to preserve commit history on the integration branch)
- After all workers complete: human reviews the summary PR from integration branch to main

### Worker Terminal States

| `auto_merge` | CI Status | Worker End State | Action |
|-------------|-----------|-----------------|--------|
| `true` | Passed | `complete` | Worker PR auto-merged to integration branch via `gh pr merge --delete-branch` |
| `true` | Failed | `failed` | PR left open, failures reported |
| `false` | Passed | `complete` | PR created, human review required |
| `false` | Failed | `failed` | PR left open, failures reported |

### Configuration

**Parallel work config** (`.codeflow/config/parallel-work/parallel-work-config.json`):

```json
{
  "worktree": { "mode": "autorun", "max_concurrent": 3, "base_dir": ".git-worktrees" },
  "sync": { "interval_secs": 30, "auto_start": true },
  "merge": { "auto_rebase": true, "queue_enabled": true, "max_rebase_attempts": 3 },
  "claims": { "default_scope_policy": "soft", "ttl_secs": 4200, "capture_events": true }
}
```

This config is optional (defaults apply when absent). See [parallel-work-config-spec.md](../../.codeflow/docs/analysis/parallel-work/parallel-work-config-spec.md) for field descriptions.

**Worker spawning procedure:**

1. `codeflow autorun` parses the batch YAML file
2. Config is loaded from `parallel-work-config.json` (defaults if absent)
3. Workers are spawned into separate worktrees (one per task, max `max_concurrent`)
4. Each worker pre-acquires claims for its `file_scope` via `acquire_batch()`
5. Workers write `scope_policy` and `file_scope` to `active-task.json`
6. Workers use `WorktreePaths` for all `.state/` access
7. On completion: claims released via `release_all()`, worktree cleaned up
8. PR merge goes through merge queue (`enqueue` before, `dequeue` after)
9. Merge conflict detection runs via `check_merge_conflicts()` before PR creation

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Active PathFlow session | PathFlow flag exists | Complete or clean up current session first (`/cf-cleanup` or `/cf-doctor --repair`) |
| Dirty working tree | Uncommitted changes | Commit or stash changes before starting |
| Invalid batch file | Malformed YAML or missing fields | Fix batch file syntax; required: `name`, `tasks`, `target_branch` |
| auto_merge + protected target | `auto_merge:true` with main/master/release/\*/production | FORBIDDEN: Change `target_branch` to a non-protected branch or set `auto_merge:false` |
| auto_merge + missing target | `auto_merge:true` with null `target_branch` | Set `target_branch` to a valid non-protected branch |
| Task not found | Task ID not in WorkGraph | Register task first via `/cf-plan`; verify task IDs |
| Worker timeout | Task exceeded time limit | Check logs (`/cf-autorun logs`); worktree preserved for debugging |
| Worker failure | Task execution error | Review worker logs; worktree preserved on failure |
| tmux not available | tmux not installed | Install tmux: `brew install tmux` (macOS) |
| Worktree conflict | Branch already checked out | Clean up stale worktrees: `/cf-autorun cleanup --merged` |

**Recovery Procedures:**

```text
ON "Active PathFlow session":
  1. Complete current work: /cf-ship or /cf-cleanup
  2. OR repair stale state: /cf-doctor --repair
  3. Retry: /cf-autorun start <batch>

ON "Worker timeout":
  1. Check logs: /cf-autorun logs <session-id>
  2. Worktree preserved at: .git/worktrees/<task-id>/
  3. Resume manually or adjust timeout and retry

ON "Worker failure":
  1. Review logs: /cf-autorun logs <session-id>
  2. Worktree preserved for debugging
  3. Fix issue and re-run failed task individually: /cf-develop <task-id>
```

---

## 9. Examples

**Example 1: Start autorun session**

```bash
/cf-autorun start .state/autorun/batches/sprint-tasks.yaml
```

Output:

```text
Autorun Session Started

  Session:    ses-20260215-001
  Batch:      Sprint 42 Tasks
  Tasks:      3
  Workers:    3 (max)
  Timeout:    1h per task
  Target:     develop

  Worker 1: FRT-TSK-001-001  RUNNING
  Worker 2: FRT-TSK-001-002  RUNNING
  Worker 3: FRT-TSK-001-003  RUNNING

Monitor: /cf-autorun status ses-20260215-001
```

**Example 2: Check status**

```bash
/cf-autorun status ses-20260215-001
```

Output:

```text
Autorun Session: ses-20260215-001
Batch: Sprint 42 Tasks

  Worker 1: FRT-TSK-001-001  COMPLETED  PR #12  (23m)
  Worker 2: FRT-TSK-001-002  RUNNING            (31m)
  Worker 3: FRT-TSK-001-003  COMPLETED  PR #13  (18m)

Progress: 2/3 complete, 1 running
```

**Example 3: List all sessions**

```bash
/cf-autorun sessions
```

Output:

```text
Autorun Sessions

  ses-20260215-001  Sprint 42 Tasks       RUNNING   2/3 complete
  ses-20260214-003  Bugfix Batch          COMPLETED 5/5 complete
  ses-20260214-001  Refactor Utilities    COMPLETED 4/4 complete
```

**Example 4: Stop a session**

```bash
/cf-autorun stop ses-20260215-001
```

Output:

```text
Stopping session ses-20260215-001...

  Worker 2: FRT-TSK-001-002  STOPPED (was running)
  Worktree preserved for debugging.

Session stopped. 2 tasks completed, 1 stopped.
```

**Example 5: Clean up after merging PRs**

```bash
/cf-autorun cleanup --merged
```

Output:

```text
Cleanup Results

  Removed worktree: FRT-TSK-001-001 (PR #12 merged)
  Removed worktree: FRT-TSK-001-003 (PR #13 merged)
  Preserved worktree: FRT-TSK-001-002 (PR not merged)

Cleaned 2 worktrees.
```

**Example 6: Dry run validation**

```bash
/cf-autorun start sprint-tasks.yaml --dry-run
```

Output:

```text
Dry Run Validation

  Batch:      Sprint 42 Tasks
  Tasks:      3 (all resolved)
  Target:     develop (exists)
  Workers:    3 (within limit)
  State:      No active PathFlow session
  Git:        Working tree clean

Validation passed. Ready to start.
```

---

## 10. References

- [CLAUDE.md](../CLAUDE.md) -- PathFlow phases and autorun mode behavior
- [PathFlow config](../../.codeflow/config/pathflow/pathflow-config.json) -- Phase and stage definitions
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md) -- WorkGraph task queries
- [cf-develop command](./cf-develop.md) -- Single-task implementation (worker equivalent)
- [cf-doctor command](./cf-doctor.md) -- Infrastructure diagnostics and repair
- [cf-cleanup command](./cf-cleanup.md) -- Session cleanup
- [Parallel work config spec](../../.codeflow/docs/analysis/parallel-work/parallel-work-config-spec.md) -- Config field descriptions
- [Parallel work config](../../.codeflow/config/parallel-work/parallel-work-config.json) -- Runtime config file
