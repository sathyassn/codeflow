//! Session-end hook handlers.
//!
//! Implements `SessionEndCleanup` as a `HookHandler` implementation.
//! This mirrors the Go handler in `internal/hooks/session/end.go`.
//!
//! Note: `SessionEndLogging` (session-end ledger event) is in
//! `hooks::logging::SessionEndLogging` -- the logging module handles all
//! session lifecycle logging.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono;

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput, PathflowTeamInfo};
use crate::pathflow;
use crate::session;

/// Time source for deterministic testing.
pub type NowFn = fn() -> String;

// ---------------------------------------------------------------------------
// SessionEndCleanup
// ---------------------------------------------------------------------------

/// Output from a session-end cleanup operation.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// The `CODEFLOW_SESSION_ID` used for this session.
    pub session_id: String,
    /// `true` when the `pathflow-pf-7` sentinel was found.
    pub pf7_valid: bool,
    /// Count of sentinel directories removed.
    pub sentinels_cleaned: u32,
    /// `true` when an active task was kept for the next session.
    pub task_preserved: bool,
    /// Team name read from `pathflow-team.json` (if any).
    pub team_name: String,
    /// Non-fatal warning messages.
    pub warnings: Vec<String>,
    /// Informational messages for stderr.
    pub messages: Vec<String>,
}

/// Session-end cleanup handler with injectable dependencies.
///
/// Performs the 13-section cleanup flow: `PathFlow` guard, PF7 validation,
/// sentinel cleanup, active task handling, team backstop, runtime cleanup,
/// and ledger event writing.
pub struct SessionEndCleanup {
    pub home_dir: PathBuf,
    pub ppid: u32,
    pub now: NowFn,
}

