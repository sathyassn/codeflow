//! Git facts shared by the guards and `integrate`: protected-branch matching,
//! git's global options, and what a configuration setting can do.
//!
//! The v1 `GitModule` command scanner that lived here was never wired to a
//! production plane and was removed (TSK-137, ADR-0008 amendment); the live
//! git protections are `hooks/git_guard.rs`.

use regex::Regex;

/// git's global options that take their value as the next word, as git 2.53
/// reads them (`git --attr-source HEAD log`). Each also has an `=` spelling,
/// which is one word. `--exec-path` is not here: without `=` it prints git's
/// program directory and takes no value. Every parser that finds the git
/// subcommand skips these, so a value is never read as the subcommand
/// (issue 120), and the offline-read test that classifies git processes.
pub const GLOBAL_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--attr-source",
    "--shallow-file",
];

/// What a git configuration setting can do when git reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigKind {
    /// Runs nothing and pulls in no other settings.
    Safe,
    /// Runs a command, or a program at a path, or pulls in other settings.
    Command,
    /// Not known either way. The guards treat it as one that can run code,
    /// since git has many such settings and adds more (review round ten).
    Unknown,
}

/// Settings known not to run a command or pull in other settings, by exact
/// name ([`config_name_matches`]); an entry ending in a dot is a whole
/// section. `gpg.format` picks among git's own
/// signing programs; the program keys stay command settings. Deliberately
/// absent: `safe.directory` and `protocol.*.allow`, which widen what git
/// trusts, and `diff.tool` and `merge.tool`, which pick the program a tool
/// command runs. Add a key here when an ordinary setting of it refuses.
const SAFE_CONFIG: &[&str] = &[
    "color.",
    "column.",
    "advice.",
    "status.",
    "grep.",
    "user.",
    "diff.renames",
    "diff.algorithm",
    "diff.context",
    "diff.colormoved",
    "diff.mnemonicprefix",
    "merge.ff",
    "merge.conflictstyle",
    "pull.ff",
    "pull.rebase",
    "push.default",
    "push.autosetupremote",
    "push.followtags",
    "fetch.prune",
    "fetch.prunetags",
    "init.defaultbranch",
    "rebase.autostash",
    "rebase.autosquash",
    "rebase.updaterefs",
    "rerere.enabled",
    "rerere.autoupdate",
    "commit.verbose",
    "commit.gpgsign",
    "tag.gpgsign",
    "gpg.format",
    "http.postbuffer",
    "maintenance.auto",
    "feature.manyfiles",
    "index.version",
    "log.date",
    "log.decorate",
    "branch.sort",
    "tag.sort",
    "help.autocorrect",
    "core.commentchar",
    "core.whitespace",
    "core.quotepath",
    "core.autocrlf",
    "core.safecrlf",
    "core.eol",
    "core.filemode",
    "core.ignorecase",
    "core.longpaths",
    "core.precomposeunicode",
    "core.excludesfile",
    "core.untrackedcache",
    "core.abbrev",
    "clean.requireforce",
    "core.fsmonitorhookversion",
];

/// Settings known to run a command, or a path to one, or to pull in other
/// settings (`include.path`), by exact name ([`config_name_matches`]). A
/// setting that is neither here nor in [`SAFE_CONFIG`] or [`BOOLEAN_CONFIG`]
/// is [`ConfigKind::Unknown`], so a program setting this list misses still
/// refuses where an unknown one does. A match on part of a name read the
/// switches beside these settings (`difftool.prompt`) as programs (review
/// round eleven).
const COMMAND_CONFIG: &[&str] = &[
    "alias.",
    "include.",
    "includeif.",
    "pager.",
    "core.pager",
    "core.editor",
    "sequence.editor",
    "core.fsmonitor",
    "core.sshcommand",
    "core.askpass",
    "core.hookspath",
    "core.gitproxy",
    "core.alternaterefscommand",
    "credential.helper",
    "credential.*.helper",
    "diff.external",
    "diff.*.command",
    "diff.*.textconv",
    "difftool.*.path",
    "difftool.*.cmd",
    "mergetool.*.path",
    "mergetool.*.cmd",
    "merge.*.driver",
    "filter.*.clean",
    "filter.*.smudge",
    "filter.*.process",
    "gpg.program",
    "gpg.*.program",
    "gpg.ssh.defaultkeycommand",
    "remote.*.uploadpack",
    "remote.*.receivepack",
    "uploadpack.packobjectshook",
    "hook.*.command",
    "trailer.*.command",
    "trailer.*.cmd",
    "sendemail.sendmailcmd",
    "sendemail.tocmd",
    "sendemail.cccmd",
    "sendemail.headercmd",
    "browser.*.path",
    "browser.*.cmd",
    "man.*.path",
    "man.*.cmd",
    "imap.tunnel",
    "instaweb.httpd",
    "submodule.*.update",
];

