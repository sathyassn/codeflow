---
name: cf-db-operations
description: Provides database CRUD operations for epics, tasks, memory, sessions, and logs. Enforces agent permissions and maintains three-tier data consistency. Use when persisting or querying CodeFlow data.
context: fork
agent: cf-general-purpose
---

# Database Operations Skill

## Type

**Procedural** - Provides step-by-step database operations with permission enforcement.

## Purpose

**Provides validated database operations that maintain consistency across SQLite, JSONL ledger, and markdown files.**

## Responsibilities

- Execute epic CRUD operations (cf-planner only)
- Execute task CRUD operations (with agent permissions)
- Store and query memory events by domain
- Record session lifecycle events
- Append to security/network/conversation logs
- NOT: Business logic (that's cf-task-management, cf-memory-management)
- NOT: Schema management (that's migrations)

## Decision Tree

```text
Need to persist epic?
├── Creating new → 🔧 epic-create (cf-planner only)
└── Updating existing → 🔧 epic-update (cf-planner only)

Need to persist task?
├── Creating new → 🔧 task-create
└── Updating existing → 🔧 task-update

Need to persist memory?
├── Storing event → 🔧 memory-store (own domain only)
└── Querying events → 🔧 memory-query

Need to track active work?
├── Creating → 🔧 active-work-create
├── Updating → 🔧 active-work-update
└── Querying → 🔧 active-work-query

Session lifecycle?
└── 🔧 session-record

Autorun operations?
├── Creating session → 🔧 autorun-session-create (cf-planner/system)
├── Creating worker → 🔧 autorun-worker-create (system only)
├── Updating session → 🔧 autorun-session-update
└── Recording task run → 🔧 autorun-task-run-record

Logging event?
└── 🔧 log-append (security/network/conversation)
```

## Operations

**Enforcement note:** cf-db-operations is an internal data-access layer. No PreToolUse hook
gates database operations directly. Write operations are enforced at the calling skill level
(e.g., memory-management, task-management). The enforcement column below reflects the
calling skill's enforcement, not a direct hook on db-operations.

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | epic-create | None (internal) | Insert epic into work graph |
| 2 | epic-update | None (internal) | Update epic status/content |
| 3 | task-create | None (internal) | Insert task linked to epic |
| 4 | task-update | None (internal) | Update task status/assignee |
| 5 | memory-store | None (internal) | Store memory entry |
| 6 | memory-query | None | Query memory by domain/topic |
| 7 | active-work-create | None (internal) | Register active work for tracking |
| 8 | active-work-update | None (internal) | Update active work status |
| 9 | active-work-query | None | Query active work by criteria |
| 10 | session-record | None (internal) | Record session lifecycle event |
| 11 | log-append | None (internal) | Append to security/network/conversation logs |
| 12 | autorun-session-create | None (internal) | Create autorun batch session |
| 13 | autorun-worker-create | None (internal) | Create worker for task execution |
| 14 | autorun-session-update | None (internal) | Update autorun session status |
| 15 | autorun-task-run-record | None (internal) | Record task execution results |

## Operation Details

### 🔧 epic-create

```text
When: /cf-plan creates new epic
Enforcement: None (internal data layer; called by cf-task-management:create-epic)
Agent: cf-planner only

Procedure:
  1. Validate required fields (area, type, domain, title)
  2. Generate both IDs:
     - id (ULID PK): epic-{ulid} (auto-generated)
     - format_id: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN} (auto-generated)
  3. Execute INSERT into epics table (both id and format_id)
  4. Append to JSONL ledger
  5. Return both IDs

DB Tables: epics
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding epic table structure or required fields
```

### 🔧 epic-update

```text
When: Epic status/priority changes or content updates
Enforcement: None (internal data layer; called by cf-task-management:update-epic)
Agent: cf-planner only

Procedure:
  1. Validate epic exists
  2. Validate agent has permission for fields
  3. Execute UPDATE on epics table
  4. Append to JSONL ledger
  5. Trigger markdown re-render if content changed
  6. Check if status change affects child tasks

DB Tables: epics
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [schema-reference.md](resources/schema-reference.md) - Load when: Understanding epic fields
   [agent-permissions.md](resources/agent-permissions.md) - Load when: Validating field permissions
```

### 🔧 task-create

```text
When: Creating new task linked to epic
Enforcement: None (internal data layer; called by cf-task-management:create-task)
Agent: cf-planner (formal), Main Agent (informal via ensure-work-registered)

Procedure:
  1. Validate epic exists and is active (by ULID PK)
  2. Generate both IDs:
     - id (ULID PK): task-{ulid} (auto-generated)
     - format_id: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN} (auto-generated)
  3. Execute INSERT into tasks table (both id and format_id, epic_id as ULID PK)
  4. Create task dependencies if specified
  5. Append to JSONL ledger
  6. Create markdown file

DB Tables: tasks, task_dependencies
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding task table structure or dependency handling
```

### 🔧 task-update

```text
When: Task status changes (dev, review, test)
Enforcement: None (internal data layer; called by cf-task-management:update-task)
Agent: cf-planner (all fields), cf-developer/cf-reviewer/cf-qa (status only)

Procedure:
  1. Validate agent has permission for field
  2. Execute UPDATE on tasks table
  3. Append to JSONL ledger
  4. Check if status change unblocks other tasks
  5. Trigger markdown re-render if needed

DB Tables: tasks, task_dependencies
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [schema-reference.md](resources/schema-reference.md) - Load when: Understanding updatable fields
   [agent-permissions.md](resources/agent-permissions.md) - Load when: Validating agent permissions
```

### 🔧 memory-store

```text
When: Recording work progress, decisions, or context
Enforcement: None (internal data layer; called by cf-memory-management operations)
Agent: All (to their own domain only)

Procedure:
  1. Validate domain matches agent's assigned domain
  2. Generate entry ID: memory-{ulid}
  3. Build JSON payload for data field
  4. Execute INSERT into memory_events
  5. Queue for entity extraction
  6. Append to JSONL ledger

DB Tables: memory_events, extraction_queue
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [schema-reference.md](resources/schema-reference.md) - Load when: Understanding memory event structure
   [agent-permissions.md](resources/agent-permissions.md) - Load when: Validating domain access
```

### 🔧 memory-query

```text
When: Searching for context, decisions, or related work
Enforcement: None (read-only)
Agent: All agents

Query Parameters:
  | Parameter | Type | Purpose |
  |-----------|------|---------|
  | domain | string | Filter by memory domain |
  | event_type | string | milestone, decision, progress, blocker |
  | work_id | string | Events for specific work |
  | search_term | string | Full-text search via FTS5 |
  | memory_type | string | episodic, semantic, procedural |
  | limit | integer | Max results (default: 50) |
  | offset | integer | Pagination offset |

Procedure:
  1. Build query from parameters
  2. For keyword search, use FTS5 with BM25 ranking
  3. For filtered query, use indexed columns
  4. Return structured results with JSON-parsed data

DB Tables: memory_events, memory_fts, active_work
Executed by: codeflow db query (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resources:
   [query-templates.md](resources/query-templates.md) - Load when: Building complex memory queries
   [schema-reference.md](resources/schema-reference.md) - Load when: Understanding FTS5 index
```

### 🔧 session-record

```text
When: Session lifecycle events (start, pause, resume, end)
Enforcement: None (internal data layer; called by session hooks)
Agent: System (automatic via hooks)

Event Types:
  | Event | Trigger |
  |-------|---------|
  | session_start | Claude Code init |
  | session_pause | Context rotation |
  | session_resume | /cf-resume invoked |
  | session_end | Stop hook |

Procedure:
  1. Determine event type
  2. Record session event in sessions table
  3. On session_end, update duration
  4. Append to JSONL ledger

DB Tables: sessions
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)
Hook: stop/stop-session-record.sh

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding session event types or duration tracking
```

### 🔧 log-append

```text
When: Security event, network call, or conversation turn
Enforcement: None (internal data layer; called by logging hooks)
Agent: System (via hooks)

Log Types:
  | Type | Table | Purpose |
  |------|-------|---------|
  | security | security_logs | Protection, blocked, sentinel events |
  | network | network_logs | WebFetch, curl, git push/pull |
  | conversation | conversation_logs | Turn tracking for context recovery |

Procedure:
  1. Determine log type
  2. Validate required fields per log type
  3. Execute INSERT via appropriate query
  4. Also append to daily JSONL file

DB Tables: security_logs, network_logs, conversation_logs
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding log table structures or required fields
```

### 🔧 active-work-create

```text
When: Starting work on a task (called by cf-memory-management:begin-work)
Enforcement: None (internal data layer; called by cf-memory-management:begin-work)
Agent: All agents (own domain)

Procedure:
  1. Validate task exists and is actionable
  2. Generate work ID: work-{ulid}
  3. Execute INSERT into active_work table
  4. Append to JSONL ledger (Tier 0)
  5. Return work_id

DB Tables: active_work
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding active_work table structure
```

### 🔧 active-work-update

```text
When: Updating active work status (progress, completion)
Enforcement: None (internal data layer; called by cf-memory-management operations)
Agent: All agents (own work only)

Procedure:
  1. Validate work_id exists and belongs to agent
  2. Execute UPDATE on active_work table
  3. Append to JSONL ledger (Tier 0)
  4. If status='completed', trigger cleanup

DB Tables: active_work
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [schema-reference.md](resources/schema-reference.md)
   Load when: Understanding active work status values
```

### 🔧 active-work-query

```text
When: Querying active work (session start, conflict check)
Enforcement: None (read-only)
Agent: All agents

Query Parameters:
  | Parameter | Type | Purpose |
  |-----------|------|---------|
  | status | string | Filter by active/paused/completed |
  | branch | string | Filter by branch name |
  | task_id | string | Filter by associated task |
  | agent | string | Filter by agent |

Procedure:
  1. Build query from parameters
  2. Execute SELECT on active_work
  3. Return structured results

DB Tables: active_work
Executed by: codeflow db query (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [query-templates.md](resources/query-templates.md)
   Load when: Building complex active work queries
```

### 🔧 autorun-session-create

```text
When: Starting autorun batch execution
Enforcement: None (internal data layer; called by Go CLI orchestrator)
Agent: cf-planner (manual), System (CLI orchestrator)

Required Fields:
  - batch_file: Path to batch configuration
  - batch_name: Human-readable batch name
  - max_session_workers: Concurrent worker limit (default: 3)
  - total_tasks: Number of tasks in batch

Procedure:
  1. Generate session ID: autorun-{ulid}
  2. Validate batch configuration exists
  3. Execute INSERT into autorun_sessions
  4. Append to JSONL ledger
  5. Return session_id

DB Tables: autorun_sessions
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [autorun-guide.md](resources/autorun-guide.md)
   Load when: Understanding autorun batch session structure
```

### 🔧 autorun-worker-create

```text
When: Spawning worker for task execution
Enforcement: None (internal data layer; called by Go CLI orchestrator)
Agent: System only (Go CLI orchestrator)

Procedure:
  1. Validate session exists and is active
  2. Check worker count < max_session_workers
  3. Generate worker ID: worker-{ulid}
  4. Execute INSERT into autorun_workers
  5. Append to JSONL ledger
  6. Return worker_id

DB Tables: autorun_workers
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [autorun-guide.md](resources/autorun-guide.md)
   Load when: Understanding worker lifecycle or max worker limits
```

### 🔧 autorun-session-update

```text
When: Updating autorun session status
Enforcement: None (internal data layer; called by Go CLI orchestrator)
Agent: cf-planner, System

Updatable Fields:
  | Field | Purpose |
  |-------|---------|
  | status | pending → running → completed/failed |
  | completed_tasks | Increment on task completion |
  | failed_tasks | Increment on task failure |
  | ended_at | Set on completion |

Procedure:
  1. Validate session exists
  2. Execute UPDATE on autorun_sessions
  3. Append to JSONL ledger
  4. If all tasks done, set status='completed'

DB Tables: autorun_sessions
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [autorun-guide.md](resources/autorun-guide.md)
   Load when: Understanding session status transitions
```

### 🔧 autorun-task-run-record

```text
When: Recording task execution result
Enforcement: None (internal data layer; called by Go CLI orchestrator)
Agent: System only

Procedure:
  1. Validate worker and task exist
  2. Execute INSERT into autorun_task_runs
  3. Update worker status
  4. Update session completed/failed counts
  5. Append to JSONL ledger

DB Tables: autorun_task_runs, autorun_workers, autorun_sessions
Executed by: codeflow db exec (Go CLI; see .codeflow/scripts/db/schema.sql for schema)

📚 Resource: [autorun-guide.md](resources/autorun-guide.md)
   Load when: Recording task run results or understanding run outcomes
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [schema-reference.md](resources/schema-reference.md) | Full schema documentation | When debugging or extending |
| [query-templates.md](resources/query-templates.md) | SQL query templates | When building complex queries |
| [agent-permissions.md](resources/agent-permissions.md) | Permission matrix | When validating access |
| [autorun-guide.md](resources/autorun-guide.md) | Autorun architecture details | When working with autorun |

**Note:** All database queries are internal to the Go CLI (`codeflow` binary).
There are no separate .sql query files at runtime. See `.codeflow/scripts/db/schema.sql`
for the schema definition.
