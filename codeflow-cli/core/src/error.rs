use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("record not found: {table}:{id}")]
    NotFound { table: String, id: String },

    #[error("duplicate record: {table}:{id}")]
    Duplicate { table: String, id: String },

    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(String),

    #[error("schema migration failed: {0}")]
    Migration(String),

    #[error("transaction failed: {0}")]
    Transaction(String),

    #[error("integrity check failed: {0}")]
    IntegrityCheck(String),

    #[error("surrealdb error: {0}")]
    Surreal(#[from] surrealdb::Error),
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("misrouted event: {event_type} belongs to {expected}, not {actual}")]
    MisroutedEvent {
        event_type: String,
        expected: String,
        actual: String,
    },

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("lock acquisition failed: {0}")]
    Lock(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("blocked: {reason}")]
    Blocked { reason: String },

    #[error("sentinel not found: {0}")]
    SentinelNotFound(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("claim conflict: {0}")]
    ClaimConflict(String),

    #[error("token mismatch: expected {expected}, found {found}")]
    TokenMismatch { expected: u64, found: u64 },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("no active session")]
    NoActiveSession,

    #[error("session already ended: {0}")]
    AlreadyEnded(String),

    #[error("invalid session id: {0}")]
    InvalidSessionId(String),

    #[error("invalid transition from {from} to {to}")]
    InvalidTransition {
        from: crate::types::SessionStatus,
        to: crate::types::SessionStatus,
    },

    #[error("database error: {0}")]
    Db(Box<DbError>),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<DbError> for SessionError {
    fn from(err: DbError) -> Self {
        Self::Db(Box::new(err))
    }
}

/// Configuration loading and validation errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(String),

    #[error("config parse error: {0}")]
    Parse(String),

    #[error("config validation error: {0}")]
    Validation(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Workgraph operation errors.
#[derive(Debug, Error)]
pub enum WorkgraphError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid transition: {entity} from {from} to {to}")]
    InvalidTransition {
        entity: String,
        from: String,
        to: String,
    },

    #[error("validation error: {0}")]
    Validation(String),

    #[error("format ID generation error: {0}")]
    FormatIdGeneration(String),

    #[error("database error: {0}")]
    Db(Box<DbError>),

    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),
}

impl From<DbError> for WorkgraphError {
    fn from(err: DbError) -> Self {
        Self::Db(Box::new(err))
    }
}

/// Git worktree operation errors.
#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("worktree not found: {0}")]
    NotFound(String),

    #[error("worktree already exists: {0}")]
    AlreadyExists(String),

    #[error("worktree creation failed: {0}")]
    Creation(String),

    #[error("worktree cleanup failed: {0}")]
    Cleanup(String),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("cleanup blocked: PathFlow session is active, use force to override")]
    PathFlowActive,

    #[error("invalid worktree name: {0}")]
    InvalidName(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml error: {0}")]
    Yaml(String),
}

/// Infrastructure diagnostics errors.
#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("check not found: {0}")]
    CheckNotFound(String),

    #[error("check failed: {name}: {reason}")]
    CheckFailed { name: String, reason: String },

    #[error("repair failed: {name}: {reason}")]
    RepairFailed { name: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Markdown validation errors.
#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("invalid frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("validation failed: {field}: {message}")]
    FieldError { field: String, message: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml parse error: {0}")]
    Yaml(String),
}

/// ID generation errors.
#[derive(Debug, Error)]
pub enum IdgenError {
    #[error("ULID generation failed: {0}")]
    Generation(String),
}

/// `PathFlow` checkpoint and sentinel errors.
#[derive(Debug, Error)]
pub enum PathflowError {
    #[error("phase not initialized: {0}")]
    PhaseNotInitialized(String),

    #[error("invalid task ID format: {0}")]
    InvalidTaskId(String),

    #[error("cross-phase registration blocked: {0}")]
    CrossPhaseBlock(String),

    #[error("sentinel error: {0}")]
    Sentinel(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("lock error: {0}")]
    Lock(String),

    #[error("invalid phase transition: {0}")]
    InvalidTransition(String),
}

/// Settings validation errors.
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings validation failed: {0}")]
    Validation(String),

    #[error("template discovery failed: {0}")]
    TemplateDiscovery(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),
}

