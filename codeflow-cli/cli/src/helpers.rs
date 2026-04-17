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
/// 1. `CODEFLOW_WORKTREE_PATH` environment variable (worktree-aware)
/// 2. `CF_PROJECT_ROOT` environment variable
/// 3. Walk up from current directory looking for a directory that contains
///    BOTH `.claude/` AND `.codeflow/` markers.
///
/// The real project root always has both markers — `codeflow init` creates
/// them together. Requiring both prevents stray sub-directories (e.g., a
/// stale `cli/.codeflow/` artifact from a past misdirected `setup --auto`)
/// from hijacking walk-up and being returned as the "project root", which
/// would then perpetuate writes into the same sub-directory.
///
/// Returns the absolute path to the project root.
pub fn detect_project_dir() -> Result<PathBuf> {
    // 1. Check CODEFLOW_WORKTREE_PATH first (worktree mode).
    if let Ok(wt_path) = std::env::var("CODEFLOW_WORKTREE_PATH") {
        let path = PathBuf::from(&wt_path);
        if path.is_dir() && path.join(".state").is_dir() {
            return Ok(path);
        }
        if !wt_path.is_empty() {
            codeflow_core::diagnostics::warn_fallback(
                "hooks",
                &format!("CODEFLOW_WORKTREE_PATH ({wt_path})"),
                "CF_PROJECT_ROOT",
            );
        }
    }

    // 2. Check CF_PROJECT_ROOT.
    if let Ok(root) = std::env::var("CF_PROJECT_ROOT") {
        let path = PathBuf::from(&root);
        if path.is_dir() {
            return Ok(path);
        }
    }

    // 3. Walk up from current directory looking for a directory with BOTH
    //    markers. See the function doc for why the AND match is required.
    let cwd = std::env::current_dir().context("getting current directory")?;
    let mut dir = cwd.as_path();
    let project_root = loop {
        if dir.join(".claude").is_dir() && dir.join(".codeflow").is_dir() {
            break dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break cwd.clone(),
        }
    };

    // 4. Per-PID env file lookup.
    //    Each Claude Code process writes its worktree path to a per-PID file
    //    at `{project_root}/.state/runtime/codeflow-env-{pid}.sh`. This avoids
    //    the shared env file overwrite problem with parallel sessions.
    if let Some(wt_path) = read_worktree_path_from_pid_file(&project_root) {
        let wt = PathBuf::from(&wt_path);
        if wt.is_dir() && wt.join(".state").is_dir() {
            return Ok(wt);
        }
    }

    Ok(project_root)
}

/// Read `CODEFLOW_WORKTREE_PATH` from a per-PID env file.
///
/// File at: `{project_root}/.state/runtime/shared/codeflow-env-{claude_code_pid}.sh`
/// Avoids shared-file overwrite: each Claude Code process has its own file.
///
/// Step 5 (shared env fallback) was removed -- the shared `codeflow-env.sh` file
/// is unreliable with parallel sessions and is the source of race conditions.
fn read_worktree_path_from_pid_file(project_root: &Path) -> Option<String> {
    let runtime_dir = project_root.join(".state").join("runtime");
    let pid = codeflow_core::session::process::get_claude_code_pid();
    codeflow_core::session::read_pid_env_file(&runtime_dir, pid)
}

/// Detect the main repository root directory (never a worktree).
///
/// Unlike [`detect_project_dir`] which may return a worktree path,
/// this function always resolves to the main repository root. This is
/// needed for worktree management commands that must operate on the
/// main repo's `.git-worktrees/` and `.state/worktrees/` directories.
///
/// Resolution order:
/// 1. `CF_PROJECT_ROOT` environment variable (if absolute and exists)
/// 2. Walk up from current directory looking for a directory with BOTH
///    `.claude/` AND `.codeflow/`, then resolve through
///    `WorktreeManager::resolve_effective_root`.
///
/// The AND match mirrors [`detect_project_dir`] — stray sub-directories
/// containing only one marker (e.g., an orphaned `cli/.codeflow/` from a
/// misdirected past run) must not be returned as the "project root".
pub fn detect_project_root() -> Result<PathBuf> {
    // Fast path: CF_PROJECT_ROOT (absolute path to main repo).
    if let Ok(root) = std::env::var("CF_PROJECT_ROOT") {
        let p = PathBuf::from(&root);
        if p.is_absolute() && p.is_dir() {
            return Ok(codeflow_core::worktree::WorktreeManager::resolve_effective_root(&p));
        }
    }
    // Walk up from current directory looking for both markers.
    let cwd = std::env::current_dir().context("getting current directory")?;
    let mut dir = cwd.as_path();
    let candidate = loop {
        if dir.join(".claude").is_dir() && dir.join(".codeflow").is_dir() {
            break dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break cwd.clone(),
        }
    };
    Ok(codeflow_core::worktree::WorktreeManager::resolve_effective_root(&candidate))
}

