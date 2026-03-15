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
//! - Current session resolution (env var > env file)

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

/// Resolve the current session ID.
///
/// Resolution priority:
/// 1. `CODEFLOW_SESSION_ID` environment variable
/// 2. `codeflow-env.sh` file at `{project_dir}/.state/runtime/codeflow-env.sh`
///
/// Takes `project_dir` (the repository root) and internally constructs the
/// canonical path to the env file. This eliminates ambiguity -- callers no
/// longer need to know the internal `.state/runtime` layout.
///
/// # Errors
///
/// Returns `SessionError::NoActiveSession` if no session ID can be found.
/// Returns `SessionError::Io` or `SessionError::InvalidSessionId` on file read
/// or parse errors.
pub fn current_session_id(project_dir: &Path) -> Result<SessionId, SessionError> {
    let env_value = std::env::var("CODEFLOW_SESSION_ID").ok();
    let runtime_dir = project_dir.join(".state").join("runtime");
    resolve_session_id(env_value.as_deref(), &runtime_dir)
}

/// Internal resolver: resolves session ID from env var value or env file.
///
/// Separated from `current_session_id` for testability without modifying
/// process environment variables. Takes the `runtime_dir` (the directory
/// containing `codeflow-env.sh`).
fn resolve_session_id(
    env_var_value: Option<&str>,
    runtime_dir: &Path,
) -> Result<SessionId, SessionError> {
    // Priority 1: environment variable
    if let Some(value) = env_var_value {
        if !value.is_empty() {
            return SessionId::new(value)
                .map_err(|e| SessionError::InvalidSessionId(e.to_string()));
        }
    }

    // Priority 2: env file
    if let Some(env_file) = read_env_file(runtime_dir)? {
        return Ok(env_file.session_id);
    }

    Err(SessionError::NoActiveSession)
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
    fn test_resolve_session_id_from_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_session_id(Some("ses-envvar123"), dir.path()).unwrap();
        assert_eq!(result.as_str(), "ses-envvar123");
    }

    #[test]
    fn test_resolve_session_id_env_var_takes_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let file_sid = SessionId::new_unchecked("ses-fromfile");
        write_env_file(dir.path(), &file_sid, "codeflow").unwrap();

        // Env var should win over file
        let result = resolve_session_id(Some("ses-fromenv"), dir.path()).unwrap();
        assert_eq!(result.as_str(), "ses-fromenv");
    }

    #[test]
    fn test_resolve_session_id_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7envfiletest12345678");
        write_env_file(dir.path(), &sid, "codeflow").unwrap();

        // No env var — should fall back to file
        let result = resolve_session_id(None, dir.path()).unwrap();
        assert_eq!(result, sid);
    }

    #[test]
    fn test_resolve_session_id_empty_env_var_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-fallthrough");
        write_env_file(dir.path(), &sid, "codeflow").unwrap();

        // Empty env var should be treated as absent
        let result = resolve_session_id(Some(""), dir.path()).unwrap();
        assert_eq!(result, sid);
    }

    #[test]
    fn test_resolve_session_id_no_source() {
        let dir = tempfile::tempdir().unwrap();

        let result = resolve_session_id(None, dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no active session")
        );
    }

    #[test]
    fn test_is_valid_session_id_with_crockford_excluded_chars() {
        // Crockford Base32 excludes i, l, o, u — these should fail validation
        // Construct a 26-char string with an excluded char
        assert!(!is_valid_session_id("ses-01jq7abcdef0123456789il0a"));
    }
}