/// Sync daemon errors.
#[derive(Debug, Error)]
pub enum SyncError {
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("loro error: {0}")]
    LoroError(#[from] loro::LoroError),

    #[error("git error: {0}")]
    GitError(#[from] git2::Error),

    #[error("pid lock failed: {}", .0.display())]
    PidLockFailed(std::path::PathBuf),

    #[error("network partition: peer {peer}, retries exhausted ({retries})")]
    NetworkPartition {
        peer: crate::coordination::PeerId,
        retries: u32,
    },

    #[error("connection error: {0}")]
    ConnectionError(String),

    #[error("schema error: {0}")]
    SchemaError(String),

    #[error("loro encode error: {0}")]
    LoroEncodeError(#[from] loro::LoroEncodeError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl SyncError {
    /// Returns `true` if this error is retryable (connection/timeout).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionError(_)
                | Self::GitError(_)
                | Self::IoError(_)
                | Self::NetworkPartition { .. }
        )
    }

    /// Returns `true` if this error is a schema/logic error (not retryable).
    #[must_use]
    pub fn is_schema_error(&self) -> bool {
        matches!(
            self,
            Self::SchemaError(_) | Self::LoroError(_) | Self::LoroEncodeError(_)
        )
    }
}

/// CRDT coordination errors.
#[derive(Debug, Error)]
pub enum CoordinationError {
    #[error("claim conflict: path '{path}' already owned by {owner}")]
    ClaimConflict {
        path: String,
        owner: crate::types::SessionId,
    },

    #[error("fencing token mismatch: expected {expected}, found {found}")]
    TokenMismatch { expected: u64, found: u64 },

    #[error("container not found: {0}")]
    ContainerNotFound(String),

