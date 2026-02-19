# Work Item Templates

Standardized templates for epics and tasks. YAML frontmatter fields align 1:1 with the database schema (`schema.sql` epics and tasks tables).

## Templates

| Template | DB Table | Fields | Purpose |
|----------|----------|--------|---------|
| `epic-template.md` | `epics` | 16 | Epic definition with scope, criteria, and task table |
| `task-template.md` | `tasks` | 34 + 1 | Task definition with approach, files, and verification |

The task template includes 34 DB-aligned fields plus `epic_format_id` (a convenience cross-reference not yet in the DB tasks table; will be added in a future schema migration).

## Format ID Convention

IDs use the pattern `{AREA}-{TYPE}-{NNN}` where area is the only hierarchical component:

| Type | Pattern | Example |
|------|---------|---------|
| Epic | `{AREA}-EPC-{NNN}` | `INF-EPC-001` |
| Task | `{AREA}-TSK-{NNN}-{NNN}` | `INF-TSK-001-001` |

The first `{NNN}` in a task ID corresponds to the parent epic number. The second `{NNN}` is the task sequence within that epic.

### Area Codes

| Code | Scope |
|------|-------|
| FRT | Frontend -- UI, components, client logic |
| BKD | Backend -- API, services, server logic |
| INF | Infrastructure -- CI/CD, deployment, DevOps |
| SHR | Shared -- Common libraries, types |
| DOC | Documentation -- Docs, guides, ADRs |
| PLN | Planning -- Design, architecture, analysis |

### Format ID Examples

| Area | Epic Example | Task Examples |
|------|-------------|---------------|
| FRT | `FRT-EPC-001` | `FRT-TSK-001-001`, `FRT-TSK-001-002` |
| BKD | `BKD-EPC-003` | `BKD-TSK-003-001`, `BKD-TSK-003-002` |
| INF | `INF-EPC-012` | `INF-TSK-012-001`, `INF-TSK-012-005` |
| SHR | `SHR-EPC-002` | `SHR-TSK-002-001` |
| DOC | `DOC-EPC-001` | `DOC-TSK-001-001`, `DOC-TSK-001-003` |
| PLN | `PLN-EPC-005` | `PLN-TSK-005-001`, `PLN-TSK-005-002` |

## Field Reference

### Epic Fields (16)

| Field | Type | Required | Default | DB Column |
|-------|------|----------|---------|-----------|
| `id` | TEXT | Yes | auto | `epics.id` (PK) |
| `format_id` | TEXT | Yes | -- | `epics.format_id` (UNIQUE) |
| `title` | TEXT | Yes | -- | `epics.title` |
| `summary` | TEXT | No | null | `epics.summary` |
| `status` | TEXT | Yes | `draft` | `epics.status` |
| `area_type` | TEXT | Yes | -- | `epics.area_type` (FK) |
| `work_type` | TEXT | Yes | -- | `epics.work_type` (FK) |
| `domain` | TEXT | Yes | -- | `epics.domain` (FK) |
| `is_ongoing` | BOOLEAN | No | `false` | `epics.is_ongoing` |
| `file_scope` | JSON | No | `[]` | `epics.file_scope` |
| `priority` | TEXT | No | `normal` | `epics.priority` |
| `pr_number` | INTEGER | No | `null` | `epics.pr_number` |
| `external_id` | TEXT | No | `null` | `epics.external_id` |
| `external_url` | TEXT | No | `null` | `epics.external_url` |
| `created_at` | TEXT | Yes | auto | `epics.created_at` |
| `updated_at` | TEXT | Yes | auto | `epics.updated_at` |

### Task Fields (34 DB + 1 convenience)

| Field | Type | Required | Default | DB Column |
|-------|------|----------|---------|-----------|
| `id` | TEXT | Yes | auto | `tasks.id` (PK) |
| `format_id` | TEXT | Yes | -- | `tasks.format_id` (UNIQUE) |
| `epic_id` | TEXT | Yes | -- | `tasks.epic_id` (FK) |
| `epic_format_id` | TEXT | No | -- | *Not in DB yet* |
| `title` | TEXT | Yes | -- | `tasks.title` |
| `description` | TEXT | No | null | `tasks.description` |
| `status` | TEXT | Yes | `todo` | `tasks.status` |
| `area_type` | TEXT | Yes | -- | `tasks.area_type` (FK) |
| `work_type` | TEXT | Yes | -- | `tasks.work_type` (FK) |
| `domain` | TEXT | Yes | -- | `tasks.domain` (FK) |
| `origin` | TEXT | No | `planned` | `tasks.origin` |
| `file_scope` | JSON | No | `[]` | `tasks.file_scope` |
| `scope_policy` | TEXT | No | `soft` | `tasks.scope_policy` |
| `scope_root` | TEXT | No | `null` | `tasks.scope_root` |
| `estimate` | TEXT | No | `null` | `tasks.estimate` (FK) |
| `priority` | TEXT | No | `normal` | `tasks.priority` |
| `assignee_id` | TEXT | No | `null` | `tasks.assignee_id` (FK) |
| `autorun_eligible` | BOOLEAN | No | `false` | `tasks.autorun_eligible` |
| `auto_commit` | BOOLEAN | No | `true` | `tasks.auto_commit` |
| `raise_pr` | BOOLEAN | No | `true` | `tasks.raise_pr` |
| `auto_merge` | BOOLEAN | No | `false` | `tasks.auto_merge` |
| `target_branch` | TEXT | No | `null` | `tasks.target_branch` |
| `acceptance` | JSON | Conditional | `[]` | `tasks.acceptance` |
| `tests` | JSON | No | `[]` | `tasks.tests` |
| `branch` | TEXT | No | `null` | `tasks.branch` |
| `pr_number` | INTEGER | No | `null` | `tasks.pr_number` |
| `external_id` | TEXT | No | `null` | `tasks.external_id` |
| `external_url` | TEXT | No | `null` | `tasks.external_url` |
| `created_at` | TEXT | Yes | auto | `tasks.created_at` |
| `updated_at` | TEXT | Yes | auto | `tasks.updated_at` |
| `started_at` | TEXT | No | `null` | `tasks.started_at` |
| `completed_at` | TEXT | No | `null` | `tasks.completed_at` |
| `stage` | TEXT | No | `null` | `tasks.stage` |
| `stage_status` | TEXT | No | `null` | `tasks.stage_status` |
| `stage_history` | JSON | No | `'[]'` | `tasks.stage_history` |

### Allowed Values

| Field | Allowed Values |
|-------|---------------|
| `status` (epic) | `draft`, `planning`, `in_progress`, `blocked`, `complete`, `archived` |
| `status` (task) | `todo`, `blocked`, `in_progress`, `complete` |
| `area_type` | `FRT`, `BKD`, `INF`, `SHR`, `DOC`, `PLN` |
| `work_type` | `FEAT`, `FIX`, `HTFX`, `RFCT`, `DOCS`, `TEST`, `CHOR`, `CICD`, `SPKE`, `PLAN` |
| `priority` | `low`, `normal`, `high`, `critical` |
| `estimate` | `XS`, `S`, `M`, `L`, `XL` |
| `origin` | `planned`, `informal`, `auto` |
| `scope_policy` | `soft`, `hard`, `permissive` |
| `stage` | `dev`, `work`, `review`, `qa`, `done` |
| `stage_status` | `pending`, `in_progress`, `complete`, `failed` |
