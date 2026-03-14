//! Blocks file writes on protected branches.
//!
//! Detects: redirect writes, sed -i, touch, tee, cp when the current
//! branch is protected. Allows writes to `/tmp/` paths.

use std::sync::OnceLock;

use regex::Regex;

use super::git::is_on_protected_branch;
use super::{CheckContext, SecurityModule, Verdict, block};

fn redirect_to_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([^0-9&>]|^)>\s*[^>&\s]").expect("valid"))
}

fn append_to_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([^0-9&]|^)>>\s*[^>&\s]").expect("valid"))
}

fn sed_inplace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"sed\s.*-i").expect("valid"))
}

fn touch_relative_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s)touch\s+[^-/]").expect("valid"))
}

fn tee_relative_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\|\s*tee\s+[^-/]").expect("valid"))
}

fn cp_command_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s)cp\s").expect("valid"))
}

// Tmp exclusion patterns.
fn tmp_redirect_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r">\s*/tmp/").expect("valid"))
}

fn tmp_append_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r">>\s*/tmp/").expect("valid"))
}

fn touch_tmp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"touch\s+/tmp").expect("valid"))
}

fn tee_tmp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"tee\s+/tmp").expect("valid"))
}

pub struct BranchModule;

impl SecurityModule for BranchModule {
    fn name(&self) -> &'static str {
        "branch-file-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;
        let branch = ctx.current_branch;

        if !is_on_protected_branch(branch, ctx.policy) {
            return None;
        }

        // Skip if all write targets are under /tmp/claude/ (safe scratch).
        if cmd.contains("/tmp/claude/") && !cp_command_re().is_match(cmd) {
            return None;
        }

        // Skip if targeting a protected path (let PathModule handle it).
        let paths = ctx.policy.all_protected_paths();
        for path in &paths {
            if cmd.contains(path.as_str()) {
                return None;
            }
        }

        // Block redirect operations (skip if redirecting to /tmp/).
        if redirect_to_file_re().is_match(cmd) && !tmp_redirect_re().is_match(cmd) {
            return Some(block(
                "Branch Protection",
                &format!("File redirect on protected branch ({branch})."),
                ">",
            ));
        }
        if append_to_file_re().is_match(cmd) && !tmp_append_re().is_match(cmd) {
            return Some(block(
                "Branch Protection",
                &format!("File append on protected branch ({branch})."),
                ">>",
            ));
        }

        if sed_inplace_re().is_match(cmd) {
            return Some(block(
                "Branch Protection",
                &format!("In-place file edit on protected branch ({branch})."),
                "sed -i",
            ));
        }

        if touch_relative_re().is_match(cmd) && !touch_tmp_re().is_match(cmd) {
            return Some(block(
                "Branch Protection",
                &format!("File creation on protected branch ({branch})."),
                "touch",
            ));
        }

        if tee_relative_re().is_match(cmd) && !tee_tmp_re().is_match(cmd) {
            return Some(block(
                "Branch Protection",
                &format!("Piped file write on protected branch ({branch})."),
                "tee",
            ));
        }

        if cp_command_re().is_match(cmd) {
            let fields: Vec<&str> = cmd.split_whitespace().collect();
            if let Some(&dest) = fields.last() {
                if !dest.is_empty() && !dest.starts_with('/') {
                    return Some(block(
                        "Branch Protection",
                        &format!("File copy on protected branch ({branch})."),
                        "cp",
                    ));
                }
            }
        }

        None
    }
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

    fn ctx_on_branch<'a>(cmd: &'a str, branch: &'a str) -> CheckContext<'a> {
        CheckContext {
            tool_name: "Bash",
            command: cmd,
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: branch,
            is_pathflow_active: false,
            policy: test_policy(),
        }
    }

    #[test]
    fn test_feature_branch_allowed() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("echo test > file.txt", "feat/test"))
                .is_none()
        );
    }

    #[test]
    fn test_redirect_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("echo test > file.txt", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_redirect_to_tmp_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("echo test > /tmp/test.txt", "main"))
                .is_none()
        );
    }

    #[test]
    fn test_sed_i_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("sed -i 's/old/new/' file.txt", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_touch_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("touch newfile.txt", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_cp_relative_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("cp source.txt dest.txt", "main"))
                .is_some()
        );
    }

    #[test]
    fn test_cp_absolute_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch("cp source.txt /tmp/dest.txt", "main"))
                .is_none()
        );
    }

    #[test]
    fn test_tmp_claude_allowed_on_main() {
        assert!(
            BranchModule
                .check(&ctx_on_branch(
                    "echo test > /tmp/claude/staging/file.txt",
                    "main"
                ))
                .is_none()
        );
    }
}
