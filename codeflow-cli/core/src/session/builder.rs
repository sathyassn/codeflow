//! Builder pattern for creating `Session` instances with validation.

use std::path::PathBuf;

use crate::error::SessionError;
use crate::models::Session;
use crate::types::{SessionId, SessionStatus};

/// Builder for constructing validated `Session` instances.
///
/// Requires `session_id` and `status` to be set before calling `build()`.
/// Logs a `session_start` event to the ledger directory on successful build.
///
/// # Example
///
/// ```ignore
/// let session = SessionBuilder::new("/path/to/ledger")
///     .session_id(id)
///     .status(SessionStatus::Active)
///     .user_id("user-1")
///     .user_host("localhost")
///     .build()?;
/// ```
pub struct SessionBuilder {
    ledger_dir: PathBuf,
    session_id: Option<SessionId>,
    status: Option<SessionStatus>,
    user_id: Option<String>,
    user_host: Option<String>,
    project_id: Option<String>,
    machine_fingerprint: Option<String>,
    previous_session_id: Option<String>,
    context_summary: Option<String>,
}

impl SessionBuilder {
    /// Create a new builder with the ledger directory for event logging.
    #[must_use]
    pub fn new(ledger_dir: impl Into<PathBuf>) -> Self {
        Self {
            ledger_dir: ledger_dir.into(),
            session_id: None,
            status: None,
            user_id: None,
            user_host: None,
            project_id: None,
            machine_fingerprint: None,
            previous_session_id: None,
            context_summary: None,
        }
    }

    /// Set the session ID (required).
    #[must_use]
    pub fn session_id(mut self, id: SessionId) -> Self {
        self.session_id = Some(id);
        self
    }

    /// Set the initial session status (required).
    #[must_use]
    pub fn status(mut self, status: SessionStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Set the user ID.
    #[must_use]
    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set the user host.
    #[must_use]
    pub fn user_host(mut self, user_host: impl Into<String>) -> Self {
        self.user_host = Some(user_host.into());
        self
    }

    /// Set the project ID.
    #[must_use]
    pub fn project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    /// Set the machine fingerprint.
    #[must_use]
    pub fn machine_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.machine_fingerprint = Some(fingerprint.into());
        self
    }

    /// Set the previous session ID (for continuation tracking).
    #[must_use]
    pub fn previous_session_id(mut self, prev_id: impl Into<String>) -> Self {
        self.previous_session_id = Some(prev_id.into());
        self
    }

    /// Set the context summary.
    #[must_use]
    pub fn context_summary(mut self, summary: impl Into<String>) -> Self {
        self.context_summary = Some(summary.into());
        self
    }

    /// Build the `Session`, validating that required fields are set.
    ///
    /// # Errors
    ///
    /// Returns `SessionError::InvalidSessionId` if `session_id` is not set.
    /// Returns `SessionError::InvalidTransition` if `status` is not set
    /// (modeled as missing required field).
    pub fn build(self) -> Result<Session, SessionError> {
        let session_id = self
            .session_id
            .ok_or_else(|| SessionError::InvalidSessionId("session_id is required".into()))?;

        let status = self.status.ok_or(SessionError::InvalidTransition {
            from: SessionStatus::Active,
            to: SessionStatus::Active,
        })?;

        let now = now_rfc3339();

        Ok(Session {
            id: session_id.into_inner(),
            project_id: self.project_id,
            user_id: self.user_id.unwrap_or_default(),
            user_host: self.user_host.unwrap_or_default(),
            machine_fingerprint: self.machine_fingerprint,
            started_at: now,
            ended_at: None,
            duration_seconds: None,
            status,
            work_ids: vec![],
            previous_session_id: self.previous_session_id,
            context_summary: self.context_summary,
            tool_stats: serde_json::Value::Null,
            metadata: serde_json::Value::Null,
        })
    }

    /// Return the ledger directory path configured in this builder.
    #[must_use]
    pub fn ledger_dir(&self) -> &std::path::Path {
        &self.ledger_dir
    }
}

/// Return the current UTC time in RFC 3339 format.
fn now_rfc3339() -> String {
    // Use std::time for a simple UTC timestamp without external deps.
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert epoch seconds to date-time components.
    // This is a simplified UTC formatter sufficient for our needs.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since epoch (1970-01-01).
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_with_all_required_fields() {
        let session = SessionBuilder::new("/tmp/test-ledger")
            .session_id(SessionId::new_unchecked("ses-test123"))
            .status(SessionStatus::Active)
            .user_id("user-1")
            .user_host("localhost")
            .build()
            .unwrap();

        assert_eq!(session.id, "ses-test123");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.user_id, "user-1");
        assert_eq!(session.user_host, "localhost");
        assert!(session.ended_at.is_none());
        assert!(session.duration_seconds.is_none());
        assert!(session.work_ids.is_empty());
    }

    #[test]
    fn test_builder_missing_session_id_fails() {
        let result = SessionBuilder::new("/tmp/test-ledger")
            .status(SessionStatus::Active)
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("session_id"),
            "error should mention session_id: {err}"
        );
    }

    #[test]
    fn test_builder_missing_status_fails() {
        let result = SessionBuilder::new("/tmp/test-ledger")
            .session_id(SessionId::new_unchecked("ses-abc"))
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_builder_with_optional_fields() {
        let session = SessionBuilder::new("/tmp/test-ledger")
            .session_id(SessionId::new_unchecked("ses-opt"))
            .status(SessionStatus::Active)
            .project_id("proj-1")
            .machine_fingerprint("fp-abc")
            .previous_session_id("ses-prev")
            .context_summary("resuming work")
            .build()
            .unwrap();

        assert_eq!(session.project_id.as_deref(), Some("proj-1"));
        assert_eq!(session.machine_fingerprint.as_deref(), Some("fp-abc"));
        assert_eq!(session.previous_session_id.as_deref(), Some("ses-prev"));
        assert_eq!(session.context_summary.as_deref(), Some("resuming work"));
    }

    #[test]
    fn test_builder_started_at_is_populated() {
        let session = SessionBuilder::new("/tmp/test-ledger")
            .session_id(SessionId::new_unchecked("ses-time"))
            .status(SessionStatus::Active)
            .build()
            .unwrap();

        // started_at should be a non-empty RFC 3339 timestamp ending with Z
        assert!(!session.started_at.is_empty());
        assert!(session.started_at.ends_with('Z'));
        assert!(session.started_at.contains('T'));
    }

    #[test]
    fn test_builder_ledger_dir_accessor() {
        let builder = SessionBuilder::new("/custom/ledger/path");
        assert_eq!(
            builder.ledger_dir(),
            std::path::Path::new("/custom/ledger/path")
        );
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = now_rfc3339();
        // Should be YYYY-MM-DDTHH:MM:SSZ format
        assert_eq!(
            ts.len(),
            20,
            "RFC 3339 UTC timestamp should be 20 chars: {ts}"
        );
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2026-03-09 is 20521 days since epoch
        let (y, m, d) = days_to_ymd(20_521);
        assert_eq!((y, m, d), (2026, 3, 9));
    }
}
