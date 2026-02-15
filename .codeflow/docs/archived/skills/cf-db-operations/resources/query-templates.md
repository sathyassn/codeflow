# Query Templates Reference

## Overview

**Do not write SQL queries directly.** All database access goes through the Go CLI (`codeflow db exec/query`). Queries are internal to the Go CLI binary, not stored as separate .sql files.

This reference documents the query patterns used internally by the Go CLI, for understanding only.

## Query Execution

All queries are compiled into the Go CLI (`codeflow` binary). There are no separate `.sql` query files at runtime. The schema is defined in `.codeflow/scripts/db/schema.sql`.

| Domain | Go CLI Command | Purpose |
|--------|----------------|---------|
| Epics | `codeflow db exec/query` | Epic CRUD operations |
| Tasks | `codeflow db exec/query` | Task CRUD operations |
| Memory | `codeflow db exec/query` | Memory event operations |
| Sessions | `codeflow db exec/query` | Session lifecycle operations |
| Logs | `codeflow db exec/query` | Audit log operations |
| Autorun | `codeflow db exec/query` | Autorun session operations |

## Epic Operations

### epic-create

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — create)
INSERT INTO epics (
    id, format_id, title, summary, status, area_type, work_type, domain,
    priority, is_ongoing, file_scope, created_at, updated_at
) VALUES (
    :id, :format_id, :title, :summary, :status, :area_type, :work_type, :domain,
    :priority, :is_ongoing, :file_scope, :created_at, :updated_at
);
```

- Generates ULID PK automatically: `epic-{ulid}`
- Generates format_id automatically: `{AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}`
- Appends to JSONL ledger
- Creates markdown file in `project-management/epics/{area-folder}/`

### epic-update

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — update)
UPDATE epics SET
    title = COALESCE(:title, title),
    summary = COALESCE(:summary, summary),
    status = COALESCE(:status, status),
    priority = COALESCE(:priority, priority),
    file_scope = COALESCE(:file_scope, file_scope),
    updated_at = :updated_at
WHERE id = :id;
```

- Validates agent permissions
- Updates all three tiers
- Re-renders markdown if content changed

## Task Operations

### task-create

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — create)
INSERT INTO tasks (
    id, format_id, epic_id, title, description, status, area_type, work_type, domain,
    origin, file_scope, scope_policy, scope_root, estimate, priority,
    assignee_id, autorun_eligible, auto_commit, raise_pr, auto_merge,
    target_branch, acceptance, tests, created_at, updated_at
) VALUES (
    :id, :format_id, :epic_id, :title, :description, :status, :area_type, :work_type, :domain,
    :origin, :file_scope, :scope_policy, :scope_root, :estimate, :priority,
    :assignee_id, :autorun_eligible, :auto_commit, :raise_pr, :auto_merge,
    :target_branch, :acceptance, :tests, :created_at, :updated_at
);
```

- Generates ULID PK automatically: `task-{ulid}`
- Generates format_id automatically: `{AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}`
- `epic_id` references parent epic ULID PK
- Links to parent epic
- Creates markdown file in `project-management/epics/{area-folder}/{epic}/tasks/`
- Appends to JSONL ledger

### task-update

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — update)
UPDATE tasks SET
    status = COALESCE(:status, status),
    assignee_id = COALESCE(:assignee_id, assignee_id),
    branch = COALESCE(:branch, branch),
    pr_number = COALESCE(:pr_number, pr_number),
    started_at = COALESCE(:started_at, started_at),
    completed_at = COALESCE(:completed_at, completed_at),
    updated_at = :updated_at
WHERE id = :id;
```

- Validates agent permissions per field
- Runs unblock-dependents check on completion
- Updates all three tiers

### Lookup by format_id

```sql
-- Executed by: codeflow db query (Go CLI internal — lookup by format_id)
SELECT * FROM epics WHERE format_id = :format_id;
SELECT * FROM tasks WHERE format_id = :format_id;
```

- Use when resolving a human-readable format_id to its ULID PK
- Returns full row including `id` (ULID PK) for FK operations

### task-ready (autorun)

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — ready-for-autorun)
SELECT t.* FROM tasks t
WHERE t.epic_id = :epic_id
  AND t.status = 'todo'
  AND t.autorun_eligible = TRUE
  AND NOT EXISTS (
      SELECT 1 FROM task_dependencies td
      JOIN tasks blocker ON td.depends_on_id = blocker.id
      WHERE td.task_id = t.id
        AND blocker.status != 'complete'
  )
ORDER BY t.priority DESC, t.created_at;
```

## Memory Operations

### memory-store

```sql
-- Executed by: codeflow db exec (Go CLI internal query)
INSERT INTO memory_events (
    id, event_type, domain, work_id, data, memory_type, created_at
) VALUES (
    :id, :event_type, :domain, :work_id, :data, :memory_type, :created_at
);

-- Also queue for entity extraction
INSERT INTO extraction_queue (id, event_id, status, priority, created_at)
VALUES (:extraction_id, :id, 'pending', :priority, :created_at);
```

- Validates domain ownership (agents can only write to their domain)
- Generates memory ID: `memory-{ulid}`
- Updates FTS index automatically (via trigger)
- Appends to JSONL ledger

### memory-query

```sql
-- Executed by: codeflow db query (Go CLI internal query)
SELECT * FROM memory_events
WHERE domain = :domain
ORDER BY created_at DESC
LIMIT :limit OFFSET :offset;

