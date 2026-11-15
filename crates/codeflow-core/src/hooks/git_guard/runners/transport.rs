//! The transport Git selects for each remote a command contacts (TSK-1489).
//!
//! To reach a remote, Git may start a helper program, `git-remote-<name>`.
//! It picks one from the remote's configuration (`remote.<name>.vcs`), from
//! a `<name>::<address>` address, or from an address scheme it does not
//! handle itself, after the rewrites `url.<base>.insteadOf` and
//! `url.<base>.pushInsteadOf` apply (Git's `transport_get` and
//! `remote.c`). Both guards read this one resolution through the runner
//! reader, so a helper program is either admitted by the landed policy and
//! judged as a program word, or refused; anything the resolution cannot
//! read refuses, naming the remote.
//!
//! The remotes a command contacts are read generously, so a doubt adds a
//! remote, never drops one:
//!
//! - every operand word of a contacting verb, and every `--<option>=<value>`
//!   value, read as a remote name, a remote group or an address;
//! - for a verb that has a default remote, every remote it may default to:
//!   `origin`, each `branch.<name>.remote` and, for a push,
//!   `branch.<name>.pushRemote` and `remote.pushDefault`;
//! - every configured remote for `fetch --all`, `remote update` with no
//!   remote named, and `maintenance`;
//! - the promisor remotes of a partial clone, for every Git command, since
//!   a missing object can be fetched from them by any command;
//! - each `submodule.<name>.url` for a command that can recurse into
//!   submodules. A submodule command runs its transports with
//!   `GIT_PROTOCOL_FROM_USER=0`, so Git itself refuses a helper there
//!   unless `protocol.allow` or `protocol.<name>.allow` is `always`; such a
//!   setting is refused as unresolved, because the guard does not read the
//!   addresses a clone's submodules will name.

use super::super::shown_word;
use crate::security::git_config::EffectiveConfig;
use std::path::Path;

/// The verbs that contact a remote, and how.
const FETCHING: &[&str] = &[
    "fetch",
    "pull",
    "ls-remote",
    "clone",
    "fetch-pack",
    "archive",
];
const PUSHING: &[&str] = &["push", "send-pack"];
/// Verbs whose remote defaults to a configured one when none is named.
const DEFAULTING: &[&str] = &["fetch", "pull", "ls-remote", "push"];
/// Verbs that can recurse into submodules and contact their remotes.
const RECURSING: &[&str] = &["clone", "fetch", "pull", "push", "submodule"];

/// The transports Git builds in or ships, as address schemes and helper
/// names. Every other scheme or name starts `git-remote-<name>`.
const NATIVE_SCHEMES: &[&str] = &["file", "git", "ssh", "git+ssh", "ssh+git"];
const SHIPPED_HELPERS: &[&str] = &["http", "https", "ftp", "ftps"];

/// The reason prefix of a refusal for a helper program the landed policy
/// does not admit; the runner reader's message for it names the way through.
pub(super) const HELPER_REFUSAL: &str = "a remote's transport starts a helper program";

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Fetch,
    Push,
}

