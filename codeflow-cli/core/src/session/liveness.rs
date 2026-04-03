//! Centralized session liveness detection.
//!
//! Replaces 11 ad-hoc inline `is_process_alive()` checks with a single
//! function that combines three independent signals:
//! 1. PID liveness (name-verified via `is_process_named`)
//! 2. Heartbeat freshness (`heartbeat::is_alive`)
//! 3. Pathflow-active flag (file existence)

use std::path::Path;

use super::{heartbeat, process};

/// Default heartbeat threshold in seconds (24 hours).
pub const DEFAULT_HEARTBEAT_THRESHOLD_SECS: u64 = 86400;

/// Result of a session liveness check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessResult {
    /// PID is alive and verified as Claude Code process.
    Active,
    /// PID is dead but heartbeat is recent (session may have just ended).
    Recent,
    /// All signals indicate the session is dead.
    Dead,
    /// Insufficient data to determine liveness (no PID, no heartbeat).
    Unknown,
}

impl LivenessResult {
    /// Returns `true` for `Active` or `Recent` — the session should be
    /// treated as alive for cleanup/skip purposes.
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Active | Self::Recent)
    }

    /// Human-readable label for display (e.g., in `codeflow worktree list`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Recent => "RECENT",
            Self::Dead => "DEAD",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl std::fmt::Display for LivenessResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Check session liveness using three independent signals.
///
/// - `lead_pid`: The PID stored in `pathflow-session-status.json` or
///   the worktree registry. 0 means unknown.
/// - `heartbeat_dir`: Directory containing the heartbeat file. `None`
///   skips the heartbeat check.
/// - `pathflow_active_path`: Path to the `pathflow-active` flag file.
///   `None` skips the flag check.
/// - `heartbeat_threshold_secs`: Max age for heartbeat to count as alive.
#[must_use]
pub fn check_session_liveness(
    lead_pid: u32,
    heartbeat_dir: Option<&Path>,
    pathflow_active_path: Option<&Path>,
    heartbeat_threshold_secs: u64,
) -> LivenessResult {
    // Signal 1: PID liveness.
    // Name verification ("claude") happens at write time via validate_claude_pid().
    // At read time, a plain alive check suffices -- the stored PID is already validated.
    let pid_alive = lead_pid > 0 && process::is_process_alive(lead_pid);

    if pid_alive {
        return LivenessResult::Active;
    }

    // Signal 2: Heartbeat freshness.
    let heartbeat_alive =
        heartbeat_dir.is_some_and(|dir| heartbeat::is_alive(dir, heartbeat_threshold_secs));

    if heartbeat_alive {
        return LivenessResult::Recent;
    }

    // Signal 3: Pathflow-active flag.
    let flag_exists = pathflow_active_path.is_some_and(Path::exists);

    if flag_exists {
        // Flag exists but neither PID nor heartbeat confirms life.
        // Treat as Recent — may be a very fresh crash.
        return LivenessResult::Recent;
    }

    // No PID available means we cannot determine state at all.
    if lead_pid == 0 && heartbeat_dir.is_none() && pathflow_active_path.is_none() {
        return LivenessResult::Unknown;
    }

    LivenessResult::Dead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_when_pid_alive() {
        // Current process PID is alive, so liveness should be Active.
        let pid = std::process::id();
        let result = check_session_liveness(pid, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Active);
    }

    #[test]
    fn test_dead_pid_not_active() {
        // A dead PID should NOT return Active.
        let result =
            check_session_liveness(4_000_000, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_ne!(result, LivenessResult::Active);
    }

    #[test]
    fn test_dead_when_pid_dead_no_heartbeat_no_flag() {
        let result =
            check_session_liveness(4_000_000, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Dead);
    }

    #[test]
    fn test_unknown_when_no_signals() {
        let result = check_session_liveness(0, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Unknown);
    }

    #[test]
    fn test_recent_when_heartbeat_alive() {
        let dir = tempfile::tempdir().unwrap();
        // Write a fresh heartbeat file.
        let hb_path = dir.path().join(".state/runtime/heartbeat");
        std::fs::create_dir_all(hb_path.parent().unwrap()).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let data = serde_json::json!({
            "session_id": "ses-test",
            "timestamp": now,
            "source": "test"
        });
        std::fs::write(&hb_path, serde_json::to_string(&data).unwrap()).unwrap();

        // Dead PID but fresh heartbeat -> Recent.
        let result = check_session_liveness(
            4_000_000,
            Some(dir.path()),
            None,
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Recent);
    }

    #[test]
    fn test_recent_when_pathflow_active_exists() {
        let dir = tempfile::tempdir().unwrap();
        let flag_path = dir.path().join("pathflow-active");
        std::fs::write(&flag_path, "").unwrap();

        // Dead PID, no heartbeat, but flag exists -> Recent.
        let result = check_session_liveness(
            4_000_000,
            None,
            Some(&flag_path),
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Recent);
    }

    #[test]
    fn test_dead_when_all_signals_negative() {
        let dir = tempfile::tempdir().unwrap();
        let flag_path = dir.path().join("nonexistent-flag");

        let result = check_session_liveness(
            4_000_000,
            Some(dir.path()),
            Some(&flag_path),
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Dead);
    }

    #[test]
    fn test_is_alive_active() {
        assert!(LivenessResult::Active.is_alive());
    }

    #[test]
    fn test_is_alive_recent() {
        assert!(LivenessResult::Recent.is_alive());
    }

    #[test]
    fn test_is_alive_dead() {
        assert!(!LivenessResult::Dead.is_alive());
    }

    #[test]
    fn test_is_alive_unknown() {
        assert!(!LivenessResult::Unknown.is_alive());
    }

    #[test]
    fn test_label_values() {
        assert_eq!(LivenessResult::Active.label(), "ACTIVE");
        assert_eq!(LivenessResult::Recent.label(), "RECENT");
        assert_eq!(LivenessResult::Dead.label(), "DEAD");
        assert_eq!(LivenessResult::Unknown.label(), "UNKNOWN");
    }

    #[test]
    fn test_display_matches_label() {
        assert_eq!(format!("{}", LivenessResult::Active), "ACTIVE");
        assert_eq!(format!("{}", LivenessResult::Dead), "DEAD");
    }
}
