//! Detects and blocks destructive commands.
//!
//! Checks for: recursive deletion of root/system dirs, disk operations,
//! dangerous permission changes, fork bombs.

use std::sync::OnceLock;

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

/// Substring patterns that always block.
const DANGEROUS_SUBSTRINGS: &[&str] =
    &["dd if=/dev/zero", "dd if=/dev/random", "mkfs.", "> /dev/sd"];

/// System directories whose recursive deletion is catastrophic.
const SYSTEM_DIRS: &[&str] = &[
    "etc", "var", "usr", "bin", "sbin", "boot", "lib", "lib64", "opt", "root", "sys", "proc", "dev",
];

/// Split a command line into simple-command segments so an `rm` after `;`,
/// `&&`, `||`, `|`, `&`, or a newline is still classified on its own.
fn command_segments(cmd: &str) -> Vec<&str> {
    let mut out = vec![cmd];
    for sep in [";", "\n", "&&", "||", "|", "&"] {
        out = out.into_iter().flat_map(|s| s.split(sep)).collect();
    }
    out
}

/// Strip shell quoting and escaping noise from a single token so equivalent
/// spellings collapse: `"/"`→`/`, `'/etc'`→`/etc`, `"$HOME"/`→`$HOME/`,
/// `\/`→`/`. Applied to `rm` *operands* only (not to command-name detection),
/// so ordinary strings like `git commit -m "rm -rf / fix"` — where the token is
/// `"rm`, which does not basename to `rm` — are never misread as a command.
fn unquote_unescape(tok: &str) -> String {
    tok.chars()
        .filter(|&c| c != '"' && c != '\'' && c != '\\')
        .collect()
}

/// Normalize a path operand so filesystem-equivalent spellings reduce to their
/// real target: collapse repeated and `.` slashes and resolve leading `..`, so
/// `//`, `/./`, `//etc`, `/./etc`, `/usr/..`, and `/tmp/..` classify correctly.
fn normalize_path(op: &str) -> String {
    let leading = op.starts_with('/');
    let mut segs: Vec<&str> = Vec::new();
    for s in op.split('/') {
        match s {
            "" | "." => {}
            ".." if leading => {
                segs.pop();
            }
            _ => segs.push(s),
        }
    }
    if leading {
        format!("/{}", segs.join("/"))
    } else {
        segs.join("/")
    }
}

/// A `~` / `$HOME` reference (bare or a path under it).
fn is_home_operand(op: &str) -> bool {
    matches!(op, "~" | "$HOME" | "${HOME}")
        || op.starts_with("~/")
        || op.starts_with("$HOME/")
        || op.starts_with("${HOME}/")
}

/// Classify an `rm` operand as a protected target (root, root glob, a system
/// directory, or the home directory) — regardless of how the path is spelled.
/// De-quotes then normalizes so `rm -rf //`, `/./`, `//etc`, `/usr/..`, `"/"`,
/// and `"$HOME"/` are all caught, not only the literal `/` / `/etc` forms.
///
/// Scope: this guards against *accidental* destructive commands (defense in
/// depth), so it deliberately does not chase command-name obfuscation like
/// `"/bin/rm"`, `r\m`, or `bash -c 'rm -rf /'` — an agent with shell access
/// needs no such evasion, and matching them would force whole-line quote
/// stripping that false-positives on ordinary `echo`/`commit` strings.
fn dangerous_rm_target(op: &str) -> Option<&'static str> {
    let op = unquote_unescape(op);
    let op = op.trim();
    if is_home_operand(op) {
        return Some("home directory");
    }
    let norm = normalize_path(op);
    if norm == "/" || norm == "/*" {
        return Some("/");
    }
    if let Some(rest) = norm.strip_prefix('/') {
        let first = rest.split('/').next().unwrap_or("");
        if SYSTEM_DIRS.contains(&first) {
            return Some("system directory");
        }
    }
    None
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

