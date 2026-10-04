//! Detects privilege escalation attempts.
//!
//! Checks for: Unix and Windows privilege launchers, run directly, chained,
//! piped to, or wrapped in a quoted shell `-c` string or an `eval`; and
//! environment manipulation (`LD_PRELOAD`, a `PATH` through `/tmp`).
//!
//! A shell `-c` string or `eval` that reaches no launcher, a leading
//! `source` or `.`, and `LD_LIBRARY_PATH` are ordinary work and are not
//! reported here: `security.privilege_escalation` blocks by default
//! (ADR-0075 D5), and these forms are script findings for
//! `security.script_bypass`, which TSK-172 reads.

use std::sync::OnceLock;

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

/// Commands that escalate privileges.
const PRIV_ESC_CMDS: &[&str] = &["sudo", "su", "doas", "pkexec", "runuser"];

fn windows_priv_esc_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[;&|]\s*)(?:\S*[\\/])?(?:gsudo|runas(?:\.exe)?)(?:\s|$)")
            .expect("valid")
    })
}

fn elevated_start_process_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bStart-Process\b[^\r\n]*\s-Verb\s+RunAs\b").expect("valid"))
}

fn bash_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"bash\s+-[a-z]*c\s+["']"#).expect("valid"))
}

fn sh_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Anchor to start-of-string or whitespace so the "sh" tail of "zsh -c"
    // is not misclassified as "sh -c" (which would shadow zsh_c_re below).
    RE.get_or_init(|| Regex::new(r#"(?:^|\s)sh\s+-[a-z]*c\s+["']"#).expect("valid"))
}

fn zsh_c_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"zsh\s+-[a-z]*c\s+["']"#).expect("valid"))
}

fn eval_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[\s;&|(])eval\s").expect("valid"))
}

