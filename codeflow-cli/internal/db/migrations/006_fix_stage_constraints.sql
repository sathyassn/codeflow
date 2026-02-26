-- Migration 006: Fix stage CHECK constraints (remove 'work', add 'plan', 'docs', 'test')
-- Version: 1.6.0
-- Date: 2026-02-25
--
-- Changes:
--   1. Rebuild tasks table: stage CHECK removes 'work', adds 'plan', 'docs', 'test'
--      New: stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa', 'done')
--   2. Rebuild active_work table: current_stage CHECK removes 'work', adds 'plan', 'docs', 'test'
--      New: current_stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa')
--   3. Set PRAGMA user_version = 6
--
-- Existing rows with stage='work' are migrated to 'dev' as the closest equivalent.
-- Idempotency: Safe to re-run. DROP TABLE IF EXISTS on temp tables handles partial runs.

BEGIN TRANSACTION;

-- =============================================================================
-- STEP 1: Rebuild tasks table with corrected stage CHECK constraint
-- =============================================================================

DROP TABLE IF EXISTS tasks_new;

CREATE TABLE tasks_new (
    id TEXT PRIMARY KEY,
    format_id TEXT UNIQUE NOT NULL,
    epic_id TEXT NOT NULL REFERENCES epics(id),
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'blocked', 'in_progress', 'complete', 'cancelled')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    origin TEXT DEFAULT 'planned'
        CHECK(origin IN ('planned', 'informal', 'auto')),
    file_scope TEXT,
    scope_policy TEXT DEFAULT 'soft'
        CHECK(scope_policy IN ('soft', 'hard', 'permissive')),
    scope_root TEXT,
    estimate TEXT,
    priority TEXT DEFAULT 'normal'
        CHECK(priority IN ('low', 'normal', 'high', 'critical')),
    assignee_id TEXT REFERENCES users(id),
    autorun_eligible BOOLEAN DEFAULT FALSE,
    auto_commit BOOLEAN DEFAULT TRUE,
    raise_pr BOOLEAN DEFAULT TRUE,
    auto_merge BOOLEAN DEFAULT FALSE,
    target_branch TEXT,
    acceptance TEXT,
    tests TEXT,
    branch TEXT,
    pr_number INTEGER,
    external_id TEXT,
    external_url TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT,
    stage TEXT DEFAULT NULL
        CHECK(stage IS NULL OR stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa', 'done')),
    stage_status TEXT DEFAULT NULL
        CHECK(stage_status IS NULL OR stage_status IN ('pending', 'in_progress', 'complete', 'failed')),
    stage_history TEXT DEFAULT '[]',
    FOREIGN KEY (area_type) REFERENCES area_types(code),
    FOREIGN KEY (work_type) REFERENCES work_types(code),
    FOREIGN KEY (domain) REFERENCES domains(code),
    FOREIGN KEY (estimate) REFERENCES estimate_types(code)
);

INSERT INTO tasks_new (id, format_id, epic_id, title, description, status, area_type, work_type,
                       domain, origin, file_scope, scope_policy, scope_root, estimate, priority,
                       assignee_id, autorun_eligible, auto_commit, raise_pr, auto_merge,
                       target_branch, acceptance, tests, branch, pr_number, external_id,
                       external_url, created_at, updated_at, started_at, completed_at,
                       stage, stage_status, stage_history)
SELECT
    id, format_id, epic_id, title, description, status, area_type, work_type,
    domain, origin, file_scope, scope_policy, scope_root, estimate, priority,
    assignee_id, autorun_eligible, auto_commit, raise_pr, auto_merge,
    target_branch, acceptance, tests, branch, pr_number, external_id,
    external_url, created_at, updated_at, started_at, completed_at,
    CASE WHEN stage = 'work' THEN 'dev' ELSE stage END,
    stage_status, stage_history
FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

-- Recreate all indexes on tasks (matching schema.sql)
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
-- STEP 2: Rebuild active_work table with corrected current_stage CHECK constraint
-- =============================================================================

DROP TABLE IF EXISTS active_work_new;

CREATE TABLE active_work_new (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(id),
    topic TEXT NOT NULL,
    status TEXT DEFAULT 'in_progress'
        CHECK(status IN ('in_progress', 'complete', 'blocked')),
    branch TEXT,
    scope TEXT,
    deliverables TEXT,
    agent TEXT,
    session_id TEXT,
    current_stage TEXT DEFAULT NULL
        CHECK(current_stage IS NULL OR current_stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa')),
    team_name TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO active_work_new (id, task_id, topic, status, branch, scope, deliverables,
                             agent, session_id, current_stage, team_name,
                             created_at, updated_at)
SELECT
    id, task_id, topic, status, branch, scope, deliverables,
    agent, session_id,
    CASE WHEN current_stage = 'work' THEN 'dev' ELSE current_stage END,
    team_name,
    created_at, updated_at
FROM active_work;

DROP TABLE active_work;
ALTER TABLE active_work_new RENAME TO active_work;

-- Recreate all indexes on active_work (matching schema.sql)
CREATE INDEX IF NOT EXISTS idx_active_work_status ON active_work(status);
CREATE INDEX IF NOT EXISTS idx_active_work_task ON active_work(task_id);
CREATE INDEX IF NOT EXISTS idx_active_work_branch ON active_work(branch);
CREATE INDEX IF NOT EXISTS idx_active_work_session ON active_work(session_id);
CREATE INDEX IF NOT EXISTS idx_active_work_stage ON active_work(current_stage);
CREATE INDEX IF NOT EXISTS idx_active_work_team ON active_work(team_name);

-- =============================================================================
-- STEP 3: Set database version
-- =============================================================================

PRAGMA user_version = 6;

COMMIT;
