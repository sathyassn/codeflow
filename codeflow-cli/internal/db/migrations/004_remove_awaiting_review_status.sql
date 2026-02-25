-- Migration 004: Remove 'awaiting_review' from tasks.status CHECK constraint
-- Version: 1.4.0
-- Date: 2026-02-23
--
-- Changes:
--   - tasks.status CHECK constraint no longer includes 'awaiting_review'
--   - Rationale: Task completion = work done (code written, reviewed, tested).
--     PR review state is self-evident from GitHub. awaiting_review duplicated
--     what GitHub already tracks.
--   - Simplified status set: todo, blocked, in_progress, complete, cancelled
--   - Any existing rows with status='awaiting_review' are migrated to 'complete'
--
-- SQLite limitation: CHECK constraints cannot be altered with ALTER TABLE.
-- This migration recreates the tasks table with the new constraint.
--
-- Idempotency: Safe to re-run. If tasks_new already exists from a failed prior
-- attempt, the migration will fail at CREATE TABLE (not silently corrupt data).
-- Clean up tasks_new manually if needed before re-running.

BEGIN TRANSACTION;

-- =============================================================================
-- STEP 1: Create new tasks table with updated CHECK constraint
-- =============================================================================

CREATE TABLE tasks_new (
    id TEXT PRIMARY KEY,              -- ULID PK: task-{ulid} (e.g., task-01BRZ4PDFLUTW5SSGG70H6GBW)
    format_id TEXT UNIQUE NOT NULL,   -- Human-readable: {AREA}-TSK-{NNN}-{NNN} (e.g., INF-TSK-001-001)
    epic_id TEXT NOT NULL REFERENCES epics(id),  -- FK to epics ULID PK
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'blocked', 'in_progress', 'complete', 'cancelled')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    origin TEXT DEFAULT 'planned'
        CHECK(origin IN ('planned', 'informal', 'auto')),
    file_scope TEXT,                   -- JSON array
    scope_policy TEXT DEFAULT 'soft'
        CHECK(scope_policy IN ('soft', 'hard', 'permissive')),
    scope_root TEXT,
    estimate TEXT,                     -- XS, S, M, L, XL
    priority TEXT DEFAULT 'normal'
        CHECK(priority IN ('low', 'normal', 'high', 'critical')),
    assignee_id TEXT REFERENCES users(id),
    autorun_eligible BOOLEAN DEFAULT FALSE,
    auto_commit BOOLEAN DEFAULT TRUE,  -- DEPRECATED: Scheduled for removal. Do not use in new code.
    raise_pr BOOLEAN DEFAULT TRUE,
    auto_merge BOOLEAN DEFAULT FALSE,
    target_branch TEXT,
    acceptance TEXT,                   -- JSON array
    tests TEXT,                        -- JSON array
    branch TEXT,
    pr_number INTEGER,
    external_id TEXT,
    external_url TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT,
    stage TEXT DEFAULT NULL
        CHECK(stage IS NULL OR stage IN ('dev', 'work', 'review', 'qa', 'done')),
    stage_status TEXT DEFAULT NULL
        CHECK(stage_status IS NULL OR stage_status IN ('pending', 'in_progress', 'complete', 'failed')),
    stage_history TEXT DEFAULT '[]',    -- JSON array of stage transition records
    FOREIGN KEY (area_type) REFERENCES area_types(code),
    FOREIGN KEY (work_type) REFERENCES work_types(code),
    FOREIGN KEY (domain) REFERENCES domains(code),
    FOREIGN KEY (estimate) REFERENCES estimate_types(code)
);

-- =============================================================================
-- STEP 2: Copy all existing data, migrating awaiting_review -> complete
-- =============================================================================

INSERT INTO tasks_new
SELECT
    id, format_id, epic_id, title, description,
    CASE WHEN status = 'awaiting_review' THEN 'complete' ELSE status END,
    area_type, work_type, domain, origin, file_scope, scope_policy, scope_root,
    estimate, priority, assignee_id, autorun_eligible, auto_commit,
    raise_pr, auto_merge, target_branch, acceptance, tests,
    branch, pr_number, external_id, external_url,
    created_at, updated_at, started_at, completed_at,
    stage, stage_status, stage_history
FROM tasks;

-- =============================================================================
-- STEP 3: Drop old table and rename new table
-- =============================================================================

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

-- =============================================================================
-- STEP 4: Recreate all indexes on the tasks table
-- =============================================================================

CREATE INDEX IF NOT EXISTS idx_tasks_epic ON tasks(epic_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_branch ON tasks(branch);
CREATE INDEX IF NOT EXISTS idx_tasks_assignee ON tasks(assignee_id);
CREATE INDEX IF NOT EXISTS idx_tasks_priority ON tasks(priority);
CREATE INDEX IF NOT EXISTS idx_tasks_ready ON tasks(epic_id, status)
    WHERE status = 'todo';
CREATE INDEX IF NOT EXISTS idx_tasks_in_progress ON tasks(status, assignee_id)
    WHERE status = 'in_progress';
CREATE INDEX IF NOT EXISTS idx_tasks_autorun ON tasks(epic_id, status, autorun_eligible)
    WHERE autorun_eligible = TRUE AND status = 'todo';
CREATE INDEX IF NOT EXISTS idx_tasks_stage ON tasks(stage, stage_status);
CREATE INDEX IF NOT EXISTS idx_tasks_stage_status ON tasks(stage_status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_format_id ON tasks(format_id);

-- =============================================================================
-- STEP 5: Update schema version
-- =============================================================================

INSERT OR REPLACE INTO schema_version (version, applied_at)
    VALUES (4, CURRENT_TIMESTAMP);

COMMIT;
