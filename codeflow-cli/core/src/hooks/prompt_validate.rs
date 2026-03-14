//! Prompt validation handler matching Go's `internal/hooks/prompt/validate.go`.
//!
//! Outputs context reminders (git status, protected branch, active task,
//! `PathFlow` mode) to a writer on each user prompt submission. Always allows —
//! never blocks the prompt.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};
use crate::session;

/// Default protected branch names (matches Go's `DefaultProtectedBranches()`).
const DEFAULT_PROTECTED_BRANCHES: &[&str] = &["main", "master"];

/// Trait for running git commands, injectable for testing.
pub trait GitRunner: Send + Sync {
    /// Returns (`branch_name`, `uncommitted_count`) for the given project dir.
    fn git_context(&self, project_dir: &Path) -> (String, usize);
}

/// Trait for checking `PathFlow` active state, injectable for testing.
pub trait PathFlowChecker: Send + Sync {
    /// Returns `true` if a `PathFlow` session is active.
    fn is_active(&self, project_dir: &Path) -> bool;
}

/// Production git runner using real git commands.
pub struct OsGitRunner;

impl GitRunner for OsGitRunner {
    fn git_context(&self, project_dir: &Path) -> (String, usize) {
        let branch = run_git_cmd(project_dir, &["branch", "--show-current"]);
        let branch = if branch.is_empty() {
            "unknown".to_string()
        } else {
            branch
        };

        let status = run_git_cmd(project_dir, &["status", "--porcelain"]);
        let count = if status.is_empty() {
            0
        } else {
            status.trim().lines().count()
        };

        (branch, count)
    }
}

/// Production `PathFlow` checker scanning session directories.
pub struct OsPathFlowChecker;

impl PathFlowChecker for OsPathFlowChecker {
    fn is_active(&self, project_dir: &Path) -> bool {
        let session_dir = project_dir.join(".state").join("session");
        let Ok(entries) = std::fs::read_dir(&session_dir) else {
            return false;
        };

        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                continue;
            }
            let pathflow_dir = session_dir.join(entry.file_name()).join("pathflow");

