//! Blocks network operations without sandbox bypass.
//!
//! Detects git network commands (`push`, `pull`, `fetch`, `clone`) and
//! GitHub CLI commands (`gh pr`, `gh issue`, etc.) when
//! `dangerouslyDisableSandbox` is not set.

use regex::Regex;

use super::{CheckContext, SecurityModule, Verdict, block};

const DEFAULT_GIT_NETWORK_PATTERNS: &[&str] = &[
    r"^git\s+(push|pull|fetch|clone)",
    r"^git\s+remote\s+update",
    r"^git\s+ls-remote",
];

const DEFAULT_GH_CLI_PATTERNS: &[&str] =
    &[r"^gh\s+(pr|issue|release|api|workflow|run|repo|gist)\s"];

pub struct NetworkModule;

impl SecurityModule for NetworkModule {
    fn name(&self) -> &'static str {
        "network-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        // If sandbox bypass is enabled, network operations are allowed.
        if ctx.sandbox_bypass {
            return None;
        }

        let cmd = ctx.command;

        // Check git network operations.
        let git_patterns: Vec<&str> = if ctx
            .policy
            .network_operations
            .git_network
            .patterns
            .is_empty()
        {
            DEFAULT_GIT_NETWORK_PATTERNS.to_vec()
        } else {
            ctx.policy
                .network_operations
                .git_network
                .patterns
                .iter()
                .map(String::as_str)
                .collect()
        };

        if matches_any_pattern(cmd, &git_patterns) {
            let mut reason =
                "Git network operation requires sandbox bypass (dangerouslyDisableSandbox: true)."
                    .to_string();
            if ctx.is_pathflow_active {
                reason += " In PathFlow mode, delegate to cf-git-operations teammate.";
            } else {
                reason += " Delegate to cf-security teammate for sandbox-check, then cf-git-operations for sync-remote.";
            }
            return Some(block("Network Operation", &reason, "git network"));
        }

        // Check GitHub CLI operations.
        let gh_patterns: Vec<&str> = if ctx.policy.network_operations.github_cli.patterns.is_empty()
        {
            DEFAULT_GH_CLI_PATTERNS.to_vec()
        } else {
            ctx.policy
                .network_operations
                .github_cli
                .patterns
                .iter()
                .map(String::as_str)
                .collect()
        };

        if matches_any_pattern(cmd, &gh_patterns) {
            return Some(block(
                "Network Operation",
                "GitHub CLI requires sandbox bypass (dangerouslyDisableSandbox: true)",
                "gh cli",
            ));
        }

        None
    }
}

fn matches_any_pattern(cmd: &str, patterns: &[&str]) -> bool {
    for p in patterns {
        if let Ok(re) = Regex::new(p) {
            if re.is_match(cmd) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::hooks::pre_tool_use::EnforcementPolicy;

    fn test_policy() -> &'static EnforcementPolicy {
        static POLICY: OnceLock<EnforcementPolicy> = OnceLock::new();
        POLICY.get_or_init(EnforcementPolicy::defaults)
    }

    fn ctx_with_bypass(cmd: &str, bypass: bool) -> CheckContext<'_> {
        CheckContext {
            tool_name: "Bash",
            command: cmd,
            sandbox_bypass: bypass,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: test_policy(),
        }
    }

    fn ctx(cmd: &str) -> CheckContext<'_> {
        ctx_with_bypass(cmd, false)
    }

    #[test]
    fn test_safe_command() {
        assert!(NetworkModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_git_push_blocked() {
        let v = NetworkModule.check(&ctx("git push origin main")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Network Operation");
    }

    #[test]
    fn test_git_pull_blocked() {
        assert!(NetworkModule.check(&ctx("git pull")).is_some());
    }

    #[test]
    fn test_git_fetch_blocked() {
        assert!(NetworkModule.check(&ctx("git fetch origin")).is_some());
    }

    #[test]
    fn test_git_clone_blocked() {
        assert!(
            NetworkModule
                .check(&ctx("git clone https://github.com/test/repo"))
                .is_some()
        );
    }

    #[test]
    fn test_gh_pr_blocked() {
        assert!(NetworkModule.check(&ctx("gh pr create")).is_some());
    }

    #[test]
    fn test_gh_issue_blocked() {
        assert!(NetworkModule.check(&ctx("gh issue list")).is_some());
    }

    #[test]
    fn test_git_push_with_bypass_allowed() {
        assert!(
            NetworkModule
                .check(&ctx_with_bypass("git push origin main", true))
                .is_none()
        );
    }

    #[test]
    fn test_gh_pr_with_bypass_allowed() {
        assert!(
            NetworkModule
                .check(&ctx_with_bypass("gh pr create", true))
                .is_none()
        );
    }

    #[test]
    fn test_git_status_allowed() {
        assert!(NetworkModule.check(&ctx("git status")).is_none());
    }

    #[test]
    fn test_git_commit_allowed() {
        assert!(NetworkModule.check(&ctx("git commit -m 'test'")).is_none());
    }

    #[test]
    fn test_pathflow_active_message() {
        let ctx = CheckContext {
            tool_name: "Bash",
            command: "git push origin main",
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: true,
            policy: test_policy(),
        };
        let v = NetworkModule.check(&ctx).unwrap();
        assert!(v.reason.contains("cf-git-operations"));
    }
}
