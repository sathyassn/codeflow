//! The working-tree files a Git call writes into the object store without
//! naming them (TSK-1002 Fable review 15).
//!
//! `git add id_rsa` names the file it stages, and the path check refuses
//! it. `git add .`, `git add -A`, `git add <dir>`, `git commit -a`, a
//! commit pathspec and `git stash -u` stage every file under their
//! pathspecs, after which `git diff --cached` or `git show stash@{0}^3`
//! prints a secret with no name on the line. Writing working-tree files
//! into the store stays inside the guard's promise under the operator's
//! committed-content decision of 2026-10-29, so each such call is judged by
//! the files Git would stage: Git itself lists them (`git ls-files`, the
//! untracked files it does not ignore, every untracked file under `-f` or
//! `stash -a`, and the tracked files with changes), and each is judged as a
//! named path is. A list the guard cannot read, a pathspec it cannot
//! resolve, or a pathspec file it does not see refuses as cannot-verify.

use crate::security::outward;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Which files a staging call takes under its pathspecs.
#[derive(Debug, Default, PartialEq, Eq)]
struct Staging {
    /// Untracked files.
    untracked: bool,
    /// Untracked files Git ignores too (`add -f`, `stash -a`).
    ignored: bool,
    pathspecs: Vec<String>,
}

/// The files `call` stages, by kind and pathspec, or `None` when it stages
/// none it does not name.
#[allow(clippy::too_many_lines)] // One reading per staging subcommand, in Git's own option order.
fn staging(call: &outward::Unwrapped) -> Result<Option<Staging>, &'static str> {
    let Some((program, rest)) = call.args.split_first() else {
        return Ok(None);
    };
    let Some(git) = crate::hooks::git_guard::git_arguments(program, rest) else {
        return Ok(None);
    };
    let Some((sub, tail)) = crate::hooks::git_guard::git_subcommand(&git) else {
        return Ok(None);
    };
    let (valued, mut staging, whole): (&[&str], Staging, bool) = match sub {
        "add" => {
            let all = tail.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "-A" | "--all" | "-u" | "--update" | "--no-ignore-removal"
                )
            });
            if tail
                .iter()
                .any(|word| matches!(word.as_str(), "-n" | "--dry-run" | "--refresh"))
            {
                return Ok(None);
            }
            let update = tail
                .iter()
                .any(|word| matches!(word.as_str(), "-u" | "--update"));
            let patch = tail.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "-p" | "--patch" | "-i" | "--interactive" | "-e" | "--edit"
                )
            });
            let force = tail
                .iter()
                .any(|word| matches!(word.as_str(), "-f" | "--force"));
            (
                &["--chmod"],
                Staging {
                    untracked: !update && !patch,
                    ignored: force,
                    pathspecs: Vec::new(),
                },
                all,
            )
        }
        "commit" => (
            &[
                "-m",
                "-F",
                "-C",
                "-c",
                "-t",
                "--message",
                "--file",
                "--reuse-message",
                "--reedit-message",
                "--template",
                "--author",
                "--date",
                "--cleanup",
                "--fixup",
                "--squash",
                "--trailer",
            ],
            Staging::default(),
            tail.iter().any(|word| {
                word == "--all"
                    || word.starts_with('-')
                        && !word.starts_with("--")
                        && word[1..]
                            .chars()
                            .take_while(|ch| !"mFCct".contains(*ch))
                            .any(|ch| ch == 'a')
            }),
        ),
        "stash" => {
            let first = tail.first().map(String::as_str);
            if first.is_some_and(|word| !word.starts_with('-') && !matches!(word, "push" | "save"))
            {
                return Ok(None);
            }
            let all = tail
                .iter()
                .any(|word| matches!(word.as_str(), "-a" | "--all"));
            let untracked = all
                || tail
                    .iter()
                    .any(|word| matches!(word.as_str(), "-u" | "--include-untracked"));
            // `git stash save` takes a message, not a pathspec.
            if first == Some("save") {
                return Ok(Some(Staging {
                    untracked,
                    ignored: all,
                    pathspecs: vec![":/".into()],
                }));
            }
            (
                &["-m", "--message"],
                Staging {
                    untracked,
                    ignored: all,
                    pathspecs: Vec::new(),
                },
                true,
            )
        }
        _ => return Ok(None),
    };
    let mut words = tail.iter().skip(usize::from(
        sub == "stash" && tail.first().is_some_and(|w| w == "push"),
    ));
    while let Some(word) = words.next() {
        if word == "--" {
            staging.pathspecs.extend(words.by_ref().cloned());
            break;
        }
        if word.starts_with("--pathspec-from-file") || word.starts_with("--pathspec-f") {
            let input = call
                .input
                .as_deref()
                .ok_or("cannot establish the pathspecs a Git call reads from a file")?;
            staging.pathspecs.extend(
                input
                    .split(['\n', '\0'])
                    .filter(|line| !line.is_empty())
                    .map(str::to_string),
            );
            continue;
        }
        if valued.contains(&word.as_str()) {
            words.next();
            continue;
        }
        if word.starts_with('-') {
            // A short cluster ends at a letter that takes a value
            // (`-am MESSAGE`); an attached value is part of the word.
            let letters = word.strip_prefix('-').filter(|rest| !rest.starts_with('-'));
            if sub == "commit"
                && letters.is_some_and(|letters| {
                    letters.char_indices().find(|(_, ch)| "mFCct".contains(*ch))
                        == letters.char_indices().last()
                        && letters.ends_with(|ch: char| "mFCct".contains(ch))
                })
            {
                words.next();
            }
            continue;
        }
        staging.pathspecs.push(word.clone());
    }
    if staging.pathspecs.is_empty() {
        if !whole {
            return Ok(None);
        }
        staging.pathspecs.push(":/".into());
    }
    Ok(Some(staging))
}

