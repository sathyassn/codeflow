//! Thin git plumbing for the scaffold engine — shells out to the `git`
//! binary; repository absence also uses the shared discovery proof. Repo detection,
//! `git init`, config get/set, staged adds, and the scaffold commit.

use std::path::Path;

use super::{ScaffoldError, Shown};
use crate::shown;

/// The git subcommand `args` runs, named only when it is one the scaffold
/// runs; never a path, key, value or message from the arguments.
fn subcommand(args: &[&str]) -> Shown {
    const KNOWN: [&str; 6] = ["add", "commit", "config", "diff", "init", "rev-parse"];
    let first = args
        .iter()
        .copied()
        .find(|arg| !arg.starts_with('-') && !arg.contains('='));
    KNOWN
        .into_iter()
        .find(|known| Some(*known) == first)
        .map_or_else(|| Shown::fixed("command"), Shown::fixed)
}

/// A failed git run, by its subcommand and exit code; git's own output is
/// not shown, since it can quote paths, keys and values.
fn failed(args: &[&str], out: &std::process::Output) -> ScaffoldError {
    let code = out
        .status
        .code()
        .and_then(|code| usize::try_from(code).ok())
        .map_or_else(
            || Shown::fixed("no exit code"),
            |code| shown!("exit code {}", Shown::count(code)),
        );
    ScaffoldError::Git(shown!("git {} failed ({})", subcommand(args), code))
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output, ScaffoldError> {
    git_env(root, args, &[])
}

fn git_env(
    root: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<std::process::Output, ScaffoldError> {
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .envs(envs.iter().copied())
        .output()
        .map_err(|e| {
            ScaffoldError::Git(shown!(
                "failed to run git {}: {}",
                subcommand(args),
                Shown::io(&e)
            ))
        })
}

fn git_ok(root: &Path, args: &[&str]) -> Result<(), ScaffoldError> {
    let out = git(root, args)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(failed(args, &out))
    }
}

// rev-parse has no distinct exit code for an absent repository. A failed
// invocation is absence only after both metadata discovery and Git's own
// configuration reader establish that there is no repository to inspect.
fn repository_absent(root: &Path) -> Result<bool, ScaffoldError> {
    if crate::hooks::repo::open(root)
        .map_err(|_| ScaffoldError::Git(Shown::fixed("cannot open the repository")))?
        .is_some()
    {
        return Ok(false);
    }
    git_ok(root, &["config", "--list"])?;
    Ok(true)
}

/// Is `root` inside a git work tree?
/// # Errors
/// Git cannot run, repository/configuration reads fail, or its answer is invalid.
pub fn is_repo(root: &Path) -> Result<bool, ScaffoldError> {
    let args = ["rev-parse", "--is-inside-work-tree"];
    let out = git(root, &args)?;
    if !out.status.success() {
        if out.status.code() == Some(128) && repository_absent(root)? {
            return Ok(false);
        }
        return Err(failed(&args, &out));
    }
    match out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout) {
        b"true" => Ok(true),
        b"false" => Ok(false),
        _ => Err(ScaffoldError::Git(Shown::fixed(
            "invalid work-tree answer from git",
        ))),
    }
}

/// Does the repo have at least one commit?
/// # Errors
/// Git cannot run or cannot resolve repository metadata.
pub fn has_commits(root: &Path) -> Result<bool, ScaffoldError> {
    let args = ["rev-parse", "--verify", "--quiet", "HEAD"];
    let out = git(root, &args)?;
    match out.status.code() {
        Some(0) => {
            let oid = out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout);
            if matches!(oid.len(), 40 | 64) && oid.iter().all(u8::is_ascii_hexdigit) {
                Ok(true)
            } else {
                Err(ScaffoldError::Git(Shown::fixed(
                    "invalid HEAD object id from git",
                )))
            }
        }
        Some(1) => Ok(false),
        _ => Err(failed(&args, &out)),
    }
}

/// The common git dir, as an absolute path. `None` outside a repository.
/// # Errors
/// Git cannot obtain a repository answer or its path cannot be represented.
pub fn common_dir(root: &Path) -> Result<Option<std::path::PathBuf>, ScaffoldError> {
    let args = ["rev-parse", "--git-common-dir"];
    let out = git(root, &args)?;
    if !out.status.success() {
        if out.status.code() == Some(128) && repository_absent(root)? {
            return Ok(None);
        }
        return Err(failed(&args, &out));
    }
    // Only Git's own newline is framing; preserve the name's exact bytes.
    let bytes = out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout);
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(ScaffoldError::Git(Shown::fixed(
            "invalid common directory from git",
        )));
    }
    let dir = crate::git::GitName::from_bytes(bytes)
        .os_path()
        .map_err(|_| {
            ScaffoldError::Git(Shown::fixed(
                "git gave a common directory that is not a path here",
            ))
        })?;
    Ok(Some(root.join(dir)))
}

