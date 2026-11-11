//! Git facts shared by the guards and `integrate`: protected-branch matching,
//! git's global options, and what a configuration setting can do.
//!
//! The v1 `GitModule` command scanner that lived here was never wired to a
//! production plane and was removed (TSK-137, ADR-0008 amendment); the live
//! git protections are `hooks/git_guard.rs`.

use std::path::Path;

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

/// Persistent changes to these keys can relocate or hide a checkout from
/// subsequent hook discovery. Command-local `-c` settings are judged separately.
pub(crate) fn relocates_checkout(key: &str) -> bool {
    ["core.worktree", "core.bare", "extensions.worktreeconfig"]
        .iter()
        .any(|name| key.eq_ignore_ascii_case(name))
}

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

/// Whether git reads the lower-case `value` as a boolean, as
/// `git_parse_maybe_bool` does: `true`, `false`, `yes`, `no`, `on`, `off`,
/// the empty value, or an `int` that [`git_int`] reads (`pager.log=2`
/// turns the pager on). Anything else is a string: `core.fsmonitor=08` is
/// a program git runs (TSK-242 review round 13).
fn git_boolean(value: &str) -> bool {
    matches!(value, "" | "true" | "false" | "yes" | "no" | "on" | "off") || git_int(value).is_some()
}

/// Git boolean semantics, including implicit true and numeric values.
pub(crate) fn config_boolean(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "" | "false" | "no" | "off" => Some(false),
        value => git_int(value).map(|number| number != 0),
    }
}

pub(crate) fn is_include_key(key: &[u8]) -> bool {
    key == b"include.path" || (key.starts_with(b"includeif.") && key.ends_with(b".path"))
}

/// Whether an environment-sourced setting can change a guard decision.
/// Harmless presentation and identity settings need no environment lookup.
pub(crate) fn config_env_changes_decision(globals: &[String]) -> bool {
    globals.iter().enumerate().any(|(at, arg)| {
        let setting = if arg == "--config-env" {
            globals.get(at + 1).map(String::as_str)
        } else if let Some(setting) = arg.strip_prefix("--config-env=") {
            Some(setting)
        } else {
            return false;
        };
        let Some((key, _)) = setting.and_then(|setting| setting.split_once('=')) else {
            return true;
        };
        let key = key.to_ascii_lowercase();
        config_kind(&key, "") != ConfigKind::Safe
            || ["alias.", "core.", "push.", "remote.", "branch."]
                .iter()
                .any(|prefix| key.starts_with(prefix))
    })
}

/// The `int` git reads from `value`, as `git_parse_int` does: `strtoimax`
/// with base 0 after leading white space (`0x` is hexadecimal, a leading
/// `0` octal), then nothing or one `k`, `m` or `g` unit, the product in
/// the range of a C `int`. `08` is `0` followed by `8`, so it is no number.
fn git_int(value: &str) -> Option<i64> {
    const MAX: i64 = i32::MAX as i64;
    const MIN: i64 = i32::MIN as i64;
    let text = value.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (negative, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let hex = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_hexdigit()));
    let (radix, digits) = match hex {
        Some(rest) => (16, rest),
        None if unsigned.starts_with('0') => (8, unsigned),
        None => (10, unsigned),
    };
    let count = digits.chars().take_while(|c| c.is_digit(radix)).count();
    if count == 0 {
        return None;
    }
    let factor: i64 = match digits[count..].to_ascii_lowercase().as_str() {
        "" => 1,
        "k" => 1 << 10,
        "m" => 1 << 20,
        "g" => 1 << 30,
        _ => return None,
    };
    // Past `intmax_t`, `strtoimax` reports a range error and git refuses.
    let magnitude = i128::from_str_radix(&digits[..count], radix).ok()?;
    let parsed = i64::try_from(if negative { -magnitude } else { magnitude }).ok()?;
    let fits = if parsed < 0 {
        parsed >= MIN / factor
    } else {
        parsed <= MAX / factor
    };
    fits.then(|| parsed * factor)
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