    #[error("loro error: {0}")]
    Loro(#[from] loro::LoroError),

    #[error("loro encode error: {0}")]
    LoroEncode(#[from] loro::LoroEncodeError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Autorun batch parsing and orchestration errors.
#[derive(Debug, Error)]
pub enum AutorunError {
    #[error("invalid batch file: {0}")]
    InvalidBatch(String),

    #[error("dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("missing task: {0}")]
    MissingTask(String),

    #[error("protected merge: {0}")]
    ProtectedMerge(String),

    #[error("worker timeout: {0}")]
    WorkerTimeout(String),

    #[error("worker failed: {0}")]
    WorkerFailed(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("yaml parse error: {0}")]
    Yaml(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- DbError --

    #[test]
    fn test_db_error_not_found() {
        let err = DbError::NotFound {
            table: "tasks".into(),
            id: "task-123".into(),
        };
        assert_eq!(err.to_string(), "record not found: tasks:task-123");
    }

    #[test]
    fn test_db_error_duplicate() {
        let err = DbError::Duplicate {
            table: "sessions".into(),
            id: "ses-abc".into(),
        };
        assert_eq!(err.to_string(), "duplicate record: sessions:ses-abc");
    }

    #[test]
    fn test_db_error_connection() {
        let err = DbError::Connection("timeout after 30s".into());
        assert_eq!(err.to_string(), "connection failed: timeout after 30s");
    }

    #[test]
    fn test_db_error_query() {
        let err = DbError::Query("syntax error near SELECT".into());
        assert_eq!(err.to_string(), "query failed: syntax error near SELECT");
    }

    #[test]
    fn test_db_error_migration() {
        let err = DbError::Migration("column already exists".into());
        assert_eq!(
            err.to_string(),
            "schema migration failed: column already exists"
        );
    }

    #[test]
    fn test_db_error_transaction() {
        let err = DbError::Transaction("deadlock detected".into());
        assert_eq!(err.to_string(), "transaction failed: deadlock detected");
    }

    #[test]
    fn test_db_error_integrity_check() {
        let err = DbError::IntegrityCheck("foreign key violation".into());
        assert_eq!(
            err.to_string(),
            "integrity check failed: foreign key violation"
        );
    }

    // -- LedgerError --

    #[test]
    fn test_ledger_error_unknown_event_type() {
        let err = LedgerError::UnknownEventType("bad_event".into());
        assert_eq!(err.to_string(), "unknown event type: bad_event");
    }

    #[test]
    fn test_ledger_error_misrouted() {
        let err = LedgerError::MisroutedEvent {
            event_type: "session_start".into(),
            expected: "sessions.jsonl".into(),
            actual: "work-graph.jsonl".into(),
        };
        assert_eq!(
            err.to_string(),
            "misrouted event: session_start belongs to sessions.jsonl, not work-graph.jsonl"
        );
    }

    #[test]
    fn test_ledger_error_validation() {
        let err = LedgerError::Validation("missing required field".into());
        assert_eq!(err.to_string(), "validation failed: missing required field");
    }

    #[test]
    fn test_ledger_error_lock() {
        let err = LedgerError::Lock("file locked by another process".into());
        assert_eq!(
            err.to_string(),
            "lock acquisition failed: file locked by another process"
        );
    }

    // -- HookError --

    #[test]
    fn test_hook_error_parse() {
        let err = HookError::Parse("invalid JSON at line 5".into());
        assert_eq!(err.to_string(), "parse error: invalid JSON at line 5");
    }

    #[test]
    fn test_hook_error_blocked() {
        let err = HookError::Blocked {
            reason: "protected resource access denied".into(),
        };
        assert_eq!(err.to_string(), "blocked: protected resource access denied");
    }

    #[test]
    fn test_hook_error_sentinel_not_found() {
        let err = HookError::SentinelNotFound("pathflow-pf-3".into());
        assert_eq!(err.to_string(), "sentinel not found: pathflow-pf-3");
    }

    #[test]
    fn test_hook_error_config() {
        let err = HookError::Config("missing hook configuration".into());
        assert_eq!(
            err.to_string(),
            "configuration error: missing hook configuration"
        );
    }

    #[test]
    fn test_hook_error_claim_conflict() {
        let err = HookError::ClaimConflict("path 'src/main.rs' held by ses-other".into());
        assert_eq!(
            err.to_string(),
            "claim conflict: path 'src/main.rs' held by ses-other"
        );
    }

    #[test]
    fn test_hook_error_token_mismatch() {
        let err = HookError::TokenMismatch {
            expected: 5,
            found: 3,
        };
        assert_eq!(err.to_string(), "token mismatch: expected 5, found 3");
    }

    // -- SessionError --

    #[test]
    fn test_session_error_no_active_session() {
        let err = SessionError::NoActiveSession;
        assert_eq!(err.to_string(), "no active session");
    }

    #[test]
    fn test_session_error_already_ended() {
        let err = SessionError::AlreadyEnded("ses-123".into());
        assert_eq!(err.to_string(), "session already ended: ses-123");
    }

    #[test]
    fn test_session_error_invalid_session_id() {
        let err = SessionError::InvalidSessionId("bad-format".into());
        assert_eq!(err.to_string(), "invalid session id: bad-format");
    }

    #[test]
    fn test_session_error_from_db_error() {
        let db_err = DbError::NotFound {
            table: "sessions".into(),
            id: "ses-123".into(),
        };
        let session_err: SessionError = db_err.into();
        assert!(matches!(session_err, SessionError::Db(_)));
        assert!(session_err.to_string().contains("database error"));
    }

    #[test]
    fn test_session_error_from_db_error_preserves_inner() {
        let db_err = DbError::Connection("timeout".into());
        let session_err: SessionError = db_err.into();
        let msg = session_err.to_string();
        assert!(
            msg.contains("connection failed"),
            "inner DbError message should be preserved: {msg}",
        );
    }

    // -- ConfigError --

    #[test]
    fn test_config_error_not_found() {
        let err = ConfigError::NotFound("pathflow-config.json".into());
        assert_eq!(
            err.to_string(),
            "config file not found: pathflow-config.json"
        );
    }

    #[test]
    fn test_config_error_parse() {
        let err = ConfigError::Parse("unexpected token at line 10".into());
        assert_eq!(
            err.to_string(),
            "config parse error: unexpected token at line 10"
        );
    }

    #[test]
    fn test_config_error_validation() {
        let err = ConfigError::Validation("missing required field 'phases'".into());
        assert_eq!(
            err.to_string(),
            "config validation error: missing required field 'phases'"
        );
    }

    // -- WorkgraphError --

    #[test]
    fn test_workgraph_error_not_found() {
        let err = WorkgraphError::NotFound("epic:foo".into());
        assert_eq!(err.to_string(), "not found: epic:foo");
    }

    #[test]
    fn test_workgraph_error_invalid_transition() {
        let err = WorkgraphError::InvalidTransition {
            entity: "task".into(),
            from: "todo".into(),
            to: "complete".into(),
        };
        assert_eq!(
            err.to_string(),
            "invalid transition: task from todo to complete"
        );
    }

    #[test]
    fn test_workgraph_error_validation() {
        let err = WorkgraphError::Validation("title is required".into());
        assert_eq!(err.to_string(), "validation error: title is required");
    }

    #[test]
    fn test_workgraph_error_format_id_generation() {
        let err = WorkgraphError::FormatIdGeneration("bad sequence".into());
        assert_eq!(err.to_string(), "format ID generation error: bad sequence");
    }

    #[test]
    fn test_workgraph_error_from_db_error() {
        let db_err = DbError::Query("select failed".into());
        let wg_err: WorkgraphError = db_err.into();
        assert!(matches!(wg_err, WorkgraphError::Db(_)));
        assert!(wg_err.to_string().contains("database error"));
    }

    // -- WorktreeError --

    #[test]
    fn test_worktree_error_not_found() {
        let err = WorktreeError::NotFound("feat/my-branch".into());
        assert_eq!(err.to_string(), "worktree not found: feat/my-branch");
    }

    #[test]
    fn test_worktree_error_already_exists() {
        let err = WorktreeError::AlreadyExists("feat/my-branch".into());
        assert_eq!(err.to_string(), "worktree already exists: feat/my-branch");
    }

    #[test]
    fn test_worktree_error_creation() {
        let err = WorktreeError::Creation("git worktree add failed".into());
        assert_eq!(
            err.to_string(),
            "worktree creation failed: git worktree add failed"
        );
    }

    #[test]
    fn test_worktree_error_cleanup() {
        let err = WorktreeError::Cleanup("directory not empty".into());
        assert_eq!(
            err.to_string(),
            "worktree cleanup failed: directory not empty"
        );
    }

    #[test]
    fn test_worktree_error_pathflow_active() {
        let err = WorktreeError::PathFlowActive;
        assert_eq!(
            err.to_string(),
            "cleanup blocked: PathFlow session is active, use force to override"
        );
    }

    #[test]
    fn test_worktree_error_invalid_name() {
        let err = WorktreeError::InvalidName("bad/name!".into());
        assert_eq!(err.to_string(), "invalid worktree name: bad/name!");
    }

    // -- DoctorError --

    #[test]
    fn test_doctor_error_check_not_found() {
        let err = DoctorError::CheckNotFound("rust-toolchain".into());
        assert_eq!(err.to_string(), "check not found: rust-toolchain");
    }

    #[test]
    fn test_doctor_error_check_failed() {
        let err = DoctorError::CheckFailed {
            name: "cargo-nextest".into(),
            reason: "not installed".into(),
        };
        assert_eq!(
            err.to_string(),
            "check failed: cargo-nextest: not installed"
        );
    }

    #[test]
    fn test_doctor_error_repair_failed() {
        let err = DoctorError::RepairFailed {
            name: "cargo-nextest".into(),
            reason: "install script failed".into(),
        };
        assert_eq!(
            err.to_string(),
            "repair failed: cargo-nextest: install script failed"
        );
    }

    // -- ValidateError --

    #[test]
    fn test_validate_error_invalid_frontmatter() {
        let err = ValidateError::InvalidFrontmatter("missing id field".into());
        assert_eq!(err.to_string(), "invalid frontmatter: missing id field");
    }

    #[test]
    fn test_validate_error_field_error() {
        let err = ValidateError::FieldError {
            field: "status".into(),
            message: "must be one of: todo, in_progress, complete".into(),
        };
        assert_eq!(
            err.to_string(),
            "validation failed: status: must be one of: todo, in_progress, complete"
        );
    }

    // -- IdgenError --

    #[test]
    fn test_idgen_error_generation() {
        let err = IdgenError::Generation("entropy source unavailable".into());
        assert_eq!(
            err.to_string(),
            "ULID generation failed: entropy source unavailable"
        );
    }

    // -- PathflowError --

    #[test]
    fn test_pathflow_error_phase_not_initialized() {
        let err = PathflowError::PhaseNotInitialized("PF4-EXECUTE".into());
        assert_eq!(err.to_string(), "phase not initialized: PF4-EXECUTE");
    }

    #[test]
    fn test_pathflow_error_invalid_task_id() {
        let err = PathflowError::InvalidTaskId("not-a-pf-task".into());
        assert_eq!(err.to_string(), "invalid task ID format: not-a-pf-task");
    }

    #[test]
    fn test_pathflow_error_cross_phase_block() {
        let err = PathflowError::CrossPhaseBlock("PF3 task registered before PF2 complete".into());
        assert_eq!(
            err.to_string(),
            "cross-phase registration blocked: PF3 task registered before PF2 complete"
        );
    }

    #[test]
    fn test_pathflow_error_sentinel() {
        let err = PathflowError::Sentinel("pathflow-pf-3 not found".into());
        assert_eq!(err.to_string(), "sentinel error: pathflow-pf-3 not found");
    }

    #[test]
    fn test_pathflow_error_invalid_transition() {
        let err = PathflowError::InvalidTransition("PF2 cannot follow PF4".into());
        assert_eq!(
            err.to_string(),
            "invalid phase transition: PF2 cannot follow PF4"
        );
    }

    // -- SettingsError --

    #[test]
    fn test_settings_error_validation() {
        let err = SettingsError::Validation("hook command not found".into());
        assert_eq!(
            err.to_string(),
            "settings validation failed: hook command not found"
        );
    }

    #[test]
    fn test_settings_error_template_discovery() {
        let err = SettingsError::TemplateDiscovery("glob pattern failed".into());
        assert_eq!(
            err.to_string(),
            "template discovery failed: glob pattern failed"
        );
    }

    // -- AutorunError --

    #[test]
    fn test_autorun_error_display() {
        let err = AutorunError::InvalidBatch("no tasks defined".into());
        assert_eq!(err.to_string(), "invalid batch file: no tasks defined");
    }

    #[test]
    fn test_autorun_error_dependency_cycle() {
        let err = AutorunError::DependencyCycle("tasks involved: a, b".into());
        assert_eq!(
            err.to_string(),
            "dependency cycle detected: tasks involved: a, b"
        );
    }

    #[test]
    fn test_autorun_error_protected_merge() {
        let err = AutorunError::ProtectedMerge("cannot auto_merge into main".into());
        assert!(err.to_string().contains("protected merge"));
    }

    #[test]
    fn test_autorun_error_missing_task() {
        let err = AutorunError::MissingTask("INF-TSK-022-019".into());
        assert_eq!(err.to_string(), "missing task: INF-TSK-022-019");
    }

    #[test]
    fn test_autorun_error_worker_timeout() {
        let err = AutorunError::WorkerTimeout("worker-3 timed out after 60m".into());
        assert_eq!(
            err.to_string(),
            "worker timeout: worker-3 timed out after 60m"
        );
    }

    #[test]
    fn test_autorun_error_worker_failed() {
        let err = AutorunError::WorkerFailed("exit code 1".into());
        assert_eq!(err.to_string(), "worker failed: exit code 1");
    }

    #[test]
    fn test_autorun_error_session_not_found() {
        let err = AutorunError::SessionNotFound("autorun-ses-abc".into());
        assert_eq!(err.to_string(), "session not found: autorun-ses-abc");
    }

    #[test]
    fn test_autorun_error_yaml_parse() {
        let err = AutorunError::Yaml("unexpected key at line 5".into());
        assert_eq!(
            err.to_string(),
            "yaml parse error: unexpected key at line 5"
        );
    }

    // -- CoordinationError --

    #[test]
    fn test_coordination_error_claim_conflict() {
        let err = CoordinationError::ClaimConflict {
            path: "src/main.rs".into(),
            owner: crate::types::SessionId::new_unchecked("ses-123"),
        };
        assert_eq!(
            err.to_string(),
            "claim conflict: path 'src/main.rs' already owned by ses-123"
        );
    }

    #[test]
    fn test_coordination_error_token_mismatch() {
        let err = CoordinationError::TokenMismatch {
            expected: 5,
            found: 3,
        };
        assert_eq!(
            err.to_string(),
            "fencing token mismatch: expected 5, found 3"
        );
    }

    #[test]
    fn test_coordination_error_container_not_found() {
        let err = CoordinationError::ContainerNotFound("claims".into());
        assert_eq!(err.to_string(), "container not found: claims");
    }

    // -- SyncError --

    #[test]
    fn test_sync_error_pid_lock_failed() {
        let err = SyncError::PidLockFailed(std::path::PathBuf::from("/tmp/daemon.pid"));
        assert_eq!(err.to_string(), "pid lock failed: /tmp/daemon.pid");
    }

    #[test]
    fn test_sync_error_network_partition() {
        let err = SyncError::NetworkPartition {
            peer: crate::coordination::PeerId::new("alice-laptop"),
            retries: 5,
        };
        assert_eq!(
            err.to_string(),
            "network partition: peer alice-laptop, retries exhausted (5)"
        );
    }

    #[test]
    fn test_sync_error_connection_error() {
        let err = SyncError::ConnectionError("timeout after 30s".into());
        assert_eq!(err.to_string(), "connection error: timeout after 30s");
    }

    #[test]
    fn test_sync_error_schema_error() {
        let err = SyncError::SchemaError("incompatible version".into());
        assert_eq!(err.to_string(), "schema error: incompatible version");
    }

    #[test]
    fn test_sync_error_is_retryable() {
        assert!(SyncError::ConnectionError("x".into()).is_retryable());
        assert!(!SyncError::SchemaError("x".into()).is_retryable());
        assert!(!SyncError::PidLockFailed(std::path::PathBuf::from("x")).is_retryable());
    }

    #[test]
    fn test_sync_error_is_schema_error() {
        assert!(SyncError::SchemaError("x".into()).is_schema_error());
        assert!(!SyncError::ConnectionError("x".into()).is_schema_error());
    }
}