pub fn init_repo(root: &Path) -> Result<(), ScaffoldError> {
    git_ok(root, &["init", "--quiet"])
}

/// `git config --get <key>` as a storage key (OS text rule, issue 79): a
/// value that is not valid UTF-8 keeps its exact bytes in the key, so it is
/// never read as unset and never equals a text value. Use it where an
/// existing setting decides what the scaffold may overwrite.
/// # Errors
/// Git cannot obtain the key or returns malformed output.
pub fn config_get_key(root: &Path, key: &str) -> Result<Option<String>, ScaffoldError> {
    // The sole absent-key status is 1; launch errors and every other failure
    // must not select the wiring/default identity branch.
    let args = ["config", "--null", "--get", key];
    let out = git(root, &args)?;
    match out.status.code() {
        Some(1) => Ok(None),
        Some(0) => {
            let bytes = out.stdout.strip_suffix(&[0]).ok_or_else(|| {
                ScaffoldError::Git(Shown::fixed("git config returned an unframed value"))
            })?;
            if bytes.contains(&0) {
                return Err(ScaffoldError::Git(Shown::fixed(
                    "git config returned multiple values",
                )));
            }
            Ok(Some(crate::git::GitName::from_bytes(bytes).storage_key()))
        }
        _ => Err(failed(&args, &out)),
    }
}

pub fn config_set(root: &Path, key: &str, value: &str) -> Result<(), ScaffoldError> {
    git_ok(root, &["config", key, value])
}