/// Block recursive `rm` of a protected location, however the flags and operand
/// are spelled. Tokenizes each simple command instead of pattern-matching the
/// raw string, so `rm -r -f /`, `rm --recursive --force /`, `rm -rf -- /`, and
/// `rm -rf "$HOME"` are all caught, not only the exact `rm -rf` form.
fn check_recursive_delete(cmd: &str) -> Option<Verdict> {
    for seg in command_segments(cmd) {
        let toks: Vec<&str> = seg.split_whitespace().collect();
        let Some(rm_idx) = toks
            .iter()
            .position(|t| t.rsplit('/').next().unwrap_or(t) == "rm")
        else {
            continue;
        };

        let mut recursive = false;
        let mut operands: Vec<&str> = Vec::new();
        let mut operands_only = false; // everything after a lone `--`
        for &a in &toks[rm_idx + 1..] {
            if operands_only {
                operands.push(a);
                continue;
            }
            if a == "--" {
                operands_only = true;
            } else if let Some(long) = a.strip_prefix("--") {
                if matches!(long, "recursive" | "dir") {
                    recursive = true;
                }
            } else if let Some(short) = a.strip_prefix('-') {
                if !short.is_empty() && short.chars().all(|c| c.is_ascii_alphabetic()) {
                    if short.contains('r') || short.contains('R') {
                        recursive = true;
                    }
                } else {
                    operands.push(a); // not a clean flag bundle → treat as operand
                }
            } else {
                operands.push(a);
            }
        }

        if !recursive {
            continue;
        }
        for op in operands {
            if let Some(target) = dangerous_rm_target(op) {
                return Some(block(
                    "Dangerous Command",
                    "Recursive deletion of a protected location",
                    target,
                ));
            }
        }
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

    // Regression: flag re-spellings that the old regex classifier missed
    // (codex pre-flip review — exec-guard bypass).
    #[test]
    fn test_rm_flag_respellings_blocked() {
        for cmd in [
            "rm -r -f /",
            "rm -f -r /",
            "rm --recursive --force /",
            "rm --recursive /",
            "rm -rf -- /",
            "rm -r -- /etc",
            "rm -R /usr",
            "rm -rf \"$HOME\"",
            "rm -rf $HOME",
            "rm -rf ${HOME}",
            "rm -rf $HOME/",
            "rm -rf --no-preserve-root /",
            "true && rm -r -f /",
            "sudo rm -rf --force /var",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    // Regression: path-equivalence spellings that reduce to a protected target
    // (codex round-2 review — normalization bypass). `//`, `/./`, and `..` are
    // filesystem-equivalent to root / a system dir and must still be blocked.
    #[test]
    fn test_rm_path_equivalence_blocked() {
        for cmd in [
            "rm -rf //",
            "rm -rf /./",
            "rm -rf ///",
            "rm -rf //etc",
            "rm -rf /./etc",
            "rm -rf /usr/..", // == /
            "rm -rf /tmp/..", // == /
            "rm -rf /var/../etc",
            "rm -rf \"/\"",
            "rm -rf '/'",
            "rm -rf \"$HOME\"/",
            "rm -rf //*",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    // Recursive rm of a non-protected path stays allowed (no false positives).
    #[test]
    fn test_rm_recursive_safe_paths_allowed() {
        for cmd in [
            "rm -rf ./build",
            "rm -r /tmp/scratch",
            "rm -rf target",
            "rm -rf node_modules",
            "rm -f /etc/hosts.bak",      // not recursive
            "rm -rf /home/user/proj/..", // == /home/user, not protected
            "rm -rf ./scratch/..",       // relative, not protected
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "should allow: {cmd}"
            );
        }
    }

    // Over-block guard: when the quote attaches to the `rm` token itself (the
    // common case for a dangerous string inside a message/argument), the raw
    // token is `"rm` / `'rm`, which does not basename to `rm`, so it is never
    // misread as a command and stays allowed.
    #[test]
    fn test_rm_quoted_strings_not_misread_as_command() {
        for cmd in [
            "git commit -m \"rm -rf / fix\"",
            "echo \"rm -rf /\"",
            "grep -r 'rm -rf /' .",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "should allow (not a real rm): {cmd}"
            );
        }
    }

    // Documented conservative behavior: a *bare* `rm` word followed by a
    // recursive flag and a protected target is blocked even inside a harmless
    // `echo '… rm -rf / …'`. A static guard cannot prove command position
    // without a shell parser, and for a safety guard failing safe (over-block a
    // rare echo) beats failing open (miss a real `rm -rf /`).
    #[test]
    fn test_bare_rm_in_string_is_conservatively_blocked() {
        assert!(DangerousModule
            .check(&ctx("echo 'do not run rm -rf /'"))
            .is_some());
    }

    #[test]
    fn test_dd_dev_zero() {
        assert!(DangerousModule
            .check(&ctx("dd if=/dev/zero of=file"))
            .is_some());
    }

    #[test]
    fn test_dd_disk() {
        assert!(DangerousModule
            .check(&ctx("dd if=image of=/dev/sda"))
            .is_some());
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
        assert!(DangerousModule
            .check(&ctx("chown -R root:root /"))
            .is_some());
    }

    #[test]
    fn test_fork_bomb() {
        assert!(DangerousModule.check(&ctx(":(){ :|:& };:")).is_some());
    }

    #[test]
    fn test_fork_bomb_dot_variant() {
        assert!(DangerousModule.check(&ctx(".(){ .|.& };.")).is_some());
    }

    #[test]
    fn test_safe_rm() {
        assert!(DangerousModule.check(&ctx("rm -rf /tmp/test")).is_none());
    }

    #[test]
    fn test_safe_chmod() {
        assert!(DangerousModule
            .check(&ctx("chmod 755 ./script.sh"))
            .is_none());
    }
}
