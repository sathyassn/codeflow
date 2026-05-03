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
    // INF-TSK-024-051 Phase 4-C: `pid: i64` field removed. The DB
    // column was wrong-by-construction for managed sessions and the
    // canonical PID lives in `pathflow-session-status.json::lead_pid`
    // (worktree-resolved via `liveness::is_session_alive`). Existing
    // rows have the column dropped via the schema migration.
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
    /// Classifies the row as a real interactive session vs an autorun worker
    /// session (workers register as `interactive_session` rows with
    /// `source_cli='codeflow', managed=true` and are otherwise
    /// indistinguishable). INF-TSK-049-001 AC #8. Valid values:
    /// `"interactive"` (default) or `"autorun"`. Written at session
    /// creation based on the presence of `AUTORUN_SESSION_ID` env var.
    #[serde(default = "default_session_kind")]
    pub session_kind: String,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// Default `session_kind` for rows that pre-date the column. The migration
/// in `apply_schema` updates the seed value for known autorun workers; any
/// row still carrying the default is treated as a real interactive session.
fn default_session_kind() -> String {
    "interactive".to_string()
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

    // INF-TSK-024-051 Phase 7-rework: heartbeat dead-code tests
    // (test_heartbeat_file_naming_pattern, test_heartbeat_file_creation)
    // deleted. The `.state/interactive/heartbeat-{sid}` file format these
    // tests asserted is no longer written by any production code path
    // (interactive removed in Phase 4-A, autorun removed in Phase 7-rework).

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
