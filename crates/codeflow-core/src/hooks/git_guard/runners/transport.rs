//! The transport Git selects for each remote a command contacts (TSK-1489).
//!
//! To reach a remote, Git may start a helper program, `git-remote-<name>`.
//! Both guards read this one resolution through the runner reader, so a
//! helper program is either admitted by the landed policy and judged as a
//! program word, or refused.
//!
//! The resolution admits only the forms it can prove Git resolves the same
//! way, and refuses every other form by name, so a gap between this reading
//! and Git's refuses instead of passing:
//!
//! - an address the command names: the transport its `<name>::` prefix or
//!   its `<scheme>://` selects after the address rewrites, as Git's
//!   `transport_get` reads it;
//! - a remote the command names that the configuration the call reads
//!   defines with exactly one address, at most one push address, no helper
//!   setting, and no remote group of the same name;
//! - for an operand-free `fetch` or `pull`, the remote the current branch
//!   names, or `origin` when it names none, under the same conditions and
//!   with fetching every remote turned off;
//! - the promisor remotes of a partial clone, under the same conditions,
//!   for every Git command, since a missing object can be fetched from them
//!   by any command.
//!
//! Refused by name: an operand-free push or `ls-remote`, every form that
//! contacts every configured remote, a remote group, a remote with a helper
//! setting, more than one address value or an empty one, a remote name a
//! legacy remote file may define, a `remote add` that fetches unless the
//! new remote's configuration is proven empty, a maintenance run unless
//! every task is a proven local one, the submodule builtins that update,
//! add or clone, and submodule recursion in a repository that has or may
//! have submodules unless the command line provably turns it off.
//!
//! Options are read as Git's option parser reads them: a long option may be
//! any unambiguous abbreviation, so every recursion or task option form is
//! matched by prefix, and a form the reading cannot place refuses.

use super::super::shown_word;
use crate::security::git_config::{EffectiveConfig, Location};
use std::path::Path;

/// The verbs that contact a remote, and how.
const FETCHING: &[&str] = &["fetch", "pull", "ls-remote", "fetch-pack"];
const PUSHING: &[&str] = &["push", "send-pack"];
/// Verbs whose remote defaults to a configured one when none is named.
const DEFAULTING: &[&str] = &["fetch", "pull", "ls-remote", "push"];

/// Options of the defaulting verbs that never take the next word as their
/// value. A word after any other option may be that option's value, so it
/// does not prove that the command names its remote.
const FLAGS: &[&str] = &[
    "-4",
    "-6",
    "-a",
    "-d",
    "-f",
    "-k",
    "-n",
    "-p",
    "-P",
    "-q",
    "-r",
    "-t",
    "-u",
    "-v",
    "--append",
    "--atomic",
    "--autostash",
    "--branches",
    "--commit",
    "--delete",
    "--dry-run",
    "--edit",
    "--exit-code",
    "--ff",
    "--ff-only",
    "--follow-tags",
    "--force",
    "--force-if-includes",
    "--force-with-lease",
    "--get-url",
    "--heads",
    "--ipv4",
    "--ipv6",
    "--keep",
    "--mirror",
    "--multiple",
    "--no-autostash",
    "--no-commit",
    "--no-edit",
    "--no-ff",
    "--no-force-with-lease",
    "--no-progress",
    "--no-rebase",
    "--no-recurse-submodules",
    "--no-show-forced-updates",
    "--no-signoff",
    "--no-squash",
    "--no-stat",
    "--no-tags",
    "--no-thin",
    "--no-verify",
    "--no-write-fetch-head",
    "--porcelain",
    "--progress",
    "--prune",
    "--prune-tags",
    "--quiet",
    "--rebase",
    "--recurse-submodules",
    "--refs",
    "--set-upstream",
    "--show-forced-updates",
    "--signoff",
    "--squash",
    "--stat",
    "--symref",
    "--tags",
    "--thin",
    "--unshallow",
    "--update-head-ok",
    "--update-shallow",
    "--verbose",
    "--verify",
    "--write-fetch-head",
];

/// The transports Git builds in or ships, as address schemes and helper
/// names. Every other scheme or name starts `git-remote-<name>`.
const NATIVE_SCHEMES: &[&str] = &["file", "git", "ssh", "git+ssh", "ssh+git"];
const SHIPPED_HELPERS: &[&str] = &["http", "https", "ftp", "ftps"];

