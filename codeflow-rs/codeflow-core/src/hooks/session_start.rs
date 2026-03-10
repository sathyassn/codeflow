//! Session-start hook handlers.
//!
//! Implements `SessionStartInit`, `SessionStartInstructions`, and
//! `SessionStartLogging` as `HookHandler` implementations. These mirror
//! the Go handlers in `internal/hooks/session/start.go`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{
    HookEvent, HookHandler, HookInput, HookOutput, PathflowTeamInfo, ProcessChecker, SessionMeta,
    TmuxChecker,
};
use crate::ledger::{Event, LedgerWriter};
use crate::pathflow;
use crate::session;
use crate::types::SessionId;

// ---------------------------------------------------------------------------
// SessionStartInit
// ---------------------------------------------------------------------------

/// Output from a successful session initialization.
#[derive(Debug, Clone)]
pub struct InitResult {
    /// The `CODEFLOW_SESSION_ID` used for this session.
    pub session_id: SessionId,
    /// `true` when an existing `pathflow-active` flag was found.
    pub is_resume: bool,
    /// `true` when an existing lead PID is alive (teammate mode).
    pub is_teammate: bool,
    /// Environment variables to output for the hook framework.
    pub env_vars: HashMap<String, String>,
    /// Non-fatal warning messages.
    pub warnings: Vec<String>,
    /// Informational messages for stdout.
    pub messages: Vec<String>,
}

/// Time source for deterministic testing.
pub type NowFn = fn() -> String;

/// Session-start init handler with injectable dependencies.
///
/// Performs the 11-section initialization flow (section 12 auto-rebuild is
/// skipped in `codeflow-core`; it belongs in the CLI binary).
pub struct SessionStartInit<P: ProcessChecker, T: TmuxChecker> {
    pub process_checker: P,
    pub tmux_checker: T,
    pub ppid: u32,
    pub home_dir: PathBuf,
    pub now: NowFn,
}

