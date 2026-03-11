//! Hook handler infrastructure for Claude Code lifecycle events.
//!
//! Provides the `HookHandler` trait and supporting types (`HookInput`,
//! `HookOutput`, `HookEvent`) that all hook implementations build on.
//! Session-specific hooks live in submodules.

pub mod logging;
pub mod post_tool_use;
pub mod pre_tool_use;
pub mod security;
pub mod session_end;
pub mod session_start;
pub mod task_completed;

use serde::{Deserialize, Serialize};

use crate::error::HookError;

/// Hook lifecycle events (maps to Claude Code hook events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    TaskCompleted,
    SessionStart,
    SessionEnd,
    Stop,
    UserPromptSubmit,
}

/// Typed input for hook handlers.
///
/// Tool-use fields (`tool_name`, `tool_input`) are optional because session
/// events do not carry tool context. The `source` field is session-specific
/// (startup, resume, compact, clear).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// The tool name from Claude Code (e.g., "Bash", "Edit", "Write", "Task").
    /// `None` for session events that have no tool context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// The tool input payload as raw JSON.
    /// `None` for session events that have no tool context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,

    /// The hook event type.
    pub event: HookEvent,

    /// Session ID (from environment or stdin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Project root directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,

    /// Session source type: startup, resume, compact, clear, or unknown.
    /// Only present for `SessionStart` / `SessionEnd` events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Path to session transcript (`SessionEnd` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

/// Result of a hook handler evaluation.
#[derive(Debug, Clone)]
pub enum HookOutput {
    /// The operation is allowed to proceed. Exit code 0.
    Allow,

    /// The operation is blocked. Exit code 2 in Claude Code hook protocol.
    Block {
        /// Human-readable reason displayed to the agent.
        reason: String,
        /// Optional classification of the block type.
        category: Option<BlockCategory>,
    },

    /// The operation is allowed but a warning is emitted to stderr.
    Warn {
        /// Warning message.
        message: String,
    },
}

impl HookOutput {
    /// Return the exit code for this output per the Claude Code hook protocol.
    ///
    /// - `Allow` -> 0
    /// - `Block` -> 2
    /// - `Warn`  -> 0 (warning is informational, does not block)
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Allow | Self::Warn { .. } => 0,
            Self::Block { .. } => 2,
        }
    }
}

/// Classification of why a hook blocked an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockCategory {
    /// `PathFlow` gate check (missing sentinel).
    Gate,
    /// Security enforcement (dangerous command).
    Security,
    /// Team guard (team dissolution protection).
    TeamGuard,
    /// Edit/write scope enforcement.
    EditWriteScope,
    /// GitHub PR guard (protected branch).
    GhPrGuard,
    /// Protected resource access.
    ProtectedResource,
    /// `WebFetch` domain block.
    WebFetch,
}

/// `HookHandler` processes a single hook enforcement module.
pub trait HookHandler: Send + Sync {
    /// Process the hook input and return a verdict.
    ///
    /// # Errors
    ///
    /// Returns `HookError` if the handler encounters a parse, I/O, or config error.
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError>;

    /// Human-readable name for this handler (for logging/diagnostics).
    fn name(&self) -> &'static str;

    /// Which hook events this handler responds to.
    fn events(&self) -> &[HookEvent];
}

/// Abstracts PID liveness checks for testability.
pub trait ProcessChecker: Send + Sync {
    /// Returns `true` if the process with the given PID is alive.
    fn is_alive(&self, pid: u32) -> bool;
}

/// Abstracts tmux pane liveness checks for testability.
pub trait TmuxChecker: Send + Sync {
    /// Returns `true` if the tmux pane with the given ID is alive.
    fn is_pane_alive(&self, pane_id: &str) -> bool;

    /// List all active tmux pane IDs.
    ///
    /// # Errors
    ///
    /// Returns `HookError::Io` if the tmux command fails.
    fn list_panes(&self) -> Result<Vec<String>, HookError>;
}