/// The reason prefix of a refusal for a helper program the landed policy
/// does not admit; the runner reader's message for it names the way through.
pub(super) const HELPER_REFUSAL: &str = "a remote's transport starts a helper program";

/// The reason prefix of a refusal for a form whose transport the guard
/// cannot prove; the runner reader's message for it names the way through.
pub(super) const UNPROVEN: &str = "the guard cannot prove which transport Git selects";

/// What Git starts to reach one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Selected {
    /// A transport Git runs itself or ships with: a local path, `file://`,
    /// `ssh://`, the short host form, `git://`, `http(s)://`, `ftp(s)://`.
    Stock,
    /// `git-remote-<name>`.
    Helper(String),
    /// The `ext` helper, which the runner reader refuses on its own terms.
    Ext,
}

/// One destination a command contacts and the transport Git selects for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Contact {
    /// The remote as the command or configuration names it, or the address.
    pub remote: String,
    pub selected: Selected,
}

/// What the resolution learns about the call's repository from Git itself,
/// read with the call's location inputs only.
pub(super) trait Probe {
    /// The repository the call works in.
    fn location(&mut self) -> Result<Location, String>;
    /// The branch the repository has checked out; an error for a detached
    /// HEAD or any failure.
    fn branch(&mut self) -> Result<String, String>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Fetch,
    Push,
}

fn unproven(why: impl std::fmt::Display) -> String {
    format!("{UNPROVEN}: {why}")
}

/// Every destination `args` contacts, with the transport Git selects for
/// it, read from `config`, the configuration the call itself reads.
///
/// # Errors
///
/// A form the resolution cannot prove, a value it cannot read, or a probe
/// of the repository that fails, each named.
pub(super) fn contacts(
    verb: &str,
    rest: &[String],
    config: &EffectiveConfig,
    probe: &mut dyn Probe,
) -> Result<Vec<Contact>, String> {
    let resolver = Resolver::new(config)?;
    let mut found = Vec::new();
    let direction = if PUSHING.contains(&verb) {
        Direction::Push
    } else {
        Direction::Fetch
    };
    if verb == "clone" {
        for word in candidates(rest) {
            resolver.address(word, direction, &mut found);
        }
        resolver.clone_settings(rest)?;
    } else if verb == "archive" {
        for word in remote_values(rest) {
            resolver.word(word, direction, &mut found, probe)?;
        }
    } else if FETCHING.contains(&verb) || PUSHING.contains(&verb) {
        if matches!(verb, "fetch" | "pull") && rest.iter().any(|word| every_remote_option(word)) {
            return Err(unproven(format!(
                "`{verb}` with every remote contacts each configured remote; name the remote"
            )));
        }
        for word in candidates(rest) {
            resolver.word(word, direction, &mut found, probe)?;
        }
        if DEFAULTING.contains(&verb) && !names_repository(rest) {
            if resolver.names_no_remote() {
                // Git falls back to `origin` without the user naming it, and
                // an undefined fallback contacts nothing (`remote.c`
                // `remotes_remote_get_1`) unless a legacy file defines it.
                legacy("origin", probe)?;
            } else if !matches!(verb, "fetch" | "pull") {
                return Err(unproven(format!(
                    "`{verb}` names no remote, and the guard does not resolve the remote it defaults to; name the remote"
                )));
            } else {
                let name = resolver.default_remote(probe)?;
                resolver.named(&name, direction, &mut found)?;
            }
        }
        if matches!(verb, "fetch" | "pull" | "push") {
            resolver.recursion(verb, rest, probe)?;
        }
    } else if verb == "remote" {
        resolver.remote_subcommand(rest, &mut found, probe)?;
    } else if verb == "maintenance" {
        maintenance(rest)?;
    } else if verb == "submodule--helper"
        && !rest
            .first()
            .is_some_and(|sub| LOCAL_SUBMODULE_HELPERS.contains(&sub.as_str()))
    {
        // Git's internal builtin behind `submodule` updates, adds and
        // clones submodules directly; only a subcommand known to stay
        // local passes.
        return Err(unproven(
            "the internal submodule builtin contacts the remotes of submodule repositories, which the guard does not read",
        ));
    } else if verb == "submodule" {
        let operands: Vec<&str> = candidates(rest).collect();
        if matches!(operands.first(), Some(&("update" | "add"))) {
            return Err(unproven(
                "a submodule update or add contacts the remotes of submodule repositories, which the guard does not read",
            ));
        }
    }
    for name in resolver.promisors()? {
        resolver.named(&name, Direction::Fetch, &mut found)?;
    }
    let mut unique: Vec<Contact> = Vec::with_capacity(found.len());
    for contact in found {
        if !unique.contains(&contact) {
            unique.push(contact);
        }
    }
    Ok(unique)
}