-- Executed by: codeflow db query (Go CLI internal query)
SELECT * FROM memory_events
WHERE work_id = :work_id
ORDER BY created_at DESC;

-- Executed by: codeflow db query (Go CLI internal FTS query)
SELECT me.*, bm25(memory_fts) as score
FROM memory_fts
JOIN memory_events me ON memory_fts.id = me.id
WHERE memory_fts MATCH :search_term
ORDER BY score
LIMIT :limit;
```

Supports multiple query modes:

- By domain
- By event type
- By work ID
- Full-text search (FTS5)

## Session Operations

### session-record

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — start)
INSERT INTO sessions (
    id, project_id, user_id, user_host, machine_fingerprint,
    started_at, status, previous_session_id
) VALUES (
    :id, :project_id, :user_id, :user_host, :machine_fingerprint,
    :started_at, 'active', :previous_session_id
);

-- Executed by: codeflow db exec/query (Go CLI internal — end)
UPDATE sessions SET
    ended_at = :ended_at,
    duration_seconds = :duration_seconds,
    status = :status,
    context_summary = :context_summary
WHERE id = :id;
```

- Handles start, pause, resume, end events
- Calculates duration on end
- Appends to JSONL ledger

## Log Operations

### log-append

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — security)
INSERT INTO security_logs (
    id, log_type, event_type, tool, target, reason,
    skill, operation, outcome, session_id, created_at
) VALUES (
    :id, :log_type, :event_type, :tool, :target, :reason,
    :skill, :operation, :outcome, :session_id, :created_at
);

-- Executed by: codeflow db exec/query (Go CLI internal — network)
INSERT INTO network_logs (
    id, operation, url, domain, method, status_code,
    response_size, duration_ms, purpose, work_id, session_id, created_at
) VALUES (
    :id, :operation, :url, :domain, :method, :status_code,
    :response_size, :duration_ms, :purpose, :work_id, :session_id, :created_at
);

-- Executed by: codeflow db exec/query (Go CLI internal — conversation)
INSERT INTO conversation_logs (
    id, role, content, tool_calls, turn_number, session_id, created_at
) VALUES (
    :id, :role, :content, :tool_calls, :turn_number, :session_id, :created_at
);
```

- Supports security, network, conversation logs
- Validates required fields per log type
- Also writes to daily JSONL file

## Autorun Operations

### autorun-session-create

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — session-create)
INSERT INTO autorun_sessions (
    id, batch_file, batch_name, status, max_session_workers,
    total_tasks, created_at
) VALUES (
    :id, :batch_file, :batch_name, 'pending', :max_session_workers,
    :total_tasks, :created_at
);
```

### autorun-worker-create

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — worker-create)
INSERT INTO autorun_workers (
    id, session_id, worker_num, task_id, status, created_at
) VALUES (
    :id, :session_id, :worker_num, :task_id, 'queued', :created_at
);
```

### autorun-session-status

```sql
-- Executed by: codeflow db exec/query (Go CLI internal — session-status)
SELECT
    ars.*,
    COUNT(arw.id) as total_workers,
    SUM(CASE WHEN arw.status = 'completed' THEN 1 ELSE 0 END) as completed_workers,
    SUM(CASE WHEN arw.status = 'failed' THEN 1 ELSE 0 END) as failed_workers,
    SUM(CASE WHEN arw.status = 'running' THEN 1 ELSE 0 END) as running_workers
FROM autorun_sessions ars
LEFT JOIN autorun_workers arw ON arw.session_id = ars.id
WHERE ars.id = :session_id
GROUP BY ars.id;
```

## Tracking Operations

### tracking-generate (master epic tracker)

```sql
-- Executed by: cf-work-state.sh or tracking scripts
-- Generates master epic tracker view at project-management/tracking/
SELECT e.id, e.title, e.status, e.area_type, e.work_type, e.priority,
       COUNT(t.id) as task_count,
       SUM(CASE WHEN t.status = 'complete' THEN 1 ELSE 0 END) as completed_tasks
FROM epics e
LEFT JOIN tasks t ON t.epic_id = e.id
GROUP BY e.id
ORDER BY e.area_type, e.work_type, e.created_at;
```

- Generates auto-updated views in `project-management/tracking/`
- Used for master epic tracker and area-level summaries

## Query Parameters

When using `cf-db-operations:memory-query`:

| Parameter | Type | Description |
|-----------|------|-------------|
| domain | string | Filter by memory domain |
| event_type | string | milestone\|decision\|progress\|blocker |
| work_id | string | Events for specific work |
| search_term | string | Full-text search |
| memory_type | string | episodic\|semantic\|procedural |
| limit | integer | Max results (default 50) |
| offset | integer | Pagination offset |

## Three-Tier Update Pattern

All write operations follow this pattern:

1. **Validate** - Check permissions and constraints
2. **SQLite** - Execute INSERT/UPDATE
3. **JSONL** - Append event to appropriate ledger file
4. **Markdown** - Create/update markdown file (if applicable)
5. **Return** - Return structured result (JSON)

Failure at any step triggers rollback of previous steps.
