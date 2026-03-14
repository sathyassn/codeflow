//! Security enforcement modules for Bash command validation.
//!
//! Orchestrates 8 modules in priority order (critical first):
//! 1. `dangerous` - Destructive commands (rm -rf /, fork bombs)
//! 2. `privilege` - Privilege escalation (sudo, su, doas)
//! 3. `git` - Git hook bypass, force push, protected branches
//! 4. `path` - Protected path operations
//! 5. `fileops` - Indirect file operations, glob bypass
//! 6. `branch` - File writes on protected branches
//! 7. `tmp` - Managed tmp folder protection
//! 8. `network` - Network operations without sandbox bypass

pub mod branch;
pub mod dangerous;
pub mod fileops;
pub mod git;
pub mod network;
pub mod path;
pub mod pattern;
pub mod privilege;
pub mod tmp;

use super::pre_tool_use::EnforcementPolicy;
use super::{BlockCategory, HookEvent, HookHandler, HookInput, HookOutput};
use crate::error::HookError;
use crate::session;
use std::path::PathBuf;

/// Result of a security module check.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// `true` if the command is permitted.
    pub allow: bool,
    /// Security violation category (e.g., "Dangerous Command").
    pub category: String,
    /// Human-readable explanation.
    pub reason: String,
    /// The specific pattern that triggered the block.
    pub pattern: String,
    /// Name of the module that produced this verdict.
    pub module: String,
}

impl Verdict {
    /// Format the block message for stderr output.
    #[must_use]
    pub fn message(&self) -> String {
        if self.allow {
            return String::new();
        }
        format!(
            "BLOCKED: Security violation detected\n\n\
             Category: {}\n\
             Reason: {}\n\
             Pattern: {}\n\n\
             This is a security restriction enforced by CodeFlow.\n\
             See: .codeflow/docs/security/README.md\n",
            self.category, self.reason, self.pattern
        )
    }
}

/// Context for security module checks.
pub struct CheckContext<'a> {
    /// The Claude Code tool being used (e.g., "Bash").
    pub tool_name: &'a str,
    /// The bash command being checked.
    pub command: &'a str,
    /// `true` if `dangerouslyDisableSandbox` is set.
    pub sandbox_bypass: bool,
    /// Repository root directory.
    pub project_dir: &'a str,
    /// Current `CodeFlow` session ID.
    pub session_id: &'a str,
    /// Current git branch name.
    pub current_branch: &'a str,
    /// `true` if a `PathFlow` session is active.
    pub is_pathflow_active: bool,
    /// Parsed enforcement policy configuration.
    pub policy: &'a EnforcementPolicy,
}

/// Trait implemented by each security enforcement module.
pub trait SecurityModule: Send + Sync {
    /// Module name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Evaluate the command against the module's rules.
    ///
    /// Returns `None` if the module has no opinion (allow), or a `Verdict`
    /// with `allow=false` to block.
    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict>;
}

/// Create a blocking verdict.
#[must_use]
pub fn block(category: &str, reason: &str, pattern: &str) -> Verdict {
    Verdict {
        allow: false,
        category: category.into(),
        reason: reason.into(),
        pattern: pattern.into(),
        module: String::new(),
    }
}

/// Return the default set of enforcement modules in execution order.
#[must_use]
pub fn default_modules() -> Vec<Box<dyn SecurityModule>> {
    vec![
        Box::new(dangerous::DangerousModule),
        Box::new(privilege::PrivilegeModule),
        Box::new(git::GitModule),
        Box::new(path::PathModule),
        Box::new(fileops::FileOpsModule),
        Box::new(branch::BranchModule),
        Box::new(tmp::TmpModule),
        Box::new(network::NetworkModule),
    ]
}

/// Orchestrates security enforcement modules.
pub struct SecurityChecker {
    modules: Vec<Box<dyn SecurityModule>>,
}

impl SecurityChecker {
    /// Create a checker with the default set of enforcement modules.
    #[must_use]
    pub fn new() -> Self {
        Self {
            modules: default_modules(),
        }
    }

    /// Run all modules against the given context. Returns the first blocking
    /// verdict, or an allow verdict if all modules pass.
    #[must_use]
    pub fn check(&self, ctx: &CheckContext<'_>) -> Verdict {
        for m in &self.modules {
            if let Some(mut v) = m.check(ctx) {
                if !v.allow {
                    v.module = m.name().to_string();
                    return v;
                }
            }
        }
        Verdict {
            allow: true,
            category: String::new(),
            reason: String::new(),
            pattern: String::new(),
            module: String::new(),
        }
    }
}

