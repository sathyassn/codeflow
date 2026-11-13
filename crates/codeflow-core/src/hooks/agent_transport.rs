//! Agents fetch from `origin` and push their own work branches (TSK-1479,
//! operator decision D1 of 2026-11-12).
//!
//! exec-guard judges every program a Git transport call starts as a command
//! of its own, so the configured GitHub CLI credential helper,
//! `gh auth git-credential`, draws the account rule on every fetch and push.
//! That rule stops an agent reading the token itself; in a fetch or push,
//! Git starts the helper and reads the token over a pipe. On two closed
//! command forms only, the account finding is skipped for that exact helper
//! body. Every other check on that body, and every other line, stays as it
//! is.
//!
//! The forms:
//!
//! - fetch: `git [-C <path>] fetch [-q|--quiet|-p|--prune] [origin [<branch>...]]`;
//! - push: `git [-C <path>] push [-u|--set-upstream] [-q] origin <src>:refs/heads/<name>`,
//!   where `<src>` is `HEAD`, a full object id or a local branch, and
//!   `git push [-u] origin HEAD` or `git push origin <name>` when `<name>` is
//!   the current branch.
//!
//! Each is a closed form over the line as written, like
//! [`super::ref_authority::recovery_fetch`]: the line holds only plain word
//! characters, blanks and, for a push, `:`, so a quote, an expansion, an
//! assignment, a launcher, a redirection or a second command fails it. The
//! repository the line names must have exactly one remote, `origin`, with
//! one URL that no rewrite changes, on a transport whose programs are
//! accounted for (`https://` or a local absolute path), and none of the
//! push, mirror or submodule settings that would widen what the command
//! does. A pushed name must sit under a work-branch prefix of the landed
//! policy, read at `origin`'s default branch, and must not be protected, an integration
//! line, a release branch, the ID registry, `origin`'s default branch,
//! `main`, `master` or a name `git.agent_push_refused_branches` lists.
//! git-guard and exec-guard refuse a push of that list's names and of the
//! default branch whatever credential Git uses
//! ([`super::git_guard::agent_push_refusals`]).
//!
//! Every program the call starts must be a credential helper, and the
//! helpers Git selects for `origin`'s URL must be the exempt helper alone.
//! The selection is modelled as Git's credential configuration makes it:
//! matching helper entries apply in configuration order, and an empty value
//! clears those before it, so a system `osxkeychain` that a later empty
//! `helper =` clears does not count. Where the model cannot read the
//! selection, every helper the call may start must be the exempt one. Any
//! read that fails withholds the admission, and the account finding stands.

use std::path::Path;

use super::git_guard::{GitRunnerBody, HelperKind};
use super::policy::GitPolicy;
use super::Violation;

/// Settings that widen what a fetch or push does, or where it goes. Any
/// value withholds the admission.
const WIDENING_SETTINGS: &[&str] = &[
    "remote.origin.pushurl",
    "remote.origin.push",
    "remote.origin.mirror",
    "remote.origin.vcs",
    "push.pushOption",
    "push.recurseSubmodules",
    "push.gpgSign",
    "fetch.recurseSubmodules",
    "submodule.recurse",
];

/// Whether `finding` is the account rule's finding, the one finding the
/// admission skips.
#[must_use]
pub(crate) fn is_account_finding(finding: &Violation) -> bool {
    finding.rule == "security.outward_actions" && finding.message.starts_with("account action:")
}

/// Whether `body` is the GitHub CLI credential helper as Git runs it:
/// a credential-helper value Git runs as written, exactly
/// `gh auth git-credential`, with `gh` named bare or by an absolute path
/// whose last component is `gh`.
#[must_use]
pub(crate) fn exempt_body(body: &GitRunnerBody) -> bool {
    body.helper == HelperKind::AsWritten && exempt_command(&body.command)
}

