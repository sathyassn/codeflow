//! Protects sensitive paths from dangerous operations.
//!
//! Checks: dangerous commands on protected paths, `.claude`/`.codeflow`
//! directory-level protection, redirect overwrite protection.

use std::sync::OnceLock;

use regex::Regex;

use super::pattern::{
    glob_to_regex, is_glob_path_targeted, is_glob_pattern, is_path_targeted, split_command_segments,
};
use super::{CheckContext, SecurityModule, Verdict, block};

fn dangerous_cmds_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(rm|unlink|mv|cp|shred|truncate|touch|sed)\s").expect("valid"))
}

fn permission_cmds_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(chmod|chown)\s").expect("valid"))
}

fn git_rm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"git\s+rm").expect("valid"))
}

fn cp_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s)(cp)\s").expect("valid"))
}

fn other_dangerous_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(rm|unlink|mv|shred|truncate|touch|sed)\s").expect("valid"))
}

/// Pre-compiled directory protection patterns.
struct DirProtection {
    dir_end: Regex,
    dir_slash: Regex,
}

fn claude_dir_patterns() -> &'static DirProtection {
    static P: OnceLock<DirProtection> = OnceLock::new();
    P.get_or_init(|| DirProtection {
        dir_end: Regex::new(r#"\s[^\s]*\.claude($|\s|['"])"#).expect("valid"),
        dir_slash: Regex::new(r#"\s[^\s]*\.claude/($|\s|['"])"#).expect("valid"),
    })
}

fn codeflow_dir_patterns() -> &'static DirProtection {
    static P: OnceLock<DirProtection> = OnceLock::new();
    P.get_or_init(|| DirProtection {
        dir_end: Regex::new(r#"\s[^\s]*\.codeflow($|\s|['"])"#).expect("valid"),
        dir_slash: Regex::new(r#"\s[^\s]*\.codeflow/($|\s|['"])"#).expect("valid"),
    })
}

pub struct PathModule;

impl SecurityModule for PathModule {
    fn name(&self) -> &'static str {
        "path-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;
        let paths = protected_paths(ctx.policy);
        if paths.is_empty() {
            return None;
        }

        let segments = split_command_segments(cmd);

        // Check protected path operations per segment.
        for path in &paths {
            for seg in &segments {
                if let Some(v) = check_segment_dangerous_op(seg, path, ctx) {
                    return Some(v);
                }
            }
        }

        // Check .claude and .codeflow directory-level protection.
        if let Some(v) = check_directory_protection(&segments) {
            return Some(v);
        }

        // Check redirect overwrite protection.
        if let Some(v) = check_redirect_protection(&segments, &paths) {
            return Some(v);
        }

        None
    }
}

fn protected_paths(policy: &crate::hooks::pre_tool_use::EnforcementPolicy) -> Vec<String> {
    policy.all_protected_paths()
}

fn check_segment_dangerous_op(
    segment: &str,
    path: &str,
    _ctx: &CheckContext<'_>,
) -> Option<Verdict> {
    let is_glob = is_glob_pattern(path);

    let path_in_segment = if is_glob {
        is_glob_path_targeted(segment, path)
    } else {
        is_path_targeted(segment, path)
    };

    if !path_in_segment {
        return None;
    }

    // cp special handling: only block if protected path is the destination.
    if cp_cmd_re().is_match(segment) && is_cp_destination(segment, path, is_glob) {
        return Some(block(
            "Protected Path Deletion",
            "Dangerous operation on protected path",
            path,
        ));
    }

    if other_dangerous_re().is_match(segment) {
        return Some(block(
            "Protected Path Deletion",
            "Dangerous operation on protected path",
            path,
        ));
    }

    if permission_cmds_re().is_match(segment) {
        return Some(block(
            "Protected Path Manipulation",
            "Permission change on protected path",
            path,
        ));
    }

    if git_rm_re().is_match(segment) {
        return Some(block(
            "Protected Path Deletion",
            "Git removal of protected path",
            path,
        ));
    }

    None
}

fn is_cp_destination(segment: &str, path: &str, is_glob: bool) -> bool {
    let fields: Vec<&str> = segment.split_whitespace().collect();
    let mut last_non_flag = "";
    for f in &fields {
        if !f.starts_with('-') {
            last_non_flag = f;
        }
    }
    let last_non_flag = last_non_flag.trim_matches(|c| c == '"' || c == '\'');

    if is_glob {
        Regex::new(&glob_to_regex(path)).is_ok_and(|re| re.is_match(last_non_flag))
    } else {
        last_non_flag.contains(path)
    }
}

fn check_directory_protection(segments: &[String]) -> Option<Verdict> {
    let claude = claude_dir_patterns();
    let codeflow = codeflow_dir_patterns();

    for seg in segments {
        if !dangerous_cmds_re().is_match(seg) {
            continue;
        }

        if claude.dir_end.is_match(seg) || claude.dir_slash.is_match(seg) {
            return Some(block(
                "Protected Directory",
                "Cannot delete .claude directory",
                ".claude",
            ));
        }
        if codeflow.dir_end.is_match(seg) || codeflow.dir_slash.is_match(seg) {
            return Some(block(
                "Protected Directory",
                "Cannot delete .codeflow directory",
                ".codeflow",
            ));
        }
    }

    None
}

fn check_redirect_protection(segments: &[String], paths: &[String]) -> Option<Verdict> {
    for path in paths {
        for seg in segments {
            if is_glob_pattern(path) {
                let regex = glob_to_regex(path);
                if let Ok(re) = Regex::new(&format!(r"([^0-9&>]|^)>[\s]*.*{regex}")) {
                    if re.is_match(seg) {
                        return Some(block(
                            "Protected File Overwrite",
                            "Redirect overwrite of protected path",
                            &format!("> {path}"),
                        ));
                    }
                }
                if let Ok(re) = Regex::new(&format!(r"([^0-9&]|^)>>[\s]*.*{regex}")) {
                    if re.is_match(seg) {
                        return Some(block(
                            "Protected File Append",
                            "Redirect append to protected path",
                            &format!(">> {path}"),
                        ));
                    }
                }
            } else {
                let qp = regex::escape(path);
                if let Ok(re) = Regex::new(&format!(r#"([^0-9&>]|^)>\s*{qp}($|[\s"'/])"#)) {
                    if re.is_match(seg) {
                        return Some(block(
                            "Protected File Overwrite",
                            "Redirect overwrite of protected path",
                            &format!("> {path}"),
                        ));
                    }
                }
                if let Ok(re) = Regex::new(&format!(r#"([^0-9&]|^)>>\s*{qp}($|[\s"'/])"#)) {
                    if re.is_match(seg) {
                        return Some(block(
                            "Protected File Append",
                            "Redirect append to protected path",
                            &format!(">> {path}"),
                        ));
                    }
                }
            }
        }
    }

    None
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
        assert!(PathModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_rm_settings_json() {
        assert!(PathModule.check(&ctx("rm .claude/settings.json")).is_some());
    }

    #[test]
    fn test_chmod_settings() {
        assert!(
            PathModule
                .check(&ctx("chmod 777 .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_rm_claude_directory() {
        assert!(PathModule.check(&ctx("rm -rf .claude")).is_some());
    }

    #[test]
    fn test_rm_codeflow_directory() {
        assert!(PathModule.check(&ctx("rm -rf .codeflow")).is_some());
    }

    #[test]
    fn test_redirect_to_protected() {
        assert!(
            PathModule
                .check(&ctx("echo test > .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_git_rm_protected() {
        assert!(
            PathModule
                .check(&ctx("git rm .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_safe_rm_unprotected() {
        assert!(PathModule.check(&ctx("rm /tmp/test.txt")).is_none());
    }

    // -- cp destination checks --

    #[test]
    fn test_cp_to_protected_destination() {
        assert!(
            PathModule
                .check(&ctx("cp evil.txt .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_cp_from_protected_source_allowed() {
        // cp FROM a protected path should be allowed (reading, not writing).
        assert!(
            PathModule
                .check(&ctx("cp .claude/settings.json /tmp/backup.json"))
                .is_none()
        );
    }

    // -- Append redirect --

    #[test]
    fn test_append_redirect_to_protected() {
        assert!(
            PathModule
                .check(&ctx("echo test >> .claude/settings.json"))
                .is_some()
        );
    }

    // -- .codeflow directory protection --

    #[test]
    fn test_rm_codeflow_slash() {
        assert!(PathModule.check(&ctx("rm -rf .codeflow/")).is_some());
    }

    #[test]
    fn test_mv_claude_directory() {
        assert!(PathModule.check(&ctx("mv .claude /tmp/backup")).is_some());
    }

    #[test]
    fn test_mv_codeflow_directory() {
        assert!(PathModule.check(&ctx("mv .codeflow /tmp/backup")).is_some());
    }

    // -- Permission changes --

    #[test]
    fn test_chown_protected() {
        assert!(
            PathModule
                .check(&ctx("chown root .claude/settings.json"))
                .is_some()
        );
    }

    // -- Chained commands with protected paths --

    #[test]
    fn test_chained_rm_protected() {
        assert!(
            PathModule
                .check(&ctx("ls -la && rm .claude/settings.json"))
                .is_some()
        );
    }

    // -- sed on protected path --

    #[test]
    fn test_sed_on_protected() {
        assert!(
            PathModule
                .check(&ctx("sed -i 's/old/new/' .claude/settings.json"))
                .is_some()
        );
    }

    // -- touch on protected path --

    #[test]
    fn test_touch_protected() {
        assert!(
            PathModule
                .check(&ctx("touch .claude/settings.json"))
                .is_some()
        );
    }

    // -- truncate on protected path --

    #[test]
    fn test_truncate_protected() {
        assert!(
            PathModule
                .check(&ctx("truncate -s 0 .claude/settings.json"))
                .is_some()
        );
    }

    // -- unlink on protected path --

    #[test]
    fn test_unlink_protected() {
        assert!(
            PathModule
                .check(&ctx("unlink .claude/settings.json"))
                .is_some()
        );
    }

    // -- shred on protected path --

    #[test]
    fn test_shred_protected() {
        assert!(
            PathModule
                .check(&ctx("shred .claude/settings.json"))
                .is_some()
        );
    }

    // -- Empty protected paths returns None --

    #[test]
    fn test_empty_protected_paths() {
        let empty_policy = EnforcementPolicy {
            protected_resources: crate::hooks::pre_tool_use::ProtectedResources {
                critical: vec![],
                high: vec![],
                moderate: vec![],
            },
            ..EnforcementPolicy::defaults()
        };
        let empty_ctx = CheckContext {
            tool_name: "Bash",
            command: "rm -rf .claude/settings.json",
            sandbox_bypass: false,
            project_dir: "/tmp/test",
            session_id: "ses-test",
            current_branch: "feat/test",
            is_pathflow_active: false,
            policy: &empty_policy,
        };
        assert!(PathModule.check(&empty_ctx).is_none());
    }
}
