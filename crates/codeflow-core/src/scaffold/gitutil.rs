//! Thin git plumbing for the scaffold engine — shells out to the `git`
//! binary; no libgit2 dependency. Only what init needs: repo detection,
//! `git init`, config get/set, staged adds, and the scaffold commit.

use std::path::Path;

use super::ScaffoldError;

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
        .map_err(|e| ScaffoldError::Git(format!("failed to run git {}: {e}", args.join(" "))))
}

fn git_ok(root: &Path, args: &[&str]) -> Result<(), ScaffoldError> {
    let out = git(root, args)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(ScaffoldError::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Is `root` inside a git work tree?
pub fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
}

/// Does the repo have at least one commit?
pub fn has_commits(root: &Path) -> bool {
    git(root, &["rev-parse", "--verify", "HEAD"]).is_ok_and(|o| o.status.success())
}

/// The common git dir (the main checkout's `.git`, shared by linked
/// worktrees), as an absolute path. `None` outside a repository.
pub fn common_dir(root: &Path) -> Option<std::path::PathBuf> {
    let out = git(root, &["rev-parse", "--git-common-dir"]).ok()?;
    if !out.status.success() {
        return None;
    }
    // OS text rule (issue 79): the folder is joined to a path, so its exact
    // bytes are used, never a lossy spelling.
    let dir = crate::git::GitName::from_bytes(out.stdout.trim_ascii())
        .os_path()
        .ok()?;
    (!dir.as_os_str().is_empty()).then(|| root.join(dir))
}

pub fn init_repo(root: &Path) -> Result<(), ScaffoldError> {
    git_ok(root, &["init", "--quiet"])
}

/// `git config --get <key>` as a storage key (OS text rule, issue 79): a
/// value that is not valid UTF-8 keeps its exact bytes in the key, so it is
/// never read as unset and never equals a text value. Use it where an
/// existing setting decides what the scaffold may overwrite.
pub fn config_get_key(root: &Path, key: &str) -> Option<String> {
    // `--null` frames the value with a NUL, so a value that ends in a carriage
    // return or a newline keeps it (a quoted value can).
    let out = git(root, &["config", "--null", "--get", key]).ok()?;
    if !out.status.success() {
        return None;
    }
    let bytes = out.stdout.strip_suffix(&[0]).unwrap_or(&out.stdout);
    (!bytes.is_empty()).then(|| crate::git::GitName::from_bytes(bytes).storage_key())
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
    if staged.status.success() {
        return Ok(()); // nothing to commit
    }

    let has_identity =
        config_get_key(root, "user.email").is_some() && config_get_key(root, "user.name").is_some();
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
        Err(ScaffoldError::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            config_get_key(dir.path(), "demo.plain").as_deref(),
            Some("ok")
        );
        let key = config_get_key(dir.path(), "demo.word").unwrap();
        assert_ne!(key, "caf\u{fffd}");
        assert_eq!(crate::git::display_key(&key), "caf\\xe9");
        assert_eq!(config_get_key(dir.path(), "demo.absent"), None);
    }
}