/// Whether `command` is `gh auth git-credential`, with `gh` named bare or
/// by a plain absolute path whose last component is `gh`.
fn exempt_command(command: &str) -> bool {
    command
        .strip_suffix(" auth git-credential")
        .is_some_and(|program| {
            program == "gh"
                || (program.starts_with('/')
                    && program.ends_with("/gh")
                    && program
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c)))
        })
}

/// Whether the configured helper `value` is the exempt helper as Git runs
/// it: a leading `!` or an absolute path, then [`exempt_command`].
fn exempt_value(value: &str) -> bool {
    match value.strip_prefix('!') {
        Some(command) => exempt_command(command),
        None => value.starts_with('/') && exempt_command(value),
    }
}

/// Whether `command`, run in `cwd`, is an admitted fetch or push whose Git
/// runner `bodies` are all credential helpers and whose effective helper
/// list for `origin` is the exempt helper alone.
#[must_use]
pub(crate) fn admits(command: &str, cwd: &Path, bodies: &[GitRunnerBody]) -> bool {
    if !bodies.iter().any(exempt_body)
        || bodies
            .iter()
            .any(|body| body.helper == HelperKind::NotHelper)
    {
        return false;
    }
    let Some(line) = line(command) else {
        return false;
    };
    let target = line
        .directory
        .map_or_else(|| cwd.to_path_buf(), |dir| cwd.join(dir));
    repository_allows(&target, &line.form, bodies).is_some()
}

enum Form<'a> {
    /// A fetch; `named` when the line names `origin`.
    Fetch { named: bool },
    /// A push of `src`; `name` is the destination branch when the line
    /// spells `<src>:refs/heads/<name>`, and `None` for the short forms.
    Push { src: &'a str, name: Option<&'a str> },
}

struct Line<'a> {
    directory: Option<&'a str>,
    form: Form<'a>,
}

/// The line read as one of the closed forms.
fn line(command: &str) -> Option<Line<'_>> {
    let plain = |c: char| c.is_ascii_alphanumeric() || " \t._/-:".contains(c);
    if !command.chars().all(plain) {
        return None;
    }
    let words: Vec<&str> = command
        .split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .collect();
    let (first, rest) = words.split_first()?;
    if *first != "git" {
        return None;
    }
    let (directory, rest) = match rest {
        ["-C", path, rest @ ..] if !path.starts_with('-') && !path.contains(':') => {
            (Some(*path), rest)
        }
        _ => (None, rest),
    };
    let (verb, args) = rest.split_first()?;
    let (options, operands): (Vec<&str>, Vec<&str>) =
        args.iter().partition(|word| word.starts_with('-'));
    let form = match *verb {
        "fetch" => {
            if command.contains(':')
                || !options
                    .iter()
                    .all(|option| matches!(*option, "-q" | "--quiet" | "-p" | "--prune"))
            {
                return None;
            }
            match operands.split_first() {
                None => Form::Fetch { named: false },
                Some((remote, branches)) => {
                    if *remote != "origin"
                        || !branches
                            .iter()
                            .all(|branch| *branch != "tag" && branch_name(branch))
                    {
                        return None;
                    }
                    Form::Fetch { named: true }
                }
            }
        }
        "push" => {
            if !options
                .iter()
                .all(|option| matches!(*option, "-u" | "--set-upstream" | "-q"))
            {
                return None;
            }
            let [remote, refspec] = operands.as_slice() else {
                return None;
            };
            if *remote != "origin" {
                return None;
            }
            match refspec.split_once(':') {
                Some((src, destination)) => {
                    let name = destination.strip_prefix("refs/heads/")?;
                    if src.is_empty() || name.contains(':') {
                        return None;
                    }
                    Form::Push {
                        src,
                        name: Some(name),
                    }
                }
                None => Form::Push {
                    src: refspec,
                    name: None,
                },
            }
        }
        _ => return None,
    };
    Some(Line { directory, form })
}