impl Default for SecurityChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SecurityHandler: HookHandler adapter for SecurityChecker
// ---------------------------------------------------------------------------

/// Adapts `SecurityChecker` to the `HookHandler` trait for integration
/// into the hook pipeline.
pub struct SecurityHandler {
    pub project_dir: PathBuf,
}

impl SecurityHandler {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for SecurityHandler {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only check Bash tool calls.
        let tool_name = match &input.tool_name {
            Some(name) if name == "Bash" => name.as_str(),
            _ => return Ok(HookOutput::Allow),
        };

        // Extract command from tool_input.
        let command = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if command.is_empty() {
            return Ok(HookOutput::Allow);
        }

        // Extract sandbox bypass flag.
        let sandbox_bypass = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("dangerouslyDisableSandbox"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // Load enforcement policy.
        let policy = EnforcementPolicy::load(&self.project_dir);

        // Determine current branch (best-effort).
        let current_branch = get_current_branch(&self.project_dir);

        // Resolve session ID from environment/filesystem, never stdin UUID.
        let session_id = session::current_session_id(&self.project_dir.join(".state"))
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;
        let session_id_str = session_id.as_ref();

        // Check if PathFlow is active.
        let is_pathflow_active = check_pathflow_active(&self.project_dir, session_id_str);

        let project_dir_str = self.project_dir.to_string_lossy();

        let ctx = CheckContext {
            tool_name,
            command,
            sandbox_bypass,
            project_dir: &project_dir_str,
            session_id: session_id_str,
            current_branch: &current_branch,
            is_pathflow_active,
            policy: &policy,
        };

        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);

        if verdict.allow {
            Ok(HookOutput::Allow)
        } else {
            Ok(HookOutput::Block {
                reason: verdict.message(),
                category: Some(BlockCategory::Security),
            })
        }
    }

    fn name(&self) -> &'static str {
        "security"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PreToolUse]
    }
}

/// Get current git branch (best-effort, returns empty on failure).
fn get_current_branch(project_dir: &std::path::Path) -> String {
    let head_path = project_dir.join(".git").join("HEAD");
    match std::fs::read_to_string(head_path) {
        Ok(content) => {
            let trimmed = content.trim();
            if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                branch.to_string()
            } else {
                String::new()
            }
        }
        Err(_) => String::new(),
    }
}

