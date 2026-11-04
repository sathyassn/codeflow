//! Command sources documented by git(1), git-config(1) and each verb's manual.
//! Keep command-valued options, configuration and environment in one inventory.
use super::super::{parse_options, Arity, OptionSpec, GIT_CONFIG_OPTIONS, GIT_REBASE_OPTIONS};

pub(super) enum Source {
    Option(&'static str, char, &'static str),
    ConfigOption(&'static str, char, &'static str),
    EncodedCommand(&'static str),
    Config(&'static str, &'static str),
    ConfigList(&'static str, &'static str),
    Environment(&'static str, &'static str),
    Transport(&'static str),
}

use Source::{
    Config, ConfigList, ConfigOption, EncodedCommand, Environment, Option as CommandOption,
    Transport,
};

pub(super) const REMOTE: &str =
    "clone fetch pull push ls-remote fetch-pack send-pack submodule archive credential";
const DIFF: &str = "diff difftool log show grep blame diff-files diff-index diff-tree format-patch range-diff stash cat-file";
const EDITOR: &str = "commit tag merge rebase notes branch am config cherry-pick revert add reset checkout restore stash replace send-email pull";
const SIGNING: &str =
    "commit tag push log show merge verify-commit verify-tag rebase cherry-pick revert am pull";
// Git 2.53 git.c USE_PAGER plus builtin setup_pager/setup_auto_pager,
// diff pagination and commands delegating to log/show. Narrow conditional
// forms before consulting their configuration and environment sources.
const PAGER: &str = "log show diff difftool grep blame annotate branch tag help reflog shortlog range-diff whatchanged config stash am rebase submodule submodule--helper format-patch add reset checkout restore";

pub(super) fn pages_by_default(verb: &str, rest: &[String]) -> bool {
    let has = |names: &[&str]| rest.iter().any(|word| names.contains(&word.as_str()));
    match verb {
        "config" => {
            let parsed = parse_options(rest, &GIT_CONFIG_OPTIONS);
            parsed.has_short(&['l'])
                || ["--list", "--get-all", "--get-regexp", "--get-urlmatch"]
                    .iter()
                    .any(|name| parsed.has_long(name))
                || parsed
                    .operands
                    .first()
                    .is_some_and(|word| matches!(*word, "list" | "get"))
        }
        "stash" => {
            rest.first()
                .is_some_and(|word| matches!(word.as_str(), "list" | "show"))
                || super::uses_editor(verb, rest)
        }
        "am" => {
            rest.iter().any(|word| {
                let name = word.split('=').next().unwrap_or(word);
                name.starts_with("--") && name.len() > 2 && "--show-current-patch".starts_with(name)
            }) || has(&["-i", "--interactive"])
        }
        "rebase" => parse_options(rest, &GIT_REBASE_OPTIONS).has_long("--show-current-patch"),
        "submodule" | "submodule--helper" => has(&["summary"]),
        "format-patch" => has(&["--stdout"]),
        "add" | "reset" | "checkout" | "restore" => super::uses_editor(verb, rest),
        _ => active(PAGER, verb),
    }
}

pub(super) fn active(verbs: &str, verb: &str) -> bool {
    verbs
        .split_ascii_whitespace()
        .any(|name| name == verb || name == "*")
}

pub(super) const SOURCES: &[Source] = &[
    ConfigOption("clone", 'c', "--config"),
    EncodedCommand("remote-ext"),
    CommandOption("rebase", 'x', "--exec"),
    CommandOption("difftool", 'x', "--extcmd"),
    CommandOption("grep", 'O', "--open-files-in-pager"),
    CommandOption("filter-branch", '\0', "--setup"),
    CommandOption("filter-branch", '\0', "--env-filter"),
    CommandOption("filter-branch", '\0', "--tree-filter"),
    CommandOption("filter-branch", '\0', "--index-filter"),
    CommandOption("filter-branch", '\0', "--parent-filter"),
    CommandOption("filter-branch", '\0', "--msg-filter"),
    CommandOption("filter-branch", '\0', "--commit-filter"),
    CommandOption("filter-branch", '\0', "--tag-name-filter"),
    CommandOption("clone", 'u', "--upload-pack"),
    CommandOption("fetch", '\0', "--upload-pack"),
    CommandOption("pull", '\0', "--upload-pack"),
    CommandOption("ls-remote", '\0', "--upload-pack"),
    CommandOption("fetch-pack", '\0', "--upload-pack"),
    CommandOption("fetch-pack", '\0', "--exec"),
    CommandOption("push", '\0', "--receive-pack"),
    CommandOption("push", '\0', "--exec"),
    CommandOption("send-pack", '\0', "--receive-pack"),
    CommandOption("send-pack", '\0', "--exec"),
    CommandOption("archive", '\0', "--exec"),
    CommandOption("daemon", '\0', "--access-hook"),
    CommandOption("instaweb", 'd', "--httpd"),
    CommandOption("send-email", '\0', "--to-cmd"),
    CommandOption("send-email", '\0', "--cc-cmd"),
    CommandOption("send-email", '\0', "--header-cmd"),
    CommandOption("send-email", '\0', "--sendmail-cmd"),
    Transport("remote.*.url"),
    Transport("remote.*.pushurl"),
    Transport("submodule.*.url"),
    Transport("url.*.insteadof"),
    Transport("url.*.pushinsteadof"),
    Config("core.pager", PAGER),
    Config("pager.*", "*"),
    Config("core.editor", EDITOR),
    Config("sequence.editor", "rebase pull"),
    Config("core.fsmonitor", "*"),
    Config("core.sshcommand", REMOTE),
    Config("core.askpass", REMOTE),
    Config("core.gitproxy", REMOTE),
    Config("core.alternaterefscommand", "*"),
    ConfigList("credential.helper", REMOTE),
    ConfigList("credential.*.helper", REMOTE),
    Config("diff.external", DIFF),
    Config("diff.*.command", DIFF),
    Config("diff.*.textconv", DIFF),
    Config("difftool.*.path", "difftool"),
    Config("difftool.*.cmd", "difftool"),
    Config("mergetool.*.path", "mergetool"),
    Config("mergetool.*.cmd", "mergetool"),
    Config("merge.*.driver", "*"),
    Config("filter.*.clean", "*"),
    Config("filter.*.smudge", "*"),
    Config("filter.*.process", "*"),
    Config("gpg.program", SIGNING),
    Config("gpg.*.program", SIGNING),
    Config("gpg.ssh.defaultkeycommand", SIGNING),
    Config("remote.*.uploadpack", REMOTE),
    Config("remote.*.receivepack", REMOTE),
    Config("uploadpack.packobjectshook", "upload-pack"),
    Config("hook.*.command", "*"),
    Config(
        "trailer.*.command",
        "interpret-trailers commit tag format-patch",
    ),
    Config(
        "trailer.*.cmd",
        "interpret-trailers commit tag format-patch",
    ),
    Config("sendemail.sendmailcmd", "send-email"),
    Config("sendemail.tocmd", "send-email"),
    Config("sendemail.cccmd", "send-email"),
    Config("sendemail.headercmd", "send-email"),
    Config("sendemail.smtpserver", "send-email"),
    Config("sendemail.*.sendmailcmd", "send-email"),
    Config("sendemail.*.tocmd", "send-email"),
    Config("sendemail.*.cccmd", "send-email"),
    Config("sendemail.*.headercmd", "send-email"),
    Config("sendemail.*.smtpserver", "send-email"),
    Config("browser.*.path", "help web--browse instaweb"),
    Config("browser.*.cmd", "help web--browse instaweb"),
    Config("man.*.path", "help"),
    Config("man.*.cmd", "help"),
    Config("imap.tunnel", "imap-send"),
    Config("instaweb.httpd", "instaweb"),
    Config("submodule.*.update", "submodule"),
    Config("tar.*.command", "archive"),
    Config("interactive.difffilter", "add reset checkout restore stash"),
    Environment("GIT_SSH", REMOTE),
    Environment("GIT_SSH_COMMAND", REMOTE),
    Environment("GIT_PROXY_COMMAND", REMOTE),
    Environment("GIT_EXTERNAL_DIFF", DIFF),
    Environment("GIT_DIFFTOOL_EXTCMD", "difftool"),
    Environment("GIT_PAGER", PAGER),
    Environment("PAGER", PAGER),
    Environment("GIT_EDITOR", EDITOR),
    Environment("GIT_SEQUENCE_EDITOR", "rebase pull"),
    Environment("EDITOR", EDITOR),
    Environment("VISUAL", EDITOR),
    Environment("GIT_ASKPASS", REMOTE),
    Environment("SSH_ASKPASS", REMOTE),
];

pub(super) fn config_matches(key: &str, pattern: &str) -> bool {
    let key = key.to_ascii_lowercase();
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            key.starts_with(prefix)
                && key.ends_with(suffix)
                && key.len() > prefix.len() + suffix.len()
        }
        None => key == pattern,
    }
}

