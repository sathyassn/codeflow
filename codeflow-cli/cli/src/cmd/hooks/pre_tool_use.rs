//! Pre-tool-use hook handlers: 7 handlers matching Go CLI.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use codeflow_core::hooks::pre_tool_use::EnforcementPolicy;

use crate::helpers;

/// Pre-tool-use handler subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum PreToolUseHandler {
    /// `PathFlow` sentinel gate enforcement
    #[command(name = "gate-check")]
    GateCheck,
    /// Team dissolution protection
    #[command(name = "team-guard")]
    TeamGuard,
    /// File scope enforcement
    #[command(name = "edit-write-guard")]
    EditWriteGuard,
    /// Protected branch merge guard
    #[command(name = "gh-pr-guard")]
    GhPrGuard,
    /// Tiered resource protection
    #[command(name = "protection-guard")]
    ProtectionGuard,
    /// Security checks
    Security,
    /// Domain validation for network access
    #[command(name = "webfetch-guard")]
    WebfetchGuard,
}

/// Resolve the sentinel directory for the current session.
fn resolve_sentinel_dir(project_dir: &std::path::Path) -> PathBuf {
    let state_dir = project_dir.join(".state");
    let session_id = codeflow_core::session::current_session_id(project_dir)
        .map(|sid| sid.as_str().to_string())
        .unwrap_or_default();

    if session_id.is_empty() {
        state_dir.join("sentinels").join("pathflow").join("unknown")
    } else {
        state_dir
            .join("sentinels")
            .join("pathflow")
            .join(&session_id)
    }
}

/// Resolve the session pathflow directory for the current session.
///
/// Returns `.state/session/{SID}/pathflow/` which is where
/// `pathflow-session-status.json` lives.
fn resolve_session_dir(project_dir: &std::path::Path) -> PathBuf {
    let state_dir = project_dir.join(".state");
    let session_id = codeflow_core::session::current_session_id(project_dir)
        .map(|sid| sid.as_str().to_string())
        .unwrap_or_default();

    if session_id.is_empty() {
        state_dir.join("session").join("unknown").join("pathflow")
    } else {
        state_dir.join("session").join(&session_id).join("pathflow")
    }
}

/// Detect the current git branch using git2.
fn detect_current_branch(project_dir: &std::path::Path) -> String {
    let Ok(repo) = git2::Repository::discover(project_dir) else {
        return String::new();
    };
    let Ok(head) = repo.head() else {
        return String::new();
    };
    head.shorthand().map(String::from).unwrap_or_default()
}

pub fn run(handler: PreToolUseHandler) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let h = build_handler(handler, project_dir);
    helpers::run_hook_handler(h.as_ref());
}

