# Task Lifecycle Reference

## Task Status Flow

```text
┌─────────┐     ┌─────────────┐     ┌───────────┐
│ pending │────▶│ in_progress │────▶│ completed │
└─────────┘     └─────────────┘     └───────────┘
     │                │
     │                ▼
     │          ┌─────────┐
     └─────────▶│ blocked │
                └─────────┘
```

## Initiation Phase

Before a task enters the status flow, it goes through initiation:

1. Task is created via ensure-work-registered or create-task
   - Returns both `id` (ULID PK: `task-{ulid}`) and `format_id` (e.g., `BKD-TSK-FEAT-API-012`)
2. active-task.json is created via cf-work-state.sh at `.state/runtime/active-task.json`
   - Fields: task_id (ULID PK), epic_id (ULID PK), task_format_id, epic_format_id, title, status, branch, session_id, started_at, updated_at
   - Note: `task_id` and `epic_id` use ULID PKs for internal references; `task_format_id` and `epic_format_id` are for display
3. Task enters `pending` status and proceeds through the flow

## Status Definitions

| Status | Description | Valid Transitions |
|--------|-------------|-------------------|
| pending | Created, waiting to start | in_progress, blocked |
| in_progress | Actively being worked on | completed, blocked, pending |
| blocked | Cannot proceed due to dependencies | pending, in_progress |
| completed | Work finished and verified | (terminal state) |

## Transition Operations

**Do not update status directly.** Use the appropriate skill operations:

### Starting Work

```text
cf-memory-management:begin-work
→ Validates task is actionable
→ Updates status to in_progress
→ Registers active_work
```

### Completing Work

```text
cf-memory-management:complete-work
→ Verifies deliverables
→ Updates status to completed
→ Creates commit sentinel
```

### Recording Blockers

```text
cf-task-management:update-task
→ Adds blocker to task
→ Updates status to blocked if needed
```

### Clearing Blockers

When a blocking task completes, `cf-db-operations:task-update` automatically:

- Removes completed task from blockers array
- Transitions blocked tasks to pending if no remaining blockers

## Agent Permissions for Status Changes

| Transition | cf-planner | cf-developer | cf-reviewer | cf-qa |
|------------|------------|--------------|-------------|-------|
| → pending | Yes | Yes | No | No |
| → in_progress | Yes | Yes | Yes | Yes |
| → blocked | Yes | Yes | Yes | Yes |
| → completed | Yes | Yes | Yes | Yes |

## Blocker Management

### Adding a Blocker

Use `cf-task-management:update-task` with blocker information.
The operation validates the blocker exists before adding.

### Automatic Unblocking

When `cf-db-operations:task-update` sets a task to completed, it runs `.codeflow/scripts/db/unblock-dependents.sh` to update any tasks that were blocked by it.

## Querying Tasks by Status

Use `cf-task-management:query-tasks` with status filter:

- `status: pending` - Tasks ready to start
- `status: in_progress` - Active work
- `status: blocked` - Waiting on dependencies
- `status: completed` - Finished work

## Parallel Work Notes

- active-task.json includes session_id for per-session isolation
- Multiple Claude Code sessions on same machine: each has own active-task context
- Multi-user/multi-machine: .state/ is gitignored, no cross-machine conflicts
- Worktrees: each worktree has own .state/ directory