/// The environment variables whose value is a program Git runs.
pub(super) fn environment_names() -> impl Iterator<Item = &'static str> {
    SOURCES.iter().filter_map(|source| match source {
        Environment(name, _) => Some(*name),
        _ => None,
    })
}

pub(super) fn environment(name: &str) -> bool {
    SOURCES
        .iter()
        .any(|source| matches!(source, Environment(known, _) if *known == name))
}

pub(super) const FILTER_OPTIONS: OptionSpec = OptionSpec {
    short: &[('f', Arity::Flag), ('d', Arity::Value)],
    long: &[
        ("--force", Arity::Flag),
        ("--setup", Arity::Value),
        ("--env-filter", Arity::Value),
        ("--tree-filter", Arity::Value),
        ("--index-filter", Arity::Value),
        ("--parent-filter", Arity::Value),
        ("--msg-filter", Arity::Value),
        ("--commit-filter", Arity::Value),
        ("--tag-name-filter", Arity::Value),
        ("--subdirectory-filter", Arity::Value),
        ("--original", Arity::Value),
        ("--state-branch", Arity::Value),
        ("--prune-empty", Arity::Flag),
        ("--remap-to-ancestor", Arity::Flag),
    ],
    git_style: false,
};

pub(super) const DIFFTOOL_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('x', Arity::Value),
        ('t', Arity::Value),
        ('g', Arity::Flag),
        ('y', Arity::Flag),
        ('d', Arity::Flag),
        ('h', Arity::Flag),
    ],
    long: &[
        ("--extcmd", Arity::Value),
        ("--tool", Arity::Value),
        ("--gui", Arity::Flag),
        ("--no-gui", Arity::Flag),
        ("--dir-diff", Arity::Flag),
        ("--no-prompt", Arity::Flag),
        ("--prompt", Arity::Flag),
        ("--trust-exit-code", Arity::Flag),
        ("--no-trust-exit-code", Arity::Flag),
        ("--symlinks", Arity::Flag),
        ("--no-symlinks", Arity::Flag),
        ("--tool-help", Arity::Flag),
        ("--cached", Arity::Flag),
        ("--staged", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const MERGETOOL_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('t', Arity::Value),
        ('g', Arity::Flag),
        ('y', Arity::Flag),
        ('h', Arity::Flag),
    ],
    long: &[
        ("--tool", Arity::Value),
        ("--tool-help", Arity::Flag),
        ("--gui", Arity::Flag),
        ("--no-gui", Arity::Flag),
        ("--prompt", Arity::Flag),
        ("--no-prompt", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const GREP_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('O', Arity::AttachedValue),
        ('e', Arity::Value),
        ('f', Arity::Value),
        ('A', Arity::Value),
        ('B', Arity::Value),
        ('C', Arity::Value),
        ('m', Arity::Value),
        ('n', Arity::Flag),
        ('i', Arity::Flag),
        ('I', Arity::Flag),
        ('w', Arity::Flag),
        ('v', Arity::Flag),
        ('h', Arity::Flag),
        ('H', Arity::Flag),
        ('l', Arity::Flag),
        ('L', Arity::Flag),
        ('z', Arity::Flag),
        ('c', Arity::Flag),
        ('q', Arity::Flag),
        ('E', Arity::Flag),
        ('G', Arity::Flag),
        ('P', Arity::Flag),
        ('F', Arity::Flag),
        ('a', Arity::Flag),
        ('o', Arity::Flag),
    ],
    long: &[
        ("--open-files-in-pager", Arity::AttachedValue),
        ("--cached", Arity::Flag),
        ("--no-index", Arity::Flag),
        ("--untracked", Arity::Flag),
        ("--textconv", Arity::Flag),
        ("--no-textconv", Arity::Flag),
        ("--recurse-submodules", Arity::Flag),
        ("--line-number", Arity::Flag),
        ("--ignore-case", Arity::Flag),
        ("--name-only", Arity::Flag),
        ("--files-with-matches", Arity::Flag),
        ("--count", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--all-match", Arity::Flag),
        ("--and", Arity::Flag),
        ("--or", Arity::Flag),
        ("--not", Arity::Flag),
        ("--threads", Arity::Value),
        ("--max-depth", Arity::Value),
        ("--color", Arity::AttachedValue),
        ("--break", Arity::Flag),
        ("--heading", Arity::Flag),
    ],
    git_style: true,
};

// Shared transport flags. A union is conservative for ambiguous prefixes:
// a spelling that cannot be resolved here refuses instead of hiding a value.
pub(super) const TRANSPORT_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('q', Arity::Flag),
        ('v', Arity::Flag),
        ('f', Arity::Flag),
        ('n', Arity::Flag),
        ('p', Arity::Flag),
        ('t', Arity::Flag),
        ('u', Arity::Flag),
        ('4', Arity::Flag),
        ('6', Arity::Flag),
        ('j', Arity::Value),
        ('o', Arity::Value),
        ('s', Arity::Value),
        ('X', Arity::Value),
    ],
    long: &[
        ("--upload-pack", Arity::Value),
        ("--receive-pack", Arity::Value),
        ("--exec", Arity::Value),
        ("--quiet", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--all", Arity::Flag),
        ("--no-all", Arity::Flag),
        ("--atomic", Arity::Flag),
        ("--tags", Arity::Flag),
        ("--no-tags", Arity::Flag),
        ("--heads", Arity::Flag),
        ("--branches", Arity::Flag),
        ("--refs", Arity::Flag),
        ("--symref", Arity::Flag),
        ("--exit-code", Arity::Flag),
        ("--get-url", Arity::Flag),
        ("--server-option", Arity::Value),
        ("--depth", Arity::Value),
        ("--deepen", Arity::Value),
        ("--shallow-since", Arity::Value),
        ("--shallow-exclude", Arity::Value),
        ("--filter", Arity::Value),
        ("--unshallow", Arity::Flag),
        ("--update-shallow", Arity::Flag),
        ("--force", Arity::Flag),
        ("--dry-run", Arity::Flag),
        ("--prune", Arity::Flag),
        ("--no-prune", Arity::Flag),
        ("--prune-tags", Arity::Flag),
        ("--no-prune-tags", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--no-recurse-submodules", Arity::Flag),
        ("--rebase", Arity::AttachedValue),
        ("--strategy", Arity::Value),
        ("--strategy-option", Arity::Value),
        ("--no-rebase", Arity::Flag),
        ("--ff-only", Arity::Flag),
        ("--ff", Arity::Flag),
        ("--no-ff", Arity::Flag),
        ("--autostash", Arity::Flag),
        ("--no-autostash", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--multiple", Arity::Flag),
        ("--append", Arity::Flag),
        ("--keep", Arity::Flag),
        ("--update-head-ok", Arity::Flag),
        ("--no-write-fetch-head", Arity::Flag),
        ("--write-fetch-head", Arity::Flag),
        ("--negotiation-only", Arity::Flag),
        ("--negotiation-tip", Arity::Value),
        ("--refmap", Arity::Value),
        ("--jobs", Arity::Value),
        ("--set-upstream", Arity::Flag),
        ("--ipv4", Arity::Flag),
        ("--ipv6", Arity::Flag),
        ("--thin", Arity::Flag),
        ("--no-thin", Arity::Flag),
        ("--stdin", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const CLONE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('u', Arity::Value),
        ('b', Arity::Value),
        ('o', Arity::Value),
        ('c', Arity::Value),
        ('j', Arity::Value),
        ('q', Arity::Flag),
        ('v', Arity::Flag),
        ('n', Arity::Flag),
        ('l', Arity::Flag),
        ('s', Arity::Flag),
    ],
    long: &[
        ("--upload-pack", Arity::Value),
        ("--branch", Arity::Value),
        ("--origin", Arity::Value),
        ("--config", Arity::Value),
        ("--template", Arity::Value),
        ("--depth", Arity::Value),
        ("--filter", Arity::Value),
        ("--reference", Arity::Value),
        ("--reference-if-able", Arity::Value),
        ("--separate-git-dir", Arity::Value),
        ("--server-option", Arity::Value),
        ("--shallow-since", Arity::Value),
        ("--shallow-exclude", Arity::Value),
        ("--jobs", Arity::Value),
        ("--revision", Arity::Value),
        ("--bundle-uri", Arity::Value),
        ("--quiet", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--bare", Arity::Flag),
        ("--mirror", Arity::Flag),
        ("--local", Arity::Flag),
        ("--no-local", Arity::Flag),
        ("--shared", Arity::Flag),
        ("--no-hardlinks", Arity::Flag),
        ("--no-checkout", Arity::Flag),
        ("--single-branch", Arity::Flag),
        ("--no-single-branch", Arity::Flag),
        ("--no-tags", Arity::Flag),
        ("--dissociate", Arity::Flag),
        ("--sparse", Arity::Flag),
        ("--recurse-submodules", Arity::AttachedValue),
        ("--recursive", Arity::AttachedValue),
        ("--shallow-submodules", Arity::Flag),
        ("--remote-submodules", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const ARCHIVE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('o', Arity::Value),
        ('v', Arity::Flag),
        ('l', Arity::Flag),
        ('0', Arity::Flag),
        ('1', Arity::Flag),
        ('2', Arity::Flag),
        ('3', Arity::Flag),
        ('4', Arity::Flag),
        ('5', Arity::Flag),
        ('6', Arity::Flag),
        ('7', Arity::Flag),
        ('8', Arity::Flag),
        ('9', Arity::Flag),
    ],
    long: &[
        ("--exec", Arity::Value),
        ("--remote", Arity::Value),
        ("--format", Arity::Value),
        ("--prefix", Arity::Value),
        ("--output", Arity::Value),
        ("--add-file", Arity::Value),
        ("--add-virtual-file", Arity::Value),
        ("--mtime", Arity::Value),
        ("--worktree-attributes", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--list", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const DAEMON_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[
        ("--access-hook", Arity::Value),
        ("--base-path", Arity::Value),
        ("--interpolated-path", Arity::Value),
        ("--listen", Arity::Value),
        ("--port", Arity::Value),
        ("--user", Arity::Value),
        ("--group", Arity::Value),
        ("--pid-file", Arity::Value),
        ("--timeout", Arity::Value),
        ("--init-timeout", Arity::Value),
        ("--max-connections", Arity::Value),
        ("--enable", Arity::Value),
        ("--disable", Arity::Value),
        ("--allow-override", Arity::Value),
        ("--forbid-override", Arity::Value),
        ("--export-all", Arity::Flag),
        ("--strict-paths", Arity::Flag),
        ("--inetd", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--verbose", Arity::Flag),
        ("--reuseaddr", Arity::Flag),
        ("--base-path-relaxed", Arity::Flag),
        ("--syslog", Arity::Flag),
    ],
    git_style: false,
};

pub(super) const EMAIL_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[
        ("--to-cmd", Arity::Value),
        ("--cc-cmd", Arity::Value),
        ("--header-cmd", Arity::Value),
        ("--sendmail-cmd", Arity::Value),
        ("--to", Arity::Value),
        ("--cc", Arity::Value),
        ("--bcc", Arity::Value),
        ("--from", Arity::Value),
        ("--subject", Arity::Value),
        ("--identity", Arity::Value),
        ("--confirm", Arity::Value),
        ("--dry-run", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--validate", Arity::Flag),
        ("--no-validate", Arity::Flag),
        ("--compose", Arity::Flag),
        ("--annotate", Arity::Flag),
    ],
    git_style: true,
};

pub(super) const INSTAWEB_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('d', Arity::Value),
        ('m', Arity::Value),
        ('p', Arity::Value),
        ('b', Arity::Value),
        ('l', Arity::Flag),
        ('h', Arity::Flag),
    ],
    long: &[
        ("--httpd", Arity::Value),
        ("--module-path", Arity::Value),
        ("--port", Arity::Value),
        ("--browser", Arity::Value),
        ("--local", Arity::Flag),
        ("--start", Arity::Flag),
        ("--stop", Arity::Flag),
        ("--restart", Arity::Flag),
        ("--help", Arity::Flag),
    ],
    git_style: false,
};

// Modes that can open an editor while editing a patch or replacement object.
// Unlisted options keep editor activation unknown and therefore active.
pub(super) const EDITOR_MODE_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('p', Arity::Flag),
        ('e', Arity::Flag),
        ('i', Arity::Flag),
        ('q', Arity::Flag),
        ('f', Arity::Flag),
        ('l', Arity::Flag),
        ('N', Arity::Flag),
        ('A', Arity::Flag),
        ('u', Arity::Flag),
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('s', Arity::Value),
        ('m', Arity::Value),
    ],
    long: &[
        ("--patch", Arity::Flag),
        ("--edit", Arity::Flag),
        ("--interactive", Arity::Flag),
        ("--hard", Arity::Flag),
        ("--soft", Arity::Flag),
        ("--mixed", Arity::Flag),
        ("--merge", Arity::Flag),
        ("--keep", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--force", Arity::Flag),
        ("--list", Arity::Flag),
        ("--all", Arity::Flag),
        ("--update", Arity::Flag),
        ("--intent-to-add", Arity::Flag),
        ("--pathspec-from-file", Arity::Value),
        ("--source", Arity::Value),
        ("--message", Arity::Value),
        ("--format", Arity::Value),
    ],
    git_style: true,
};

// Checkout and restore use -m as a flag, while stash uses it for a message.
pub(super) const CHECKOUT_EDITOR_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('p', Arity::Flag),
        ('e', Arity::Flag),
        ('i', Arity::Flag),
        ('q', Arity::Flag),
        ('f', Arity::Flag),
        ('l', Arity::Flag),
        ('N', Arity::Flag),
        ('A', Arity::Flag),
        ('u', Arity::Flag),
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('s', Arity::Value),
        ('m', Arity::Flag),
    ],
    long: EDITOR_MODE_OPTIONS.long,
    git_style: true,
};

// Output options and the value-taking options before them, from each verb's
// Git 2.53 manual (TSK-379 review round 9). The output reader parses with
// these tables so another option's value is never read as the output.

/// The diff option `--output`, accepted by every verb that takes diff
/// options. Git's diff parser takes only the full name.
pub(super) const DIFF_OUTPUT_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[("--output", Arity::Value)],
    git_style: false,
};

/// `git format-patch` (git-format-patch(1)); its own options take no
/// abbreviation. Short letters with values: `-o`, `-v`; the rest are flags.
pub(super) const FORMAT_PATCH_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('o', Arity::Value),
        ('v', Arity::Value),
        ('n', Arity::Flag),
        ('N', Arity::Flag),
        ('s', Arity::Flag),
        ('k', Arity::Flag),
        ('p', Arity::Flag),
        ('q', Arity::Flag),
        ('1', Arity::Flag),
        ('2', Arity::Flag),
        ('3', Arity::Flag),
        ('4', Arity::Flag),
        ('5', Arity::Flag),
        ('6', Arity::Flag),
        ('7', Arity::Flag),
        ('8', Arity::Flag),
        ('9', Arity::Flag),
        ('0', Arity::Flag),
    ],
    long: &[
        ("--output-directory", Arity::Value),
        ("--output", Arity::Value),
        ("--reroll-count", Arity::Value),
        ("--start-number", Arity::Value),
        ("--subject-prefix", Arity::Value),
        ("--signature", Arity::Value),
        ("--signature-file", Arity::Value),
        ("--suffix", Arity::Value),
        ("--to", Arity::Value),
        ("--cc", Arity::Value),
        ("--in-reply-to", Arity::Value),
        ("--add-header", Arity::Value),
        ("--base", Arity::Value),
        ("--interdiff", Arity::Value),
        ("--range-diff", Arity::Value),
        ("--creation-factor", Arity::Value),
        ("--description-file", Arity::Value),
        ("--filename-max-length", Arity::Value),
        ("--cover-from-description", Arity::Value),
        ("--stdout", Arity::Flag),
    ],
    git_style: false,
};

