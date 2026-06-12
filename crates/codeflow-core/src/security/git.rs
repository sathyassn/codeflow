//! Validates git commands for security violations.
//!
//! Sections: hook bypass, force push, hook path manipulation,
//! `.git/hooks` directory protection, protected branch operations.

use std::sync::OnceLock;

use regex::Regex;

use super::pattern::get_flags_portion;
use super::{CheckContext, SecurityModule, Verdict, block};

// Section 1: Git hook bypass patterns.
fn git_no_verify_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"git\s+(commit|push|rebase|cherry-pick|merge|am)\s+.*--no-verify")
            .expect("valid")
    })
}

fn git_level_no_verify_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+--no-verify").expect("valid"))
}

fn git_commit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^git\s+commit\s").expect("valid"))
}

fn standalone_n_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s-n($|\s)").expect("valid"))
}

fn combined_n_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s-[a-mo-z]*n[a-mo-z]*($|\s)").expect("valid"))
}

// Section 2: Force push patterns.
fn git_push_force_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+push\s+.*--force($|\s)").expect("valid"))
}

fn git_push_force_lease_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+push\s+.*--force-with-lease($|\s)").expect("valid"))
}

fn git_push_f_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+push\s+(.*\s)?-f($|\s)").expect("valid"))
}

// Section 3: Hook path manipulation.
fn git_config_hooks_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+config.*core\.hooksPath").expect("valid"))
}

fn git_c_hooks_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+-c\s+core\.hooksPath").expect("valid"))
}

fn git_unset_hooks_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+config.*--unset.*core\.hooksPath").expect("valid"))
}

fn husky_bypass_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"HUSKY\s*=\s*0").expect("valid"))
}

// Section 4: .git/hooks directory protection.
fn git_hooks_modify_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(rm|mv|chmod|chown)\s.*\.git/hooks").expect("valid"))
}

fn git_hooks_redirect_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r">\s*\.git/hooks").expect("valid"))
}

// Section 5: Protected branch operations.
fn git_merge_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^git\s+merge(\s|$)").expect("valid"))
}

fn git_cherry_pick_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^git\s+cherry-pick(\s|$)").expect("valid"))
}

fn git_rebase_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^git\s+rebase(\s|$)").expect("valid"))
}

fn git_reset_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^git\s+reset(\s|$)").expect("valid"))
}

fn git_checkout_chained_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"git\s+checkout\s+(\S+)\s*(&&|;|\|)\s*git\s+(merge|cherry-pick|rebase|reset)")
            .expect("valid")
    })
}

fn git_switch_chained_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"git\s+switch\s+(\S+)\s*(&&|;|\|)\s*git\s+(merge|cherry-pick|rebase|reset)")
            .expect("valid")
    })
}

pub struct GitModule;

impl SecurityModule for GitModule {
    fn name(&self) -> &'static str {
        "git-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        if let Some(v) = check_hook_bypass(cmd) {
            return Some(v);
        }
        if let Some(v) = check_force_push(cmd, ctx) {
            return Some(v);
        }
        if let Some(v) = check_hook_path_manipulation(cmd) {
            return Some(v);
        }
        if let Some(v) = check_git_hooks_dir(cmd) {
            return Some(v);
        }
        if let Some(v) = check_protected_branch_ops(cmd, ctx) {
            return Some(v);
        }

        None
    }
}

fn check_hook_bypass(cmd: &str) -> Option<Verdict> {
    if git_no_verify_re().is_match(cmd) {
        return Some(block(
            "Git Hook Bypass",
            "Hook bypass flag detected",
            "--no-verify",
        ));
    }
    if git_level_no_verify_re().is_match(cmd) {
        return Some(block(
            "Git Hook Bypass",
            "Hook bypass flag at git level",
            "git --no-verify",
        ));
    }

    // -n flag for git commit only (NOT git push where -n = --dry-run).
    if git_commit_re().is_match(cmd) {
        let flags = get_flags_portion(cmd);
        if standalone_n_re().is_match(flags) {
            return Some(block(
                "Git Hook Bypass",
                "Hook bypass flag (short form)",
                "-n",
            ));
        }
        if combined_n_re().is_match(flags) {
            return Some(block(
                "Git Hook Bypass",
                "Hook bypass in combined flags",
                "-n in combined flags",
            ));
        }
    }

    None
}

fn check_force_push(cmd: &str, ctx: &CheckContext<'_>) -> Option<Verdict> {
    let branch = ctx.current_branch;
    if !is_on_protected_branch(branch, ctx.policy) {
        return None;
    }

    if git_push_force_re().is_match(cmd) {
        return Some(block(
            "Force Push",
            &format!("Force push to protected branch '{branch}'"),
            "--force",
        ));
    }
    if git_push_force_lease_re().is_match(cmd) {
        return Some(block(
            "Force Push",
            &format!("Force push (with lease) to protected branch '{branch}'"),
            "--force-with-lease",
        ));
    }
    if git_push_f_re().is_match(cmd) {
        return Some(block(
            "Force Push",
            &format!("Force push (short form) to protected branch '{branch}'"),
            "-f",
        ));
    }

    None
}

