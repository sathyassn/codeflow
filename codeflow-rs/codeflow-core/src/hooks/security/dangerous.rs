//! Detects and blocks destructive commands.
//!
//! Checks for: recursive deletion of root/system dirs, disk operations,
//! dangerous permission changes, fork bombs.

use std::sync::OnceLock;

use regex::Regex;

use super::{CheckContext, SecurityModule, Verdict, block};

/// Substring patterns that always block.
const DANGEROUS_SUBSTRINGS: &[&str] =
    &["dd if=/dev/zero", "dd if=/dev/random", "mkfs.", "> /dev/sd"];

fn rm_rf_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+/(\s|$|\*)").expect("valid"))
}

fn rm_fr_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"rm\s+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*\s+/(\s|$|\*)").expect("valid")
    })
}

fn rm_home_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rm\s+-[a-zA-Z]*r[a-zA-Z]*\s+~(\s|$|/\*)").expect("valid"))
}

fn rm_system_dir_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"rm\s+-[a-zA-Z]*r[a-zA-Z]*\s+/(etc|var|usr|bin|sbin|boot|lib|lib64|opt|root|sys|proc)(\s|$|/)",
        )
        .expect("valid")
    })
}

fn dd_disk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"dd\s.*of=/dev/(sd|hd|nvme|vd)[a-z]").expect("valid"))
}

fn format_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(mkfs|mke2fs|mkswap)\s").expect("valid"))
}

fn chmod_777_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"chmod\s.*777\s+/(etc|var|usr|bin|sbin|boot|lib|opt|root)(\s|$|/)|chmod\s.*777\s+/(\s|$)",
        )
        .expect("valid")
    })
}

fn chmod_r_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"chmod\s+-R\s.*\s/($|\s)").expect("valid"))
}

fn chown_r_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"chown\s+-R\s.*\s/($|\s)").expect("valid"))
}

fn fork_bomb_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r":\(\)\s*\{.*\|.*:\s*&\s*\}\s*;\s*:").expect("valid"))
}

fn fork_bomb_dot_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.\(\)\s*\{.*\|.*\.\s*&\s*\}\s*;\s*\.").expect("valid"))
}

pub struct DangerousModule;

impl SecurityModule for DangerousModule {
    fn name(&self) -> &'static str {
        "dangerous-commands"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        // Substring pattern checks.
        for &pattern in DANGEROUS_SUBSTRINGS {
            if cmd.contains(pattern) {
                return Some(block(
                    "Dangerous Command",
                    "Destructive operation detected",
                    pattern,
                ));
            }
        }

        // Recursive deletion checks.
        if let Some(v) = check_recursive_delete(cmd) {
            return Some(v);
        }

        // Disk operation checks.
        if let Some(v) = check_disk_operations(cmd) {
            return Some(v);
        }

        // Permission change checks.
        if let Some(v) = check_permission_changes(cmd) {
            return Some(v);
        }

        // Fork bomb checks.
        if let Some(v) = check_fork_bomb(cmd) {
            return Some(v);
        }

        None
    }
}

fn check_recursive_delete(cmd: &str) -> Option<Verdict> {
    if rm_rf_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive deletion of root directory",
            "rm -r[f] /",
        ));
    }
    if rm_fr_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive forced deletion of root",
            "rm -fr /",
        ));
    }
    if rm_home_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive deletion of home directory",
            "rm -r ~",
        ));
    }
    if rm_system_dir_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive deletion of system directory",
            "rm -r /system-dir",
        ));
    }
    None
}

fn check_disk_operations(cmd: &str) -> Option<Verdict> {
    if dd_disk_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Direct disk write operation",
            "dd of=/dev/*",
        ));
    }
    if format_cmd_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Disk format operation",
            "mkfs/mke2fs/mkswap",
        ));
    }
    None
}

fn check_permission_changes(cmd: &str) -> Option<Verdict> {
    if chmod_777_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Dangerous permission change on system path",
            "chmod 777 /system-path",
        ));
    }
    if chmod_r_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive permission change on root",
            "chmod -R /",
        ));
    }
    if chown_r_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive ownership change on root",
            "chown -R /",
        ));
    }
    None
}

fn check_fork_bomb(cmd: &str) -> Option<Verdict> {
    if fork_bomb_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Fork bomb detected",
            ":(){:|:&};:",
        ));
    }
    if fork_bomb_dot_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Fork bomb variation detected",
            ".(){.|.&};.",
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    use crate::hooks::pre_tool_use::EnforcementPolicy;

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
        assert!(DangerousModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_rm_rf_root() {
        let v = DangerousModule.check(&ctx("rm -rf /")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Dangerous Command");
    }

    #[test]
    fn test_rm_rf_root_star() {
        assert!(DangerousModule.check(&ctx("rm -rf /*")).is_some());
    }

    #[test]
    fn test_rm_fr_root() {
        assert!(DangerousModule.check(&ctx("rm -fr /")).is_some());
    }

    #[test]
    fn test_rm_r_home() {
        assert!(DangerousModule.check(&ctx("rm -r ~")).is_some());
    }

    #[test]
    fn test_rm_r_system_dir() {
        assert!(DangerousModule.check(&ctx("rm -rf /etc")).is_some());
        assert!(DangerousModule.check(&ctx("rm -r /usr/")).is_some());
        assert!(DangerousModule.check(&ctx("rm -rf /var")).is_some());
    }

    #[test]
    fn test_dd_dev_zero() {
        assert!(
            DangerousModule
                .check(&ctx("dd if=/dev/zero of=file"))
                .is_some()
        );
    }

    #[test]
    fn test_dd_disk() {
        assert!(
            DangerousModule
                .check(&ctx("dd if=image of=/dev/sda"))
                .is_some()
        );
    }

    #[test]
    fn test_mkfs() {
        assert!(DangerousModule.check(&ctx("mkfs.ext4 /dev/sda1")).is_some());
    }

    #[test]
    fn test_chmod_777_system() {
        assert!(DangerousModule.check(&ctx("chmod 777 /etc")).is_some());
    }

    #[test]
    fn test_chmod_r_root() {
        assert!(DangerousModule.check(&ctx("chmod -R 755 /")).is_some());
    }

    #[test]
    fn test_chown_r_root() {
        assert!(
            DangerousModule
                .check(&ctx("chown -R root:root /"))
                .is_some()
        );
    }

    #[test]
    fn test_fork_bomb() {
        assert!(DangerousModule.check(&ctx(":(){ :|:& };:")).is_some());
    }

    #[test]
    fn test_safe_rm() {
        assert!(DangerousModule.check(&ctx("rm -rf /tmp/test")).is_none());
    }

    #[test]
    fn test_safe_chmod() {
        assert!(
            DangerousModule
                .check(&ctx("chmod 755 ./script.sh"))
                .is_none()
        );
    }
}
