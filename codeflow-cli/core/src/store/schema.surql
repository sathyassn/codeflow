-- CodeFlow SurrealDB Schema
-- Applied idempotently on every startup via OVERWRITE.
-- Order: tables -> fields -> indexes -> relation tables -> vector indexes.
-- Translates Go's 37 SQLite tables, consolidating where SurrealDB's
-- nested objects and graph relations reduce table count.

-- =============================================================================
-- CORE TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE project_config SCHEMAFULL;
DEFINE FIELD OVERWRITE name            ON TABLE project_config TYPE string;
DEFINE FIELD OVERWRITE description     ON TABLE project_config TYPE option<string>;
DEFINE FIELD OVERWRITE git_remote_url  ON TABLE project_config TYPE option<string>;
DEFINE FIELD OVERWRITE default_branch  ON TABLE project_config TYPE string;
DEFINE FIELD OVERWRITE created_at      ON TABLE project_config TYPE string;
DEFINE FIELD OVERWRITE updated_at      ON TABLE project_config TYPE string;
DEFINE FIELD OVERWRITE metadata        ON TABLE project_config TYPE option<object>;

DEFINE TABLE OVERWRITE user SCHEMAFULL;
DEFINE FIELD OVERWRITE email                  ON TABLE user TYPE string;
DEFINE FIELD OVERWRITE display_name           ON TABLE user TYPE option<string>;
DEFINE FIELD OVERWRITE git_username           ON TABLE user TYPE option<string>;
DEFINE FIELD OVERWRITE role                   ON TABLE user TYPE string;
DEFINE FIELD OVERWRITE last_host              ON TABLE user TYPE option<string>;
DEFINE FIELD OVERWRITE last_machine_fingerprint ON TABLE user TYPE option<string>;
DEFINE FIELD OVERWRITE first_seen_at          ON TABLE user TYPE string;
DEFINE FIELD OVERWRITE last_active_at         ON TABLE user TYPE option<string>;
DEFINE FIELD OVERWRITE preferences            ON TABLE user TYPE option<object>;
DEFINE FIELD OVERWRITE metadata               ON TABLE user TYPE option<object>;

DEFINE INDEX OVERWRITE idx_user_email ON TABLE user FIELDS email UNIQUE;
DEFINE INDEX OVERWRITE idx_user_git_username ON TABLE user FIELDS git_username;

-- =============================================================================
-- WORK GRAPH TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE epic SCHEMAFULL;
DEFINE FIELD OVERWRITE format_id   ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE title       ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE summary     ON TABLE epic TYPE option<string>;
DEFINE FIELD OVERWRITE status      ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE area_type   ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE work_type   ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE domain      ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE is_ongoing  ON TABLE epic TYPE bool;
DEFINE FIELD OVERWRITE file_scope  ON TABLE epic TYPE array<string>;
DEFINE FIELD OVERWRITE priority    ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE pr_number   ON TABLE epic TYPE option<int>;
DEFINE FIELD OVERWRITE external_id ON TABLE epic TYPE option<string>;
DEFINE FIELD OVERWRITE external_url ON TABLE epic TYPE option<string>;
DEFINE FIELD OVERWRITE created_at  ON TABLE epic TYPE string;
DEFINE FIELD OVERWRITE updated_at  ON TABLE epic TYPE string;

DEFINE INDEX OVERWRITE idx_epic_format_id ON TABLE epic FIELDS format_id UNIQUE;
DEFINE INDEX OVERWRITE idx_epic_status    ON TABLE epic FIELDS status;
DEFINE INDEX OVERWRITE idx_epic_area_type ON TABLE epic FIELDS area_type;
DEFINE INDEX OVERWRITE idx_epic_work_type ON TABLE epic FIELDS work_type;