/// Settings a boolean value leaves harmless: the switches beside the program
/// settings of the tool sections, `pager.<command>`, which takes a boolean or
/// a program, and `core.fsmonitor`, which takes a boolean (git's own
/// monitor daemon) or a hook program (review round twelve). Set to anything
/// else, a switch is [`ConfigKind::Unknown`] and the others are programs.
const BOOLEAN_CONFIG: &[&str] = &[
    "pager.",
    "core.fsmonitor",
    "difftool.prompt",
    "difftool.trustexitcode",
    "mergetool.prompt",
    "mergetool.keepbackup",
    "mergetool.keeptemporaries",
    "mergetool.writetotemp",
    "mergetool.hideresolved",
    "mergetool.*.trustexitcode",
    "mergetool.*.hideresolved",
    "uploadpack.allowfilter",
    "uploadpack.allowanysha1inwant",
    "uploadpack.allowtipsha1inwant",
    "uploadpack.allowreachablesha1inwant",
    "uploadpack.allowrefinwant",
    "uploadpack.allowsidebandall",
    "rebase.reschedulefailedexec",
];

/// Whether the lower-case setting name `key` matches `pattern`: an entry
/// ending in a dot is a whole section, `section.*.name` is that name under
/// any subsection (`difftool.vimdiff.path`), and anything else is the exact
/// name.
fn config_name_matches(key: &str, pattern: &str) -> bool {
    if pattern.ends_with('.') {
        return key.starts_with(pattern);
    }
    match pattern.split_once(".*.") {
        Some((section, name)) => key
            .strip_prefix(section)
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|rest| rest.strip_suffix(name))
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|subsection| !subsection.is_empty()),
        None => key == pattern,
    }
}

