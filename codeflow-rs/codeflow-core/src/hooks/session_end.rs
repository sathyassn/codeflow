//! Session-end hook handlers.
//!
//! Implements `SessionEndCleanup` and `SessionEndLogging` as `HookHandler`
//! implementations. These mirror the Go handlers in
//! `internal/hooks/session/end.go`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{
    HookEvent, HookHandler, HookInput, HookOutput, PathflowTeamInfo, ProcessChecker, SessionMeta,
};
use crate::ledger::{Event, LedgerWriter};
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
pub struct SessionEndCleanup<P: ProcessChecker> {
    pub process_checker: P,
    pub home_dir: PathBuf,
    pub ppid: u32,
    pub now: NowFn,
}

#[allow(clippy::unused_self)]
impl<P: ProcessChecker> SessionEndCleanup<P> {
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

        // --- Section 3: PathFlow guard ---
        if self.should_skip_cleanup(&session_state_dir, &mut result) {
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
        self.clean_team_artifacts(&team_name, &mut result);

        // --- Section 10: Session state directory cleanup ---
        self.clean_session_state(&session_state_dir, &session_id, &mut result);

        // --- Section 11: Runtime file cleanup ---
        self.clean_runtime_files(project_dir, &mut result);

        // --- Section 12: Project temp directory cleanup ---
        self.clean_project_temp(project_dir);

        // --- Section 13: Write session_end ledger event ---
        self.write_ledger_event(project_dir, &session_id, &mut result);

        write_messages(writer, &result.messages)?;

        Ok(result)
    }