/// `git bundle create`, read to its first operand, the bundle file.
pub(super) const BUNDLE_CREATE_OPTIONS: OptionSpec = OptionSpec {
    short: &[('q', Arity::Flag)],
    long: &[
        ("--quiet", Arity::Flag),
        ("--progress", Arity::Flag),
        ("--no-progress", Arity::Flag),
        ("--all-progress", Arity::Flag),
        ("--all-progress-implied", Arity::Flag),
        ("--version", Arity::Value),
    ],
    git_style: true,
};

/// `git merge-file`, whose first operand is the file it rewrites.
pub(super) const MERGE_FILE_OPTIONS: OptionSpec = OptionSpec {
    short: &[('p', Arity::Flag), ('q', Arity::Flag), ('L', Arity::Value)],
    long: &[
        ("--stdout", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--object-id", Arity::Flag),
        ("--diff3", Arity::Flag),
        ("--zdiff3", Arity::Flag),
        ("--ours", Arity::Flag),
        ("--theirs", Arity::Flag),
        ("--union", Arity::Flag),
        ("--marker-size", Arity::Value),
        ("--diff-algorithm", Arity::Value),
    ],
    git_style: true,
};

/// `git checkout-index`; `--prefix` places every file it writes.
pub(super) const CHECKOUT_INDEX_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('u', Arity::Flag),
        ('q', Arity::Flag),
        ('a', Arity::Flag),
        ('f', Arity::Flag),
        ('n', Arity::Flag),
        ('z', Arity::Flag),
    ],
    long: &[
        ("--index", Arity::Flag),
        ("--quiet", Arity::Flag),
        ("--all", Arity::Flag),
        ("--force", Arity::Flag),
        ("--no-create", Arity::Flag),
        ("--stdin", Arity::Flag),
        ("--temp", Arity::Flag),
        ("--ignore-skip-worktree-bits", Arity::Flag),
        ("--prefix", Arity::Value),
        ("--stage", Arity::Value),
    ],
    git_style: true,
};