/// A Git configuration key as a message shows it: the section and the
/// variable lowercased, as Git folds them, and any subsection withheld as
/// `<name>`, so `filter.<name>.smudge`. A subsection is free text from a
/// configuration file or a command and can hold a credential, as in
/// `url.<base>.insteadof` or `credential.<url>.helper`, so it is never
/// shown in any form (TSK-416 review round 6). Git splits a key at its
/// first and last dot, so a subsection with dots is withheld whole. A
/// section or variable outside Git's own name characters is withheld too.
pub(crate) fn shown_config_key(key: &str) -> String {
    match (key.find('.'), key.rfind('.')) {
        (Some(first), Some(last)) if first < last => format!(
            "{}.<name>.{}",
            shown_config_part(&key[..first]),
            shown_config_part(&key[last + 1..])
        ),
        (Some(dot), _) => format!(
            "{}.{}",
            shown_config_part(&key[..dot]),
            shown_config_part(&key[dot + 1..])
        ),
        (None, _) => shown_config_part(key),
    }
}

/// The longest remote name a message prints.
const SHOWN_REMOTE_MAX: usize = 64;

/// Whether a configured remote name is a plain name: 1 to 64 bytes of
/// ASCII letters, digits, `.`, `_` and `-`. A remote name is the subsection
/// of `remote.<name>.url` and can be any text, a URL with a credential
/// included (TSK-416 review round 7), so only a plain name is ever shown.
#[must_use]
pub fn plain_remote_name(name: &str) -> bool {
    (1..=SHOWN_REMOTE_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Whether a message may print the remote `name` of the repository at
/// `root`: a [`plain_remote_name`] of a remote configured now that holds
/// no URL or credential part, of any length, the repository's configured
/// URLs carry. A name no remote is configured under is withheld whatever it
/// looks like (TSK-416 review round 9): its remote's URLs are gone from the
/// inventory, so a credential the name holds can no longer be found. An
/// inventory that cannot be read withholds the name.
#[must_use]
pub fn remote_name_shown(root: &Path, name: &str) -> bool {
    plain_remote_name(name)
        && configured_remote_names(root)
            .is_some_and(|names| names.iter().any(|each| each == name.as_bytes()))
        && crate::git::remote_query::holds_configured_credential(root, name) == Some(false)
}

/// A configured remote as a message names it: the name itself when
/// [`remote_name_shown`] allows it, else `<remote N, name withheld>`, N
/// being the place of its first `[remote]` section among the `[remote]`
/// sections of the repository's configuration file, or `<remote, name
/// withheld>` when no section there defines it. The only step for a
/// withheld remote is a rename with an editor ([`withheld_remote_step`]);
/// no command names it.
#[must_use]
pub fn shown_remote(root: &Path, name: &str) -> String {
    if remote_name_shown(root, name) {
        return name.to_string();
    }
    let place = remote_sections(root, name)
        .and_then(|(_, sections)| sections.first().copied())
        .map_or_else(String::new, |at| format!(" {at}"));
    format!("<remote{place}, name withheld>")
}

/// The names of the configured remotes of the repository at `root`, in
/// byte order without repeats: the subsection of every
/// `remote.<name>.<variable>` setting at any configuration level, so a
/// remote with no URL counts too. `None` when the configuration cannot be
/// read.
fn configured_remote_names(root: &Path) -> Option<Vec<Vec<u8>>> {
    let repo = git2::Repository::discover(root).ok()?;
    let config = repo.config().ok()?;
    let mut entries = config.entries(Some("^remote\\.")).ok()?;
    let mut names = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.ok()?;
        let Some(rest) = entry.name_bytes().strip_prefix(b"remote.") else {
            continue;
        };
        if let Some(last) = rest.iter().rposition(|byte| *byte == b'.') {
            if last > 0 {
                names.push(rest[..last].to_vec());
            }
        }
    }
    names.sort_unstable();
    names.dedup();
    Some(names)
}

/// The repository configuration file of the repository at `root` as a
/// step names it (relative to the working tree when inside it), and the
/// places of the `[remote]` sections in that file that define the remote
/// `name`, each counted among that file's `[remote]` sections in file
/// order, by the parser that reads the remote. Settings an included file
/// holds are not that file's sections and are not counted. A section is
/// counted from its settings, so a section header with none is not
/// counted, and two headers for one remote in a row count once. `None`
/// when the file cannot be read.
fn remote_sections(root: &Path, name: &str) -> Option<(String, Vec<usize>)> {
    let repo = git2::Repository::discover(root).ok()?;
    let path = repo.commondir().join("config");
    let config = git2::Config::open(&path).ok()?;
    let mut entries = config.entries(None).ok()?;
    let mut count = 0;
    let mut current: Option<Vec<u8>> = None;
    let mut places = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.ok()?;
        let subsection = (entry.include_depth() == 0)
            .then(|| entry.name_bytes().strip_prefix(b"remote."))
            .flatten()
            .and_then(|rest| {
                let last = rest.iter().rposition(|byte| *byte == b'.')?;
                (last > 0).then(|| rest[..last].to_vec())
            });
        if subsection.is_some() && subsection != current {
            count += 1;
            if subsection.as_deref() == Some(name.as_bytes()) {
                places.push(count);
            }
        }
        current = subsection;
    }
    let shown = repo
        .workdir()
        .and_then(|workdir| path.strip_prefix(workdir).ok())
        .unwrap_or(&path);
    Some((crate::git::display_path(shown), places))
}