/// Production `ProcessChecker` using `kill -0`.
pub struct OsProcessChecker;

impl ProcessChecker for OsProcessChecker {
    fn is_alive(&self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // On Unix, kill(pid, 0) checks if the process exists.
        libc_signal_zero(pid)
    }
}

/// Check process liveness via `kill(pid, 0)`.
///
/// Uses `std::process::Command` to avoid requiring libc crate.
fn libc_signal_zero(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Production `TmuxChecker` using `tmux list-panes`.
pub struct OsTmuxChecker;

impl TmuxChecker for OsTmuxChecker {
    fn is_pane_alive(&self, pane_id: &str) -> bool {
        if pane_id.is_empty() {
            return false;
        }
        self.list_panes()
            .is_ok_and(|panes| panes.iter().any(|p| p == pane_id))
    }

    fn list_panes(&self) -> Result<Vec<String>, HookError> {
        let output = std::process::Command::new("tmux")
            .args(["list-panes", "-a", "-F", "#{pane_id}"])
            .output()
            .map_err(HookError::Io)?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let panes = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(panes)
    }
}

/// JSON structure for `pathflow-team.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathflowTeamInfo {
    /// PID of the lead agent process.
    #[serde(default)]
    pub lead_pid: u32,
    /// Team name for Claude Code teams.
    #[serde(default)]
    pub team_name: String,
}

