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
    let text = String::from_utf8(config.stdout)
        .map_err(|_| "cannot decode remote update configuration".to_string())?;
    let entries: Vec<_> = text
        .split_terminator('\0')
        .map(|entry| entry.split_once('\n').unwrap_or((entry, "true")))
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
        for name in remotes.iter().flatten().flatten() {
            let skip_key = format!("remote.{name}.skipdefaultupdate");
            let alias_key = format!("remote.{name}.skipfetchall");
            let skip = entries
                .iter()
                .rev()
                .find(|(key, _)| *key == skip_key || *key == alias_key);
            if !skip.is_some_and(|(_, value)| git2::Config::parse_bool(*value).unwrap_or(false)) {
                names.push(name.to_string());
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
    let names: Vec<_> = names.iter().flatten().flatten().collect();
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
    let name = remote.name().ok()??;
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
    if !effective.status.success() || String::from_utf8_lossy(&effective.stdout).trim() != raw {
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