/// Whether `name` is a valid branch name.
fn branch_name(name: &str) -> bool {
    !name.starts_with('-') && git2::Reference::is_valid_name(&format!("refs/heads/{name}"))
}

/// `Some` when the configuration read for `origin`'s `url` selects only the
/// exempt helper, sets no widening setting and maps fetches only under
/// `refs/remotes/origin/`; `None` otherwise.
fn config_allows(
    config: &crate::security::git_config::EffectiveConfig,
    url: &str,
    bodies: &[GitRunnerBody],
) -> Option<()> {
    // Git runs the helpers its credential configuration selects for
    // `origin`'s URL. Where the model reads that list, it must be the exempt
    // helper alone; where it cannot, every helper body must be exempt.
    match effective_helpers(config, url) {
        Some(helpers) => {
            if !matches!(helpers.as_slice(), [only] if exempt_value(only)) {
                return None;
            }
        }
        None => {
            if !bodies.iter().all(exempt_body) {
                return None;
            }
        }
    }
    if WIDENING_SETTINGS
        .iter()
        .any(|key| !config.get_all(key).is_empty())
    {
        return None;
    }
    for (_, _, mapping) in config.get_all("remote.origin.fetch") {
        let mapping = std::str::from_utf8(mapping).ok()?;
        let mapping = mapping.strip_prefix('+').unwrap_or(mapping);
        if mapping.starts_with('^') {
            continue;
        }
        if let Some((_, destination)) = mapping.split_once(':') {
            if !destination.starts_with("refs/remotes/origin/") {
                return None;
            }
        }
    }
    Some(())
}

/// `Some` when the repository at `target` lets the form through; `None` on
/// any refusal or read failure.
fn repository_allows(target: &Path, form: &Form<'_>, bodies: &[GitRunnerBody]) -> Option<()> {
    let repo = super::repo::open(target).ok()??;
    let workdir = repo.workdir()?.to_path_buf();
    let remotes = repo.remotes().ok()?;
    let names: Vec<&[u8]> = remotes.iter_bytes().collect();
    if names != [b"origin".as_slice()] {
        return None;
    }
    let config = crate::security::git_config::read(
        target,
        &[],
        &crate::security::git_config::Environment::new(),
    )
    .ok()?;
    let urls = config.get_all("remote.origin.url");
    let [(_, _, url)] = urls.as_slice() else {
        return None;
    };
    let url = std::str::from_utf8(url).ok()?;
    if !url_allowed(url) {
        return None;
    }
    config_allows(&config, url, bodies)?;
    unrewritten(target, url)?;
    // Only a `.gitmodules` proven absent lets the form through; an entry of
    // any kind, a dangling link or a read error withholds the admission.
    match crate::absence::symlink_metadata_optional(&workdir.join(".gitmodules")) {
        Ok(None) => {}
        _ => return None,
    }
    let landed = super::landed_policy::load_local(target).ok()?;
    let authority = landed.authority.as_ref()?;
    if !landed.remote_tracked || !authority.reference.starts_with("refs/remotes/origin/") {
        return None;
    }
    let current = current_branch(&repo);
    match form {
        Form::Fetch { named: true } => Some(()),
        Form::Fetch { named: false } => {
            // A bare fetch goes to the current branch's remote, which must
            // be `origin` when one is set.
            if let Head::Branch(current) = current? {
                let key = format!("branch.{current}.remote");
                if let Some((_, _, remote)) = config.get_all(&key).last() {
                    if *remote != b"origin" {
                        return None;
                    }
                }
            }
            Some(())
        }
        Form::Push { src, name } => {
            let name = if let Some(name) = name {
                source_allowed(&repo, src)?;
                (*name).to_string()
            } else {
                let Some(Head::Branch(current)) = current else {
                    return None;
                };
                let name = if *src == "HEAD" {
                    current.clone()
                } else {
                    (*src).to_string()
                };
                if name != current {
                    return None;
                }
                let resolved = repo.resolve_reference_from_short_name(&name).ok()?;
                if !names_branch(&resolved, &name) {
                    return None;
                }
                name
            };
            // `origin`'s default branch, whatever its name, is never a
            // work branch.
            let default = authority.reference.strip_prefix("refs/remotes/origin/")?;
            if name.eq_ignore_ascii_case(default)
                || !name_admitted(&landed.policy.git, &name)
                || tag_exists(&repo, &name)?
            {
                return None;
            }
            Some(())
        }
    }
}

