//! Validates file operations for security violations.
//!
//! Sections: indirect file operations (cp, dd, tee), glob pattern bypass
//! prevention, interpreter-based file write detection.

use std::sync::OnceLock;

use regex::Regex;

use super::pattern::is_path_or_glob_targeted;
use super::{CheckContext, SecurityModule, Verdict, block};

fn indirect_write_cmds_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s|/)(cp|dd|tee|rsync|scp|install|ln)\s").expect("valid"))
}

fn interpreter_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s)(python3?|perl|ruby|node)\s+-[ce]\s").expect("valid"))
}

fn tmp_claude_final_dest_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s/tmp/claude($|/)\S*\s*$").expect("valid"))
}

fn tmp_claude_redirect_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r">/tmp/claude($|/)").expect("valid"))
}

/// Matches cp/rsync commands whose destination is the staging area.
/// The staging area mirrors the project structure under protected-edits/,
/// so destination paths contain protected path substrings.
fn staging_dest_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(cp|rsync)\s.*\s/tmp/claude/\S*/managed/protected-edits/").expect("valid")
    })
}

fn dd_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|\s)(dd)\s").expect("valid"))
}

fn piped_tee_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\|\s*(tee)\s").expect("valid"))
}

fn cat_append_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"cat\s.*>>\s*").expect("valid"))
}

/// Paths that shouldn't be written via interpreters.
const INTERPRETER_PROTECTED: &[&str] = &[
    ".claude/",
    ".state/",
    ".codeflow/config/",
    "settings.json",
    "enforcement-policy",
];

pub struct FileOpsModule;

impl SecurityModule for FileOpsModule {
    fn name(&self) -> &'static str {
        "file-operations"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;
        let paths = ctx.policy.all_protected_paths();
        if paths.is_empty() {
            return None;
        }

        if let Some(v) = check_indirect_file_ops(cmd, &paths) {
            return Some(v);
        }

        if let Some(v) = check_glob_bypass(cmd, &paths) {
            return Some(v);
        }

        if let Some(v) = check_interpreter_write(cmd) {
            return Some(v);
        }

        None
    }
}

