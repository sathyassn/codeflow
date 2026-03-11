//! Stop hook handler: 1 handler matching Go CLI.

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Stop handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum StopHandler {
    /// Stop event logging
    Logging,
}

pub fn run(handler: StopHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: StopHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        StopHandler::Logging => {
            Box::new(codeflow_core::hooks::logging::StopLogging::new(project_dir))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StopHandler, build_handler};

    #[test]
    fn test_build_handler_logging_returns_correct_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        assert_eq!(h.name(), "stop-logging");
    }

    #[test]
    fn test_build_handler_logging_handles_stop_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        assert!(
            h.events().contains(&codeflow_core::HookEvent::Stop),
            "stop logging should handle Stop events"
        );
    }

    #[test]
    fn test_build_handler_logging_handle_with_valid_input() {
        let dir = tempfile::tempdir().unwrap();
        // Create minimal state directory for handler.
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::Stop,
            session_id: Some("ses-teststoplogging12345678".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        // Logging handlers always Allow.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }
}
