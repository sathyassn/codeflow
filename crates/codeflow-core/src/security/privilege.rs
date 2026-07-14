//! Detects and blocks privilege escalation attempts.
//!
//! Checks for: sudo/su/doas, shell chaining to privilege commands,
//! script bypass (bash -c, eval), environment manipulation (`LD_PRELOAD`).

use std::sync::OnceLock;

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

/// Commands that escalate privileges.
const PRIV_ESC_CMDS: &[&str] = &["sudo", "su", "doas", "pkexec", "runuser"];

fn bash_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"bash\s+-c\s+["']"#).expect("valid"))
}

fn sh_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Anchor to start-of-string or whitespace so the "sh" tail of "zsh -c"
    // is not misclassified as "sh -c" (which would shadow zsh_c_re below).
    RE.get_or_init(|| Regex::new(r#"(?:^|\s)sh\s+-c\s+["']"#).expect("valid"))
}

fn zsh_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"zsh\s+-c\s+["']"#).expect("valid"))
}

fn eval_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"eval\s+.*(rm|sudo|su|doas|pkexec|runuser)").expect("valid"))
}

fn source_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^source\s").expect("valid"))
}

fn dot_source_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\.\s+").expect("valid"))
}

fn path_tmp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"PATH=.*:/tmp").expect("valid"))
}

fn ld_preload_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"LD_PRELOAD=").expect("valid"))
}

fn ld_lib_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"LD_LIBRARY_PATH=").expect("valid"))
}

/// Build pipe-to-privilege patterns lazily.
fn priv_pipe_patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        PRIV_ESC_CMDS
            .iter()
            .map(|priv_cmd| {
                Regex::new(&format!(r"\|\s*{}\s", regex::escape(priv_cmd))).expect("valid")
            })
            .collect()
    })
}

pub struct PrivilegeModule;

impl SecurityModule for PrivilegeModule {
    fn name(&self) -> &'static str {
        "privilege-protection"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        if let Some(v) = check_priv_chaining(cmd) {
            return Some(v);
        }

        if let Some(v) = check_direct_priv_esc(cmd) {
            return Some(v);
        }

        if let Some(v) = check_script_bypass(cmd) {
            return Some(v);
        }

        if let Some(v) = check_env_manipulation(cmd) {
            return Some(v);
        }

        None
    }
}

fn check_priv_chaining(cmd: &str) -> Option<Verdict> {
    let pipe_patterns = priv_pipe_patterns();

    for (i, priv_cmd) in PRIV_ESC_CMDS.iter().enumerate() {
        // && or || chaining.
        if cmd.contains(&format!("&& {priv_cmd} ")) || cmd.contains(&format!("|| {priv_cmd} ")) {
            return Some(block(
                "Privilege Escalation",
                &format!("Chained {priv_cmd} command"),
                &format!("&& {priv_cmd} / || {priv_cmd}"),
            ));
        }
        // Semicolon chaining.
        if cmd.contains(&format!("; {priv_cmd}")) {
            return Some(block(
                "Privilege Escalation",
                &format!("Semicolon chained {priv_cmd}"),
                &format!("; {priv_cmd}"),
            ));
        }
        // Pipe to privilege command.
        if pipe_patterns[i].is_match(cmd) {
            return Some(block(
                "Privilege Escalation",
                &format!("Piped to {priv_cmd} command"),
                &format!("| {priv_cmd}"),
            ));
        }
    }
    None
}

fn check_direct_priv_esc(cmd: &str) -> Option<Verdict> {
    for priv_cmd in PRIV_ESC_CMDS {
        if cmd.starts_with(&format!("{priv_cmd} ")) || cmd == *priv_cmd {
            return Some(block(
                "Privilege Escalation",
                &format!("{priv_cmd} command not permitted"),
                priv_cmd,
            ));
        }
    }
    None
}