/// Every destination `args` contacts, with the transport Git selects for
/// it, read from `config`. The repository's common Git directory, the
/// folder of its local `config` file, is where Git also reads remotes
/// defined outside its configuration.
///
/// # Errors
///
/// A value the resolution needs is not UTF-8, a remote group names an
/// unknown remote, a remote may be defined outside the configuration, or
/// the configuration allows every protocol for a submodule command.
pub(super) fn contacts(
    verb: &str,
    rest: &[String],
    config: &EffectiveConfig,
) -> Result<Vec<Contact>, String> {
    let resolver = Resolver::new(config)?;
    let mut found = Vec::new();
    let direction = if PUSHING.contains(&verb) {
        Direction::Push
    } else {
        Direction::Fetch
    };
    let words = || candidates(rest);
    if FETCHING.contains(&verb) || PUSHING.contains(&verb) {
        for word in words() {
            resolver.word(word, direction, &mut found)?;
        }
        if DEFAULTING.contains(&verb) {
            for name in resolver.defaults(direction)? {
                resolver.word(&name, direction, &mut found)?;
            }
        }
        if matches!(verb, "fetch" | "pull") && rest.iter().any(|word| word == "--all") {
            resolver.all(direction, &mut found)?;
        }
    } else if verb == "remote" {
        let operands: Vec<&str> = words().collect();
        match operands.split_first() {
            Some((&"update", [])) => resolver.all(Direction::Fetch, &mut found)?,
            Some((sub, rest))
                if matches!(*sub, "update" | "show" | "prune" | "set-head" | "add") =>
            {
                for word in rest {
                    resolver.word(word, Direction::Fetch, &mut found)?;
                }
            }
            _ => {}
        }
    } else if verb == "maintenance" {
        resolver.all(Direction::Fetch, &mut found)?;
    }
    if RECURSING.contains(&verb) {
        resolver.submodules(&mut found)?;
    }
    for name in resolver.promisors()? {
        resolver.word(&name, Direction::Fetch, &mut found)?;
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
/// and the value of every `--<option>=<value>`.
fn candidates(rest: &[String]) -> impl Iterator<Item = &str> {
    rest.iter().filter_map(|word| {
        if let Some(option) = word.strip_prefix("--") {
            option.split_once('=').map(|(_, value)| value)
        } else if word.starts_with('-') {
            None
        } else {
            Some(word.as_str())
        }
    })
}

/// The transport Git selects for `address`, or for a remote whose
/// `remote.<name>.vcs` is `vcs`, in Git's order: the remote's helper, a
/// `<name>::` prefix, then the address scheme.
pub(super) fn select(address: &str, vcs: Option<&str>) -> Selected {
    if let Some(name) = vcs {
        return helper(name);
    }
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

/// Git's `valid_remote_nick`.
fn remote_nick(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

struct Remote {
    urls: Vec<String>,
    /// The first URL before the last empty value cleared the list, which an
    /// older Git still reads as the first.
    first_before_reset: Option<String>,
    pushurls: Vec<String>,
    vcs: Option<String>,
}

struct Resolver<'a> {
    config: &'a EffectiveConfig,
    /// The common Git directories: the folders of the local scope's
    /// `config` files.
    local: Vec<&'a Path>,
    /// `(prefix, base)` pairs of `url.<base>.insteadOf`.
    rewrites: Vec<(String, String)>,
    /// `(prefix, base)` pairs of `url.<base>.pushInsteadOf`.
    push_rewrites: Vec<(String, String)>,
    keys: Vec<&'a str>,
}

impl<'a> Resolver<'a> {
    fn new(config: &'a EffectiveConfig) -> Result<Self, String> {
        let keys = config.keys()?;
        let local = config
            .local
            .iter()
            .filter(|file| file.file_name().is_some_and(|name| name == "config"))
            .filter_map(|file| file.parent())
            .collect();
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
            local,
            rewrites,
            push_rewrites,
            keys,
        })
    }

    /// The configured remote `name`, when the configuration gives it a
    /// URL, a push URL or a helper.
    fn remote(&self, name: &str) -> Result<Option<Remote>, String> {
        let key = |variable: &str| format!("remote.{name}.{variable}");
        let unresolved = |why: String| {
            format!(
                "cannot resolve the transport of remote `{}`: {why}",
                shown_word(name)
            )
        };
        let mut urls = Vec::new();
        let mut first_before_reset = None;
        for value in values_raw(self.config, &key("url")).map_err(unresolved)? {
            if value.is_empty() {
                first_before_reset = first_before_reset.or_else(|| urls.first().cloned());
                urls.clear();
            } else {
                urls.push(value);
            }
        }
        let mut pushurls = Vec::new();
        for value in values_raw(self.config, &key("pushurl")).map_err(unresolved)? {
            if value.is_empty() {
                pushurls.clear();
            } else {
                pushurls.push(value);
            }
        }
        let vcs = values_raw(self.config, &key("vcs"))
            .map_err(unresolved)?
            .pop()
            .filter(|value| !value.is_empty());
        if urls.is_empty() && pushurls.is_empty() && vcs.is_none() && first_before_reset.is_none() {
            return Ok(None);
        }
        Ok(Some(Remote {
            urls,
            first_before_reset,
            pushurls,
            vcs,
        }))
    }

    /// Resolve `word` as a configured remote, a remote group or an address.
    fn word(
        &self,
        word: &str,
        direction: Direction,
        found: &mut Vec<Contact>,
    ) -> Result<(), String> {
        if let Some(remote) = self.remote(word)? {
            self.destinations(word, &remote, direction, found);
            return Ok(());
        }
        let group = format!("remotes.{word}");
        if self.keys.iter().any(|key| key.eq_ignore_ascii_case(&group)) {
            for members in values(self.config, &group)? {
                for member in members.split_ascii_whitespace() {
                    let Some(remote) = self.remote(member)? else {
                        return Err(format!(
                            "cannot resolve the transport of remote `{}`: the remote group `{}` names it, and no remote by that name is configured",
                            shown_word(member),
                            shown_word(word)
                        ));
                    };
                    self.destinations(member, &remote, direction, found);
                }
            }
            return Ok(());
        }
        if remote_nick(word) {
            for local in &self.local {
                for folder in ["remotes", "branches"] {
                    let path = local.join(folder).join(word);
                    // Anything short of proven absence may define the remote.
                    let present = !crate::absence::proven_absent(&path).unwrap_or(false);
                    if present {
                        return Err(format!(
                            "cannot resolve the transport of remote `{}`: it may be defined in the repository's `{folder}` directory, which the guard does not read",
                            shown_word(word)
                        ));
                    }
                }
            }
        }
        let anonymous = Remote {
            urls: vec![word.to_string()],
            first_before_reset: None,
            pushurls: Vec::new(),
            vcs: None,
        };
        self.destinations(word, &anonymous, direction, found);
        Ok(())
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
        match direction {
            Direction::Fetch => {
                for url in remote
                    .urls
                    .first()
                    .into_iter()
                    .chain(&remote.first_before_reset)
                {
                    addresses.extend(rewritten(url, &self.rewrites));
                }
            }
            Direction::Push => {
                if remote.pushurls.is_empty() {
                    // A URL a push rewrite matches is pushed as rewritten.
                    // Whether Git still pushes to the URLs no push rewrite
                    // matches, when some do, is read both ways: each is
                    // kept with its fetch rewrite.
                    for url in &remote.urls {
                        match matched(url, &self.push_rewrites) {
                            Some(pushed) => addresses.extend(pushed),
                            None => addresses.extend(rewritten(url, &self.rewrites)),
                        }
                    }
                } else {
                    // Git applies the fetch rewrite to a push URL; a push
                    // rewrite that also matches is kept too, so no reading
                    // of the two drops a destination.
                    for url in &remote.pushurls {
                        addresses.extend(rewritten(url, &self.rewrites));
                        addresses.extend(matched(url, &self.push_rewrites).unwrap_or_default());
                    }
                }
            }
        }
        if addresses.is_empty() {
            // A remote with a helper and no address still starts it.
            addresses.push(String::new());
        }
        for address in addresses {
            found.push(Contact {
                remote: name.to_string(),
                selected: select(&address, remote.vcs.as_deref()),
            });
        }
    }

    /// Every remote `direction` may default to.
    fn defaults(&self, direction: Direction) -> Result<Vec<String>, String> {
        let mut names = vec!["origin".to_string()];
        for key in &self.keys {
            let Some(rest) = key.strip_prefix("branch.") else {
                continue;
            };
            let default_key = rest.ends_with(".remote")
                || (direction == Direction::Push && rest.ends_with(".pushremote"));
            if default_key {
                names.extend(values(self.config, key)?);
            }
        }
        if direction == Direction::Push {
            names.extend(values(self.config, "remote.pushdefault")?);
        }
        names.retain(|name| !name.is_empty());
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Every configured remote, resolved in `direction`.
    fn all(&self, direction: Direction, found: &mut Vec<Contact>) -> Result<(), String> {
        for name in self.names() {
            self.word(&name, direction, found)?;
        }
        Ok(())
    }

    /// The names of every remote the configuration defines.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .keys
            .iter()
            .filter_map(|key| {
                let rest = key.strip_prefix("remote.")?;
                let (name, _) = rest.rsplit_once('.')?;
                Some(name.to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The remotes a partial clone fetches missing objects from.
    fn promisors(&self) -> Result<Vec<String>, String> {
        let mut names = values(self.config, "extensions.partialclone")?;
        for name in self.names() {
            let key = format!("remote.{name}.promisor");
            if let Some(value) = self.config.get(&key)? {
                if crate::security::git::config_boolean(value) != Some(false) {
                    names.push(name);
                }
            }
        }
        names.retain(|name| !name.is_empty());
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// The submodule addresses the configuration names, and a refusal when
    /// it lets submodule commands use every protocol.
    fn submodules(&self, found: &mut Vec<Contact>) -> Result<(), String> {
        for key in &self.keys {
            if key == &"protocol.allow"
                || key.strip_prefix("protocol.").is_some_and(|rest| {
                    rest.strip_suffix(".allow").is_some_and(|name| {
                        !NATIVE_SCHEMES.contains(&name) && !SHIPPED_HELPERS.contains(&name)
                    })
                })
            {
                for value in values(self.config, key)? {
                    if value.eq_ignore_ascii_case("always") {
                        return Err(format!(
                            "cannot resolve the transports of submodules: `{key}` lets them start a helper program the guard cannot read"
                        ));
                    }
                }
            }
        }
        for key in &self.keys {
            let Some(name) = key
                .strip_prefix("submodule.")
                .and_then(|rest| rest.strip_suffix(".url"))
            else {
                continue;
            };
            for url in values(self.config, key)? {
                let anonymous = Remote {
                    urls: vec![url],
                    first_before_reset: None,
                    pushurls: Vec::new(),
                    vcs: None,
                };
                self.destinations(name, &anonymous, Direction::Fetch, found);
            }
        }
        Ok(())
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
