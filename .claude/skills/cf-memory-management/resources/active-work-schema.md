# Active Work Schema Reference

## What is Active Work?

The `active_work` table tracks currently in-progress work for:

- Context recovery across sessions
- Scope validation during edits
- Progress tracking

## Schema Overview

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `work-{ulid}` format |
| task_id | TEXT | Link to parent task |
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
