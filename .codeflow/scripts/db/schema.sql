-- CodeFlow Database Schema
-- Version: 1.2.0
-- Generated from v3 specification; V4 PathFlow and Agent Teams columns
--
-- This schema defines all 37 tables for the Knowledge Layer plus 3 FTS virtual tables.
-- Rebuild authority: .state/ledger/*.jsonl files

-- =============================================================================
-- CORE TABLES
-- =============================================================================

-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- Project configuration
CREATE TABLE IF NOT EXISTS project_config (
    id TEXT PRIMARY KEY,              -- project-{ulid}
    name TEXT NOT NULL,
    description TEXT,
    git_remote_url TEXT,
    default_branch TEXT DEFAULT 'main',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    metadata TEXT                     -- JSON for extensibility
);

-- Users table (normalized user tracking)
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,              -- user-{ulid}
    email TEXT NOT NULL UNIQUE,       -- Primary identifier (git email)
    display_name TEXT,
    git_username TEXT,                -- GitHub/GitLab username
    role TEXT DEFAULT 'contributor'
        CHECK(role IN ('contributor', 'maintainer', 'admin')),
    last_host TEXT,                   -- Last known host machine
    last_machine_fingerprint TEXT,
    first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
    last_active_at TEXT,
    preferences TEXT,                 -- JSON for user preferences
    metadata TEXT                     -- JSON for extensibility
);

CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_git_username ON users(git_username);
CREATE INDEX IF NOT EXISTS idx_users_last_active ON users(last_active_at);

-- Project members (many-to-many)
CREATE TABLE IF NOT EXISTS project_members (
    project_id TEXT NOT NULL REFERENCES project_config(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT DEFAULT 'contributor'
        CHECK(role IN ('contributor', 'maintainer', 'admin')),
    joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_project_members_user ON project_members(user_id);

-- =============================================================================
-- WORK GRAPH CONFIGURATION TABLES
-- =============================================================================

-- Area types configuration
CREATE TABLE IF NOT EXISTS area_types (
    code TEXT PRIMARY KEY,           -- FRT, BKD, INF, etc.
    name TEXT NOT NULL,
    description TEXT,
    scope_patterns TEXT,             -- JSON array
    is_active BOOLEAN DEFAULT TRUE
);

-- Work types configuration
CREATE TABLE IF NOT EXISTS work_types (
    code TEXT PRIMARY KEY,           -- FEAT, FIX, HTFX, etc.
    name TEXT NOT NULL,
    branch_prefix TEXT NOT NULL,     -- feat/, fix/, hotfix/
    commit_type TEXT,
    urgency TEXT DEFAULT 'normal'
        CHECK(urgency IN ('normal', 'high', 'critical')),
    default_scope_policy TEXT DEFAULT 'soft'
        CHECK(default_scope_policy IN ('soft', 'hard', 'permissive')),
    is_active BOOLEAN DEFAULT TRUE
);

-- Domains configuration
CREATE TABLE IF NOT EXISTS domains (
    code TEXT PRIMARY KEY,           -- GENL, AUTH, API, etc.
    name TEXT NOT NULL,
    description TEXT,
    is_reserved BOOLEAN DEFAULT FALSE,
    is_active BOOLEAN DEFAULT TRUE
);

-- Estimate types configuration
CREATE TABLE IF NOT EXISTS estimate_types (
    code TEXT PRIMARY KEY,           -- XS, S, M, L, XL
    name TEXT NOT NULL,
    description TEXT,
    sort_order INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE
);

-- =============================================================================
-- WORK GRAPH TABLES
-- =============================================================================

-- Epics table
CREATE TABLE IF NOT EXISTS epics (
    id TEXT PRIMARY KEY,              -- ULID PK: epic-{ulid} (e.g., epic-01ARZ3NDEKTSV4RRFFQ69G5FAV)
    format_id TEXT UNIQUE NOT NULL,   -- Human-readable: {AREA}-EPC-{NNN} (e.g., INF-EPC-001)
    title TEXT NOT NULL,
    summary TEXT,
    status TEXT DEFAULT 'draft'
        CHECK(status IN ('draft', 'planning', 'in_progress', 'blocked', 'complete', 'archived')),
    area_type TEXT NOT NULL,
    work_type TEXT NOT NULL,
    domain TEXT NOT NULL,
    is_ongoing BOOLEAN DEFAULT FALSE,
    file_scope TEXT,                   -- JSON array
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

CREATE INDEX IF NOT EXISTS idx_epics_status ON epics(status);
CREATE INDEX IF NOT EXISTS idx_epics_area ON epics(area_type);
CREATE INDEX IF NOT EXISTS idx_epics_work_type ON epics(work_type);
CREATE INDEX IF NOT EXISTS idx_epics_domain ON epics(domain);
CREATE INDEX IF NOT EXISTS idx_epics_format_id ON epics(format_id);
CREATE INDEX IF NOT EXISTS idx_epics_ongoing ON epics(area_type, work_type, is_ongoing)
    WHERE is_ongoing = TRUE;
CREATE INDEX IF NOT EXISTS idx_epics_active ON epics(status)
    WHERE status IN ('draft', 'planning', 'in_progress', 'blocked');

-- Tasks table
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,              -- ULID PK: task-{ulid} (e.g., task-01BRZ4PDFLUTW5SSGG70H6GBW)
    format_id TEXT UNIQUE NOT NULL,   -- Human-readable: {AREA}-TSK-{NNN}-{NNN} (e.g., INF-TSK-001-001)
    epic_id TEXT NOT NULL REFERENCES epics(id),  -- FK to epics ULID PK
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'todo'
        CHECK(status IN ('todo', 'blocked', 'in_progress', 'awaiting_review', 'complete')),
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

-- Task dependencies
CREATE TABLE IF NOT EXISTS task_dependencies (
    task_id TEXT REFERENCES tasks(id),       -- FK to tasks ULID PK
    depends_on_id TEXT REFERENCES tasks(id), -- FK to tasks ULID PK
    dependency_type TEXT DEFAULT 'blocked_by'
        CHECK(dependency_type IN ('blocked_by', 'related')),
    PRIMARY KEY (task_id, depends_on_id)
);

CREATE INDEX IF NOT EXISTS idx_task_dependencies_depends_on ON task_dependencies(depends_on_id);

-- Acceptance criteria
CREATE TABLE IF NOT EXISTS acceptance_criteria (
    id TEXT PRIMARY KEY,              -- ac-{ulid}
    epic_id TEXT NOT NULL REFERENCES epics(id),  -- FK to epics ULID PK
    criterion TEXT NOT NULL,
    met BOOLEAN DEFAULT FALSE,
    met_at TEXT,
    met_by TEXT REFERENCES tasks(id)  -- FK to tasks ULID PK
);

CREATE INDEX IF NOT EXISTS idx_acceptance_criteria_epic ON acceptance_criteria(epic_id);
CREATE INDEX IF NOT EXISTS idx_acceptance_criteria_met ON acceptance_criteria(epic_id, met)
    WHERE met = FALSE;

-- =============================================================================
-- MEMORY EVENTS TABLES
-- =============================================================================

-- Memory events table
CREATE TABLE IF NOT EXISTS memory_events (
    id TEXT PRIMARY KEY,               -- memory-{ulid}
    event_type TEXT NOT NULL
        CHECK(event_type IN ('progress', 'decision', 'milestone', 'blocker',
                             'stage_transition', 'stage_complete', 'rework_limit')),
    domain TEXT NOT NULL
        CHECK(domain IN ('planning', 'development', 'review', 'qa', 'ops', 'documentation')),
    work_id TEXT,                      -- Reference to active_work.id
    data TEXT NOT NULL,                -- JSON payload
    memory_type TEXT
        CHECK(memory_type IS NULL OR memory_type IN ('episodic', 'semantic', 'procedural')),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memory_events_domain ON memory_events(domain);
CREATE INDEX IF NOT EXISTS idx_memory_events_work_id ON memory_events(work_id);
CREATE INDEX IF NOT EXISTS idx_memory_events_type ON memory_events(event_type);
CREATE INDEX IF NOT EXISTS idx_memory_events_memory_type ON memory_events(memory_type);
CREATE INDEX IF NOT EXISTS idx_memory_events_created ON memory_events(created_at);

-- Entities table (Lite GraphRAG)
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,               -- entity-{ulid}
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL
        CHECK(entity_type IN ('person', 'component', 'file', 'concept', 'decision', 'library')),
    description TEXT,
    source_event_id TEXT REFERENCES memory_events(id),
    category_id TEXT,
    properties TEXT DEFAULT '{}',      -- JSON
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, entity_type)
);

CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
CREATE INDEX IF NOT EXISTS idx_entities_source_event ON entities(source_event_id);

-- Relationships table
CREATE TABLE IF NOT EXISTS relationships (
    id TEXT PRIMARY KEY,               -- rel-{ulid}
    source_entity_id TEXT NOT NULL REFERENCES entities(id),
    target_entity_id TEXT NOT NULL REFERENCES entities(id),
    relation_type TEXT NOT NULL
        CHECK(relation_type IN ('works_on', 'depends_on', 'uses', 'references', 'implements')),
    weight REAL DEFAULT 1.0,
    properties TEXT DEFAULT '{}',      -- JSON
    source_event_id TEXT REFERENCES memory_events(id),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_entity_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_entity_id);
CREATE INDEX IF NOT EXISTS idx_relationships_type ON relationships(relation_type);
CREATE INDEX IF NOT EXISTS idx_relationships_source_event ON relationships(source_event_id);

-- Memory categories
CREATE TABLE IF NOT EXISTS memory_categories (
    id TEXT PRIMARY KEY,               -- category-{ulid}
    event_id TEXT NOT NULL REFERENCES memory_events(id) UNIQUE,
    primary_category TEXT NOT NULL
        CHECK(primary_category IN ('progress', 'decision', 'milestone', 'blocker')),
    sub_category TEXT,
    confidence REAL DEFAULT 1.0
        CHECK(confidence >= 0.0 AND confidence <= 1.0),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memory_categories_event ON memory_categories(event_id);
CREATE INDEX IF NOT EXISTS idx_memory_categories_category ON memory_categories(primary_category);

-- Extraction queue
CREATE TABLE IF NOT EXISTS extraction_queue (
    id TEXT PRIMARY KEY,              -- extraction-{ulid}
    event_id TEXT NOT NULL REFERENCES memory_events(id),
    status TEXT DEFAULT 'pending'
        CHECK(status IN ('pending', 'processing', 'completed', 'failed')),
    priority INTEGER DEFAULT 0,
    attempts INTEGER DEFAULT 0,
    last_error TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    processed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_extraction_queue_status ON extraction_queue(status);
CREATE INDEX IF NOT EXISTS idx_extraction_queue_priority ON extraction_queue(priority DESC);
CREATE INDEX IF NOT EXISTS idx_extraction_queue_event ON extraction_queue(event_id);
CREATE INDEX IF NOT EXISTS idx_extraction_queue_pending ON extraction_queue(status, priority DESC)
    WHERE status = 'pending';

-- Topics
CREATE TABLE IF NOT EXISTS topics (
    id TEXT PRIMARY KEY,               -- topic-{ulid}
    domain TEXT NOT NULL
        CHECK(domain IN ('planning', 'development', 'general-work', 'documentation')),
    topic_slug TEXT NOT NULL,
    work_id TEXT,
    folder_path TEXT NOT NULL,
    file_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    archived_at TEXT,
    summary_id TEXT,
    UNIQUE(domain, topic_slug)
);

CREATE INDEX IF NOT EXISTS idx_topics_domain ON topics(domain);
CREATE INDEX IF NOT EXISTS idx_topics_work_id ON topics(work_id);
CREATE INDEX IF NOT EXISTS idx_topics_summary ON topics(summary_id);

-- Topic files
CREATE TABLE IF NOT EXISTS topic_files (
    id TEXT PRIMARY KEY,               -- topicfile-{ulid}
    topic_id TEXT NOT NULL REFERENCES topics(id),
    filename TEXT NOT NULL,
    file_type TEXT NOT NULL
        CHECK(file_type IN ('decisions', 'analysis', 'notes', 'implementation')),
    file_path TEXT NOT NULL,
    last_modified TEXT,
    content_hash TEXT,
    line_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(topic_id, filename)
);

CREATE INDEX IF NOT EXISTS idx_topic_files_topic ON topic_files(topic_id);
CREATE INDEX IF NOT EXISTS idx_topic_files_type ON topic_files(file_type);
CREATE INDEX IF NOT EXISTS idx_topic_files_path ON topic_files(file_path);

-- Decisions
CREATE TABLE IF NOT EXISTS decisions (
    id TEXT PRIMARY KEY,               -- decision-{ulid}
    work_id TEXT,
    topic_id TEXT REFERENCES topics(id),
    decision_number INTEGER NOT NULL,
    title TEXT NOT NULL,
    tier INTEGER NOT NULL
        CHECK(tier IN (1, 2, 3)),
    context TEXT,
    decision_text TEXT NOT NULL,
    rationale TEXT,
    alternatives TEXT,                 -- JSON array
    impact TEXT,
    decided_by TEXT,
    event_id TEXT REFERENCES memory_events(id),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(work_id, decision_number)
);

CREATE INDEX IF NOT EXISTS idx_decisions_work_id ON decisions(work_id);
CREATE INDEX IF NOT EXISTS idx_decisions_topic_id ON decisions(topic_id);
CREATE INDEX IF NOT EXISTS idx_decisions_tier ON decisions(tier);
CREATE INDEX IF NOT EXISTS idx_decisions_event ON decisions(event_id);

-- Long-term summaries
CREATE TABLE IF NOT EXISTS long_term_summaries (
    id TEXT PRIMARY KEY,               -- summary-{ulid}
    original_work_id TEXT NOT NULL,
    domain TEXT NOT NULL
        CHECK(domain IN ('planning', 'development', 'review', 'qa', 'ops', 'documentation')),
    topic TEXT NOT NULL,
    summary_path TEXT NOT NULL,
    title TEXT,
    outcomes TEXT,                     -- JSON array
    key_decisions_count INTEGER DEFAULT 0,
    key_artifacts TEXT,                -- JSON array
    lessons_learned TEXT,              -- JSON array
    search_keywords TEXT,
    completed_at TEXT NOT NULL,
    archived_at TEXT NOT NULL,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(original_work_id, topic)
);

CREATE INDEX IF NOT EXISTS idx_long_term_summaries_domain ON long_term_summaries(domain);
CREATE INDEX IF NOT EXISTS idx_long_term_summaries_work ON long_term_summaries(original_work_id);
CREATE INDEX IF NOT EXISTS idx_long_term_summaries_topic ON long_term_summaries(topic);
CREATE INDEX IF NOT EXISTS idx_long_term_summaries_archived ON long_term_summaries(archived_at);

-- Memory chunks (for embeddings)
CREATE TABLE IF NOT EXISTS memory_chunks (
    id TEXT PRIMARY KEY,               -- chunk-{ulid}
    event_id TEXT NOT NULL REFERENCES memory_events(id),
    chunk_index INTEGER NOT NULL,
    chunk_text TEXT NOT NULL,
    char_offset_start INTEGER,
    char_offset_end INTEGER,
    token_estimate INTEGER,
    has_overlap_prev BOOLEAN DEFAULT FALSE,
    has_overlap_next BOOLEAN DEFAULT FALSE,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(event_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_memory_chunks_event ON memory_chunks(event_id);
CREATE INDEX IF NOT EXISTS idx_memory_chunks_event_order ON memory_chunks(event_id, chunk_index);

-- Chunk embeddings
CREATE TABLE IF NOT EXISTS chunk_embeddings (
    chunk_id TEXT PRIMARY KEY REFERENCES memory_chunks(id),
    embedding BLOB NOT NULL,           -- 384 floats for all-MiniLM-L6-v2
    model_version TEXT DEFAULT 'all-MiniLM-L6-v2',
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- =============================================================================
-- ACTIVE WORK TABLES
-- =============================================================================

-- Active work
CREATE TABLE IF NOT EXISTS active_work (
    id TEXT PRIMARY KEY,               -- work-{ulid}
    task_id TEXT REFERENCES tasks(id),
    topic TEXT NOT NULL,
    status TEXT DEFAULT 'in_progress'
        CHECK(status IN ('in_progress', 'complete', 'blocked')),
    branch TEXT,
    scope TEXT,                        -- JSON array
    deliverables TEXT,                 -- JSON array
    agent TEXT,
    session_id TEXT,
    current_stage TEXT DEFAULT NULL
        CHECK(current_stage IS NULL OR current_stage IN ('dev', 'work', 'review', 'qa')),
    team_name TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_active_work_status ON active_work(status);
CREATE INDEX IF NOT EXISTS idx_active_work_task ON active_work(task_id);
CREATE INDEX IF NOT EXISTS idx_active_work_branch ON active_work(branch);
CREATE INDEX IF NOT EXISTS idx_active_work_session ON active_work(session_id);
CREATE INDEX IF NOT EXISTS idx_active_work_stage ON active_work(current_stage);
CREATE INDEX IF NOT EXISTS idx_active_work_team ON active_work(team_name);

-- Work claims
CREATE TABLE IF NOT EXISTS work_claims (
    id TEXT PRIMARY KEY,              -- claim-{ulid}
    work_id TEXT NOT NULL REFERENCES active_work(id),
    pattern TEXT NOT NULL,
    mode TEXT DEFAULT 'exclusive'
        CHECK(mode IN ('exclusive', 'shared')),
    owner_id TEXT NOT NULL REFERENCES users(id),
    owner_host TEXT,
    fencing_token INTEGER NOT NULL,
    expires_at TEXT,
    status TEXT DEFAULT 'active'
        CHECK(status IN ('active', 'released', 'contested', 'expired')),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_work_claims_pattern ON work_claims(pattern);
CREATE INDEX IF NOT EXISTS idx_work_claims_fencing ON work_claims(fencing_token);
CREATE INDEX IF NOT EXISTS idx_work_claims_work ON work_claims(work_id);
CREATE INDEX IF NOT EXISTS idx_work_claims_status ON work_claims(status);
CREATE INDEX IF NOT EXISTS idx_work_claims_expires ON work_claims(expires_at);
CREATE INDEX IF NOT EXISTS idx_work_claims_owner ON work_claims(owner_id);
CREATE INDEX IF NOT EXISTS idx_work_claims_active ON work_claims(status, pattern)
    WHERE status = 'active';

-- Worktrees
CREATE TABLE IF NOT EXISTS worktrees (
    path TEXT PRIMARY KEY,
    branch TEXT NOT NULL,
    agent TEXT,
    work_id TEXT REFERENCES active_work(id),
    purpose TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    last_activity TEXT
);

CREATE INDEX IF NOT EXISTS idx_worktrees_branch ON worktrees(branch);
CREATE INDEX IF NOT EXISTS idx_worktrees_work_id ON worktrees(work_id);

-- Operation log (idempotency)
CREATE TABLE IF NOT EXISTS operation_log (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    operation_type TEXT NOT NULL
        CHECK(operation_type IN ('begin-work', 'complete-work', 'create-claim', 'release-claim',
                                  'sync-crdt', 'rebuild-crdt')),
    result TEXT NOT NULL
        CHECK(result IN ('success', 'failed', 'partial')),
    work_id TEXT,
    session_id TEXT,
    error_message TEXT,
    completed_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_operation_log_key ON operation_log(idempotency_key);
CREATE INDEX IF NOT EXISTS idx_operation_log_type ON operation_log(operation_type);
CREATE INDEX IF NOT EXISTS idx_operation_log_work ON operation_log(work_id);
CREATE INDEX IF NOT EXISTS idx_operation_log_completed ON operation_log(completed_at);

-- Recovery queue
CREATE TABLE IF NOT EXISTS recovery_queue (
    id TEXT PRIMARY KEY,
    operation_type TEXT NOT NULL
        CHECK(operation_type IN ('jsonl_append', 'crdt_update', 'sqlite_sync')),
    target_path TEXT,
    operation_data TEXT NOT NULL,
    status TEXT DEFAULT 'pending'
        CHECK(status IN ('pending', 'processing', 'recovered', 'failed')),
    retry_count INTEGER DEFAULT 0,
    max_retries INTEGER DEFAULT 3,
    last_error TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    last_attempted_at TEXT,
    recovered_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_recovery_queue_status ON recovery_queue(status);
CREATE INDEX IF NOT EXISTS idx_recovery_queue_pending ON recovery_queue(status, created_at)
    WHERE status = 'pending';

-- =============================================================================
-- SESSION TABLES
-- =============================================================================

-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,              -- session-{ulid}
    project_id TEXT REFERENCES project_config(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    user_host TEXT NOT NULL,
    machine_fingerprint TEXT,
    started_at TEXT DEFAULT CURRENT_TIMESTAMP,
    ended_at TEXT,
    duration_seconds INTEGER,
    status TEXT DEFAULT 'active'
        CHECK(status IN ('active', 'paused', 'completed', 'crashed', 'archived')),
    work_ids TEXT,                    -- JSON array
    previous_session_id TEXT REFERENCES sessions(id),
    context_summary TEXT,
    tool_stats TEXT,                  -- JSON
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_sessions_previous ON sessions(previous_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);

-- Conversation logs
CREATE TABLE IF NOT EXISTS conversation_logs (
    id TEXT PRIMARY KEY,              -- conv-{ulid}
    role TEXT NOT NULL
        CHECK(role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    tool_calls TEXT,                  -- JSON array
    turn_number INTEGER,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_conversation_logs_session ON conversation_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_conversation_logs_turn ON conversation_logs(session_id, turn_number);
CREATE INDEX IF NOT EXISTS idx_conversation_logs_role ON conversation_logs(session_id, role);

-- =============================================================================
-- AUDIT TABLES
-- =============================================================================

-- Security logs
CREATE TABLE IF NOT EXISTS security_logs (
    id TEXT PRIMARY KEY,               -- seclog-{ulid}
    log_type TEXT NOT NULL
        CHECK(log_type IN ('protection', 'blocked', 'sentinel')),
    event_type TEXT NOT NULL
        CHECK(event_type IN ('PROTECT', 'UNPROTECT', 'BLOCKED', 'CREATE', 'VALIDATE', 'EXPIRE')),
    tool TEXT,
    target TEXT,
    reason TEXT,
    skill TEXT,
    operation TEXT,
    outcome TEXT,
    session_id TEXT REFERENCES sessions(id),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_security_logs_type ON security_logs(log_type);
CREATE INDEX IF NOT EXISTS idx_security_logs_event ON security_logs(event_type);
CREATE INDEX IF NOT EXISTS idx_security_logs_session ON security_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_security_logs_created ON security_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_security_logs_tool ON security_logs(tool);

-- Network logs
CREATE TABLE IF NOT EXISTS network_logs (
    id TEXT PRIMARY KEY,               -- netlog-{ulid}
    operation TEXT NOT NULL
        CHECK(operation IN ('WebFetch', 'WebSearch', 'curl', 'wget')),
    url TEXT NOT NULL,
    domain TEXT,
    method TEXT,
    status_code INTEGER,
    response_size INTEGER,
    duration_ms INTEGER,
    purpose TEXT,
    work_id TEXT,
    session_id TEXT REFERENCES sessions(id),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_network_logs_session ON network_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_network_logs_created ON network_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_network_logs_work ON network_logs(work_id);
CREATE INDEX IF NOT EXISTS idx_network_logs_domain ON network_logs(domain);
CREATE INDEX IF NOT EXISTS idx_network_logs_operation ON network_logs(operation);

-- =============================================================================
-- AUTORUN TABLES
-- =============================================================================

-- Autorun sessions
CREATE TABLE IF NOT EXISTS autorun_sessions (
    id TEXT PRIMARY KEY,
    batch_file TEXT NOT NULL,
    batch_name TEXT,
    status TEXT DEFAULT 'pending'
        CHECK(status IN ('pending', 'running', 'paused', 'completed',
                         'failed', 'cancelled', 'timeout')),
    max_session_workers INTEGER DEFAULT 3,
    total_tasks INTEGER DEFAULT 0,
    completed_tasks INTEGER DEFAULT 0,
    failed_tasks INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_autorun_sessions_status ON autorun_sessions(status);
CREATE INDEX IF NOT EXISTS idx_autorun_sessions_created ON autorun_sessions(created_at);

-- Autorun workers
CREATE TABLE IF NOT EXISTS autorun_workers (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES autorun_sessions(id),
    worker_num INTEGER NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    status TEXT DEFAULT 'queued'
        CHECK(status IN ('queued', 'starting', 'running', 'completed',
                         'failed', 'skipped', 'timeout', 'cancelled')),
    tmux_session TEXT,
    worktree_path TEXT,
    pr_number INTEGER,
    started_at TEXT,
    completed_at TEXT,
    UNIQUE(session_id, worker_num)
);

CREATE INDEX IF NOT EXISTS idx_autorun_workers_session ON autorun_workers(session_id);
CREATE INDEX IF NOT EXISTS idx_autorun_workers_status ON autorun_workers(status);
CREATE INDEX IF NOT EXISTS idx_autorun_workers_task ON autorun_workers(task_id);

-- Autorun task runs
CREATE TABLE IF NOT EXISTS autorun_task_runs (
    id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL REFERENCES autorun_workers(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    session_id TEXT NOT NULL REFERENCES autorun_sessions(id),
    status TEXT DEFAULT 'pending'
        CHECK(status IN ('pending', 'running', 'completed', 'failed', 'skipped', 'timeout', 'cancelled')),
    branch_name TEXT,
    worktree_path TEXT,
    pr_number INTEGER,
    pr_url TEXT,
    started_at TEXT,
    completed_at TEXT,
    duration_seconds INTEGER,
    exit_code INTEGER,
    error_message TEXT,
    verification_result TEXT
        CHECK(verification_result IS NULL OR json_valid(verification_result)),
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_autorun_task_runs_session ON autorun_task_runs(session_id);
CREATE INDEX IF NOT EXISTS idx_autorun_task_runs_worker ON autorun_task_runs(worker_id);
CREATE INDEX IF NOT EXISTS idx_autorun_task_runs_task ON autorun_task_runs(task_id);
CREATE INDEX IF NOT EXISTS idx_autorun_task_runs_status ON autorun_task_runs(status);
CREATE INDEX IF NOT EXISTS idx_autorun_task_runs_pr ON autorun_task_runs(pr_number)
    WHERE pr_number IS NOT NULL;

-- =============================================================================
-- SEARCH INDICES (FTS5)
-- =============================================================================

-- Memory full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    id,
    data
);

-- Trigger to keep FTS in sync
CREATE TRIGGER IF NOT EXISTS memory_fts_insert AFTER INSERT ON memory_events BEGIN
    INSERT INTO memory_fts(id, data) VALUES (NEW.id, NEW.data);
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_delete AFTER DELETE ON memory_events BEGIN
    DELETE FROM memory_fts WHERE id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS memory_fts_update AFTER UPDATE ON memory_events BEGIN
    DELETE FROM memory_fts WHERE id = OLD.id;
    INSERT INTO memory_fts(id, data) VALUES (NEW.id, NEW.data);
END;

-- Entities full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
    id,
    name
);

CREATE TRIGGER IF NOT EXISTS entities_fts_insert AFTER INSERT ON entities BEGIN
    INSERT INTO entities_fts(id, name) VALUES (NEW.id, NEW.name);
END;

CREATE TRIGGER IF NOT EXISTS entities_fts_delete AFTER DELETE ON entities BEGIN
    DELETE FROM entities_fts WHERE id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS entities_fts_update AFTER UPDATE ON entities BEGIN
    DELETE FROM entities_fts WHERE id = OLD.id;
    INSERT INTO entities_fts(id, name) VALUES (NEW.id, NEW.name);
END;

-- Summaries full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS summaries_fts USING fts5(
    id,
    title,
    outcomes,
    lessons_learned,
    search_keywords
);

CREATE TRIGGER IF NOT EXISTS summaries_fts_insert AFTER INSERT ON long_term_summaries BEGIN
    INSERT INTO summaries_fts(id, title, outcomes, lessons_learned, search_keywords)
    VALUES (NEW.id, NEW.title, NEW.outcomes, NEW.lessons_learned, NEW.search_keywords);
END;

CREATE TRIGGER IF NOT EXISTS summaries_fts_delete AFTER DELETE ON long_term_summaries BEGIN
    DELETE FROM summaries_fts WHERE id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS summaries_fts_update AFTER UPDATE ON long_term_summaries BEGIN
    DELETE FROM summaries_fts WHERE id = OLD.id;
    INSERT INTO summaries_fts(id, title, outcomes, lessons_learned, search_keywords)
    VALUES (NEW.id, NEW.title, NEW.outcomes, NEW.lessons_learned, NEW.search_keywords);
END;

-- =============================================================================
-- SEED DATA
-- =============================================================================

-- Insert schema version
INSERT OR IGNORE INTO schema_version (version) VALUES (2);

-- Seed domains
INSERT OR IGNORE INTO domains (code, name, is_reserved) VALUES ('GENL', 'General', TRUE);
INSERT OR IGNORE INTO domains (code, name, description, is_reserved) VALUES ('QUAL', 'Quality Assurance', 'Testing and QA', FALSE);
INSERT OR IGNORE INTO domains (code, name, description, is_reserved) VALUES ('PMGT', 'Project Management', 'Project management and tracking', FALSE);

-- Seed default estimates
INSERT OR IGNORE INTO estimate_types (code, name, description, sort_order) VALUES
    ('XS', 'Extra Small', 'Trivial change, <1 hour', 1),
    ('S', 'Small', 'Simple change, 1-4 hours', 2),
    ('M', 'Medium', 'Moderate complexity, 1-2 days', 3),
    ('L', 'Large', 'Significant work, 3-5 days', 4),
    ('XL', 'Extra Large', 'Major feature, 1+ week', 5);

-- Seed default area types
INSERT OR IGNORE INTO area_types (code, name, description) VALUES
    ('FRT', 'Frontend', 'User-facing frontend code'),
    ('BKD', 'Backend', 'Server-side backend code'),
    ('INF', 'Infrastructure', 'CI/CD, deployment, cloud'),
    ('SHR', 'Shared', 'Libraries used by multiple areas'),
    ('DOC', 'Documentation', 'Documentation only'),
    ('PLN', 'Planning', 'Planning, epics, roadmaps, ADRs');

-- ============================================================================
-- AREA FOLDER MAPPING
-- Maps area type codes to filesystem folder names.
-- Convention: folder_name is the lowercase of the area code itself.
-- ============================================================================

CREATE TABLE IF NOT EXISTS area_folder_mapping (
    area_type TEXT NOT NULL REFERENCES area_types(code),
    folder_name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    PRIMARY KEY (area_type)
);

INSERT OR IGNORE INTO area_folder_mapping (area_type, folder_name, display_name) VALUES
    ('FRT', 'frt', 'Frontend'),
    ('BKD', 'bkd', 'Backend'),
    ('INF', 'inf', 'Infrastructure'),
    ('SHR', 'shr', 'Shared'),
    ('DOC', 'doc', 'Documentation'),
    ('PLN', 'pln', 'Planning');

-- Seed default work types
INSERT OR IGNORE INTO work_types (code, name, branch_prefix, commit_type, urgency) VALUES
    ('FEAT', 'Feature', 'feat/', 'feat', 'normal'),
    ('FIX', 'Bug Fix', 'fix/', 'fix', 'normal'),
    ('HTFX', 'Hotfix', 'hotfix/', 'fix', 'critical'),
    ('RFCT', 'Refactor', 'refactor/', 'refactor', 'normal'),
    ('DOCS', 'Documentation', 'docs/', 'docs', 'normal'),
    ('TEST', 'Test', 'test/', 'test', 'normal'),
    ('CHOR', 'Chore', 'chore/', 'chore', 'normal'),
    ('CICD', 'CI/CD', 'cicd/', 'ci', 'normal'),
    ('SPKE', 'Spike', 'spike/', 'chore', 'normal'),
    ('PLAN', 'Plan', 'plan/', 'plan', 'normal');