DEFINE TABLE OVERWRITE task SCHEMAFULL;
DEFINE FIELD OVERWRITE format_id        ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE epic_id          ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE title            ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE description      ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE status           ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE area_type        ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE work_type        ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE domain           ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE origin           ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE file_scope       ON TABLE task TYPE array<string>;
DEFINE FIELD OVERWRITE scope_policy     ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE scope_root       ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE estimate         ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE priority         ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE assignee_id      ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE autorun_eligible ON TABLE task TYPE bool;
DEFINE FIELD OVERWRITE auto_commit      ON TABLE task TYPE bool;
DEFINE FIELD OVERWRITE raise_pr         ON TABLE task TYPE bool;
DEFINE FIELD OVERWRITE auto_merge       ON TABLE task TYPE bool;
DEFINE FIELD OVERWRITE target_branch    ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE acceptance       ON TABLE task TYPE array<string>;
DEFINE FIELD OVERWRITE tests            ON TABLE task TYPE array<string>;
DEFINE FIELD OVERWRITE branch           ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE pr_number        ON TABLE task TYPE option<int>;
DEFINE FIELD OVERWRITE external_id      ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE external_url     ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE created_at       ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE updated_at       ON TABLE task TYPE string;
DEFINE FIELD OVERWRITE started_at       ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE completed_at     ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE stage            ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE stage_status     ON TABLE task TYPE option<string>;
DEFINE FIELD OVERWRITE stage_history    ON TABLE task TYPE array;

DEFINE INDEX OVERWRITE idx_task_format_id ON TABLE task FIELDS format_id UNIQUE;
DEFINE INDEX OVERWRITE idx_task_epic_id   ON TABLE task FIELDS epic_id;
DEFINE INDEX OVERWRITE idx_task_status    ON TABLE task FIELDS status;
DEFINE INDEX OVERWRITE idx_task_branch    ON TABLE task FIELDS branch;
DEFINE INDEX OVERWRITE idx_task_assignee  ON TABLE task FIELDS assignee_id;

-- =============================================================================
-- SESSION TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE session SCHEMAFULL;
DEFINE FIELD OVERWRITE project_id          ON TABLE session TYPE option<string>;
DEFINE FIELD OVERWRITE user_id             ON TABLE session TYPE string;
DEFINE FIELD OVERWRITE user_host           ON TABLE session TYPE string;
DEFINE FIELD OVERWRITE machine_fingerprint ON TABLE session TYPE option<string>;
DEFINE FIELD OVERWRITE started_at          ON TABLE session TYPE string;
DEFINE FIELD OVERWRITE ended_at            ON TABLE session TYPE option<string>;
DEFINE FIELD OVERWRITE duration_seconds    ON TABLE session TYPE option<int>;
DEFINE FIELD OVERWRITE status              ON TABLE session TYPE string;
DEFINE FIELD OVERWRITE work_ids            ON TABLE session TYPE array<string>;
DEFINE FIELD OVERWRITE previous_session_id ON TABLE session TYPE option<string>;
DEFINE FIELD OVERWRITE context_summary     ON TABLE session TYPE option<string>;
DEFINE FIELD OVERWRITE tool_stats          ON TABLE session TYPE option<object>;
DEFINE FIELD OVERWRITE metadata            ON TABLE session TYPE option<object>;

DEFINE INDEX OVERWRITE idx_session_status  ON TABLE session FIELDS status;
DEFINE INDEX OVERWRITE idx_session_user_id ON TABLE session FIELDS user_id;

-- =============================================================================
-- ACTIVE WORK TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE active_work SCHEMAFULL;
DEFINE FIELD OVERWRITE task_id       ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE topic         ON TABLE active_work TYPE string;
DEFINE FIELD OVERWRITE status        ON TABLE active_work TYPE string;
DEFINE FIELD OVERWRITE branch        ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE scope         ON TABLE active_work TYPE array<string>;
DEFINE FIELD OVERWRITE deliverables  ON TABLE active_work TYPE array<string>;
DEFINE FIELD OVERWRITE agent         ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE session_id    ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE current_stage ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE team_name     ON TABLE active_work TYPE option<string>;
DEFINE FIELD OVERWRITE created_at    ON TABLE active_work TYPE string;
DEFINE FIELD OVERWRITE updated_at    ON TABLE active_work TYPE string;

DEFINE INDEX OVERWRITE idx_active_work_status ON TABLE active_work FIELDS status;

