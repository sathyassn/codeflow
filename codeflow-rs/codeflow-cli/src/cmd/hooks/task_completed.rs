//! Task completed hook handler: 1 handler matching Go CLI.

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Task completed handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum TaskCompletedHandler {
    /// Checkpoint completion tracking
    #[command(name = "checkpoint-complete")]
    CheckpointComplete,
}

pub fn run(handler: TaskCompletedHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: TaskCompletedHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        TaskCompletedHandler::CheckpointComplete => {
            Box::new(codeflow_core::hooks::task_completed::CheckpointComplete::new(project_dir))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskCompletedHandler, build_handler};

    #[test]
    fn test_build_handler_checkpoint_complete_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(
            TaskCompletedHandler::CheckpointComplete,
            dir.path().to_path_buf(),
        );
        assert_eq!(h.name(), "checkpoint-complete");
    }

    #[test]
    fn test_build_handler_checkpoint_complete_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(
            TaskCompletedHandler::CheckpointComplete,
            dir.path().to_path_buf(),
        );
        assert!(
            h.events()
                .contains(&codeflow_core::HookEvent::TaskCompleted),
            "checkpoint-complete should handle TaskCompleted events"
        );
    }

    #[test]
    fn test_build_handler_checkpoint_complete_handle() {
        let dir = tempfile::tempdir().unwrap();
        // Create checkpoint state directory.
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("logs")).unwrap();
        let h = build_handler(
            TaskCompletedHandler::CheckpointComplete,
            dir.path().to_path_buf(),
        );
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::TaskCompleted,
            session_id: Some("ses-testtaskcomplete12345678".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        assert!(result.is_ok());
    }
}
