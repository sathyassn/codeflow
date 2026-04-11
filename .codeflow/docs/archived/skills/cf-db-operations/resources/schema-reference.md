# Schema Reference

## Overview

CodeFlow uses a three-tier data consistency model:

1. **Tier 0: JSONL Ledger** - Append-only audit trail, source of truth for rebuilds
2. **Tier 1: SurrealDB** - Fast queries, indexed access, rebuildable from ledger
3. **Tier 2: Markdown** - Human-readable, version controlled

All writes must update all tiers atomically via cf-db-operations.

## Work Graph Tables

### epics

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | TEXT | PK | ULID PK: `epic-{ulid}` |
| format_id | TEXT | UNIQUE NOT NULL | Human-readable: `{AREA}-EPC-{NNN}` |
| title | TEXT | NOT NULL | Epic title |
| summary | TEXT | | One-line summary |
| status | TEXT | 'draft' | draft\|planning\|in_progress\|blocked\|complete\|archived |
| area_type | TEXT | NOT NULL | FRT\|BKD\|INF\|SHR\|DOC\|PLN |
| work_type | TEXT | NOT NULL | FEAT\|FIX\|HTFX\|RFCT\|DOCS\|TEST\|CHOR\|CICD\|SPKE |
| domain | TEXT | NOT NULL | GENL (reserved) or project domain |
| priority | TEXT | 'normal' | low\|normal\|high\|critical |
| is_ongoing | BOOLEAN | FALSE | True for informal work containers |
| file_scope | TEXT | | JSON array of file patterns |
| pr_number | INTEGER | | Associated PR number |
| external_id | TEXT | | External tracker ID |
| external_url | TEXT | | External tracker URL |
| created_at | TEXT | CURRENT_TIMESTAMP | ISO 8601 timestamp |
| updated_at | TEXT | CURRENT_TIMESTAMP | Last modification |

### tasks

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | TEXT | PK | ULID PK: `task-{ulid}` |
| format_id | TEXT | UNIQUE NOT NULL | Human-readable: `{AREA}-TSK-{NNN}-{NNN}` |
| epic_id | TEXT | NOT NULL FK | Parent epic ULID PK reference |
| title | TEXT | NOT NULL | Task title |
| description | TEXT | | One-line description |
| status | TEXT | 'todo' | todo\|blocked\|in_progress\|complete |
| area_type | TEXT | NOT NULL | FRT\|BKD\|INF\|SHR\|DOC\|PLN |
| work_type | TEXT | NOT NULL | FEAT\|FIX\|HTFX\|RFCT\|DOCS\|TEST\|CHOR\|CICD\|SPKE |
| domain | TEXT | NOT NULL | Task domain |
| origin | TEXT | 'planned' | planned\|informal\|auto |
| file_scope | TEXT | | JSON array of file patterns |
| scope_policy | TEXT | 'soft' | soft\|hard\|permissive |
| scope_root | TEXT | | Root directory for scope |
| estimate | TEXT | | XS\|S\|M\|L\|XL |
| priority | TEXT | 'normal' | low\|normal\|high\|critical |
| assignee_id | TEXT | FK | User ID of assignee |
| autorun_eligible | BOOLEAN | FALSE | Can run in autorun mode |
| auto_commit | BOOLEAN | TRUE | Auto-commit on completion |
| raise_pr | BOOLEAN | TRUE | Create PR on completion |
| auto_merge | BOOLEAN | FALSE | Auto-merge after verification |
| target_branch | TEXT | | Branch to merge PR into |
| acceptance | TEXT | | JSON array of acceptance criteria |
| tests | TEXT | | JSON array of test commands |
| branch | TEXT | | Git branch name |
| pr_number | INTEGER | | Associated PR number |
| external_id | TEXT | | External tracker ID |
| external_url | TEXT | | External tracker URL |
| created_at | TEXT | CURRENT_TIMESTAMP | Creation timestamp |
| updated_at | TEXT | CURRENT_TIMESTAMP | Last modification |
| started_at | TEXT | | Work started timestamp |
| completed_at | TEXT | | Completion timestamp |

### task_dependencies

| Field | Type | Description |
|-------|------|-------------|
| task_id | TEXT | Task that is blocked |
| depends_on_id | TEXT | Task that blocks |
| dependency_type | TEXT | blocked_by\|related |

### acceptance_criteria

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `ac-{ulid}` |
| epic_id | TEXT | Parent epic |
| criterion | TEXT | Criterion text |
| met | BOOLEAN | Whether satisfied |
| met_at | TEXT | When satisfied |
| met_by | TEXT | Task ID that satisfied |

## Active Work Tables

### active_work

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | TEXT | PK | `work-{ulid}` |
| task_id | TEXT | FK | Parent task reference (optional) |
| topic | TEXT | NOT NULL | Work description |
| status | TEXT | 'in_progress' | in_progress\|complete\|blocked |
| branch | TEXT | | Git branch |
| scope | TEXT | | JSON array of file patterns |
| deliverables | TEXT | | JSON array of deliverables |
| agent | TEXT | | Agent name |
| session_id | TEXT | FK | Current session |
| created_at | TEXT | CURRENT_TIMESTAMP | Start timestamp |
| updated_at | TEXT | CURRENT_TIMESTAMP | Last update |