/// The words of `rest` that may name a remote or an address: every operand,
/// every word after `--`, and the value of every `--<option>=<value>`.
fn candidates(rest: &[String]) -> impl Iterator<Item = &str> {
    let end = rest.iter().position(|word| word == "--");
    rest.iter().enumerate().filter_map(move |(at, word)| {
        if end.is_some_and(|end| at > end) {
            Some(word.as_str())
        } else if end == Some(at) {
            None
        } else if let Some(option) = word.strip_prefix("--") {
            option.split_once('=').map(|(_, value)| value)
        } else if word.starts_with('-') {
            None
        } else {
            Some(word.as_str())
        }
    })
}

/// Whether a `remote add` in `rest` may fetch: any short option cluster
/// holding `f`, or any long option that abbreviates or negates `--fetch`.
/// A negation still counts, so the reading never relies on option order.
fn remote_add_fetches(rest: &[String]) -> bool {
    rest.iter()
        .skip_while(|word| *word != "add")
        .take_while(|word| *word != "--")
        .any(|word| {
            if let Some(option) = word.strip_prefix("--") {
                let name = option.split_once('=').map_or(option, |(name, _)| name);
                let name = name.strip_prefix("no-").unwrap_or(name);
                !name.is_empty() && "fetch".starts_with(name)
            } else {
                word.len() > 1 && word.starts_with('-') && word.contains('f')
            }
        })
}

/// The values of `archive`'s `--remote` option, the only remote it contacts.
fn remote_values(rest: &[String]) -> Vec<&str> {
    let mut values = Vec::new();
    for (at, word) in rest.iter().enumerate() {
        if let Some(value) = word.strip_prefix("--remote=") {
            values.push(value);
        } else if word == "--remote" {
            if let Some(value) = rest.get(at + 1) {
                values.push(value.as_str());
            }
        }
    }
    values
}

/// `--all` and its unambiguous abbreviation, which make `fetch` contact
/// every configured remote.
fn every_remote_option(word: &str) -> bool {
    matches!(word, "--all" | "--al")
}

/// Whether `rest` surely names the remote: a word that cannot be an
/// option's value is an operand, and the first operand is the remote.
fn names_repository(rest: &[String]) -> bool {
    let mut value_may_follow = false;
    for (at, word) in rest.iter().enumerate() {
        if word == "--" {
            return at + 1 < rest.len();
        }
        if word.len() > 1 && word.starts_with('-') {
            value_may_follow = !word.contains('=') && !FLAGS.contains(&word.as_str());
            continue;
        }
        if !value_may_follow {
            return true;
        }
        value_may_follow = false;
    }
    false
}

/// The subcommands of Git's internal submodule builtin that never contact a
/// remote themselves; a command `foreach` runs is judged on its own.
const LOCAL_SUBMODULE_HELPERS: &[&str] = &[
    "foreach",
    "init",
    "status",
    "sync",
    "deinit",
    "summary",
    "absorbgitdirs",
    "set-url",
    "set-branch",
    "push-check",
];

/// The maintenance tasks that never contact a remote, as Git names them;
/// Git reads a task name without regard to case.
const LOCAL_MAINTENANCE_TASKS: &[&str] = &[
    "gc",
    "commit-graph",
    "loose-objects",
    "incremental-repack",
    "pack-refs",
    "reflog-expire",
    "worktree-prune",
    "rerere-gc",
];

/// `maintenance run` contacts every configured remote when its tasks may
/// include the prefetch task. It passes only when every word after `run`
/// is a task option, in any abbreviation Git accepts, naming a task that
/// never contacts a remote, or a known option that selects no task; any
/// other word refuses, so no spelling hides a task.
fn maintenance(rest: &[String]) -> Result<(), String> {
    let refuse = || {
        unproven(
            "a maintenance run may prefetch from every configured remote; name only tasks that do not prefetch, each with --task=<task>",
        )
    };
    let mut words = rest.iter();
    match words.next().map(String::as_str) {
        Some("run") => {}
        Some(word) if word.starts_with('-') && rest.iter().any(|word| word == "run") => {
            return Err(refuse())
        }
        _ => return Ok(()),
    }
    let mut tasks = 0;
    while let Some(word) = words.next() {
        let Some(option) = word.strip_prefix("--").filter(|o| !o.is_empty()) else {
            if matches!(word.as_str(), "-q") {
                continue;
            }
            return Err(refuse());
        };
        let (name, value) = match option.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (option, None),
        };
        if !name.is_empty() && "task".starts_with(name) {
            let task = match value {
                Some(task) => task,
                None => words.next().map_or("", String::as_str),
            };
            if !LOCAL_MAINTENANCE_TASKS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(task))
            {
                return Err(refuse());
            }
            tasks += 1;
        } else if value.is_some() || !matches!(name, "quiet" | "no-quiet" | "detach" | "no-detach")
        {
            return Err(refuse());
        }
    }
    if tasks == 0 {
        return Err(refuse());
    }
    Ok(())
}