#[allow(clippy::unused_self)]
impl<P: ProcessChecker, T: TmuxChecker> SessionStartInit<P, T> {
    /// Run the full session-start initialization flow.
    ///
    /// `project_dir` is the absolute path to the repository root.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures or missing required state.
    pub fn run(
        &self,
        input: &HookInput,
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<InitResult, HookError> {
        let source = input.source.as_deref().unwrap_or("unknown");
        let claude_session_id = input.session_id.as_deref().unwrap_or("");

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let runtime_dir = project_dir.join(".state").join("runtime");

        // --- Section 1: PID-based stale session cleanup + teammate detection ---
        let (existing_sid, team_mode) =
            self.handle_stale_cleanup(project_dir, &runtime_dir, source, &mut result);

        if team_mode {
            if let Some(sid) = &existing_sid {
                result.is_teammate = true;
                result.session_id = sid.clone();
                result.messages.push(format!(
                    "TEAMMATE MODE: You are a teammate joining session {}.",
                    sid.as_str()
                ));
                result
                    .env_vars
                    .insert("CODEFLOW_SESSION_ID".into(), sid.as_str().to_string());
                let project_name = project_dir
                    .file_name()
                    .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
                result
                    .env_vars
                    .insert("CF_PROJECT_ROOT".into(), project_name);

                write_env_json(writer, &result.env_vars)?;
                return Ok(result);
            }
        }

        // --- Section 2: Session ID generation (source-gated) ---
        let session_id = self.resolve_or_generate_session_id(
            project_dir,
            &runtime_dir,
            source,
            claude_session_id,
            existing_sid,
            &mut result,
        )?;
        result.session_id = session_id.clone();

        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        result.env_vars.insert(
            "CODEFLOW_SESSION_ID".into(),
            session_id.as_str().to_string(),
        );
        result
            .env_vars
            .insert("CF_PROJECT_ROOT".into(), project_name.clone());

        // --- Section 3: Directory creation ---
        self.create_directories(project_dir, session_id.as_str());

        // --- Section 4: Stale session detection (warning-only) ---
        let stale_warnings = self.detect_stale_sessions(project_dir, session_id.as_str());
        result.warnings.extend(stale_warnings);

        // --- Section 5: Orphan sentinel sweep ---
        self.sweep_orphan_sentinels(project_dir, session_id.as_str());

        // --- Section 6: Active task context expiry ---
        self.cleanup_active_task(project_dir);

        // --- Section 7: PathFlow flag creation ---
        let is_resume = self.create_pathflow_flag(project_dir, session_id.as_str(), &mut result);
        result.is_resume = is_resume;

        // --- Section 7c: Checkpoint pre-initialization ---
        self.init_checkpoint(project_dir, session_id.as_str(), &mut result);

        // --- Section 8: Session metadata ---
        self.write_session_metadata(project_dir, session_id.as_str(), source, &mut result);

        // --- Section 9: Stale team detection ---
        let team_warnings = self.detect_stale_teams();
        result.warnings.extend(team_warnings);

        // --- Section 10: Compact recovery detection ---
        self.detect_compact_recovery(project_dir, source, &mut result);

        // --- Section 11: Project temp directory ---
        self.create_project_temp_dir(project_dir, &mut result);

        // Write env JSON to stdout
        write_env_json(writer, &result.env_vars)?;

        Ok(result)
    }

    /// Handle stale session cleanup and teammate detection.
    ///
    /// Returns `(existing_session_id, is_teammate_mode)`.
    fn handle_stale_cleanup(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        source: &str,
        result: &mut InitResult,
    ) -> (Option<SessionId>, bool) {
        let env_data = match session::read_env_file(runtime_dir) {
            Ok(Some(env)) => env,
            Ok(None) => return (None, false),
            Err(e) => {
                result
                    .warnings
                    .push(format!("stale cleanup: env read error: {e}"));
                return (None, false);
            }
        };

        let old_sid = env_data.session_id;

        // Check for pathflow-team.json
        let team_file_path = project_dir
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow")
            .join("pathflow-team.json");

        if let Ok(team_data) = fs::read_to_string(&team_file_path) {
            if let Ok(team_info) = serde_json::from_str::<PathflowTeamInfo>(&team_data) {
                if team_info.lead_pid > 0 && self.process_checker.is_alive(team_info.lead_pid) {
                    // Lead is alive -- teammate mode.
                    return (Some(old_sid), true);
                }

                // Lead PID is dead.
                if source == "startup" || source == "unknown" {
                    // Fresh startup with dead PID -- full cleanup.
                    self.cleanup_stale_session(
                        project_dir,
                        runtime_dir,
                        &old_sid,
                        &team_info.team_name,
                    );
                    return (None, false);
                }

                // Compact/resume with dead PID -- update lead PID.
                self.update_lead_pid(&team_file_path);
                return (Some(old_sid), false);
            }
        }

        // No team file -- check pathflow-active flag.
        let flag_path = project_dir
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow")
            .join("is-pathflow-active");

        if !flag_path.exists() {
            // No flag either -- orphan env file, clean for startup.
            if source == "startup" || source == "unknown" {
                let _ = session::remove_env_file(runtime_dir);
                return (None, false);
            }
        }

        // Reuse existing SID for non-startup sources.
        if source != "startup" && source != "unknown" {
            return (Some(old_sid), false);
        }

        (None, false)
    }

    /// Resolve or generate a session ID based on source type.
    fn resolve_or_generate_session_id(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        source: &str,
        _claude_session_id: &str,
        existing_sid: Option<SessionId>,
        result: &mut InitResult,
    ) -> Result<SessionId, HookError> {
        // Priority 1: Use existing SID from stale cleanup.
        if let Some(sid) = existing_sid {
            return Ok(sid);
        }

        // Priority 2: Check env var.
        if let Ok(val) = std::env::var("CODEFLOW_SESSION_ID") {
            if !val.is_empty() {
                if let Ok(sid) = SessionId::new(&val) {
                    return Ok(sid);
                }
            }
        }

        // Priority 3: For startup/unknown, generate new ID.
        if source == "startup" || source == "unknown" {
            let sid = session::generate_session_id();
            // Write env file atomically.
            let project_name = project_dir
                .file_name()
                .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
            if let Err(e) = session::write_env_file(runtime_dir, &sid, &project_name) {
                result.warnings.push(format!("env file write error: {e}"));
            }
            return Ok(sid);
        }

        // Priority 4: Non-startup source -- try env file.
        if let Ok(Some(env)) = session::read_env_file(runtime_dir) {
            return Ok(env.session_id);
        }

        Err(HookError::Config(format!(
            "source={source} requires existing session but none found"
        )))
    }

    /// Create required session directories.
    fn create_directories(&self, project_dir: &Path, session_id: &str) {
        let dirs = [
            project_dir.join(".state").join("runtime"),
            project_dir.join(".state").join("ledger"),
            project_dir
                .join(".state")
                .join("session")
                .join(session_id)
                .join("pathflow"),
            project_dir
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(session_id),
            project_dir.join(".state").join("logs"),
        ];
        for dir in &dirs {
            let _ = fs::create_dir_all(dir);
        }
    }

    /// Detect stale sessions with active sentinels (warning-only).
    fn detect_stale_sessions(&self, project_dir: &Path, current_sid: &str) -> Vec<String> {
        let sentinel_base = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow");
        let mut warnings = Vec::new();

        if let Ok(entries) = fs::read_dir(&sentinel_base) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name != current_sid {
                        warnings.push(format!("stale sentinel directory: {name}"));
                    }
                }
            }
        }

        warnings
    }

    /// Sweep orphan sentinel directories from previous sessions.
    fn sweep_orphan_sentinels(&self, project_dir: &Path, current_sid: &str) {
        let sentinel_base = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow");

        if let Ok(entries) = fs::read_dir(&sentinel_base) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name != current_sid {
                        // Check if the session's pathflow-active flag is gone.
                        let flag = project_dir
                            .join(".state")
                            .join("session")
                            .join(&name)
                            .join("pathflow")
                            .join("is-pathflow-active");
                        if !flag.exists() {
                            let _ = fs::remove_dir_all(entry.path());
                        }
                    }
                }
            }
        }
    }

    /// Clean up expired active task context.
    fn cleanup_active_task(&self, project_dir: &Path) {
        let runtime_dir = project_dir.join(".state").join("runtime");
        if let Ok(Some(task)) = session::get_active_task(&runtime_dir) {
            // If the task is complete or cancelled, remove it.
            if let Some(status) = &task.status {
                if status == "complete" || status == "cancelled" {
                    let _ = session::clear_active_task(&runtime_dir);
                }
            }
        }
    }

    /// Create the `is-pathflow-active` flag file. Returns `true` if the flag
    /// already existed (resume scenario).
    fn create_pathflow_flag(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut InitResult,
    ) -> bool {
        let flag_dir = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow");
        let flag_path = flag_dir.join("is-pathflow-active");

        if flag_path.exists() {
            result
                .messages
                .push("PathFlow: Resuming active session (flag already exists).".into());
            return true;
        }

        let _ = fs::create_dir_all(&flag_dir);
        let content = serde_json::json!({
            "session_id": session_id,
            "created_at": (self.now)(),
            "ppid": self.ppid,
        });
        let _ = fs::write(
            &flag_path,
            serde_json::to_string_pretty(&content).unwrap_or_default(),
        );

        false
    }

    /// Initialize the `PathFlow` checkpoint from `pathflow-config.json`.
    fn init_checkpoint(&self, project_dir: &Path, session_id: &str, result: &mut InitResult) {
        let config_path = project_dir
            .join(".codeflow")
            .join("config")
            .join("pathflow")
            .join("pathflow-config.json");

        if !config_path.exists() {
            result
                .warnings
                .push("pathflow-config.json not found, skipping checkpoint init".into());
            return;
        }

        let checkpoint_path = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json");

        let cp = pathflow::checkpoint::Checkpoint::new();
        if let Err(e) = cp.init_all_phases(&checkpoint_path, &config_path) {
            result.warnings.push(format!("checkpoint init error: {e}"));
        }
    }

    /// Write session metadata file.
    fn write_session_metadata(
        &self,
        project_dir: &Path,
        session_id: &str,
        source: &str,
        result: &mut InitResult,
    ) {
        let meta = SessionMeta {
            session_id: session_id.to_string(),
            created_at: (self.now)(),
            source: source.to_string(),
            ppid: self.ppid,
            version: "rust-dev".to_string(),
            permission_mode: None,
        };

        let meta_dir = project_dir.join(".state").join("session").join(session_id);
        let _ = fs::create_dir_all(&meta_dir);
        let meta_path = meta_dir.join("session-meta.json");

        match serde_json::to_string_pretty(&meta) {
            Ok(json) => {
                if let Err(e) = fs::write(&meta_path, format!("{json}\n")) {
                    result.warnings.push(format!("metadata write error: {e}"));
                }
            }
            Err(e) => result
                .warnings
                .push(format!("metadata serialize error: {e}")),
        }
    }

    /// Detect stale team directories under `~/.claude/teams/`.
    fn detect_stale_teams(&self) -> Vec<String> {
        let teams_dir = self.home_dir.join(".claude").join("teams");
        let mut warnings = Vec::new();

        if let Ok(entries) = fs::read_dir(&teams_dir) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let config_path = entry.path().join("config.json");
                    if config_path.exists() {
                        // Check if the team config references a dead PID.
                        if let Ok(data) = fs::read_to_string(&config_path) {
                            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&data) {
                                if let Some(pid) =
                                    config.get("leadPid").and_then(serde_json::Value::as_u64)
                                {
                                    #[allow(clippy::cast_possible_truncation)]
                                    let pid_u32 = pid as u32;
                                    if !self.process_checker.is_alive(pid_u32) {
                                        warnings.push(format!(
                                            "stale team: {} (lead PID {} dead)",
                                            entry.file_name().to_string_lossy(),
                                            pid
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        warnings
    }

    /// Detect compact recovery (when `source=compact` and active work exists).
    fn detect_compact_recovery(&self, project_dir: &Path, source: &str, result: &mut InitResult) {
        if source != "compact" {
            return;
        }

        let runtime_dir = project_dir.join(".state").join("runtime");
        if let Ok(Some(task)) = session::get_active_task(&runtime_dir) {
            result.messages.push(format!(
                "COMPACT RECOVERY: Active task {} detected. Context was compacted.",
                task.task_id.as_str()
            ));
        }
    }

    /// Create the project-scoped temp directory.
    fn create_project_temp_dir(&self, project_dir: &Path, _result: &mut InitResult) {
        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());

        let tmp_dir = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("managed");
        let _ = fs::create_dir_all(&tmp_dir);
    }

    /// Clean up a stale session (full cleanup for startup with dead lead PID).
    fn cleanup_stale_session(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        old_sid: &SessionId,
        team_name: &str,
    ) {
        // Remove sentinel directory.
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(old_sid.as_str());
        let _ = fs::remove_dir_all(&sentinel_dir);

        // Remove session state directory.
        let session_dir = project_dir
            .join(".state")
            .join("session")
            .join(old_sid.as_str());
        let _ = fs::remove_dir_all(&session_dir);

        // Clean team artifacts if present.
        if !team_name.is_empty() {
            let teams_dir = self.home_dir.join(".claude").join("teams").join(team_name);
            let _ = fs::remove_dir_all(&teams_dir);
            let tasks_dir = self.home_dir.join(".claude").join("tasks").join(team_name);
            let _ = fs::remove_dir_all(&tasks_dir);
        }

        // Remove env file.
        let _ = session::remove_env_file(runtime_dir);
    }

    /// Update the lead PID in `pathflow-team.json` to the current PPID.
    fn update_lead_pid(&self, team_file_path: &Path) {
        if let Ok(data) = fs::read_to_string(team_file_path) {
            if let Ok(mut info) = serde_json::from_str::<PathflowTeamInfo>(&data) {
                info.lead_pid = self.ppid;
                if let Ok(json) = serde_json::to_string_pretty(&info) {
                    let _ = fs::write(team_file_path, format!("{json}\n"));
                }
            }
        }
    }
}

impl<P: ProcessChecker, T: TmuxChecker> HookHandler for SessionStartInit<P, T> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input.project_dir.as_deref().ok_or_else(|| {
            HookError::Config("project_dir required for session-start init".into())
        })?;

        let mut buf = Vec::new();
        self.run(&input, Path::new(project_dir), &mut buf)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-init"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// SessionStartInstructions
// ---------------------------------------------------------------------------

/// Session-start instructions handler.
///
/// Outputs active task context and `PathFlow` recovery information to help the
/// agent orient after startup or context overflow.
pub struct SessionStartInstructions;

impl SessionStartInstructions {
    /// Generate instruction output for the agent.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures when writing to the writer.
    pub fn generate(&self, project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        let runtime_dir = project_dir.join(".state").join("runtime");

        // Output active task context.
        if let Ok(Some(task)) = session::get_active_task(&runtime_dir) {
            writeln!(
                writer,
                "Active task: {} ({})",
                task.task_format_id
                    .as_ref()
                    .map_or("unknown", crate::types::FormatId::as_str),
                task.title.as_deref().unwrap_or("untitled")
            )
            .map_err(HookError::Io)?;

            if let Some(stage) = &task.current_stage {
                writeln!(writer, "Current stage: {stage}").map_err(HookError::Io)?;
            }
            if let Some(branch) = &task.branch {
                writeln!(writer, "Branch: {branch}").map_err(HookError::Io)?;
            }
        }

        // Output PathFlow recovery info.
        let state_dir = project_dir.join(".state");
        if let Ok(sid) = session::current_session_id(&state_dir.join("runtime")) {
            let sentinel_dir = pathflow::sentinel::resolve_dir(project_dir, sid.as_str());
            if let Ok(sentinel_dir) = sentinel_dir {
                if let Ok(sentinels) = pathflow::sentinel::list_sentinels(&sentinel_dir) {
                    if !sentinels.is_empty() {
                        writeln!(writer, "PathFlow sentinels: {}", sentinels.join(", "))
                            .map_err(HookError::Io)?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl HookHandler for SessionStartInstructions {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input
            .project_dir
            .as_deref()
            .ok_or_else(|| HookError::Config("project_dir required for instructions".into()))?;

        let mut buf = Vec::new();
        self.generate(Path::new(project_dir), &mut buf)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-instructions"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// SessionStartLogging
// ---------------------------------------------------------------------------

/// Session-start logging handler.
///
/// Captures `permission_mode` and writes a `session_start` event to the
/// sessions JSONL ledger.
pub struct SessionStartLogging<L: LedgerWriter> {
    pub ledger: L,
    pub now: NowFn,
}

impl<L: LedgerWriter> SessionStartLogging<L> {
    /// Write the `session_start` ledger event.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on ledger write failure.
    pub fn log_start(
        &self,
        session_id: &str,
        source: &str,
        permission_mode: Option<&str>,
    ) -> Result<(), HookError> {
        let mut data = HashMap::new();
        data.insert(
            "source".into(),
            serde_json::Value::String(source.to_string()),
        );
        if let Some(mode) = permission_mode {
            data.insert(
                "permission_mode".into(),
                serde_json::Value::String(mode.to_string()),
            );
        }

        let event = Event {
            event_type: "session_start".into(),
            timestamp: (self.now)(),
            session_id: Some(session_id.to_string()),
            data,
        };

        self.ledger
            .append_event(event)
            .map_err(|e| HookError::Config(format!("ledger write error: {e}")))?;

        Ok(())
    }
}

impl<L: LedgerWriter> HookHandler for SessionStartLogging<L> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let session_id = input.session_id.as_deref().unwrap_or("unknown");
        let source = input.source.as_deref().unwrap_or("unknown");

        self.log_start(session_id, source, None)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write the env JSON output to the writer.
///
/// Format: `{"env": {"CODEFLOW_SESSION_ID": "...", "CF_PROJECT_ROOT": "..."}}`
fn write_env_json(
    writer: &mut dyn Write,
    env_vars: &HashMap<String, String>,
) -> Result<(), HookError> {
    let output = serde_json::json!({ "env": env_vars });
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&output).unwrap_or_default()
    )
    .map_err(HookError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{ProcessChecker, TmuxChecker};
    use crate::types::SessionId;

    struct MockProcessChecker {
        alive_pids: Vec<u32>,
    }

    impl ProcessChecker for MockProcessChecker {
        fn is_alive(&self, pid: u32) -> bool {
            self.alive_pids.contains(&pid)
        }
    }

    struct MockTmuxChecker;

    impl TmuxChecker for MockTmuxChecker {
        fn is_pane_alive(&self, _pane_id: &str) -> bool {
            false
        }

        fn list_panes(&self) -> Result<Vec<String>, HookError> {
            Ok(Vec::new())
        }
    }

    fn fixed_now() -> String {
        "2026-03-10T00:00:00Z".to_string()
    }

    fn make_init(
        alive_pids: Vec<u32>,
        home: PathBuf,
    ) -> SessionStartInit<MockProcessChecker, MockTmuxChecker> {
        SessionStartInit {
            process_checker: MockProcessChecker { alive_pids },
            tmux_checker: MockTmuxChecker,
            ppid: 1000,
            home_dir: home,
            now: fixed_now,
        }
    }

    fn make_input(source: &str, project_dir: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("claude-uuid-123".into()),
            project_dir: Some(project_dir.into()),
            source: Some(source.into()),
            transcript_path: None,
        }
    }

    // --- SessionStartInit tests ---

    #[test]
    fn test_init_startup_generates_new_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(vec![], home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.session_id.as_str().starts_with("ses-"));
        assert!(!result.is_resume);
        assert!(!result.is_teammate);
        assert!(result.env_vars.contains_key("CODEFLOW_SESSION_ID"));
        assert!(result.env_vars.contains_key("CF_PROJECT_ROOT"));

        // Verify env JSON was written to stdout
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("CODEFLOW_SESSION_ID"));
    }

    #[test]
    fn test_init_compact_reuses_existing_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        // Write an existing env file.
        let existing_sid = SessionId::new_unchecked("ses-01jq7existing123456789ab");
        session::write_env_file(&runtime_dir, &existing_sid, "codeflow").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let input = make_input("compact", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert_eq!(result.session_id, existing_sid);
    }

    #[test]
    fn test_init_teammate_detection() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let sid = SessionId::new_unchecked("ses-01jq7teammate12345678abc");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create pathflow-team.json with alive lead PID.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&team_dir).unwrap();
        let team_info = PathflowTeamInfo {
            lead_pid: 42,
            team_name: "test-team".into(),
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // PID 42 is "alive" in our mock.
        let init = make_init(vec![42], home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.is_teammate);
        assert_eq!(result.session_id, sid);
        assert!(result.messages.iter().any(|m| m.contains("TEAMMATE MODE")));
    }

    #[test]
    fn test_init_resume_flag_detection() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        // First run: fresh start.
        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();
        let sid = result.session_id.clone();
        assert!(!result.is_resume);

        // Manually set up env file for the second run (to reuse same SID).
        let runtime_dir = dir.path().join(".state").join("runtime");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Second run with compact: flag already exists -> resume.
        let input2 = make_input("compact", dir.path().to_str().unwrap());
        let mut buf2 = Vec::new();
        let result2 = init.run(&input2, dir.path(), &mut buf2).unwrap();
        assert!(result2.is_resume);
    }

    #[test]
    fn test_init_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let sid = result.session_id.as_str();
        assert!(dir.path().join(".state").join("runtime").exists());
        assert!(dir.path().join(".state").join("ledger").exists());
        assert!(dir.path().join(".state").join("logs").exists());
        assert!(
            dir.path()
                .join(".state")
                .join("session")
                .join(sid)
                .join("pathflow")
                .exists()
        );
        assert!(
            dir.path()
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(sid)
                .exists()
        );
    }

    #[test]
    fn test_init_error_on_missing_project_dir() {
        let init = make_init(vec![], PathBuf::from("/tmp/nonexistent-home"));
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: None,
            project_dir: None,
            source: Some("startup".into()),
            transcript_path: None,
        };
        let result = init.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    // --- SessionStartInstructions tests ---

    #[test]
    fn test_instructions_with_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01abc"),
            epic_id: None,
            task_format_id: Some(crate::types::FormatId::new_unchecked("INF-TSK-022-015")),
            epic_format_id: None,
            title: Some("Implement session hooks".into()),
            status: Some("in_progress".into()),
            branch: Some("feat/rust-session-hooks".into()),
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: Some("WS-DEV".into()),
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let handler = SessionStartInstructions;
        let mut buf = Vec::new();
        handler.generate(dir.path(), &mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("INF-TSK-022-015"));
        assert!(output.contains("Implement session hooks"));
        assert!(output.contains("WS-DEV"));
        assert!(output.contains("feat/rust-session-hooks"));
    }

    #[test]
    fn test_instructions_no_active_task() {
        let dir = tempfile::tempdir().unwrap();

        let handler = SessionStartInstructions;
        let mut buf = Vec::new();
        handler.generate(dir.path(), &mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn test_instructions_handler_trait() {
        let handler = SessionStartInstructions;
        assert_eq!(handler.name(), "session-start-instructions");
        assert_eq!(handler.events(), &[HookEvent::SessionStart]);
    }

    // --- SessionStartLogging tests ---

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
    fn test_logging_writes_session_start_event() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        handler
            .log_start("ses-test123", "startup", Some("default"))
            .unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert_eq!(event.event_type, "session_start");
        assert_eq!(event.session_id.as_deref(), Some("ses-test123"));
        assert_eq!(event.timestamp, "2026-03-10T00:00:00Z");
        assert_eq!(
            event.data.get("source"),
            Some(&serde_json::Value::String("startup".into()))
        );
        assert_eq!(
            event.data.get("permission_mode"),
            Some(&serde_json::Value::String("default".into()))
        );
    }

    #[test]
    fn test_logging_without_permission_mode() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        handler.log_start("ses-test", "compact", None).unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert!(!event.data.contains_key("permission_mode"));
    }

    #[test]
    fn test_logging_handler_trait() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        assert_eq!(handler.name(), "session-start-logging");
        assert_eq!(handler.events(), &[HookEvent::SessionStart]);

        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("ses-logging-test".into()),
            project_dir: None,
            source: Some("startup".into()),
            transcript_path: None,
        };

        let output = handler.handle(input).unwrap();
        assert_eq!(output.exit_code(), 0);
    }

    // --- write_env_json tests ---

    #[test]
    fn test_write_env_json_format() {
        let mut env = HashMap::new();
        env.insert("CODEFLOW_SESSION_ID".into(), "ses-abc".into());
        env.insert("CF_PROJECT_ROOT".into(), "myproject".into());

        let mut buf = Vec::new();
        write_env_json(&mut buf, &env).unwrap();

        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(
            parsed["env"]["CODEFLOW_SESSION_ID"],
            serde_json::Value::String("ses-abc".into())
        );
    }

    // --- Coverage tests for uncovered paths ---

    #[test]
    fn test_cleanup_stale_session_removes_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7stale00000000000000");

        // Set up env file.
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create sentinel directory for the old session.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(old_sid.as_str());
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        // Create session state directory.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("is-pathflow-active"), "{}").unwrap();

        // Create team artifacts.
        let team_name = "test-stale-team";
        let teams_dir = home.path().join(".claude").join("teams").join(team_name);
        fs::create_dir_all(&teams_dir).unwrap();
        fs::write(teams_dir.join("config.json"), "{}").unwrap();
        let tasks_dir = home.path().join(".claude").join("tasks").join(team_name);
        fs::create_dir_all(&tasks_dir).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());

        // Call cleanup_stale_session directly.
        init.cleanup_stale_session(dir.path(), &runtime_dir, &old_sid, team_name);

        // Verify sentinel directory removed.
        assert!(!sentinel_dir.exists(), "sentinel dir should be removed");
        // Verify session state directory removed.
        assert!(
            !dir.path()
                .join(".state")
                .join("session")
                .join(old_sid.as_str())
                .exists(),
            "session dir should be removed"
        );
        // Verify team artifacts removed.
        assert!(!teams_dir.exists(), "teams dir should be removed");
        assert!(!tasks_dir.exists(), "tasks dir should be removed");
        // Verify env file removed.
        assert!(
            session::read_env_file(&runtime_dir).unwrap().is_none(),
            "env file should be removed"
        );
    }

    #[test]
    fn test_cleanup_stale_session_empty_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7stale00000000000001");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());

        // Call with empty team name -- should not attempt team cleanup.
        init.cleanup_stale_session(dir.path(), &runtime_dir, &old_sid, "");

        // Env file should still be removed.
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());
    }

    #[test]
    fn test_update_lead_pid_writes_new_pid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let team_file = dir.path().join("pathflow-team.json");

        let initial = PathflowTeamInfo {
            lead_pid: 999,
            team_name: "my-team".into(),
        };
        fs::write(&team_file, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.update_lead_pid(&team_file);

        // Read back and verify PID was updated to init.ppid (1000).
        let data = fs::read_to_string(&team_file).unwrap();
        let info: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert_eq!(info.lead_pid, 1000);
        assert_eq!(info.team_name, "my-team");
    }

    #[test]
    fn test_update_lead_pid_nonexistent_file() {
        let home = tempfile::tempdir().unwrap();
        let init = make_init(vec![], home.path().to_path_buf());

        // Should not panic on nonexistent file.
        init.update_lead_pid(Path::new("/tmp/nonexistent-team-file.json"));
    }

    #[test]
    fn test_detect_stale_teams_with_dead_pid() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team directory with a dead lead PID.
        let team_dir = teams_dir.join("stale-team");
        fs::create_dir_all(&team_dir).unwrap();
        let config = serde_json::json!({
            "leadPid": 4294967295_u64,
            "teamName": "stale-team"
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(
            !warnings.is_empty(),
            "should detect stale team with dead PID"
        );
        assert!(warnings[0].contains("stale team: stale-team"));
        assert!(warnings[0].contains("dead"));
    }

    #[test]
    fn test_detect_stale_teams_with_alive_pid() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team directory with our own PID (alive).
        let my_pid = std::process::id();
        let team_dir = teams_dir.join("alive-team");
        fs::create_dir_all(&team_dir).unwrap();
        let config = serde_json::json!({
            "leadPid": my_pid,
            "teamName": "alive-team"
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let init = make_init(vec![my_pid], home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(warnings.is_empty(), "should not flag team with alive PID");
    }

    #[test]
    fn test_detect_stale_teams_no_config_file() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team directory without config.json.
        let team_dir = teams_dir.join("no-config-team");
        fs::create_dir_all(&team_dir).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(warnings.is_empty(), "no config.json means no warnings");
    }

    #[test]
    fn test_detect_stale_teams_no_teams_dir() {
        let home = tempfile::tempdir().unwrap();
        // Don't create .claude/teams/ at all.

        let init = make_init(vec![], home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(
            warnings.is_empty(),
            "missing teams dir should produce no warnings"
        );
    }

    #[test]
    fn test_detect_compact_recovery_with_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        // Set up an active task.
        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01compact"),
            epic_id: None,
            task_format_id: Some(crate::types::FormatId::new_unchecked("INF-TSK-022-015")),
            epic_format_id: None,
            title: Some("Test task".into()),
            status: Some("in_progress".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.detect_compact_recovery(dir.path(), "compact", &mut result);

        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("COMPACT RECOVERY")),
            "should detect compact recovery with active task"
        );
    }

    #[test]
    fn test_detect_compact_recovery_non_compact_source() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // source != "compact" should be a no-op.
        init.detect_compact_recovery(dir.path(), "startup", &mut result);

        assert!(
            result.messages.is_empty(),
            "non-compact source should not add messages"
        );
    }

    #[test]
    fn test_detect_compact_recovery_no_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // compact source but no active task.
        init.detect_compact_recovery(dir.path(), "compact", &mut result);

        assert!(
            result.messages.is_empty(),
            "compact with no active task should not add messages"
        );
    }

    #[test]
    fn test_sweep_orphan_sentinels_removes_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000000";

        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");

        // Create current session sentinel dir.
        fs::create_dir_all(sentinel_base.join(current_sid)).unwrap();

        // Create orphan sentinel dir (no pathflow-active flag).
        let orphan_sid = "ses-01jq7orphan0000000000000";
        fs::create_dir_all(sentinel_base.join(orphan_sid)).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.sweep_orphan_sentinels(dir.path(), current_sid);

        // Current session sentinel should still exist.
        assert!(sentinel_base.join(current_sid).exists());
        // Orphan sentinel should be removed.
        assert!(
            !sentinel_base.join(orphan_sid).exists(),
            "orphan sentinel dir should be removed"
        );
    }

    #[test]
    fn test_sweep_orphan_sentinels_preserves_active_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000001";

        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");
        fs::create_dir_all(sentinel_base.join(current_sid)).unwrap();

        // Create another session's sentinel dir WITH an active flag.
        let other_sid = "ses-01jq7other00000000000000";
        fs::create_dir_all(sentinel_base.join(other_sid)).unwrap();
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(other_sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(flag_dir.join("is-pathflow-active"), "{}").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.sweep_orphan_sentinels(dir.path(), current_sid);

        // Both should still exist (other has active flag).
        assert!(sentinel_base.join(current_sid).exists());
        assert!(
            sentinel_base.join(other_sid).exists(),
            "session with active flag should not be swept"
        );
    }

    #[test]
    fn test_detect_stale_sessions_finds_stale_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000002";

        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");
        fs::create_dir_all(sentinel_base.join(current_sid)).unwrap();
        fs::create_dir_all(sentinel_base.join("ses-01jq7stale00000000000002")).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let warnings = init.detect_stale_sessions(dir.path(), current_sid);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("stale sentinel directory"));
    }

    #[test]
    fn test_detect_stale_sessions_no_stale() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000003";

        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");
        fs::create_dir_all(sentinel_base.join(current_sid)).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let warnings = init.detect_stale_sessions(dir.path(), current_sid);

        assert!(warnings.is_empty());
    }

    #[test]
    fn test_handle_stale_cleanup_dead_pid_startup_triggers_full_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7deadpid00000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create pathflow-team.json with a dead PID.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&team_dir).unwrap();
        let team_info = PathflowTeamInfo {
            lead_pid: 99999,
            team_name: "dead-team".into(),
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // PID 99999 is NOT in alive_pids, so it's "dead".
        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "startup", &mut result);

        assert!(existing.is_none(), "dead PID + startup should return None");
        assert!(!team_mode, "dead PID should not be team mode");
    }

    #[test]
    fn test_handle_stale_cleanup_dead_pid_compact_updates_pid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7deadcompact0000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create pathflow-team.json with a dead PID.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&team_dir).unwrap();
        let team_info = PathflowTeamInfo {
            lead_pid: 88888,
            team_name: "compact-team".into(),
        };
        let team_file = team_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string(&team_info).unwrap()).unwrap();

        // PID 88888 is NOT alive -> dead. Source is "compact" -> should update PID.
        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "compact", &mut result);

        assert!(
            existing.is_some(),
            "compact with dead PID should return existing SID"
        );
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode, "dead PID should not be team mode");

        // Verify PID was updated in the file.
        let updated_data = fs::read_to_string(&team_file).unwrap();
        let updated_info: PathflowTeamInfo = serde_json::from_str(&updated_data).unwrap();
        assert_eq!(updated_info.lead_pid, 1000, "PID should be updated to ppid");
    }

    #[test]
    fn test_handle_stale_cleanup_no_team_file_with_active_flag_compact() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7noteam000000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create pathflow-active flag but NO team file.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(flag_dir.join("is-pathflow-active"), "{}").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // compact source with active flag but no team file -> reuse SID.
        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "compact", &mut result);

        assert!(
            existing.is_some(),
            "compact with active flag should reuse SID"
        );
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode);
    }

    #[test]
    fn test_handle_stale_cleanup_no_team_file_no_flag_startup_cleans_env() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7orphanenv000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // No team file, no pathflow-active flag -> orphan env file.
        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "startup", &mut result);

        assert!(
            existing.is_none(),
            "orphan env + startup should return None"
        );
        assert!(!team_mode);
        // Env file should be removed.
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());
    }

    #[test]
    fn test_cleanup_active_task_removes_completed() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01complete"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Done task".into()),
            status: Some("complete".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_none(),
            "completed task should be cleared"
        );
    }

    #[test]
    fn test_cleanup_active_task_preserves_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01active"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Active task".into()),
            status: Some("in_progress".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_some(),
            "in_progress task should be preserved"
        );
    }

    #[test]
    fn test_cleanup_active_task_removes_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01cancel"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Cancelled task".into()),
            status: Some("cancelled".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_none(),
            "cancelled task should be cleared"
        );
    }

    #[test]
    fn test_init_checkpoint_missing_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.init_checkpoint(dir.path(), "ses-test", &mut result);

        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("pathflow-config.json not found")),
            "should warn about missing config"
        );
    }

    #[test]
    fn test_create_pathflow_flag_new() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7newflag0000000000000";

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let is_resume = init.create_pathflow_flag(dir.path(), sid, &mut result);
        assert!(!is_resume, "new flag should not be resume");

        // Verify flag file was created.
        let flag_path = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("is-pathflow-active");
        assert!(flag_path.exists(), "flag file should be created");
    }

    #[test]
    fn test_create_pathflow_flag_existing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7existflag000000000000";

        // Pre-create the flag.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(flag_dir.join("is-pathflow-active"), "{}").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let is_resume = init.create_pathflow_flag(dir.path(), sid, &mut result);
        assert!(is_resume, "existing flag should indicate resume");
        assert!(result.messages.iter().any(|m| m.contains("Resuming")));
    }

    #[test]
    fn test_write_session_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7metadata00000000000";

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.write_session_metadata(dir.path(), sid, "startup", &mut result);

        let meta_path = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("session-meta.json");
        assert!(meta_path.exists(), "metadata file should be created");

        let data = fs::read_to_string(&meta_path).unwrap();
        let meta: SessionMeta = serde_json::from_str(&data).unwrap();
        assert_eq!(meta.session_id, sid);
        assert_eq!(meta.source, "startup");
        assert_eq!(meta.ppid, 1000);
        assert_eq!(meta.version, "rust-dev");
        assert!(result.warnings.is_empty(), "no warnings expected");
    }

    #[test]
    fn test_create_project_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.create_project_temp_dir(dir.path(), &mut result);

        let project_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let expected = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("managed");
        assert!(expected.exists(), "project temp dir should be created");

        // Cleanup.
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    #[test]
    fn test_instructions_handler_error_on_missing_project_dir() {
        let handler = SessionStartInstructions;
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    #[test]
    fn test_resolve_session_id_from_existing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let existing = SessionId::new_unchecked("ses-01jq7exist000000000000a");
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "startup",
                "",
                Some(existing.clone()),
                &mut result,
            )
            .unwrap();

        assert_eq!(sid, existing, "should use existing SID when provided");
    }

    #[test]
    fn test_resolve_session_id_non_startup_no_env_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let err = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "resume",
                "",
                None,
                &mut result,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("requires existing session"),
            "resume with no env file should error"
        );
    }

    #[test]
    fn test_resolve_session_id_non_startup_with_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let existing_sid = SessionId::new_unchecked("ses-01jq7envfile0000000000a");
        session::write_env_file(&runtime_dir, &existing_sid, "codeflow").unwrap();

        let init = make_init(vec![], home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "resume",
                "",
                None,
                &mut result,
            )
            .unwrap();

        assert_eq!(sid, existing_sid, "resume should use env file SID");
    }
}