/// `1st`, `2nd`, `3rd`, `4th` and so on.
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The one step for a remote [`shown_remote`] withholds (TSK-416 review
/// round 9): the operator renames it to a plain name with an editor and
/// reruns, which then gives the ordinary steps for that name. No command
/// names a withheld remote, since a name that holds shell syntax runs it
/// when typed or pasted, and listing remotes prints a name that holds a
/// credential. The remote is located by its `[remote]` section in the
/// repository's configuration file, counted by libgit2, which reads it.
#[must_use]
pub fn withheld_remote_step(root: &Path, name: &str) -> String {
    let rename = "renames it to a plain name (ASCII letters, digits, `.`, `_` or `-`) in its header and in its `fetch` lines, and reruns; no command here names that remote, since its name is not safe to print, paste or type";
    let configured = configured_remote_names(root)
        .is_none_or(|names| names.iter().any(|each| each == name.as_bytes()));
    if !configured {
        return "no remote is configured under that name now, so no fetch reaches its refs and no command here names them, since a name kept only in a ref cannot be checked for a credential; the operator works from a fresh clone, or removes those refs from a git client outside the guard".into();
    }
    match remote_sections(root, name) {
        Some((file, places)) if !places.is_empty() => {
            let sections = places
                .iter()
                .map(|at| ordinal(*at))
                .collect::<Vec<_>>()
                .join(" and ");
            let plural = if places.len() > 1 { "s" } else { "" };
            format!(
                "the operator opens {file} in an editor, finds its {sections} [remote] section{plural} (counting only [remote] sections, as of this run), whose name can hold a URL or credential, {rename}"
            )
        }
        _ => format!(
            "the operator opens the configuration file that defines that remote in an editor (it is not a [remote] section of this repository's own configuration file, so it may be a global or included file), finds the [remote] section whose name holds a URL or credential, {rename}"
        ),
    }
}

/// A ref name as a message shows it, with the name of any configured remote
/// of the repository at `root` that it holds under `refs/remotes/` shown as
/// [`shown_remote`] shows it, and the rest escaped
/// ([`crate::git::display_text`]). A remote name can hold `/`, so the
/// longest configured name that prefixes the ref is the one it holds. A
/// ref that matches no configured remote, such as one kept after its
/// remote's configuration was removed, has everything after
/// `refs/remotes/` withheld, whatever it looks like (TSK-416 review round
/// 9), as does a configuration that cannot be read.
#[must_use]
pub fn shown_ref(root: &Path, reference: &str) -> String {
    remote_ref(root, reference, crate::git::display_text)
}

/// [`shown_ref`] for a ref name already in display form, such as a path a
/// scan escaped: only the remote's name is withheld, and nothing else is
/// escaped again.
#[must_use]
pub fn shown_escaped_ref(root: &Path, reference: &str) -> String {
    remote_ref(root, reference, str::to_string)
}

fn remote_ref(root: &Path, reference: &str, escape: fn(&str) -> String) -> String {
    let Some(rest) = reference.strip_prefix("refs/remotes/") else {
        return escape(reference);
    };
    let withheld = || "refs/remotes/<remote, name withheld>/<ref>".to_string();
    let Some(remotes) = configured_remote_names(root) else {
        return withheld();
    };
    // Compared as bytes: the prefix ends at `/` or at the end of `rest`,
    // so it is text whenever it matches.
    let Some(len) = remotes
        .iter()
        .filter(|name| {
            rest.as_bytes()
                .strip_prefix(name.as_slice())
                .is_some_and(|tail| tail.is_empty() || tail.starts_with(b"/"))
        })
        .map(Vec::len)
        .max()
    else {
        return withheld();
    };
    let name = &rest[..len];
    format!(
        "refs/remotes/{}{}",
        shown_remote(root, name),
        escape(&rest[name.len()..])
    )
}

/// [`shown_remote`] for a remote name held as bytes; a name that is not
/// valid UTF-8 is withheld.
#[must_use]
pub fn shown_git_remote(root: &Path, name: &crate::git::GitName) -> String {
    std::str::from_utf8(name.bytes()).map_or_else(
        |_| "<remote, name withheld>".to_string(),
        |name| shown_remote(root, name),
    )
}

/// `text` with the name of every configured remote of the repository at
/// `root` that messages withhold replaced by [`shown_remote`]'s form.
#[must_use]
pub fn without_remote_names(root: &Path, text: &str) -> String {
    WithheldRemotes::read(root).scrub(text)
}

/// The configured remotes of one repository whose names messages withhold,
/// read once for a report that shows many names: each raw name, its
/// escaped form, and the form [`shown_remote`] gives it. When the remotes
/// cannot be read the state is unavailable, and every text is withheld,
/// since any remote name could be in it (TSK-416 review round 8). Text
/// read while the remotes were readable is scrubbed with the names read
/// then, so a configuration that turns unreadable later withholds nothing
/// less.
#[derive(Debug)]
pub struct WithheldRemotes(Option<Vec<(String, String, String)>>);

/// What [`WithheldRemotes::scrub`] gives for any text when the configured
/// remotes could not be read.
pub const UNREAD_REMOTES_TEXT: &str =
    "<withheld, since the configured remote names could not be read>";

impl WithheldRemotes {
    /// Read the remotes of the repository at `root`. A name that is not
    /// valid UTF-8 cannot appear in text as itself, so only its escaped
    /// form is replaced.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let Some(remotes) = configured_remote_names(root) else {
            return Self(None);
        };
        let mut names: Vec<(String, String, String)> = remotes
            .iter()
            .filter_map(|bytes| {
                let escaped = crate::git::GitName::from_bytes(bytes).display().to_string();
                match std::str::from_utf8(bytes) {
                    Ok(name) if remote_name_shown(root, name) => None,
                    Ok(name) => Some((name.to_string(), escaped, shown_remote(root, name))),
                    Err(_) => Some((escaped.clone(), escaped, "<remote, name withheld>".into())),
                }
            })
            .collect();
        // The longest first, so a name that holds another is replaced whole.
        names.sort_by_key(|(name, _, _)| std::cmp::Reverse(name.len()));
        Self(Some(names))
    }

    /// `text` with each withheld name, raw or escaped, replaced; when the
    /// remotes could not be read, [`UNREAD_REMOTES_TEXT`].
    #[must_use]
    pub fn scrub(&self, text: &str) -> String {
        let Some(names) = &self.0 else {
            return UNREAD_REMOTES_TEXT.to_string();
        };
        names
            .iter()
            .fold(text.to_string(), |text, (raw, escaped, shown)| {
                text.replace(raw.as_str(), shown)
                    .replace(escaped.as_str(), shown)
            })
    }

    /// Each of `lines` scrubbed as [`Self::scrub`] does.
    #[must_use]
    pub fn scrub_all(&self, lines: &[String]) -> Vec<String> {
        lines.iter().map(|line| self.scrub(line)).collect()
    }
}