/// Check if `PathFlow` is active for any session (best-effort).
///
/// Uses `pathflow-session-status.json` as the sole authority.
fn check_pathflow_active(project_dir: &std::path::Path, session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let pathflow_dir = project_dir
        .join(".state")
        .join("session")
        .join(session_id)
        .join("pathflow");
    let status_path = pathflow_dir.join("pathflow-session-status.json");
    if let Ok(data) = std::fs::read_to_string(&status_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
            let status = val.get("status").and_then(|v| v.as_str()).unwrap_or("");
            return !status.is_empty() && status != "pf-complete";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_helper() {
        let v = block("Test Category", "test reason", "test pattern");
        assert!(!v.allow);
        assert_eq!(v.category, "Test Category");
        assert_eq!(v.reason, "test reason");
        assert_eq!(v.pattern, "test pattern");
    }

    #[test]
    fn test_verdict_message_allow() {
        let v = Verdict {
            allow: true,
            category: String::new(),
            reason: String::new(),
            pattern: String::new(),
            module: String::new(),
        };
        assert_eq!(v.message(), "");
    }

    #[test]
    fn test_verdict_message_block() {
        let v = block("Dangerous Command", "rm -rf /", "rm -rf");
        let msg = v.message();
        assert!(msg.contains("BLOCKED: Security violation detected"));
        assert!(msg.contains("Dangerous Command"));
        assert!(msg.contains("rm -rf /"));
    }

    #[test]
    fn test_default_modules_count() {
        let modules = default_modules();
        assert_eq!(modules.len(), 8);
    }

    #[test]
    fn test_default_modules_names() {
        let modules = default_modules();
        let names: Vec<&str> = modules.iter().map(|m| m.name()).collect();
        assert_eq!(
            names,
            vec![
                "dangerous-commands",
                "privilege-protection",
                "git-protection",
                "path-protection",
                "file-operations",
                "branch-file-protection",
                "tmp-protection",
                "network-protection",
            ]
        );
    }

    #[test]
    fn test_security_checker_allows_safe_command() {
        let policy = EnforcementPolicy::defaults();
        let ctx = CheckContext {
            tool_name: "Bash",
            command: "ls -la",
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(verdict.allow);
    }

    #[test]
    fn test_security_checker_blocks_rm_rf_root() {
        let policy = EnforcementPolicy::defaults();
        let ctx = CheckContext {
            tool_name: "Bash",
            command: "rm -rf /",
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(!verdict.allow);
        assert_eq!(verdict.category, "Dangerous Command");
    }

    #[test]
    fn test_security_checker_blocks_sudo() {
        let policy = EnforcementPolicy::defaults();
        let ctx = CheckContext {
            tool_name: "Bash",
            command: "sudo rm -rf /tmp/test",
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: &policy,
        };
        let checker = SecurityChecker::new();
        let verdict = checker.check(&ctx);
        assert!(!verdict.allow);
    }

    #[test]
    fn test_security_handler_ignores_non_bash() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SecurityHandler::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "/tmp/test.rs"})),
            event: HookEvent::PreToolUse,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_security_handler_name_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SecurityHandler::new(dir.path().to_path_buf());
        assert_eq!(handler.name(), "security");
        assert_eq!(handler.events(), &[HookEvent::PreToolUse]);
    }

    #[test]
    fn test_get_current_branch_no_git() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(get_current_branch(dir.path()), "");
    }

    #[test]
    fn test_get_current_branch_with_ref() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/feat/test\n").unwrap();
        assert_eq!(get_current_branch(dir.path()), "feat/test");
    }

    #[test]
    fn test_check_pathflow_active_no_flag() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!check_pathflow_active(dir.path(), "ses-test"));
    }

    #[test]
    fn test_check_pathflow_active_with_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let pathflow_dir = dir.path().join(".state/session/ses-test/pathflow");
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();
        assert!(check_pathflow_active(dir.path(), "ses-test"));
    }

    #[test]
    fn test_check_pathflow_active_complete_status() {
        let dir = tempfile::tempdir().unwrap();
        let pathflow_dir = dir.path().join(".state/session/ses-test/pathflow");
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-complete","team_name":"test-team"}"#,
        )
        .unwrap();
        assert!(!check_pathflow_active(dir.path(), "ses-test"));
    }

    #[test]
    fn test_check_pathflow_active_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let pathflow_dir = dir.path().join(".state/session/ses-test/pathflow");
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            r#"{"status": "pf-in-progress"}"#,
        )
        .unwrap();
        assert!(check_pathflow_active(dir.path(), "ses-test"));
    }

    #[test]
    fn test_check_pathflow_active_empty_session() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!check_pathflow_active(dir.path(), ""));
    }

    #[test]
    fn test_security_handler_uses_filesystem_session_id_not_stdin() {
        // Set up a tempdir with a session ID file (filesystem source).
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let runtime_dir = state_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Write a known session ID to the filesystem.
        let fs_session_id = "ses-1234567890abc";
        std::fs::write(runtime_dir.join("current-session-id"), fs_session_id).unwrap();
        // Write codeflow-env.sh so session::current_session_id can find it.
        // Both CODEFLOW_SESSION_ID and CF_PROJECT_ROOT are required by the parser.
        std::fs::write(
            state_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID=\"{fs_session_id}\"\nexport CF_PROJECT_ROOT=\"test-project\"\n"),
        )
        .unwrap();

        // Create pathflow status file under the FILESYSTEM session ID.
        let pathflow_dir = dir
            .path()
            .join(format!(".state/session/{fs_session_id}/pathflow"));
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"test-team"}"#,
        )
        .unwrap();

        let handler = SecurityHandler::new(dir.path().to_path_buf());

        // Pass a DIFFERENT session_id in the input (simulating stdin UUID).
        let stdin_uuid = "totally-different-uuid-from-stdin";
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "git push origin main"})),
            event: HookEvent::PreToolUse,
            session_id: Some(stdin_uuid.into()),
            project_dir: None,
            source: None,
            transcript_path: None,
        };

        // The handler should use the filesystem session ID, so it finds
        // pathflow-active and adds the cf-git-operations delegation message.
        let result = handler.handle(input).unwrap();
        // Network operation blocked (git push without sandbox bypass).
        assert_eq!(result.exit_code(), 2);
        if let HookOutput::Block { reason, .. } = &result {
            // The block message should mention cf-git-operations (PathFlow active).
            assert!(
                reason.contains("cf-git-operations"),
                "Expected PathFlow delegation message but got: {reason}"
            );
        }
    }
}