/// Whether Git may be given `url` for the admission: only a transport whose
/// programs are accounted for. That is an `https://` URL, which Git serves
/// with the HTTPS helper it ships, or a local absolute path, which Git
/// serves with its own upload and receive programs (a configured
/// replacement for either is a runner body that is not a credential helper,
/// which withholds the admission). Every other URL refuses: Git starts a
/// remote helper program named after any other scheme, a scheme spelled in
/// another case or a `<transport>::<address>` form; SSH starts `ssh` with
/// its own configuration; and plain-text HTTP would send the token in the
/// clear.
fn url_allowed(url: &str) -> bool {
    if url.contains("::") {
        return false;
    }
    match url.strip_prefix("https://") {
        Some(rest) => !rest.is_empty(),
        None => !url.contains("://") && local_absolute_path(url),
    }
}

/// Whether Git reads `url` as a local absolute path: a leading `/`, or on
/// Windows a drive letter, `:` and a separator. Elsewhere Git reads
/// `C:/x` as an SSH address, so a drive path is local only on Windows.
fn local_absolute_path(url: &str) -> bool {
    if url.starts_with('/') {
        return true;
    }
    let bytes = url.as_bytes();
    cfg!(windows)
        && bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// The credential helpers Git runs for `url`, in order, as Git's credential
/// configuration selects them: every `credential.helper` value, and every
/// `credential.<pattern>.helper` value whose pattern matches `url`, applied
/// in configuration order, where an empty value clears the helpers selected
/// before it. `None` when the model cannot read the list exactly: `url` is
/// not a plain `https://<host>/<path>` URL, a helper key's pattern is not a
/// plain `https://<host>` or `https://<host>/`, or a helper key or value is
/// unreadable.
fn effective_helpers(
    config: &crate::security::git_config::EffectiveConfig,
    url: &str,
) -> Option<Vec<String>> {
    let host = https_host(url, false)?;
    let mut helpers = Vec::new();
    for (key, value) in config.entries() {
        // Section and variable names arrive folded to lower case.
        let Some(rest) = key.strip_prefix(b"credential.") else {
            continue;
        };
        let applies = if rest == b"helper" {
            true
        } else if let Some(pattern) = rest.strip_suffix(b".helper") {
            let pattern = std::str::from_utf8(pattern).ok()?;
            https_host(pattern, true)?.eq_ignore_ascii_case(host)
        } else {
            continue;
        };
        if !applies {
            continue;
        }
        // A helper key without `=` is an error to Git; the model stops.
        let value = std::str::from_utf8(value?).ok()?;
        if value.is_empty() {
            helpers.clear();
        } else {
            helpers.push(value.to_string());
        }
    }
    Some(helpers)
}

/// The host of a plain `https://<host>` URL: no user, port, query or
/// fragment, and a host of ASCII letters, digits, `.` and `-`. With
/// `pattern`, the URL may end at the host or at one `/`; otherwise it must
/// carry a path after the host.
fn https_host(url: &str, pattern: bool) -> Option<&str> {
    let rest = url
        .get(..8)?
        .eq_ignore_ascii_case("https://")
        .then(|| &url[8..])?;
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, Some(path)),
        None => (rest, None),
    };
    let plain_host = !host.is_empty()
        && !host.starts_with(['.', '-'])
        && !host.ends_with('.')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    let path_ok = if pattern {
        path.is_none_or(str::is_empty)
    } else {
        path.is_some_and(|path| {
            !path.is_empty()
                && path
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
        })
    };
    (plain_host && path_ok).then_some(host)
}

