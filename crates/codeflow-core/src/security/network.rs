//! Blocks network operations without sandbox bypass.
//!
//! Detects git network commands (`push`, `pull`, `fetch`, `clone`) and
//! GitHub CLI commands (`gh pr`, `gh issue`, etc.) when
//! `dangerouslyDisableSandbox` is not set. Pattern lists are configurable
//! via [`crate::security::SecurityPolicy`]; empty lists fall back to the
//! built-in defaults.

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

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
        let git_patterns: Vec<&str> = if ctx.policy.git_network_patterns.is_empty() {
            DEFAULT_GIT_NETWORK_PATTERNS.to_vec()
        } else {
            ctx.policy
                .git_network_patterns
                .iter()
                .map(String::as_str)
                .collect()
        };

        if matches_any_pattern(cmd, &git_patterns) {
            return Some(block(
                "Network Operation",
                "Git network operation requires sandbox bypass (dangerouslyDisableSandbox: true)",
                "git network",
            ));
        }

        // Check GitHub CLI operations.
        let gh_patterns: Vec<&str> = if ctx.policy.github_cli_patterns.is_empty() {
            DEFAULT_GH_CLI_PATTERNS.to_vec()
        } else {
            ctx.policy
                .github_cli_patterns
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
    use crate::security::SecurityPolicy;

    fn test_policy() -> &'static SecurityPolicy {
        static POLICY: OnceLock<SecurityPolicy> = OnceLock::new();
        POLICY.get_or_init(SecurityPolicy::defaults)
    }

    fn ctx_with_bypass(cmd: &str, bypass: bool) -> CheckContext<'_> {
        CheckContext {
            command: cmd,
            sandbox_bypass: bypass,
            current_branch: "feat/test",
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
        assert!(NetworkModule
            .check(&ctx("git clone https://github.com/test/repo"))
            .is_some());
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
        assert!(NetworkModule
            .check(&ctx_with_bypass("git push origin main", true))
            .is_none());
    }

    #[test]
    fn test_gh_pr_with_bypass_allowed() {
        assert!(NetworkModule
            .check(&ctx_with_bypass("gh pr create", true))
            .is_none());
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
    fn test_policy_overrides_git_patterns() {
        // A policy with custom git patterns replaces the built-in list.
        let policy = SecurityPolicy {
            git_network_patterns: vec![r"^git\s+push".into()],
            ..SecurityPolicy::defaults()
        };
        let blocked = CheckContext {
            command: "git push origin main",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        assert!(NetworkModule.check(&blocked).is_some());

        // git pull is not in the custom list, so it passes.
        let allowed = CheckContext {
            command: "git pull",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        assert!(NetworkModule.check(&allowed).is_none());
    }
}