            // Check status file (sole authority).
            let status_path = pathflow_dir.join("pathflow-session-status.json");
            if let Ok(data) = std::fs::read_to_string(&status_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                    let status = val.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if !status.is_empty() && status != "pf-complete" {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Run a git command and return trimmed stdout. Returns empty string on error.
fn run_git_cmd(project_dir: &Path, args: &[&str]) -> String {
    let dir_str = project_dir.to_string_lossy();
    let mut full_args = vec!["-C", &*dir_str];
    full_args.extend_from_slice(args);

    let output = std::process::Command::new("git")
        .args(&full_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// Prompt validator matching Go's `PromptValidator` struct.
///
/// Gathers context reminders and writes them wrapped in
/// `<user-prompt-submit-hook>` tags. Always returns `Allow`.
pub struct PromptValidator<G: GitRunner, P: PathFlowChecker> {
    pub project_dir: PathBuf,
    pub protected_branches: Vec<String>,
    pub git_runner: G,
    pub pathflow_checker: P,
}

impl<G: GitRunner, P: PathFlowChecker> PromptValidator<G, P> {
    /// Gather context reminders and write them to `writer`.
    ///
    /// Matches Go's `PromptValidator::Validate()` output format exactly:
    /// 4 checks (git context, protected branch, active task, `PathFlow` mode),
    /// each wrapped in `<user-prompt-submit-hook>` tags.
    ///
    /// # Errors
    ///
    /// Returns `HookError::Io` on write failures.
    pub fn validate(&self, writer: &mut dyn Write) -> Result<(), HookError> {
        let mut reminders: Vec<String> = Vec::new();

        // 1. Git context.
        let (branch, uncommitted) = self.git_runner.git_context(&self.project_dir);
        if uncommitted > 0 {
            reminders.push(format!(
                "Note: {uncommitted} uncommitted changes on branch '{branch}'"
            ));
        }

        // 2. Protected branch warning.
        if self.is_protected_branch(&branch) {
            reminders.push(format!(
                "Warning: On protected branch '{branch}'. Create a feature branch before changes.\n  \
                 Delegate to cf-git-operations teammate: \
                 SendMessage(recipient=\"cf-git-operations\", content=\"create-feature-branch name=feat/...\")"
            ));
        }

        // 3. Active task.
        let task_id = self.active_task_id();
        if let Some(id) = task_id {
            reminders.push(format!("Active work: {id}"));
        } else {
            reminders.push(
                "Note: No active task registered. Delegate to cf-knowledge-layer teammate: \
                 SendMessage(recipient=\"cf-knowledge-layer\", content=\"ensure-work-registered\")"
                    .to_string(),
            );
        }

        // 4. PathFlow mode.
        if self.pathflow_checker.is_active(&self.project_dir) {
            reminders.push(
                "PathFlow mode active: Coordinate with teammates for specialized tasks.\n  \
                 Git operations -> cf-git-operations | Code review -> cf-review"
                    .to_string(),
            );
        }

        // Write each reminder wrapped in tags (matches Go's fmt.Fprintf format).
        for r in &reminders {
            writeln!(
                writer,
                "<user-prompt-submit-hook>\n{r}\n</user-prompt-submit-hook>"
            )
            .map_err(HookError::Io)?;
        }

        Ok(())
    }

    /// Check if `branch` matches any protected branch pattern.
    fn is_protected_branch(&self, branch: &str) -> bool {
        for protected in &self.protected_branches {
            if branch == protected {
                return true;
            }
            // Glob-style matching (e.g., "release/*").
            if protected.contains('*') && glob_match(protected, branch) {
                return true;
            }
        }
        false
    }

    /// Read the active task ID from `.state/runtime/active-task.json`.
    fn active_task_id(&self) -> Option<String> {
        let runtime_dir = self.project_dir.join(".state").join("runtime");
        let task = session::get_active_task(&runtime_dir).ok()??;
        // Prefer format_id, fall back to task_id (matches Go logic).
        if let Some(ref fmt_id) = task.task_format_id {
            let s = fmt_id.as_str();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
        let s = task.task_id.as_str();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    }
}

/// Simple glob matching for patterns like "release/*".
/// Only supports trailing `*` after a `/` (e.g., `release/*`).
fn glob_match(pattern: &str, text: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        text.starts_with(prefix)
    } else {
        // For more complex patterns, fall back to exact match.
        pattern == text
    }
}

/// Create a `PromptValidator` with production dependencies and default protected branches.
#[must_use]
pub fn new_prompt_validator(
    project_dir: PathBuf,
) -> PromptValidator<OsGitRunner, OsPathFlowChecker> {
    PromptValidator {
        project_dir,
        protected_branches: DEFAULT_PROTECTED_BRANCHES
            .iter()
            .map(|&s| s.to_string())
            .collect(),
        git_runner: OsGitRunner,
        pathflow_checker: OsPathFlowChecker,
    }
}

impl<G: GitRunner, P: PathFlowChecker> HookHandler for PromptValidator<G, P> {
    fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        self.validate(&mut out)?;
        out.flush().map_err(HookError::Io)?;
        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "prompt-validate"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::UserPromptSubmit]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock git runner for testing.
    struct MockGitRunner {
        branch: String,
        uncommitted: usize,
    }

    impl GitRunner for MockGitRunner {
        fn git_context(&self, _project_dir: &Path) -> (String, usize) {
            (self.branch.clone(), self.uncommitted)
        }
    }

    /// Mock PathFlow checker for testing.
    struct MockPathFlowChecker {
        active: bool,
    }

    impl PathFlowChecker for MockPathFlowChecker {
        fn is_active(&self, _project_dir: &Path) -> bool {
            self.active
        }
    }

    fn make_validator(
        dir: &Path,
        branch: &str,
        uncommitted: usize,
        pathflow_active: bool,
    ) -> PromptValidator<MockGitRunner, MockPathFlowChecker> {
        PromptValidator {
            project_dir: dir.to_path_buf(),
            protected_branches: vec!["main".into(), "master".into()],
            git_runner: MockGitRunner {
                branch: branch.into(),
                uncommitted,
            },
            pathflow_checker: MockPathFlowChecker {
                active: pathflow_active,
            },
        }
    }

    #[test]
    fn test_no_uncommitted_no_reminders_about_changes() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("uncommitted"),
            "should not mention uncommitted changes when count is 0"
        );
    }

    #[test]
    fn test_uncommitted_changes_reminder() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 3, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("3 uncommitted changes on branch 'feat/test'"),
            "should report uncommitted count: {output}"
        );
        assert!(output.contains("<user-prompt-submit-hook>"));
    }

