# Part 9: Knowledge Layer Integration

> Schema additions, three-tier consistency for stage changes, cross-session recovery, JSONL event format, and agent permissions.

---

## Table of Contents

- [9.1 Overview](#91-overview)
- [9.2 Schema Additions: Tasks Table](#92-schema-additions-tasks-table)
- [9.3 Schema Additions: Active Work Table](#93-schema-additions-active-work-table)
- [9.4 Three-Tier Consistency for Stage Changes](#94-three-tier-consistency-for-stage-changes)
- [9.5 JSONL Event Format for Stage Transitions](#95-jsonl-event-format-for-stage-transitions)
- [9.6 Cross-Session Context Recovery](#96-cross-session-context-recovery)
- [9.7 Agent Permissions for New Fields](#97-agent-permissions-for-new-fields)
- [9.8 Stage History as Audit Trail](#98-stage-history-as-audit-trail)

---

## 9.1 Overview

The Knowledge Layer is CodeFlow's persistent data system, managed exclusively by the cf-knowledge-layer function teammate. PathFlow v3 adds work stage tracking to the existing schema. All additions are purely additive -- no existing columns are modified or removed.

The Knowledge Layer uses a three-tier data model:

```
Tier 0: JSONL Ledger     (append-only, source of truth for rebuilds)
     |
     v
Tier 1: SQLite            (fast queries, indexed, rebuildable from ledger)
     |
     v
Tier 2: Markdown           (human-readable, version controlled)
```

Stage transitions must update all three tiers atomically via cf-db-operations.

---

## 9.2 Schema Additions: Tasks Table

Three new columns on the existing `tasks` table:

```sql
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;
  -- Values: null | dev | work | review | qa | done
  -- Current stage in the pipeline (dev for code, work for non-code)

ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL;
  -- Values: null | pending | in_progress | complete | failed
  -- Current status within the active stage

ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]';
  -- JSON array of stage progression records
  -- Full audit trail of every stage transition
```

**Relationship to existing `status` column**:

The existing `status` field (`todo | blocked | in_progress | complete`) tracks the OVERALL task lifecycle. The new `stage` and `stage_status` fields track WHERE the task is in the development pipeline.

```
+-------------------------------+
|          tasks table          |
+-------------------------------+
| status          | stage       |    Combined meaning:
|                 | stage_status|
+-----------------+-------------+
| in_progress     | dev         |    "Task is active, currently
|                 | in_progress |     being developed"
+-----------------+-------------+
| in_progress     | review      |    "Task is active, currently
|                 | in_progress |     being reviewed"
+-----------------+-------------+
| in_progress     | dev         |    "Task is active, dev failed
|                 | failed      |     and needs retry"
+-----------------+-------------+
| in_progress     | qa          |    "Task is active, tests are
|                 | complete    |     passing"
+-----------------+-------------+
| complete        | done        |    "Task is finished, all stages
|                 | complete    |     passed"
+-----------------+-------------+
| todo            | null        |    "Task hasn't entered the
|                 | null        |     pipeline yet"
+-----------------+-------------+
```

**NULL values**: Tasks that don't go through the stage pipeline (planning, research) keep `stage` and `stage_status` as NULL. Existing queries that don't reference these columns continue to work unchanged.

---

## 9.3 Schema Additions: Active Work Table

Two new columns on the existing `active_work` table:

```sql
ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL;
  -- Values: null | dev | work | review | qa
  -- Quick lookup: what stage is active right now?

ALTER TABLE active_work ADD COLUMN team_name TEXT DEFAULT NULL;
  -- The Agent Teams team name for this work session
  -- Enables team context recovery across sessions
```

**Why `current_stage` on active_work?**

The `tasks.stage` field tracks the same information, but `active_work` is the operational table queried at session start for "what's currently happening?" Having `current_stage` here avoids a join and makes context detection fast:

```sql
-- Quick: "What stage is the current work in?"
SELECT current_stage FROM active_work WHERE status = 'in_progress';

-- Without current_stage, would need:
SELECT t.stage FROM active_work aw
  JOIN tasks t ON aw.task_id = t.id
  WHERE aw.status = 'in_progress';
```

**Why `team_name` on active_work?**

When a session resumes work that was started with a team, the resuming session needs to know what team configuration to recreate. The `team_name` field enables this:

```
Session A: Creates team "feat-auth-42", starts DEV stage, crashes
Session B: Detects active_work with team_name="feat-auth-42"
           Can recreate team with same structure
```

---

## 9.4 Three-Tier Consistency for Stage Changes

When a stage transition occurs, all three data tiers must be updated atomically. cf-knowledge-layer handles this via cf-db-operations.

```
Stage Transition: DEV -> REVIEW
(cf-reviewer sends: "Dev stage complete for FRT-TSK-042")

cf-knowledge-layer executes:

TIER 0 (JSONL):
  Append to .state/ledger/events.jsonl:
    { "type": "stage_transition", "task_id": "FRT-TSK-042",
      "from_stage": "dev", "to_stage": "review", ... }

TIER 1 (SQLite):
  UPDATE tasks SET
    stage = 'review',
    stage_status = 'pending',
    stage_history = json_insert(stage_history, '$[#]', '{"stage":"dev","status":"complete",...}'),
    updated_at = CURRENT_TIMESTAMP
  WHERE id = 'FRT-TSK-FEAT-AUTH-042';

  UPDATE active_work SET
    current_stage = 'review',
    updated_at = CURRENT_TIMESTAMP
  WHERE task_id = 'FRT-TSK-FEAT-AUTH-042';

TIER 2 (Markdown):
  Update epics/{epic-id}/tasks/{task-id}.md:
    Stage: review (pending)
    Stage History:
      - DEV: complete (2026-02-07T14:00-14:25, cf-developer, pass)
```

**Data flow diagram**:

```
cf-developer                 cf-knowledge-layer                  Data Tiers
     |                            |                            |
     |  "Dev complete for         |                            |
     |   FRT-TSK-042"             |                            |
     |--------------------------->|                            |
     |                            |  1. Append JSONL event     |
     |                            |--------------------------->| Tier 0
     |                            |                            |
     |                            |  2. UPDATE tasks table     |
     |                            |--------------------------->| Tier 1
     |                            |                            |
     |                            |  3. UPDATE active_work     |
     |                            |--------------------------->| Tier 1
     |                            |                            |
     |                            |  4. Update task markdown   |
     |                            |--------------------------->| Tier 2
     |                            |                            |
     |                            |  (atomic: all or none)     |
```

---

## 9.5 JSONL Event Format for Stage Transitions

Stage transitions are recorded in the JSONL ledger as events. The format extends the existing `memory_events` format.

**Stage transition event**:

```json
{
  "id": "memory-01JK8ABC123",
  "event_type": "stage_transition",
  "domain": "development",
  "work_id": "work-01JK8DEF456",
  "data": {
    "task_id": "FRT-TSK-FEAT-AUTH-042",
    "from_stage": "dev",
    "from_status": "complete",
    "to_stage": "review",
    "to_status": "pending",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Implemented login validation. 3 files changed.",
    "rework": false,
    "iteration": 1
  },
  "memory_type": "episodic",
  "created_at": "2026-02-07T14:25:00Z"
}
```

**Stage complete event** (records a stage's final outcome):

```json
{
  "id": "memory-01JK8ABC789",
  "event_type": "stage_complete",
  "domain": "development",
  "work_id": "work-01JK8DEF456",
  "data": {
    "task_id": "FRT-TSK-FEAT-AUTH-042",
    "stage": "review",
    "status": "complete",
    "agent": "cf-reviewer",
    "verdict": "changes_requested",
    "findings": ["Missing error handling for network timeout"],
    "duration_seconds": 540,
    "iteration": 1
  },
  "memory_type": "episodic",
  "created_at": "2026-02-07T14:35:00Z"
}
```

**Rework event** (records a rework cycle entry):

```json
{
  "id": "memory-01JK8ABCDEF",
  "event_type": "stage_transition",
  "domain": "development",
  "work_id": "work-01JK8DEF456",
  "data": {
    "task_id": "FRT-TSK-FEAT-AUTH-042",
    "from_stage": "review",
    "from_status": "complete",
    "to_stage": "dev",
    "to_status": "pending",
    "agent": "cf-reviewer",
    "verdict": "changes_requested",
    "notes": "Routing back to DEV: missing error handling",
    "rework": true,
    "iteration": 2
  },
  "memory_type": "episodic",
  "created_at": "2026-02-07T14:36:00Z"
}
```

**Key fields in `data`**:

| Field | Type | Description |
|-------|------|-------------|
| `task_id` | string | WorkGraph task ID |
| `from_stage` / `to_stage` | string | Stage being left / entered |
| `agent` | string | Teammate that handled the stage |
| `verdict` | string | Stage outcome: pass, approved, changes_requested, fail |
| `findings` | array | Review/QA findings (optional) |
| `rework` | boolean | Whether this is a rework iteration |
| `iteration` | integer | Which iteration (1 = first pass, 2+ = rework) |

---

## 9.6 Cross-Session Context Recovery

When a session crashes or is interrupted mid-pipeline, the next session must recover context and resume. The Knowledge Layer provides all the data needed.

**Recovery flow**:

```
New session starts (PF-1 -> PF-2 Context Awareness)

cf-knowledge-layer queries:
  1. SELECT * FROM active_work WHERE status = 'in_progress'
     -> Found: work-01JK8DEF456, task_id=FRT-TSK-FEAT-AUTH-042,
        current_stage=review, team_name=feat-auth-42

  2. SELECT stage, stage_status, stage_history
     FROM tasks WHERE id = 'FRT-TSK-FEAT-AUTH-042'
     -> stage=review, stage_status=in_progress
     -> stage_history: [{ stage: "dev", status: "complete", ... }]

  3. Read latest JSONL events for this task
     -> Last event: stage_transition dev->review

cf-knowledge-layer reports to lead:
  "Resuming FRT-TSK-FEAT-AUTH-042.
   DEV stage completed by cf-developer.
   REVIEW stage was in progress when session ended.
   Previous team: feat-auth-42.
   Recommendation: Recreate team, resume at WS-REV."
```

**What the lead does with this information**:

1. Skips PF-1 through PF-3 (work already classified and registered)
2. Creates team with same structure
3. Creates PF-4 tasks starting at WS-REV
4. Spawns cf-reviewer to continue the interrupted review
5. Creates sentinels for completed phases (pathflow:pf-3, pathflow:ws-dev-done)

**Data available for recovery**:

| Source | Information |
|--------|-------------|
| `active_work.current_stage` | Where the pipeline was interrupted |
| `active_work.team_name` | What team structure to recreate |
| `tasks.stage_history` | Full progression including rework cycles |
| JSONL events | Detailed timeline with agent actions and findings |
| Tier 2 markdown | Human-readable summary of progress |

---

## 9.7 Agent Permissions for New Fields

Not all teammates should update stage fields. Permissions are enforced via instructions (agent definitions) and validated by cf-knowledge-layer.

| Field | Who Can Update | How |
|-------|---------------|-----|
| `tasks.stage` | cf-knowledge-layer only | Via cf-db-operations after teammate stage reports |
| `tasks.stage_status` | cf-knowledge-layer only | Same |
| `tasks.stage_history` | cf-knowledge-layer only | Appended automatically on stage transitions |
| `active_work.current_stage` | cf-knowledge-layer only | Updated alongside tasks.stage |
| `active_work.team_name` | cf-knowledge-layer only | Set at PF-3 when team is created |

**Why cf-knowledge-layer is the sole updater**: Centralizing stage updates through one teammate ensures:

1. Three-tier consistency (JSONL + SQLite + Markdown always in sync)
2. No conflicting concurrent updates to stage fields
3. Single audit point for all stage transitions
4. Other teammates simply message cf-knowledge-layer with their stage outcomes

**Role teammate interaction pattern**:

```
cf-developer:  "Dev stage complete for FRT-TSK-042. Verdict: pass."
               (sends message to cf-knowledge-layer)

cf-knowledge-layer:  Validates the message
               Updates Tier 0 (JSONL event)
               Updates Tier 1 (tasks.stage, stage_status, stage_history; active_work.current_stage)
               Updates Tier 2 (task markdown)
               Responds: "Acknowledged. Stage updated to review:pending."
```

---

## 9.8 Stage History as Audit Trail

The `stage_history` JSON array on the tasks table provides a complete audit trail of every stage transition, including rework cycles.

**Example for a task that went through one rework cycle**:

```json
[
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-07T14:00:00Z",
    "completed_at": "2026-02-07T14:25:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Implemented login validation"
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-07T14:26:00Z",
    "completed_at": "2026-02-07T14:35:00Z",
    "agent": "cf-reviewer",
    "verdict": "changes_requested",
    "notes": "Missing error handling for network timeout"
  },
  {
    "stage": "dev",
    "status": "complete",
    "started_at": "2026-02-07T14:36:00Z",
    "completed_at": "2026-02-07T14:42:00Z",
    "agent": "cf-developer",
    "verdict": "pass",
    "notes": "Added network timeout error handling",
    "rework": true,
    "iteration": 2
  },
  {
    "stage": "review",
    "status": "complete",
    "started_at": "2026-02-07T14:43:00Z",
    "completed_at": "2026-02-07T14:48:00Z",
    "agent": "cf-reviewer",
    "verdict": "approved",
    "iteration": 2
  },
  {
    "stage": "qa",
    "status": "complete",
    "started_at": "2026-02-07T14:49:00Z",
    "completed_at": "2026-02-07T15:05:00Z",
    "agent": "cf-qa",
    "verdict": "pass",
    "notes": "8/8 tests pass. Coverage 94%."
  }
]
```

**What this tells us**: The task went through DEV, was reviewed and sent back for rework, went through DEV again, passed review on the second attempt, and passed QA. Total pipeline time: 65 minutes, 2 DEV iterations, 2 review iterations, 1 QA pass.

---

## Related Documents

- [04-workgraph-schema.md](../pathflow-revised/04-workgraph-schema.md) -- Original schema change proposal
- [07-work-stages.md](07-work-stages.md) -- Stage definitions and routing
- [08-enforcement-model.md](08-enforcement-model.md) -- How stages are enforced
- [10-session-lifecycle.md](10-session-lifecycle.md) -- Session boundary and recovery
- [12-changes-and-claude-components.md](12-changes-and-claude-components.md) -- Schema migration details
