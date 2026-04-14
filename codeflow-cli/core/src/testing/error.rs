//! Error types for the generic testing engine.

use std::path::PathBuf;

use thiserror::Error;

/// Errors produced by the testing engine.
#[derive(Debug, Error)]
pub enum TestingError {
    /// Config file not found at the expected path.
    #[error("test config not found: {0}")]
    ConfigNotFound(PathBuf),

    /// Config file failed JSON schema validation.
    #[error("invalid test config at {path}: {message}")]
    ConfigInvalid { path: PathBuf, message: String },

    /// Config schema version is not supported.
    #[error("unsupported schema_version={version}; this engine supports: [{supported}]")]
    UnsupportedSchemaVersion { version: String, supported: String },

    /// Config is missing the required `schema_version` field.
    #[error("missing required field schema_version")]
    MissingSchemaVersion,

    /// A target referenced by name was not found in the config.
    #[error("target not found: {0}")]
    TargetNotFound(String),

    /// Duplicate target name in config.
    #[error("duplicate target name: {0}")]
    DuplicateTarget(String),

    /// JUnit XML parse failure with file path and byte offset.
    #[error("JUnit XML parse error in {path} at byte {offset}: {message}")]
    JunitParseError {
        path: PathBuf,
        offset: usize,
        message: String,
    },

    /// CTRF JSON parse failure.
    #[error("CTRF parse error in {path}: {message}")]
    CtrfParseError { path: PathBuf, message: String },

    /// Coverage file parse failure.
    #[error("coverage parse error in {path}: {message}")]
    CoverageParseError { path: PathBuf, message: String },

    /// Coverage artifact was expected but not found after command execution.
    #[error("coverage artifact not found: {0}")]
    CoverageArtifactMissing(PathBuf),

    /// A target's command failed (non-zero exit).
    #[error("target {target} command failed with exit code {exit_code}")]
    CommandFailed { target: String, exit_code: i32 },

    /// A target's command could not be spawned.
    #[error("failed to spawn command for target {target}: {message}")]
    CommandSpawnError { target: String, message: String },

    /// Coverage threshold violation.
    #[error("coverage threshold not met: {0}")]
    ThresholdViolation(String),

    /// Report file not found.
    #[error("report file not found: {0}")]
    ReportNotFound(PathBuf),

    /// I/O error wrapper.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The legacy engine has been removed. Use the new generic engine.
    #[error("legacy test engine removed; use `codeflow test` with test-config.json")]
    LegacyEngineRemoved,

    /// Coverage transform command failed.
    #[error("coverage transform failed for target {target} with exit code {exit_code}")]
    CoverageTransformFailed { target: String, exit_code: i32 },

    /// Unknown mode requested for a target that does not define it.
    #[error("target {target} does not define mode {mode}")]
    ModeNotConfigured { target: String, mode: String },

    /// Parallel execution thread panicked.
    #[error("parallel execution panicked for target {target}: {message}")]
    ParallelExecutionError { target: String, message: String },

    /// Target cwd escapes the project root (path traversal).
    #[error("target cwd escapes project root: {0}")]
    CwdEscapesRoot(String),

    /// Invalid environment variable key.
    #[error("env key '{key}' is invalid: {reason}")]
    InvalidEnvKey { key: String, reason: String },

    /// Unbalanced quotes detected in a command string.
    #[error("unbalanced quotes in command: {0}")]
    UnbalancedQuotes(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_not_found_display() {
        let err = TestingError::ConfigNotFound(PathBuf::from("/tmp/test-config.json"));
        assert_eq!(
            err.to_string(),
            "test config not found: /tmp/test-config.json"
        );
    }

    #[test]
    fn test_config_invalid_display() {
        let err = TestingError::ConfigInvalid {
            path: PathBuf::from("/tmp/test-config.json"),
            message:
                "targets[1].coverage.rules[0].scope: expected one of [per_file, ...], got \"line\""
                    .to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid test config"));
        assert!(msg.contains("targets[1].coverage.rules[0].scope"));
    }

    #[test]
    fn test_unsupported_schema_version_display() {
        let err = TestingError::UnsupportedSchemaVersion {
            version: "2.0".to_string(),
            supported: "1.0".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "unsupported schema_version=2.0; this engine supports: [1.0]"
        );
    }

    #[test]
    fn test_missing_schema_version_display() {
        let err = TestingError::MissingSchemaVersion;
        assert_eq!(err.to_string(), "missing required field schema_version");
    }

    #[test]
    fn test_junit_parse_error_display() {
        let err = TestingError::JunitParseError {
            path: PathBuf::from("report.xml"),
            offset: 42,
            message: "unexpected end of input".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("report.xml"));
        assert!(msg.contains("byte 42"));
        assert!(msg.contains("unexpected end of input"));
    }

    #[test]
    fn test_command_failed_display() {
        let err = TestingError::CommandFailed {
            target: "rust-core".to_string(),
            exit_code: 1,
        };
        assert_eq!(
            err.to_string(),
            "target rust-core command failed with exit code 1"
        );
    }

    #[test]
    fn test_legacy_engine_removed_display() {
        let err = TestingError::LegacyEngineRemoved;
        assert!(err.to_string().contains("legacy test engine removed"));
    }

    #[test]
    fn test_coverage_transform_failed_display() {
        let err = TestingError::CoverageTransformFailed {
            target: "node-api".to_string(),
            exit_code: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("node-api"));
        assert!(msg.contains("exit code 1"));
    }

    #[test]
    fn test_mode_not_configured_display() {
        let err = TestingError::ModeNotConfigured {
            target: "web".to_string(),
            mode: "full".to_string(),
        };
        assert_eq!(err.to_string(), "target web does not define mode full");
    }

    #[test]
    fn test_unbalanced_quotes_display() {
        let err = TestingError::UnbalancedQuotes("echo 'hello".to_string());
        assert!(err.to_string().contains("unbalanced quotes"));
    }

    #[test]
    fn test_io_error_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = TestingError::from(io_err);
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn test_json_error_from() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err = TestingError::from(json_err);
        assert!(err.to_string().contains("json error"));
    }

    #[test]
    fn test_target_not_found_display() {
        let err = TestingError::TargetNotFound("missing-target".to_string());
        assert_eq!(err.to_string(), "target not found: missing-target");
    }

    #[test]
    fn test_duplicate_target_display() {
        let err = TestingError::DuplicateTarget("dupe".to_string());
        assert_eq!(err.to_string(), "duplicate target name: dupe");
    }

    #[test]
    fn test_parallel_execution_error_display() {
        let err = TestingError::ParallelExecutionError {
            target: "web".to_string(),
            message: "thread panicked".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("web"));
        assert!(msg.contains("thread panicked"));
    }

    #[test]
    fn test_cwd_escapes_root_display() {
        let err = TestingError::CwdEscapesRoot("../etc".to_string());
        assert_eq!(err.to_string(), "target cwd escapes project root: ../etc");
    }

    #[test]
    fn test_invalid_env_key_display() {
        let err = TestingError::InvalidEnvKey {
            key: "FOO=BAR".to_string(),
            reason: "must not contain '='".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("FOO=BAR"));
        assert!(msg.contains("must not contain '='"));
    }
}