/// Stages `paths` (project-root relative) and commits with `message`.
/// Falls back to a scaffold identity when the machine has none configured
/// (bootstrap grace must work on a fresh machine).
pub fn add_and_commit(root: &Path, paths: &[String], message: &str) -> Result<(), ScaffoldError> {
    // Only stage paths that exist: with assets authored in a parallel
    // workstream, a scaffold run may legitimately produce fewer files than
    // it tracked (missing-asset grace).
    let existing: Vec<&str> = paths
        .iter()
        .map(String::as_str)
        .filter(|p| root.join(p).exists())
        .collect();
    if existing.is_empty() {
        return Ok(());
    }
    let mut add_args: Vec<&str> = vec!["add", "--"];
    add_args.extend(existing);
    git_ok(root, &add_args)?;

    // Anything staged?
    let staged = git(root, &["diff", "--cached", "--quiet"])?;
    match staged.status.code() {
        Some(0) => return Ok(()), // nothing to commit
        Some(1) => {}             // a successfully obtained difference
        _ => return Err(failed(&["diff", "--cached", "--quiet"], &staged)),
    }

    let email = config_get_key(root, "user.email")?;
    let name = config_get_key(root, "user.name")?;
    let has_identity = email.is_some() && name.is_some();
    let mut args: Vec<&str> = vec![];
    if !has_identity {
        args.extend([
            "-c",
            "user.name=codeflow",
            "-c",
            "user.email=codeflow@localhost",
        ]);
    }
    // No --no-verify: the scaffold commit is a sanctioned path (charter D9).
    // Policy is armed BEFORE this commit so the committed project.toml carries
    // policy_armed = true (a checkout must never resurrect a disarmed state);
    // the commit itself passes the armed hooks via the gate-context token.
    args.extend(["commit", "--quiet", "-m", message]);
    let out = git_env(
        root,
        &args,
        &[(crate::integrate::GATE_TOKEN_ENV, "scaffold")],
    )?;
    if out.status.success() {
        Ok(())
    } else {
        Err(failed(&args, &out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn broken_include(unreadable: bool) -> tempfile::TempDir {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let include = dir.path().join("included-config");
        std::fs::write(
            &include,
            if unreadable {
                "[core]\n hooksPath = own-hooks\n"
            } else {
                "[broken\n"
            },
        )
        .unwrap();
        #[cfg(unix)]
        if unreadable {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&include, std::fs::Permissions::from_mode(0o0)).unwrap();
        }
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git/config"))
            .unwrap();
        writeln!(config, "[include]\n path = {}", include.display()).unwrap();
        dir
    }

    fn assert_hook_queries_refuse(dir: &Path) {
        assert!(super::super::detect::configured_hooks_path(dir).is_err());
        assert!(super::super::detect::detect_hook_manager(dir).is_err());
    }

    #[test]
    fn r20_malformed_include_refuses_hook_queries() {
        let dir = broken_include(false);
        assert_hook_queries_refuse(dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn r20_unreadable_include_refuses_hook_queries() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = broken_include(true);
        let result = std::panic::catch_unwind(|| assert_hook_queries_refuse(dir.path()));
        std::fs::set_permissions(
            dir.path().join("included-config"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        result.unwrap();
    }

    #[test]
    fn r20_repository_query_failure_is_not_absence() {
        let dir = broken_include(false);
        assert!(is_repo(dir.path()).is_err());
    }

    #[test]
    fn r20_commit_query_failure_is_not_an_unborn_branch() {
        let dir = broken_include(false);
        assert!(has_commits(dir.path()).is_err());
    }

    #[test]
    fn r20_common_directory_query_failure_is_not_absence() {
        let dir = broken_include(false);
        assert!(common_dir(dir.path()).is_err());
    }

    #[test]
    fn r20_failed_index_comparison_refuses_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, commit) = crate::git::repo_with_tree(dir.path(), &[(b"a.txt", b"before")]);
        // Keep HEAD valid but remove its tree. A nonexistent HEAD is treated
        // by `diff --cached` as an unborn branch, so it cannot exercise the
        // acquiring-error branch of this command.
        let tree = repo.find_commit(commit).unwrap().tree_id().to_string();
        std::fs::remove_file(
            repo.path()
                .join("objects")
                .join(&tree[..2])
                .join(&tree[2..]),
        )
        .unwrap();
        std::fs::write(dir.path().join("a.txt"), "after").unwrap();
        let error = add_and_commit(dir.path(), &["a.txt".to_string()], "fixture").unwrap_err();
        // The step is named by its subcommand only; git's own output and
        // arguments are not shown (TSK-1029).
        assert!(error.to_string().contains("git diff failed"), "{error}");
    }

    #[test]
    fn r20_genuine_repository_and_key_absence_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_repo(dir.path()).unwrap());
        assert_eq!(common_dir(dir.path()).unwrap(), None);
        git2::Repository::init(dir.path()).unwrap();
        assert!(is_repo(dir.path()).unwrap());
        assert!(!has_commits(dir.path()).unwrap());
        assert_eq!(
            config_get_key(dir.path(), "codeflow.r20-absent").unwrap(),
            None
        );
        config_set(dir.path(), "core.hooksPath", "").unwrap();
        assert_eq!(
            super::super::detect::configured_hooks_path(dir.path()).unwrap(),
            Some(String::new())
        );
        assert!(
            matches!(super::super::detect::detect_hook_manager(dir.path()).unwrap(), Some(super::super::detect::HookManager::HooksPath(path)) if path.is_empty())
        );
    }

    /// A hooks path that is not valid UTF-8 is kept as a key, never read as
    /// unset, so init does not overwrite it.
    #[test]
    fn a_config_key_keeps_a_value_that_is_not_utf8() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[demo]\n\tword = caf\xe9\n\tplain = ok\n")
            .unwrap();
        assert_eq!(
            config_get_key(dir.path(), "demo.plain").unwrap().as_deref(),
            Some("ok")
        );
        let key = config_get_key(dir.path(), "demo.word").unwrap().unwrap();
        assert_ne!(key, "caf\u{fffd}");
        assert_eq!(crate::git::display_key(&key), "caf\\xe9");
        assert_eq!(config_get_key(dir.path(), "demo.absent").unwrap(), None);
    }

    /// Round eight on issue 79: a git directory whose own name ends in a
    /// carriage return is that directory, not its trimmed spelling.
    #[test]
    fn the_common_dir_keeps_a_trailing_carriage_return() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir(&main).unwrap();
        crate::git::repo_with_tree(&main, &[(b"a.txt", b"x")]);
        let bare = dir.path().join("meta\r");
        let clone = codeflow_fixture::clone(dir.path(), &main, &bare)
            .bare()
            .output();
        if !clone.status.success() {
            return; // this volume refuses the name
        }
        let out = crate::git::command()
            .args(["worktree", "add", "-q", "../wt", "-b", "other", "main"])
            .current_dir(&bare)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let common = common_dir(&dir.path().join("wt"))
            .unwrap()
            .expect("a common dir");
        assert_eq!(common.file_name().unwrap(), "meta\r");
    }
}