-- =============================================================================
-- MEMORY TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE memory_event SCHEMAFULL;
DEFINE FIELD OVERWRITE event_type  ON TABLE memory_event TYPE string;
DEFINE FIELD OVERWRITE domain      ON TABLE memory_event TYPE string;
DEFINE FIELD OVERWRITE work_id     ON TABLE memory_event TYPE option<string>;
DEFINE FIELD OVERWRITE data        ON TABLE memory_event TYPE string;
DEFINE FIELD OVERWRITE memory_type ON TABLE memory_event TYPE option<string>;
DEFINE FIELD OVERWRITE created_at  ON TABLE memory_event TYPE string;
DEFINE FIELD OVERWRITE embedding   ON TABLE memory_event TYPE option<array<float>>;

DEFINE INDEX OVERWRITE idx_memory_event_domain ON TABLE memory_event FIELDS domain;
DEFINE INDEX OVERWRITE idx_memory_event_type   ON TABLE memory_event FIELDS event_type;

-- =============================================================================
-- AUTORUN TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE autorun_session SCHEMAFULL;
DEFINE FIELD OVERWRITE batch_file          ON TABLE autorun_session TYPE string;
DEFINE FIELD OVERWRITE batch_name          ON TABLE autorun_session TYPE option<string>;
DEFINE FIELD OVERWRITE status              ON TABLE autorun_session TYPE string;
DEFINE FIELD OVERWRITE max_session_workers ON TABLE autorun_session TYPE int;
DEFINE FIELD OVERWRITE total_tasks         ON TABLE autorun_session TYPE int;
DEFINE FIELD OVERWRITE completed_tasks     ON TABLE autorun_session TYPE int;
DEFINE FIELD OVERWRITE failed_tasks        ON TABLE autorun_session TYPE int;
DEFINE FIELD OVERWRITE pid                 ON TABLE autorun_session TYPE option<int>;
DEFINE FIELD OVERWRITE skipped_tasks       ON TABLE autorun_session TYPE int;
DEFINE FIELD OVERWRITE created_at          ON TABLE autorun_session TYPE string;
DEFINE FIELD OVERWRITE completed_at        ON TABLE autorun_session TYPE option<string>;

DEFINE INDEX OVERWRITE idx_session_created ON autorun_session FIELDS created_at;

DEFINE TABLE OVERWRITE autorun_worker SCHEMAFULL;
DEFINE FIELD OVERWRITE session_id    ON TABLE autorun_worker TYPE string;
DEFINE FIELD OVERWRITE worker_num    ON TABLE autorun_worker TYPE int;
DEFINE FIELD OVERWRITE task_id       ON TABLE autorun_worker TYPE string;
DEFINE FIELD OVERWRITE status        ON TABLE autorun_worker TYPE string;
DEFINE FIELD OVERWRITE tmux_session  ON TABLE autorun_worker TYPE option<string>;
DEFINE FIELD OVERWRITE worktree_path ON TABLE autorun_worker TYPE option<string>;
DEFINE FIELD OVERWRITE file_scope    ON TABLE autorun_worker TYPE array<string>;
DEFINE FIELD OVERWRITE scope_policy  ON TABLE autorun_worker TYPE string;
DEFINE FIELD OVERWRITE worker_session_id ON TABLE autorun_worker TYPE option<string>;
DEFINE FIELD OVERWRITE pr_number     ON TABLE autorun_worker TYPE option<int>;
DEFINE FIELD OVERWRITE started_at    ON TABLE autorun_worker TYPE option<string>;
DEFINE FIELD OVERWRITE completed_at  ON TABLE autorun_worker TYPE option<string>;

DEFINE INDEX OVERWRITE idx_worker_session ON autorun_worker FIELDS session_id;

DEFINE TABLE OVERWRITE autorun_task_run SCHEMAFULL;
DEFINE FIELD OVERWRITE worker_id           ON TABLE autorun_task_run TYPE string;
DEFINE FIELD OVERWRITE task_id             ON TABLE autorun_task_run TYPE string;
DEFINE FIELD OVERWRITE session_id          ON TABLE autorun_task_run TYPE string;
DEFINE FIELD OVERWRITE status              ON TABLE autorun_task_run TYPE string;
DEFINE FIELD OVERWRITE branch_name         ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE worktree_path       ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE pr_number           ON TABLE autorun_task_run TYPE option<int>;
DEFINE FIELD OVERWRITE pr_url              ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE blocked_reason      ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE claim_conflicts     ON TABLE autorun_task_run TYPE option<array<string>>;
DEFINE FIELD OVERWRITE merge_conflicts     ON TABLE autorun_task_run TYPE option<array<string>>;
DEFINE FIELD OVERWRITE started_at          ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE completed_at        ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE duration_seconds    ON TABLE autorun_task_run TYPE option<int>;
DEFINE FIELD OVERWRITE exit_code           ON TABLE autorun_task_run TYPE option<int>;
DEFINE FIELD OVERWRITE error_message       ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE verification_result ON TABLE autorun_task_run TYPE option<string>;
DEFINE FIELD OVERWRITE created_at          ON TABLE autorun_task_run TYPE string;

