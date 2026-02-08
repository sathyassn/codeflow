# Part 4: WorkGraph Schema Changes

> Schema additions to support multi-stage work tracking in the Knowledge Layer.

---

## Table of Contents

- [4.1 Tasks Table Additions](#41-tasks-table-additions)
- [4.2 Stage History Format](#42-stage-history-format)
- [4.3 Active Work Table Enhancement](#43-active-work-table-enhancement)
- [4.4 Current Schema Reference](#44-current-schema-reference)

---

## 4.1 Tasks Table Additions

New columns on the existing `tasks` table:

```sql
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;
  -- null | dev | review | qa | deploy | done
ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL;
  -- null | pending | in_progress | complete | failed
ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]';
  -- JSON array of stage progression records
```

**Why these additions?**

The current `status` field (`todo | blocked | in_progress | complete`) tracks the OVERALL task lifecycle. The new `stage` and `stage_status` fields track WHERE the task is in the development pipeline (dev -> review -> QA -> deploy).

This is an important distinction:
- `status = in_progress` + `stage = review` + `stage_status = in_progress` means "the task is active and currently being reviewed"
- `status = in_progress` + `stage = dev` + `stage_status = failed` means "the task is active but dev work failed (needs retry)"

**NULL values** are used when the task hasn't entered the stage pipeline yet (e.g., a planning task that doesn't go through dev/review/QA).

---

## 4.2 Stage History Format

The `stage_history` column stores a JSON array of stage progression records. Each entry records one stage transition:

```json
[
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-06T14:00:00Z",
    "completed_at": "2026-02-06T14:25:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Implemented login validation"
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-06T14:26:00Z",
    "completed_at": "2026-02-06T14:35:00Z",
    "agent": "cf-reviewer",
    "verdict": "changes_requested",
    "notes": "Missing error handling for network timeout"
  },
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-06T14:36:00Z",
    "completed_at": "2026-02-06T14:42:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Added network timeout error handling"
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-06T14:43:00Z",
    "completed_at": "2026-02-06T14:48:00Z",
    "agent": "cf-reviewer",
    "verdict": "approved"
  }
]
```

**Key fields**:
- `stage`: Which stage (dev, review, qa, deploy)
- `status`: Outcome (complete, failed)
- `started_at` / `completed_at`: ISO 8601 timestamps
- `agent`: Which teammate handled this stage
- `verdict`: Stage-specific outcome (pass, approved, changes_requested, fail)
- `notes`: Optional description of what happened

**Repeated stages**: When a review requests changes, the task cycles back to dev, then re-enters review. The stage_history captures this full progression, making the entire workflow auditable.

---

## 4.3 Active Work Table Enhancement

New columns on the existing `active_work` table:

```sql
ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL;
ALTER TABLE active_work ADD COLUMN team_name TEXT DEFAULT NULL;
```

- `current_stage`: Which work stage is currently active (dev, review, qa, deploy, null)
- `team_name`: The Agent Teams team name associated with this work session

This enables:
- Quick lookup of "what stage is this task in right now?" without parsing stage_history
- Associating a team with active work for context recovery across sessions

---

## 4.4 Current Schema Reference

For context, the existing schema from `.claude/skills/cf-db-operations/resources/schema-reference.md`:

**tasks table** (existing columns):
```sql
CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  epic_id TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  status TEXT DEFAULT 'todo',          -- todo | blocked | in_progress | complete
  priority TEXT DEFAULT 'medium',
  assignee TEXT,
  estimated_hours REAL,
  actual_hours REAL,
  acceptance_criteria TEXT,            -- JSON array
  tags TEXT,                           -- JSON array
  metadata TEXT,                       -- JSON object
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  completed_at DATETIME,
  FOREIGN KEY (epic_id) REFERENCES epics(id)
);
```

**active_work table** (existing columns):
```sql
CREATE TABLE active_work (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  branch_name TEXT,
  started_at DATETIME DEFAULT CURRENT_TIMESTAMP,
  last_active DATETIME DEFAULT CURRENT_TIMESTAMP,
  status TEXT DEFAULT 'active',        -- active | paused | completed
  context TEXT,                        -- JSON context blob
  FOREIGN KEY (task_id) REFERENCES tasks(id)
);
```

The proposed additions are purely additive — no existing columns are modified or removed. All new columns have sensible defaults (NULL or '[]') so existing queries continue to work unchanged.
