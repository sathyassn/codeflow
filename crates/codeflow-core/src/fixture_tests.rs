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

/// Whether a folder above `root` holds a repository.
fn enclosed(root: &Path) -> bool {
    root.ancestors()
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
