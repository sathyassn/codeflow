-- Migration 001: Add V4 PathFlow and Agent Teams columns to tasks table
-- Version: schema.sql V4
-- Date: 2026-02-18
--
-- Adds 10 columns to the tasks table:
--   format_id, acceptance, tests, auto_commit, raise_pr,
--   auto_merge, target_branch, stage, stage_status, stage_history
--
-- Adds domains: QUAL, PMGT (INSERT OR IGNORE for idempotency)
-- Adds indexes: idx_tasks_format_id, idx_tasks_stage, idx_tasks_stage_status
--
-- Idempotency: ALTER TABLE ADD COLUMN errors on duplicate columns but
-- sqlite3 continues processing remaining statements. INSERT OR IGNORE
-- and CREATE INDEX IF NOT EXISTS are inherently idempotent.
-- Re-running this migration is safe (no data loss or corruption).

-- =============================================================================
-- ADD V4 COLUMNS TO TASKS TABLE
-- =============================================================================

-- Human-readable format ID (e.g., INF-TSK-005-001)
-- NOTE: Cannot enforce NOT NULL via ALTER TABLE for existing rows.
-- UNIQUE constraint enforced via idx_tasks_format_id index below.
ALTER TABLE tasks ADD COLUMN format_id TEXT;

-- Task specification columns (JSON arrays)
ALTER TABLE tasks ADD COLUMN acceptance TEXT;              -- JSON array of acceptance criteria
ALTER TABLE tasks ADD COLUMN tests TEXT;                   -- JSON array of test specs

-- Automation control columns
ALTER TABLE tasks ADD COLUMN auto_commit BOOLEAN DEFAULT TRUE;
ALTER TABLE tasks ADD COLUMN raise_pr BOOLEAN DEFAULT TRUE;
ALTER TABLE tasks ADD COLUMN auto_merge BOOLEAN DEFAULT FALSE;
ALTER TABLE tasks ADD COLUMN target_branch TEXT;

-- PathFlow stage tracking columns
-- NOTE: schema.sql defines CHECK constraints on stage and stage_status columns:
--   stage CHECK(stage IS NULL OR stage IN ('dev', 'work', 'review', 'qa', 'done'))
--   stage_status CHECK(stage_status IS NULL OR stage_status IN ('pending', 'in_progress', 'complete', 'failed'))
-- ALTER TABLE ADD COLUMN cannot add CHECK constraints in SQLite.
-- Application-layer validation must enforce these constraints until table rebuild.
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;       -- Current PathFlow work stage
ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL; -- Status within current stage
ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]'; -- JSON array of stage transitions

-- =============================================================================
-- SEED DATA: Add QUAL and PMGT domains
-- =============================================================================

INSERT OR IGNORE INTO domains (code, name, description, is_reserved)
    VALUES ('QUAL', 'Quality Assurance', 'Testing and QA', FALSE);

INSERT OR IGNORE INTO domains (code, name, description, is_reserved)
    VALUES ('PMGT', 'Project Management', 'Project management and tracking', FALSE);

-- =============================================================================
-- POPULATE format_id FOR EXISTING ROWS
-- =============================================================================

-- Existing rows use format-style IDs as their primary key (id column).
-- Copy id -> format_id so existing data is consistent with V4 schema.
UPDATE tasks SET format_id = id WHERE format_id IS NULL;

-- =============================================================================
-- INDEXES
-- =============================================================================

CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_format_id ON tasks(format_id);
CREATE INDEX IF NOT EXISTS idx_tasks_stage ON tasks(stage, stage_status);
CREATE INDEX IF NOT EXISTS idx_tasks_stage_status ON tasks(stage_status);
