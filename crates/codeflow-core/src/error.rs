//! Error types for codeflow-core modules.
//!
//! Imported from v1 and pruned per charter D22: `PathFlow`, coordination,
//! sync, session, worktree, and autorun-worker error types are gone with
//! their owning subsystems. Each surviving enum is owned by a surviving
//! module (ledger, workgraph, validate, doctor, settings, git, security).

use thiserror::Error;

/// Ledger (append-only JSONL event log) errors.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("unknown ledger type: {0}")]
    UnknownType(String),

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

/// Hook (git-guard / session hooks) errors.
#[derive(Debug, Error)]
pub enum HookError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("blocked: {reason}")]
    Blocked { reason: String },

    #[error("configuration error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
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

/// Infrastructure diagnostics errors.
#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("check not found: {0}")]
    CheckNotFound(String),

    #[error("check failed: {name}: {reason}")]
    CheckFailed { name: String, reason: String },

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

/// Settings merge and validation errors.
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings validation failed: {0}")]
    Validation(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(#[from] ConfigError),
}

/// Git operation errors (merge conflict detection, branch analysis).
#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to open repository: {0}")]
    RepoOpen(String),

    #[error("ref not found: {0}")]
    RefNotFound(String),

    #[error("no common ancestor between HEAD and {0}")]
    NoCommonAncestor(String),

    #[error("merge analysis failed: {0}")]
    MergeFailed(String),

    #[error("git2 error: {0}")]
    Git2(#[from] git2::Error),
}

/// Batch parsing errors (deferred driver; worker variants pruned with the
/// v1 autorun worker/orchestrator).
#[derive(Debug, Error)]
pub enum BatchError {
    #[error("invalid batch file: {0}")]
    InvalidBatch(String),

    #[error("dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("missing task: {0}")]
    MissingTask(String),

    #[error("protected merge: {0}")]
    ProtectedMerge(String),

    #[error("yaml parse error: {0}")]
    Yaml(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- LedgerError --

    #[test]
    fn test_ledger_error_unknown_event_type() {
        let err = LedgerError::UnknownEventType("bad_event".into());
        assert_eq!(err.to_string(), "unknown event type: bad_event");
    }

    #[test]
    fn test_ledger_error_unknown_type() {
        let err = LedgerError::UnknownType("bogus".into());
        assert_eq!(err.to_string(), "unknown ledger type: bogus");
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
            reason: "protected branch push denied".into(),
        };
        assert_eq!(err.to_string(), "blocked: protected branch push denied");
    }

    #[test]
    fn test_hook_error_config() {
        let err = HookError::Config("missing hook configuration".into());
        assert_eq!(
            err.to_string(),
            "configuration error: missing hook configuration"
        );
    }

    // -- ConfigError --

    #[test]
    fn test_config_error_not_found() {
        let err = ConfigError::NotFound("policy.json".into());
        assert_eq!(err.to_string(), "config file not found: policy.json");
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
        let err = ConfigError::Validation("missing required field 'git'".into());
        assert_eq!(
            err.to_string(),
            "config validation error: missing required field 'git'"
        );
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
            name: "hooks".into(),
            reason: "binary not in PATH".into(),
        };
        assert_eq!(err.to_string(), "check failed: hooks: binary not in PATH");
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

    // -- SettingsError --

    #[test]
    fn test_settings_error_validation() {
        let err = SettingsError::Validation("settings.json is not an object".into());
        assert_eq!(
            err.to_string(),
            "settings validation failed: settings.json is not an object"
        );
    }

    #[test]
    fn test_settings_error_from_config_error() {
        let cfg_err = ConfigError::Parse("bad json".into());
        let err: SettingsError = cfg_err.into();
        assert!(matches!(err, SettingsError::Config(_)));
        assert!(err.to_string().contains("bad json"));
    }

    // -- GitError --

    #[test]
    fn test_git_error_repo_open() {
        let err = GitError::RepoOpen("not a git repository".into());
        assert_eq!(
            err.to_string(),
            "failed to open repository: not a git repository"
        );
    }

    #[test]
    fn test_git_error_ref_not_found() {
        let err = GitError::RefNotFound("refs/remotes/origin/main".into());
        assert_eq!(err.to_string(), "ref not found: refs/remotes/origin/main");
    }

    #[test]
    fn test_git_error_no_common_ancestor() {
        let err = GitError::NoCommonAncestor("main".into());
        assert_eq!(err.to_string(), "no common ancestor between HEAD and main");
    }

    #[test]
    fn test_git_error_merge_failed() {
        let err = GitError::MergeFailed("index conflict".into());
        assert_eq!(err.to_string(), "merge analysis failed: index conflict");
    }

    #[test]
    fn test_git_error_from_git2() {
        let git2_err = git2::Error::from_str("test git2 error");
        let err: GitError = git2_err.into();
        assert!(matches!(err, GitError::Git2(_)));
        assert!(err.to_string().contains("test git2 error"));
    }

    // -- BatchError --

    #[test]
    fn test_batch_error_invalid_batch() {
        let err = BatchError::InvalidBatch("no tasks defined".into());
        assert_eq!(err.to_string(), "invalid batch file: no tasks defined");
    }

    #[test]
    fn test_batch_error_dependency_cycle() {
        let err = BatchError::DependencyCycle("tasks involved: a, b".into());
        assert_eq!(
            err.to_string(),
            "dependency cycle detected: tasks involved: a, b"
        );
    }

    #[test]
    fn test_batch_error_protected_merge() {
        let err = BatchError::ProtectedMerge("cannot auto_merge into main".into());
        assert!(err.to_string().contains("protected merge"));
    }

    #[test]
    fn test_batch_error_missing_task() {
        let err = BatchError::MissingTask("TSK-001-001".into());
        assert_eq!(err.to_string(), "missing task: TSK-001-001");
    }

    #[test]
    fn test_batch_error_yaml_parse() {
        let err = BatchError::Yaml("unexpected key at line 5".into());
        assert_eq!(
            err.to_string(),
            "yaml parse error: unexpected key at line 5"
        );
    }
}
