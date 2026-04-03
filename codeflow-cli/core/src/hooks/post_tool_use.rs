//! `PostToolUse` hook handlers.
//!
//! Four handler types that run after tool invocations:
//! - `SentinelWrite`: Creates stage sentinels on `STAGE-COMPLETE` messages,
//!   handles `TeamCreate`/`Task`/`Agent`/`TeamDelete` for pathflow-team.json
//! - `CheckpointRegister`: Registers `PF{N}-TSK-{NN}` tasks in checkpoint
//! - `SettingsValidate`: Validates settings.json consistency
//!
//! Note: `PostToolUse` logging was moved to `hooks::logging::ToolUseLogging`
//! for full-featured tool-use logging with redaction, truncation, and config.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use super::{BlockCategory, HookEvent, HookHandler, HookInput, HookOutput, PathflowTeamInfo};
use crate::error::HookError;
use crate::pathflow::{checkpoint::Checkpoint, sentinel};
use crate::session;

// ---------------------------------------------------------------------------
// Shared regex patterns (compiled once)
// ---------------------------------------------------------------------------

fn stage_complete_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"STAGE-COMPLETE:\s+WS-(DEV|REV|QA|TEST|PLAN|DOCS)").expect("valid regex")
    })
}

fn pf_task_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"PF\d+-TSK-\d+").expect("valid regex"))
}

/// Primary work stages that must complete before `WS-REV`.
const PRIMARY_STAGES: &[&str] = &["dev", "plan", "docs", "test"];

// ---------------------------------------------------------------------------
// SentinelWrite handler
// ---------------------------------------------------------------------------

/// Creates stage sentinels when `STAGE-COMPLETE: WS-{STAGE}` appears in
/// `SendMessage` content. Also handles `TeamCreate`, `Task` (teammate spawn),
/// and `TeamDelete` events for `pathflow-team.json` management.
///
/// Validates stage ordering (REV needs a primary stage sentinel, QA needs
/// `ws-dev` or `ws-test`).
pub struct SentinelWrite {
    /// Project root directory for resolving sentinel paths.
    pub project_dir: PathBuf,
}

impl SentinelWrite {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    fn sentinel_dir(&self) -> Result<PathBuf, HookError> {
        let sid = session::current_session_id(&self.project_dir)
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;
        sentinel::resolve_dir(&self.project_dir, sid.as_ref())
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }

    fn session_pathflow_dir(&self) -> Result<(PathBuf, String), HookError> {
        let sid = session::current_session_id(&self.project_dir)
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;

        // self.project_dir is already worktree-aware (resolved by detect_project_dir),
        // so we can construct the pathflow dir directly without env var checks.
        let dir = self
            .project_dir
            .join(".state")
            .join("session")
            .join(sid.as_ref())
            .join("pathflow");
        Ok((dir, sid.as_str().to_string()))
    }

    fn handle_send_message(&self, input: &HookInput) -> Result<HookOutput, HookError> {
        let content = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if content.is_empty() {
            return Ok(HookOutput::Allow);
        }

        // Normalize: uppercase + collapse whitespace.
        let normalized = collapse_whitespace(&content.to_uppercase());

        let Some(captures) = stage_complete_re().captures(&normalized) else {
            return Ok(HookOutput::Allow);
        };

        let stage_lower = captures[1].to_lowercase();

        let sentinel_dir = self.sentinel_dir()?;

        // Config-driven cumulative stage ordering validation.
        if let Some(block_output) = self.validate_stage_ordering(&sentinel_dir, &stage_lower) {
            return Ok(block_output);
        }

        // Create sentinel file.
        let sentinel_name = format!("ws-{stage_lower}");
        sentinel::create_by_name(&sentinel_dir, &sentinel_name)
            .map_err(|e| HookError::Config(format!("sentinel creation failed: {e}")))?;

        // Update session status with completed stage.
        // Only set last_completed_stage — status is driven by phase checkpoint
        // completion, not stage completion (matches Go stage.go:170).
        if let Ok((session_dir, _)) = self.session_pathflow_dir() {
            update_session_status(
                &session_dir,
                &serde_json::json!({
                    "last_completed_stage": format!("ws-{}", stage_lower),
                }),
            );
        }

        // Release all claims held by the completing session (AC #6).
        self.release_claims_on_stage_complete();

        Ok(HookOutput::Allow)
    }