/// A privilege launcher as a whole word, optionally path-qualified, inside
/// a wrapped command string: `sudo` in `bash -c "sudo id"`, but not in
/// `sudoku` or `pseudo`.
fn launcher_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(?:^|[\s;&|("'`])(?:[^\s;&|("'`]*[\\/])?(sudo|su|doas|pkexec|runuser|gsudo|runas)(?:\.exe)?(?:$|[\s;&|)"'`])"#,
        )
        .expect("valid")
    })
}

fn path_tmp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"PATH=.*:/tmp").expect("valid"))
}

fn ld_preload_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"LD_PRELOAD=").expect("valid"))
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

        if let Some(v) = check_wrapped_launcher(cmd) {
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
        if semicolon_chains_launcher(cmd, priv_cmd) {
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

/// True when `; {priv_cmd}` appears with the launcher name ending at a word
/// boundary, so a launch such as `; su -` refuses but `; supersedes` and
/// `; doasync` do not. The test stays on the raw line: it reads no quoting,
/// heredoc or other text structure, so a launch inside a string is still
/// refused as before.
fn semicolon_chains_launcher(cmd: &str, priv_cmd: &str) -> bool {
    let needle = format!("; {priv_cmd}");
    cmd.match_indices(&needle).any(|(at, _)| {
        cmd[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(ends_launcher_word)
    })
}

/// A character after a launcher name that ends the word: whitespace, a
/// control character, or a shell metacharacter or expansion starter that
/// leaves the name as the program (`su;`, `su|`, `su$IFS`, `su{,}`, `su*`, or
/// a quote, which can end the name as in an empty string `su` then two quotes).
/// Any other character continues the word, as in `supersedes`.
fn ends_launcher_word(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            ';' | '&' | '|' | '(' | ')' | '<' | '>' | '"' | '\'' | '`' | '$' | '\\' | '{' | '*'
        )
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
    if windows_priv_esc_re().is_match(cmd) {
        return Some(block(
            "Privilege Escalation",
            "Windows privilege launcher not permitted",
            "gsudo/runas",
        ));
    }
    if elevated_start_process_re().is_match(cmd) {
        return Some(block(
            "Privilege Escalation",
            "Elevated PowerShell process launch not permitted",
            "Start-Process -Verb RunAs",
        ));
    }
    None
}

/// A launcher reached through a quoted shell `-c` string or an `eval`.
fn check_wrapped_launcher(cmd: &str) -> Option<Verdict> {
    let wrapper = [
        (bash_c_re(), "bash -c"),
        (sh_c_re(), "sh -c"),
        (zsh_c_re(), "zsh -c"),
        (eval_re(), "eval"),
    ]
    .into_iter()
    .find(|(re, _)| re.is_match(cmd))
    .map(|(_, name)| name)?;
    let launcher = launcher_word_re().captures(cmd)?.get(1)?.as_str();
    Some(block(
        "Privilege Escalation",
        &format!("{launcher} wrapped in {wrapper}"),
        wrapper,
    ))
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
    fn test_windows_privilege_launchers() {
        for cmd in [
            "gsudo winget upgrade",
            "runas /user:Administrator cmd",
            "echo ok && gsudo Remove-Item file",
            "Start-Process powershell -Verb RunAs",
        ] {
            assert!(
                PrivilegeModule.check(&ctx(cmd)).is_some(),
                "should identify privilege escalation: {cmd}"
            );
        }
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

    /// Every launcher chained after `; ` still refuses, whatever follows the
    /// name: end of line, whitespace, a shell metacharacter or an expansion.
    #[test]
    fn test_semicolon_launchers_still_refuse_at_every_word_boundary() {
        for launcher in PRIV_ESC_CMDS {
            for tail in [
                "", " ", " -", " -i", " id", "\t-", "\t", "\n", "\r\n", ";", "&", "&&", "|", ")",
                "(id)", "<in", ">out", "''", "\"\"", "'", "\"", "`", "$IFS-", "${IFS}-", "\\\n-",
                "{,}", "*", "\u{b}-",
            ] {
                for head in ["true", "ls -la", "cd /tmp", "echo ok && true", "(true"] {
                    let cmd = format!("{head}; {launcher}{tail}");
                    let v = PrivilegeModule
                        .check(&ctx(&cmd))
                        .unwrap_or_else(|| panic!("should refuse: {cmd:?}"));
                    assert!(!v.allow, "{cmd:?}");
                    assert_eq!(v.category, "Privilege Escalation", "{cmd:?}");
                }
            }
        }
    }

    /// The refusal names the launcher it found: `sudo` before `su`.
    #[test]
    fn test_semicolon_refusal_reason_is_unchanged() {
        for (cmd, reason, pattern) in [
            ("true; su -", "Semicolon chained su", "; su"),
            ("true; sudo -i", "Semicolon chained sudo", "; sudo"),
            ("true; doas sh", "Semicolon chained doas", "; doas"),
            ("true; pkexec id", "Semicolon chained pkexec", "; pkexec"),
            (
                "true; runuser -u x id",
                "Semicolon chained runuser",
                "; runuser",
            ),
            ("echo supersedes; su", "Semicolon chained su", "; su"),
            (
                "echo a; supersedes; sudo id",
                "Semicolon chained sudo",
                "; sudo",
            ),
        ] {
            let v = PrivilegeModule.check(&ctx(cmd)).unwrap();
            assert_eq!((v.reason.as_str(), v.pattern.as_str()), (reason, pattern));
        }
    }

    /// A word that only starts with a launcher name is not a launch (issue
    /// 66): prose, a heredoc body or a pattern after a semicolon.
    #[test]
    fn test_semicolon_before_a_word_that_starts_with_a_launcher_name_is_allowed() {
        for cmd in [
            "printf '%s\\n' 'first; such as this' > note.md",
            "cat <<'EOF' > a.md\nA3 (supersedes; summary below)\nEOF",
            "grep -n '; supersedes' file.txt",
            "grep -rn -i 'chained su\\|; summary' docs/",
            "echo \"(...; supersedes A3's ...)\"",
            "echo 'a; subtotal'",
            "echo 'a; doasync b'",
            "echo 'a; sudoku b'",
            "echo 'a; sudoers file'",
            "echo 'a; pkexecute'",
            "echo 'a; runusers'",
            "echo 'a; suit'",
            "echo 'a; su-like'",
            "echo 'a; su.'",
            "echo 'a; su,'",
            "echo 'a; sue'",
            "echo 'a; sudo=1'",
            "echo 'a; su#x'",
            "true; supersedes",
        ] {
            assert!(PrivilegeModule.check(&ctx(cmd)).is_none(), "{cmd:?}");
        }
    }

    #[test]
    fn test_piped_sudo() {
        assert!(PrivilegeModule
            .check(&ctx("echo test | sudo tee file"))
            .is_some());
    }

    #[test]
    fn test_bash_c_reaching_a_launcher() {
        let v = PrivilegeModule.check(&ctx("bash -c 'sudo id'")).unwrap();
        assert_eq!(v.category, "Privilege Escalation");
        assert_eq!(v.reason, "sudo wrapped in bash -c");
        assert!(PrivilegeModule
            .check(&ctx("bash -c \"echo x && /usr/bin/doas true\""))
            .is_some());
        assert!(PrivilegeModule.check(&ctx("bash -lc 'sudo id'")).is_some());
    }

    #[test]
    fn test_sh_c() {
        let v = PrivilegeModule.check(&ctx("sh -c 'su -'")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Privilege Escalation");
        assert_eq!(v.reason, "su wrapped in sh -c");
        assert_eq!(v.pattern, "sh -c");
    }

    #[test]
    fn test_zsh_c() {
        // Regression: the "sh" tail of "zsh -c" must not be mislabeled as
        // "sh -c" — zsh_c_re owns this input and reports the zsh label.
        let v = PrivilegeModule.check(&ctx("zsh -c 'pkexec x'")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.reason, "pkexec wrapped in zsh -c");
        assert_eq!(v.pattern, "zsh -c");
    }

    #[test]
    fn test_eval_reaching_a_launcher() {
        assert!(PrivilegeModule.check(&ctx("eval sudo rm -rf /")).is_some());
    }

    /// D5 blocks escalation, including wrapped forms; a wrapper that reaches
    /// no launcher, `source`, `.` and `LD_LIBRARY_PATH` are ordinary work.
    #[test]
    fn test_ordinary_wrappers_and_library_paths_are_not_escalation() {
        for cmd in [
            "bash -c 'printf ok'",
            "bash -lc 'cargo test --workspace'",
            "sh -c 'echo sudoku'",
            "zsh -c 'echo pseudo'",
            "eval \"$(direnv export bash)\"",
            "eval rm -rf build",
            "source .venv/bin/activate",
            ". ./env.sh",
            "LD_LIBRARY_PATH=/opt/lib cargo test",
            "sudoku --help",
            "grep -rn sudo docs/",
        ] {
            assert!(PrivilegeModule.check(&ctx(cmd)).is_none(), "{cmd}");
        }
    }

    #[test]
    fn test_ld_preload() {
        assert!(PrivilegeModule
            .check(&ctx("LD_PRELOAD=/tmp/evil.so ls"))
            .is_some());
    }

    #[test]
    fn test_path_tmp() {
        assert!(PrivilegeModule.check(&ctx("PATH=/usr/bin:/tmp")).is_some());
    }
}
