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
| `start <batch-file>` | `/cf-autorun start sprint-tasks.yaml` | Start autorun session from batch definition |
| `status [session-id]` | `/cf-autorun status` | Show session/worker status |
| `sessions` | `/cf-autorun sessions` | List all autorun sessions |
| `stop <session-id>` | `/cf-autorun stop ses-abc123` | Stop a running session |
| `cleanup --merged` | `/cf-autorun cleanup --merged` | Clean up merged PRs and worktrees |
| `logs <session-id>` | `/cf-autorun logs ses-abc123` | View session logs |

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

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `subcommand` | Yes | One of: `start`, `status`, `sessions`, `stop`, `cleanup`, `logs` |
| `batch-file` | For `start` | Path to batch definition YAML file |
| `session-id` | For `stop`, `logs`; optional for `status` | Autorun session identifier |

**Flags:**

| Flag | Short | Applies To | Description | Default |
|------|-------|------------|-------------|---------|
| `--merged` | | `cleanup` | Clean up only merged PRs and their worktrees | Required for cleanup |
| `--max-workers` | `-w` | `start` | Override max concurrent workers | Config default (3) |
| `--timeout` | `-t` | `start` | Override per-task timeout | Config default (1h) |
| `--dry-run` | | `start` | Validate batch without launching workers | false |

**Examples:**

```bash
# Start autorun from batch file
/cf-autorun start .state/autorun/batches/sprint-tasks.yaml

# Check overall status
/cf-autorun status

# Check specific session
/cf-autorun status ses-abc123

# List all sessions
/cf-autorun sessions

# Stop a running session
/cf-autorun stop ses-abc123

# View logs
/cf-autorun logs ses-abc123

# Clean up after PR merge
/cf-autorun cleanup --merged

# Dry run to validate batch
/cf-autorun start sprint-tasks.yaml --dry-run
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
```

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
flowchart TD
    Start([/cf-autorun start invoked]) --> Parse[Parse subcommand and arguments]
    Parse --> ValidateBatch{Batch file valid?}
    ValidateBatch -->|No| Error([ERROR: Invalid batch])
    ValidateBatch -->|Yes| CheckState

    CheckState{PathFlow active?}
    CheckState -->|Yes| ErrorActive([ERROR: Active PathFlow session])
    CheckState -->|No| CheckClean

    CheckClean{Git tree clean?}
    CheckClean -->|No| ErrorDirty([ERROR: Uncommitted changes])
    CheckClean -->|Yes| ResolveTasks

    ResolveTasks[Resolve task IDs from WorkGraph] --> DryRun{--dry-run?}
    DryRun -->|Yes| ReportDry([Report validation results])
    DryRun -->|No| CreateSession

    CreateSession[Create autorun session record] --> SpawnWorkers
    SpawnWorkers[Spawn tmux workers with worktrees] --> Monitor

    Monitor[Workers execute PathFlow PF1-PF7] --> WorkerDone{All workers done?}
    WorkerDone -->|No| Monitor
    WorkerDone -->|Yes| Summarize

    Summarize[Summarize: PRs created, failures, time] --> End([Complete])
```

### 4.2 Execution Steps

**Step 1: Parse Subcommand**

- Identify subcommand (`start`, `status`, `sessions`, `stop`, `cleanup`, `logs`)
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
- LLM-based Stop hook (Haiku) verifies completion per worker
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
| Stop (in workers) | Worker task completes | Haiku-based completion verification |

**Worker-Level Hooks:**

Each worker runs as an independent Claude session with its own hook lifecycle. The Stop hook in each worker uses a Haiku-class model to verify acceptance criteria are met before marking the task complete.

```text
Worker Stop Hook Flow:
  Worker reaches stop point
    ↓
  Haiku evaluates: Are acceptance criteria met?
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
| `.state/ledger/pathflow-events.jsonl` | Event log (worker progress) |

### Files Created/Modified

| File | When |
|------|------|
| `.state/autorun/sessions/{session-id}.json` | Session created (`start`) |
| `.state/autorun/sessions/{session-id}.json` | Session updated (`stop`) |
| Worker worktree directories | Created per worker (`start`), removed (`cleanup`) |

### Batch File Format

```yaml
# .state/autorun/batches/sprint-tasks.yaml
name: Sprint 42 Tasks
description: Authentication feature tasks
tasks:
  - FRT-TSK-FEAT-AUTH-001
  - FRT-TSK-FEAT-AUTH-002
  - FRT-TSK-FEAT-AUTH-003
target_branch: develop
max_session_workers: 3
timeout: 1h
```

### Configuration

```yaml
# .codeflow/config/autorun.yaml
execution:
  max_session_workers: 3
  order: dependency
timeouts:
  per_task: 1h
  per_session: 8h
pr:
  create: true
  merge: false    # Human review required
worktree:
  cleanup_on_merged: true
  preserve_on_failure: true
```

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Active PathFlow session | PathFlow flag exists | Complete or clean up current session first (`/cf-cleanup` or `/cf-doctor --repair`) |
| Dirty working tree | Uncommitted changes | Commit or stash changes before starting |
| Invalid batch file | Malformed YAML or missing fields | Fix batch file syntax; required: `name`, `tasks`, `target_branch` |
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

  Worker 1: FRT-TSK-FEAT-AUTH-001  RUNNING
  Worker 2: FRT-TSK-FEAT-AUTH-002  RUNNING
  Worker 3: FRT-TSK-FEAT-AUTH-003  RUNNING

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

  Worker 1: FRT-TSK-FEAT-AUTH-001  COMPLETED  PR #12  (23m)
  Worker 2: FRT-TSK-FEAT-AUTH-002  RUNNING            (31m)
  Worker 3: FRT-TSK-FEAT-AUTH-003  COMPLETED  PR #13  (18m)

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

  Worker 2: FRT-TSK-FEAT-AUTH-002  STOPPED (was running)
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

  Removed worktree: FRT-TSK-FEAT-AUTH-001 (PR #12 merged)
  Removed worktree: FRT-TSK-FEAT-AUTH-003 (PR #13 merged)
  Preserved worktree: FRT-TSK-FEAT-AUTH-002 (PR not merged)

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
