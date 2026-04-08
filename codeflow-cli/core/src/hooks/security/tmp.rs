//! Managed tmp folder protection.
//!
//! Prevents deletion/rename of managed folders under `/tmp/claude/{project}/`
//! and blocks deletion of state files (while allowing creation and editing).

use regex::Regex;

use super::{CheckContext, SecurityModule, Verdict, block};

pub struct TmpModule;

impl SecurityModule for TmpModule {
    fn name(&self) -> &'static str {
        "tmp-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        let project_root = {
            let raw = std::env::var("CF_PROJECT_ROOT").unwrap_or_else(|_| "codeflow".into());
            std::path::Path::new(&raw)
                .file_name()
                .map_or(raw.clone(), |n| n.to_string_lossy().to_string())
        };

        let folders = ctx.policy.managed_tmp_folders(&project_root);
        let state_folder = ctx.policy.state_folder_path(&project_root);

        // Block deletion/rename of managed folders.
        for folder in &folders {
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

        // Block deletion of state files (but allow creation and editing).
        let state_prefix = format!("{state_folder}/");
        if !state_folder.is_empty() && cmd.contains(&state_prefix) {
            let qs = regex::escape(&state_folder);
            if let Ok(re) = Regex::new(&format!(r"(rm|unlink)\s+((-[a-zA-Z]+\s+)*){qs}/")) {
                if re.is_match(cmd) {
                    return Some(block(
                        "State File Protection",
                        "State files protected from deletion. User can rm manually if needed.",
                        &format!("{state_folder}/*"),
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
    use serial_test::serial;

    fn test_policy() -> &'static EnforcementPolicy {
        static POLICY: OnceLock<EnforcementPolicy> = OnceLock::new();
        POLICY.get_or_init(EnforcementPolicy::defaults)
    }

    fn ctx(cmd: &str) -> CheckContext<'_> {
        CheckContext {
            tool_name: "Bash",
            command: cmd,
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: test_policy(),
        }
    }

    #[test]
    fn test_safe_command() {
        assert!(
            TmpModule
                .check(&ctx("ls /tmp/claude/codeflow/managed"))
                .is_none()
        );
    }

    #[test]
    #[serial(env_vars)]
    fn test_rm_managed_folder() {
        assert!(
            TmpModule
                .check(&ctx("rm -rf /tmp/claude/codeflow/managed"))
                .is_some()
        );
    }

    #[test]
    #[serial(env_vars)]
    fn test_rmdir_managed_folder() {
        assert!(
            TmpModule
                .check(&ctx("rmdir /tmp/claude/codeflow/managed"))
                .is_some()
        );
    }

    #[test]
    #[serial(env_vars)]
    fn test_mv_managed_folder() {
        assert!(
            TmpModule
                .check(&ctx("mv /tmp/claude/codeflow/managed /tmp/other"))
                .is_some()
        );
    }

    #[test]
    #[serial(env_vars)]
    fn test_rm_state_file() {
        assert!(
            TmpModule
                .check(&ctx("rm /tmp/claude/codeflow/managed/state/active.json"))
                .is_some()
        );
    }

    #[test]
    fn test_echo_to_state_file_allowed() {
        // Writing to state files should be allowed (only deletion is blocked).
        assert!(
            TmpModule
                .check(&ctx(
                    "echo test > /tmp/claude/codeflow/managed/state/active.json"
                ))
                .is_none()
        );
    }

    #[test]
    #[serial(env_vars)]
    fn test_tmp_check_extracts_basename_from_absolute_path() {
        // CF_PROJECT_ROOT may contain an absolute path; TmpModule must extract
        // the basename so that managed tmp folder paths use "codeflow", not
        // "/Volumes/.../codeflow".
        // SAFETY: Test-only env var manipulation.
        let prev = std::env::var("CF_PROJECT_ROOT").ok();
        unsafe { std::env::set_var("CF_PROJECT_ROOT", "/Volumes/DATA/projects/codeflow") };

        // rm -rf on the managed folder should still be blocked using the basename.
        let result = TmpModule.check(&ctx("rm -rf /tmp/claude/codeflow/managed"));
        assert!(result.is_some(), "should block rm -rf on managed folder");

        // Restore original env.
        match prev {
            Some(v) => unsafe { std::env::set_var("CF_PROJECT_ROOT", v) },
            None => unsafe { std::env::remove_var("CF_PROJECT_ROOT") },
        }
    }
}
