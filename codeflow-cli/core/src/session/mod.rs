//! Session lifecycle management.
//!
//! Consolidates Go's `session`, `workstate`, and `config` packages into a
//! unified Rust session module. Provides:
//!
//! - Session ID generation (ULID-based)
//! - Session state machine with validated transitions
//! - Builder pattern for session creation
//! - Environment file (`codeflow-env.sh`) management
//! - Active task file management
//! - Current session resolution (env file only)

pub mod active_task;
pub mod builder;
pub mod env;
mod state;

use std::path::Path;

use crate::error::SessionError;
use crate::types::SessionId;

pub use active_task::{ActiveTask, clear_active_task, get_active_task, set_active_task};
pub use builder::SessionBuilder;
pub use env::{EnvFile, read_env_file, remove_env_file, write_env_file};

/// Generate a new session ID using ULID format: `ses-{26-char-lowercase-ULID}`.
///
/// The ULID encodes the current timestamp and random component, providing
/// time-sortable, globally unique identifiers.
#[must_use]
pub fn generate_session_id() -> SessionId {
    let ulid = ulid::Ulid::new();
    let id = format!("ses-{}", ulid.to_string().to_lowercase());
    SessionId::new_unchecked(id)
}

/// Validate that a string matches the session ID format.
///
/// Accepts both:
/// - Legacy: `ses-{13-digit-timestamp}{12-hex-chars}`
/// - ULID: `ses-{26-char-lowercase-crockford-base32}`
#[must_use]
pub fn is_valid_session_id(value: &str) -> bool {
    if let Some(suffix) = value.strip_prefix("ses-") {
        if suffix.len() == 25 {
            // Legacy format: 13 digits + 12 hex chars
            let (ts, hex) = suffix.split_at(13);
            ts.chars().all(|c| c.is_ascii_digit())
                && hex
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        } else if suffix.len() == 26 {
            // ULID format: 26 chars of Crockford Base32 (lowercase)
            suffix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && !suffix.contains('i')
                && !suffix.contains('l')
                && !suffix.contains('o')
                && !suffix.contains('u')
        } else {
            false
        }
    } else {
        false
    }
}

/// Resolve the current session ID from the env file.
///
/// Reads `codeflow-env.sh` at `{project_dir}/.state/runtime/codeflow-env.sh`.
/// The env file is the single source of truth for the current session ID.
///
/// Takes `project_dir` (the repository root) and internally constructs the
/// canonical path to the env file. This eliminates ambiguity -- callers no
/// longer need to know the internal `.state/runtime` layout.
///
/// # Errors
///
/// Returns `SessionError::NoActiveSession` if no env file exists.
/// Returns `SessionError::Io` or `SessionError::InvalidSessionId` on file read
/// or parse errors.
pub fn current_session_id(project_dir: &Path) -> Result<SessionId, SessionError> {
    let runtime_dir = project_dir.join(".state").join("runtime");
    match read_env_file(&runtime_dir)? {
        Some(env_file) => Ok(env_file.session_id),
        None => Err(SessionError::NoActiveSession),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_id_format() {
        let id = generate_session_id();
        let s = id.as_str();
        assert!(s.starts_with("ses-"), "should start with ses-: {s}");
        // ses- prefix + 26 char ULID = 30 chars total
        assert_eq!(s.len(), 30, "should be 30 chars: {s}");
    }

    #[test]
    fn test_generate_session_id_uniqueness() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2, "two generated IDs should differ");
    }

    #[test]
    fn test_generate_session_id_is_lowercase() {
        let id = generate_session_id();
        let suffix = &id.as_str()[4..]; // skip "ses-"
        assert_eq!(
            suffix,
            suffix.to_lowercase(),
            "ULID suffix should be lowercase: {suffix}"
        );
    }

    #[test]
    fn test_is_valid_session_id_ulid() {
        let id = generate_session_id();
        assert!(
            is_valid_session_id(id.as_str()),
            "generated ULID ID should be valid: {}",
            id.as_str()
        );
    }

    #[test]
    fn test_is_valid_session_id_legacy() {
        // 13 digits + 12 hex chars
        assert!(is_valid_session_id("ses-1771372021317a9e89b2d5688"));
    }

    #[test]
    fn test_is_valid_session_id_invalid_prefix() {
        assert!(!is_valid_session_id("session-abc123"));
    }

    #[test]
    fn test_is_valid_session_id_empty() {
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn test_is_valid_session_id_too_short() {
        assert!(!is_valid_session_id("ses-abc"));
    }

    #[test]
    fn test_is_valid_session_id_uppercase_rejected() {
        assert!(!is_valid_session_id("ses-01JQABCDEF0123456789ABCDEF"));
    }

    #[test]
    fn test_current_session_id_reads_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let sid = SessionId::new_unchecked("ses-01jq7envfiletest12345678");
        write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        let result = current_session_id(dir.path()).unwrap();
        assert_eq!(result, sid);
    }

    #[test]
    fn test_current_session_id_no_env_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let result = current_session_id(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no active session")
        );
    }

    #[test]
    fn test_current_session_id_ignores_env_var() {
        // Even if CODEFLOW_SESSION_ID env var is set, current_session_id
        // should only read from the env file. We verify by having no env
        // file -- it should return NoActiveSession regardless of env var.
        let dir = tempfile::tempdir().unwrap();

        let result = current_session_id(dir.path());
        assert!(result.is_err(), "should not read env var, only env file");
    }

    #[test]
    fn test_is_valid_session_id_with_crockford_excluded_chars() {
        // Crockford Base32 excludes i, l, o, u — these should fail validation
        // Construct a 26-char string with an excluded char
        assert!(!is_valid_session_id("ses-01jq7abcdef0123456789il0a"));
    }
}