/// `git bugreport` and `git diagnose`.
pub(super) const REPORT_OPTIONS: OptionSpec = OptionSpec {
    short: &[('o', Arity::Value), ('s', Arity::Value)],
    long: &[
        ("--output-directory", Arity::Value),
        ("--suffix", Arity::Value),
        ("--no-suffix", Arity::Flag),
        ("--mode", Arity::Value),
        ("--diagnose", Arity::AttachedValue),
    ],
    git_style: true,
};

/// `git index-pack`: `-o FILE` names the index it writes.
pub(super) const INDEX_PACK_OPTIONS: OptionSpec = OptionSpec {
    short: &[('o', Arity::Value), ('v', Arity::Flag)],
    long: &[],
    git_style: false,
};

/// `git mailsplit`: `-o<directory>` takes its value attached.
pub(super) const MAILSPLIT_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('o', Arity::AttachedValue),
        ('d', Arity::AttachedValue),
        ('f', Arity::AttachedValue),
        ('b', Arity::Flag),
    ],
    long: &[("--keep-cr", Arity::Flag), ("--mboxrd", Arity::Flag)],
    git_style: false,
};

/// `git fast-export` and `git fast-import` marks files.
pub(super) const MARKS_OPTIONS: OptionSpec = OptionSpec {
    short: &[],
    long: &[
        ("--export-marks", Arity::Value),
        ("--import-marks", Arity::Value),
        ("--import-marks-if-exists", Arity::Value),
    ],
    git_style: true,
};

/// `git worktree add`, read to its first operand, the new checkout.
pub(super) const WORKTREE_ADD_OPTIONS: OptionSpec = OptionSpec {
    short: &[
        ('b', Arity::Value),
        ('B', Arity::Value),
        ('f', Arity::Flag),
        ('q', Arity::Flag),
    ],
    long: &[
        ("--force", Arity::Flag),
        ("--detach", Arity::Flag),
        ("--checkout", Arity::Flag),
        ("--no-checkout", Arity::Flag),
        ("--lock", Arity::Flag),
        ("--reason", Arity::Value),
        ("--orphan", Arity::Flag),
        ("--track", Arity::Flag),
        ("--no-track", Arity::Flag),
        ("--guess-remote", Arity::Flag),
        ("--no-guess-remote", Arity::Flag),
        ("--relative-paths", Arity::Flag),
        ("--no-relative-paths", Arity::Flag),
        ("--quiet", Arity::Flag),
    ],
    git_style: true,
};