/// The long options of `rest` before `--`, each as its name without the
/// dashes and its `=` value.
fn long_options(rest: &[String]) -> impl Iterator<Item = (&str, Option<&str>)> {
    rest.iter()
        .take_while(|word| *word != "--")
        .filter_map(|word| word.strip_prefix("--"))
        .filter(|option| !option.is_empty())
        .map(|option| match option.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (option, None),
        })
}

/// Whether the option `name` may be a submodule recursion option: one of
/// them, an abbreviation Git may accept for one, or a longer spelling that
/// starts with one.
fn recursion_name(name: &str) -> bool {
    [
        "recurse-submodules",
        "no-recurse-submodules",
        "recursive",
        "no-recursive",
    ]
    .iter()
    .any(|full| full.starts_with(name) || name.starts_with(full))
}

/// Whether the command line of a fetch, pull or push turns submodule
/// recursion on (`Some(true)`) or off (`Some(false)`); the last such option
/// wins. Only the full negation, or the full option with a value Git reads
/// as false, turns it off; an abbreviation or any other value reads as on.
fn recursion_on_line(rest: &[String]) -> Option<bool> {
    let mut state = None;
    for (name, value) in long_options(rest) {
        let off = match (name, value) {
            ("no-recurse-submodules", None) => true,
            ("recurse-submodules", Some(value)) => {
                crate::security::git::config_boolean(value) == Some(false)
            }
            _ => false,
        };
        if off {
            state = Some(false);
        } else if recursion_name(name) {
            state = Some(true);
        }
    }
    state
}

/// Whether a clone's command line turns recursion on (`Some(true)`) or off
/// (`Some(false)`); the last such option wins. A clone reads the value of
/// its recursion option as a pathspec, never as a boolean, so every
/// recursion option, abbreviation or value turns it on, and only the full
/// negation turns it off.
fn clone_recursion_on_line(rest: &[String]) -> Option<bool> {
    let mut state = None;
    for (name, value) in long_options(rest) {
        if name == "no-recurse-submodules" && value.is_none() {
            state = Some(false);
        } else if recursion_name(name) {
            state = Some(true);
        }
    }
    state
}

/// The keys of the settings a clone's own `-c` or `--config` options give
/// the new repository; an option without a value reads as an empty key. A
/// short-option group that holds `c` counts, read from that letter on, so
/// no spelling of the option is missed.
fn clone_setting_keys(rest: &[String]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut words = rest.iter().take_while(|word| *word != "--");
    while let Some(word) = words.next() {
        let short = word
            .strip_prefix('-')
            .filter(|cluster| !cluster.starts_with('-'))
            .and_then(|cluster| cluster.split_once('c'));
        let setting = if let Some((_, setting)) = short {
            if setting.is_empty() {
                words.next().map_or("", String::as_str)
            } else {
                setting
            }
        } else if let Some((name, value)) = word.strip_prefix("--").and_then(|o| o.split_once('='))
        {
            if name.is_empty() || !"config".starts_with(name) {
                continue;
            }
            value
        } else if word.len() > 2 && "--config".starts_with(word.as_str()) {
            words.next().map_or("", String::as_str)
        } else {
            continue;
        };
        let key = setting.split_once('=').map_or(setting, |(key, _)| key);
        keys.push(key.to_ascii_lowercase());
    }
    keys
}

/// The transport Git selects for `address`, in Git's order: a `<name>::`
/// prefix, then the address scheme.
pub(super) fn select(address: &str) -> Selected {
    if let Some(name) = helper_prefix(address) {
        return helper(name);
    }
    match scheme(address) {
        None => Selected::Stock,
        Some(scheme) if NATIVE_SCHEMES.contains(&scheme) => Selected::Stock,
        Some(scheme) => helper(scheme),
    }
}

