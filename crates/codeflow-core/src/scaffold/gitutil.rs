//! Thin git plumbing for the scaffold engine — shells out to the `git`
//! binary; no libgit2 dependency. Only what init needs: repo detection,
//! `git init`, config get/set, staged adds, and the scaffold commit.

use std::path::Path;
use std::process::Command;

use super::ScaffoldError;

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output, ScaffoldError> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
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
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// Does the repo have at least one commit?
pub fn has_commits(root: &Path) -> bool {
    git(root, &["rev-parse", "--verify", "HEAD"])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn init_repo(root: &Path) -> Result<(), ScaffoldError> {
    git_ok(root, &["init", "--quiet"])
}

/// `git config --get <key>` → Some(value) when set and non-empty.
pub fn config_get(root: &Path, key: &str) -> Option<String> {
    git(root, &["config", "--get", key]).ok().and_then(|o| {
        if o.status.success() {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (!v.is_empty()).then_some(v)
        } else {
            None
        }
    })
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
        config_get(root, "user.email").is_some() && config_get(root, "user.name").is_some();
    let mut args: Vec<&str> = vec![];
    if !has_identity {
        args.extend([
            "-c",
            "user.name=codeflow",
            "-c",
            "user.email=codeflow@localhost",
        ]);
    }
    // No --no-verify: the scaffold commit passes through the freshly wired
    // hooks legitimately because policy_armed is still false (bootstrap grace).
    args.extend(["commit", "--quiet", "-m", message]);
    git_ok(root, &args)
}
