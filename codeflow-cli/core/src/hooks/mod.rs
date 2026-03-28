//! Hook handler infrastructure for Claude Code lifecycle events.
//!
//! Provides the `HookHandler` trait and supporting types (`HookInput`,
//! `HookOutput`, `HookEvent`) that all hook implementations build on.
//! Session-specific hooks live in submodules.

pub mod logging;
pub mod pipeline;
pub mod post_tool_use;
pub mod pre_tool_use;
pub mod prompt_validate;
pub mod security;
pub mod session_end;
pub mod session_start;
pub mod task_completed;

use serde::{Deserialize, Serialize};

use crate::error::HookError;

/// Hook lifecycle events (maps to Claude Code hook events).
///
/// Serializes as `snake_case` (internal format). Deserializes from both
/// `snake_case` (internal/tests) and `PascalCase` (Claude Code stdin sends
/// `hook_event_name` with PascalCase values like `"SessionStart"`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    #[default]
    #[serde(alias = "PreToolUse")]
    PreToolUse,
    #[serde(alias = "PostToolUse")]
    PostToolUse,
    #[serde(alias = "TaskCompleted")]
    TaskCompleted,
    #[serde(alias = "SessionStart")]
    SessionStart,
    #[serde(alias = "SessionEnd")]
    SessionEnd,
    #[serde(alias = "Stop")]
    Stop,
    #[serde(alias = "UserPromptSubmit")]
    UserPromptSubmit,
}

/// Typed input for hook handlers.
///
/// Tool-use fields (`tool_name`, `tool_input`) are optional because session
/// events do not carry tool context. The `source` field is session-specific
/// (startup, resume, compact, clear).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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
    ///
    /// Claude Code sends this as `hook_event_name` in stdin JSON.
    /// Internal tests use `event`. The alias accepts both.
    #[serde(alias = "hook_event_name")]
    pub event: HookEvent,

    /// Session ID (from environment or stdin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Project root directory.
    ///
    /// Claude Code sends this as `cwd` in stdin JSON.
    /// Internal tests use `project_dir`. The alias accepts both.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "cwd")]
    pub project_dir: Option<String>,

    /// Session source type: startup, resume, compact, clear, or unknown.
    /// Only present for `SessionStart` / `SessionEnd` events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Path to session transcript (`SessionEnd` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,

    /// Task subject (`TaskCompleted` events).
    /// Claude Code sends this as a top-level field, not nested in `tool_input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_subject: Option<String>,

    /// Task ID (`TaskCompleted` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// Task description (`TaskCompleted` events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_description: Option<String>,
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

/// JSON structure for `pathflow-team.json`.
///
/// Stores team composition and process tracking. Created at `TeamCreate`,
/// deleted at `TeamDelete`. The `lead_pid` is the Claude Code process PID
/// of the lead (obtained via `parent_id()` in the hook process).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathflowTeamInfo {
    /// Team name for Claude Code teams.
    #[serde(default)]
    pub team_name: String,
    /// The `CODEFLOW_SESSION_ID` at team creation time.
    #[serde(default)]
    pub codeflow_session_id: String,
    /// Claude Code PID of the team lead (for liveness checking).
    /// Obtained via `std::os::unix::process::parent_id()` in the hook process.
    #[serde(default)]
    pub lead_pid: u32,
    /// Whether at least one teammate has been spawned.
    #[serde(default)]
    pub teammate_spawned: bool,
    /// RFC 3339 timestamp of team creation.
    #[serde(default)]
    pub created_at: String,
    /// Name of the last spawned teammate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_spawn_name: Option<String>,
    /// Registry of spawned teammates with name, PID, and spawn time.
    /// PID is 0 until the teammate's own SessionStart updates it (tmux only).
    /// In-process teammates never fire SessionStart, so PID stays 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teammates: Vec<TeammateEntry>,
}

