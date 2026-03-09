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
    Git(String),
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