fn check_indirect_file_ops(cmd: &str, paths: &[String]) -> Option<Verdict> {
    // Skip if the final destination is /tmp/claude (no command chains).
    let skip_section9 = (tmp_claude_final_dest_re().is_match(cmd)
        && !cmd.contains("&&")
        && !cmd.contains("||")
        && !cmd.contains(';'))
        || tmp_claude_redirect_re().is_match(cmd);

    // Staging area exemption: commands whose cp/write destination is
    // /tmp/claude/*/managed/protected-edits/ are staging copies for the
    // auto-staging workflow. Allow even in command chains (e.g.,
    // "mkdir -p /tmp/claude/.../dir && cp source /tmp/claude/.../dir/file").
    let is_staging_workflow =
        cmd.contains("/managed/protected-edits/") && staging_dest_re().is_match(cmd);

    if !skip_section9 && !is_staging_workflow {
        for path in paths {
            if is_path_or_glob_targeted(cmd, path) && indirect_write_cmds_re().is_match(cmd) {
                return Some(block(
                    "Protected Path Write",
                    "Indirect write operation targeting protected path",
                    path,
                ));
            }
        }
    }

    // dd command - special handling for of= parameter.
    if dd_cmd_re().is_match(cmd) {
        for path in paths {
            if cmd.contains("of=") && cmd.contains(path.as_str()) {
                return Some(block(
                    "Protected Path Copy",
                    "dd operation targeting protected path",
                    path,
                ));
            }
        }
    }

    // Piped tee to protected paths.
    if piped_tee_re().is_match(cmd) {
        for path in paths {
            if is_path_or_glob_targeted(cmd, path) {
                return Some(block(
                    "Protected Path Pipe",
                    "Piped tee to protected path",
                    path,
                ));
            }
        }
    }

    // Cat append redirection.
    if cat_append_re().is_match(cmd) {
        for path in paths {
            let qp = regex::escape(path);
            if let Ok(re) = Regex::new(&format!(r#">>\s*{qp}($|[\s"'/])"#)) {
                if re.is_match(cmd) {
                    return Some(block(
                        "Protected Path Append",
                        "cat append redirection to protected path",
                        path,
                    ));
                }
            }
        }
    }

    None
}

fn check_glob_bypass(cmd: &str, paths: &[String]) -> Option<Verdict> {
    if !indirect_write_cmds_re().is_match(cmd) {
        return None;
    }
    if !cmd.contains('*') && !cmd.contains('?') {
        return None;
    }

    for path in paths {
        // Check ? glob matching.
        if path.len() >= 2 {
            let prefix = &path[..path.len() - 1];
            if cmd.contains(&format!("{prefix}?")) {
                return Some(block(
                    "Glob Bypass Attempt",
                    "Glob pattern '?' could match protected path",
                    path,
                ));
            }
        }

        // Check * glob matching.
        let mut prefix = path.as_str();
        while prefix.len() >= 5 {
            if cmd.contains(prefix) {
                let qp = regex::escape(prefix);
                if let Ok(re) = Regex::new(&format!(r#"{qp}[^"/\s]*\*"#)) {
                    if re.is_match(cmd) {
                        return Some(block(
                            "Glob Bypass Attempt",
                            "Glob pattern '*' could match protected path",
                            path,
                        ));
                    }
                }
            }
            prefix = &prefix[..prefix.len() - 1];
        }
    }

    None
}

fn check_interpreter_write(cmd: &str) -> Option<Verdict> {
    if !interpreter_cmd_re().is_match(cmd) {
        return None;
    }

    for &pattern in INTERPRETER_PROTECTED {
        if cmd.contains(pattern) {
            return Some(block(
                "Interpreter File Write",
                "Interpreter-based file write to protected path detected. Use Edit/Write tools instead.",
                pattern,
            ));
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
        assert!(FileOpsModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_cp_to_protected_path() {
        assert!(
            FileOpsModule
                .check(&ctx("cp malicious.sh .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_cp_to_tmp_claude_allowed() {
        assert!(
            FileOpsModule
                .check(&ctx("cp file.txt /tmp/claude/test"))
                .is_none()
        );
    }

    #[test]
    fn test_dd_to_protected() {
        assert!(
            FileOpsModule
                .check(&ctx("dd if=evil of=.claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_piped_tee_protected() {
        assert!(
            FileOpsModule
                .check(&ctx("echo test | tee .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_interpreter_write() {
        assert!(
            FileOpsModule
                .check(&ctx("python3 -c 'open(\".claude/settings.json\",\"w\")'"))
                .is_some()
        );
    }

    #[test]
    fn test_interpreter_safe() {
        assert!(FileOpsModule.check(&ctx("python3 -c 'print(1)'")).is_none());
    }

    // -- Glob bypass tests --

    #[test]
    fn test_glob_question_mark_bypass() {
        // Use ? to match a protected path character.
        assert!(
            FileOpsModule
                .check(&ctx("cp malicious.sh .claude/settings.jso?"))
                .is_some()
        );
    }

    #[test]
    fn test_glob_star_bypass() {
        assert!(
            FileOpsModule
                .check(&ctx("cp malicious.sh .claude/settings*"))
                .is_some()
        );
    }

    #[test]
    fn test_glob_no_write_cmd_ignored() {
        // Glob patterns without a write command should not trigger.
        assert!(FileOpsModule.check(&ctx("ls .claude/settings*")).is_none());
    }

    #[test]
    fn test_glob_no_pattern_allowed() {
        // Write command without glob pattern.
        assert!(
            FileOpsModule
                .check(&ctx("cp safe.txt /tmp/output.txt"))
                .is_none()
        );
    }

    // -- dd command tests --

    #[test]
    fn test_dd_safe_no_protected_path() {
        assert!(
            FileOpsModule
                .check(&ctx("dd if=/dev/zero of=/tmp/test bs=1M count=1"))
                .is_none()
        );
    }

    // -- Piped tee tests --

    #[test]
    fn test_piped_tee_safe() {
        assert!(
            FileOpsModule
                .check(&ctx("echo test | tee /tmp/output.txt"))
                .is_none()
        );
    }

    // -- cat append tests --

    #[test]
    fn test_cat_append_to_protected() {
        assert!(
            FileOpsModule
                .check(&ctx("cat malicious.sh >> .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_cat_append_safe() {
        assert!(
            FileOpsModule
                .check(&ctx("cat file.txt >> /tmp/output.txt"))
                .is_none()
        );
    }

    // -- Interpreter tests for other languages --

    #[test]
    fn test_interpreter_perl_write() {
        assert!(
            FileOpsModule
                .check(&ctx("perl -e 'open(F, \">.claude/settings.json\")'"))
                .is_some()
        );
    }

    #[test]
    fn test_interpreter_ruby_write() {
        assert!(
            FileOpsModule
                .check(&ctx("ruby -e 'File.write(\".state/config\", \"evil\")'"))
                .is_some()
        );
    }

    #[test]
    fn test_interpreter_node_write() {
        assert!(
            FileOpsModule
                .check(&ctx(
                    "node -e 'require(\"fs\").writeFileSync(\".codeflow/config/test\", \"x\")'"
                ))
                .is_some()
        );
    }

    // -- tmp/claude exclusion --

    #[test]
    fn test_tmp_claude_redirect_allowed() {
        assert!(
            FileOpsModule
                .check(&ctx("cp file.txt >/tmp/claude/output"))
                .is_none()
        );
    }

    #[test]
    fn test_tmp_claude_with_chained_cmd_blocked() {
        // Chained commands with /tmp/claude should NOT be excluded.
        assert!(
            FileOpsModule
                .check(&ctx(
                    "cp .claude/settings.json /tmp/claude/test && cp /tmp/claude/test .claude/settings.json"
                ))
                .is_some()
        );
    }

    // -- rsync and scp --

    #[test]
    fn test_rsync_to_protected() {
        assert!(
            FileOpsModule
                .check(&ctx("rsync -a source/ .claude/settings.json"))
                .is_some()
        );
    }

    #[test]
    fn test_scp_to_protected() {
        assert!(
            FileOpsModule
                .check(&ctx("scp evil.txt .claude/settings.json"))
                .is_some()
        );
    }

    // -- Empty protected paths --

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
        assert!(FileOpsModule.check(&empty_ctx).is_none());
    }

    // -- Staging area exemption tests --

    #[test]
    fn test_cp_to_staging_with_chain_allowed() {
        // mkdir && cp to staging area — the exact workflow that was blocked.
        let result = FileOpsModule.check(&ctx(
            "mkdir -p /tmp/claude/codeflow/managed/protected-edits/.claude && cp .claude/CLAUDE.md /tmp/claude/codeflow/managed/protected-edits/.claude/CLAUDE.md",
        ));
        assert!(
            result.is_none(),
            "mkdir && cp to staging area should be allowed"
        );
    }

    #[test]
    fn test_cp_to_staging_simple_allowed() {
        // Simple cp to staging without chain.
        let result = FileOpsModule.check(&ctx(
            "cp .claude/CLAUDE.md /tmp/claude/codeflow/managed/protected-edits/.claude/CLAUDE.md",
        ));
        assert!(result.is_none(), "cp to staging area should be allowed");
    }

    #[test]
    fn test_cp_chain_to_non_staging_blocked() {
        // cp with chain to a non-staging destination — should still be blocked.
        let result = FileOpsModule.check(&ctx(
            "mkdir -p /tmp/evil && cp .claude/settings.json /tmp/evil/settings.json",
        ));
        assert!(
            result.is_some(),
            "cp with chain to non-staging dest should be blocked"
        );
    }

    #[test]
    fn test_staging_exemption_only_for_cp_rsync() {
        // tee is in indirect_write_cmds but NOT covered by staging exemption
        // (staging_dest_re only matches cp|rsync).
        let result = FileOpsModule.check(&ctx("echo evil | tee .claude/settings.json"));
        assert!(
            result.is_some(),
            "tee to protected path should still be blocked"
        );
    }
}
