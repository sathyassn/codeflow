//! The repository a Git call reads (TSK-1002 Fable review 13).
//!
//! A `--git-dir`, `--work-tree` or `GIT_DIR` the line sets can move the
//! work tree onto a store, so `git --work-tree=~/.ssh add -A` stages the
//! store and `git diff --cached` prints it with no secret name on the line.
//! Such a setting is accepted only when it names exactly the repository Git
//! would use without it.
//!
//! What content already committed to Git's object store holds (an object
//! named by id, a batch record, an archive, or a commit's history and
//! patches) is outside the text guard's promise by the operator's
//! direction of 2026-10-29: secrets already in history are handled once,
//! by a full-history secret scan, and the commit-time and hosted secret
//! scans stop new ones. Every check on the paths a command names, and on
//! writing working-tree files into the store, stays.

use crate::security::outward;
use std::path::{Path, PathBuf};

/// Why a work tree or Git directory set on the line refuses.
pub(super) const MOVED: &str = "cannot establish that a Git directory or work tree the line sets is the repository Git would use without it; run Git in the repository without --git-dir, --work-tree or GIT_DIR";

/// The index of the first word after the Git subcommand of `call`.
fn subcommand_end(call: &outward::Unwrapped) -> Option<usize> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let (_, tail) = crate::hooks::git_guard::git_subcommand(&git)?;
    Some(call.args.len() - tail.len())
}

/// The directory a Git call runs in after its `-C` options, each taken
/// from the one before as Git does (TSK-1002 Fable review 15): `None` when
/// `call` is not Git or sets no `-C`, `Some(None)` when a `-C` value cannot
/// be established. A relative operand of the call is read from there.
#[allow(clippy::option_option)] // Not moved, moved to a known directory, or moved to an unknown one.
pub(crate) fn call_directory(
    call: &outward::Unwrapped,
    home: Option<&Path>,
) -> Option<Option<PathBuf>> {
    let (program, rest) = call.args.split_first()?;
    let git = crate::hooks::git_guard::git_arguments(program, rest)?;
    let end = crate::hooks::git_guard::git_subcommand(&git)
        .map_or(git.len(), |(_, tail)| git.len() - tail.len() - 1);
    let mut directory = call.cwd.clone();
    let mut moved = false;
    let mut words = git[..end].iter();
    while let Some(word) = words.next() {
        let location = match word.as_str() {
            "-C" => words.next().map(String::as_str),
            option if crate::security::git::GLOBAL_VALUE_OPTIONS.contains(&option) => {
                words.next();
                continue;
            }
            // Git takes `-C` with a separate value only; an attached one is
            // read as a directory all the same.
            option => match option.strip_prefix("-C") {
                Some(location) => Some(location),
                None => continue,
            },
        };
        moved = true;
        directory = match location {
            None => None,
            // Git skips an empty `-C ""`.
            Some("") => directory,
            Some(location) => joined(directory.as_deref(), location, home),
        };
    }
    moved.then_some(directory)
}

/// `location` read from `base` as `chdir` reads it, or `None` when the
/// guard cannot establish it: a value the shell fills in, a pattern, a
/// `~user` spelling, or a relative one from an unknown directory.
fn joined(base: Option<&Path>, location: &str, home: Option<&Path>) -> Option<PathBuf> {
    if crate::hooks::git_guard::unresolved_word(location)
        || location.contains(['\u{1}', '\u{2}'])
        || crate::hooks::git_guard::has_glob(location)
    {
        return None;
    }
    let tilde = match location.strip_prefix('~') {
        None => None,
        Some("") => Some(home?.to_path_buf()),
        Some(rest) => match rest.strip_prefix(['/', '\\']) {
            Some(rest) => Some(home?.join(rest)),
            None => return None,
        },
    };
    if let Some(path) = tilde {
        return Some(path);
    }
    let path = Path::new(location);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    base.map(|base| base.join(path))
}

/// Why `call` refuses for a Git directory or work tree it sets
/// (`--git-dir`, `--work-tree`, `GIT_DIR` on the line), or `None` when it
/// sets neither. Git itself reports the Git directory and work tree with
/// and without them, from the directory the call runs in; they must be the
/// same, so the setting changes nothing. With a Git directory alone, Git
/// takes the current directory as the work tree, and a `.git` file is read
/// as a gitfile, as Git does. Anything the guard cannot establish refuses.
pub(super) fn moved_repository(call: &outward::Unwrapped, command: &str) -> Option<&'static str> {
    let start = subcommand_end(call)?;
    let globals = if start >= 2 {
        &call.args[1..start - 1]
    } else {
        &[][..]
    };
    let mut kept = Vec::new();
    let mut moved: Vec<Option<String>> = crate::security::git_preflight::git_dir_values(command)
        .into_iter()
        .map(|value| value.map(|value| format!("--git-dir={value}")))
        .collect();
    let mut at = 0;
    while at < globals.len() {
        let word = globals[at].as_str();
        let next = globals.get(at + 1);
        match word {
            // The call's directory already holds its `-C` options.
            "-C" => at += 2,
            "-c" => {
                kept.push(word.to_string());
                kept.extend(next.cloned());
                at += 2;
            }
            "--git-dir" | "--work-tree" => {
                moved.push(next.map(|value| format!("{word}={value}")));
                at += 2;
            }
            _ => {
                if word.starts_with("--git-dir=") || word.starts_with("--work-tree=") {
                    moved.push(Some(word.to_string()));
                }
                at += 1;
            }
        }
    }
    if moved.is_empty() {
        return None;
    }
    let Some(cwd) = call.cwd.as_deref() else {
        return Some(MOVED);
    };
    let Some(moved) = moved
        .into_iter()
        .map(|value| value.filter(|value| !value.contains(['$', '`', '~'])))
        .collect::<Option<Vec<String>>>()
    else {
        return Some(MOVED);
    };
    let mut with = kept.clone();
    with.extend(moved);
    let found = |globals: &[String]| match crate::security::git_config::repository_of(cwd, globals)
    {
        Ok(Some((dir, top))) => crate::portable_path::canonicalize(&dir)
            .ok()
            .zip(crate::portable_path::canonicalize(&top).ok()),
        _ => None,
    };
    match (found(&kept), found(&with)) {
        (Some(without), Some(set)) if without == set => None,
        _ => Some(MOVED),
    }
}