/// What git hands a hook or a guard as the remote, a configured remote's
/// name or a URL typed in its place, as a message shows it: a configured
/// name as [`shown_remote`] shows it, anything else as a URL with its
/// credential parts redacted ([`crate::git::remote_query::display_url`]).
#[must_use]
pub fn shown_remote_or_url(root: &Path, remote: &str) -> String {
    let configured = configured_remote_names(root)
        .is_none_or(|names| names.iter().any(|name| name == remote.as_bytes()));
    if configured {
        shown_remote(root, remote)
    } else {
        crate::git::remote_query::display_url(remote).into_owned()
    }
}

/// `text`, such as a libgit2 error that can quote a ref or a key, with the
/// remote `name`, raw or escaped, replaced by [`shown_remote`]'s form when
/// messages withhold that name.
#[must_use]
pub fn without_remote_name(root: &Path, name: &str, text: &str) -> String {
    if name.is_empty() || remote_name_shown(root, name) {
        return text.to_string();
    }
    let shown = shown_remote(root, name);
    text.replace(name, &shown)
        .replace(&crate::git::display_text(name), &shown)
}

/// A short tracking name, `<remote>/<branch>`, as [`shown_ref`] shows
/// `refs/remotes/<remote>/<branch>`. Any other text is a ref name and is
/// shown by [`shown_ref`] as it is.
#[must_use]
pub fn shown_tracking(root: &Path, short: &str) -> String {
    if short.starts_with("refs/") {
        return shown_ref(root, short);
    }
    let full = shown_ref(root, &format!("refs/remotes/{short}"));
    if let Some(shown) = full.strip_prefix("refs/remotes/") {
        return shown.to_string();
    }
    full
}