fn helper(name: &str) -> Selected {
    if name == "ext" {
        Selected::Ext
    } else if SHIPPED_HELPERS.contains(&name) {
        Selected::Stock
    } else {
        Selected::Helper(name.to_string())
    }
}

/// Git's `is_urlschemechar`: a letter or digit, or after the first
/// character also `+`, `-` or `.`.
fn scheme_char(first: bool, c: char) -> bool {
    c.is_ascii_alphanumeric() || (!first && matches!(c, '+' | '-' | '.'))
}

/// The `<name>` of a `<name>::<address>` address, possibly empty, as Git's
/// `transport_get` reads it.
fn helper_prefix(address: &str) -> Option<&str> {
    let end = address
        .char_indices()
        .find(|&(index, c)| !scheme_char(index == 0, c))
        .map_or(address.len(), |(index, _)| index);
    address[end..].starts_with("::").then(|| &address[..end])
}

/// The scheme of a `<scheme>://` address, as Git's `is_url` reads it.
fn scheme(address: &str) -> Option<&str> {
    let (scheme, rest) = address.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (scheme_char(true, first) && chars.all(|c| scheme_char(false, c)) && rest.starts_with("//"))
        .then_some(scheme)
}

/// Every name Git may look up in the legacy remote files: Git's
/// `valid_remote_nick` excludes only a slash on POSIX, and also a backslash
/// on Windows, so this reading holds the names of both.
fn remote_nick(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// Refuse a name that a legacy remote file in the common Git directory may
/// define, unless both files are proven absent.
fn legacy(word: &str, probe: &mut dyn Probe) -> Result<(), String> {
    let location = probe.location()?;
    for folder in ["remotes", "branches"] {
        if !absent(&location.common.join(folder).join(word)) {
            return Err(unproven(format!(
                "remote `{}` may be defined in a legacy remote file in the repository's `{folder}` directory",
                shown_word(word)
            )));
        }
    }
    Ok(())
}

/// Whether `path` is proven absent; anything short of that may exist.
fn absent(path: &Path) -> bool {
    crate::absence::proven_absent(path).unwrap_or(false)
}

/// A remote whose transport the resolution can prove: one address and at
/// most one push address.
struct Remote {
    url: String,
    pushurl: Option<String>,
}

struct Resolver<'a> {
    config: &'a EffectiveConfig,
    /// `(prefix, base)` pairs of `url.<base>.insteadOf`.
    rewrites: Vec<(String, String)>,
    /// `(prefix, base)` pairs of `url.<base>.pushInsteadOf`.
    push_rewrites: Vec<(String, String)>,
    keys: Vec<&'a str>,
}

impl<'a> Resolver<'a> {
    fn new(config: &'a EffectiveConfig) -> Result<Self, String> {
        let keys = config.keys()?;
        let mut rewrites = Vec::new();
        let mut push_rewrites = Vec::new();
        for key in &keys {
            let Some(rest) = key.strip_prefix("url.") else {
                continue;
            };
            let (base, list) = if let Some(base) = rest.strip_suffix(".insteadof") {
                (base, &mut rewrites)
            } else if let Some(base) = rest.strip_suffix(".pushinsteadof") {
                (base, &mut push_rewrites)
            } else {
                continue;
            };
            for value in values(config, key)? {
                list.push((value, base.to_string()));
            }
        }
        Ok(Self {
            config,
            rewrites,
            push_rewrites,
            keys,
        })
    }

    /// Whether the configuration has any `remote.<name>.<variable>` key.
    fn configured(&self, name: &str) -> bool {
        let prefix = format!("remote.{name}.");
        self.keys.iter().any(|key| {
            key.strip_prefix(&prefix)
                .is_some_and(|variable| !variable.is_empty() && !variable.contains('.'))
        })
    }

    /// Whether the configuration names no remote at all: no `remote.*` key
    /// and no branch setting that names a remote.
    fn names_no_remote(&self) -> bool {
        self.keys.iter().all(|key| {
            let branch = key
                .strip_prefix("branch.")
                .and_then(|rest| rest.rsplit_once('.'))
                .is_some_and(|(_, variable)| matches!(variable, "remote" | "pushremote"));
            !key.starts_with("remote.") && !branch
        })
    }

    /// Whether `name` names a remote group.
    fn group(&self, name: &str) -> bool {
        let group = format!("remotes.{name}");
        self.keys.iter().any(|key| key.eq_ignore_ascii_case(&group))
    }