DEFINE INDEX OVERWRITE idx_taskrun_session ON autorun_task_run FIELDS session_id;

-- =============================================================================
-- INTERACTIVE SESSION TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE interactive_session SCHEMAFULL;
DEFINE FIELD OVERWRITE session_id    ON TABLE interactive_session TYPE string;
DEFINE FIELD OVERWRITE pid           ON TABLE interactive_session TYPE int;
DEFINE FIELD OVERWRITE status        ON TABLE interactive_session TYPE string;
DEFINE FIELD OVERWRITE worktree_path ON TABLE interactive_session TYPE option<string>;
DEFINE FIELD OVERWRITE branch        ON TABLE interactive_session TYPE option<string>;
DEFINE FIELD OVERWRITE work_type     ON TABLE interactive_session TYPE option<string>;
DEFINE FIELD OVERWRITE team_name     ON TABLE interactive_session TYPE option<string>;
DEFINE FIELD OVERWRITE source_cli    ON TABLE interactive_session TYPE string;
DEFINE FIELD OVERWRITE managed       ON TABLE interactive_session TYPE bool;
DEFINE FIELD OVERWRITE created_at    ON TABLE interactive_session TYPE string;
DEFINE FIELD OVERWRITE updated_at    ON TABLE interactive_session TYPE option<string>;
DEFINE FIELD OVERWRITE completed_at  ON TABLE interactive_session TYPE option<string>;

DEFINE INDEX OVERWRITE idx_interactive_session_sid    ON interactive_session FIELDS session_id UNIQUE;
DEFINE INDEX OVERWRITE idx_interactive_session_status ON interactive_session FIELDS status;

-- =============================================================================
-- COORDINATION TABLES
-- =============================================================================

DEFINE TABLE OVERWRITE coordination_event SCHEMAFULL;
DEFINE FIELD OVERWRITE event_type   ON TABLE coordination_event TYPE string;
DEFINE FIELD OVERWRITE session_id   ON TABLE coordination_event TYPE string;
DEFINE FIELD OVERWRITE path         ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE branch       ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE target_branch ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE held_by      ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE task_id      ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE success      ON TABLE coordination_event TYPE option<bool>;
DEFINE FIELD OVERWRITE original_scope ON TABLE coordination_event TYPE option<array<string>>;
DEFINE FIELD OVERWRITE timestamp    ON TABLE coordination_event TYPE string;

DEFINE INDEX OVERWRITE idx_coordination_event_type       ON TABLE coordination_event FIELDS event_type;
DEFINE INDEX OVERWRITE idx_coordination_event_session_id  ON TABLE coordination_event FIELDS session_id;

-- =============================================================================
-- GRAPH RELATION TABLES
-- =============================================================================

-- Task belongs to an epic (replaces foreign key task.epic_id -> epic.id).
DEFINE TABLE OVERWRITE belongs_to TYPE RELATION SCHEMAFULL;
DEFINE FIELD OVERWRITE created_at ON TABLE belongs_to TYPE string;

-- Task depends on another task (replaces task_dependencies junction table).
DEFINE TABLE OVERWRITE depends_on TYPE RELATION SCHEMAFULL;
DEFINE FIELD OVERWRITE dependency_type ON TABLE depends_on TYPE string;
DEFINE FIELD OVERWRITE created_at      ON TABLE depends_on TYPE string;

-- =============================================================================
-- VECTOR INDEXES (placeholder for Epic C embedding support)
-- =============================================================================

DEFINE INDEX OVERWRITE idx_memory_event_embedding
    ON TABLE memory_event
    FIELDS embedding
    HNSW DIMENSION 384 TYPE F32 DIST COSINE;
