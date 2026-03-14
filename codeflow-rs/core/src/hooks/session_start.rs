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
    HookEvent, HookHandler, HookInput, HookOutput, SessionMeta,
};
use crate::ledger::{Event, LedgerWriter};
use crate::pathflow;
use crate::session;
use crate::types::SessionId;

// ---------------------------------------------------------------------------
// Session lock
// ---------------------------------------------------------------------------

/// Acquire an exclusive file lock on `session.lock` in `runtime_dir`.
///
/// Serializes concurrent session creation across multiple agents. The returned
/// `File` holds the lock via `flock(LOCK_EX)`. The lock is released when the
/// file is dropped.
fn acquire_session_lock(runtime_dir: &Path) -> Result<std::fs::File, std::io::Error> {
    fs::create_dir_all(runtime_dir)?;
    let lock_path = runtime_dir.join("session.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

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
pub struct SessionStartInit {
    pub ppid: u32,
    pub home_dir: PathBuf,
    pub now: NowFn,
}

#[allow(clippy::unused_self)]
impl SessionStartInit {
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

        // --- Section 0: Acquire session lock ---
        // Serialize concurrent session creation across multiple agents.
        // Lock is acquired before stale cleanup and released after env file write.
        let lock_file = match acquire_session_lock(&runtime_dir) {
            Ok(f) => Some(f),
            Err(e) => {
                result
                    .warnings
                    .push(format!("session lock: {e} (proceeding without lock)"));
                None
            }
        };

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

                // Release session lock -- teammate detected, env file already exists.
                drop(lock_file);

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

        // Release session lock -- env file written, teammates can now detect this session.
        drop(lock_file);

        // --- Section 3: Directory creation ---
        self.create_directories(project_dir, session_id.as_str());

        // --- Section 4+5: Sweep all stale sessions (replaces detect_stale_sessions + sweep_orphan_sentinels) ---
        if source == "startup" || source == "unknown" {
            self.sweep_all_stale_sessions(project_dir, session_id.as_str());
        }

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
        // Only detect stale teams on startup. On compact/resume/clear the lead's
        // context overflowed but teammates are likely still alive — destroying their
        // team config would kill an active session.
        if source == "startup" || source == "unknown" {
            let team_warnings = self.detect_stale_teams();
            result.warnings.extend(team_warnings);
        }

        // --- Section 10: Compact recovery detection ---
        self.detect_compact_recovery(project_dir, source, &mut result);

        // --- Section 11: Project temp directory ---
        self.create_project_temp_dir(project_dir, &mut result);

        // Write env JSON to stdout
        write_env_json(writer, &result.env_vars)?;

        Ok(result)
    }

    /// Handle stale session cleanup and teammate detection using multi-signal
    /// approach based on `pathflow-session-status.json` and env var.
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

        let existing_sid = env_data.session_id;

        // STEP 2a: Minimal path for compact/resume/clear.
        if source == "compact" || source == "resume" || source == "clear" {
            return (Some(existing_sid), false);
        }

        // STEP 3: Teammate detection (startup/unknown only).
        let env_sid = std::env::var("CODEFLOW_SESSION_ID").unwrap_or_default();

        if env_sid.is_empty() && existing_sid.as_str().is_empty() {
            return (None, false);
        }
        if !env_sid.is_empty() && env_sid != existing_sid.as_str() {
            return (None, false);
        }

        // Signal 2: pathflow-session-status.json
        let pathflow_dir = project_dir
            .join(".state")
            .join("session")
            .join(existing_sid.as_str())
            .join("pathflow");
        let status_path = pathflow_dir.join("pathflow-session-status.json");

        let status_data = match fs::read_to_string(&status_path) {
            Ok(data) => data,
            Err(_) => return (None, false),
        };
        let status: serde_json::Value = match serde_json::from_str(&status_data) {
            Ok(v) => v,
            Err(_) => return (None, false),
        };
        let session_status = status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if session_status.is_empty() || session_status == "pf-complete" {
            return (None, false);
        }

        let team_name = status
            .get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if team_name.is_empty() {
            return (None, false);
        }

        let config_path = self
            .home_dir
            .join(".claude")
            .join("teams")
            .join(team_name)
            .join("config.json");
        if !config_path.exists() {
            return (None, false);
        }

        // Case D: envSID matches existingSID -> teammate confirmed.
        if !env_sid.is_empty() && env_sid == existing_sid.as_str() {
            result.messages.push(format!(
                "TEAMMATE MODE: Detected via status.json + env var match (team: {team_name})"
            ));
            return (Some(existing_sid), true);
        }

        // Case B: envSID="" but active session exists -- new lead.
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

    /// Sweep ALL stale sessions using `pathflow-session-status.json`.
    fn sweep_all_stale_sessions(&self, project_dir: &Path, current_sid: &str) {
        let session_base = project_dir.join(".state").join("session");

        if let Ok(entries) = fs::read_dir(&session_base) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("ses-") || name == current_sid {
                    continue;
                }

                let status_path = session_base
                    .join(&name)
                    .join("pathflow")
                    .join("pathflow-session-status.json");

                let status_data = match fs::read_to_string(&status_path) {
                    Ok(data) => data,
                    Err(_) => {
                        self.remove_stale_session_artifacts(project_dir, &name, None);
                        continue;
                    }
                };
                let status: serde_json::Value = match serde_json::from_str(&status_data) {
                    Ok(v) => v,
                    Err(_) => {
                        self.remove_stale_session_artifacts(project_dir, &name, None);
                        continue;
                    }
                };

                let session_status = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let team_name = status.get("team_name").and_then(|v| v.as_str()).unwrap_or("");
                let age_hours = Self::status_age_hours(&status);

                match session_status {
                    "pf-complete" => {
                        let tn = if team_name.is_empty() { None } else { Some(team_name) };
                        self.remove_stale_session_artifacts(project_dir, &name, tn);
                    }
                    "created" if age_hours > 1.0 => {
                        self.remove_stale_session_artifacts(project_dir, &name, None);
                    }
                    "pf-started" | "pf-in-progress" => {
                        if team_name.is_empty() {
                            if age_hours > 1.0 {
                                self.remove_stale_session_artifacts(project_dir, &name, None);
                            }
                        } else {
                            let cfg = self.home_dir.join(".claude").join("teams").join(team_name).join("config.json");
                            if !cfg.exists() || age_hours > 24.0 {
                                self.remove_stale_session_artifacts(project_dir, &name, Some(team_name));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Sweep orphan sentinel dirs.
        let sentinel_base = project_dir.join(".state").join("sentinels").join("pathflow");
        if let Ok(entries) = fs::read_dir(&sentinel_base) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) { continue; }
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("ses-") || name == current_sid { continue; }
                if !session_base.join(&name).exists() {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    /// Calculate status file age in hours.
    fn status_age_hours(status: &serde_json::Value) -> f64 {
        let ts_str = status.get("updated_at").or_else(|| status.get("created_at"))
            .and_then(|v| v.as_str()).unwrap_or("");
        if ts_str.len() < 19 { return f64::MAX; }
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let parts: Vec<&str> = ts_str[..19].split(|c: char| !c.is_ascii_digit()).collect();
        if parts.len() < 6 { return f64::MAX; }
        let (Ok(y), Ok(mo), Ok(d), Ok(h), Ok(mi), Ok(s)) = (
            parts[0].parse::<u64>(), parts[1].parse::<u64>(), parts[2].parse::<u64>(),
            parts[3].parse::<u64>(), parts[4].parse::<u64>(), parts[5].parse::<u64>(),
        ) else { return f64::MAX; };
        let days = days_from_ymd(y, mo, d);
        let ts_secs = days * 86_400 + h * 3_600 + mi * 60 + s;
        if now_secs > ts_secs { (now_secs - ts_secs) as f64 / 3_600.0 } else { 0.0 }
    }

    /// Remove all artifacts for a stale session: session dir, sentinel dir, team config/tasks.
    fn remove_stale_session_artifacts(
        &self,
        project_dir: &Path,
        sid: &str,
        team_name: Option<&str>,
    ) {
        let _ = fs::remove_dir_all(project_dir.join(".state").join("session").join(sid));
        let _ = fs::remove_dir_all(
            project_dir
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(sid),
        );
        if let Some(name) = team_name {
            let _ = fs::remove_dir_all(self.home_dir.join(".claude").join("teams").join(name));
            let _ = fs::remove_dir_all(self.home_dir.join(".claude").join("tasks").join(name));
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

    /// Create the `pathflow-session-status.json` file. Returns `true` if the
    /// file already existed (resume scenario).
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
        let status_path = flag_dir.join("pathflow-session-status.json");

        if status_path.exists() {
            result
                .messages
                .push("PathFlow: Resuming active session (status file already exists).".into());
            return true;
        }

        let _ = fs::create_dir_all(&flag_dir);
        let content = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "created",
            "last_completed_phase": "",
            "last_completed_stage": "",
            "created_at": (self.now)(),
            "updated_at": (self.now)(),
        });
        // Atomic write: tmp file + rename to avoid partial reads.
        let tmp_path = status_path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(&content) {
            let _ = fs::write(&tmp_path, json).and_then(|()| fs::rename(&tmp_path, &status_path));
        }

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

    /// Detect stale team directories under `~/.claude/teams/` and clean them up.
    ///
    /// A team is stale when ALL its tmux pane members are dead.
    /// Returns warnings for each team that was cleaned.
    ///
    /// SAFETY: If tmux is unavailable (not installed, server not running), this
    /// function skips detection entirely. Treating tmux failure as "all panes dead"
    /// would destroy live sessions that use in-process agents (no tmux panes).
    fn detect_stale_teams(&self) -> Vec<String> {
        let teams_dir = self.home_dir.join(".claude").join("teams");
        let mut warnings = Vec::new();

        let live_panes = match list_tmux_panes() {
            Some(panes) => panes,
            None => return warnings,
        };

        if let Ok(entries) = fs::read_dir(&teams_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let team_name = entry.file_name().to_string_lossy().to_string();
                let config_path = entry.path().join("config.json");
                let Ok(data) = fs::read_to_string(&config_path) else {
                    continue;
                };
                let Ok(config) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };

                // Extract members array and count alive tmux panes.
                let members = match config.get("members").and_then(serde_json::Value::as_array) {
                    Some(m) if !m.is_empty() => m,
                    _ => continue,
                };

                let alive = members
                    .iter()
                    .filter(|m| {
                        m.get("tmuxPaneId")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|id| !id.is_empty() && live_panes.contains(&id.to_string()))
                    })
                    .count();

                // If ALL members are dead, remove the team directory and task list.
                if alive == 0 {
                    let _ = fs::remove_dir_all(entry.path());
                    let _ = fs::remove_dir_all(
                        self.home_dir.join(".claude").join("tasks").join(&team_name),
                    );
                    warnings.push(format!(
                        "STALE TEAM CLEANED: '{}' ({} members, all panes dead)",
                        team_name,
                        members.len()
                    ));
                }
            }
        }

        warnings
    }

    /// Detect compact recovery (when `source=compact/resume/clear` and active work exists).
    fn detect_compact_recovery(&self, project_dir: &Path, source: &str, result: &mut InitResult) {
        if source != "compact" && source != "resume" && source != "clear" {
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


}

impl HookHandler for SessionStartInit {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input.project_dir.as_deref().ok_or_else(|| {
            HookError::Config("project_dir required for session-start init".into())
        })?;

        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        self.run(&input, Path::new(project_dir), &mut out)?;
        out.flush().map_err(HookError::Io)?;

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

/// Hardcoded fallback instructions when config is unavailable.
/// Matches Go's `fallbackInstructions` constant in `instructions.go`.
const FALLBACK_INSTRUCTIONS: &str = "SESSION START - EXECUTE CLAUDE.md SECTION 2\n\
You MUST execute the Session Start procedure from CLAUDE.md Section 2.\n\
Check for active work (grep Status: active), present options to user, wait for choice.";

/// Instructions config JSON structure matching Go's `instructionsConfig`.
#[derive(serde::Deserialize)]
struct InstructionsConfig {
    hooks: InstructionsHooks,
}

#[derive(serde::Deserialize)]
struct InstructionsHooks {
    #[serde(rename = "SessionStart", default)]
    session_start: HashMap<String, InstructionEntry>,
}

#[derive(serde::Deserialize)]
struct InstructionEntry {
    file: Option<String>,
    enabled: Option<bool>,
}

/// Session-start instructions handler.
///
/// Outputs config-driven instructions, active task context, and `PathFlow`
/// recovery information to help the agent orient after startup or context
/// overflow. Matches Go's `RunInstructions` in `instructions.go`.
pub struct SessionStartInstructions;

impl SessionStartInstructions {
    /// Generate instruction output for the agent.
    ///
    /// Three sections matching Go:
    /// 1. Config-driven instruction loading (with fallback)
    /// 2. Active task context (with "None" fallback)
    /// 3. `PathFlow` context (sentinels, recovery)
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures when writing to the writer.
    pub fn generate(&self, project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        // Section 1: Config-driven instruction loading.
        Self::output_instructions(project_dir, writer)?;

        // Section 2: Active task context.
        Self::output_active_task(project_dir, writer)?;

        // Section 3: PathFlow context.
        Self::output_pathflow_context(project_dir, writer)?;

        Ok(())
    }

    /// Load and output instructions from config, falling back to hardcoded text.
    fn output_instructions(project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        let instructions_dir = project_dir
            .join(".codeflow")
            .join("config")
            .join("instructions");
        let config_path = instructions_dir.join("instructions-config.json");

        let Ok(data) = fs::read_to_string(&config_path) else {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        };

        let Ok(cfg) = serde_json::from_str::<InstructionsConfig>(&data) else {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        };

        if cfg.hooks.session_start.is_empty() {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        }

        let mut wrote = false;
        for entry in cfg.hooks.session_start.values() {
            let enabled = entry.enabled.unwrap_or(false);
            let file = match &entry.file {
                Some(f) if !f.is_empty() && enabled => f,
                _ => continue,
            };
            let file_path = instructions_dir.join(file);
            if let Ok(content) = fs::read_to_string(&file_path) {
                write!(writer, "{content}").map_err(HookError::Io)?;
                writeln!(writer).map_err(HookError::Io)?; // Blank line between.
                wrote = true;
            }
        }

        if !wrote {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
        }

        Ok(())
    }

    /// Output active task context or "None" fallback with register-work reminder.
    fn output_active_task(project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        let runtime_dir = project_dir.join(".state").join("runtime");

        if let Ok(Some(task)) = session::get_active_task(&runtime_dir) {
            let task_id = task
                .task_format_id
                .as_ref()
                .map_or_else(|| task.task_id.as_str(), crate::types::FormatId::as_str);

            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "ACTIVE TASKS DETECTED").map_err(HookError::Io)?;
            writeln!(writer, "=====================").map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "Incomplete tasks found:").map_err(HookError::Io)?;
            writeln!(
                writer,
                "  - {} ({})",
                task_id,
                task.status.as_deref().unwrap_or("unknown")
            )
            .map_err(HookError::Io)?;
            if let Some(title) = &task.title {
                if !title.is_empty() {
                    writeln!(writer, "    \"{title}\"").map_err(HookError::Io)?;
                }
            }
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "Options:").map_err(HookError::Io)?;
            writeln!(writer, "  1. Resume task").map_err(HookError::Io)?;
            writeln!(writer, "  2. Start new work").map_err(HookError::Io)?;
            writeln!(writer, "  3. Review tasks").map_err(HookError::Io)?;
        } else {
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "ACTIVE TASKS DETECTED: None").map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "IMPORTANT: Register work before making modifications."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "Delegate to cf-knowledge-layer teammate: \
                 SendMessage(recipient=\"cf-knowledge-layer\", \
                 content=\"ensure-work-registered\")"
            )
            .map_err(HookError::Io)?;
        }

        Ok(())
    }

    /// Output `PathFlow` context: active session, completed phases, recovery checklist.
    fn output_pathflow_context(
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<(), HookError> {
        let state_dir = project_dir.join(".state");
        let Ok(session_id) = session::current_session_id(&state_dir.join("runtime")) else {
            return Ok(());
        };

        // Check if PathFlow is active via status.json.
        let status_path = state_dir
            .join("session")
            .join(session_id.as_str())
            .join("pathflow")
            .join("pathflow-session-status.json");
        let pf_active = if let Ok(data) = fs::read_to_string(&status_path) {
            serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(String::from))
                .is_some_and(|s| !s.is_empty() && s != "pf-complete")
        } else {
            false
        };
        if !pf_active {
            return Ok(());
        }

        writeln!(writer).map_err(HookError::Io)?;
        writeln!(writer, "PATHFLOW SESSION ACTIVE").map_err(HookError::Io)?;
        writeln!(writer, "======================").map_err(HookError::Io)?;

        // List completed phase sentinels.
        let sentinel_dir = state_dir
            .join("sentinels")
            .join("pathflow")
            .join(session_id.as_str());
        let names = pathflow::sentinel::list_sentinels(&sentinel_dir).unwrap_or_default();

        let phases: Vec<&str> = names
            .iter()
            .filter(|n| n.starts_with("pf-"))
            .map(String::as_str)
            .collect();

        if phases.is_empty() {
            writeln!(writer, "No completed phases found").map_err(HookError::Io)?;
        } else {
            writeln!(writer, "Completed phases:").map_err(HookError::Io)?;
            for p in &phases {
                writeln!(writer, "  - pathflow-{p}").map_err(HookError::Io)?;
            }
        }

        writeln!(writer, "Mode: pathflow").map_err(HookError::Io)?;
        writeln!(writer, "PCV: bypassed (WS-REV provides quality assurance)")
            .map_err(HookError::Io)?;
        writeln!(writer).map_err(HookError::Io)?;

        // Compact recovery checklist (only if phase sentinels exist).
        if !phases.is_empty() {
            writeln!(
                writer,
                "COMPACT RECOVERY: Task tracker registration check required."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "Phase sentinels exist from prior context. Task tracker may be out of sync."
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "MANDATORY: Resume task tracker registration after context overflow."
            )
            .map_err(HookError::Io)?;
            writeln!(writer, "  Step 1: Read checkpoint state at .state/session/{{SID}}/pathflow/pathflow-phase-tasks.json").map_err(HookError::Io)?;
            writeln!(writer, "  Step 2: Identify current phase from sentinel files at .state/sentinels/pathflow/{{SID}}/").map_err(HookError::Io)?;
            writeln!(writer, "  Step 3: Backfill completed phases: TaskCreate then TaskUpdate to completed for each missing task").map_err(HookError::Io)?;
            writeln!(
                writer,
                "  Step 4: Register current phase tasks: TaskCreate for EVERY PF{{N}}-TSK-{{NN}}"
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "  Step 5: Verify sentinel pipeline resumes creating sentinels"
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Skipping task tracker registration after context overflow."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Clubbing multiple PF{{N}}-TSK-{{NN}} entries into a single TaskCreate."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Proceeding past a phase gate without verifying its sentinel exists."
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
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

        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        self.generate(Path::new(project_dir), &mut out)?;
        out.flush().map_err(HookError::Io)?;

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

/// List all active tmux pane IDs. Returns `None` if tmux is unavailable.
fn list_tmux_panes() -> Option<Vec<String>> {
    let output = std::process::Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_id}"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout)
        .lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Convert (year, month, day) to days since Unix epoch.
fn days_from_ymd(year: u64, month: u64, day: u64) -> u64 {
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

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
    use crate::hooks::PathflowTeamInfo;
    use crate::types::SessionId;

    fn fixed_now() -> String {
        "2026-03-10T00:00:00Z".to_string()
    }

    fn make_init(home: PathBuf) -> SessionStartInit {
        SessionStartInit {
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
        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
        let input = make_input("compact", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert_eq!(result.session_id, existing_sid);
    }

    #[test]
    fn test_init_teammate_detection_via_status_json() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let sid = SessionId::new_unchecked("ses-01jq7teammate12345678abc");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create pathflow-session-status.json with active status.
        let pathflow_dir = dir.path().join(".state").join("session").join(sid.as_str()).join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": sid.as_str(), "status": "pf-in-progress",
                "team_name": "test-team", "created_at": "2026-03-10T00:00:00Z",
                "updated_at": "2026-03-10T00:00:00Z",
            })).unwrap(),
        ).unwrap();

        // Create team config (existence check).
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), r#"{"members": []}"#).unwrap();

        let init = make_init(home.path().to_path_buf());

        // Set CODEFLOW_SESSION_ID to match (simulates teammate).
        // SAFETY: test-only, single-threaded access to env var.
        unsafe { std::env::set_var("CODEFLOW_SESSION_ID", sid.as_str()) };
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();
        // SAFETY: test-only cleanup.
        unsafe { std::env::remove_var("CODEFLOW_SESSION_ID") };

        assert!(result.is_teammate);
        assert_eq!(result.session_id, sid);
        assert!(result.messages.iter().any(|m| m.contains("TEAMMATE MODE")));
    }

    #[test]
    fn test_init_resume_flag_detection() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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
        let init = make_init(PathBuf::from("/tmp/nonexistent-home"));
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
        // Section 1: Fallback instructions (no config file in temp dir).
        assert!(output.contains("SESSION START"));
        // Section 2: Active task detected in full format.
        assert!(output.contains("ACTIVE TASKS DETECTED"));
        assert!(output.contains("INF-TSK-022-015"));
        assert!(output.contains("in_progress"));
        assert!(output.contains("Implement session hooks"));
    }

    #[test]
    fn test_instructions_no_active_task() {
        let dir = tempfile::tempdir().unwrap();

        let handler = SessionStartInstructions;
        let mut buf = Vec::new();
        handler.generate(dir.path(), &mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        // Section 1: Fallback instructions (no config file in temp dir).
        assert!(output.contains("SESSION START"));
        assert!(output.contains("CLAUDE.md"));
        // Section 2: No active task — shows "None" with register-work reminder.
        assert!(output.contains("ACTIVE TASKS DETECTED: None"));
        assert!(output.contains("ensure-work-registered"));
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

    // --- Status.json and helper tests ---

    #[test]
    fn test_days_from_ymd_roundtrip() {
        assert_eq!(days_from_ymd(1970, 1, 1), 0);
        assert_eq!(days_from_ymd(2026, 3, 10), 20_522);
    }

    #[test]
    fn test_status_age_hours_recent() {
        let now_ts = crate::util::now_rfc3339();
        let status = serde_json::json!({"updated_at": now_ts});
        let age = SessionStartInit::status_age_hours(&status);
        assert!(age < 1.0, "recent timestamp should be < 1h, got {age}");
    }

    #[test]
    fn test_detect_stale_teams_all_panes_dead_cleans_up() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");
        let tasks_dir = home.path().join(".claude").join("tasks");

        // Create a team directory with members that have dead panes.
        let team_dir = teams_dir.join("stale-team");
        fs::create_dir_all(&team_dir).unwrap();
        let task_dir = tasks_dir.join("stale-team");
        fs::create_dir_all(&task_dir).unwrap();
        let config = serde_json::json!({
            "leadPid": 99999,
            "teamName": "stale-team",
            "members": [
                {"tmuxPaneId": "%dead1"},
                {"tmuxPaneId": "%dead2"}
            ]
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        // If tmux is unavailable, detect_stale_teams skips entirely (returns []).
        // If tmux is available, dead panes are detected and team is cleaned.
        if list_tmux_panes().is_some() {
            assert!(!warnings.is_empty(), "should detect stale team with all dead panes");
            assert!(warnings[0].contains("STALE TEAM CLEANED"));
            assert!(!team_dir.exists(), "stale team directory should be removed");
            assert!(!task_dir.exists(), "stale team task list should be removed");
        }
        // If tmux unavailable, test passes silently (safe default: skip detection).
    }

    #[test]
    fn test_detect_stale_teams_some_panes_alive_not_cleaned() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team with one alive and one dead pane.
        let team_dir = teams_dir.join("alive-team");
        fs::create_dir_all(&team_dir).unwrap();
        let config = serde_json::json!({
            "leadPid": 99999,
            "teamName": "alive-team",
            "members": [
                {"tmuxPaneId": "%alive1"},
                {"tmuxPaneId": "%dead1"}
            ]
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(
            warnings.is_empty(),
            "team with at least one alive pane should not be cleaned"
        );
        assert!(team_dir.exists(), "team dir should still exist");
    }

    #[test]
    fn test_detect_stale_teams_no_config_file() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team directory without config.json.
        let team_dir = teams_dir.join("no-config-team");
        fs::create_dir_all(&team_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(warnings.is_empty(), "no config.json means no warnings");
        assert!(team_dir.exists(), "team dir without config should remain");
    }

    #[test]
    fn test_detect_stale_teams_no_teams_dir() {
        let home = tempfile::tempdir().unwrap();
        // Don't create .claude/teams/ at all.

        let init = make_init(home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(
            warnings.is_empty(),
            "missing teams dir should produce no warnings"
        );
    }

    #[test]
    fn test_detect_stale_teams_empty_members_skipped() {
        let home = tempfile::tempdir().unwrap();
        let teams_dir = home.path().join(".claude").join("teams");

        // Create a team with empty members array.
        let team_dir = teams_dir.join("empty-members-team");
        fs::create_dir_all(&team_dir).unwrap();
        let config = serde_json::json!({
            "leadPid": 99999,
            "teamName": "empty-members-team",
            "members": []
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let warnings = init.detect_stale_teams();

        assert!(
            warnings.is_empty(),
            "team with empty members should be skipped"
        );
        assert!(
            team_dir.exists(),
            "team dir with empty members should remain"
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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
    fn test_sweep_stale_sessions_cleans_pf_complete() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000000";

        let session_base = dir.path().join(".state").join("session");
        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");

        fs::create_dir_all(session_base.join(current_sid)).unwrap();

        // Create a completed session with status.json.
        let stale_sid = "ses-01jq7stale00000000000000";
        let stale_pathflow = session_base.join(stale_sid).join("pathflow");
        fs::create_dir_all(&stale_pathflow).unwrap();
        fs::write(
            stale_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"pf-complete","team_name":"stale-team","updated_at":"2026-03-10T00:00:00Z"}"#,
        ).unwrap();
        fs::create_dir_all(sentinel_base.join(stale_sid)).unwrap();
        let stale_team_dir = home.path().join(".claude").join("teams").join("stale-team");
        fs::create_dir_all(&stale_team_dir).unwrap();
        let stale_task_dir = home.path().join(".claude").join("tasks").join("stale-team");
        fs::create_dir_all(&stale_task_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(!session_base.join(stale_sid).exists(), "pf-complete session should be removed");
        assert!(!sentinel_base.join(stale_sid).exists(), "sentinel dir should be removed");
        assert!(!stale_team_dir.exists(), "team dir should be removed");
        assert!(!stale_task_dir.exists(), "task dir should be removed");
        assert!(session_base.join(current_sid).exists());
    }

    #[test]
    fn test_sweep_stale_sessions_skips_active_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000001";

        let session_base = dir.path().join(".state").join("session");

        // Create an active session with recent status.json + team config.
        let alive_sid = "ses-01jq7alive00000000000001";
        let alive_pathflow = session_base.join(alive_sid).join("pathflow");
        fs::create_dir_all(&alive_pathflow).unwrap();
        fs::write(
            alive_pathflow.join("pathflow-session-status.json"),
            serde_json::to_string(&serde_json::json!({
                "status": "pf-in-progress",
                "team_name": "alive-team",
                "updated_at": crate::util::now_rfc3339(),
            })).unwrap(),
        ).unwrap();

        // Create team config.
        let config_dir = home.path().join(".claude").join("teams").join("alive-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            session_base.join(alive_sid).exists(),
            "active session with config should not be removed"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_cleans_no_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000002";

        let session_base = dir.path().join(".state").join("session");

        // Create a session dir with no team file.
        let orphan_sid = "ses-01jq7orphan0000000000002";
        fs::create_dir_all(session_base.join(orphan_sid)).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        // Session without team file should be removed.
        assert!(
            !session_base.join(orphan_sid).exists(),
            "session without team file should be removed"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_sweeps_orphan_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000003";

        let session_base = dir.path().join(".state").join("session");
        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");

        // Create orphan sentinel dir (no matching session dir).
        let orphan_sid = "ses-01jq7orphan0000000000003";
        fs::create_dir_all(sentinel_base.join(orphan_sid)).unwrap();

        // Create sentinel dir with matching active session (status.json + config).
        let good_sid = "ses-01jq7goodsid000000000003";
        fs::create_dir_all(sentinel_base.join(good_sid)).unwrap();
        let good_pathflow = session_base.join(good_sid).join("pathflow");
        fs::create_dir_all(&good_pathflow).unwrap();
        fs::write(
            good_pathflow.join("pathflow-session-status.json"),
            serde_json::to_string(&serde_json::json!({
                "status": "pf-in-progress",
                "team_name": "good-team",
                "updated_at": crate::util::now_rfc3339(),
            })).unwrap(),
        ).unwrap();

        let config_dir = home.path().join(".claude").join("teams").join("good-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !sentinel_base.join(orphan_sid).exists(),
            "orphan sentinel dir should be removed"
        );
        assert!(
            sentinel_base.join(good_sid).exists(),
            "non-orphan sentinel dir should remain"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_skips_current_and_non_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000004";

        let session_base = dir.path().join(".state").join("session");

        // Create current session.
        fs::create_dir_all(session_base.join(current_sid)).unwrap();
        // Create a non-session directory.
        fs::create_dir_all(session_base.join("not-a-session")).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        // Neither should be removed.
        assert!(
            session_base.join(current_sid).exists(),
            "current session should not be removed"
        );
        assert!(
            session_base.join("not-a-session").exists(),
            "non-session dir should not be removed"
        );
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
            ..Default::default()
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // PID 99999 is NOT in alive_pids, so it's "dead".
        let init = make_init(home.path().to_path_buf());
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
    fn test_handle_stale_cleanup_dead_pid_compact_does_not_update_pid() {
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
            ..Default::default()
        };
        let team_file = team_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string(&team_info).unwrap()).unwrap();

        // PID 88888 is NOT alive -> dead. Source is "compact" -> should NOT update PID.
        // The caller is a surviving tmux teammate whose lead died.
        let init = make_init(home.path().to_path_buf());
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

        // Verify PID was NOT updated -- should still be the original dead PID.
        let updated_data = fs::read_to_string(&team_file).unwrap();
        let updated_info: PathflowTeamInfo = serde_json::from_str(&updated_data).unwrap();
        assert_eq!(
            updated_info.lead_pid, 88888,
            "compact should NOT update PID -- caller is a surviving teammate"
        );
    }

    #[test]
    fn test_handle_stale_cleanup_resume_returns_existing_sid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7deadresume00000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Resume always takes minimal path -- returns existing SID without mutation.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "resume", &mut result);

        assert!(existing.is_some(), "resume should return existing SID");
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode, "resume should not be team mode");
    }

    #[test]
    fn test_handle_stale_cleanup_no_team_file_with_active_flag_compact() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7noteam000000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create status file but NO team file.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7noteam000000000000"}"#,
        ).unwrap();

        let init = make_init(home.path().to_path_buf());
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
    fn test_handle_stale_cleanup_no_status_file_startup_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7orphanenv000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // No status.json -> no active session -> new lead.
        let init = make_init(home.path().to_path_buf());
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
            "no status file + startup should return None (new lead)"
        );
        assert!(!team_mode);
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let is_resume = init.create_pathflow_flag(dir.path(), sid, &mut result);
        assert!(!is_resume, "new status file should not be resume");

        // Verify status file was created.
        let status_path = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("pathflow-session-status.json");
        assert!(status_path.exists(), "status file should be created");

        // Verify content.
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();
        assert_eq!(data["status"], "created");
        assert_eq!(data["session_id"], sid);
    }

    #[test]
    fn test_create_pathflow_flag_existing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7existflag000000000000";

        // Pre-create the status file.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7existflag000000000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let is_resume = init.create_pathflow_flag(dir.path(), sid, &mut result);
        assert!(is_resume, "existing status file should indicate resume");
        assert!(result.messages.iter().any(|m| m.contains("Resuming")));
    }

    #[test]
    fn test_write_session_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7metadata00000000000";

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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

        let init = make_init(home.path().to_path_buf());
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
    fn test_resolve_session_id_non_startup_with_existing_sid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let existing_sid = SessionId::new_unchecked("ses-01jq7envfile0000000000a");
        session::write_env_file(&runtime_dir, &existing_sid, "codeflow").unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // In the new design, handle_stale_cleanup for resume always returns
        // Some(existing_sid), so resolve_or_generate_session_id gets it
        // as Priority 1 (not None).
        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "resume",
                "",
                Some(existing_sid.clone()),
                &mut result,
            )
            .unwrap();

        assert_eq!(sid, existing_sid, "resume should use existing SID from stale cleanup");
    }

    // --- Session lock tests ---

    #[test]
    fn test_acquire_session_lock_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        // Acquire first lock.
        let lock1 = acquire_session_lock(&runtime_dir).unwrap();

        // Try to acquire second lock in another thread -- should block.
        let runtime_dir2 = runtime_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            // Signal that we're about to try locking.
            tx.send("trying").unwrap();
            let _lock2 = acquire_session_lock(&runtime_dir2).unwrap();
            tx.send("acquired").unwrap();
        });

        // Wait for the thread to start trying.
        assert_eq!(rx.recv().unwrap(), "trying");

        // Give it a moment -- it should NOT have acquired yet.
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "second lock should block while first is held"
        );

        // Release first lock.
        drop(lock1);

        // Now the second lock should succeed.
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "acquired"
        );
        handle.join().unwrap();
    }

    #[test]
    fn test_acquire_session_lock_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("nonexistent").join("runtime");

        let lock = acquire_session_lock(&runtime_dir).unwrap();
        assert!(runtime_dir.join("session.lock").exists());
        drop(lock);
    }

    #[test]
    fn test_concurrent_startup_single_session_id() {
        // Simulates the real race: multiple agents start concurrently.
        // The lead (thread 0) runs first (serialized by lock), generates a
        // session, writes env file, writes pathflow-team.json (simulating
        // TeamCreate), and creates team config with tmux pane IDs.
        // Subsequent threads acquire the lock, see the env file + team file,
        // and check tmux panes to enter teammate mode -- all converging on
        // the same SID.
        use std::sync::{Arc, Barrier, Mutex};
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project_dir = Arc::new(dir.path().to_path_buf());
        let home_dir = Arc::new(home.path().to_path_buf());

        // Phase 1: Lead agent creates the session.
        // SAFETY: test-only, single-threaded at this point.
        unsafe { std::env::remove_var("CODEFLOW_SESSION_ID") };
        let lead_init = SessionStartInit {
            ppid: 1000,
            home_dir: home_dir.as_ref().clone(),
            now: fixed_now,
        };
        let lead_input = make_input("startup", project_dir.to_str().unwrap());
        let mut buf = Vec::new();
        let lead_result = lead_init.run(&lead_input, &project_dir, &mut buf).unwrap();
        let lead_sid = lead_result.session_id.clone();

        // Lead writes pathflow-session-status.json (simulates TeamCreate PostToolUse).
        let pathflow_dir = project_dir.join(".state").join("session")
            .join(lead_sid.as_str()).join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": lead_sid.as_str(), "status": "pf-in-progress",
                "team_name": "test-team", "updated_at": fixed_now(),
            })).unwrap(),
        ).unwrap();

        // Create Claude Code team config.
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), r#"{"members": []}"#).unwrap();

        // Phase 2: Multiple teammate agents start concurrently.
        // Set env var so teammates are detected via env var + status.json match.
        // SAFETY: test-only, threads haven't started yet.
        unsafe { std::env::set_var("CODEFLOW_SESSION_ID", lead_sid.as_str()) };

        let num_teammates = 4;
        let barrier = Arc::new(Barrier::new(num_teammates));
        let results = Arc::new(Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..num_teammates)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let project_dir = Arc::clone(&project_dir);
                let home_dir = Arc::clone(&home_dir);
                let results = Arc::clone(&results);

                thread::spawn(move || {
                    let init = SessionStartInit {
                        ppid: 2000,
                        home_dir: home_dir.as_ref().clone(),
                        now: fixed_now,
                    };
                    let input = make_input("startup", project_dir.to_str().unwrap());

                    // Synchronize all threads to start at once.
                    barrier.wait();

                    let mut buf = Vec::new();
                    let result = init.run(&input, &project_dir, &mut buf).unwrap();
                    results.lock().unwrap().push(result);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // SAFETY: test-only cleanup.
        unsafe { std::env::remove_var("CODEFLOW_SESSION_ID") };

        let teammate_results = results.lock().unwrap();
        // All teammates should get the same SID as the lead.
        for (i, r) in teammate_results.iter().enumerate() {
            assert_eq!(
                r.session_id,
                lead_sid,
                "teammate {i} got different SID: {} vs {lead_sid}",
                r.session_id.as_str()
            );
            assert!(r.is_teammate, "teammate {i} should be in teammate mode");
        }
    }
}
