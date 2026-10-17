//! Fixture folders for unit tests that must not touch a repository outside
//! their own temporary folder (issue 104).
//!
//! The engine treats the nearest enclosing folder that holds a repository as
//! the project, so a test that writes into a bare temporary folder writes into
//! the enclosing repository when `TMPDIR` points inside one. A folder given a
//! repository of its own is the project. The default temporary directory is
//! outside any repository and is left as the test made it, so a normal run
//! still covers the paths that need a bare folder.

use std::path::Path;

/// Whether a folder above `root` holds a repository. The path is resolved
/// first: a `TMPDIR` reached through a link into a repository's folder has no
/// `.git` among its lexical parents.
fn enclosed(root: &Path) -> bool {
    let physical = root
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot resolve fixture {}: {error}", root.display()));
    physical
        .ancestors()
        .skip(1)
        .any(|ancestor| ancestor.join(".git").symlink_metadata().is_ok())
}

/// Give `root` a repository of its own when an enclosing folder already holds
/// one.
pub(crate) fn own_repository(root: &Path) {
    if !enclosed(root) || root.join(".git").symlink_metadata().is_ok() {
        return;
    }
    let out = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git init in {} failed: {}",
        root.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A temporary project folder that never reaches an enclosing repository.
pub(crate) fn project_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project tempdir");
    own_repository(dir.path());
    dir
}

/// A temporary folder with no repository above it, for a test that asserts
/// there is none. `None` when the temporary directory is inside a repository:
/// the test cannot hold there, and running it would reach the enclosing
/// repository, so it skips.
pub(crate) fn outside_repository() -> Option<tempfile::TempDir> {
    let dir = tempfile::tempdir().expect("tempdir");
    if enclosed(dir.path()) {
        eprintln!("skipped: the temporary directory is inside a repository");
        return None;
    }
    Some(dir)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{outside_repository, own_repository};

    /// A fixture reached through a link into a repository's folder still gets
    /// a repository of its own: its lexical parents hold no `.git`.
    #[test]
    fn a_link_into_a_repository_does_not_hide_it() {
        let repository = tempfile::tempdir().unwrap();
        std::fs::create_dir(repository.path().join(".git")).unwrap();
        let inside = repository.path().join("tmp");
        std::fs::create_dir(&inside).unwrap();
        let alias = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&inside, alias.path().join("link")).unwrap();
        let fixture = alias.path().join("link/proj");
        std::fs::create_dir(&fixture).unwrap();
        assert!(super::enclosed(&fixture));
        own_repository(&fixture);
        assert!(fixture.join(".git").exists());
    }

    #[test]
    fn a_fixture_outside_any_repository_stays_bare() {
        let Some(dir) = outside_repository() else {
            return;
        };
        let fixture = dir.path().join("proj");
        std::fs::create_dir(&fixture).unwrap();
        own_repository(&fixture);
        assert!(!fixture.join(".git").exists());
    }
}
