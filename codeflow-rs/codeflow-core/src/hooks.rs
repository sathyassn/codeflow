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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    /// The tool name from Claude Code (e.g., "Bash", "Edit", "Write", "Task").
    pub tool_name: String,

    /// The tool input payload as raw JSON.
    pub tool_input: serde_json::Value,

    /// The hook event type.
    pub event: HookEvent,

    /// Session ID (from environment or stdin).
    pub session_id: Option<String>,

    /// Project root directory.
    pub project_dir: Option<String>,
}

/// Result of a hook handler evaluation.
#[derive(Debug, Clone)]
pub enum HookOutput {
    /// The operation is allowed to proceed.
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
    fn name(&self) -> &str;

    /// Which hook events this handler responds to.
    fn events(&self) -> &[HookEvent];
}
