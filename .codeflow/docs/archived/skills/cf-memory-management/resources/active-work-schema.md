# Active Work Schema Reference

## active-task.json (Work State File)

The primary work tracking state file at `.state/runtime/active-task.json`, managed by cf-work-state.sh.

| Field | Type | Description |
|-------|------|-------------|
| task_id | string | ULID PK of parent task (e.g., `task-01JQ3KN2X9...`) |
| epic_id | string | ULID PK of parent epic (e.g., `epic-01JQ3KM7V8...`) |
| task_format_id | string | Human-readable task ID (e.g., `BKD-TSK-FEAT-API-012`) |
| epic_format_id | string | Human-readable epic ID (e.g., `BKD-EPC-FEAT-API-001`) |
| title | string | Human-readable task description |
| status | string | pending, in_progress, completed |
| branch | string | Git branch name |
| session_id | string | Current session ID (for per-session isolation) |
| started_at | string | ISO 8601 timestamp |
| updated_at | string | ISO 8601 timestamp |

### Parallel Work Isolation

- Each Claude Code session gets its own active-task.json via session_id
- Multi-user/multi-machine: .state/ is gitignored, no cross-machine conflicts
- Worktrees: each worktree has own .state/ directory
- cf-work-state.sh uses flock for atomicity on concurrent writes

## What is Active Work?

The `active_work` table tracks currently in-progress work for:

- Context recovery across sessions
- Scope validation during edits
- Progress tracking

## Schema Overview

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `work-{ulid}` format |
| task_id | TEXT | ULID PK of parent task (`task-{ulid}`) |
| topic | TEXT | Human-readable description |
| branch | TEXT | Git branch name |
| worktree_path | TEXT | Path if using worktree |
| file_scope | JSON | Array of allowed file patterns |
| status | TEXT | in_progress, paused, completed |
| started_at | TEXT | ISO 8601 timestamp |
| last_progress_at | TEXT | Last progress event |
| completed_at | TEXT | Completion timestamp |
| session_id | TEXT | Current session ID |

## Work ID Format

```text
work-{ulid}
```

Generated automatically by `cf-memory-management:begin-work`.

## Status Values

| Status | Description | Operations Allowed |
|--------|-------------|-------------------|
| in_progress | Actively being worked | Edit, progress, complete |
| paused | Temporarily stopped | Resume, complete |
| completed | Work finished | (read-only) |

## Operations

**Never modify active_work directly.** Use these operations:

### Starting Work

```text
cf-memory-management:begin-work
→ Creates active_work entry
→ Links to task
→ Sets initial scope
```

### Recording Progress

```text
cf-memory-management:record-work-progress
→ Updates last_progress_at
→ Creates memory event
```

### Completing Work

```text
cf-memory-management:complete-work
→ Sets status to completed
→ Archives context
→ Creates commit sentinel
```

### Finding Active Work

```text
cf-memory-management:detect-active-work
→ Returns in_progress work for session
→ Suggests resume options
```

### Loading Context

```text
cf-memory-management:load-work-context
→ Restores full work context
→ Loads recent progress events
```

## File Scope

The `file_scope` field contains a JSON array of glob patterns:

```json
["src/auth/**/*.ts", "tests/auth/**/*.test.ts"]
```

See [scope-patterns.md](scope-patterns.md) for pattern syntax.

## Relationships

```text
active_work
    │
    ├──▶ tasks (via task_id)
    │       └──▶ epics
    │
    └──▶ memory_events (via work_id)
```