    /// The configured remote `name`, when its form is one the resolution
    /// can prove; `None` when the configuration does not define it.
    fn remote(&self, name: &str) -> Result<Option<Remote>, String> {
        if !self.configured(name) {
            return Ok(None);
        }
        let shown = shown_word(name);
        let key = |variable: &str| format!("remote.{name}.{variable}");
        let unread =
            |why: String| format!("cannot resolve the transport of remote `{shown}`: {why}");
        if !self.config.get_all(&key("vcs")).is_empty() {
            return Err(unproven(format!(
                "remote `{shown}` selects a helper through its configuration"
            )));
        }
        let urls = values_raw(self.config, &key("url")).map_err(unread)?;
        let url = match urls.as_slice() {
            [url] if !url.is_empty() => url.clone(),
            [] => {
                return Err(unproven(format!("remote `{shown}` has no address")));
            }
            _ => {
                return Err(unproven(format!(
                    "remote `{shown}` has more than one address value or an empty one"
                )));
            }
        };
        let pushurls = values_raw(self.config, &key("pushurl")).map_err(unread)?;
        let pushurl = match pushurls.as_slice() {
            [] => None,
            [pushurl] if !pushurl.is_empty() => Some(pushurl.clone()),
            _ => {
                return Err(unproven(format!(
                    "remote `{shown}` has more than one push address value or an empty one"
                )));
            }
        };
        Ok(Some(Remote { url, pushurl }))
    }

    /// Resolve `word`, which the command names, as a configured remote or
    /// an address.
    fn word(
        &self,
        word: &str,
        direction: Direction,
        found: &mut Vec<Contact>,
        probe: &mut dyn Probe,
    ) -> Result<(), String> {
        if self.group(word) {
            return Err(unproven(format!(
                "`{}` names a remote group",
                shown_word(word)
            )));
        }
        if let Some(remote) = self.remote(word)? {
            self.destinations(word, &remote, direction, found);
            return Ok(());
        }
        if remote_nick(word) {
            legacy(word, probe)?;
        }
        self.address(word, direction, found);
        Ok(())
    }

    /// Resolve `name`, which the configuration names, as a configured remote;
    /// a name it does not define refuses.
    fn named(
        &self,
        name: &str,
        direction: Direction,
        found: &mut Vec<Contact>,
    ) -> Result<(), String> {
        if self.group(name) {
            return Err(unproven(format!(
                "`{}` names a remote group",
                shown_word(name)
            )));
        }
        let Some(remote) = self.remote(name)? else {
            return Err(unproven(format!(
                "remote `{}` is not configured",
                shown_word(name)
            )));
        };
        self.destinations(name, &remote, direction, found);
        Ok(())
    }

    /// Resolve `word` as an address.
    fn address(&self, word: &str, direction: Direction, found: &mut Vec<Contact>) {
        let anonymous = Remote {
            url: word.to_string(),
            pushurl: None,
        };
        self.destinations(word, &anonymous, direction, found);
    }

    /// The destinations Git uses for `remote` in `direction`, after its
    /// rewrites (`remote.c` `alias_all_urls`, `push_url_of_remote`).
    fn destinations(
        &self,
        name: &str,
        remote: &Remote,
        direction: Direction,
        found: &mut Vec<Contact>,
    ) {
        let mut addresses: Vec<String> = Vec::new();
        match (direction, &remote.pushurl) {
            (Direction::Fetch, _) => addresses.extend(rewritten(&remote.url, &self.rewrites)),
            (Direction::Push, None) => match matched(&remote.url, &self.push_rewrites) {
                Some(pushed) => addresses.extend(pushed),
                None => addresses.extend(rewritten(&remote.url, &self.rewrites)),
            },
            (Direction::Push, Some(pushurl)) => {
                // Git applies the fetch rewrite to a push address; a push
                // rewrite that also matches is kept too, so no reading of
                // the two drops a destination.
                addresses.extend(rewritten(pushurl, &self.rewrites));
                addresses.extend(matched(pushurl, &self.push_rewrites).unwrap_or_default());
            }
        }
        for address in addresses {
            found.push(Contact {
                remote: name.to_string(),
                selected: select(&address),
            });
        }
    }