    /// Release all Loro CRDT claims held by the current session.
    ///
    /// Called on STAGE-COMPLETE to ensure claims are released when a work
    /// stage finishes. The entire load-release-save cycle is wrapped in a
    /// sidecar file lock (`state.loro.lock`) to prevent races.
    /// Errors are logged but do not block sentinel creation.
    fn release_claims_on_stage_complete(&self) {
        use crate::coordination::claims::release_all;
        use crate::coordination::loro::LoroCoordinator;

        let state_path = self
            .project_dir
            .join(".state")
            .join("coordination")
            .join("state.loro");

        if !state_path.exists() {
            return; // No coordination state — nothing to release.
        }

        let sid = match session::current_session_id(&self.project_dir) {
            Ok(sid) => sid,
            Err(_) => return, // No session — nothing to release.
        };

        let result = crate::file_lock::locked_binary_rmw(
            &state_path,
            LoroCoordinator::in_memory,
            |bytes| {
                LoroCoordinator::from_bytes(bytes, &state_path)
                    .map_err(|e| format!("loro load: {e}"))
            },
            |coord| coord.export_bytes().map_err(|e| format!("loro save: {e}")),
            |coord| {
                match release_all(coord, &sid) {
                    Ok(count) => {
                        if count > 0 {
                            eprintln!(
                                "sentinel-write: released {count} claim(s) for session {sid}"
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("sentinel-write: claim release failed: {e}");
                    }
                }
                Ok(())
            },
        );

        if let Err(e) = result {
            eprintln!("sentinel-write: claim release failed (lock/io): {e}");
        }
    }

    /// Config-driven cumulative stage ordering validation.
    /// Mirrors Go `validateStageOrdering()` in sentinel/stage.go.
    fn validate_stage_ordering(
        &self,
        sentinel_dir: &Path,
        stage_lower: &str,
    ) -> Option<HookOutput> {
        use super::pipeline;

        // Try config-driven validation first.
        if let Ok((session_dir, _)) = self.session_pathflow_dir() {
            let work_type = pipeline::read_work_type_from_session_status(&session_dir);
            if !work_type.is_empty() {
                let config_dir = pipeline::derive_config_dir(sentinel_dir);
                if let Ok(pipelines) = pipeline::load_pipelines(&config_dir) {
                    if let Some(pipeline_stages) = pipelines.get(&work_type) {
                        let current_stage = format!("WS-{}", stage_lower.to_uppercase());
                        if let Some(current_index) =
                            pipeline_stages.iter().position(|s| s == &current_stage)
                        {
                            if current_index > 0 {
                                let (ok, missing) = pipeline::verify_cumulative_stage_sentinels(
                                    sentinel_dir,
                                    pipeline_stages,
                                    current_index - 1,
                                );
                                if !ok {
                                    return Some(HookOutput::Block {
                                        reason: format!(
                                            "BLOCKED: ws-{stage_lower} requires prior stage sentinel \
                                             '{missing}' ({work_type} pipeline: {pipeline_stages:?})"
                                        ),
                                        category: Some(BlockCategory::Gate),
                                    });
                                }
                            }
                            return None; // Config-driven check passed
                        }
                    }
                }
            }
        }

        // Fallback to hardcoded ordering when config is unavailable.
        match stage_lower {
            "rev" => {
                let has_primary = PRIMARY_STAGES
                    .iter()
                    .any(|ps| sentinel::check_by_name(sentinel_dir, &format!("ws-{ps}")));
                if !has_primary {
                    return Some(HookOutput::Block {
                        reason: "BLOCKED: ws-rev requires prior primary stage \
                                 (ws-dev/ws-plan/ws-docs/ws-test)"
                            .into(),
                        category: Some(BlockCategory::Gate),
                    });
                }
            }
            "qa" => {
                if !sentinel::check_by_name(sentinel_dir, "ws-dev")
                    && !sentinel::check_by_name(sentinel_dir, "ws-test")
                {
                    return Some(HookOutput::Block {
                        reason: "BLOCKED: ws-qa requires prior ws-dev or ws-test sentinel".into(),
                        category: Some(BlockCategory::Gate),
                    });
                }
            }
            _ => {}
        }
        None
    }
}

impl HookHandler for SentinelWrite {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        match input.tool_name.as_deref() {
            Some("SendMessage") => self.handle_send_message(&input),
            Some("TeamCreate") => {
                let tool_input = input.tool_input.as_ref();
                let team_name = tool_input
                    .and_then(|v| v.get("team_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if team_name.is_empty() {
                    return Ok(HookOutput::Allow);
                }

                match self.session_pathflow_dir() {
                    Ok((session_dir, session_id)) => {
                        handle_team_create(team_name, &session_dir, &session_id)
                    }
                    Err(_) => Ok(HookOutput::Allow),
                }
            }
            Some("Task") => {
                let tool_input = input.tool_input.as_ref();
                let agent_name = tool_input
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match self.session_pathflow_dir() {
                    Ok((session_dir, _)) => handle_teammate_spawn(agent_name, &session_dir, None),
                    Err(_) => Ok(HookOutput::Allow),
                }
            }
            Some("Agent") => {
                let tool_input = input.tool_input.as_ref();
                let agent_name = tool_input
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if agent_name.is_empty() {
                    return Ok(HookOutput::Allow);
                }

                let model = tool_input
                    .and_then(|v| v.get("model"))
                    .and_then(|v| v.as_str());
                let subagent_type = tool_input
                    .and_then(|v| v.get("subagent_type"))
                    .and_then(|v| v.as_str());
                let meta = TeammateSpawnMeta {
                    model,
                    subagent_type,
                };

                match self.session_pathflow_dir() {
                    Ok((session_dir, _)) => {
                        handle_teammate_spawn(agent_name, &session_dir, Some(&meta))
                    }
                    Err(_) => Ok(HookOutput::Allow),
                }
            }
            Some("TeamDelete") => match self.session_pathflow_dir() {
                Ok((session_dir, session_id)) => handle_team_delete(
                    &session_dir,
                    &self.project_dir,
                    &session_id,
                    input.tool_response.as_ref(),
                ),
                Err(_) => Ok(HookOutput::Allow),
            },
            _ => Ok(HookOutput::Allow),
        }
    }

    fn name(&self) -> &'static str {
        "sentinel-write"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// Session status helper
// ---------------------------------------------------------------------------

/// Update a field in `pathflow-session-status.json`.
///
/// Reads the existing status file, merges the provided updates, and writes
/// back atomically. Non-fatal: returns silently on any I/O or parse error.
pub fn update_session_status(session_dir: &Path, updates: &serde_json::Value) {
    let status_path = session_dir.join("pathflow-session-status.json");
    let updates = updates.clone();
    let _ = crate::pathflow::file_lock::locked_rmw(&status_path, |status| {
        if let (Some(obj), Some(upd)) = (status.as_object_mut(), updates.as_object()) {
            for (k, v) in upd {
                obj.insert(k.clone(), v.clone());
            }
            obj.insert(
                "updated_at".to_string(),
                serde_json::Value::String(crate::util::now_rfc3339()),
            );
        }
    });
}

// ---------------------------------------------------------------------------
// TeamCreate handler
// ---------------------------------------------------------------------------

/// Handle a `TeamCreate` post-tool-use event. Creates `pathflow-team.json`
/// in the session pathflow directory with team metadata, and updates the
/// session status to `pf-started`.
///
/// Mirrors Go's `HandleTeamCreate` in `sentinel/stage.go`.
///
/// # Errors
///
/// Returns `HookError` on I/O or serialization failures.
pub fn handle_team_create(
    team_name: &str,
    session_dir: &Path,
    session_id: &str,
) -> Result<HookOutput, HookError> {
    if team_name.is_empty() || session_id.is_empty() {
        return Ok(HookOutput::Allow);
    }

    let lead_pid = crate::session::process::get_claude_code_pid();
    let team = PathflowTeamInfo {
        team_name: team_name.to_string(),
        codeflow_session_id: session_id.to_string(),
        lead_pid,
        teammate_spawned: false,
        created_at: crate::util::now_rfc3339(),
        last_spawn_name: None,
        teammates: vec![],
    };

    let team_json = serde_json::to_string_pretty(&team)
        .map_err(|e| HookError::Config(format!("pathflow-team.json marshal failed: {e}")))?;

    fs::create_dir_all(session_dir).map_err(HookError::Io)?;

    let team_file_path = session_dir.join("pathflow-team.json");
    atomic_write_file(&team_file_path, team_json.as_bytes())?;

    // Update session status to pf-started with team_name.
    // Reset phase/stage fields so a new session starts clean.
    update_session_status(
        session_dir,
        &serde_json::json!({
            "status": "pf-started",
            "team_name": team_name,
            "last_completed_phase": "",
            "last_completed_stage": "",
        }),
    );

    Ok(HookOutput::Allow)
}

// ---------------------------------------------------------------------------
// TeammateSpawn handler
// ---------------------------------------------------------------------------

/// Optional metadata for teammate spawn entries.
/// Populated from `Agent` tool inputs; `Task` tool spawns pass `None`.
pub struct TeammateSpawnMeta<'a> {
    pub model: Option<&'a str>,
    pub subagent_type: Option<&'a str>,
}

/// Handle a `Task` or `Agent` (teammate spawn) post-tool-use event. Updates
/// `pathflow-team.json` with `teammate_spawned=true` and `last_spawn_name`.
///
/// Mirrors Go's `HandleTeammateSpawn` in `sentinel/stage.go`.
///
/// # Errors
///
/// Returns `HookError` on I/O or serialization failures.
pub fn handle_teammate_spawn(
    agent_name: &str,
    session_dir: &Path,
    meta: Option<&TeammateSpawnMeta<'_>>,
) -> Result<HookOutput, HookError> {
    let team_file_path = session_dir.join("pathflow-team.json");

    if !team_file_path.exists() {
        // pathflow-team.json doesn't exist yet -- skip silently.
        return Ok(HookOutput::Allow);
    }

    let agent_name = agent_name.to_string();
    let model = meta.and_then(|m| m.model).map(String::from);
    let subagent_type = meta.and_then(|m| m.subagent_type).map(String::from);
    crate::pathflow::file_lock::locked_rmw(&team_file_path, |team| {
        if let Some(obj) = team.as_object_mut() {
            obj.insert(
                "teammate_spawned".to_string(),
                serde_json::Value::Bool(true),
            );
            if !agent_name.is_empty() {
                obj.insert(
                    "last_spawn_name".to_string(),
                    serde_json::Value::String(agent_name.clone()),
                );

                // Resolve backend_type from Claude Code team config.
                let backend_type = resolve_backend_type(obj, &agent_name);

                // Add teammate entry with pid=0 placeholder.
                // The teammate's own SessionStart (tmux) updates pid to its Claude Code PID.
                // In-process teammates never fire SessionStart, so pid stays 0.
                let mut entry = serde_json::json!({
                    "name": agent_name,
                    "pid": 0,
                    "spawned_at": crate::util::now_rfc3339(),
                });
                if let Some(m) = &model {
                    entry["model"] = serde_json::Value::String(m.clone());
                }
                if let Some(st) = &subagent_type {
                    entry["subagent_type"] = serde_json::Value::String(st.clone());
                }
                entry["backend_type"] = serde_json::Value::String(backend_type);
                if let Some(arr) = obj
                    .entry("teammates")
                    .or_insert_with(|| serde_json::json!([]))
                    .as_array_mut()
                {
                    arr.push(entry);
                }
            }
        }
    })
    .map_err(|e| HookError::Config(format!("pathflow-team.json locked_rmw failed: {e}")))?;

    Ok(HookOutput::Allow)
}

// ---------------------------------------------------------------------------
// Backend type resolution
// ---------------------------------------------------------------------------

/// Look up the backend type for a teammate from the Claude Code team config.
///
/// Reads `~/.claude/teams/{team_name}/config.json` and matches the agent by
/// looking for a member whose `agentId` starts with `{agent_name}@`.
///
/// Returns `"unknown"` if the team config is unreadable or the member is not
/// found. Never fails -- used inside a `locked_rmw` closure.
fn resolve_backend_type(
    team_obj: &serde_json::Map<String, serde_json::Value>,
    agent_name: &str,
) -> String {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return "unknown".to_string(),
    };
    resolve_backend_type_with_home(team_obj, agent_name, &home)
}

/// Testable core: resolves backend type given an explicit home directory.
fn resolve_backend_type_with_home(
    team_obj: &serde_json::Map<String, serde_json::Value>,
    agent_name: &str,
    home: &Path,
) -> String {
    let team_name = match team_obj.get("team_name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n,
        _ => return "unknown".to_string(),
    };

    let config_path = home
        .join(".claude")
        .join("teams")
        .join(team_name)
        .join("config.json");

    let config_str = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return "unknown".to_string(),
    };