/// Resolve the current session ID from environment or state files.
///
/// Takes `project_dir` (the repository root). The underlying
/// `current_session_id` constructs the canonical env file path internally.
pub fn resolve_session_id(project_dir: &Path) -> Result<String> {
    let sid =
        codeflow_core::session::current_session_id(project_dir).context("resolving session ID")?;
    Ok(sid.as_str().to_string())
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

    // Heartbeat: update on every hook invocation (throttled internally).
    touch_heartbeat(&input, handler.name());

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

/// Update the heartbeat file on every hook invocation.
///
/// Best-effort: failures are silently ignored (heartbeat is defense-in-depth,
/// not a blocking prerequisite). Uses the hook event name as the `source` field.
fn touch_heartbeat(_input: &HookInput, handler_name: &str) {
    // Resolve project directory (same logic as detect_project_dir but without Result).
    let project_dir = std::env::var("CODEFLOW_WORKTREE_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| detect_project_dir().ok());

    let Some(dir) = project_dir else {
        return;
    };

    // Resolve session ID from env file.
    let Ok(sid) = codeflow_core::session::current_session_id(&dir) else {
        return;
    };

    let source = handler_name;
    let _ = codeflow_core::session::heartbeat::touch(&dir, sid.as_str(), source);
}

#[cfg(test)]
mod tests {
    use super::*;

    use codeflow_core::{HookError, HookEvent};
    use serial_test::serial;

    /// Save and restore env vars around test bodies. Usage:
    /// `let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);`
    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }
    impl EnvGuard {
        fn new(vars: &[&str]) -> Self {
            let saved = vars
                .iter()
                .map(|v| (v.to_string(), std::env::var(v).ok()))
                .collect();
            Self { saved }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, prev) in &self.saved {
                // SAFETY: Test-only env var restoration on drop.
                match prev {
                    Some(v) => unsafe { std::env::set_var(name, v) },
                    None => unsafe { std::env::remove_var(name) },
                }
            }
        }
    }

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
            ..Default::default()
        }
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_returns_path() {
        // Env var serialization handled by #[serial] attribute.
        // Should return a valid path (either from env or cwd).
        let result = detect_project_dir();
        assert!(result.is_ok());
        assert!(result.unwrap().is_dir());
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_worktree_path_valid() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH"]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".state")).unwrap();

        let wt_str = dir.path().to_string_lossy().to_string();
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::set_var("CODEFLOW_WORKTREE_PATH", &wt_str) };
        let result = detect_project_dir();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dir.path());
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_worktree_path_invalid_falls_through() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH"]);
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::set_var("CODEFLOW_WORKTREE_PATH", "/nonexistent/worktree/path") };
        let result = detect_project_dir();

        assert!(result.is_ok());
        assert_ne!(result.unwrap(), PathBuf::from("/nonexistent/worktree/path"));
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_worktree_path_not_set() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH"]);
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
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
    #[serial(env_vars)]
    fn test_detect_project_dir_uses_env_file_worktree_when_valid() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".claude")).unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let wt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(wt_dir.path().join(".state")).unwrap();

        let wt_path_str = wt_dir.path().to_string_lossy().to_string();
        let env_content = format!(
            "export CODEFLOW_SESSION_ID='ses-test123'\nexport CF_PROJECT_ROOT='test'\nexport CODEFLOW_WORKTREE_PATH='{wt_path_str}'\n"
        );
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();

        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        // Change to the project dir and test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(project_dir.path()).unwrap();

        let result = detect_project_dir();
        assert!(result.is_ok());
        // Step 5 (shared env fallback) was removed. detect_project_dir no longer
        // reads from the shared codeflow-env.sh file. With env vars unset and no
        // per-PID file, it should return the CWD project root (step 3).
        let expected = project_dir.path().canonicalize().unwrap();
        let actual = result.unwrap().canonicalize().unwrap();
        assert_eq!(
            actual, expected,
            "should return CWD project root (step 3), not worktree from shared env file"
        );

        std::env::set_current_dir(original_dir).unwrap();
        // EnvGuard restores on drop.
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_ignores_env_file_when_worktree_missing() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".claude")).unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let env_content = "export CODEFLOW_SESSION_ID='ses-test123'\nexport CF_PROJECT_ROOT='test'\nexport CODEFLOW_WORKTREE_PATH='/nonexistent/worktree'\n";
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();

        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        // Change to the project dir and test
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(project_dir.path()).unwrap();

        let result = detect_project_dir();
        assert!(result.is_ok());
        let actual = result.unwrap();
        // The env file's nonexistent worktree path must NOT be returned.
        // Note: we don't assert the exact fallback path because in worktree
        // environments, CODEFLOW_WORKTREE_PATH may be re-set by another thread
        // (env var manipulation is inherently racy). Instead, verify the invalid
        // path from the env file was correctly rejected.
        assert_ne!(
            actual,
            std::path::PathBuf::from("/nonexistent/worktree"),
            "should NOT return the invalid worktree path from env file"
        );

        let _ = std::env::set_current_dir(original_dir);
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_ignores_env_file_without_worktree_var() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".claude")).unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Write env file WITHOUT CODEFLOW_WORKTREE_PATH
        let env_content =
            "export CODEFLOW_SESSION_ID='ses-test123'\nexport CF_PROJECT_ROOT='test'\n";
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();

        // Ensure env vars are NOT set
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(project_dir.path()).unwrap();

        let result = detect_project_dir();
        assert!(result.is_ok());
        let expected = project_dir.path().canonicalize().unwrap();
        let actual = result.unwrap().canonicalize().unwrap();
        assert_eq!(
            actual, expected,
            "should return project root when env file has no worktree path"
        );

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_resolve_session_id_with_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let sid = "ses-testhelpersresolve123456";
        let env_content =
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n");
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();
        let result = resolve_session_id(dir.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), sid);
    }

    // ─── detect_project_dir step 5 removal verification ─────────────────────

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_does_not_read_shared_env_file() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".claude")).unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Create a worktree dir that would be reachable via env file
        let wt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(wt_dir.path().join(".state")).unwrap();

        // Write shared env file with worktree path (step 5 -- should NOT be read)
        let wt_path_str = wt_dir.path().to_string_lossy().to_string();
        let env_content = format!(
            "export CODEFLOW_SESSION_ID='ses-test'\nexport CF_PROJECT_ROOT='test'\nexport CODEFLOW_WORKTREE_PATH='{wt_path_str}'\n"
        );
        std::fs::write(runtime_dir.join("codeflow-env.sh"), env_content).unwrap();

        // Clear env vars so only steps 3-4 are possible
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(project_dir.path()).unwrap();

        let result = detect_project_dir();
        assert!(result.is_ok());
        let actual = result.unwrap().canonicalize().unwrap();
        let wt_canonical = wt_dir.path().canonicalize().unwrap();
        // The worktree path from the shared env file must NOT be returned.
        // detect_project_dir should return the CWD project root (step 3).
        assert_ne!(
            actual, wt_canonical,
            "step 5 removed: shared env file must NOT be read"
        );

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_step4_pid_file_lookup() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        let project_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project_dir.path().join(".claude")).unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // Create a fake worktree dir with .state
        let wt_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(wt_dir.path().join(".state")).unwrap();

        // Write per-PID env file for the current process
        let wt_path_str = wt_dir.path().to_string_lossy().to_string();
        let pid = codeflow_core::session::process::get_claude_code_pid();
        codeflow_core::session::write_pid_env_file(&runtime_dir, pid, &wt_path_str);

        // Clear env vars so step 4 (per-PID file) is the only path
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(project_dir.path()).unwrap();

        let result = detect_project_dir();
        assert!(result.is_ok());
        let actual = result.unwrap().canonicalize().unwrap();
        let expected = wt_dir.path().canonicalize().unwrap();
        assert_eq!(
            actual, expected,
            "step 4: per-PID file should return worktree path"
        );

        // Cleanup
        codeflow_core::session::remove_pid_env_file(&runtime_dir, pid);
        std::env::set_current_dir(original_dir).unwrap();
    }

    // ─── proptest: HookInput JSON roundtrip stability ───────────────────────

    use proptest::prelude::*;

    proptest! {
        /// All valid HookEvent values survive a serialize → deserialize roundtrip
        /// through `parse_hook_input`. The JSON produced by serde_json must be
        /// parseable by `parse_hook_input` without error.
        #[test]
        fn proptest_hook_input_serialize_roundtrip(
            tool_name in proptest::option::of("[A-Za-z][A-Za-z0-9_-]{0,30}"),
            session_id in proptest::option::of("ses-[a-z0-9]{20}"),
            project_dir in proptest::option::of("/[a-z/]{1,40}"),
        ) {
            let input = HookInput {
                tool_name: tool_name.clone(),
                tool_input: None,
                event: HookEvent::PreToolUse,
                session_id: session_id.clone(),
                project_dir: project_dir.clone(),
                source: None,
                transcript_path: None,
            ..Default::default()
            };
            let json = serde_json::to_string(&input).expect("serialize");
            let result = parse_hook_input(&json);
            prop_assert!(result.is_ok(), "roundtrip parse failed: {}", result.unwrap_err().message);
            let parsed = result.unwrap();
            // Verify the handler would receive the same fields it serialized.
            let parsed_input: HookInput = serde_json::from_str(&json).expect("re-parse");
            prop_assert_eq!(parsed_input.tool_name, tool_name);
            prop_assert_eq!(parsed_input.session_id, session_id);
            prop_assert_eq!(parsed_input.project_dir, project_dir);
            prop_assert_eq!(parsed_input.event, HookEvent::PreToolUse);
            // Verify parse_hook_input returns EXIT_SUCCESS path (no structural error).
            drop(parsed);
        }

        /// Any JSON string containing an unknown event value should fail to parse,
        /// and the error should carry EXIT_SUCCESS exit code (graceful degradation).
        #[test]
        fn proptest_hook_input_invalid_event_fails_gracefully(
            bad_event in "[a-z_]{3,20}",
        ) {
            // Avoid accidentally generating a valid event name.
            let valid_events = [
                "pre_tool_use", "post_tool_use", "task_completed",
                "session_start", "session_end", "stop", "user_prompt_submit",
            ];
            prop_assume!(!valid_events.contains(&bad_event.as_str()));
            let json = format!(r#"{{"event": "{bad_event}"}}"#);
            let result = parse_hook_input(&json);
            prop_assert!(result.is_err(), "invalid event '{bad_event}' should fail to parse");
            prop_assert_eq!(result.unwrap_err().code, EXIT_SUCCESS,
                "invalid event parse error should use EXIT_SUCCESS (graceful)");
        }

        /// process_hook_input with an allow handler returns EXIT_SUCCESS for any
        /// well-formed HookInput JSON string.
        #[test]
        fn proptest_process_hook_input_allow_returns_zero(
            tool_name in "[A-Za-z]{1,20}",
        ) {
            let handler = AllowHandler;
            let json = format!(
                r#"{{"event":"pre_tool_use","tool_name":"{tool_name}"}}"#
            );
            let result = process_hook_input(&handler, &json);
            prop_assert!(result.is_ok(), "allow handler should not fail: {:?}", result.err());
            prop_assert_eq!(result.unwrap(), EXIT_SUCCESS);
        }
    }

    // ─── insta snapshots: hook JSON output format stability ─────────────────

    #[test]
    fn test_hook_input_session_start_json_snapshot() {
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("ses-snap00000000000000000001".into()),
            project_dir: Some("/project".into()),
            source: Some("startup".into()),
            transcript_path: None,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&input).expect("serialize");
        insta::assert_snapshot!(json);
    }

    #[test]
    fn test_hook_input_pre_tool_use_json_snapshot() {
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls -la"})),
            event: HookEvent::PreToolUse,
            session_id: Some("ses-snap00000000000000000002".into()),
            project_dir: Some("/project".into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&input).expect("serialize");
        insta::assert_snapshot!(json);
    }

    #[test]
    fn test_hook_input_session_end_with_transcript_json_snapshot() {
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: Some("ses-snap00000000000000000003".into()),
            project_dir: Some("/project".into()),
            source: Some("compact".into()),
            transcript_path: Some("/project/.state/transcripts/session.jsonl".into()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&input).expect("serialize");
        insta::assert_snapshot!(json);
    }

    #[test]
    fn test_hook_input_minimal_session_end_json_snapshot() {
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&input).expect("serialize");
        insta::assert_snapshot!(json);
    }

    // ─── AND-match walk-up regression tests (AC 5 fix) ───────────────────────

    /// A directory containing ONLY `.codeflow/` (no `.claude/`) must NOT be
    /// returned by the walk-up — the walk must continue past it and find the
    /// canonical root with BOTH markers. This prevents the stale-dir
    /// regeneration cycle where `cli/.codeflow/` (an orphaned artifact) would
    /// previously be returned as "project root", perpetuating writes into the
    /// same sub-directory on every subsequent `detect_project_dir()` call.
    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_skips_dir_with_only_codeflow_marker() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let root = tempfile::tempdir().unwrap();
        // Canonical project root — has BOTH markers.
        std::fs::create_dir_all(root.path().join(".claude")).unwrap();
        std::fs::create_dir_all(root.path().join(".codeflow")).unwrap();
        // Nested orphaned sub-dir — has ONLY .codeflow/, no .claude/.
        let nested = root.path().join("codeflow-cli").join("cli");
        std::fs::create_dir_all(nested.join(".codeflow")).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let result = detect_project_dir();
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "detect should succeed: {result:?}");
        let resolved = result.unwrap().canonicalize().unwrap();
        let canonical_root = root.path().canonicalize().unwrap();
        assert_eq!(
            resolved, canonical_root,
            "walk-up must skip nested dir with only .codeflow/ and reach canonical root"
        );
    }

    /// Fresh project — neither marker anywhere — falls through to cwd.
    /// This is the bootstrap case (first `codeflow init` run).
    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_fresh_project_falls_through_to_cwd() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let dir = tempfile::tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = detect_project_dir();
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        // With no markers anywhere on the ancestor chain, walk-up falls
        // through to cwd. This is the documented bootstrap contract —
        // `codeflow init` relies on it.
        //
        // We compare via canonicalize() so macOS /private/var/... ↔
        // /var/... symlink differences don't cause spurious mismatches.
        let resolved = result.unwrap().canonicalize().unwrap();
        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(resolved, expected);
    }

    /// A directory with `.claude/` only (no `.codeflow/`) must also be
    /// skipped by the walk-up. Legitimate `codeflow init` creates both
    /// together, so a lone `.claude/` indicates a partial or corrupted
    /// state — do not anchor walk-up there.
    #[test]
    #[serial(env_vars)]
    fn test_detect_project_dir_skips_dir_with_only_claude_marker() {
        let _guard = EnvGuard::new(&["CODEFLOW_WORKTREE_PATH", "CF_PROJECT_ROOT"]);
        // SAFETY: Test-only env var manipulation.
        unsafe { std::env::remove_var("CODEFLOW_WORKTREE_PATH") };
        unsafe { std::env::remove_var("CF_PROJECT_ROOT") };

        let root = tempfile::tempdir().unwrap();
        // Canonical root — both markers.
        std::fs::create_dir_all(root.path().join(".claude")).unwrap();
        std::fs::create_dir_all(root.path().join(".codeflow")).unwrap();
        // Nested partial state — only .claude/.
        let nested = root.path().join("sub");
        std::fs::create_dir_all(nested.join(".claude")).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let result = detect_project_dir();
        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let resolved = result.unwrap().canonicalize().unwrap();
        let canonical_root = root.path().canonicalize().unwrap();
        assert_eq!(
            resolved, canonical_root,
            "walk-up must skip nested dir with only .claude/ and reach canonical root"
        );
    }
}