    /// The remote an operand-free `fetch` or `pull` contacts: the current
    /// branch's remote, or `origin` when the branch names none, with
    /// fetching every remote turned off.
    fn default_remote(&self, probe: &mut dyn Probe) -> Result<String, String> {
        match self.config.get("fetch.all")? {
            None => {}
            Some(value) => match crate::security::git::config_boolean(value) {
                Some(false) => {}
                Some(true) => {
                    return Err(unproven(
                        "an operand-free fetch contacts every configured remote by configuration; name the remote",
                    ))
                }
                None => {
                    return Err(unproven(
                        "the setting that makes a fetch contact every remote cannot be read",
                    ))
                }
            },
        }
        let branch = probe.branch().map_err(|why| {
            unproven(format!(
                "an operand-free fetch needs the current branch: {why}"
            ))
        })?;
        let key = format!("branch.{branch}.remote");
        let named = values(self.config, &key)?;
        match named.as_slice() {
            [] => Ok("origin".to_string()),
            [name] if !name.is_empty() => Ok(name.clone()),
            _ => Err(unproven(format!(
                "the remote of branch `{}` has more than one value or an empty one",
                shown_word(&branch)
            ))),
        }
    }

    /// Refuse submodule recursion in a repository that has or may have
    /// submodules, unless the command line turns it off.
    fn recursion(&self, verb: &str, rest: &[String], probe: &mut dyn Probe) -> Result<(), String> {
        // Git reads the push setting and the general one in configuration
        // order, so either one turning recursion on reads as on.
        let on = match recursion_on_line(rest) {
            Some(on) => on,
            None if verb == "push" => {
                self.setting_on("push.recursesubmodules")?
                    || self.setting_on("submodule.recurse")?
            }
            None => true,
        };
        if on && self.may_have_submodules(probe)? {
            let way = if verb == "remote" {
                "name the remote in a fetch with --no-recurse-submodules"
            } else {
                "turn recursion off with --no-recurse-submodules"
            };
            return Err(unproven(format!(
                "`{verb}` may recurse into submodule repositories, whose remotes the guard does not read; {way}"
            )));
        }
        Ok(())
    }

    /// Whether any value of the setting `key` may turn recursion on: every
    /// value but one Git reads as false.
    fn setting_on(&self, key: &str) -> Result<bool, String> {
        Ok(values(self.config, key)?
            .iter()
            .any(|value| crate::security::git::config_boolean(value) != Some(false)))
    }

    /// Whether the repository has or may have submodules: a submodule
    /// setting, the common Git directory's `modules` folder or the work
    /// tree's `.gitmodules` file, each unless proven absent.
    fn may_have_submodules(&self, probe: &mut dyn Probe) -> Result<bool, String> {
        if self.keys.iter().any(|key| {
            *key == "submodule.active"
                || key
                    .strip_prefix("submodule.")
                    .is_some_and(|rest| rest.contains('.'))
        }) {
            return Ok(true);
        }
        let location = probe.location()?;
        if !absent(&location.common.join("modules")) {
            return Ok(true);
        }
        if location.bare {
            return Ok(false);
        }
        Ok(location
            .top
            .as_ref()
            .is_none_or(|top| !absent(&top.join(".gitmodules"))))
    }

    /// A clone fetches from its address; refuse a helper setting the
    /// configuration would give the new remote, and recursion into the
    /// submodules the cloned project names.
    fn clone_settings(&self, rest: &[String]) -> Result<(), String> {
        if self.keys.iter().any(|key| {
            key.strip_prefix("remote.")
                .and_then(|rest| rest.rsplit_once('.'))
                .is_some_and(|(_, variable)| variable == "vcs")
        }) {
            return Err(unproven(
                "the configuration gives a remote a helper setting, which a clone would use",
            ));
        }
        if clone_setting_keys(rest)
            .iter()
            .any(|key| key.is_empty() || key.starts_with("submodule."))
        {
            return Err(unproven(
                "a clone's own submodule setting may turn recursion on; clone without it",
            ));
        }
        let on = match clone_recursion_on_line(rest) {
            Some(on) => on,
            None => self.setting_on("submodule.recurse")?,
        };
        if on {
            return Err(unproven(
                "a clone that recurses into submodules contacts the addresses the cloned project names; clone without recursion",
            ));
        }
        Ok(())
    }

