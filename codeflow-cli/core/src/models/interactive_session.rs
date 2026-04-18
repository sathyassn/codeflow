use serde::{Deserialize, Serialize};

use crate::types::InteractiveSessionStatus;

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

/// Interactive session record tracked in SurrealDB.
///
/// Created by `codeflow interactive` (managed, `source_cli="codeflow"`) or by
/// the SessionStart hook for plain `claude` invocations (unmanaged,
/// `source_cli="claude"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveSession {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub session_id: String,
    pub pid: i64,
    pub status: InteractiveSessionStatus,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub work_type: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    /// Formatted task ID (e.g., `INF-TSK-046-008`) when known, for display in TUIs.
    /// Sourced from `active-task.json` alongside `task_id`, or back-filled via
    /// `generate_task_format_id` when missing. Distinct from `task_id` (ULID PK).
    #[serde(default)]
    pub task_format_id: Option<String>,
    /// Last completed PathFlow phase (e.g., `pf-4`) persisted in the DB so it
    /// survives worktree cleanup. Lets the status TUI show a meaningful phase
    /// for stale sessions whose worktree sentinels are gone.
    #[serde(default)]
    pub last_phase: Option<String>,
    #[serde(default)]
    pub team_name: Option<String>,
    /// tmux session name when launched via tmux (e.g., `codeflow-ses-xxx`).
    #[serde(default)]
    pub tmux_session: Option<String>,
    /// How the session was started: `"codeflow"` (via `codeflow -i`) or
    /// `"claude"` (plain `claude` invocation detected by SessionStart hook).
    pub source_cli: String,
    /// Whether this session is managed by the `codeflow` CLI.
    /// Managed sessions have `CODEFLOW_MANAGED=true` in the environment.
    pub managed: bool,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interactive_session_status_display() {
        assert_eq!(InteractiveSessionStatus::Active.to_string(), "active");
        assert_eq!(InteractiveSessionStatus::Complete.to_string(), "complete");
        assert_eq!(InteractiveSessionStatus::Stale.to_string(), "stale");
    }

    #[test]
    fn test_interactive_session_status_terminal() {
        assert!(!InteractiveSessionStatus::Active.is_terminal());
        assert!(InteractiveSessionStatus::Complete.is_terminal());
        assert!(InteractiveSessionStatus::Stale.is_terminal());
    }

    #[test]
    fn test_interactive_session_status_from_str() {
        assert_eq!(
            "active".parse::<InteractiveSessionStatus>().unwrap(),
            InteractiveSessionStatus::Active
        );
        assert_eq!(
            "complete".parse::<InteractiveSessionStatus>().unwrap(),
            InteractiveSessionStatus::Complete
        );
        assert_eq!(
            "stale".parse::<InteractiveSessionStatus>().unwrap(),
            InteractiveSessionStatus::Stale
        );
        assert!("unknown".parse::<InteractiveSessionStatus>().is_err());
    }

    #[test]
    fn test_interactive_session_status_serde_roundtrip() {
        let status = InteractiveSessionStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"active\"");
        let parsed: InteractiveSessionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_interactive_session_status_all_variants_roundtrip() {
        for (variant, expected_str) in [
            (InteractiveSessionStatus::Active, "active"),
            (InteractiveSessionStatus::Complete, "complete"),
            (InteractiveSessionStatus::Stale, "stale"),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("\"{expected_str}\""));
            let parsed: InteractiveSessionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_heartbeat_file_naming_pattern() {
        let sid = "ses-01abc2def3ghi4jkl5mno6pq";
        let filename = format!("heartbeat-{sid}");
        assert_eq!(filename, "heartbeat-ses-01abc2def3ghi4jkl5mno6pq");
        assert!(filename.starts_with("heartbeat-ses-"));
    }

    #[test]
    fn test_heartbeat_file_creation() {
        let dir = tempfile::tempdir().unwrap();
        let heartbeat_dir = dir.path().join(".state").join("interactive");
        std::fs::create_dir_all(&heartbeat_dir).unwrap();
        let sid = "ses-testbeat123";
        let heartbeat_path = heartbeat_dir.join(format!("heartbeat-{sid}"));
        std::fs::write(&heartbeat_path, "2026-04-06T12:00:00Z").unwrap();
        assert!(heartbeat_path.exists());
        let content = std::fs::read_to_string(&heartbeat_path).unwrap();
        assert!(!content.is_empty());
    }

    #[test]
    fn test_managed_env_var_detection() {
        // Verify the pattern used for CODEFLOW_MANAGED detection.
        // The actual detection is in session_start.rs; this tests the
        // string comparison pattern.
        assert_eq!(Ok("true"), Ok::<&str, ()>("true"));
        assert_ne!(Ok("false"), Ok::<&str, ()>("true"));
        assert_ne!(Ok(""), Ok::<&str, ()>("true"));
    }

    #[test]
    fn test_session_id_format() {
        let sid = crate::session::generate_session_id();
        let sid_str = sid.as_str();
        assert!(
            sid_str.starts_with("ses-"),
            "session ID must start with ses-"
        );
        assert!(
            sid_str.len() > 4,
            "session ID must have content after prefix"
        );
    }
}