fn check_script_bypass(cmd: &str) -> Option<Verdict> {
    if bash_c_re().is_match(cmd) {
        return Some(block("Script Bypass", "bash -c execution", "bash -c"));
    }
    if sh_c_re().is_match(cmd) {
        return Some(block("Script Bypass", "sh -c execution", "sh -c"));
    }
    if zsh_c_re().is_match(cmd) {
        return Some(block("Script Bypass", "zsh -c execution", "zsh -c"));
    }
    if eval_re().is_match(cmd) {
        return Some(block(
            "Script Bypass",
            "eval with dangerous command",
            "eval",
        ));
    }
    if source_cmd_re().is_match(cmd) {
        return Some(block(
            "Script Bypass",
            "source command not permitted",
            "source",
        ));
    }
    if dot_source_re().is_match(cmd) {
        return Some(block(
            "Script Bypass",
            "dot source command not permitted",
            ". (dot source)",
        ));
    }
    None
}

fn check_env_manipulation(cmd: &str) -> Option<Verdict> {
    if path_tmp_re().is_match(cmd) {
        return Some(block(
            "Environment Manipulation",
            "PATH modification with /tmp",
            "PATH=/tmp",
        ));
    }
    if ld_preload_re().is_match(cmd) {
        return Some(block(
            "Environment Manipulation",
            "LD_PRELOAD injection attempt",
            "LD_PRELOAD",
        ));
    }
    if ld_lib_path_re().is_match(cmd) {
        return Some(block(
            "Environment Manipulation",
            "LD_LIBRARY_PATH injection attempt",
            "LD_LIBRARY_PATH",
        ));
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
        assert!(PrivilegeModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_sudo_direct() {
        let v = PrivilegeModule.check(&ctx("sudo apt-get install")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Privilege Escalation");
    }

    #[test]
    fn test_su_direct() {
        assert!(PrivilegeModule.check(&ctx("su root")).is_some());
    }

    #[test]
    fn test_doas_direct() {
        assert!(PrivilegeModule
            .check(&ctx("doas cat /etc/shadow"))
            .is_some());
    }

    #[test]
    fn test_chained_sudo() {
        assert!(PrivilegeModule
            .check(&ctx("echo test && sudo rm file"))
            .is_some());
    }

    #[test]
    fn test_or_chained_sudo() {
        // The `|| {priv}` OR-chaining branch (distinct from `&&`).
        assert!(PrivilegeModule.check(&ctx("false || sudo rm f")).is_some());
    }

    #[test]
    fn test_semicolon_sudo() {
        assert!(PrivilegeModule
            .check(&ctx("echo test; sudo rm file"))
            .is_some());
    }

    #[test]
    fn test_piped_sudo() {
        assert!(PrivilegeModule
            .check(&ctx("echo test | sudo tee file"))
            .is_some());
    }

    #[test]
    fn test_bash_c() {
        assert!(PrivilegeModule.check(&ctx("bash -c 'echo test'")).is_some());
    }

    #[test]
    fn test_sh_c() {
        let v = PrivilegeModule.check(&ctx("sh -c 'x'")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Script Bypass");
        assert_eq!(v.reason, "sh -c execution");
        assert_eq!(v.pattern, "sh -c");
    }

    #[test]
    fn test_zsh_c() {
        // Regression: the "sh" tail of "zsh -c" must not be mislabeled as
        // "sh -c" — zsh_c_re owns this input and reports the zsh label.
        let v = PrivilegeModule.check(&ctx("zsh -c 'x'")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Script Bypass");
        assert_eq!(v.reason, "zsh -c execution");
        assert_eq!(v.pattern, "zsh -c");
    }

    #[test]
    fn test_eval_with_rm() {
        assert!(PrivilegeModule.check(&ctx("eval rm -rf /")).is_some());
    }

    #[test]
    fn test_source_command() {
        assert!(PrivilegeModule.check(&ctx("source /etc/profile")).is_some());
    }

    #[test]
    fn test_dot_source() {
        assert!(PrivilegeModule.check(&ctx(". /etc/profile")).is_some());
    }

    #[test]
    fn test_ld_preload() {
        assert!(PrivilegeModule
            .check(&ctx("LD_PRELOAD=/tmp/evil.so ls"))
            .is_some());
    }

    #[test]
    fn test_ld_library_path() {
        assert!(PrivilegeModule
            .check(&ctx("LD_LIBRARY_PATH=/tmp ls"))
            .is_some());
    }

    #[test]
    fn test_path_tmp() {
        assert!(PrivilegeModule.check(&ctx("PATH=/usr/bin:/tmp")).is_some());
    }
}
