//! Heartbeat-based session liveness detection.
//!
//! Replaces the unreliable `lead_pid` approach (which stores the ephemeral
//! `sh -c` shell PID that exits immediately after hook completion) with a
//! file-based heartbeat that is updated on every hook invocation.
//!
//! The heartbeat file lives at `.state/runtime/heartbeat` (local per-worktree
//! via `WorktreePaths`). Its presence and recency indicate session liveness.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Heartbeat file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatData {
    pub session_id: String,
    pub timestamp: String,
    pub source: String,
}

/// Default throttle interval: skip writes if the file was written less than
/// this many seconds ago. `SessionStart` and `SessionEnd` bypass throttling.
const DEFAULT_THROTTLE_SECS: u64 = 5;

/// Return the canonical heartbeat file path for a project directory.
///
/// The heartbeat lives at `{project_dir}/.state/runtime/heartbeat`.
#[must_use]
pub fn heartbeat_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".state").join("runtime").join("heartbeat")
}

/// Update the heartbeat file with the current timestamp.
///
/// Writes a JSON file containing `session_id`, `timestamp`, and `source`.
/// Throttled: if the file was written less than `DEFAULT_THROTTLE_SECS` ago,
/// the write is skipped (unless `source` is a session lifecycle event which
/// always writes).
///
/// # Errors
///
/// Returns `std::io::Error` on filesystem failures (directory creation or write).
pub fn touch(project_dir: &Path, session_id: &str, source: &str) -> Result<(), std::io::Error> {
    let path = heartbeat_path(project_dir);

    // Session lifecycle events always write (no throttle).
    let bypass_throttle = source == "SessionStart"
        || source == "SessionEnd"
        || source == "init"
        || source == "cleanup";

    if !bypass_throttle && should_throttle(&path, DEFAULT_THROTTLE_SECS) {
        return Ok(());
    }

    let data = HeartbeatData {
        session_id: session_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        source: source.to_string(),
    };

    let json = serde_json::to_string_pretty(&data).map_err(std::io::Error::other)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, json)
}

/// Remove the heartbeat file (called by `SessionEnd` cleanup).
///
/// Silently succeeds if the file does not exist.
pub fn remove(project_dir: &Path) {
    let path = heartbeat_path(project_dir);
    let _ = fs::remove_file(&path);
}

/// Check whether the session is alive based on heartbeat recency.
///
/// Returns `true` if the heartbeat file exists and its `timestamp` field
/// is within `max_age_secs` of now. Returns `false` if the file is missing,
/// unreadable, or stale.
#[must_use]
pub fn is_alive(project_dir: &Path, max_age_secs: u64) -> bool {
    read_timestamp(project_dir)
        .and_then(|ts| ts.elapsed().ok())
        .is_some_and(|age| age.as_secs() <= max_age_secs)
}

/// Read the timestamp from the heartbeat file as a `SystemTime`.
///
/// Parses the JSON `timestamp` field (RFC 3339) and converts to `SystemTime`.
/// Returns `None` if the file is missing, unreadable, or has an invalid timestamp.
#[must_use]
pub fn read_timestamp(project_dir: &Path) -> Option<SystemTime> {
    let path = heartbeat_path(project_dir);
    let content = fs::read_to_string(&path).ok()?;
    let data: HeartbeatData = serde_json::from_str(&content).ok()?;
    let dt = chrono::DateTime::parse_from_rfc3339(&data.timestamp).ok()?;
    Some(SystemTime::from(dt))
}

