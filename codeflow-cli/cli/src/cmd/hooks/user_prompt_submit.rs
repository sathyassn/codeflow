//! User prompt submit hook handlers: 2 handlers matching Go CLI.
//!
//! - `validate`: outputs context reminders via `PromptValidator` in codeflow-core
//!   (matches Go's `internal/hooks/prompt/validate.go`)
//! - `logging`: logs prompt events via `PromptLogging`

use anyhow::Result;
use clap::Subcommand;
use codeflow_core::HookHandler;

use crate::helpers;

/// User prompt submit handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum UserPromptSubmitHandler {
    /// Validate user prompt
    Validate,
    /// User prompt logging
    Logging,
}

pub fn run(handler: UserPromptSubmitHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: UserPromptSubmitHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn HookHandler> {
    match handler {
        UserPromptSubmitHandler::Validate => Box::new(
            codeflow_core::hooks::prompt_validate::new_prompt_validator(project_dir),
        ),
        UserPromptSubmitHandler::Logging => Box::new(
            codeflow_core::hooks::logging::PromptLogging::new(project_dir),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::{HookEvent, HookInput, HookOutput};

    #[test]
    fn test_build_handler_validate_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(UserPromptSubmitHandler::Validate, dir.path().to_path_buf());
        assert_eq!(h.name(), "prompt-validate");
    }

    #[test]
    fn test_build_handler_logging_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(UserPromptSubmitHandler::Logging, dir.path().to_path_buf());
        assert_eq!(h.name(), "user-prompt-logging");
    }

    #[test]
    fn test_build_handler_validate_always_allows() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(UserPromptSubmitHandler::Validate, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::UserPromptSubmit,
            session_id: Some("ses-testpromptvalidateall".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_validate_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(UserPromptSubmitHandler::Validate, dir.path().to_path_buf());
        assert!(
            h.events().contains(&HookEvent::UserPromptSubmit),
            "prompt validate should handle UserPromptSubmit events"
        );
    }

    #[test]
    fn test_build_handler_logging_handle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(UserPromptSubmitHandler::Logging, dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::UserPromptSubmit,
            session_id: Some("ses-testpromptlogginghand".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_logging_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(UserPromptSubmitHandler::Logging, dir.path().to_path_buf());
        assert!(
            h.events().contains(&HookEvent::UserPromptSubmit),
            "prompt logging should handle UserPromptSubmit events"
        );
    }
}