### work_claims

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `claim-{ulid}` |
| work_id | TEXT | Parent work reference |
| pattern | TEXT | File pattern claimed |
| mode | TEXT | exclusive\|shared |
| owner_id | TEXT | User ID of owner |
| owner_host | TEXT | Host machine |
| fencing_token | INTEGER | Monotonic counter for conflicts |
| expires_at | TEXT | Expiration timestamp |
| status | TEXT | active\|released\|contested\|expired |

## Memory Tables

### memory_events

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | TEXT | PK | `memory-{ulid}` |
| event_type | TEXT | NOT NULL | progress\|decision\|milestone\|blocker |
| domain | TEXT | NOT NULL | planning\|development\|review\|qa\|ops\|documentation |
| work_id | TEXT | | Parent work reference |
| data | TEXT | NOT NULL | JSON payload |
| memory_type | TEXT | | episodic\|semantic\|procedural |
| created_at | TEXT | CURRENT_TIMESTAMP | Event timestamp |

### entities

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `entity-{ulid}` |
| name | TEXT | Entity name |
| entity_type | TEXT | person\|component\|file\|concept\|decision\|library |
| source_event_id | TEXT | Source memory event |
| metadata | TEXT | JSON metadata |

### relationships

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `rel-{ulid}` |
| source_entity_id | TEXT | Source entity |
| target_entity_id | TEXT | Target entity |
| relationship_type | TEXT | works_on\|depends_on\|uses\|references\|implements |
| source_event_id | TEXT | Source memory event |

## Session Tables

### sessions

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `session-{ulid}` |
| project_id | TEXT | Project reference |
| user_id | TEXT | User who created session |
| user_host | TEXT | Host machine |
| machine_fingerprint | TEXT | Machine identifier |
| started_at | TEXT | Start timestamp |
| ended_at | TEXT | End timestamp |
| duration_seconds | INTEGER | Session duration |
| status | TEXT | active\|paused\|completed\|crashed\|archived |
| work_ids | TEXT | JSON array of work IDs |
| previous_session_id | TEXT | Previous session for resume |
| context_summary | TEXT | Context for resume |

## Autorun Tables

### autorun_sessions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | TEXT | PK | `autorun-{ulid}` |
| batch_file | TEXT | NOT NULL | Path to batch file |
| batch_name | TEXT | | Human-readable name |
| status | TEXT | 'pending' | pending\|running\|paused\|completed\|failed\|cancelled\|timeout |
| max_session_workers | INTEGER | 3 | Max concurrent workers |
| total_tasks | INTEGER | 0 | Total tasks in batch |
| completed_tasks | INTEGER | 0 | Completed count |
| failed_tasks | INTEGER | 0 | Failed count |

### autorun_workers

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `{session_id}-w-{NNN}` |
| session_id | TEXT | Parent autorun session |
| worker_num | INTEGER | Worker number |
| task_id | TEXT | Task being executed |
| status | TEXT | queued\|starting\|running\|completed\|failed\|skipped\|timeout\|cancelled |
| tmux_session | TEXT | Tmux session name |
| worktree_path | TEXT | Git worktree path |
| pr_number | INTEGER | Created PR number |

### autorun_task_runs

| Field | Type | Description |
|-------|------|-------------|
| id | TEXT | `taskrun-{ulid}` |
| worker_id | TEXT | Worker that executed |
| task_id | TEXT | Task executed |
| session_id | TEXT | Parent session |
| status | TEXT | pending\|running\|completed\|failed\|skipped\|timeout\|cancelled |
| branch_name | TEXT | Git branch |
| pr_number | INTEGER | PR number |
| pr_url | TEXT | PR URL |
| duration_seconds | INTEGER | Execution duration |
| exit_code | INTEGER | Exit code |
| error_message | TEXT | Error if failed |
| verification_result | TEXT | JSON: {decision, reason, criteria_met} |

## Accessing Data

**Never query SurrealDB directly.** Use the appropriate operations:

| Need | Operation |
|------|-----------|
| Create epic | `cf-db-operations:epic-create` |
| Update epic | `cf-db-operations:epic-update` |
| Create task | `cf-db-operations:task-create` |
| Update task | `cf-db-operations:task-update` |
| Store memory | `cf-db-operations:memory-store` |
| Query memory | `cf-db-operations:memory-query` |
| Record session | `cf-db-operations:session-record` |
| Append log | `cf-db-operations:log-append` |

## Area Folder Mapping

Reference mapping from area codes to filesystem folder names:

| Area Code | Folder Name | Used In |
|-----------|-------------|---------|
| FRT | FRT/ | project-management/epics/FRT/ |
| BKD | BKD/ | project-management/epics/BKD/ |
| INF | INF/ | project-management/epics/INF/ |
| SHR | SHR/ | project-management/epics/SHR/ |
| DOC | DOC/ | project-management/epics/DOC/ |
| PLN | PLN/ | project-management/epics/PLN/ |

Area code IS the folder name. This mapping is used by the Go CLI and cf-work-state.sh
when resolving epic/task paths under `project-management/epics/`.

## Schema Location

- **Full schema**: `.codeflow/scripts/db/schema.sql` (source of truth; embedded by Go CLI at build time)
- **Query execution**: Internal to Go CLI (`codeflow db query/exec`)
- **Database file**: `.state/db/codeflow.db`