/// JSON structure for session metadata file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// Session ID.
    #[serde(default)]
    pub session_id: String,
    /// Creation timestamp (RFC 3339).
    #[serde(default)]
    pub created_at: String,
    /// Source type (startup, resume, compact, clear).
    #[serde(default)]
    pub source: String,
    /// Parent process ID.
    #[serde(default)]
    pub ppid: u32,
    /// Binary version.
    #[serde(default)]
    pub version: String,
    /// Permission mode from stdin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_input_deserialize_tool_use_event() {
        let json = r#"{
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "event": "pre_tool_use",
            "session_id": "ses-abc123"
        }"#;
        let input: HookInput = serde_json::from_str(json).expect("deserialize");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert_eq!(input.event, HookEvent::PreToolUse);
        assert!(input.tool_input.is_some());
    }

    #[test]
    fn test_hook_input_deserialize_session_event_no_tool_fields() {
        let json = r#"{
            "event": "session_start",
            "session_id": "ses-abc123",
            "source": "startup"
        }"#;
        let input: HookInput = serde_json::from_str(json).expect("deserialize");
        assert!(input.tool_name.is_none());
        assert!(input.tool_input.is_none());
        assert_eq!(input.event, HookEvent::SessionStart);
        assert_eq!(input.source.as_deref(), Some("startup"));
    }

    #[test]
    fn test_hook_input_roundtrip_with_all_fields() {
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file": "test.rs"})),
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: Some("/project".into()),
            source: None,
            transcript_path: None,
        };
        let json = serde_json::to_string(&input).expect("serialize");
        let parsed: HookInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.tool_name.as_deref(), Some("Edit"));
        assert_eq!(parsed.event, HookEvent::PostToolUse);
    }

    #[test]
    fn test_hook_input_minimal() {
        let json = r#"{"event": "session_end"}"#;
        let input: HookInput = serde_json::from_str(json).expect("deserialize");
        assert!(input.tool_name.is_none());
        assert!(input.tool_input.is_none());
        assert!(input.session_id.is_none());
        assert!(input.source.is_none());
        assert_eq!(input.event, HookEvent::SessionEnd);
    }

    #[test]
    fn test_hook_output_exit_codes() {
        assert_eq!(HookOutput::Allow.exit_code(), 0);
        assert_eq!(
            HookOutput::Block {
                reason: "test".into(),
                category: None,
            }
            .exit_code(),
            2
        );
        assert_eq!(
            HookOutput::Warn {
                message: "test".into(),
            }
            .exit_code(),
            0
        );
    }

    #[test]
    fn test_hook_output_variants() {
        let allow = HookOutput::Allow;
        assert!(matches!(allow, HookOutput::Allow));

        let block = HookOutput::Block {
            reason: "missing sentinel".to_string(),
            category: Some(BlockCategory::Gate),
        };
        assert!(matches!(block, HookOutput::Block { .. }));

        let warn = HookOutput::Warn {
            message: "deprecated usage".to_string(),
        };
        assert!(matches!(warn, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_hook_event_serde_roundtrip() {
        let event = HookEvent::PreToolUse;
        let json = serde_json::to_string(&event).expect("serialize");
        assert_eq!(json, "\"pre_tool_use\"");
        let parsed: HookEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, event);
    }

    #[test]
    fn test_hook_event_session_start() {
        let json = "\"session_start\"";
        let event: HookEvent = serde_json::from_str(json).expect("deserialize");
        assert_eq!(event, HookEvent::SessionStart);
    }

    #[test]
    fn test_hook_event_session_end() {
        let json = "\"session_end\"";
        let event: HookEvent = serde_json::from_str(json).expect("deserialize");
        assert_eq!(event, HookEvent::SessionEnd);
    }

    #[test]
    fn test_pathflow_team_info_deserialize() {
        let json = r#"{"lead_pid": 12345, "team_name": "codeflow-team"}"#;
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.lead_pid, 12345);
        assert_eq!(info.team_name, "codeflow-team");
    }

    #[test]
    fn test_pathflow_team_info_defaults() {
        let json = r#"{}"#;
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.lead_pid, 0);
        assert_eq!(info.team_name, "");
    }

    #[test]
    fn test_session_meta_roundtrip() {
        let meta = SessionMeta {
            session_id: "ses-test".into(),
            created_at: "2026-03-10T00:00:00Z".into(),
            source: "startup".into(),
            ppid: 1234,
            version: "dev".into(),
            permission_mode: Some("default".into()),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let parsed: SessionMeta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.session_id, "ses-test");
        assert_eq!(parsed.ppid, 1234);
        assert_eq!(parsed.permission_mode.as_deref(), Some("default"));
    }

    #[test]
    fn test_session_meta_defaults() {
        let json = r#"{}"#;
        let meta: SessionMeta = serde_json::from_str(json).expect("deserialize");
        assert_eq!(meta.session_id, "");
        assert_eq!(meta.created_at, "");
        assert_eq!(meta.source, "");
        assert_eq!(meta.ppid, 0);
        assert_eq!(meta.version, "");
        assert!(meta.permission_mode.is_none());
    }

    #[test]
    fn test_session_meta_without_permission_mode() {
        let meta = SessionMeta {
            session_id: "ses-test".into(),
            created_at: "2026-03-10T00:00:00Z".into(),
            source: "startup".into(),
            ppid: 1234,
            version: "dev".into(),
            permission_mode: None,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        // permission_mode should be skipped entirely in JSON
        assert!(!json.contains("permission_mode"));
        let parsed: SessionMeta = serde_json::from_str(&json).expect("deserialize");
        assert!(parsed.permission_mode.is_none());
    }

    #[test]
    fn test_block_category_serde_roundtrip_all_variants() {
        let variants = [
            (BlockCategory::Gate, "\"gate\""),
            (BlockCategory::Security, "\"security\""),
            (BlockCategory::TeamGuard, "\"team_guard\""),
            (BlockCategory::EditWriteScope, "\"edit_write_scope\""),
            (BlockCategory::GhPrGuard, "\"gh_pr_guard\""),
            (BlockCategory::ProtectedResource, "\"protected_resource\""),
            (BlockCategory::WebFetch, "\"web_fetch\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).expect("serialize");
            assert_eq!(&json, expected_json, "serialize mismatch for {variant:?}");
            let parsed: BlockCategory = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, variant, "roundtrip mismatch for {variant:?}");
        }
    }

    #[test]
    fn test_hook_output_block_with_all_categories() {
        let categories = [
            BlockCategory::Gate,
            BlockCategory::Security,
            BlockCategory::TeamGuard,
            BlockCategory::EditWriteScope,
            BlockCategory::GhPrGuard,
            BlockCategory::ProtectedResource,
            BlockCategory::WebFetch,
        ];
        for cat in &categories {
            let output = HookOutput::Block {
                reason: format!("blocked by {cat:?}"),
                category: Some(*cat),
            };
            assert_eq!(output.exit_code(), 2);
        }
    }

    #[test]
    fn test_hook_output_block_without_category() {
        let output = HookOutput::Block {
            reason: "generic block".into(),
            category: None,
        };
        assert_eq!(output.exit_code(), 2);
    }

    #[test]
    fn test_hook_event_all_variants_serde() {
        let variants = [
            (HookEvent::PreToolUse, "\"pre_tool_use\""),
            (HookEvent::PostToolUse, "\"post_tool_use\""),
            (HookEvent::TaskCompleted, "\"task_completed\""),
            (HookEvent::SessionStart, "\"session_start\""),
            (HookEvent::SessionEnd, "\"session_end\""),
            (HookEvent::Stop, "\"stop\""),
            (HookEvent::UserPromptSubmit, "\"user_prompt_submit\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).expect("serialize");
            assert_eq!(&json, expected_json, "serialize mismatch for {variant:?}");
            let parsed: HookEvent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&parsed, variant, "roundtrip mismatch for {variant:?}");
        }
    }

    #[test]
    fn test_os_process_checker_pid_zero() {
        let checker = OsProcessChecker;
        // PID 0 should always return false (guarded in is_alive).
        assert!(!checker.is_alive(0));
    }

    #[test]
    fn test_os_process_checker_nonexistent_pid() {
        let checker = OsProcessChecker;
        // PID 4_294_967_295 is extremely unlikely to exist.
        assert!(!checker.is_alive(4_294_967_295));
    }

    #[test]
    fn test_os_process_checker_current_process() {
        let checker = OsProcessChecker;
        // Our own PID should be alive.
        let my_pid = std::process::id();
        assert!(checker.is_alive(my_pid));
    }

    #[test]
    fn test_libc_signal_zero_nonexistent() {
        // PID 4_294_967_295 should not exist on any system.
        assert!(!libc_signal_zero(4_294_967_295));
    }

    #[test]
    fn test_libc_signal_zero_self() {
        let my_pid = std::process::id();
        assert!(libc_signal_zero(my_pid));
    }

    #[test]
    fn test_os_tmux_checker_empty_pane_id() {
        let checker = OsTmuxChecker;
        assert!(!checker.is_pane_alive(""));
    }

    #[test]
    fn test_os_tmux_checker_nonexistent_pane() {
        let checker = OsTmuxChecker;
        // A pane ID that doesn't exist should return false.
        assert!(!checker.is_pane_alive("%99999"));
    }

    #[test]
    fn test_os_tmux_checker_list_panes_returns_vec() {
        let checker = OsTmuxChecker;
        // list_panes should not error even if tmux is not running
        // (returns empty vec on command failure).
        let result = checker.list_panes();
        assert!(result.is_ok());
    }

    #[test]
    fn test_hook_input_with_transcript_path() {
        let json = r#"{
            "event": "session_end",
            "session_id": "ses-abc123",
            "transcript_path": "/tmp/transcript.jsonl"
        }"#;
        let input: HookInput = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            input.transcript_path.as_deref(),
            Some("/tmp/transcript.jsonl")
        );
    }

    #[test]
    fn test_hook_input_skip_serializing_none_fields() {
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let json = serde_json::to_string(&input).expect("serialize");
        // None fields with skip_serializing_if should not appear.
        assert!(!json.contains("tool_name"));
        assert!(!json.contains("tool_input"));
        assert!(!json.contains("session_id"));
        assert!(!json.contains("project_dir"));
        assert!(!json.contains("source"));
        assert!(!json.contains("transcript_path"));
        // But event should always be present.
        assert!(json.contains("session_start"));
    }
}