fn check_hook_path_manipulation(cmd: &str) -> Option<Verdict> {
    if git_config_hooks_path_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Hook path modification attempt",
            "core.hooksPath",
        ));
    }
    if git_c_hooks_path_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Hook path override via -c flag",
            "-c core.hooksPath",
        ));
    }
    if git_unset_hooks_path_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Hook path unset attempt",
            "--unset core.hooksPath",
        ));
    }

    if cmd.contains("GIT_HOOKS_PATH") {
        return Some(block(
            "Hook Manipulation",
            "Hook path env var",
            "GIT_HOOKS_PATH",
        ));
    }
    if cmd.contains("SKIP_HOOKS") {
        return Some(block(
            "Hook Manipulation",
            "Hook skip env var",
            "SKIP_HOOKS",
        ));
    }
    if cmd.contains("GIT_SKIP_HOOKS") {
        return Some(block(
            "Hook Manipulation",
            "Hook skip env var",
            "GIT_SKIP_HOOKS",
        ));
    }

    if husky_bypass_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Husky bypass attempt",
            "HUSKY=0",
        ));
    }
    if cmd.contains("PRE_COMMIT_ALLOW_NO_CONFIG") {
        return Some(block(
            "Hook Manipulation",
            "pre-commit bypass attempt",
            "PRE_COMMIT_ALLOW_NO_CONFIG",
        ));
    }

    None
}

fn check_git_hooks_dir(cmd: &str) -> Option<Verdict> {
    if git_hooks_modify_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Direct .git/hooks modification",
            ".git/hooks",
        ));
    }
    if git_hooks_redirect_re().is_match(cmd) {
        return Some(block(
            "Hook Manipulation",
            "Redirect to .git/hooks",
            "> .git/hooks",
        ));
    }
    None
}

fn check_protected_branch_ops(cmd: &str, ctx: &CheckContext<'_>) -> Option<Verdict> {
    let branch = ctx.current_branch;

    if is_on_protected_branch(branch, ctx.policy) {
        if git_merge_re().is_match(cmd) {
            return Some(block(
                "Protected Branch",
                &format!("Merge to protected branch '{branch}' blocked"),
                &format!("git merge on {branch}"),
            ));
        }
        if git_cherry_pick_re().is_match(cmd) {
            return Some(block(
                "Protected Branch",
                &format!("Cherry-pick to protected branch '{branch}' blocked"),
                &format!("git cherry-pick on {branch}"),
            ));
        }
        if git_rebase_re().is_match(cmd) {
            return Some(block(
                "Protected Branch",
                &format!("Rebase on protected branch '{branch}' blocked"),
                &format!("git rebase on {branch}"),
            ));
        }
        if git_reset_re().is_match(cmd) {
            return Some(block(
                "Protected Branch",
                &format!("Reset on protected branch '{branch}' blocked"),
                &format!("git reset on {branch}"),
            ));
        }
    }

    // Chained checkout/switch + merge.
    if let Some(captures) = git_checkout_chained_re().captures(cmd) {
        let target = &captures[1];
        if is_on_protected_branch(target, ctx.policy) {
            return Some(block(
                "Protected Branch",
                &format!("Chained checkout+merge to protected branch '{target}' blocked"),
                "checkout && merge",
            ));
        }
    }
    if let Some(captures) = git_switch_chained_re().captures(cmd) {
        let target = &captures[1];
        if is_on_protected_branch(target, ctx.policy) {
            return Some(block(
                "Protected Branch",
                &format!("Chained switch+merge to protected branch '{target}' blocked"),
                "switch && merge",
            ));
        }
    }

    None
}

