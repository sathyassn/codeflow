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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            session_err.to_string().contains("connection failed"),
            "inner DbError message should be preserved: {}",
            session_err
        );
    }
}
