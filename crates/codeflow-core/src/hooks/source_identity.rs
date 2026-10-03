//! Reproducible source-input identity shared by the build script and hooks.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const INPUT_ROOTS: &[&str] = &[
    "crates/codeflow-core/src/hooks",
    "crates/codeflow-core/src/security",
    "crates/codeflow-core/src/remedy.rs",
    "crates/codeflow-core/src/portable_path.rs",
    "assets/base/policy.json",
    "assets/base/git-hooks",
];

/// Enumerate the exact input set, sorted by repository-relative path.
///
/// # Errors
/// An unreadable input or symbolic link is not a reproducible source tree.
pub fn input_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn visit(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_symlink() {
            return Err(std::io::Error::other("source input is a symbolic link"));
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(path)? {
                visit(&entry?.path(), files)?;
            }
        } else if meta.is_file() {
            files.push(path.to_path_buf());
        }
        Ok(())
    }
    let mut files = Vec::new();
    for path in INPUT_ROOTS {
        visit(&root.join(path), &mut files)?;
    }
    files.sort();
    Ok(files)
}

/// Hash paths and bytes, with lengths so file boundaries cannot collide.
///
/// # Errors
/// Returns an error on an unreadable or missing source input.
pub fn input_digest(root: &Path) -> std::io::Result<String> {
    let mut digest = Sha256::new();
    for path in input_files(root)? {
        let relative = path
            .strip_prefix(root)
            .map_err(std::io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path)?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(hex_digest(&digest.finalize()))
}

/// Lowercase hexadecimal digest bytes.
#[must_use]
pub fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("writing to String");
    }
    out
}

/// How a caller starts git: the crate's one constructor, or, in the build
/// script that shares this file, a plain process (it runs no hook).
pub type Git<'a> = &'a dyn Fn() -> std::process::Command;

fn git(root: &Path, args: &[&str], make: Git<'_>) -> Option<String> {
    let out = make()
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Source revision, dirty state and Git metadata paths to watch. Archive
/// builds use the supplied revision and never discover an enclosing repo.
#[must_use]
pub fn revision(
    root: &Path,
    supplied: Option<&str>,
    make: Git<'_>,
) -> (String, String, Vec<PathBuf>) {
    if !root.join(".git").exists() {
        return (
            supplied
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("unavailable")
                .into(),
            "unavailable".into(),
            Vec::new(),
        );
    }
    let revision = git(root, &["rev-parse", "HEAD"], make).unwrap_or_else(|| "unavailable".into());
    let dirty = git(
        root,
        &["status", "--porcelain", "--untracked-files=normal"],
        make,
    )
    .map_or_else(
        || "unavailable".into(),
        |status| (!status.is_empty()).to_string(),
    );
    let mut paths = Vec::new();
    for name in [
        Some("HEAD".to_string()),
        git(root, &["symbolic-ref", "-q", "HEAD"], make),
        Some("packed-refs".to_string()),
        Some("index".to_string()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(path) = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", &name],
            make,
        ) {
            paths.push(PathBuf::from(path));
        }
    }
    (revision, dirty, paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sources(root: &Path) {
        for path in INPUT_ROOTS {
            let path = root.join(path);
            if path.extension().is_some() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, "base").unwrap();
            } else {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(path.join("input.rs"), "base").unwrap();
            }
        }
    }
    #[test]
    fn same_head_and_second_edit_change_inputs_and_restore_reproduces_digest() {
        let dir = tempfile::tempdir().unwrap();
        sources(dir.path());
        let original = input_digest(dir.path()).unwrap();
        let file = dir.path().join(INPUT_ROOTS[0]).join("input.rs");
        std::fs::write(&file, "first").unwrap();
        let first = input_digest(dir.path()).unwrap();
        std::fs::write(&file, "second").unwrap();
        let second = input_digest(dir.path()).unwrap();
        assert_ne!(original, first);
        assert_ne!(first, second);
        std::fs::write(&file, "base").unwrap();
        assert_eq!(original, input_digest(dir.path()).unwrap());
    }
    #[test]
    fn archive_revision_is_supplied_or_unavailable_even_inside_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let out = crate::git::command()
            .args(["init", "-q"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        let archive = dir.path().join("archive");
        std::fs::create_dir(&archive).unwrap();
        let make = crate::git::command;
        assert_eq!(revision(&archive, Some("candidate"), &make).0, "candidate");
        assert_eq!(revision(&archive, None, &make).0, "unavailable");
    }
    #[test]
    fn linked_and_detached_worktrees_resolve_native_revision_and_head_path() {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let out = crate::git::command()
                .args(args)
                .current_dir(dir.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.test")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.test")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["commit", "--allow-empty", "-qm", "base"]);
        let expected = run(&["rev-parse", "HEAD"]);
        let linked = dir.path().join("linked");
        run(&[
            "worktree",
            "add",
            "-qb",
            "task/linked",
            linked.to_str().unwrap(),
        ]);
        let make = crate::git::command;
        let (sha, _, paths) = revision(&linked, None, &make);
        assert_eq!(sha, expected);
        assert!(paths
            .iter()
            .any(|p| p.file_name().unwrap() == "HEAD" && p.exists()));
        let out = crate::git::command()
            .args(["checkout", "--detach", "-q"])
            .current_dir(&linked)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(revision(&linked, None, &make).0, expected);
    }
}