    let config: serde_json::Value = match serde_json::from_str(&config_str) {
        Ok(v) => v,
        Err(_) => return "unknown".to_string(),
    };

    let prefix = format!("{agent_name}@");
    let members = config.get("members").and_then(|v| v.as_array());
    if let Some(members) = members {
        for member in members {
            let agent_id = member.get("agentId").and_then(|v| v.as_str()).unwrap_or("");
            if agent_id.starts_with(&prefix) {
                return member
                    .get("backendType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
            }
        }
    }

    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// TeamDelete cleanup handler
// ---------------------------------------------------------------------------

/// Handle a `TeamDelete` post-tool-use event. Cleans up `PathFlow` artifacts:
/// 1. Remove the sentinel directory
/// 2. Remove pathflow-team.json
/// 3. Reset the checkpoint file
/// 4. Update session status to `pf-complete`
///
/// Mirrors Go's `HandlePostTeamDelete` in `team/guard.go`.
///
/// All operations are non-fatal: errors are logged but do not block.
///
/// # Errors
///
/// Returns `HookError` on I/O failures.
pub fn handle_team_delete(
    session_dir: &Path,
    project_dir: &Path,
    session_id: &str,
    tool_response: Option<&serde_json::Value>,
) -> Result<HookOutput, HookError> {
    // Check if TeamDelete actually succeeded before cleaning up.
    // Claude Code sends tool_response with {"success": bool} for team operations.
    let success = tool_response
        .and_then(|r| {
            // Handle string-encoded JSON response.
            if let Some(s) = r.as_str() {
                serde_json::from_str::<serde_json::Value>(s)
                    .ok()
                    .and_then(|v| v.get("success").and_then(serde_json::Value::as_bool))
            } else {
                r.get("success").and_then(serde_json::Value::as_bool)
            }
        })
        .unwrap_or(true); // Default: proceed if no response (backward compat)

    if !success {
        return Ok(HookOutput::Allow);
    }

    // 1. Update session status to pf-complete FIRST (before destroying sentinels).
    // This ordering is critical: SessionEnd reads the status file to decide
    // whether cleanup is safe. The status must be "pf-complete" before the
    // sentinel directory is removed, or a concurrent SessionEnd could see
    // "pf-in-progress" with no sentinels and skip cleanup incorrectly.
    update_session_status(
        session_dir,
        &serde_json::json!({
            "status": "pf-complete",
        }),
    );

    // 2. Remove sentinel directory.
    let sentinel_dir = project_dir
        .join(".state")
        .join("sentinels")
        .join("pathflow")
        .join(session_id);

    if sentinel_dir.exists() {
        let _ = fs::remove_dir_all(&sentinel_dir);
    }

    // 3. Remove pathflow-team.json.
    // Worktree stays Active. SessionEnd marks PendingCleanup when process exits.
    let team_file_path = session_dir.join("pathflow-team.json");
    if team_file_path.exists() {
        let _ = fs::remove_file(&team_file_path);
    }

    // 4. Reset checkpoint file via init_all_phases (same as Go's ResetAllPhases).
    let checkpoint_path = session_dir.join("pathflow-phase-tasks.json");
    let config_path = project_dir
        .join(".codeflow")
        .join("config")
        .join("pathflow")
        .join("pathflow-config.json");

    if checkpoint_path.exists() {
        let _ = fs::remove_file(&checkpoint_path);
    }

    if config_path.exists() {
        let cp = Checkpoint::new();
        let _ = cp.init_all_phases(&checkpoint_path, &config_path);
    }

    Ok(HookOutput::Allow)
}

// ---------------------------------------------------------------------------
// CheckpointRegister handler
// ---------------------------------------------------------------------------

/// Registers `PF{N}-TSK-{NN}` tasks in the checkpoint file on `TaskCreate`
/// events. Blocks cross-phase registration when the previous phase sentinel
/// is missing.
pub struct CheckpointRegister {
    pub project_dir: PathBuf,
}

impl CheckpointRegister {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    fn checkpoint_path(&self, session_id: &str) -> PathBuf {
        self.project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json")
    }

    fn sentinel_dir(&self, session_id: &str) -> Result<PathBuf, HookError> {
        sentinel::resolve_dir(&self.project_dir, session_id)
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }
}

impl HookHandler for CheckpointRegister {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only process TaskCreate PostToolUse events.
        match &input.tool_name {
            Some(name) if name == "TaskCreate" => {}
            _ => return Ok(HookOutput::Allow),
        }

        // Extract subject from tool_input.
        let subject = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("subject"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Extract PF{N}-TSK-{NN} from subject.
        let task_id = match pf_task_id_re().find(subject) {
            Some(m) => m.as_str().to_string(),
            None => return Ok(HookOutput::Allow),
        };

        // Resolve session ID.
        let sid = session::current_session_id(&self.project_dir)
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;

        let checkpoint_path = self.checkpoint_path(sid.as_ref());
        let sentinel_dir = self.sentinel_dir(sid.as_ref())?;

        let cp = Checkpoint::new();
        match cp.register_task(&checkpoint_path, &sentinel_dir, &task_id) {
            Ok(()) => Ok(HookOutput::Allow),
            Err(crate::error::PathflowError::CrossPhaseBlock(reason)) => Ok(HookOutput::Block {
                reason: format!("BLOCKED: {reason}"),
                category: Some(BlockCategory::Gate),
            }),
            Err(e) => {
                // Non-blocking: log warning but allow through.
                Ok(HookOutput::Warn {
                    message: format!("checkpoint register warning: {e}"),
                })
            }
        }
    }

    fn name(&self) -> &'static str {
        "checkpoint-register"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// SettingsValidate handler
// ---------------------------------------------------------------------------

/// Validates `settings.json` consistency after Edit/Write operations that
/// target the settings file.
pub struct SettingsValidate {
    pub project_dir: PathBuf,
}

impl SettingsValidate {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for SettingsValidate {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only process Edit/Write PostToolUse events that target settings.
        match &input.tool_name {
            Some(name) if name == "Edit" || name == "Write" => {}
            _ => return Ok(HookOutput::Allow),
        }

        // Check if the file path targets settings.json.
        let file_path = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !file_path.contains("settings.json") {
            return Ok(HookOutput::Allow);
        }

        // Validate that the file is valid JSON.
        match fs::read_to_string(file_path) {
            Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(_) => Ok(HookOutput::Allow),
                Err(e) => Ok(HookOutput::Warn {
                    message: format!("settings.json has invalid JSON: {e}"),
                }),
            },
            Err(e) => Ok(HookOutput::Warn {
                message: format!("could not read settings.json for validation: {e}"),
            }),
        }
    }

    fn name(&self) -> &'static str {
        "settings-validate"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Atomically write file contents: write to `.tmp` file then rename.
fn atomic_write_file(path: &Path, data: &[u8]) -> Result<(), HookError> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data).map_err(HookError::Io)?;
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        HookError::Io(e)
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(tool_name: &str, tool_input: serde_json::Value) -> HookInput {
        HookInput {
            tool_name: Some(tool_name.into()),
            tool_input: Some(tool_input),
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: Some("/tmp/test-project".into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        }
    }

    // -- SentinelWrite tests --

    #[test]
    fn test_sentinel_write_ignores_non_send_message() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_ignores_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("SendMessage", serde_json::json!({"message": ""}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_ignores_non_stage_content() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "DEV-COMPLETE: all done"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_stage_complete_regex_matches() {
        let re = stage_complete_re();
        assert!(re.is_match("STAGE-COMPLETE: WS-DEV"));
        assert!(re.is_match("STAGE-COMPLETE:  WS-REV"));
        assert!(re.is_match("STAGE-COMPLETE: WS-QA"));
        assert!(re.is_match("STAGE-COMPLETE: WS-TEST"));
        assert!(re.is_match("STAGE-COMPLETE: WS-PLAN"));
        assert!(re.is_match("STAGE-COMPLETE: WS-DOCS"));
        assert!(!re.is_match("STAGE-COMPLETE: WS-UNKNOWN"));
    }

    #[test]
    fn test_pf_task_id_regex_matches() {
        let re = pf_task_id_re();
        assert!(re.is_match("PF1-TSK-01"));
        assert!(re.is_match("PF7-TSK-03"));
        assert!(re.is_match("Create PF4-TSK-05: register task"));
        assert!(!re.is_match("TSK-01"));
        assert!(!re.is_match("PF-TSK-01"));
    }

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(collapse_whitespace("hello  world"), "hello world");
        assert_eq!(
            collapse_whitespace("  STAGE-COMPLETE:   WS-DEV  "),
            "STAGE-COMPLETE: WS-DEV"
        );
    }

    // -- CheckpointRegister tests --

    #[test]
    fn test_checkpoint_register_ignores_non_task_create() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_checkpoint_register_ignores_non_pf_subject() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input(
            "TaskCreate",
            serde_json::json!({"subject": "Some regular task"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- SettingsValidate tests --

    #[test]
    fn test_settings_validate_ignores_non_edit_write() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_ignores_non_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": "/tmp/other-file.rs"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        fs::write(&settings_path, r#"{"key": "value"}"#).unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        fs::write(&settings_path, "not valid json {{{").unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_settings_validate_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": "/tmp/nonexistent/settings.json"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    // -- SentinelWrite: stage ordering and sentinel creation --

    /// Helper to set up a tempdir with session env so sentinel_dir() works.
    fn setup_sentinel_env(dir: &std::path::Path) -> String {
        let sid = "ses-1234567890abc";
        let runtime_dir = dir.join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID=\"{sid}\"\nexport CF_PROJECT_ROOT=\"test\"\n"),
        )
        .unwrap();
        sid.to_string()
    }

    #[test]
    fn test_sentinel_write_creates_dev_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-DEV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Verify sentinel file was created.
        let sentinel_path = dir
            .path()
            .join(".state/sentinels/pathflow")
            .join(&sid)
            .join("pathflow-ws-dev");
        assert!(sentinel_path.exists(), "ws-dev sentinel should exist");
    }

    #[test]
    fn test_sentinel_write_releases_claims_on_stage_complete() {
        use crate::coordination::Coordinator;
        use crate::coordination::loro::LoroCoordinator;
        use crate::types::SessionId;

        let dir = tempfile::tempdir().unwrap();
        let sid_str = setup_sentinel_env(dir.path());

        // Pre-populate a claim for this session.
        let coord_state_path = dir
            .path()
            .join(".state")
            .join("coordination")
            .join("state.loro");
        let session_id = SessionId::new_unchecked(&sid_str);
        {
            let mut coord = LoroCoordinator::new(&coord_state_path).unwrap();
            coord.acquire("src/main.rs", &session_id).unwrap();
            coord.acquire("src/lib.rs", &session_id).unwrap();
            coord.persist().unwrap();

            // Verify claims exist before STAGE-COMPLETE.
            assert!(
                coord.check("src/main.rs").is_some(),
                "claim should exist before release"
            );
            assert!(
                coord.check("src/lib.rs").is_some(),
                "claim should exist before release"
            );
        }

        // Trigger STAGE-COMPLETE: WS-DEV via handle().
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-DEV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Verify claims were released.
        let coord_after = LoroCoordinator::new(&coord_state_path).unwrap();
        assert!(
            coord_after.check("src/main.rs").is_none(),
            "claim on src/main.rs should be released after STAGE-COMPLETE"
        );
        assert!(
            coord_after.check("src/lib.rs").is_none(),
            "claim on src/lib.rs should be released after STAGE-COMPLETE"
        );
    }

    #[test]
    fn test_sentinel_write_rev_blocked_without_primary() {
        let dir = tempfile::tempdir().unwrap();
        let _sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-REV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "ws-rev should be blocked without a primary stage sentinel"
        );
    }

    #[test]
    fn test_sentinel_write_rev_allowed_after_dev() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        // Create ws-dev sentinel first.
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-ws-dev"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-REV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_qa_blocked_without_dev_or_test() {
        let dir = tempfile::tempdir().unwrap();
        let _sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Block { .. }));
    }

    #[test]
    fn test_sentinel_write_qa_allowed_after_dev() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-ws-dev"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_qa_allowed_after_test() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-ws-test"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_normalizes_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"message": "STAGE-COMPLETE:   WS-PLAN"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let sentinel_path = dir
            .path()
            .join(".state/sentinels/pathflow")
            .join(&sid)
            .join("pathflow-ws-plan");
        assert!(sentinel_path.exists());
    }

    #[test]
    fn test_sentinel_write_no_tool_input() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("SendMessage".into()),
            tool_input: None,
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- CheckpointRegister: with valid PF task subjects --

    #[test]
    fn test_checkpoint_register_no_subject_field() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input("TaskCreate", serde_json::json!({"description": "stuff"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- SettingsValidate: Write tool variant --

    #[test]
    fn test_settings_validate_write_tool_valid() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        fs::write(&settings_path, r#"{"hooks": []}"#).unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_write_tool_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        fs::write(&settings_path, "{broken").unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_settings_validate_no_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input("Edit", serde_json::json!({"content": "hello"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- Handler metadata tests --

    #[test]
    fn test_handler_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            SentinelWrite::new(dir.path().to_path_buf()).name(),
            "sentinel-write"
        );
        assert_eq!(
            CheckpointRegister::new(dir.path().to_path_buf()).name(),
            "checkpoint-register"
        );
        assert_eq!(
            SettingsValidate::new(dir.path().to_path_buf()).name(),
            "settings-validate"
        );
    }

    #[test]
    fn test_handler_events() {
        let dir = tempfile::tempdir().unwrap();
        let sw = SentinelWrite::new(dir.path().to_path_buf());
        assert_eq!(sw.events(), &[HookEvent::PostToolUse]);

        let cr = CheckpointRegister::new(dir.path().to_path_buf());
        assert_eq!(cr.events(), &[HookEvent::PostToolUse]);
    }

    // -- TeamCreate tests --

    #[test]
    fn test_handle_team_create_creates_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");

        let result = handle_team_create("my-team", &session_dir, "ses-abc123").unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let team_file = session_dir.join("pathflow-team.json");
        assert!(team_file.exists(), "pathflow-team.json should be created");

        let data = fs::read_to_string(&team_file).unwrap();
        let info: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert_eq!(info.team_name, "my-team");
        assert_eq!(info.codeflow_session_id, "ses-abc123");
        assert!(!info.teammate_spawned);
        assert!(info.last_spawn_name.is_none());
        assert!(!info.created_at.is_empty());
        assert!(!info.created_at.is_empty());
    }

    #[test]
    fn test_handle_team_create_empty_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");

        let result = handle_team_create("", &session_dir, "ses-abc123").unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // No file should be created.
        let team_file = session_dir.join("pathflow-team.json");
        assert!(!team_file.exists());
    }

    #[test]
    fn test_handle_team_create_empty_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");

        let result = handle_team_create("my-team", &session_dir, "").unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let team_file = session_dir.join("pathflow-team.json");
        assert!(!team_file.exists());
    }

