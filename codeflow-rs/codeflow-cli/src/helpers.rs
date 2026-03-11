//! CLI helper utilities for project directory detection, session resolution,
//! stdin reading, and hook handler dispatch.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use codeflow_core::{HookHandler, HookInput, HookOutput};

use crate::exit::{EXIT_HOOK_BLOCK, EXIT_SUCCESS, ExitError};

/// Detect the project root directory.
///
/// Resolution order:
/// 1. `CF_PROJECT_ROOT` environment variable
/// 2. Walk up from current directory looking for `.claude/` or `.codeflow/`
///
/// Returns the absolute path to the project root.
pub fn detect_project_dir() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("CF_PROJECT_ROOT") {
        let path = PathBuf::from(&root);
        if path.is_dir() {
            return Ok(path);
        }
    }

    let cwd = std::env::current_dir().context("getting current directory")?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".claude").is_dir() || dir.join(".codeflow").is_dir() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    // Fall back to cwd if no markers found.
    Ok(cwd)
}

/// Resolve the current session ID from environment or state files.
pub fn resolve_session_id(state_dir: &Path) -> Result<String> {
    let sid =
        codeflow_core::session::current_session_id(state_dir).context("resolving session ID")?;
    Ok(sid.as_str().to_string())
}

/// Read all of stdin into a string.
///
/// Returns an empty string if stdin is not available or empty.
#[allow(dead_code)]
pub fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}

/// Run a hook handler: parse stdin JSON, call handler, map output to exit code.
///
/// This implements the graceful degradation contract: parse/read errors
/// result in exit 0 (allow), never blocking on infrastructure errors.
pub fn run_hook_handler(handler: &dyn HookHandler) -> ! {
    let result = run_hook_handler_inner(handler);
    match result {
        Ok(code) => std::process::exit(code),
        Err(_) => {
            // Graceful degradation: infrastructure errors never block.
            std::process::exit(EXIT_SUCCESS);
        }
    }
}

/// Inner hook handler logic, returning the exit code or an error.
fn run_hook_handler_inner(handler: &dyn HookHandler) -> Result<i32, ExitError> {
    let stdin = read_stdin_for_hook()?;
    process_hook_input(handler, &stdin)
}

/// Process hook handler from a pre-read input string, returning exit code.
fn process_hook_input(handler: &dyn HookHandler, stdin: &str) -> Result<i32, ExitError> {
    let input = parse_hook_input(stdin)?;
    let output = execute_hook_handler(handler, input)?;
    Ok(map_hook_output(&output))
}

/// Read stdin for hook processing. Returns empty string on failure (graceful).
fn read_stdin_for_hook() -> Result<String, ExitError> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| ExitError::new(EXIT_SUCCESS, format!("stdin read error: {e}")))?;
    Ok(buf)
}

/// Parse JSON input for a hook handler. Returns exit 0 on parse failure.
fn parse_hook_input(stdin: &str) -> Result<HookInput, ExitError> {
    if stdin.trim().is_empty() {
        return Err(ExitError::new(EXIT_SUCCESS, String::new()));
    }
    serde_json::from_str(stdin)
        .map_err(|e| ExitError::new(EXIT_SUCCESS, format!("json parse error: {e}")))
}

/// Execute the hook handler. Returns exit 0 on handler error (graceful).
fn execute_hook_handler(
    handler: &dyn HookHandler,
    input: HookInput,
) -> Result<HookOutput, ExitError> {
    handler
        .handle(input)
        .map_err(|e| ExitError::new(EXIT_SUCCESS, format!("handler error: {e}")))
}

