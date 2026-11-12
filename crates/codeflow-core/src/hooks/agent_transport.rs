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
//! one URL that no rewrite changes, and none of the push, mirror or
//! submodule settings that would widen what the command does. A pushed name
//! must sit under a work-branch prefix of the landed policy, read at
//! `origin`'s default branch, and must not be protected, an integration
//! line, a release branch, the ID registry, `origin`'s default branch,
//! `main` or `master`. Any read
//! that fails withholds the admission, and the account finding stands.

use std::path::Path;

use super::git_guard::GitRunnerBody;
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
    body.credential_helper
        && body
            .command
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

/// Whether `command`, run in `cwd`, is an admitted fetch or push whose Git
/// runner `bodies` are all the exempt helper.
#[must_use]
pub(crate) fn admits(command: &str, cwd: &Path, bodies: &[GitRunnerBody]) -> bool {
    if bodies.is_empty() || !bodies.iter().all(exempt_body) {
        return false;
    }
    let Some(line) = line(command) else {
        return false;
    };
    let target = line
        .directory
        .map_or_else(|| cwd.to_path_buf(), |dir| cwd.join(dir));
    repository_allows(&target, &line.form).is_some()
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

/// `Some` when the repository at `target` lets the form through; `None` on
/// any refusal or read failure.
fn repository_allows(target: &Path, form: &Form<'_>) -> Option<()> {
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
    unrewritten(target, url)?;
    match std::fs::symlink_metadata(workdir.join(".gitmodules")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
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
            if let Some(current) = current {
                let key = format!("branch.{current}.remote");
                if let Some((_, _, remote)) = config.get_all(&key).last() {
                    if *remote != b"origin" {
                        return None;
                    }
                }
            } else {
                repo.head().ok()?;
            }
            Some(())
        }
        Form::Push { src, name } => {
            let name = match name {
                Some(name) => {
                    source_allowed(&repo, src)?;
                    (*name).to_string()
                }
                None => {
                    let current = current?;
                    let name = if *src == "HEAD" {
                        current.clone()
                    } else {
                        (*src).to_string()
                    };
                    if name != current {
                        return None;
                    }
                    let resolved = repo.resolve_reference_from_short_name(&name).ok()?;
                    if resolved.name().ok()? != format!("refs/heads/{name}") {
                        return None;
                    }
                    name
                }
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

/// Whether Git may be given `url` for the admission: not plain-text HTTP,
/// which would send the token in the clear, and no remote-helper form
/// (`<transport>::<address>`), which runs another program.
fn url_allowed(url: &str) -> bool {
    !url.is_empty()
        && !url.contains("::")
        && !url.starts_with('-')
        && !url
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
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

/// The current branch's name, or `None` when `HEAD` is detached, unborn
/// or unreadable.
fn current_branch(repo: &git2::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }
    head.name()
        .ok()?
        .strip_prefix("refs/heads/")
        .map(str::to_string)
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
    if resolved.name().ok()? != format!("refs/heads/{src}") || tag_exists(repo, src)? {
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
/// work-branch prefix, and is not protected, an integration line, a
/// release branch, the ID registry, `main` or `master`.
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
        && !matches!(lower.as_str(), "main" | "master" | "integration")
        && !lower.starts_with("integration/")
        && lower != registry
        && !lower.starts_with(&format!("{registry}/"))
        && !release.matches(name)
        && !release.matches(&lower)
}
