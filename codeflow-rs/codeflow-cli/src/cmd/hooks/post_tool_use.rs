//! Post-tool-use hook handlers: 4 handlers matching Go CLI.

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Post-tool-use handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum PostToolUseHandler {
    /// Create stage sentinels on STAGE-COMPLETE messages
    #[command(name = "sentinel-write")]
    SentinelWrite,
    /// Validate settings.json consistency
    #[command(name = "settings-validate")]
    SettingsValidate,
    /// Register PF tasks in checkpoint
    #[command(name = "checkpoint-register")]
    CheckpointRegister,
    /// Post-tool-use logging
    Logging,
}

pub fn run(handler: PostToolUseHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: PostToolUseHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        PostToolUseHandler::SentinelWrite => Box::new(
            codeflow_core::hooks::post_tool_use::SentinelWrite::new(project_dir),
        ),
        PostToolUseHandler::SettingsValidate => Box::new(
            codeflow_core::hooks::post_tool_use::SettingsValidate::new(project_dir),
        ),
        PostToolUseHandler::CheckpointRegister => {
            Box::new(codeflow_core::hooks::post_tool_use::CheckpointRegister::new(project_dir))
        }
        PostToolUseHandler::Logging => Box::new(
            codeflow_core::hooks::logging::ToolUseLogging::new(project_dir),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{PostToolUseHandler, build_handler};

    fn make_input(dir: &std::path::Path) -> codeflow_core::HookInput {
        codeflow_core::HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: codeflow_core::HookEvent::PostToolUse,
            session_id: Some("ses-testposttooluse12345678".into()),
            project_dir: Some(dir.to_string_lossy().into()),
            source: None,
            transcript_path: None,
        }
    }

    #[test]
    fn test_build_handler_sentinel_write() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PostToolUseHandler::SentinelWrite, dir.path().to_path_buf());
        assert_eq!(h.name(), "sentinel-write");
    }

    #[test]
    fn test_build_handler_settings_validate() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(
            PostToolUseHandler::SettingsValidate,
            dir.path().to_path_buf(),
        );
        assert_eq!(h.name(), "settings-validate");
    }

    #[test]
    fn test_build_handler_checkpoint_register() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(
            PostToolUseHandler::CheckpointRegister,
            dir.path().to_path_buf(),
        );
        assert_eq!(h.name(), "checkpoint-register");
    }

    #[test]
    fn test_build_handler_logging() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PostToolUseHandler::Logging, dir.path().to_path_buf());
        assert_eq!(h.name(), "post-tool-use-logging");
    }

    #[test]
    fn test_sentinel_write_handle_allows_non_matching_input() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(PostToolUseHandler::SentinelWrite, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_logging_handle_allows() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(PostToolUseHandler::Logging, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }
}