#[allow(clippy::unused_self)]
impl SessionEndCleanup {
    /// Run the full session-end cleanup flow.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures.
    pub fn run(
        &self,
        _input: &HookInput,
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<CleanupResult, HookError> {
        let mut result = CleanupResult {
            session_id: String::new(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // --- Section 2: Resolve session ID ---
        let runtime_dir = project_dir.join(".state").join("runtime");
        let Ok(sid) = session::current_session_id(&runtime_dir) else {
            result
                .messages
                .push("no session ID found, skipping cleanup".into());
            write_messages(writer, &result.messages)?;
            return Ok(result);
        };
        let session_id = sid.as_str().to_string();
        result.session_id.clone_from(&session_id);

        let session_state_dir = project_dir.join(".state").join("session").join(&session_id);

        // Log cleanup start.
        write_cleanup_log(project_dir, &serde_json::json!({
            "event": "cleanup_started",
            "session_id": session_id,
            "pid": self.ppid,
        }));

        // --- Section 3: PathFlow guard ---
        if self.should_skip_cleanup(&session_state_dir, &mut result) {
            write_cleanup_log(project_dir, &serde_json::json!({
                "event": "cleanup_skipped",
                "session_id": session_id,
                "reason": "session_active",
            }));
            write_messages(writer, &result.messages)?;
            return Ok(result);
        }

        // --- Section 4: PF7 diagnostic ---
        result.pf7_valid = self.validate_pf7(project_dir, &session_id, &mut result);

        // --- Section 5: PathFlow sentinel cleanup ---
        self.clean_pathflow_sentinels(project_dir, &session_id, &mut result);

        // --- Section 6: Active task preservation ---
        self.handle_active_task(project_dir, &mut result);

        // --- Section 8: Read team info ---
        let team_name = self.read_team_name(&session_state_dir);
        result.team_name.clone_from(&team_name);

        // --- Section 9: Team config/task list backstop cleanup ---
        self.clean_team_artifacts(&team_name, &session_state_dir, &mut result);

        // --- Section 10: Session state directory cleanup ---
        self.clean_session_state(&session_state_dir, &session_id, &mut result);

        // --- Section 11: Runtime file cleanup ---
        self.clean_runtime_files(project_dir, &mut result);

        // --- Section 12: Project temp directory cleanup ---
        self.clean_project_temp(project_dir);

        // Note: session_end ledger event is written by SessionEndLogging
        // handler (hooks::logging module), not here.

        // Log cleanup completion.
        write_cleanup_log(project_dir, &serde_json::json!({
            "event": "cleanup_completed",
            "session_id": session_id,
            "pf7_valid": result.pf7_valid,
            "sentinels_cleaned": result.sentinels_cleaned,
            "task_preserved": result.task_preserved,
            "team_name": result.team_name,
            "warnings_count": result.warnings.len(),
        }));

        write_messages(writer, &result.messages)?;

        Ok(result)
    }

    /// Check the `PathFlow` guard using multi-signal approach. Returns `true`
    /// if cleanup should be skipped (session still active).
    ///
    /// Uses `pathflow-session-status.json` as the sole authority, with
    /// team config existence as a secondary check.
    fn should_skip_cleanup(&self, session_state_dir: &Path, result: &mut CleanupResult) -> bool {
        let pathflow_dir = session_state_dir.join("pathflow");
        let status_path = pathflow_dir.join("pathflow-session-status.json");

        // Read the status file -- sole authority for session state.
        let status_data = match fs::read_to_string(&status_path) {
            Ok(data) => data,
            Err(_) => {
                // No status file -- no active PathFlow session, allow cleanup.
                return false;
            }
        };

        let status: serde_json::Value = match serde_json::from_str(&status_data) {
            Ok(v) => v,
            Err(_) => {
                result.messages.push(
                    "SessionEnd: pathflow-session-status.json unreadable -- proceeding".into(),
                );
                return false;
            }
        };

        let session_status = status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Status "created" means no team was ever created -- safe to clean up.
        if session_status == "created" {
            result.messages.push(
                "SessionEnd: Session status is 'created' (no team) -- proceeding".into(),
            );
            return false;
        }

        // Status "pf-complete" means session finished normally -- clean up.
        if session_status == "pf-complete" {
            result.messages.push(
                "SessionEnd: Session status is 'pf-complete' -- proceeding with cleanup".into(),
            );
            return false;
        }

        // Status is "pf-started" or "pf-in-progress" -- session may be active.
        let team_name = status
            .get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if team_name.is_empty() {
            result
                .messages
                .push("SessionEnd: Session active but no team name -- proceeding".into());
            return false;
        }

        // Check if team config still exists (team not yet deleted).
        let config_path = self
            .home_dir
            .join(".claude")
            .join("teams")
            .join(team_name)
            .join("config.json");

        if !config_path.exists() {
            // Config missing -- but check if sentinel dir still has pathflow state.
            // If sentinels exist, config was likely deleted by a race condition
            // (e.g., concurrent session cleanup). Preserve the session.
            let sentinel_dir = session_state_dir
                .parent() // .state/session/{sid}
                .and_then(|p| p.parent()) // .state/session
                .and_then(|p| p.parent()) // .state
                .map(|state_dir| {
                    let sid = session_state_dir.file_name().unwrap_or_default();
                    state_dir.join("sentinels").join("pathflow").join(sid)
                });

            let has_sentinels = sentinel_dir
                .as_ref()
                .is_some_and(|d| d.exists() && fs::read_dir(d).is_ok_and(|mut r| r.next().is_some()));

            if has_sentinels {
                result.messages.push(format!(
                    "SessionEnd: Team config gone for '{team_name}' but sentinels exist -- skipping cleanup (possible race)"
                ));
                return true;
            }

            result.messages.push(format!(
                "SessionEnd: Session active but team config gone for '{team_name}' -- proceeding"
            ));
            return false;
        }

        // Edge case: status stuck but PF7 already completed.
        let last_phase = status
            .get("last_completed_phase")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if last_phase == "pf-7" {
            result.messages.push(
                "SessionEnd: Status stuck but PF7 completed -- proceeding with cleanup".into(),
            );
            return false;
        }

        // All signals indicate active session -- skip cleanup.
        result.messages.push(format!(
            "SessionEnd: Session active (status={session_status}, team={team_name}, phase={last_phase}) -- skipping cleanup"
        ));
        true
    }

    /// Validate PF7 completion (check for `pathflow-pf-7` sentinel).
    fn validate_pf7(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut CleanupResult,
    ) -> bool {
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);

        let pf7_exists =
            pathflow::sentinel::check_sentinel(&sentinel_dir, crate::types::Sentinel::PathflowPf7);

        if pf7_exists {
            result
                .messages
                .push("SessionEnd: Clean PF7 shutdown (all phases completed)".into());
        } else {
            result
                .messages
                .push("SessionEnd: Incomplete PF7 shutdown (pf-7 sentinel absent)".into());

            if let Ok(sentinels) = pathflow::sentinel::list_sentinels(&sentinel_dir) {
                if !sentinels.is_empty() {
                    result.messages.push(format!(
                        "SessionEnd: existing sentinels: {}",
                        sentinels.join(", ")
                    ));
                }
            }
        }

        pf7_exists
    }

    /// Remove all `PathFlow` sentinels for this session.
    fn clean_pathflow_sentinels(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut CleanupResult,
    ) {
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);

        if sentinel_dir.exists() && fs::remove_dir_all(&sentinel_dir).is_ok() {
            result.sentinels_cleaned += 1;
        }
    }

    /// Handle active task preservation (keep `in_progress`, remove completed).
    fn handle_active_task(&self, project_dir: &Path, result: &mut CleanupResult) {
        let runtime_dir = project_dir.join(".state").join("runtime");
        match session::get_active_task(&runtime_dir) {
            Ok(Some(task)) => {
                if let Some(status) = &task.status {
                    if status == "in_progress" {
                        result.task_preserved = true;
                        result.messages.push(format!(
                            "SessionEnd: Task {} preserved for next session",
                            task.task_id.as_str()
                        ));
                        return;
                    }
                }
                let _ = session::clear_active_task(&runtime_dir);
            }
            Ok(None) => {}
            Err(_) => {
                // Unreadable -- clean it.
                let _ = session::clear_active_task(&runtime_dir);
            }
        }
    }

    /// Read the team name from `pathflow-team.json`.
    fn read_team_name(&self, session_state_dir: &Path) -> String {
        let team_file_path = session_state_dir
            .join("pathflow")
            .join("pathflow-team.json");

        fs::read_to_string(&team_file_path)
            .ok()
            .and_then(|data| serde_json::from_str::<PathflowTeamInfo>(&data).ok())
            .map(|info| info.team_name)
            .unwrap_or_default()
    }

    /// Remove stale team config and task list directories.
    ///
    /// Re-checks session status before deleting to guard against race conditions
    /// where the session became active again between the skip check and this call.
    fn clean_team_artifacts(&self, team_name: &str, session_state_dir: &Path, result: &mut CleanupResult) {
        if team_name.is_empty() {
            return;
        }

        // Re-check session status -- guard against race where session reactivated.
        let status_path = session_state_dir
            .join("pathflow")
            .join("pathflow-session-status.json");
        if let Ok(data) = fs::read_to_string(&status_path) {
            if let Ok(status) = serde_json::from_str::<serde_json::Value>(&data) {
                let s = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if s == "pf-started" || s == "pf-in-progress" {
                    result.messages.push(format!(
                        "SessionEnd: Skipping team artifact cleanup -- session still active (status={s})"
                    ));
                    return;
                }
            }
        }

        let teams_dir = self.home_dir.join(".claude").join("teams").join(team_name);
        if teams_dir.exists() && fs::remove_dir_all(&teams_dir).is_ok() {
            result
                .messages
                .push(format!("SessionEnd: Cleaned team config: {team_name}"));
        }

        let tasks_dir = self.home_dir.join(".claude").join("tasks").join(team_name);
        if tasks_dir.exists() {
            let _ = fs::remove_dir_all(&tasks_dir);
        }
    }

    /// Remove the session state directory.
    fn clean_session_state(
        &self,
        session_state_dir: &Path,
        _session_id: &str,
        _result: &mut CleanupResult,
    ) {
        if session_state_dir.exists() {
            let _ = fs::remove_dir_all(session_state_dir);
        }
    }

    /// Remove runtime files (env file, session lock, `current-session-id`).
    ///
    /// Race safety: reads `codeflow-env.sh` and only removes it if the session ID
    /// inside matches the session being cleaned up. If a different session owns the
    /// file (concurrent session started between cleanup phases), it is preserved.
    fn clean_runtime_files(&self, project_dir: &Path, result: &mut CleanupResult) {
        let runtime_dir = project_dir.join(".state").join("runtime");

        // Race-safe env file removal: only remove if owned by this session.
        match session::read_env_file(&runtime_dir) {
            Ok(Some(env)) => {
                let file_sid = env.session_id.as_str().to_string();
                if file_sid == result.session_id {
                    let _ = session::remove_env_file(&runtime_dir);
                } else {
                    result.warnings.push(format!(
                        "SessionEnd: env file owned by different session ({file_sid}), preserving"
                    ));
                }
            }
            Ok(None) => {} // No env file, nothing to remove.
            Err(_) => {
                // Unreadable env file -- may belong to another session mid-write.
                // Preserve it; next SessionStart will overwrite if needed.
                result.warnings.push("SessionEnd: env file unreadable, preserving".to_string());
            }
        }

        // Remove session lock file.
        let lock_path = runtime_dir.join("session.lock");
        if lock_path.exists() {
            let _ = fs::remove_file(&lock_path);
        }

        // Also remove current-session-id if it exists (legacy cleanup).
        let current_sid_path = runtime_dir.join("current-session-id");
        if current_sid_path.exists() {
            let _ = fs::remove_file(&current_sid_path);
        }
    }

    /// Remove the project temp directory.
    fn clean_project_temp(&self, project_dir: &Path) {
        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());

        let tmp_dir = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("managed");

        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
    }

}

