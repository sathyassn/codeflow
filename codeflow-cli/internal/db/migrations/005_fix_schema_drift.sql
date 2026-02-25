-- Migration 005: Fix schema drift (format_id, active_work columns, user_version=5)
-- Version: 1.5.0
-- Date: 2026-02-24
--
-- Changes:
--   1. Rebuild epics table to add format_id TEXT UNIQUE NOT NULL column
--      Backfill: use external_id if it matches {AREA}-EPC-{NNN}, else generate
--   2. Rebuild active_work table to add current_stage TEXT (with CHECK) and team_name TEXT
--   3. Tasks table: no changes needed (migrations 002-004 handle CHECK constraints)
--   4. Set PRAGMA user_version = 5
--
-- SQLite limitation: ALTER TABLE cannot add UNIQUE/NOT NULL columns to existing rows
-- or add CHECK constraints. Table rebuild required for epics and active_work.
--
-- Idempotency: Safe to re-run. DROP TABLE IF EXISTS on temp tables handles both
-- crash recovery and repeated execution. Data is reprocessed but produces identical
-- results for external_id-derived format_ids. Generated format_ids use a high offset
-- (900+) to avoid collisions with existing external_id-derived numbers.

BEGIN TRANSACTION;

-- =============================================================================
-- STEP 1: Rebuild epics table with format_id column
-- =============================================================================

-- Clean up any leftover temp table from partial prior run
DROP TABLE IF EXISTS epics_new;

CREATE TABLE epics_new (
    id TEXT PRIMARY KEY,
    format_id TEXT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    summary TEXT,
    status TEXT DEFAULT 'draft'
        CHECK(status IN ('draft', 'planning', 'in_progress', 'blocked', 'complete', 'archived')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    is_ongoing BOOLEAN DEFAULT FALSE,
    file_scope TEXT,
    priority TEXT DEFAULT 'normal'
        CHECK(priority IN ('low', 'normal', 'high', 'critical')),
    pr_number INTEGER,
    external_id TEXT,
    external_url TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (area_type) REFERENCES area_types(code),
    FOREIGN KEY (work_type) REFERENCES work_types(code),
    FOREIGN KEY (domain) REFERENCES domains(code)
);

-- Copy data with format_id backfill:
--   1. Use external_id if it matches {AREA}-EPC-{NNN} pattern
--   2. Otherwise generate: {area_type}-EPC-{900+row_number}
--      Offset 900 avoids collision with external_id-derived format_ids
INSERT INTO epics_new (id, format_id, title, summary, status, area_type, work_type, domain,
                       is_ongoing, file_scope, priority, pr_number, external_id, external_url,
                       created_at, updated_at)
SELECT
    id,
    CASE
        WHEN external_id GLOB '[A-Z][A-Z][A-Z]-EPC-[0-9][0-9][0-9]' THEN external_id
        ELSE area_type || '-EPC-' || printf('%03d', 900 + ROW_NUMBER() OVER (
            PARTITION BY area_type ORDER BY created_at, id
        ))
    END,
    title, summary, status, area_type, work_type, domain,
    is_ongoing, file_scope, priority, pr_number, external_id, external_url,
    created_at, updated_at
FROM epics;

DROP TABLE epics;
ALTER TABLE epics_new RENAME TO epics;

-- Recreate all indexes on epics (matching schema.sql)
CREATE INDEX IF NOT EXISTS idx_epics_status ON epics(status);
CREATE INDEX IF NOT EXISTS idx_epics_area ON epics(area_type);
CREATE INDEX IF NOT EXISTS idx_epics_work_type ON epics(work_type);
CREATE INDEX IF NOT EXISTS idx_epics_domain ON epics(domain);
CREATE UNIQUE INDEX IF NOT EXISTS idx_epics_format_id ON epics(format_id);
CREATE INDEX IF NOT EXISTS idx_epics_ongoing ON epics(area_type, work_type, is_ongoing)
    WHERE is_ongoing = TRUE;
CREATE INDEX IF NOT EXISTS idx_epics_active ON epics(status)
    WHERE status IN ('draft', 'planning', 'in_progress', 'blocked');

-- =============================================================================
-- STEP 2: Rebuild active_work table with current_stage and team_name columns
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
        CHECK(current_stage IS NULL OR current_stage IN ('dev', 'work', 'review', 'qa')),
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
    NULL,   -- current_stage defaults to NULL for existing rows
    NULL,   -- team_name defaults to NULL for existing rows
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
-- STEP 3: Verify tasks table stage CHECK constraint
-- =============================================================================

-- Migrations 002-004 already rebuilt the tasks table with the correct CHECK constraint:
--   stage CHECK(stage IS NULL OR stage IN ('dev', 'work', 'review', 'qa', 'done'))
-- No action needed. Verified by test-migration-005.sh.

-- =============================================================================
-- STEP 4: Set database version
-- =============================================================================

PRAGMA user_version = 5;

COMMIT;