/// Check if the given branch is in the protected list.
#[must_use]
pub fn is_on_protected_branch(branch: &str, policy: &crate::security::SecurityPolicy) -> bool {
    if branch.is_empty() {
        return false;
    }

    let branches = policy.protected_branch_list();

    for pb in &branches {
        if pb.contains('*') {
            // Wildcard pattern: convert to regex.
            let pattern = format!("^{}$", regex::escape(pb).replace(r"\*", ".*"));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(branch) {
                    return true;
                }
            }
        } else if branch == pb.as_str() {
            return true;
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

    fn ctx_on_branch<'a>(cmd: &'a str, branch: &'a str) -> CheckContext<'a> {
        CheckContext {
            command: cmd,
            sandbox_bypass: false,
            current_branch: branch,
            policy: test_policy(),
        }
    }

    fn ctx(cmd: &str) -> CheckContext<'_> {
        ctx_on_branch(cmd, "feat/test")
    }

    #[test]
    fn test_safe_git_command() {
        assert!(GitModule.check(&ctx("git status")).is_none());
    }

    #[test]
    fn test_no_verify() {
        assert!(
            GitModule
                .check(&ctx("git commit --no-verify -m 'test'"))
                .is_some()
        );
    }

    #[test]
    fn test_git_level_no_verify() {
        assert!(
            GitModule
                .check(&ctx("git --no-verify commit -m 'test'"))
                .is_some()
        );
    }

    #[test]
    fn test_commit_n_flag() {
        assert!(GitModule.check(&ctx("git commit -n -m 'test'")).is_some());
    }

    #[test]
    fn test_force_push_protected() {
        let v = GitModule
            .check(&ctx_on_branch("git push --force", "main"))
            .unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Force Push");
    }

    #[test]
    fn test_force_push_non_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git push --force", "feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_force_with_lease_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git push --force-with-lease", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_hooks_path_manipulation() {
        assert!(
            GitModule
                .check(&ctx("git config core.hooksPath /tmp"))
                .is_some()
        );
    }

    #[test]
    fn test_git_hooks_dir_modify() {
        assert!(GitModule.check(&ctx("rm -rf .git/hooks")).is_some());
    }

    #[test]
    fn test_git_hooks_redirect() {
        assert!(
            GitModule
                .check(&ctx("echo '#!/bin/sh' > .git/hooks/pre-commit"))
                .is_some()
        );
    }

    #[test]
    fn test_husky_bypass() {
        assert!(GitModule.check(&ctx("HUSKY=0 git commit")).is_some());
    }

    #[test]
    fn test_merge_on_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git merge feat/branch", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_merge_on_feature() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git merge main", "feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_chained_checkout_merge_protected() {
        assert!(
            GitModule
                .check(&ctx("git checkout main && git merge feat/test"))
                .is_some()
        );
    }

    #[test]
    fn test_is_on_protected_branch_defaults() {
        let policy = SecurityPolicy::defaults();
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("master", &policy));
        assert!(!is_on_protected_branch("feat/test", &policy));
        assert!(!is_on_protected_branch("", &policy));
    }

    #[test]
    fn test_is_on_protected_branch_glob_patterns() {
        let policy = SecurityPolicy {
            protected_branches: vec!["main".into(), "release/*".into()],
            ..SecurityPolicy::defaults()
        };
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("release/v1.0", &policy));
        assert!(!is_on_protected_branch("release", &policy));
        assert!(!is_on_protected_branch("feat/release", &policy));
    }

    // -- Force push: short form -f --

    #[test]
    fn test_force_push_short_f_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git push -f", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_force_push_short_f_non_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git push -f", "feat/test"))
                .is_none()
        );
    }

    // -- Hook path manipulation: more patterns --

    #[test]
    fn test_git_c_hooks_path() {
        assert!(
            GitModule
                .check(&ctx("git -c core.hooksPath=/dev/null commit"))
                .is_some()
        );
    }

    #[test]
    fn test_git_unset_hooks_path() {
        assert!(
            GitModule
                .check(&ctx("git config --unset core.hooksPath"))
                .is_some()
        );
    }

    #[test]
    fn test_git_hooks_path_env() {
        assert!(
            GitModule
                .check(&ctx("GIT_HOOKS_PATH=/tmp git commit"))
                .is_some()
        );
    }

    #[test]
    fn test_skip_hooks_env() {
        assert!(
            GitModule
                .check(&ctx("SKIP_HOOKS=1 git commit -m 'test'"))
                .is_some()
        );
    }

    #[test]
    fn test_git_skip_hooks_env() {
        assert!(GitModule.check(&ctx("GIT_SKIP_HOOKS=1 git push")).is_some());
    }

    #[test]
    fn test_pre_commit_allow_no_config() {
        assert!(
            GitModule
                .check(&ctx("PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit"))
                .is_some()
        );
    }

    // -- Protected branch operations: cherry-pick, rebase, reset --

    #[test]
    fn test_cherry_pick_on_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git cherry-pick abc123", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_cherry_pick_on_feature() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git cherry-pick abc123", "feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_rebase_on_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git rebase feat/branch", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_rebase_on_feature() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git rebase main", "feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_reset_on_protected() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git reset --hard HEAD~1", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_reset_on_feature() {
        assert!(
            GitModule
                .check(&ctx_on_branch("git reset --hard HEAD~1", "feat/test"))
                .is_none()
        );
    }

    // -- Chained switch+merge --

    #[test]
    fn test_chained_switch_merge_protected() {
        assert!(
            GitModule
                .check(&ctx("git switch main && git merge feat/test"))
                .is_some()
        );
    }

    #[test]
    fn test_chained_switch_merge_non_protected() {
        assert!(
            GitModule
                .check(&ctx("git switch feat/dev && git merge feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_chained_checkout_merge_non_protected() {
        assert!(
            GitModule
                .check(&ctx("git checkout feat/dev && git merge main"))
                .is_none()
        );
    }

    // -- Commit -n combined flags --

    #[test]
    fn test_commit_combined_n_flag() {
        assert!(GitModule.check(&ctx("git commit -an -m 'test'")).is_some());
    }

    // -- No-verify on other commands --

    #[test]
    fn test_push_no_verify() {
        assert!(GitModule.check(&ctx("git push --no-verify")).is_some());
    }

    #[test]
    fn test_rebase_no_verify() {
        assert!(GitModule.check(&ctx("git rebase --no-verify")).is_some());
    }

    #[test]
    fn test_merge_no_verify() {
        assert!(
            GitModule
                .check(&ctx("git merge --no-verify feat/test"))
                .is_some()
        );
    }
}