    #[test]
    fn test_handle_team_create_resets_phase_and_stage() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Pre-create a status file with stale phase/stage from a previous session.
        let status = serde_json::json!({
            "status": "created",
            "team_name": "",
            "last_completed_phase": "pf-6",
            "last_completed_stage": "WS-QA",
            "created_at": "2026-03-14T00:00:00Z",
            "updated_at": "2026-03-14T00:00:00Z",
        });
        fs::write(
            session_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let result = handle_team_create("new-team", &session_dir, "ses-newses123").unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Read back the status file and verify phase/stage are cleared.
        let data = fs::read_to_string(session_dir.join("pathflow-session-status.json")).unwrap();
        let updated: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(updated["status"], "pf-started");
        assert_eq!(updated["team_name"], "new-team");
        assert_eq!(
            updated["last_completed_phase"], "",
            "last_completed_phase should be cleared on TeamCreate"
        );
        assert_eq!(
            updated["last_completed_stage"], "",
            "last_completed_stage should be cleared on TeamCreate"
        );
    }

    // -- TeammateSpawn tests --

    #[test]
    fn test_handle_teammate_spawn_updates_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Create initial team file.
        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            codeflow_session_id: "ses-abc".into(),
            lead_pid: 0,
            teammate_spawned: false,
            created_at: "2026-03-10T00:00:00Z".into(),
            last_spawn_name: None,
            teammates: vec![],
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        let result = handle_teammate_spawn("cf-development", &session_dir, None).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Verify updates.
        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-development"));
        // Unchanged fields.
        assert_eq!(updated.team_name, "my-team");
    }

    #[test]
    fn test_handle_teammate_spawn_no_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // No pathflow-team.json exists.
        let result = handle_teammate_spawn("cf-review", &session_dir, None).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_handle_teammate_spawn_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        let result = handle_teammate_spawn("", &session_dir, None).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        // Empty name should not update last_spawn_name.
        assert!(updated.last_spawn_name.is_none());
    }

    #[test]
    fn test_handle_teammate_spawn_multiple_spawns() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        // First spawn.
        handle_teammate_spawn("cf-development", &session_dir, None).unwrap();
        // Second spawn should update last_spawn_name.
        handle_teammate_spawn("cf-review", &session_dir, None).unwrap();

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-review"));
    }

    // -- Agent tool spawn tests --

    #[test]
    fn test_handle_teammate_spawn_agent_with_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            codeflow_session_id: "ses-abc".into(),
            lead_pid: 0,
            teammate_spawned: false,
            created_at: "2026-03-10T00:00:00Z".into(),
            last_spawn_name: None,
            teammates: vec![],
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        let meta = TeammateSpawnMeta {
            model: Some("opus"),
            subagent_type: Some("general-purpose"),
        };
        let result = handle_teammate_spawn("cf-development", &session_dir, Some(&meta)).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-development"));
        assert_eq!(updated.teammates.len(), 1);
        assert_eq!(updated.teammates[0].name, "cf-development");
        assert_eq!(updated.teammates[0].model.as_deref(), Some("opus"));
        assert_eq!(
            updated.teammates[0].subagent_type.as_deref(),
            Some("general-purpose")
        );
        assert_eq!(updated.teammates[0].pid, 0);
    }

    #[test]
    fn test_handle_teammate_spawn_agent_partial_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        // Only model, no subagent_type.
        let meta = TeammateSpawnMeta {
            model: Some("sonnet"),
            subagent_type: None,
        };
        let result = handle_teammate_spawn("cf-security", &session_dir, Some(&meta)).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert_eq!(updated.teammates.len(), 1);
        assert_eq!(updated.teammates[0].model.as_deref(), Some("sonnet"));
        assert!(updated.teammates[0].subagent_type.is_none());
    }

    #[test]
    fn test_handle_teammate_spawn_agent_no_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        // No metadata (like Task tool).
        let result = handle_teammate_spawn("cf-git-operations", &session_dir, None).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert_eq!(updated.teammates.len(), 1);
        assert!(updated.teammates[0].model.is_none());
        assert!(updated.teammates[0].subagent_type.is_none());
    }

    #[test]
    fn test_handle_teammate_spawn_mixed_task_and_agent() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        // First spawn via Task (no metadata).
        handle_teammate_spawn("cf-security", &session_dir, None).unwrap();

        // Second spawn via Agent (with metadata).
        let meta = TeammateSpawnMeta {
            model: Some("opus"),
            subagent_type: Some("general-purpose"),
        };
        handle_teammate_spawn("cf-development", &session_dir, Some(&meta)).unwrap();

        let data = fs::read_to_string(&team_file).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert_eq!(updated.teammates.len(), 2);
        // First entry: Task-spawned, no metadata.
        assert_eq!(updated.teammates[0].name, "cf-security");
        assert!(updated.teammates[0].model.is_none());
        // Second entry: Agent-spawned, with metadata.
        assert_eq!(updated.teammates[1].name, "cf-development");
        assert_eq!(updated.teammates[1].model.as_deref(), Some("opus"));
        assert_eq!(
            updated.teammates[1].subagent_type.as_deref(),
            Some("general-purpose")
        );
        // last_spawn_name should be the most recent.
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-development"));
    }

    // -- TeamDelete tests --

    #[test]
    fn test_handle_team_delete_removes_sentinels_and_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        let session_id = "ses-delete-test";
        let project_dir = dir.path();

        fs::create_dir_all(&session_dir).unwrap();

        // Create pathflow-team.json.
        let team = PathflowTeamInfo {
            team_name: "my-team".into(),
            ..Default::default()
        };
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string_pretty(&team).unwrap()).unwrap();

        // Create sentinel directory with some sentinels.
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        fs::write(sentinel_dir.join("pathflow-ws-dev"), "").unwrap();

        let result = handle_team_delete(
            &session_dir,
            project_dir,
            session_id,
            Some(&serde_json::json!({"success": true})),
        )
        .unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Sentinel directory should be gone.
        assert!(!sentinel_dir.exists(), "sentinel dir should be removed");
        // pathflow-team.json should be gone.
        assert!(!team_file.exists(), "pathflow-team.json should be removed");
    }

    #[test]
    fn test_handle_team_delete_missing_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // No sentinel dir, no team file -- should not error.
        let result = handle_team_delete(
            &session_dir,
            dir.path(),
            "ses-missing-test",
            Some(&serde_json::json!({"success": true})),
        )
        .unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_handle_team_delete_resets_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        let project_dir = dir.path();

        fs::create_dir_all(&session_dir).unwrap();

        // Create a dummy checkpoint file.
        let checkpoint_path = session_dir.join("pathflow-phase-tasks.json");
        fs::write(&checkpoint_path, r#"{"PF1": {"expected": ["PF1-TSK-01"]}}"#).unwrap();

        // Create pathflow-config.json.
        let config_dir = project_dir
            .join(".codeflow")
            .join("config")
            .join("pathflow");
        fs::create_dir_all(&config_dir).unwrap();
        let config = serde_json::json!({
            "phases": {
                "PF1-INIT": {
                    "required_tasks": ["PF1-TSK-01"],
                    "tasks": [{"id": "PF1-TSK-01"}]
                }
            }
        });
        fs::write(
            config_dir.join("pathflow-config.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();

        let result = handle_team_delete(
            &session_dir,
            project_dir,
            "ses-reset-test",
            Some(&serde_json::json!({"success": true})),
        )
        .unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Checkpoint should have been reset (re-initialized fresh).
        assert!(
            checkpoint_path.exists(),
            "checkpoint should be re-initialized"
        );
        let data = fs::read_to_string(&checkpoint_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        // PF1 should exist with empty registered/completed.
        let pf1 = parsed
            .get("PF1")
            .expect("PF1 should exist in reset checkpoint");
        assert!(
            pf1.get("registered")
                .and_then(|v| v.as_object())
                .is_some_and(serde_json::Map::is_empty),
            "registered should be empty after reset"
        );
    }

    #[test]
    fn test_handle_team_delete_skips_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        let session_id = "ses-fail-test";
        let project_dir = dir.path();

        fs::create_dir_all(&session_dir).unwrap();

        // Create pathflow-team.json.
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, r#"{"team_name":"my-team"}"#).unwrap();

        // Create sentinel directory with sentinels.
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        // Call with tool_response indicating failure.
        let result = handle_team_delete(
            &session_dir,
            project_dir,
            session_id,
            Some(&serde_json::json!({"success": false})),
        )
        .unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Team file and sentinels should still exist (no cleanup performed).
        assert!(
            team_file.exists(),
            "team file should survive failed TeamDelete"
        );
        assert!(
            sentinel_dir.exists(),
            "sentinels should survive failed TeamDelete"
        );
    }

    #[test]
    fn test_handle_team_delete_cleans_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        let session_id = "ses-success-test";
        let project_dir = dir.path();

        fs::create_dir_all(&session_dir).unwrap();

        // Create pathflow-team.json.
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, r#"{"team_name":"my-team"}"#).unwrap();

        // Create sentinel directory.
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        let result = handle_team_delete(
            &session_dir,
            project_dir,
            session_id,
            Some(&serde_json::json!({"success": true})),
        )
        .unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Cleanup should have proceeded.
        assert!(
            !team_file.exists(),
            "team file should be removed on success"
        );
        assert!(
            !sentinel_dir.exists(),
            "sentinels should be removed on success"
        );
    }

    #[test]
    fn test_handle_team_delete_no_response_proceeds() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("pathflow");
        let session_id = "ses-noresponse-test";
        let project_dir = dir.path();

        fs::create_dir_all(&session_dir).unwrap();

        // Create pathflow-team.json.
        let team_file = session_dir.join("pathflow-team.json");
        fs::write(&team_file, r#"{"team_name":"my-team"}"#).unwrap();

        // Create sentinel directory.
        let sentinel_dir = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        // Call with None tool_response (backward compatibility).
        let result = handle_team_delete(&session_dir, project_dir, session_id, None).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Cleanup should proceed (backward compat default = true).
        assert!(
            !team_file.exists(),
            "team file should be removed with no response"
        );
        assert!(
            !sentinel_dir.exists(),
            "sentinels should be removed with no response"
        );
    }

    // -- SentinelWrite dispatch to TeamCreate/Task/TeamDelete via handler --

    #[test]
    fn test_sentinel_write_dispatches_team_create() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        // Create the session pathflow dir.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "TeamCreate",
            serde_json::json!({"team_name": "dispatch-team"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let team_file = session_dir.join("pathflow-team.json");
        assert!(
            team_file.exists(),
            "TeamCreate dispatch should create pathflow-team.json"
        );
    }

    #[test]
    fn test_sentinel_write_dispatches_task_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Pre-create team file.
        let team = PathflowTeamInfo {
            team_name: "t".into(),
            ..Default::default()
        };
        fs::write(
            session_dir.join("pathflow-team.json"),
            serde_json::to_string(&team).unwrap(),
        )
        .unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("Task", serde_json::json!({"name": "cf-development"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(session_dir.join("pathflow-team.json")).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-development"));
    }

    #[test]
    fn test_sentinel_write_dispatches_agent_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Pre-create team file.
        let team = PathflowTeamInfo {
            team_name: "t".into(),
            ..Default::default()
        };
        fs::write(
            session_dir.join("pathflow-team.json"),
            serde_json::to_string(&team).unwrap(),
        )
        .unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "Agent",
            serde_json::json!({
                "name": "cf-development",
                "model": "opus",
                "subagent_type": "general-purpose"
            }),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(session_dir.join("pathflow-team.json")).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.last_spawn_name.as_deref(), Some("cf-development"));
        assert_eq!(updated.teammates.len(), 1);
        assert_eq!(updated.teammates[0].model.as_deref(), Some("opus"));
        assert_eq!(
            updated.teammates[0].subagent_type.as_deref(),
            Some("general-purpose")
        );
    }

    #[test]
    fn test_sentinel_write_dispatches_agent_spawn_without_name() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Pre-create team file.
        let team = PathflowTeamInfo {
            team_name: "t".into(),
            ..Default::default()
        };
        fs::write(
            session_dir.join("pathflow-team.json"),
            serde_json::to_string(&team).unwrap(),
        )
        .unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        // Agent without name field -- should skip without error.
        let input = make_input("Agent", serde_json::json!({"model": "opus"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(session_dir.join("pathflow-team.json")).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        // Should not have modified the file.
        assert!(!updated.teammate_spawned);
        assert!(updated.teammates.is_empty());
    }

    #[test]
    fn test_sentinel_write_dispatches_agent_spawn_no_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        let team = PathflowTeamInfo {
            team_name: "t".into(),
            ..Default::default()
        };
        fs::write(
            session_dir.join("pathflow-team.json"),
            serde_json::to_string(&team).unwrap(),
        )
        .unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        // Agent with name but no model or subagent_type.
        let input = make_input("Agent", serde_json::json!({"name": "cf-security"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let data = fs::read_to_string(session_dir.join("pathflow-team.json")).unwrap();
        let updated: PathflowTeamInfo = serde_json::from_str(&data).unwrap();
        assert!(updated.teammate_spawned);
        assert_eq!(updated.teammates.len(), 1);
        assert_eq!(updated.teammates[0].name, "cf-security");
        assert!(updated.teammates[0].model.is_none());
        assert!(updated.teammates[0].subagent_type.is_none());
    }

    #[test]
    fn test_sentinel_write_dispatches_team_delete() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&sid)
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Create team file to verify removal.
        fs::write(
            session_dir.join("pathflow-team.json"),
            r#"{"team_name":"t"}"#,
        )
        .unwrap();

        // Create sentinel dir.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&sid);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("TeamDelete", serde_json::json!({}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        assert!(
            !sentinel_dir.exists(),
            "sentinel dir should be cleaned by TeamDelete"
        );
        assert!(
            !session_dir.join("pathflow-team.json").exists(),
            "team file should be cleaned by TeamDelete"
        );
    }

    #[test]
    fn test_sentinel_write_team_create_empty_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let _sid = setup_sentinel_env(dir.path());

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("TeamCreate", serde_json::json!({"team_name": ""}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- atomic_write_file tests --

    #[test]
    fn test_atomic_write_file_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");

        atomic_write_file(&path, b"hello world").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
        // Temp file should not remain.
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn test_atomic_write_file_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");

        atomic_write_file(&path, b"first").unwrap();
        atomic_write_file(&path, b"second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    // -- Backend type resolution tests --

    #[test]
    fn test_handle_teammate_spawn_reads_backend_type() {
        // Test resolve_backend_type_with_home directly (avoids unsafe env var mutation).
        let home = tempfile::tempdir().unwrap();
        let team_name = "backend-test-team";

        // Create Claude Code team config with backendType.
        let config_dir = home.path().join(".claude").join("teams").join(team_name);
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "members": [
                    {
                        "agentId": format!("cf-development@{team_name}"),
                        "backendType": "tmux"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        let mut team_obj = serde_json::Map::new();
        team_obj.insert(
            "team_name".to_string(),
            serde_json::Value::String(team_name.to_string()),
        );

        let bt = resolve_backend_type_with_home(&team_obj, "cf-development", home.path());
        assert_eq!(bt, "tmux");
    }

    #[test]
    fn test_handle_teammate_spawn_default_backend_unknown() {
        // No team config exists -> should default to "unknown".
        let home = tempfile::tempdir().unwrap();

        let mut team_obj = serde_json::Map::new();
        team_obj.insert(
            "team_name".to_string(),
            serde_json::Value::String("no-config-team".to_string()),
        );

        let bt = resolve_backend_type_with_home(&team_obj, "cf-review", home.path());
        assert_eq!(bt, "unknown");
    }
}
