//! Protects sensitive paths from dangerous operations.
//!
//! Checks: dangerous commands on protected paths, `.claude`/`.codeflow`
//! directory-level protection, redirect overwrite protection.

use std::sync::OnceLock;

use regex::Regex;

use super::pattern::{
    extract_variable_assignments, glob_to_regex, has_variable_indirection, is_glob_path_targeted,
    is_glob_pattern, is_path_or_glob_targeted, is_path_targeted, split_command_segments,
};
use super::{block, CheckContext, SecurityModule, Verdict};

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

        // Check variable indirection bypass (e.g., F=".claude/settings.json" && rm $F).
        if let Some(v) = check_variable_indirection(&segments, &paths) {
            return Some(v);
        }

        // Check eval with protected path literals (e.g., eval "rm .claude/settings.json").
        if let Some(v) = check_eval_bypass(&segments, &paths) {
            return Some(v);
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

fn protected_paths(policy: &crate::security::SecurityPolicy) -> Vec<String> {
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

        // cp special handling: only block if protected directory is the
        // destination, not the source. Copying OUT of a protected directory
        // is a read and stays allowed.
        if cp_cmd_re().is_match(seg) {
            let is_claude_dest = (claude.dir_end.is_match(seg) || claude.dir_slash.is_match(seg))
                && is_cp_destination_dir(seg, ".claude");
            let is_codeflow_dest = (codeflow.dir_end.is_match(seg)
                || codeflow.dir_slash.is_match(seg))
                && is_cp_destination_dir(seg, ".codeflow");

            if is_claude_dest {
                return Some(block(
                    "Protected Directory",
                    "Cannot copy to .claude directory",
                    ".claude",
                ));
            }
            if is_codeflow_dest {
                return Some(block(
                    "Protected Directory",
                    "Cannot copy to .codeflow directory",
                    ".codeflow",
                ));
            }
            // cp with protected dir as source → allowed (read-only copy)
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

/// Check if a protected directory pattern appears in the cp destination.
/// Uses the same last-non-flag heuristic as `is_cp_destination`.
fn is_cp_destination_dir(segment: &str, dir_pattern: &str) -> bool {
    let fields: Vec<&str> = segment.split_whitespace().collect();
    let mut last_non_flag = "";
    for f in &fields {
        if !f.starts_with('-') {
            last_non_flag = f;
        }
    }
    last_non_flag.contains(dir_pattern)
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

/// Detect `eval` commands that contain protected path literals.
fn check_eval_bypass(segments: &[String], paths: &[String]) -> Option<Verdict> {
    for seg in segments {
        let trimmed = seg.trim();
        let first_word = trimmed.split_whitespace().next().unwrap_or("");
        if first_word != "eval" {
            continue;
        }
        for path in paths {
            if is_path_or_glob_targeted(trimmed, path) {
                return Some(block(
                    "Protected Path Indirection",
                    "eval command with protected path argument",
                    path,
                ));
            }
        }
    }
    None
}

/// Detect dangerous commands using shell variables assigned protected path values.
fn check_variable_indirection(segments: &[String], paths: &[String]) -> Option<Verdict> {
    let assignments = extract_variable_assignments(segments);
    if assignments.is_empty() {
        return None;
    }

    let mut protected_vars: Vec<(&str, &str)> = Vec::new();
    for (name, value) in &assignments {
        for path in paths {
            // Match if the value IS a protected path, or if any protected
            // path starts with the value as a directory prefix (e.g.,
            // ".claude" is a parent of ".claude/settings.json").
            if is_path_or_glob_targeted(value, path)
                || value.as_str() == path
                || path.starts_with(&format!("{value}/"))
            {
                protected_vars.push((name, path));
                break;
            }
        }
    }
    if protected_vars.is_empty() {
        return None;
    }

    let dangerous = dangerous_cmds_re();
    let perm = permission_cmds_re();
    let git = git_rm_re();

    for seg in segments {
        let trimmed = seg.trim();
        if !dangerous.is_match(trimmed) && !perm.is_match(trimmed) && !git.is_match(trimmed) {
            continue;
        }
        if !has_variable_indirection(trimmed) {
            continue;
        }
        if let Some((_, protected_path)) = protected_vars.first() {
            return Some(block(
                "Protected Path Indirection",
                "Variable indirection targeting protected path",
                protected_path,
            ));
        }
    }

    None
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
        assert!(PathModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_rm_settings_json() {
        assert!(PathModule.check(&ctx("rm .claude/settings.json")).is_some());
    }

    #[test]
    fn test_chmod_settings() {
        assert!(PathModule
            .check(&ctx("chmod 777 .claude/settings.json"))
            .is_some());
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
        assert!(PathModule
            .check(&ctx("echo test > .claude/settings.json"))
            .is_some());
    }

    #[test]
    fn test_git_rm_protected() {
        assert!(PathModule
            .check(&ctx("git rm .claude/settings.json"))
            .is_some());
    }

    #[test]
    fn test_safe_rm_unprotected() {
        assert!(PathModule.check(&ctx("rm /tmp/test.txt")).is_none());
    }

    // -- cp destination checks --

    #[test]
    fn test_cp_to_protected_destination() {
        assert!(PathModule
            .check(&ctx("cp evil.txt .claude/settings.json"))
            .is_some());
    }

    #[test]
    fn test_cp_from_protected_source_allowed() {
        // cp FROM a protected path should be allowed (reading, not writing).
        assert!(PathModule
            .check(&ctx("cp .claude/settings.json /tmp/backup.json"))
            .is_none());
    }

    // -- Append redirect --

    #[test]
    fn test_append_redirect_to_protected() {
        assert!(PathModule
            .check(&ctx("echo test >> .claude/settings.json"))
            .is_some());
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
        assert!(PathModule
            .check(&ctx("chown root .claude/settings.json"))
            .is_some());
    }

    // -- Chained commands with protected paths --

    #[test]
    fn test_chained_rm_protected() {
        assert!(PathModule
            .check(&ctx("ls -la && rm .claude/settings.json"))
            .is_some());
    }

    // -- sed on protected path --

    #[test]
    fn test_sed_on_protected() {
        assert!(PathModule
            .check(&ctx("sed -i 's/old/new/' .claude/settings.json"))
            .is_some());
    }

    // -- touch on protected path --

    #[test]
    fn test_touch_protected() {
        assert!(PathModule
            .check(&ctx("touch .claude/settings.json"))
            .is_some());
    }

    // -- truncate on protected path --

    #[test]
    fn test_truncate_protected() {
        assert!(PathModule
            .check(&ctx("truncate -s 0 .claude/settings.json"))
            .is_some());
    }

    // -- unlink on protected path --

    #[test]
    fn test_unlink_protected() {
        assert!(PathModule
            .check(&ctx("unlink .claude/settings.json"))
            .is_some());
    }

    // -- shred on protected path --

    #[test]
    fn test_shred_protected() {
        assert!(PathModule
            .check(&ctx("shred .claude/settings.json"))
            .is_some());
    }

    // -- Empty protected paths returns None --

    #[test]
    fn test_empty_protected_paths() {
        let empty_policy = SecurityPolicy {
            protected_paths: vec![],
            ..SecurityPolicy::defaults()
        };
        let empty_ctx = CheckContext {
            command: "rm -rf .claude/settings.json",
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: &empty_policy,
        };
        assert!(PathModule.check(&empty_ctx).is_none());
    }

    // -- Variable indirection bypass --

    #[test]
    fn test_indirection_rm_var_and() {
        // F=".claude/settings.json" && rm $F → BLOCKED
        let result = PathModule.check(&ctx(r#"F=".claude/settings.json" && rm $F"#));
        assert!(result.is_some(), "Should block rm via variable indirection");
    }

    #[test]
    fn test_indirection_rm_var_semicolon() {
        // F=".claude/settings.json"; rm "$F" → BLOCKED
        let result = PathModule.check(&ctx(r#"F=".claude/settings.json"; rm "$F""#));
        assert!(
            result.is_some(),
            "Should block rm via variable indirection with semicolon"
        );
    }

    #[test]
    fn test_indirection_rm_braced_var() {
        // DIR=".claude" && rm -rf ${DIR} → BLOCKED
        let result = PathModule.check(&ctx(r#"DIR=".claude" && rm -rf ${DIR}"#));
        assert!(
            result.is_some(),
            "Should block rm via braced variable indirection"
        );
    }

    #[test]
    fn test_eval_rm_protected() {
        // eval "rm .claude/settings.json" → BLOCKED
        let result = PathModule.check(&ctx(r#"eval "rm .claude/settings.json""#));
        assert!(result.is_some(), "Should block eval with protected path rm");
    }

    #[test]
    fn test_indirection_safe_path_allowed() {
        // X="/tmp/safe" && rm $X → ALLOWED (not a protected path)
        let result = PathModule.check(&ctx(r#"X="/tmp/safe" && rm $X"#));
        assert!(
            result.is_none(),
            "Should allow rm via variable when path is not protected"
        );
    }

    #[test]
    fn test_indirection_echo_path_allowed() {
        // echo $PATH → ALLOWED (no dangerous cmd on protected path)
        let result = PathModule.check(&ctx("echo $PATH"));
        assert!(result.is_none(), "Should allow echo $PATH");
    }

    #[test]
    fn test_indirection_cat_protected_allowed() {
        // F=".claude/settings.json" && cat $F → ALLOWED (cat is not dangerous)
        let result = PathModule.check(&ctx(r#"F=".claude/settings.json" && cat $F"#));
        assert!(
            result.is_none(),
            "Should allow cat via variable indirection (read-only)"
        );
    }

    #[test]
    fn test_indirection_glob_path_via_var() {
        // P=".codeflow/.baseline/**" && rm $P → BLOCKED
        let result = PathModule.check(&ctx(r#"P=".codeflow/.baseline/**" && rm $P"#));
        assert!(
            result.is_some(),
            "Should block rm via variable with glob protected path"
        );
    }

    #[test]
    fn test_eval_false_positive_gh_pr() {
        // gh pr create --body "checkEvalBypass detects eval" → ALLOWED
        let result = PathModule.check(&ctx(
            r#"gh pr create --body "checkEvalBypass detects eval .claude/settings.json""#,
        ));
        assert!(
            result.is_none(),
            "Should allow gh pr with eval mentioned in body text"
        );
    }

    #[test]
    fn test_eval_false_positive_echo() {
        // echo "the eval command is dangerous" → ALLOWED
        let result = PathModule.check(&ctx(
            r#"echo "the eval command is dangerous .claude/settings.json""#,
        ));
        assert!(
            result.is_none(),
            "Should allow echo with eval mentioned in string"
        );
    }

    #[test]
    fn test_eval_still_blocked() {
        // eval "rm .claude/settings.json" → BLOCKED (actual eval command)
        let result = PathModule.check(&ctx(r#"eval "rm .claude/settings.json""#));
        assert!(
            result.is_some(),
            "Should still block actual eval with protected path"
        );
    }

    // -- cp directory protection: source vs destination --

    #[test]
    fn test_cp_from_claude_dir_allowed() {
        // cp FROM the .claude/ directory to a scratch path → ALLOWED
        // (read-only copy; only writes into protected directories block).
        let result = PathModule.check(&ctx("cp .claude/agents/cf-reviewer.md /tmp/backup.md"));
        assert!(result.is_none(), "cp FROM .claude dir should be allowed");
    }

    #[test]
    fn test_cp_to_claude_dir_blocked() {
        // cp TO .claude directory → BLOCKED (write to protected directory).
        let result = PathModule.check(&ctx("cp /tmp/evil.md .claude"));
        assert!(result.is_some(), "cp TO .claude dir should be blocked");
    }

    #[test]
    fn test_cp_to_claude_slash_dir_blocked() {
        // cp TO .claude/ directory → BLOCKED.
        let result = PathModule.check(&ctx("cp -r /tmp/evil/ .claude/"));
        assert!(result.is_some(), "cp TO .claude/ dir should be blocked");
    }

    #[test]
    fn test_cp_to_codeflow_dir_blocked() {
        // cp TO .codeflow directory → BLOCKED.
        let result = PathModule.check(&ctx("cp -r /tmp/evil/ .codeflow/"));
        assert!(result.is_some(), "cp TO .codeflow dir should be blocked");
    }

    #[test]
    fn test_cp_between_unprotected_allowed() {
        // cp between unprotected paths → ALLOWED.
        let result = PathModule.check(&ctx("cp /tmp/a.txt /tmp/b.txt"));
        assert!(
            result.is_none(),
            "cp between unprotected paths should be allowed"
        );
    }

    #[test]
    fn test_rm_claude_dir_still_blocked() {
        // Regression: rm on .claude directory must still be blocked after cp fix.
        let result = PathModule.check(&ctx("rm -rf .claude"));
        assert!(
            result.is_some(),
            "rm on .claude dir should still be blocked"
        );
    }

    #[test]
    fn test_rm_codeflow_dir_still_blocked() {
        // Regression: rm on .codeflow directory must still be blocked.
        let result = PathModule.check(&ctx("rm -rf .codeflow/"));
        assert!(
            result.is_some(),
            "rm on .codeflow dir should still be blocked"
        );
    }

    #[test]
    fn test_mv_claude_dir_still_blocked() {
        // mv .claude is destructive — should be blocked.
        let result = PathModule.check(&ctx("mv .claude /tmp/backup"));
        assert!(
            result.is_some(),
            "mv .claude dir should be blocked (destructive)"
        );
    }

    // -- cp into protected files --

    #[test]
    fn test_cp_from_tmp_to_protected_file_blocked() {
        // Copying FROM scratch space TO a protected file is a write.
        let result = PathModule.check(&ctx("cp /tmp/staged.json .claude/settings.json"));
        assert!(result.is_some(), "cp TO a protected path should be blocked");
    }

    #[test]
    fn test_cp_policy_file_to_tmp_allowed() {
        // Reading the policy out to scratch space is fine.
        let result = PathModule.check(&ctx("cp .codeflow/policy.json /tmp/policy-copy.json"));
        assert!(
            result.is_none(),
            "cp FROM a protected path to scratch should be allowed"
        );
    }
}