    /// The remotes the `remote` subcommands in `rest` contact.
    fn remote_subcommand(
        &self,
        rest: &[String],
        found: &mut Vec<Contact>,
        probe: &mut dyn Probe,
    ) -> Result<(), String> {
        let operands: Vec<&str> = candidates(rest).collect();
        match operands.split_first() {
            Some((&"update", [])) => Err(unproven(
                "`remote update` without a remote contacts every configured remote; name the remote",
            )),
            Some((sub, names)) if matches!(*sub, "update" | "show" | "prune" | "set-head") => {
                for name in names {
                    self.word(name, Direction::Fetch, found, probe)?;
                }
                if *sub == "update" {
                    // `remote update` fetches with no way to turn recursion
                    // off on its own line.
                    self.recursion("remote", &[], probe)?;
                }
                Ok(())
            }
            Some((&"add", words)) => {
                for word in words {
                    self.address(word, Direction::Fetch, found);
                }
                if remote_add_fetches(rest) {
                    self.added_remote(rest, probe)?;
                    self.recursion("remote", &[], probe)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// A `remote add` that fetches uses the new remote's whole
    /// configuration, which may already hold settings for its name, such as
    /// a helper setting. It passes only for one name and one address, with
    /// no other option that takes a value, a name no setting, remote group
    /// or legacy remote file defines.
    fn added_remote(&self, rest: &[String], probe: &mut dyn Probe) -> Result<(), String> {
        let mut operands = Vec::new();
        let mut words = rest.iter().skip_while(|word| *word != "add").skip(1);
        while let Some(word) = words.next() {
            if word == "--" {
                operands.extend(words.by_ref().map(String::as_str));
            } else if word.len() > 1 && word.starts_with('-') {
                let fetch = word
                    .strip_prefix("--")
                    .map(|name| name.strip_prefix("no-").unwrap_or(name))
                    .is_some_and(|name| !name.is_empty() && "fetch".starts_with(name));
                let known = fetch || matches!(word.as_str(), "-f" | "--tags" | "--no-tags");
                if !known {
                    return Err(unproven(format!(
                        "`remote add` with `{}` fetches with settings the guard does not resolve; add the remote, then fetch it by name",
                        shown_word(word)
                    )));
                }
            } else {
                operands.push(word);
            }
        }
        let [name, _address] = operands.as_slice() else {
            return Err(unproven(
                "`remote add` that fetches needs exactly one name and one address",
            ));
        };
        if self.configured(name) || self.group(name) {
            return Err(unproven(format!(
                "the configuration already holds settings for remote `{}`, which the fetch of `remote add` would use",
                shown_word(name)
            )));
        }
        if remote_nick(name) {
            legacy(name, probe)?;
        }
        Ok(())
    }

    /// The remotes a partial clone fetches missing objects from.
    fn promisors(&self) -> Result<Vec<String>, String> {
        let mut names = values(self.config, "extensions.partialclone")?;
        for key in &self.keys {
            let Some(name) = key
                .strip_prefix("remote.")
                .and_then(|rest| rest.strip_suffix(".promisor"))
            else {
                continue;
            };
            if let Some(value) = self.config.get(key)? {
                if crate::security::git::config_boolean(value) != Some(false) {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }
}

/// `url` after the longest matching rewrite in `rewrites`, or as written.
/// Rewrites of the same length that match are all kept, so a tie Git
/// breaks by configuration order adds a destination instead of guessing.
fn rewritten(url: &str, rewrites: &[(String, String)]) -> Vec<String> {
    matched(url, rewrites).unwrap_or_else(|| vec![url.to_string()])
}

fn matched(url: &str, rewrites: &[(String, String)]) -> Option<Vec<String>> {
    let longest = rewrites
        .iter()
        .filter(|(prefix, _)| url.starts_with(prefix.as_str()))
        .map(|(prefix, _)| prefix.len())
        .max()?;
    let mut out: Vec<String> = rewrites
        .iter()
        .filter(|(prefix, _)| prefix.len() == longest && url.starts_with(prefix.as_str()))
        .map(|(prefix, base)| format!("{base}{}", &url[prefix.len()..]))
        .collect();
    out.sort();
    out.dedup();
    Some(out)
}

/// Every value of `key`, in Git's order, each as UTF-8.
fn values(config: &EffectiveConfig, key: &str) -> Result<Vec<String>, String> {
    values_raw(config, key).map_err(|why| format!("cannot resolve a remote transport: {why}"))
}

fn values_raw(config: &EffectiveConfig, key: &str) -> Result<Vec<String>, String> {
    config
        .get_all(key)
        .into_iter()
        .map(|(_, _, bytes)| {
            std::str::from_utf8(bytes)
                .map(str::to_string)
                .map_err(|_| format!("`{}` is not UTF-8", shown_word(key)))
        })
        .collect()
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
