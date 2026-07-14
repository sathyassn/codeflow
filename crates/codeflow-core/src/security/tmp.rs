//! Managed tmp folder protection.
//!
//! Prevents deletion/rename of managed scratch folders listed in
//! [`crate::security::SecurityPolicy::managed_tmp_folders`]. The list is
//! empty by default, making this module inert until a repo opts in.

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

pub struct TmpModule;

impl SecurityModule for TmpModule {
    fn name(&self) -> &'static str {
        "tmp-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        // Block deletion/rename of managed folders.
        for folder in &ctx.policy.managed_tmp_folders {
            let qf = regex::escape(folder);

            // rm/rmdir/mv targeting the folder itself.
            if let Ok(re) = Regex::new(&format!(r"(rm|rmdir|mv)\s+((-[a-zA-Z]+\s+)*){qf}($|\s)")) {
                if re.is_match(cmd) {
                    return Some(block(
                        "Managed Tmp Protection",
                        "Cannot delete/rename managed folder",
                        folder,
                    ));
                }
            }

            // rm -rf targeting the folder.
            let rm_rf_ok = Regex::new(&format!(
                r"rm\s+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*\s+{qf}($|\s)"
            ));
            let rm_fr_ok = Regex::new(&format!(
                r"rm\s+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*\s+{qf}($|\s)"
            ));
            if let (Ok(rm_rf), Ok(rm_fr)) = (rm_rf_ok, rm_fr_ok) {
                if rm_rf.is_match(cmd) || rm_fr.is_match(cmd) {
                    return Some(block(
                        "Managed Tmp Protection",
                        "Cannot delete managed folder recursively",
                        folder,
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
    use crate::security::SecurityPolicy;

    fn test_policy() -> &'static SecurityPolicy {
        static POLICY: OnceLock<SecurityPolicy> = OnceLock::new();
        POLICY.get_or_init(|| SecurityPolicy {
            managed_tmp_folders: vec!["/tmp/claude/codeflow/managed".into()],
            ..SecurityPolicy::defaults()
        })
    }

    fn ctx(cmd: &str) -> CheckContext<'_> {
        CheckContext {
            command: cmd,
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: test_policy(),
        }
    }

    #[test]
    fn test_safe_command() {
        assert!(TmpModule
            .check(&ctx("ls /tmp/claude/codeflow/managed"))
            .is_none());
    }

    #[test]
    fn test_rm_managed_folder() {
        assert!(TmpModule
            .check(&ctx("rm -rf /tmp/claude/codeflow/managed"))
            .is_some());
    }

    #[test]
    fn test_rmdir_managed_folder() {
        assert!(TmpModule
            .check(&ctx("rmdir /tmp/claude/codeflow/managed"))
            .is_some());
    }

    #[test]
    fn test_mv_managed_folder() {
        assert!(TmpModule
            .check(&ctx("mv /tmp/claude/codeflow/managed /tmp/other"))
            .is_some());
    }

    #[test]
    fn test_write_inside_managed_folder_allowed() {
        // Writing inside the folder is fine; only deletion/rename of the
        // folder itself is blocked.
        assert!(TmpModule
            .check(&ctx("echo test > /tmp/claude/codeflow/managed/note.txt"))
            .is_none());
    }

    #[test]
    fn test_default_policy_is_inert() {
        // Default policy has no managed folders — module never blocks.
        let policy = SecurityPolicy::defaults();
        let c = CheckContext {
            command: "rm -rf /tmp/claude/codeflow/managed",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &policy,
        };
        assert!(TmpModule.check(&c).is_none());
    }
}