/// Map `HookOutput` to exit code, printing messages to stderr as needed.
fn map_hook_output(output: &HookOutput) -> i32 {
    match output {
        HookOutput::Allow => EXIT_SUCCESS,
        HookOutput::Block { reason, .. } => {
            eprintln!("{reason}");
            EXIT_HOOK_BLOCK
        }
        HookOutput::Warn { message } => {
            eprintln!("{message}");
            EXIT_SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::{HookError, HookEvent};

    struct AllowHandler;
    impl HookHandler for AllowHandler {
        fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
            Ok(HookOutput::Allow)
        }
        fn name(&self) -> &'static str {
            "allow-test"
        }
        fn events(&self) -> &[HookEvent] {
            &[HookEvent::PreToolUse]
        }
    }

    struct BlockHandler;
    impl HookHandler for BlockHandler {
        fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
            Ok(HookOutput::Block {
                reason: "test block".into(),
                category: None,
            })
        }
        fn name(&self) -> &'static str {
            "block-test"
        }
        fn events(&self) -> &[HookEvent] {
            &[HookEvent::PreToolUse]
        }
    }

    struct ErrorHandler;
    impl HookHandler for ErrorHandler {
        fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
            Err(HookError::Config("test error".into()))
        }
        fn name(&self) -> &'static str {
            "error-test"
        }
        fn events(&self) -> &[HookEvent] {
            &[HookEvent::PreToolUse]
        }
    }

    fn make_input() -> HookInput {
        HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-test123".into()),
            project_dir: Some("/tmp/test".into()),
            source: None,
            transcript_path: None,
        }
    }

    #[test]
    fn test_detect_project_dir_returns_path() {
        // Should return a valid path (either from env or cwd).
        let result = detect_project_dir();
        assert!(result.is_ok());
        assert!(result.unwrap().is_dir());
    }

    #[test]
    fn test_parse_hook_input_valid_json() {
        let json = r#"{"event": "pre_tool_use", "tool_name": "Bash"}"#;
        let result = parse_hook_input(json);
        assert!(result.is_ok());
        let input = result.unwrap();
        assert_eq!(input.event, HookEvent::PreToolUse);
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn test_parse_hook_input_empty_string() {
        let result = parse_hook_input("");
        assert!(result.is_err());
        // Should be exit 0 (graceful).
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_parse_hook_input_invalid_json() {
        let result = parse_hook_input("not json");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_execute_hook_handler_allow() {
        let handler = AllowHandler;
        let input = make_input();
        let result = execute_hook_handler(&handler, input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Allow));
    }

    #[test]
    fn test_execute_hook_handler_block() {
        let handler = BlockHandler;
        let input = make_input();
        let result = execute_hook_handler(&handler, input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Block { .. }));
    }

    #[test]
    fn test_execute_hook_handler_error_returns_exit_success() {
        let handler = ErrorHandler;
        let input = make_input();
        let result = execute_hook_handler(&handler, input);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_resolve_session_id_nonexistent_dir() {
        let result = resolve_session_id(Path::new("/nonexistent/state/dir"));
        assert!(result.is_err());
    }

    #[test]
    fn test_map_hook_output_allow() {
        let output = HookOutput::Allow;
        assert_eq!(map_hook_output(&output), EXIT_SUCCESS);
    }

    #[test]
    fn test_map_hook_output_block() {
        let output = HookOutput::Block {
            reason: "blocked".into(),
            category: None,
        };
        assert_eq!(map_hook_output(&output), EXIT_HOOK_BLOCK);
    }

    #[test]
    fn test_map_hook_output_warn() {
        let output = HookOutput::Warn {
            message: "warning".into(),
        };
        assert_eq!(map_hook_output(&output), EXIT_SUCCESS);
    }

    struct WarnHandler;
    impl HookHandler for WarnHandler {
        fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
            Ok(HookOutput::Warn {
                message: "test warning".into(),
            })
        }
        fn name(&self) -> &'static str {
            "warn-test"
        }
        fn events(&self) -> &[HookEvent] {
            &[HookEvent::PreToolUse]
        }
    }

    #[test]
    fn test_execute_hook_handler_warn() {
        let handler = WarnHandler;
        let input = make_input();
        let result = execute_hook_handler(&handler, input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Warn { .. }));
    }

    #[test]
    fn test_parse_hook_input_whitespace_only() {
        let result = parse_hook_input("   \n  ");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_parse_hook_input_with_all_fields() {
        let json = r#"{"event":"post_tool_use","tool_name":"Edit","tool_input":{"file":"/tmp/a.rs"},"session_id":"ses-123","project_dir":"/project"}"#;
        let result = parse_hook_input(json);
        assert!(result.is_ok());
        let input = result.unwrap();
        assert_eq!(input.event, HookEvent::PostToolUse);
        assert_eq!(input.tool_name.as_deref(), Some("Edit"));
        assert!(input.tool_input.is_some());
        assert_eq!(input.session_id.as_deref(), Some("ses-123"));
    }

    #[test]
    fn test_map_hook_output_block_with_category() {
        let output = HookOutput::Block {
            reason: "blocked".into(),
            category: Some(codeflow_core::hooks::BlockCategory::Gate),
        };
        assert_eq!(map_hook_output(&output), EXIT_HOOK_BLOCK);
    }

    #[test]
    fn test_map_hook_output_block_prints_reason_to_stderr() {
        // Block output maps to EXIT_HOOK_BLOCK and the reason is printed.
        // We can at least verify the exit code is correct regardless of stderr.
        let output = HookOutput::Block {
            reason: "sentinel pf-3 missing".into(),
            category: Some(codeflow_core::hooks::BlockCategory::Gate),
        };
        assert_eq!(map_hook_output(&output), EXIT_HOOK_BLOCK);
    }

    #[test]
    fn test_execute_allow_then_map_produces_exit_zero() {
        let handler = AllowHandler;
        let input = make_input();
        let output = execute_hook_handler(&handler, input).unwrap();
        let code = map_hook_output(&output);
        assert_eq!(code, EXIT_SUCCESS, "Allow handler should map to exit 0");
    }

    #[test]
    fn test_execute_block_then_map_produces_exit_two() {
        let handler = BlockHandler;
        let input = make_input();
        let output = execute_hook_handler(&handler, input).unwrap();
        let code = map_hook_output(&output);
        assert_eq!(code, EXIT_HOOK_BLOCK, "Block handler should map to exit 2");
    }

    #[test]
    fn test_execute_warn_then_map_produces_exit_zero() {
        let handler = WarnHandler;
        let input = make_input();
        let output = execute_hook_handler(&handler, input).unwrap();
        let code = map_hook_output(&output);
        assert_eq!(code, EXIT_SUCCESS, "Warn handler should map to exit 0");
    }

    #[test]
    fn test_handler_metadata_coverage() {
        // Exercise name() and events() on all test handlers.
        let h = AllowHandler;
        assert_eq!(h.name(), "allow-test");
        assert_eq!(h.events(), &[HookEvent::PreToolUse]);

        let h = BlockHandler;
        assert_eq!(h.name(), "block-test");
        assert_eq!(h.events(), &[HookEvent::PreToolUse]);

        let h = ErrorHandler;
        assert_eq!(h.name(), "error-test");
        assert_eq!(h.events(), &[HookEvent::PreToolUse]);

        let h = WarnHandler;
        assert_eq!(h.name(), "warn-test");
        assert_eq!(h.events(), &[HookEvent::PreToolUse]);
    }

    #[test]
    fn test_process_hook_input_allow_handler() {
        let handler = AllowHandler;
        let json =
            r#"{"event": "pre_tool_use", "tool_name": "Bash", "tool_input": {"command": "ls"}}"#;
        let result = process_hook_input(&handler, json);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), EXIT_SUCCESS);
    }

    #[test]
    fn test_process_hook_input_block_handler() {
        let handler = BlockHandler;
        let json = r#"{"event": "pre_tool_use", "tool_name": "Bash"}"#;
        let result = process_hook_input(&handler, json);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), EXIT_HOOK_BLOCK);
    }

    #[test]
    fn test_process_hook_input_error_handler() {
        let handler = ErrorHandler;
        let json = r#"{"event": "pre_tool_use"}"#;
        let result = process_hook_input(&handler, json);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_process_hook_input_empty_stdin() {
        let handler = AllowHandler;
        let result = process_hook_input(&handler, "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_process_hook_input_invalid_json() {
        let handler = AllowHandler;
        let result = process_hook_input(&handler, "not json");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, EXIT_SUCCESS);
    }

    #[test]
    fn test_process_hook_input_warn_handler() {
        let handler = WarnHandler;
        let json = r#"{"event": "pre_tool_use", "tool_name": "Bash"}"#;
        let result = process_hook_input(&handler, json);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), EXIT_SUCCESS);
    }

    #[test]
    fn test_resolve_session_id_with_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let sid = "ses-testhelpersresolve123456";
        let env_content =
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n");
        std::fs::write(state_dir.join("codeflow-env.sh"), env_content).unwrap();
        let result = resolve_session_id(&state_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), sid);
    }
}