/// Build the concrete handler for the given variant.
fn build_handler(
    handler: PreToolUseHandler,
    project_dir: std::path::PathBuf,
) -> Box<dyn codeflow_core::HookHandler> {
    match handler {
        PreToolUseHandler::GateCheck => {
            let sentinel_dir = resolve_sentinel_dir(&project_dir);
            let state_path = project_dir
                .join(".state")
                .join("coordination")
                .join("state.loro");
            let session_id = codeflow_core::session::current_session_id(&project_dir)
                .unwrap_or_else(|_| codeflow_core::types::SessionId::new_unchecked("unknown"));
            Box::new(codeflow_core::hooks::pre_tool_use::GateCheck::new(
                sentinel_dir,
                state_path,
                session_id,
            ))
        }
        PreToolUseHandler::TeamGuard => {
            let sentinel_dir = resolve_sentinel_dir(&project_dir);
            let session_dir = resolve_session_dir(&project_dir);
            Box::new(codeflow_core::hooks::pre_tool_use::TeamGuard::new(
                session_dir,
                sentinel_dir,
            ))
        }
        PreToolUseHandler::EditWriteGuard => {
            let policy = EnforcementPolicy::load(&project_dir);
            let branch = detect_current_branch(&project_dir);
            Box::new(codeflow_core::hooks::pre_tool_use::EditWriteGuard::new(
                project_dir,
                policy,
                branch,
            ))
        }
        PreToolUseHandler::GhPrGuard => {
            let policy = EnforcementPolicy::load(&project_dir);
            let protected = policy.protected_branch_list();
            Box::new(codeflow_core::hooks::pre_tool_use::GhPrGuard::new(
                protected,
            ))
        }
        PreToolUseHandler::ProtectionGuard => {
            let policy = EnforcementPolicy::load(&project_dir);
            Box::new(codeflow_core::hooks::pre_tool_use::ProtectionGuard::new(
                policy,
                project_dir,
            ))
        }
        PreToolUseHandler::Security => Box::new(
            codeflow_core::hooks::security::SecurityHandler::new(project_dir),
        ),
        PreToolUseHandler::WebfetchGuard => {
            let policy = EnforcementPolicy::load(&project_dir);
            let trusted_path = project_dir
                .join(".codeflow")
                .join("config")
                .join("enforcement")
                .join("trusted-domains.txt");
            Box::new(
                codeflow_core::hooks::pre_tool_use::WebFetchGuard::from_policy(
                    &policy,
                    &trusted_path,
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_sentinel_dir_with_unknown_session() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = resolve_sentinel_dir(dir.path());
        assert!(
            sentinel_dir
                .to_string_lossy()
                .contains("sentinels/pathflow")
        );
    }

    #[test]
    fn test_resolve_session_dir_with_unknown_session() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = resolve_session_dir(dir.path());
        let s = session_dir.to_string_lossy();
        assert!(s.contains("session"), "expected 'session' in path: {s}");
        assert!(
            s.ends_with("pathflow"),
            "expected path to end with 'pathflow': {s}"
        );
    }

    #[test]
    fn test_detect_current_branch_nonexistent_dir() {
        let branch = detect_current_branch(std::path::Path::new("/nonexistent"));
        assert!(branch.is_empty());
    }

    #[test]
    fn test_detect_current_branch_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let branch = detect_current_branch(dir.path());
        assert!(branch.is_empty());
    }

    fn make_input(dir: &std::path::Path) -> codeflow_core::HookInput {
        codeflow_core::HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: codeflow_core::HookEvent::PreToolUse,
            session_id: Some("ses-testpretooluse12345678".into()),
            project_dir: Some(dir.to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_build_handler_gate_check_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::GateCheck, dir.path().to_path_buf());
        assert_eq!(h.name(), "gate-check");
    }

    #[test]
    fn test_build_handler_team_guard_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::TeamGuard, dir.path().to_path_buf());
        assert_eq!(h.name(), "team-guard");
    }

    #[test]
    fn test_build_handler_edit_write_guard_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::EditWriteGuard, dir.path().to_path_buf());
        assert_eq!(h.name(), "edit-write-guard");
    }

    #[test]
    fn test_build_handler_gh_pr_guard_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::GhPrGuard, dir.path().to_path_buf());
        assert_eq!(h.name(), "gh-pr-guard");
    }

    #[test]
    fn test_build_handler_protection_guard_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::ProtectionGuard, dir.path().to_path_buf());
        assert_eq!(h.name(), "protection-guard");
    }

    #[test]
    fn test_build_handler_security_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::Security, dir.path().to_path_buf());
        assert_eq!(h.name(), "security");
    }

    #[test]
    fn test_build_handler_webfetch_guard_name() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::WebfetchGuard, dir.path().to_path_buf());
        assert_eq!(h.name(), "webfetch-guard");
    }

    #[test]
    fn test_build_handler_gate_check_handle_allows_non_edit() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::GateCheck, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        // GateCheck allows non-Edit/Write tools regardless of sentinel state.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_team_guard_handle_allows_non_team_delete() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::TeamGuard, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        // TeamGuard allows tools other than TeamDelete.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_edit_write_guard_handle() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::EditWriteGuard, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        // EditWriteGuard with no enforcement policy should allow Bash tool.
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_handler_gh_pr_guard_handle_allows_non_gh() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::GhPrGuard, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        // GhPrGuard allows non-gh-pr tools.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_protection_guard_handle() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::ProtectionGuard, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_handler_security_handle_allows_non_bash() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::Security, dir.path().to_path_buf());
        // Security handler only inspects Bash commands; non-Bash tools are allowed.
        let input = codeflow_core::HookInput {
            tool_name: Some("Edit".into()),
            tool_input: Some(serde_json::json!({"file_path": "/tmp/a.rs"})),
            event: codeflow_core::HookEvent::PreToolUse,
            session_id: Some("ses-testpretoolsecurity123".into()),
            project_dir: Some(dir.path().to_string_lossy().into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = h.handle(input);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_build_handler_security_handle_bash_with_session() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let runtime_dir = state_dir.join("runtime");
        std::fs::create_dir_all(state_dir.join("logs")).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let sid = "ses-testsecuritybashcmd1234";
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n"),
        )
        .unwrap();
        let h = build_handler(PreToolUseHandler::Security, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_handler_webfetch_guard_handle_allows_non_webfetch() {
        let dir = tempfile::tempdir().unwrap();
        let h = build_handler(PreToolUseHandler::WebfetchGuard, dir.path().to_path_buf());
        let input = make_input(dir.path());
        let result = h.handle(input);
        // WebfetchGuard allows non-WebFetch tools.
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), codeflow_core::HookOutput::Allow));
    }

    #[test]
    fn test_resolve_sentinel_dir_falls_back_to_unknown_subpath() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = resolve_sentinel_dir(dir.path());
        assert!(
            sentinel_dir.ends_with("unknown"),
            "expected 'unknown' fallback in path: {}",
            sentinel_dir.display()
        );
    }

    #[test]
    fn test_resolve_session_dir_falls_back_to_unknown_subpath() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = resolve_session_dir(dir.path());
        let s = session_dir.to_string_lossy();
        assert!(
            s.contains("unknown"),
            "expected 'unknown' fallback in path: {s}",
        );
        assert!(
            s.ends_with("unknown/pathflow"),
            "expected path to end with 'unknown/pathflow': {s}",
        );
    }

    #[test]
    fn test_resolve_sentinel_dir_with_session_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let sid = "ses-testsentineldirresolve12";
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n"),
        )
        .unwrap();
        let sentinel_dir = resolve_sentinel_dir(dir.path());
        assert!(
            sentinel_dir.ends_with(sid),
            "expected session ID in path: {}",
            sentinel_dir.display()
        );
    }

    #[test]
    fn test_resolve_session_dir_with_session_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let sid = "ses-testsessiondirresolve123";
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID='{sid}'\nexport CF_PROJECT_ROOT='/tmp/test'\n"),
        )
        .unwrap();
        let session_dir = resolve_session_dir(dir.path());
        let s = session_dir.to_string_lossy();
        assert!(s.contains(sid), "expected session ID in path: {s}",);
        let expected_suffix = format!("{sid}/pathflow");
        assert!(
            s.ends_with(&expected_suffix),
            "expected path to end with '{expected_suffix}': {s}",
        );
    }
}