/// A single teammate entry in the team registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeammateEntry {
    /// Teammate name (e.g., "cf-development", "cf-review").
    pub name: String,
    /// Claude Code PID of the teammate process. Set to 0 by the lead at spawn
    /// time, updated to the actual PID by the teammate's SessionStart hook
    /// (tmux mode only — in-process teammates don't fire SessionStart).
    #[serde(default)]
    pub pid: u32,
    /// RFC 3339 timestamp of when the teammate was spawned.
    #[serde(default)]
    pub spawned_at: String,
    /// Model used for the teammate (e.g., "opus", "sonnet").
    /// Only populated for Agent tool spawns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Subagent type (e.g., "general-purpose").
    /// Only populated for Agent tool spawns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Backend type from Claude Code team config (e.g., "in-process", "tmux").
    /// Used by teammate detection to distinguish pending tmux spawns from
    /// in-process teammates (which never fire SessionStart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<String>,
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
            task_subject: Some("PF1-TSK-01: Init".into()),
            task_id: Some("task-42".into()),
            task_description: Some("Initialize session".into()),
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
        let json = r#"{"team_name": "codeflow-team"}"#;
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.team_name, "codeflow-team");
        assert_eq!(info.codeflow_session_id, "");
        assert!(!info.teammate_spawned);
        assert_eq!(info.created_at, "");
        assert!(info.last_spawn_name.is_none());
    }

    #[test]
    fn test_pathflow_team_info_full_deserialize() {
        let json = r#"{
            "team_name": "codeflow-team",
            "codeflow_session_id": "ses-abc123",
            "teammate_spawned": true,
            "created_at": "2026-03-10T00:00:00Z",
            "last_spawn_name": "cf-development"
        }"#;
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.team_name, "codeflow-team");
        assert_eq!(info.codeflow_session_id, "ses-abc123");
        assert!(info.teammate_spawned);
        assert_eq!(info.created_at, "2026-03-10T00:00:00Z");
        assert_eq!(info.last_spawn_name.as_deref(), Some("cf-development"));
    }

    #[test]
    fn test_pathflow_team_info_defaults() {
        let json = r"{}";
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.team_name, "");
        assert_eq!(info.codeflow_session_id, "");
        assert!(!info.teammate_spawned);
        assert_eq!(info.created_at, "");
        assert!(info.last_spawn_name.is_none());
    }

    #[test]
    fn test_pathflow_team_info_skip_serializing_none_last_spawn() {
        let info = PathflowTeamInfo {
            team_name: "t".into(),
            codeflow_session_id: "ses-x".into(),
            lead_pid: 0,
            teammate_spawned: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            last_spawn_name: None,
            teammates: vec![],
        };
        let json = serde_json::to_string(&info).expect("serialize");
        assert!(!json.contains("last_spawn_name"));
        assert!(!json.contains("teammates"));
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
    fn test_teammate_entry_with_optional_fields() {
        let json = r#"{
            "name": "cf-development",
            "pid": 0,
            "spawned_at": "2026-03-10T00:00:00Z",
            "model": "opus",
            "subagent_type": "general-purpose"
        }"#;
        let entry: TeammateEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(entry.name, "cf-development");
        assert_eq!(entry.model.as_deref(), Some("opus"));
        assert_eq!(entry.subagent_type.as_deref(), Some("general-purpose"));
    }

    #[test]
    fn test_teammate_entry_without_optional_fields() {
        let json = r#"{
            "name": "cf-security",
            "pid": 1234,
            "spawned_at": "2026-03-10T00:00:00Z"
        }"#;
        let entry: TeammateEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(entry.name, "cf-security");
        assert_eq!(entry.pid, 1234);
        assert!(entry.model.is_none());
        assert!(entry.subagent_type.is_none());
    }

    #[test]
    fn test_teammate_entry_skip_serializing_none_optional_fields() {
        let entry = TeammateEntry {
            name: "cf-review".into(),
            pid: 0,
            spawned_at: "2026-03-10T00:00:00Z".into(),
            model: None,
            subagent_type: None,
            backend_type: None,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(!json.contains("model"));
        assert!(!json.contains("subagent_type"));
    }

    #[test]
    fn test_teammate_entry_serialize_with_optional_fields() {
        let entry = TeammateEntry {
            name: "cf-development".into(),
            pid: 0,
            spawned_at: "2026-03-10T00:00:00Z".into(),
            model: Some("opus".into()),
            subagent_type: Some("general-purpose".into()),
            backend_type: None,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"model\":\"opus\""));
        assert!(json.contains("\"subagent_type\":\"general-purpose\""));
    }

    #[test]
    fn test_teammate_entry_backward_compat_roundtrip() {
        // Old format without new fields should deserialize and re-serialize cleanly.
        let old_json = r#"{"name":"cf-git","pid":42,"spawned_at":"2026-01-01T00:00:00Z"}"#;
        let entry: TeammateEntry = serde_json::from_str(old_json).expect("deserialize");
        assert_eq!(entry.name, "cf-git");
        assert_eq!(entry.pid, 42);
        assert!(entry.model.is_none());
        assert!(entry.subagent_type.is_none());
        let reserialized = serde_json::to_string(&entry).expect("serialize");
        assert!(!reserialized.contains("model"));
        assert!(!reserialized.contains("subagent_type"));
    }

    #[test]
    fn test_pathflow_team_info_with_enriched_teammates() {
        let json = r#"{
            "team_name": "test-team",
            "teammate_spawned": true,
            "teammates": [
                {
                    "name": "cf-development",
                    "pid": 0,
                    "spawned_at": "2026-03-10T00:00:00Z",
                    "model": "opus",
                    "subagent_type": "general-purpose"
                },
                {
                    "name": "cf-security",
                    "pid": 1234,
                    "spawned_at": "2026-03-10T00:00:00Z"
                }
            ]
        }"#;
        let info: PathflowTeamInfo = serde_json::from_str(json).expect("deserialize");
        assert_eq!(info.teammates.len(), 2);
        assert_eq!(info.teammates[0].model.as_deref(), Some("opus"));
        assert!(info.teammates[1].model.is_none());
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
            ..Default::default()
        };
        let json = serde_json::to_string(&input).expect("serialize");
        // None fields with skip_serializing_if should not appear.
        assert!(!json.contains("tool_name"));
        assert!(!json.contains("tool_input"));
        assert!(!json.contains("session_id"));
        assert!(!json.contains("project_dir"));
        assert!(!json.contains("source"));
        assert!(!json.contains("transcript_path"));
        assert!(!json.contains("task_subject"));
        assert!(!json.contains("task_id"));
        assert!(!json.contains("task_description"));
        // But event should always be present.
        assert!(json.contains("session_start"));
    }
}