    /// Check the `PathFlow` guard. Returns `true` if cleanup should be skipped
    /// (teammate shutdown while lead is alive).
    fn should_skip_cleanup(&self, session_state_dir: &Path, result: &mut CleanupResult) -> bool {
        let flag_path = session_state_dir
            .join("pathflow")
            .join("is-pathflow-active");
        if !flag_path.exists() {
            return false;
        }

        let team_file_path = session_state_dir
            .join("pathflow")
            .join("pathflow-team.json");

        let Ok(team_data) = fs::read_to_string(&team_file_path) else {
            result.messages.push(
                "SessionEnd: PathFlow active but no team file -- proceeding with cleanup".into(),
            );
            return false;
        };

        let Ok(team_info) = serde_json::from_str::<PathflowTeamInfo>(&team_data) else {
            result
                .messages
                .push("SessionEnd: PathFlow active but team file unreadable -- proceeding".into());
            return false;
        };

        if team_info.lead_pid == 0 {
            result
                .messages
                .push("SessionEnd: PathFlow active but lead PID unknown -- proceeding".into());
            return false;
        }

        // Check: Is this the lead's own SessionEnd?
        if team_info.lead_pid == self.ppid {
            result.messages.push(format!(
                "SessionEnd: PathFlow active, PPID matches lead PID {} -- proceeding (lead's own SessionEnd)",
                team_info.lead_pid
            ));
            return false;
        }

        // Check: Is the lead alive?
        if self.process_checker.is_alive(team_info.lead_pid) {
            result.messages.push(format!(
                "SessionEnd: PathFlow active, lead PID {} alive -- skipping cleanup (teammate shutdown)",
                team_info.lead_pid
            ));
            return true;
        }

        // Lead is dead -- orphaned session.
        result.messages.push(format!(
            "SessionEnd: PathFlow active but lead PID {} dead -- proceeding (orphaned session)",
            team_info.lead_pid
        ));
        false
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
    fn clean_team_artifacts(&self, team_name: &str, result: &mut CleanupResult) {
        if team_name.is_empty() {
            return;
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
    fn clean_runtime_files(&self, project_dir: &Path, _result: &mut CleanupResult) {
        let runtime_dir = project_dir.join(".state").join("runtime");
        let _ = session::remove_env_file(&runtime_dir);

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

    /// Write the `session_end` ledger event.
    fn write_ledger_event(&self, project_dir: &Path, session_id: &str, result: &mut CleanupResult) {
        let ledger_dir = project_dir.join(".state").join("ledger");
        if !ledger_dir.exists() {
            return;
        }

        // Compute duration from session-meta.json.
        let duration_seconds = self.compute_duration(project_dir, session_id);

        let mut data = HashMap::new();
        data.insert(
            "pf7_valid".into(),
            serde_json::Value::Bool(result.pf7_valid),
        );
        data.insert(
            "sentinels_cleaned".into(),
            serde_json::json!(result.sentinels_cleaned),
        );
        data.insert(
            "task_preserved".into(),
            serde_json::Value::Bool(result.task_preserved),
        );
        if let Some(duration) = duration_seconds {
            data.insert("duration_seconds".into(), serde_json::json!(duration));
        }

        let event = Event {
            event_type: "session_end".into(),
            timestamp: (self.now)(),
            session_id: Some(session_id.to_string()),
            data,
        };

        // Best-effort write -- don't fail cleanup on ledger errors.
        match crate::ledger::JsonlWriter::new(ledger_dir) {
            Ok(writer) => {
                if let Err(e) = writer.append_event(event) {
                    result.warnings.push(format!("ledger write error: {e}"));
                }
            }
            Err(e) => {
                result.warnings.push(format!("ledger init error: {e}"));
            }
        }
    }

    /// Compute session duration from `session-meta.json` `created_at`.
    fn compute_duration(&self, project_dir: &Path, session_id: &str) -> Option<f64> {
        let meta_path = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("session-meta.json");

        let data = fs::read_to_string(&meta_path).ok()?;
        let meta: SessionMeta = serde_json::from_str(&data).ok()?;

        let start = crate::util::parse_timestamp(&meta.created_at).ok()?;
        let now_str = (self.now)();
        let now = crate::util::parse_timestamp(&now_str).ok()?;

        let duration = now.signed_duration_since(start);
        #[allow(clippy::cast_precision_loss)]
        Some(duration.num_seconds() as f64)
    }
}

impl<P: ProcessChecker> HookHandler for SessionEndCleanup<P> {
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
// SessionEndLogging
// ---------------------------------------------------------------------------

/// Session-end logging handler.
///
/// Computes session duration from the session-meta file and writes a
/// `session_end` event to the sessions JSONL ledger.
pub struct SessionEndLogging<L: LedgerWriter> {
    pub ledger: L,
    pub now: NowFn,
}

impl<L: LedgerWriter> SessionEndLogging<L> {
    /// Write the `session_end` ledger event.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on ledger write failure.
    pub fn log_end(&self, session_id: &str, project_dir: &Path) -> Result<(), HookError> {
        // Compute duration from session-meta.json.
        let duration_seconds = self.compute_duration(project_dir, session_id);

        let mut data = HashMap::new();
        if let Some(duration) = duration_seconds {
            data.insert("duration_seconds".into(), serde_json::json!(duration));
        }

        let event = Event {
            event_type: "session_end".into(),
            timestamp: (self.now)(),
            session_id: Some(session_id.to_string()),
            data,
        };

        self.ledger
            .append_event(event)
            .map_err(|e| HookError::Config(format!("ledger write error: {e}")))?;

        Ok(())
    }

    fn compute_duration(&self, project_dir: &Path, session_id: &str) -> Option<f64> {
        let meta_path = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("session-meta.json");

        let data = fs::read_to_string(&meta_path).ok()?;
        let meta: SessionMeta = serde_json::from_str(&data).ok()?;

        let start = crate::util::parse_timestamp(&meta.created_at).ok()?;
        let now_str = (self.now)();
        let now = crate::util::parse_timestamp(&now_str).ok()?;

        let duration = now.signed_duration_since(start);
        #[allow(clippy::cast_precision_loss)]
        Some(duration.num_seconds() as f64)
    }
}

impl<L: LedgerWriter> HookHandler for SessionEndLogging<L> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let session_id = input.session_id.as_deref().unwrap_or("unknown");
        let project_dir = input.project_dir.as_deref().unwrap_or(".");

        self.log_end(session_id, Path::new(project_dir))?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-end-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionEnd]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_messages(writer: &mut dyn Write, messages: &[String]) -> Result<(), HookError> {
    for msg in messages {
        writeln!(writer, "{msg}").map_err(HookError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::ProcessChecker;
    use crate::types::SessionId;

    struct MockProcessChecker {
        alive_pids: Vec<u32>,
    }

    impl ProcessChecker for MockProcessChecker {
        fn is_alive(&self, pid: u32) -> bool {
            self.alive_pids.contains(&pid)
        }
    }

    fn fixed_now() -> String {
        "2026-03-10T01:00:00Z".to_string()
    }

    fn make_cleaner(alive_pids: Vec<u32>, home: PathBuf) -> SessionEndCleanup<MockProcessChecker> {
        SessionEndCleanup {
            process_checker: MockProcessChecker { alive_pids },
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

        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
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
    fn test_cleanup_skip_when_teammate() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-team.json with alive lead PID.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let flag_path = team_dir.join("is-pathflow-active");
        fs::write(&flag_path, b"{}").unwrap();

        let team_info = PathflowTeamInfo {
            lead_pid: 42,
            team_name: "test-team".into(),
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // PID 42 is alive, PPID is 1000 (not the lead).
        let cleaner = make_cleaner(vec![42], home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should be skipped.
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
    fn test_cleanup_proceeds_when_lead_dead() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-team.json with dead lead PID.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let flag_path = team_dir.join("is-pathflow-active");
        fs::write(&flag_path, b"{}").unwrap();

        let team_info = PathflowTeamInfo {
            lead_pid: 99999,
            team_name: "test-team".into(),
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // PID 99999 is NOT alive.
        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should proceed (lead is dead = orphaned session).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("orphaned session"))
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

        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
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

        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
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
        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.session_id.is_empty());
        assert!(result.messages.iter().any(|m| m.contains("no session ID")));
    }

    #[test]
    fn test_cleanup_handler_trait_error_on_missing_project_dir() {
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
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

    // --- SessionEndLogging tests ---

    struct MockLedger {
        events: std::sync::Mutex<Vec<Event>>,
    }

    impl MockLedger {
        fn new() -> Self {
            Self {
                events: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn last_event(&self) -> Option<Event> {
            self.events.lock().unwrap().last().cloned()
        }
    }

    impl LedgerWriter for MockLedger {
        fn append_event(&self, event: Event) -> Result<(), crate::error::LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn append_event_to_file(
            &self,
            _target_file: &str,
            event: Event,
        ) -> Result<(), crate::error::LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn route_event(&self, _event_type: &str) -> Result<String, crate::error::LedgerError> {
            Ok("sessions.jsonl".into())
        }

        fn dir(&self) -> &Path {
            Path::new("/tmp")
        }
    }

    #[test]
    fn test_end_logging_writes_event() {
        let dir = tempfile::tempdir().unwrap();

        // Create session-meta.json for duration computation.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join("ses-log-test");
        fs::create_dir_all(&session_dir).unwrap();
        let meta = SessionMeta {
            session_id: "ses-log-test".into(),
            created_at: "2026-03-10T00:00:00Z".into(),
            source: "startup".into(),
            ppid: 1000,
            version: "test".into(),
            permission_mode: None,
        };
        fs::write(
            session_dir.join("session-meta.json"),
            serde_json::to_string(&meta).unwrap(),
        )
        .unwrap();

        let ledger = MockLedger::new();
        let handler = SessionEndLogging {
            ledger,
            now: fixed_now, // 2026-03-10T01:00:00Z
        };

        handler.log_end("ses-log-test", dir.path()).unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert_eq!(event.event_type, "session_end");
        assert_eq!(event.session_id.as_deref(), Some("ses-log-test"));

        // Duration should be 3600 seconds (1 hour).
        let duration = event.data.get("duration_seconds").unwrap();
        assert_eq!(duration.as_f64().unwrap(), 3600.0);
    }

    #[test]
    fn test_end_logging_no_meta_file() {
        let dir = tempfile::tempdir().unwrap();

        let ledger = MockLedger::new();
        let handler = SessionEndLogging {
            ledger,
            now: fixed_now,
        };

        handler.log_end("ses-no-meta", dir.path()).unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert_eq!(event.event_type, "session_end");
        // No duration since no meta file exists.
        assert!(!event.data.contains_key("duration_seconds"));
    }

    #[test]
    fn test_end_logging_handler_trait() {
        let ledger = MockLedger::new();
        let handler = SessionEndLogging {
            ledger,
            now: fixed_now,
        };

        assert_eq!(handler.name(), "session-end-logging");
        assert_eq!(handler.events(), &[HookEvent::SessionEnd]);
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

        let cleaner = make_cleaner(vec![], home.path().to_path_buf());
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