/// `Some` when Git's own fetch and push URLs for `origin` are the
/// configured `url`: no `insteadOf` or `pushInsteadOf` rewrite, at any
/// scope, changes it.
fn unrewritten(target: &Path, url: &str) -> Option<()> {
    for args in [
        &["remote", "get-url", "origin"][..],
        &["remote", "get-url", "--push", "origin"][..],
    ] {
        let output = crate::git::command()
            .current_dir(target)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = std::str::from_utf8(&output.stdout).ok()?;
        if text.strip_suffix('\n').unwrap_or(text) != url {
            return None;
        }
    }
    Some(())
}

/// Where `HEAD` points, as the admission reads it.
enum Head {
    /// A branch, by its name under `refs/heads/`.
    Branch(String),
    /// A detached `HEAD`.
    Detached,
}

/// Where `HEAD` points, or `None` when `HEAD` is unborn or unreadable or
/// names a branch whose name is not UTF-8, which the forms never admit.
fn current_branch(repo: &git2::Repository) -> Option<Head> {
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return Some(Head::Detached);
    }
    let name = crate::git::name::reference_name(&head);
    let name = name.rule_text().ok()?.strip_prefix("refs/heads/")?;
    Some(Head::Branch(name.to_string()))
}

/// Whether `reference`'s full name is `refs/heads/<name>`, compared as bytes.
fn names_branch(reference: &git2::Reference<'_>, name: &str) -> bool {
    crate::git::name::reference_name(reference).bytes() == format!("refs/heads/{name}").as_bytes()
}

/// `Some` when the push source is `HEAD`, a full object id, or a local
/// branch that Git's short-name lookup resolves to that branch and no tag
/// shares.
fn source_allowed(repo: &git2::Repository, src: &str) -> Option<()> {
    if src == "HEAD" {
        repo.head().ok()?;
        return Some(());
    }
    if matches!(src.len(), 40 | 64) && src.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(());
    }
    if !branch_name(src) || src.starts_with("refs/") {
        return None;
    }
    let resolved = repo.resolve_reference_from_short_name(src).ok()?;
    if !names_branch(&resolved, src) || tag_exists(repo, src)? {
        return None;
    }
    Some(())
}

/// Whether a local tag has `name`; `None` when the tag ref cannot be read.
fn tag_exists(repo: &git2::Repository, name: &str) -> Option<bool> {
    match repo.find_reference(&format!("refs/tags/{name}")) {
        Ok(_) => Some(true),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Some(false),
        Err(_) => None,
    }
}

/// Whether the landed policy admits pushing `name`: it sits under a
/// work-branch prefix, and is not protected, a branch agents never push
/// (`git.agent_push_refused_branches` and its floor), an integration line,
/// a release branch or the ID registry.
fn name_admitted(git: &GitPolicy, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let registry = crate::ids::REGISTRY_BRANCH;
    let pattern = git
        .release_branch_pattern
        .as_deref()
        .unwrap_or(crate::workgraph::release_line::BUILTIN_PATTERN);
    let Ok(release) = glob::Pattern::new(pattern) else {
        return false;
    };
    name.is_ascii()
        && branch_name(name)
        && git.branch_prefixes.iter().any(|prefix| {
            prefix.len() > 1
                && prefix.ends_with('/')
                && name.len() > prefix.len()
                && name.starts_with(prefix.as_str())
        })
        && !git.branch_is_protected_any_case(name)
        && !git.branch_folds_onto_protected(name)
        && git.agent_push_refusal(name).is_none()
        && !matches!(lower.as_str(), "main" | "master" | "integration")
        && !lower.starts_with("integration/")
        && lower != registry
        && !lower.starts_with(&format!("{registry}/"))
        && !release.matches(name)
        && !release.matches(&lower)
}