/// Whether `call` stages a file `judge` finds secret without naming it:
/// `None` when it stages none it does not name.
pub(super) fn staged_secret(
    call: &outward::Unwrapped,
    home: Option<&Path>,
    judge: &dyn Fn(&str) -> Result<bool, &'static str>,
) -> Option<Result<bool, &'static str>> {
    let staging = match staging(call) {
        Ok(Some(staging)) => staging,
        Ok(None) => return None,
        Err(why) => return Some(Err(why)),
    };
    Some(listed_secret(call, &staging, home, judge))
}

/// A pathspec with the home a leading `~` names, as the shell expands it.
fn expanded(pathspec: &str, home: Option<&Path>) -> Result<String, &'static str> {
    const UNREAD: &str = "cannot establish the home a pathspec names";
    match pathspec.strip_prefix('~') {
        None => Ok(pathspec.to_string()),
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = home.and_then(Path::to_str).ok_or(UNREAD)?;
            Ok(format!("{home}{rest}"))
        }
        Some(_) => Err(UNREAD),
    }
}

/// `path` with its longest existing ancestor resolved through links, as
/// Git resolves an absolute pathspec against its work tree.
fn resolved(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return Some(rest.iter().rev().fold(real, |path, part| path.join(part)));
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    }
}

fn listed_secret(
    call: &outward::Unwrapped,
    staging: &Staging,
    home: Option<&Path>,
    judge: &dyn Fn(&str) -> Result<bool, &'static str>,
) -> Result<bool, &'static str> {
    const UNREAD: &str = "cannot list the files the Git call stages";
    if staging
        .pathspecs
        .iter()
        .any(|pathspec| crate::hooks::git_guard::unresolved_word(pathspec))
    {
        return Err("cannot establish a pathspec of the files the Git call stages");
    }
    let directory = call
        .cwd
        .as_deref()
        .ok_or("cannot establish the directory the Git call stages files in")?;
    let pathspecs = staging
        .pathspecs
        .iter()
        .map(|pathspec| expanded(pathspec, home))
        .collect::<Result<Vec<_>, _>>()?;
    // An absolute pathspec outside the work tree fails the whole Git call,
    // which then stages nothing.
    if pathspecs
        .iter()
        .any(|pathspec| Path::new(pathspec).is_absolute())
    {
        let top = crate::security::git_config::repository_of(directory, &[])
            .map_err(|_| UNREAD)?
            .and_then(|(_, top)| resolved(&top))
            .ok_or(UNREAD)?;
        if pathspecs.iter().any(|pathspec| {
            Path::new(pathspec).is_absolute()
                && resolved(Path::new(pathspec)).is_none_or(|path| !path.starts_with(&top))
        }) {
            return Ok(false);
        }
    }
    let mut kinds = vec![vec!["--modified"]];
    if staging.untracked {
        kinds.push(if staging.ignored {
            vec!["--others"]
        } else {
            vec!["--others", "--exclude-standard"]
        });
    }
    for kind in kinds {
        let mut command = crate::git::command();
        command
            .current_dir(directory)
            .stdin(Stdio::null())
            .args(["--no-pager", "ls-files", "-z"])
            .args(kind)
            .arg("--")
            .args(&pathspecs);
        let deadline = Instant::now() + Duration::from_secs(3);
        let bytes = crate::security::git_config::bounded_output(&mut command, deadline)
            .map_err(|_| UNREAD)?;
        for path in bytes
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let path = std::str::from_utf8(path).map_err(|_| UNREAD)?;
            if judge(path)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &str) -> Option<Staging> {
        let call = outward::Unwrapped {
            args: crate::hooks::git_guard::command_argv(line),
            input: None,
            stdin_bound: false,
            unresolved: Vec::new(),
            supplier: None,
            expands: None,
            cwd: Some(std::env::temp_dir()),
            links: Vec::new(),
        };
        staging(&call).unwrap()
    }

    #[test]
    fn staging_calls_are_read_by_their_pathspecs() {
        let whole = |untracked, ignored| Staging {
            untracked,
            ignored,
            pathspecs: vec![":/".into()],
        };
        assert_eq!(read("git add -A"), Some(whole(true, false)));
        assert_eq!(read("git add -u"), Some(whole(false, false)));
        assert_eq!(read("git add -f -A"), Some(whole(true, true)));
        assert_eq!(read("git commit -a -m x"), Some(whole(false, false)));
        assert_eq!(read("git commit -am x"), Some(whole(false, false)));
        assert_eq!(read("git stash -u"), Some(whole(true, false)));
        assert_eq!(read("git stash push --all"), Some(whole(true, true)));
        assert_eq!(read("git stash"), Some(whole(false, false)));
        let named = |untracked, path: &str| Staging {
            untracked,
            ignored: false,
            pathspecs: vec![path.into()],
        };
        assert_eq!(read("git add ."), Some(named(true, ".")));
        assert_eq!(read("git add copies"), Some(named(true, "copies")));
        assert_eq!(
            read("git commit -m 'msg' -- src"),
            Some(named(false, "src"))
        );
        assert_eq!(read("git commit -m msg src"), Some(named(false, "src")));
        for line in [
            "git add",
            "git add -n .",
            "git commit -m msg",
            "git commit --amend --no-edit",
            "git stash pop",
            "git stash list",
            "git status",
        ] {
            assert_eq!(read(line), None, "{line}");
        }
    }
}
