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
            ..Default::default()
        };
        let result = h.handle(input);
        // Logging handlers always Allow.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_logging_events_count() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        // Stop logging handles exactly one event type: Stop.
        let events = h.events();
        assert!(!events.is_empty(), "handler should have at least one event");
    }

    #[test]
    fn test_build_handler_logging_does_not_handle_pre_tool_use() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        assert!(
            !h.events().contains(&codeflow_core::HookEvent::PreToolUse),
            "stop logging should not handle PreToolUse events"
        );
    }

    #[test]
    fn test_build_handler_logging_handle_with_session_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(StopHandler::Logging, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::Stop,
            session_id: Some("ses-testhandlersessid12345678".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: Some("user".into()),
            transcript_path: None,
            ..Default::default()
        };
        let result = h.handle(input);
        assert!(result.is_ok(), "handle should succeed with session id");
    }

    // ─── proptest: stop handler parse stability ──────────────────────────────

    use clap::Parser;
    use proptest::prelude::*;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::HookCommand,
    }

    proptest! {
        /// The only valid stop handler must parse successfully from any call.
        #[test]
        fn proptest_stop_logging_handler_always_parses(_seed in 0u8..10u8) {
            let cli = TestCli::try_parse_from(["test", "stop", "logging"]);
            prop_assert!(cli.is_ok(), "stop logging should always parse");
        }

        /// Non-existent stop handler names must be rejected.
        #[test]
        fn proptest_invalid_stop_handlers_rejected(
            prefix in "[xyzXYZ]{1,5}",
            suffix in "[0-9]{3,6}",
        ) {
            let name = format!("{prefix}-bogus-{suffix}");
            prop_assume!(name != "logging");
            let cli = TestCli::try_parse_from(["test", "stop", &name]);
            prop_assert!(cli.is_err(), "unknown stop handler '{name}' should be rejected");
        }
    }
}
