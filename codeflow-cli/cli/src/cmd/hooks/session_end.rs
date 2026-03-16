//! Session end hook handlers: 2 handlers matching Go CLI.

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Session end handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SessionEndHandler {
    /// Session cleanup
    Cleanup,
    /// Session end logging
    Logging,
}

/// ISO 8601 UTC timestamp for session end events.
fn now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();

    let days = total_secs / 86_400;
    let day_secs = total_secs % 86_400;
    let hours = day_secs / 3_600;
    let minutes = (day_secs % 3_600) / 60;
    let seconds = day_secs % 60;

    let (year, month, day) = super::session_start::days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

pub fn run(handler: SessionEndHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: SessionEndHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        SessionEndHandler::Cleanup => {
            let lead_pid = std::os::unix::process::parent_id();
            let home_dir = std::env::var("HOME").map_or_else(
                |_| std::path::PathBuf::from("/tmp"),
                std::path::PathBuf::from,
            );
            Box::new(codeflow_core::hooks::session_end::SessionEndCleanup {
                home_dir,
                lead_pid,
                now,
            })
        }
        SessionEndHandler::Logging => Box::new(
            codeflow_core::hooks::logging::SessionEndLogging::new(project_dir),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_returns_iso8601() {
        let ts = now();
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert_eq!(ts.len(), 20, "timestamp should be 20 chars: {ts}");
    }

    #[test]
    fn test_now_format_structure() {
        let ts = now();
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn test_build_handler_cleanup_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionEndHandler::Cleanup, dir.path().to_path_buf());
        assert_eq!(h.name(), "session-end-cleanup");
    }

    #[test]
    fn test_build_handler_logging_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionEndHandler::Logging, dir.path().to_path_buf());
        assert_eq!(h.name(), "session-end-logging");
    }

    #[test]
    fn test_build_handler_cleanup_events() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(SessionEndHandler::Cleanup, dir.path().to_path_buf());
        assert!(
            h.events().contains(&codeflow_core::HookEvent::SessionEnd),
            "cleanup handler should handle SessionEnd events"
        );
    }

    #[test]
    fn test_build_handler_logging_handle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(SessionEndHandler::Logging, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::SessionEnd,
            session_id: Some("ses-testsessionendlog12345".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_cleanup_handle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state").join("logs")).unwrap();
        let h = build_handler(SessionEndHandler::Cleanup, dir.path().to_path_buf());
        let input = codeflow_core::HookInput {
            tool_name: None,
            tool_input: None,
            event: codeflow_core::HookEvent::SessionEnd,
            session_id: Some("ses-testsessionendclean123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        let result = h.handle(input);
        assert!(result.is_ok());
    }
}