/// Whether git reads the lower-case `value` as a boolean: `true`, `false`,
/// `yes`, `no`, `on`, `off`, the empty value, or an integer with an
/// optional `k`, `m` or `g` unit (`pager.log=2` turns the pager on).
fn git_boolean(value: &str) -> bool {
    if matches!(value, "" | "true" | "false" | "yes" | "no" | "on" | "off") {
        return true;
    }
    let digits = value.strip_prefix(['-', '+']).unwrap_or(value);
    let digits = digits.strip_suffix(['k', 'm', 'g']).unwrap_or(digits);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// What the git setting `key` set to `value` can do. A key on
/// [`SAFE_CONFIG`] runs nothing, and so does a key on [`BOOLEAN_CONFIG`]
/// set to a boolean and a `submodule.<name>.update` set to one of git's own
/// modes; a `submodule.<name>.update` set to `!command` runs it.
pub(crate) fn config_kind(key: &str, value: &str) -> ConfigKind {
    let key = key.to_lowercase();
    let value = value.to_lowercase();
    let named = |list: &[&str]| list.iter().any(|p| config_name_matches(&key, p));
    if named(SAFE_CONFIG)
        || (named(BOOLEAN_CONFIG) && git_boolean(&value))
        || (config_name_matches(&key, "submodule.*.update")
            && matches!(value.as_str(), "checkout" | "rebase" | "merge" | "none"))
    {
        ConfigKind::Safe
    } else if named(COMMAND_CONFIG) {
        ConfigKind::Command
    } else {
        ConfigKind::Unknown
    }
}

/// Whether every setting a section can hold is safe, so renaming another
/// section into it cannot make a setting run code (`user.x` is; `alias`
/// and `core` are not).
pub(crate) fn config_section_is_safe(section: &str) -> bool {
    let first = section.split('.').next().unwrap_or(section).to_lowercase();
    SAFE_CONFIG
        .iter()
        .any(|k| k.strip_suffix('.') == Some(first.as_str()))
}

/// Check if the given branch is in the protected list.
#[must_use]
pub fn is_on_protected_branch(branch: &str, policy: &crate::security::SecurityPolicy) -> bool {
    if branch.is_empty() {
        return false;
    }

    let branches = policy.protected_branch_list();

    for pb in &branches {
        if pb.contains('*') {
            // Wildcard pattern: convert to regex.
            let pattern = format!("^{}$", regex::escape(pb).replace(r"\*", ".*"));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(branch) {
                    return true;
                }
            }
        } else if branch == pb.as_str() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;

    #[test]
    fn config_kind_matches_names_exactly_and_booleans_by_value() {
        // Review round eleven: a match on part of a name made these
        // switches programs.
        for (key, value) in [
            ("difftool.prompt", "false"),
            ("mergetool.keepBackup", "true"),
            ("mergetool.vimdiff.trustExitCode", "yes"),
            ("uploadpack.allowFilter", "true"),
            ("rebase.rescheduleFailedExec", "false"),
            ("pager.log", "2"),
            ("pager.log", "off"),
            ("pager.log", ""),
            ("core.fsmonitor", "true"),
            ("core.fsmonitor", "False"),
            ("submodule.a.b.update", "checkout"),
            ("color.ui", "auto"),
            ("clean.requireForce", "yes"),
        ] {
            assert_eq!(config_kind(key, value), ConfigKind::Safe, "{key}={value}");
        }
        for (key, value) in [
            ("difftool.vimdiff.path", "/x"),
            ("difftool.vimdiff.cmd", "x"),
            ("mergetool.vimdiff.cmd", "x"),
            ("mergetool.vimdiff.path", "/x"),
            ("gpg.ssh.defaultKeyCommand", "x"),
            ("pager.log", "less"),
            ("core.fsmonitor", "./hook"),
            ("core.fsmonitor", "/usr/local/bin/watchman-hook"),
            ("submodule.a.update", "!x"),
            ("credential.https://example.invalid.helper", "store"),
            ("diff.tool.command", "x"),
            ("filter.lfs.clean", "x"),
            ("alias.co", "checkout"),
            ("include.path", "x"),
            ("includeIf.gitdir:~/w/.path", "x"),
            ("hook.pre-commit.command", "x"),
        ] {
            assert_eq!(
                config_kind(key, value),
                ConfigKind::Command,
                "{key}={value}"
            );
        }
        for (key, value) in [
            ("difftool.prompt", "sh x"),
            ("uploadpack.allowFilter", "x"),
            ("rebase.exec", "x"),
            ("difftool.path", "/x"),
            ("credential.helpers", "x"),
            ("safe.directory", "*"),
            ("diff.tool", "vimdiff"),
            ("mystery.key", "1"),
        ] {
            assert_eq!(
                config_kind(key, value),
                ConfigKind::Unknown,
                "{key}={value}"
            );
        }
    }

    #[test]
    fn test_is_on_protected_branch_defaults() {
        let policy = SecurityPolicy::defaults();
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("master", &policy));
        assert!(!is_on_protected_branch("feat/test", &policy));
        assert!(!is_on_protected_branch("", &policy));
    }

    #[test]
    fn test_is_on_protected_branch_glob_patterns() {
        let policy = SecurityPolicy {
            protected_branches: vec!["main".into(), "release/*".into()],
            ..SecurityPolicy::defaults()
        };
        assert!(is_on_protected_branch("main", &policy));
        assert!(is_on_protected_branch("release/v1.0", &policy));
        assert!(!is_on_protected_branch("release", &policy));
        assert!(!is_on_protected_branch("feat/release", &policy));
    }
}
