//! Ref plumbing and transport must not replace the landed policy authority.
use std::path::Path;

/// Keys whose writes could replace a configured remote or its transport.
pub(super) fn protected_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "fetch.followremotehead"
        || key.starts_with("remote.")
        || key.starts_with("remotes.")
        || key.starts_with("include.")
        || key.starts_with("includeif.")
        || (key.starts_with("url.")
            && (key.ends_with(".insteadof") || key.ends_with(".pushinsteadof")))
}

/// Return a refusal reason for authority-changing Git arguments.
pub(super) fn check(root: &Path, args: &[String]) -> Option<String> {
    let (sub, rest) = super::git_guard::git_subcommand(args)?;
    let globals = &args[..args.len() - rest.len() - 1];
    let protected = |arg: &String| {
        let key = arg
            .strip_prefix("--config-env=")
            .or_else(|| arg.strip_prefix("-c"))
            .unwrap_or(arg);
        protected_key(key.split('=').next().unwrap_or(key))
    };
    if globals.iter().any(protected) || (sub == "config" && rest.iter().any(protected)) {
        let read = sub == "config"
            && !rest.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--unset"
                        | "--unset-all"
                        | "--remove-section"
                        | "--rename-section"
                        | "unset"
                        | "set"
                        | "rename-section"
                        | "remove-section"
                )
            })
            && (rest.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--get" | "--get-all" | "--get-regexp" | "--list" | "-l" | "get" | "list"
                )
            }) || rest.iter().filter(|a| !a.starts_with('-')).count() == 1);
        if !read {
            return Some(
                "writing remote, HEAD-follow, remote-group, URL rewrite or include configuration can replace policy authority"
                    .into(),
            );
        }
    }
    match sub {
        "update-ref" | "symbolic-ref" if super::git_guard::ref_plumbing_tampers(sub, rest) => Some(
            "remote-tracking refs may only be updated by the configured remote's fetch mapping"
                .into(),
        ),
        "branch"
            if rest.iter().any(|s| {
                s == "--remotes" || (s.starts_with('-') && !s.starts_with("--") && s.contains('r'))
            }) && rest.iter().any(|s| {
                s == "--delete"
                    || (s.starts_with('-') && !s.starts_with("--") && s.contains(['d', 'D']))
            }) =>
        {
            Some("deleting remote-tracking branches removes policy authority".into())
        }
        "remote"
            if rest.iter().find(|s| !s.starts_with('-')).is_some_and(|s| {
                matches!(
                    s.as_str(),
                    "add" | "set-url" | "set-head" | "rename" | "remove" | "rm"
                )
            }) =>
        {
            Some("the operator manages remote identity and default-branch authority".into())
        }
        "remote" if remote_transport_args(sub, rest).is_some() => {
            check_remote_transport(root, remote_transport_args(sub, rest).unwrap())
        }
        "fetch" | "pull" => fetch(root, rest, sub == "pull"),
        "push" => {
            let operands = operands(rest, false);
            let destination = rest
                .iter()
                .enumerate()
                .filter_map(|(i, arg)| {
                    arg.strip_prefix("--repo=").or_else(|| {
                        (arg == "--repo")
                            .then(|| rest.get(i + 1).map(String::as_str))
                            .flatten()
                    })
                })
                .next_back()
                .or_else(|| operands.first().map(String::as_str))?;
            let repo = git2::Repository::discover(root).ok()?;
            if repo.find_remote(destination).is_err()
                && (destination == "."
                    || destination == ".."
                    || destination.starts_with('/')
                    || destination.starts_with("./")
                    || destination.starts_with("../")
                    || destination.starts_with("file:")
                    || (destination.contains('/') && !destination.contains(':'))
                    || root.join(destination).exists())
            {
                Some("pushing to a local path can rewrite policy or branch refs; push through the configured remote".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Shared transport classification, including remote's leading verbose option.
pub(super) fn remote_transport_args<'a>(
    sub: &str,
    rest: &'a [String],
) -> Option<(&'a str, &'a [String])> {
    if sub != "remote" {
        return None;
    }
    let index = rest.iter().position(|arg| !arg.starts_with('-'))?;
    matches!(rest[index].as_str(), "update" | "prune")
        .then(|| (rest[index].as_str(), &rest[index + 1..]))
}

fn check_remote_transport(root: &Path, (operation, args): (&str, &[String])) -> Option<String> {
    if operation == "prune" {
        args.iter()
            .filter(|arg| !arg.starts_with('-'))
            .find_map(|remote| fetch(root, std::slice::from_ref(remote), false))
    } else {
        remote_update(root, args)
    }
}

fn remote_update(root: &Path, args: &[String]) -> Option<String> {
    match update_remotes(root, args) {
        Ok(names) => names
            .into_iter()
            .find_map(|name| fetch(root, &[name], false)),
        Err(reason) => Some(reason),
    }
}

/// What a config value that is not valid UTF-8 reads as. It holds a NUL, so
/// it is no remote name and no boolean.
const UNREADABLE_VALUE: &str = "\0not-valid-utf8";

/// The `key`, `value` pairs of `git config --null --list`.
///
/// OS text rule (issue 79, `docs/architecture.md`): the listing holds every
/// value in the user's effective git configuration, such as a name or an
/// alias in another encoding, and this check reads only `remotes.*` and
/// `remote.<name>.skip*` keys and boolean values. A value that is not valid
/// UTF-8 reads as [`UNREADABLE_VALUE`], never as a lossy spelling: it equals
/// no boolean, and as a `remotes.*` group member it names no remote, so a
/// fetch of it is refused as not configured, while an unrelated key's value
/// cannot refuse the command. A key is
/// identity: it names a remote, so every key is retained as a lossless storage
/// key, including one that is not valid UTF-8. No lossy spelling can replace
/// a different remote's setting. Such an unreadable remote
/// is refused by name in [`utf8_remote_names`].
fn config_entries(listing: &[u8]) -> Vec<(String, String)> {
    listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (key, value) = match entry.iter().position(|byte| *byte == b'\n') {
                Some(split) => (&entry[..split], &entry[split + 1..]),
                None => (entry, b"true".as_slice()),
            };
            let key = crate::git::GitName::from_bytes(key).storage_key();
            let value = std::str::from_utf8(value)
                .map_or_else(|_| UNREADABLE_VALUE.to_string(), str::to_string);
            (key, value)
        })
        .collect()
}

fn update_remotes(root: &Path, args: &[String]) -> Result<Vec<String>, String> {
    // Read the same effective config as Git, including includes and global scope.
    let config = crate::git::command()
        .current_dir(root)
        .args(["config", "--null", "--list"])
        .output()
        .map_err(|error| format!("cannot inspect remote update configuration: {error}"))?;
    if !config.status.success() {
        return Err("cannot inspect remote update configuration; the operator checks git config --show-origin --list".into());
    }
    let entries = config_entries(&config.stdout);
    for (key, value) in &entries {
        if value == UNREADABLE_VALUE
            && (key.starts_with("remotes.")
                || (key.starts_with("remote.")
                    && (key.ends_with(".skipdefaultupdate") || key.ends_with(".skipfetchall"))))
        {
            return Err(format!(
                "cannot read remote update configuration value for {key} as UTF-8"
            ));
        }
    }
    let entries: Vec<_> = entries
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let group = |name: &str| {
        let key = format!("remotes.{name}");
        entries
            .iter()
            .filter(|(k, _)| *k == key)
            .flat_map(|(_, value)| value.split([' ', '\t', '\n']))
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let mut options = true;
    let mut selected: Vec<_> = args
        .iter()
        .filter(|arg| {
            if options && arg.as_str() == "--" {
                options = false;
                false
            } else {
                !options || !arg.starts_with('-')
            }
        })
        .cloned()
        .collect();
    if selected.is_empty() {
        selected.push("default".into());
    }
    if selected.last().is_some_and(|name| name == "default")
        && !entries.iter().any(|(key, _)| *key == "remotes.default")
    {
        let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
        let remotes = repo.remotes().map_err(|error| error.to_string())?;
        let mut names = Vec::new();
        for name in utf8_remote_names(&remotes)? {
            let skip_key = format!("remote.{name}.skipdefaultupdate");
            let alias_key = format!("remote.{name}.skipfetchall");
            let skip = entries
                .iter()
                .rev()
                .find(|(key, _)| *key == skip_key || *key == alias_key);
            let skip = skip
                .map(|(_, value)| {
                    git2::Config::parse_bool(value).map_err(|error| {
                        format!("cannot read remote update skip setting for {name}: {error}")
                    })
                })
                .transpose()?
                .unwrap_or(false);
            if !skip {
                names.push(name.clone());
            }
        }
        return Ok(names);
    }
    Ok(selected
        .into_iter()
        .flat_map(|name| {
            let members = group(&name);
            if members.is_empty() {
                vec![name]
            } else {
                members
            }
        })
        .collect())
}

/// The configured remote names as text.
///
/// OS text rule (issue 79, `docs/architecture.md`): kept strict, and
/// refusing. A remote name picks the fetch mapping the guard proves does not
/// write policy authority, and a name it cannot read is a remote it cannot
/// prove, so skipping it would let `git fetch` or `git remote update` run an
/// unchecked mapping. The operator reads the remote with `git remote -v`.
pub(super) fn utf8_remote_names(
    remotes: &git2::string_array::StringArray,
) -> Result<Vec<String>, String> {
    remotes
        .iter_bytes()
        .map(|name| {
            std::str::from_utf8(name).map(str::to_owned).map_err(|_| {
                "a configured remote name is not valid UTF-8, so its fetch mapping cannot be checked; the operator inspects git remote -v".to_string()
            })
        })
        .collect()
}

fn operands(args: &[String], pull: bool) -> Vec<String> {
    let mut result = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(
            arg.as_str(),
            "--repo"
                | "--upload-pack"
                | "--receive-pack"
                | "--exec"
                | "--depth"
                | "--deepen"
                | "--shallow-since"
                | "--shallow-exclude"
                | "-j"
                | "--jobs"
        ) || (pull
            && matches!(
                arg.as_str(),
                "-s" | "--strategy" | "-X" | "--strategy-option"
            ))
        {
            skip = true;
            continue;
        }
        if !arg.starts_with('-') {
            result.push(arg.clone());
        }
    }
    result
}

fn fetch(root: &Path, args: &[String], pull: bool) -> Option<String> {
    if args
        .iter()
        .any(|arg| arg.starts_with("--refmap") || arg.contains(":refs/") || arg.contains(":+refs/"))
    {
        return Some("fetch must use the configured remote's own mapping, without a destination ref or --refmap".into());
    }
    let repo = git2::Repository::discover(root).ok()?;
    let operands = operands(args, pull);
    let names = repo.remotes().ok()?;
    let names = match utf8_remote_names(&names) {
        Ok(names) => names,
        Err(reason) => return Some(reason),
    };
    let names: Vec<_> = names.iter().map(String::as_str).collect();
    let requested = operands.first().map(String::as_str);
    let selected = requested.or_else(|| {
        if names.contains(&"origin") {
            Some("origin")
        } else {
            names.first().copied()
        }
    });
    let selected = selected?;
    let Some(remote) = repo.find_remote(selected).ok().or_else(|| {
        names.iter().find_map(|name| {
            repo.find_remote(name)
                .ok()
                .filter(|r| r.url().ok() == Some(selected))
        })
    }) else {
        return Some("fetch source is not a configured remote or its URL".into());
    };
    // OS text rule (issue 79): the remote is named in git config keys and
    // arguments below, which need text. A name that is not valid UTF-8
    // refuses instead of passing the fetch unchecked.
    let remote_name = crate::git::GitName::from_bytes(remote.name_bytes()?);
    let Ok(name) = remote_name.rule_text() else {
        return Some(format!(
            "the fetch remote's name is not valid UTF-8 ({}); the operator renames it",
            remote_name.display()
        ));
    };
    // Remote::url already expands insteadOf. Config retains the literal URL.
    let raw = match repo.config().and_then(|config| config.get_string(&format!("remote.{name}.url"))) {
        Ok(raw) => raw,
        Err(error) => return Some(format!("cannot read raw remote.{name}.url: {error}; the operator inspects git config --show-origin --get remote.{name}.url")),
    };
    // Git applies includes and insteadOf at every scope; compare its resolved URL.
    let effective = crate::git::command()
        .current_dir(root)
        .args(["remote", "get-url", name])
        .output()
        .ok()?;
    // OS text rule (issue 79): a URL that is not valid UTF-8 differs from the
    // configured text, so it is a refusal.
    if !effective.status.success()
        || std::str::from_utf8(&effective.stdout)
            .map_or(true, |text| text.strip_suffix('\n').unwrap_or(text) != raw)
    {
        return Some(format!("effective URL for {name} differs from remote.{name}.url; the operator inspects git config --show-origin --get-regexp 'url.*|include.*'"));
    }
    if requested.is_some_and(|source| !names.contains(&source) && source != raw) {
        return Some("fetch source is not the configured remote's URL".into());
    }
    if operands.iter().skip(1).any(|s| s.contains(':')) {
        return Some("an explicit fetch destination can replace policy authority; use git fetch with its configured mapping".into());
    }
    None
}

/// A recovery fetch must be a single plain configured-remote fetch.
#[must_use]
pub fn recovery_fetch(command: &str, root: &Path) -> bool {
    let commands = super::git_guard::simple_commands(command);
    if commands.len() != 1 || super::git_guard::transport_config_environment(command) {
        return false;
    }
    let words = &commands[0];
    words.first().is_some_and(|s| s == "git")
        && words.get(1).is_some_and(|s| s == "fetch")
        && words.len() <= 3
        && words.get(2).is_none_or(|s| {
            git2::Repository::discover(root).is_ok_and(|r| r.find_remote(s).is_ok())
        })
        && check(root, &words[1..]).is_none()
}

#[cfg(test)]
mod tests {
    #[test]
    fn r15_owned_config_keys_are_not_dropped_on_decode_failure() {
        let entries = config_entries(b"remote.caf\xff.url\nurl\0remote.ok.url\nurl2\0");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            crate::git::GitName::from_storage_key(&entries[0].0).bytes(),
            b"remote.caf\xff.url"
        );
    }

    use super::*;

    /// A repository with the remote `origin` and `extra` lines appended to its
    /// configuration, written as bytes so the lines can hold text that is not
    /// valid UTF-8.
    fn repository_with_config(extra: &[u8]) -> tempfile::TempDir {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        repo.remote("origin", "https://example.invalid/origin.git")
            .unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config.write_all(extra).unwrap();
        dir
    }

    /// Round twelve on issue 79: an `insteadOf` rewrite that ends in a carriage
    /// return gives a different URL, so the fetch is refused and not matched
    /// to the configured one by trimming.
    #[test]
    fn a_rewritten_url_that_differs_by_a_carriage_return_is_refused() {
        let dir = repository_with_config(
            b"[url \"https://example.invalid/origin.git\r\"]\n\tinsteadOf = https://example.invalid/origin.git\n",
        );
        let why = fetch(dir.path(), &[], false).expect("a refusal");
        assert!(why.contains("effective URL for origin differs"), "{why}");
    }

    /// Issue 79: one value in the user's git configuration that is not valid
    /// UTF-8 used to refuse every `git remote update`.
    #[test]
    fn a_config_value_that_is_not_utf8_does_not_refuse_the_remote_update() {
        let dir = repository_with_config(b"[user]\n\tname = caf\xe9\n");
        assert_eq!(update_remotes(dir.path(), &[]).unwrap(), ["origin"]);
        assert!(check_remote_transport(dir.path(), ("update", &[])).is_none());
    }

    #[test]
    fn a_remote_group_is_still_read_beside_a_value_that_is_not_utf8() {
        let dir =
            repository_with_config(b"[user]\n\tname = caf\xe9\n[remotes]\n\tgroup = origin\n");
        assert_eq!(
            update_remotes(dir.path(), &["group".to_string()]).unwrap(),
            ["origin"]
        );
    }

    /// Review finding: a config key that is not valid UTF-8 must not become
    /// the key of a different, valid remote. `remote."caf\xff"` read lossily is
    /// `remote."caf\u{fffd}"`, so its `skipdefaultupdate` would hide that
    /// valid remote from `git remote update`.
    #[test]
    fn a_config_key_that_is_not_utf8_never_sets_another_remotes_option() {
        let dir = repository_with_config(
            b"[remote \"caf\xef\xbf\xbd\"]\n\turl = https://example.invalid/b.git\n[remote \"caf\xff\"]\n\tskipdefaultupdate = true\n",
        );
        // The valid remote stays in the update, whatever the other key says.
        assert_eq!(
            update_remotes(dir.path(), &[]).unwrap(),
            ["caf\u{fffd}", "origin"]
        );
        let parsed =
            config_entries(b"remote.caf\xff.skipdefaultupdate\ntrue\0remote.ok.url\nu\0flag\0");
        assert_eq!(
            parsed,
            [
                (
                    crate::git::GitName::from_bytes(b"remote.caf\xff.skipdefaultupdate")
                        .storage_key(),
                    "true".to_string()
                ),
                ("remote.ok.url".to_string(), "u".to_string()),
                ("flag".to_string(), "true".to_string())
            ]
        );
    }

    /// Kept strict: a remote name the guard cannot read is a mapping it cannot
    /// prove, so the command is refused rather than skipped.
    #[test]
    fn a_remote_name_that_is_not_utf8_refuses_the_command() {
        let dir = repository_with_config(
            b"[remote \"caf\xe9\"]\n\turl = https://example.invalid/other.git\n",
        );
        let reason = update_remotes(dir.path(), &[]).unwrap_err();
        assert!(reason.contains("not valid UTF-8"), "{reason}");
        let refusal = fetch(dir.path(), &[], false).expect("a fetch is refused");
        assert!(refusal.contains("not valid UTF-8"), "{refusal}");
    }

    /// Issue 79: a config value that is not valid UTF-8 reads as one marker
    /// that is no remote name and no boolean, never as its lossy spelling.
    #[test]
    fn a_config_value_that_is_not_utf8_names_no_remote() {
        let entries = config_entries(b"remotes.group\ncaf\xe9\0remote.a.skipDefaultUpdate\0");
        assert_eq!(
            entries[0],
            ("remotes.group".to_string(), UNREADABLE_VALUE.to_string())
        );
        assert_eq!(
            entries[1],
            ("remote.a.skipDefaultUpdate".to_string(), "true".to_string())
        );
        assert!(UNREADABLE_VALUE.contains('\0'));
    }
}