impl HookHandler for SessionEndCleanup {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input.project_dir.as_deref().ok_or_else(|| {
            HookError::Config("project_dir required for session-end cleanup".into())
        })?;

        let mut buf = Vec::new();
        self.run(&input, Path::new(project_dir), &mut buf)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-end-cleanup"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionEnd]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a structured JSONL cleanup event to `.state/logs/sessions/cleanup-{date}.jsonl`.
fn write_cleanup_log(project_dir: &Path, event: &serde_json::Value) {
    let now = chrono::Utc::now();
    let date = now.format("%Y-%m-%d").to_string();
    let log_dir = project_dir.join(".state").join("logs").join("sessions");
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!("cleanup-{date}.jsonl"));

    let mut entry = event.clone();
    if let Some(obj) = entry.as_object_mut() {
        obj.insert("timestamp".into(), serde_json::Value::String(now.to_rfc3339()));
    }

    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            });
    }
}

fn write_messages(writer: &mut dyn Write, messages: &[String]) -> Result<(), HookError> {
    for msg in messages {
        writeln!(writer, "{msg}").map_err(HookError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::SessionMeta;
    use crate::types::SessionId;

    fn fixed_now() -> String {
        "2026-03-10T01:00:00Z".to_string()
    }

    fn make_cleaner(home: PathBuf) -> SessionEndCleanup {
        SessionEndCleanup {
            home_dir: home,
            ppid: 1000,
            now: fixed_now,
        }
    }

    fn make_input(project_dir: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: None,
            project_dir: Some(project_dir.into()),
            source: None,
            transcript_path: None,
        }
    }

    /// Set up a project directory with a valid session for cleanup testing.
    fn setup_session(dir: &Path) -> String {
        let runtime_dir = dir.join(".state").join("runtime");
        let sid = SessionId::new_unchecked("ses-01jq7cleanup1234567890ab");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create session state directories.
        let session_dir = dir
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Create sentinel directory with some sentinels.
        let sentinel_dir = dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid.as_str());
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), b"").unwrap();

        // Create session meta.
        let meta = SessionMeta {
            session_id: sid.as_str().to_string(),
            created_at: "2026-03-10T00:00:00Z".into(),
            source: "startup".into(),
            ppid: 1000,
            version: "test".into(),
            permission_mode: None,
        };
        let meta_path = dir
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("session-meta.json");
        fs::write(&meta_path, serde_json::to_string(&meta).unwrap()).unwrap();

        // Create ledger directory.
        fs::create_dir_all(dir.join(".state").join("ledger")).unwrap();

        sid.as_str().to_string()
    }

    // --- SessionEndCleanup tests ---

    #[test]
    fn test_cleanup_full_flow() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert_eq!(result.session_id, session_id);
        assert!(!result.pf7_valid); // No pf-7 sentinel.
        assert_eq!(result.sentinels_cleaned, 1);

        // Verify sentinel directory was removed.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        assert!(!sentinel_dir.exists());

        // Verify env file was removed.
        let runtime_dir = dir.path().join(".state").join("runtime");
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());
    }

    #[test]
    fn test_cleanup_skip_when_session_active() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-session-status.json with active status.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "test-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-4",
            "last_completed_stage": "WS-DEV",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:30:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        // Create Claude Code team config so the team existence check passes.
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev"}]}"#,
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should be skipped (session active with team config present).
        assert_eq!(result.sentinels_cleaned, 0);
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("skipping cleanup"))
        );

        // Verify sentinel directory still exists.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        assert!(sentinel_dir.exists());
    }

    #[test]
    fn test_cleanup_proceeds_when_session_complete() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-session-status.json with pf-complete status.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "test-team",
            "status": "pf-complete",
            "last_completed_phase": "pf-7",
            "last_completed_stage": "WS-QA",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T01:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should proceed (session completed normally).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("pf-complete"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_pf7_valid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create the pf-7 sentinel.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        pathflow::sentinel::create_sentinel(&sentinel_dir, crate::types::Sentinel::PathflowPf7)
            .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.pf7_valid);
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Clean PF7 shutdown"))
        );
    }

    #[test]
    fn test_cleanup_task_preservation() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        setup_session(dir.path());

        // Set an in_progress active task.
        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-preserve"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: Some("in_progress".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.task_preserved);
        assert!(result.messages.iter().any(|m| m.contains("preserved")));

        // Active task file should still exist.
        assert!(session::get_active_task(&runtime_dir).unwrap().is_some());
    }

    #[test]
    fn test_cleanup_no_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        // No env file exists.
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.session_id.is_empty());
        assert!(result.messages.iter().any(|m| m.contains("no session ID")));
    }

    #[test]
    fn test_cleanup_handler_trait_error_on_missing_project_dir() {
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };

        let result = cleaner.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    #[test]
    fn test_cleanup_proceeds_when_team_config_gone() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file claiming active, but NO team config directory.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "missing-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-3",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:15:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (team config gone).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("team config gone"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_empty_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with active status but empty team_name.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "pf-started",
            "last_completed_phase": "",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:05:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (no team name).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("no team name"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_status_created() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with "created" status (no team ever created).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "created",
            "last_completed_phase": "",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (status is "created").
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("created"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_pf7_stuck_status() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with active status but last_completed_phase = PF7
        // (status stuck at pf-in-progress but PF7 already done).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "stuck-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-7",
            "last_completed_stage": "WS-QA",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T01:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        // Team config exists.
        let config_dir = home.path().join(".claude").join("teams").join("stuck-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev"}]}"#,
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (PF7 completed, status just stuck).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("PF7 completed"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_no_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        setup_session(dir.path());

        // No pathflow-session-status.json and no legacy flag -- should proceed.
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (no status file present).
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_no_status_file_allows_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // No status file -- cleanup should proceed (no active PathFlow session).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // No status file means no active session -- cleanup proceeds.
        assert_eq!(result.session_id, session_id);
    }

    #[test]
    fn test_cleanup_removes_session_lock() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create a session.lock file in runtime dir.
        let runtime_dir = dir.path().join(".state").join("runtime");
        let lock_path = runtime_dir.join("session.lock");
        fs::write(&lock_path, b"").unwrap();
        assert!(lock_path.exists(), "lock file should exist before cleanup");

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let _result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Verify lock file was removed.
        assert!(
            !lock_path.exists(),
            "session.lock should be removed during cleanup"
        );

        // Verify env file was also removed (existing behavior).
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());

        // Verify session_id was resolved correctly (cleanup proceeded).
        assert_eq!(_result.session_id, session_id);
    }
}