    #[test]
    fn test_protected_branch_warning() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "main", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Warning: On protected branch 'main'"),
            "should warn about protected branch: {output}"
        );
    }

    #[test]
    fn test_non_protected_branch_no_warning() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/something", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("protected branch"),
            "should not warn about non-protected branch"
        );
    }

    #[test]
    fn test_no_active_task_reminder() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("No active task registered"),
            "should mention no active task: {output}"
        );
    }

    #[test]
    fn test_active_task_format_id() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let task = serde_json::json!({
            "task_id": "task-abc123",
            "task_format_id": "INF-TSK-001-001"
        });
        std::fs::write(
            runtime_dir.join("active-task.json"),
            serde_json::to_string(&task).unwrap(),
        )
        .unwrap();

        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Active work: INF-TSK-001-001"),
            "should show format_id: {output}"
        );
    }

    #[test]
    fn test_active_task_falls_back_to_task_id() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let task = serde_json::json!({
            "task_id": "task-abc123"
        });
        std::fs::write(
            runtime_dir.join("active-task.json"),
            serde_json::to_string(&task).unwrap(),
        )
        .unwrap();

        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Active work: task-abc123"),
            "should fall back to task_id: {output}"
        );
    }

    #[test]
    fn test_pathflow_active_reminder() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 0, true);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("PathFlow mode active"),
            "should mention PathFlow active: {output}"
        );
    }

    #[test]
    fn test_pathflow_inactive_no_reminder() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("PathFlow mode active"),
            "should not mention PathFlow when inactive"
        );
    }

    #[test]
    fn test_all_reminders_wrapped_in_tags() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "main", 5, true);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let open_count = output.matches("<user-prompt-submit-hook>").count();
        let close_count = output.matches("</user-prompt-submit-hook>").count();
        assert_eq!(
            open_count, close_count,
            "open and close tag counts should match"
        );
        // Should have: uncommitted + protected branch + no active task + pathflow = 4
        assert_eq!(
            open_count, 4,
            "expected 4 reminders (uncommitted, protected, no task, pathflow): {output}"
        );
    }

    #[test]
    fn test_handler_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "feat/test", 0, false);
        assert_eq!(v.name(), "prompt-validate");
        assert_eq!(v.events(), &[HookEvent::UserPromptSubmit]);
    }

    #[test]
    fn test_handler_always_allows() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "main", 10, true);
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::UserPromptSubmit,
            session_id: Some("ses-test".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
        };
        // handle() writes to real stdout, which we can't capture in unit tests,
        // but we can verify it returns Allow.
        let result = v.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), HookOutput::Allow));
    }

    #[test]
    fn test_glob_match_trailing_star() {
        assert!(glob_match("release/*", "release/v1.0"));
        assert!(glob_match("release/*", "release/"));
        assert!(!glob_match("release/*", "main"));
        assert!(!glob_match("release/*", "releases/v1.0"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("main", "main"));
        assert!(!glob_match("main", "master"));
    }

    #[test]
    fn test_protected_branch_glob_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let v = PromptValidator {
            project_dir: dir.path().to_path_buf(),
            protected_branches: vec!["main".into(), "release/*".into()],
            git_runner: MockGitRunner {
                branch: "release/v2.0".into(),
                uncommitted: 0,
            },
            pathflow_checker: MockPathFlowChecker { active: false },
        };
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Warning: On protected branch 'release/v2.0'"),
            "should match glob pattern: {output}"
        );
    }

    #[test]
    fn test_master_is_protected() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "master", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("Warning: On protected branch 'master'"),
            "master should be protected: {output}"
        );
    }

    #[test]
    fn test_run_git_cmd_nonexistent_dir() {
        let result = run_git_cmd(Path::new("/nonexistent/path"), &["status"]);
        assert!(result.is_empty(), "should return empty on error");
    }

    #[test]
    fn test_os_pathflow_checker_no_session_dir() {
        let dir = tempfile::tempdir().unwrap();
        let checker = OsPathFlowChecker;
        assert!(!checker.is_active(dir.path()));
    }

    #[test]
    fn test_os_pathflow_checker_with_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join("ses-test123")
            .join("pathflow");
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test"}"#,
        )
        .unwrap();

        let checker = OsPathFlowChecker;
        assert!(checker.is_active(dir.path()));
    }

    #[test]
    fn test_os_pathflow_checker_without_active_flag() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join("ses-test123")
            .join("pathflow");
        std::fs::create_dir_all(&session_dir).unwrap();

        let checker = OsPathFlowChecker;
        assert!(!checker.is_active(dir.path()));
    }

    #[test]
    fn test_new_prompt_validator_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let v = new_prompt_validator(dir.path().to_path_buf());
        assert_eq!(v.protected_branches, vec!["main", "master"]);
        assert_eq!(v.name(), "prompt-validate");
    }

    #[test]
    fn test_validate_empty_branch_name() {
        let dir = tempfile::tempdir().unwrap();
        let v = make_validator(dir.path(), "", 0, false);
        assert!(!v.is_protected_branch(""));
    }

    #[test]
    fn test_active_task_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(runtime_dir.join("active-task.json"), "not json").unwrap();

        let v = make_validator(dir.path(), "feat/test", 0, false);
        let mut buf = Vec::new();
        v.validate(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("No active task registered"),
            "malformed JSON should be treated as no active task: {output}"
        );
    }
}