/// The step that renames a remote [`shown_remote`] withholds, which pastes
/// no name: the operator gives it a plain name with an editor in the
/// configuration file that defines it, since printing the configuration
/// shows remote names and URLs, which can hold a credential.
pub const WITHHELD_REMOTE_STEP: &str = "the operator gives that remote a plain name (ASCII letters, digits, `.`, `_` or `-`) with an editor in the configuration file that defines it, in its [remote] section header and its `fetch` lines, and reruns, since printing the configuration shows remote names and URLs, which can hold a credential";

/// A configuration section name, as `git config --rename-section` takes
/// it, shown as [`shown_config_key`] shows a key: `filter.<name>` for a
/// section with a subsection.
pub(crate) fn shown_config_section(section: &str) -> String {
    match section.split_once('.') {
        Some((name, _)) => format!("{}.<name>", shown_config_part(name)),
        None => shown_config_part(section),
    }
}

/// A section or variable name lowercased when it holds only Git's name
/// characters (ASCII letters, digits and `-`), else `<invalid>`.
fn shown_config_part(part: &str) -> String {
    if !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        part.to_ascii_lowercase()
    } else {
        "<invalid>".into()
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

    /// git 2.53.0's own reading, from `git -c probe.v=VALUE config
    /// --type=bool probe.v`: a value git rejects as a boolean is a program
    /// to `core.fsmonitor` and `pager.<command>` (TSK-242 review round 13,
    /// where `core.fsmonitor=08` ran a program named `08`).
    #[test]
    fn booleans_are_what_git_parses_as_booleans() {
        let booleans = [
            "true",
            "false",
            "yes",
            "no",
            "on",
            "off",
            "",
            "1",
            "0",
            "+1",
            "-1",
            "1k",
            "1K",
            "2",
            "0x10",
            "0X1F",
            "00",
            "07",
            "010",
            " 1",
            "2147483647",
            "-2147483647",
            "-2147483648",
            "2097151k",
            "-2097152k",
            "2047m",
            "1g",
        ];
        let programs = [
            "08",
            "+08",
            "-08",
            "008",
            "08k",
            "0x",
            "1 ",
            "-",
            "2147483648",
            "2097152k",
            "2048m",
            "2g",
            "9g",
            "-2097153k",
            "0xffffffff",
            "99999999999999999999999",
            "1kb",
            "./hook",
        ];
        for value in booleans {
            assert!(git_boolean(&value.to_lowercase()), "[{value}] is a boolean");
            assert_eq!(
                config_kind("core.fsmonitor", value),
                ConfigKind::Safe,
                "[{value}]"
            );
        }
        for value in programs {
            assert!(
                !git_boolean(&value.to_lowercase()),
                "[{value}] is a program"
            );
            assert_eq!(
                config_kind("core.fsmonitor", value),
                ConfigKind::Command,
                "[{value}]"
            );
            assert_eq!(
                config_kind("pager.log", value),
                ConfigKind::Command,
                "[{value}]"
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

    /// TSK-416 review round 6: a key is shown with its subsection withheld
    /// in every form, its section and variable lowercased. The credentials
    /// are synthetic.
    #[test]
    fn a_configuration_key_is_shown_without_its_subsection() {
        for (key, shown) in [
            (
                "filter.https://user:OPAQUECANARY@host.invalid/x.smudge",
                "filter.<name>.smudge",
            ),
            (
                "url.https://user:OPAQUECANARY@host.invalid/.insteadOf",
                "url.<name>.insteadof",
            ),
            // A quoted subsection with dots is withheld whole.
            (
                "credential.\"a.b.OPAQUECANARY.host.invalid\".Helper",
                "credential.<name>.helper",
            ),
            ("Core.HooksPath", "core.hookspath"),
            ("credential.helper", "credential.helper"),
            ("Diff.Tool.Command", "diff.<name>.command"),
            // A dotless or malformed part is never shown as written.
            ("https://OPAQUECANARY@host", "<invalid>"),
            (
                "https://user:OPAQUECANARY@host.invalid",
                "<invalid>.invalid",
            ),
            ("$KEY", "<invalid>"),
        ] {
            assert_eq!(shown_config_key(key), shown, "{key}");
        }
        assert_eq!(
            shown_config_section("Filter.https://OPAQUECANARY@host.invalid"),
            "filter.<name>"
        );
        assert_eq!(shown_config_section("Alias"), "alias");
        assert_eq!(shown_config_section("$S"), "<invalid>");
    }

    /// TSK-416 review round 7: a remote's name prints only when it is plain
    /// and the credential inventory does not flag it; any other remote is
    /// named by its place among the configured remotes, in a ref name, a
    /// short tracking name and free text alike. The credentials are
    /// synthetic.
    #[test]
    fn a_remote_name_prints_only_when_plain_and_unflagged() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let url_name = "https://user:OPAQUECANARY@host.invalid";
        let at_name = "user@host.invalid";
        // Plain, but the credential of a configured URL.
        let flagged = "OPAQUECANARY";
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap()
            .write_all(
                format!(
                    "[remote \"origin\"]\n\turl = file:///nonexistent/o.git\n\
                     [remote \"{url_name}\"]\n\turl = file:///nonexistent/u.git\n\
                     [remote \"{at_name}\"]\n\turl = file:///nonexistent/a.git\n\
                     [remote \"{flagged}\"]\n\turl = https://u:OPAQUECANARY@x.invalid/f.git\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let root = dir.path();
        assert!(plain_remote_name("origin"));
        assert!(plain_remote_name("fork-2.x_y"));
        for name in [url_name, at_name, "a:b", "a/b", "", &"a".repeat(65)] {
            assert!(!plain_remote_name(name), "{name}");
            assert!(!remote_name_shown(root, name), "{name}");
        }
        assert!(plain_remote_name(flagged));
        assert!(!remote_name_shown(root, flagged));
        assert!(remote_name_shown(root, "origin"));
        // The place among the file's [remote] sections: origin,
        // https://…, user@…, OPAQUECANARY (review round 9).
        assert_eq!(shown_remote(root, "origin"), "origin");
        assert_eq!(shown_remote(root, flagged), "<remote 4, name withheld>");
        assert_eq!(shown_remote(root, url_name), "<remote 2, name withheld>");
        assert_eq!(shown_remote(root, at_name), "<remote 3, name withheld>");
        assert_eq!(shown_remote(root, "gone:x"), "<remote, name withheld>");
        assert_eq!(
            shown_ref(root, &format!("refs/remotes/{at_name}/main")),
            "refs/remotes/<remote 3, name withheld>/main"
        );
        assert_eq!(
            shown_ref(root, "refs/remotes/origin/main"),
            "refs/remotes/origin/main"
        );
        assert_eq!(shown_ref(root, "refs/heads/topic"), "refs/heads/topic");
        // A ref of no configured remote has its remote part withheld,
        // whatever it looks like (review round 9).
        for kept in [
            "refs/remotes/old@host.invalid/main",
            "refs/remotes/old/main",
        ] {
            assert_eq!(
                shown_ref(root, kept),
                "refs/remotes/<remote, name withheld>/<ref>"
            );
        }
        assert!(!remote_name_shown(root, "old"));
        assert_eq!(
            shown_tracking(root, &format!("{at_name}/main")),
            "<remote 3, name withheld>/main"
        );
        let text = format!("fetch {url_name} failed; also on {at_name}/topic; origin/main is fine");
        let scrubbed = without_remote_names(root, &text);
        assert!(!scrubbed.contains("OPAQUECANARY"), "{scrubbed}");
        assert!(!scrubbed.contains("host.invalid"), "{scrubbed}");
        assert!(scrubbed.contains("origin/main"), "{scrubbed}");
        assert_eq!(
            shown_remote_or_url(root, url_name),
            "<remote 2, name withheld>"
        );
        assert!(
            !shown_remote_or_url(root, "https://u:OPAQUECANARY@y.invalid/r")
                .contains("OPAQUECANARY")
        );
        // An inventory that cannot be read withholds every name.
        let outside = tempfile::tempdir().unwrap();
        assert!(!remote_name_shown(outside.path(), "origin"));
        assert_eq!(
            shown_ref(outside.path(), "refs/remotes/origin/main"),
            "refs/remotes/<remote, name withheld>/<ref>"
        );
    }

    /// A repository whose configuration ends with `extra`.
    fn configured(extra: &str) -> tempfile::TempDir {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap()
            .write_all(extra.as_bytes())
            .unwrap();
        dir
    }

    /// TSK-416 review round 8, finding 1: a credential part shorter than
    /// the six characters a diagnostic redacts still withholds a plain
    /// remote name that holds it. The password is synthetic.
    #[test]
    fn a_remote_name_holding_a_short_credential_is_withheld() {
        let dir = configured(
            "[remote \"fork\"]\n\turl = https://user:abc12@host.invalid/x.git\n\
             [remote \"fork-abc12\"]\n\turl = file:///nonexistent/f.git\n\
             [remote \"origin\"]\n\turl = file:///nonexistent/o.git\n",
        );
        let root = dir.path();
        assert!(plain_remote_name("fork-abc12"));
        assert!(!remote_name_shown(root, "fork-abc12"));
        assert!(remote_name_shown(root, "fork"));
        assert!(remote_name_shown(root, "origin"));
        // The second [remote] section of the file: fork, fork-abc12, origin.
        assert_eq!(
            shown_remote(root, "fork-abc12"),
            "<remote 2, name withheld>"
        );
        // Its one step is a rename with an editor, located by section; no
        // command names the remote (review round 9).
        let step = withheld_remote_step(root, "fork-abc12");
        assert!(
            step.starts_with(
                "the operator opens .git/config in an editor, finds its 2nd [remote] section (counting only [remote] sections, as of this run)"
            ),
            "{step}"
        );
        assert!(!step.contains("abc12"), "{step}");
        assert!(!step.contains("`git"), "{step}");
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(12), "12th");
        assert_eq!(ordinal(23), "23rd");
        for text in [
            shown_ref(root, "refs/remotes/fork-abc12/main"),
            shown_tracking(root, "fork-abc12/main"),
            without_remote_names(root, "fetch fork-abc12 failed"),
        ] {
            assert!(!text.contains("abc12"), "{text}");
        }
    }

    /// TSK-416 review round 9, finding 2: a tracking ref kept after its
    /// remote's configuration, which carried the password `abc12`, is
    /// removed matches no configured remote, so its remote part is withheld
    /// whatever it looks like. The password is synthetic.
    #[test]
    fn a_tracking_ref_of_a_removed_remote_is_withheld() {
        let dir =
            configured("[remote \"fork-abc12\"]\n\turl = https://user:abc12@host.invalid/x.git\n");
        let root = dir.path();
        let repo = git2::Repository::open(root).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let commit = repo
            .commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
            .unwrap();
        let kept = "refs/remotes/fork-abc12/main";
        repo.reference(kept, commit, false, "fixture").unwrap();
        // The remote's configuration is removed; its tracking ref stays.
        let config = root.join(".git").join("config");
        let text = std::fs::read_to_string(&config).unwrap();
        let start = text.find("[remote \"fork-abc12\"]").unwrap();
        std::fs::write(&config, &text[..start]).unwrap();
        assert!(repo.find_reference(kept).is_ok());
        let quoted = format!("the reference '{kept}' cannot be peeled");
        for shown in [
            shown_ref(root, kept),
            shown_escaped_ref(root, kept),
            shown_tracking(root, "fork-abc12/main"),
            shown_remote(root, "fork-abc12"),
            without_remote_name(root, "fork-abc12", &quoted),
        ] {
            assert!(!shown.contains("abc12"), "{shown}");
        }
        assert!(!remote_name_shown(root, "fork-abc12"));
    }

    /// TSK-416 review round 8, finding 3: remote names read before the
    /// configuration turns unreadable still withhold a name in text read
    /// with them, and a read that fails withholds every name instead of
    /// replacing none. The credential is synthetic.
    #[test]
    fn withheld_remotes_stay_closed_when_the_configuration_turns_unreadable() {
        let name = "user-OPAQUECANARY@host.invalid";
        let dir = configured(&format!(
            "[remote \"{name}\"]\n\turl = file:///nonexistent/u.git\n"
        ));
        let root = dir.path();
        let acquired = WithheldRemotes::read(root);
        let text = format!("cannot read refs/remotes/{name}/main from {name}");
        std::fs::write(root.join(".git").join("config"), "[remote \"broken\n").unwrap();
        let later = WithheldRemotes::read(root);
        for scrubbed in [
            acquired.scrub(&text),
            later.scrub(&text),
            without_remote_names(root, &text),
            shown_ref(root, &format!("refs/remotes/{name}/main")),
        ] {
            assert!(!scrubbed.contains("OPAQUECANARY"), "{scrubbed}");
            assert!(!scrubbed.contains("host.invalid"), "{scrubbed}");
        }
        assert!(acquired.scrub(&text).contains("<remote 1, name withheld>"));
        assert!(!remote_name_shown(root, "origin"));
    }
}
