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
/// (issue 120).
pub(crate) const GLOBAL_VALUE_OPTIONS: &[&str] = &[
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

/// Settings known not to run a command or pull in other settings. An entry
/// ending in a dot is a whole section. `gpg.format` picks among git's own
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
/// settings (`include.path`), matched by what the name contains. A setting
/// that is neither here nor in [`SAFE_CONFIG`] is [`ConfigKind::Unknown`].
const COMMAND_CONFIG: &[&str] = &[
    "pager",
    "editor",
    "fsmonitor",
    "sshcommand",
    "askpass",
    "hookspath",
    "helper",
    "external",
    "textconv",
    "alias.",
    "driver",
    "program",
    "smudge",
    "clean",
    "process",
    "command",
    "exec",
    "gitproxy",
    "uploadpack",
    "receivepack",
    "include.",
    "includeif.",
    ".cmd",
    "difftool.",
    "mergetool.",
];

/// What the git setting `key` set to `value` can do. A key on
/// [`SAFE_CONFIG`] runs nothing, and so does a `pager.<command>` set to a
/// boolean and a `submodule.<name>.update` set to one of git's own modes; a
/// `submodule.<name>.update` set to `!command` runs it.
pub(crate) fn config_kind(key: &str, value: &str) -> ConfigKind {
    let key = key.to_lowercase();
    let value = value.to_lowercase();
    let boolean = matches!(
        value.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "0" | "1"
    );
    let submodule_update = key.starts_with("submodule.") && key.ends_with(".update");
    if SAFE_CONFIG.iter().any(|k| {
        if k.ends_with('.') {
            key.starts_with(k)
        } else {
            key == *k
        }
    }) || (key.starts_with("pager.") && boolean)
        || (submodule_update && matches!(value.as_str(), "checkout" | "rebase" | "merge" | "none"))
    {
        ConfigKind::Safe
    } else if submodule_update || COMMAND_CONFIG.iter().any(|k| key.contains(k)) {
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