/// Check whether the heartbeat file was written recently enough that we
/// should skip this write (throttle).
fn should_throttle(path: &Path, min_interval_secs: u64) -> bool {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|mtime| mtime.elapsed().ok())
        .is_some_and(|age| age.as_secs() < min_interval_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_touch_creates_heartbeat_file_with_correct_json() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqtestbeat0000000000000", "UserPromptSubmit").unwrap();

        let path = heartbeat_path(project);
        assert!(path.exists(), "heartbeat file should exist after touch");

        let content = fs::read_to_string(&path).unwrap();
        let data: HeartbeatData = serde_json::from_str(&content).unwrap();
        assert_eq!(data.session_id, "ses-01jqtestbeat0000000000000");
        assert_eq!(data.source, "UserPromptSubmit");
        assert!(!data.timestamp.is_empty());
    }

    #[test]
    fn test_touch_throttle_skips_within_interval() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqthrottletest000000000", "PreToolUse").unwrap();
        let path = heartbeat_path(project);
        let mtime1 = fs::metadata(&path).unwrap().modified().unwrap();

        // Second call within 5s should be throttled (no write).
        touch(project, "ses-01jqthrottletest000000000", "PreToolUse").unwrap();
        let mtime2 = fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(
            mtime1, mtime2,
            "mtime should not change within throttle window"
        );
    }

    #[test]
    fn test_touch_session_start_bypasses_throttle() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqbypasstest0000000000", "PreToolUse").unwrap();

        // SessionStart should always write, even within throttle window.
        touch(project, "ses-01jqbypasstest0000000000", "SessionStart").unwrap();
        let path = heartbeat_path(project);
        let content = fs::read_to_string(&path).unwrap();
        let data: HeartbeatData = serde_json::from_str(&content).unwrap();
        assert_eq!(data.source, "SessionStart");
    }

    #[test]
    fn test_touch_init_bypasses_throttle() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqinitbypass0000000000", "PreToolUse").unwrap();
        touch(project, "ses-01jqinitbypass0000000000", "init").unwrap();

        let path = heartbeat_path(project);
        let content = fs::read_to_string(&path).unwrap();
        let data: HeartbeatData = serde_json::from_str(&content).unwrap();
        assert_eq!(data.source, "init");
    }

    #[test]
    fn test_remove_deletes_heartbeat_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqremovetest0000000000", "init").unwrap();
        let path = heartbeat_path(project);
        assert!(path.exists());

        remove(project);
        assert!(!path.exists(), "heartbeat file should be removed");
    }

    #[test]
    fn test_remove_on_missing_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        // No heartbeat file exists -- remove should not panic.
        remove(dir.path());
    }

    #[test]
    fn test_is_alive_true_for_recent_heartbeat() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        touch(project, "ses-01jqalivetest00000000000", "init").unwrap();

        assert!(
            is_alive(project, 60),
            "heartbeat written just now should be alive with 60s threshold"
        );
    }

    #[test]
    fn test_is_alive_false_for_stale_heartbeat() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        // Write a heartbeat with a timestamp in the past.
        let past = chrono::Utc::now() - chrono::Duration::seconds(200);
        let data = HeartbeatData {
            session_id: "ses-01jqstaletest00000000000".to_string(),
            timestamp: past.to_rfc3339(),
            source: "test".to_string(),
        };
        let path = heartbeat_path(project);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_string_pretty(&data).unwrap()).unwrap();

        assert!(
            !is_alive(project, 60),
            "heartbeat with 200s-old timestamp should be stale with 60s threshold"
        );
    }

    #[test]
    fn test_is_alive_false_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !is_alive(dir.path(), 60),
            "missing heartbeat should be not alive"
        );
    }

    #[test]
    fn test_read_timestamp_returns_correct_system_time() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        let before = SystemTime::now();
        touch(project, "ses-01jqtstamptest0000000000", "init").unwrap();
        let after = SystemTime::now();

        let ts = read_timestamp(project).expect("should read timestamp");
        assert!(
            ts >= before && ts <= after,
            "timestamp should be between before and after"
        );
    }

    #[test]
    fn test_read_timestamp_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_timestamp(dir.path()).is_none());
    }

    #[test]
    fn test_heartbeat_path_resolves_correctly() {
        let dir = std::path::Path::new("/tmp/test-project");
        let path = heartbeat_path(dir);
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/test-project/.state/runtime/heartbeat")
        );
    }

    #[test]
    fn test_should_throttle_false_for_missing_file() {
        let path = std::path::Path::new("/tmp/nonexistent-heartbeat-throttle-test");
        assert!(!should_throttle(path, 5));
    }

    #[test]
    fn test_should_throttle_true_for_recent_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-throttle-recent");
        fs::write(&path, "data").unwrap();

        assert!(should_throttle(&path, 5));
    }

    #[test]
    fn test_touch_after_throttle_window_writes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();

        // Write initial heartbeat.
        touch(project, "ses-01jqafterwindow0000000000", "init").unwrap();

        // Manually set mtime to 10 seconds ago to simulate time passing.
        let path = heartbeat_path(project);
        let ten_ago =
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(10));
        filetime::set_file_mtime(&path, ten_ago).unwrap();

        // Now touch should write (not throttled).
        touch(project, "ses-01jqafterwindow0000000000", "PostToolUse").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let data: HeartbeatData = serde_json::from_str(&content).unwrap();
        assert_eq!(data.source, "PostToolUse");
    }
}
