//! A fixture project that never reaches a repository outside its own
//! temporary folder (issue 104, TSK-282).
//!
//! The engine treats the nearest enclosing folder that holds a repository as
//! the project: `codeflow init` configures it and `estimate check` resolves
//! sources against it. When `TMPDIR` points inside a repository, a fixture
//! folder without a repository of its own is therefore read and written as
//! part of that repository. `GIT_CEILING_DIRECTORIES` does not help, because
//! the engine also discovers repositories with libgit2, which ignores it.

use std::path::Path;
use std::process::Command;

/// Give the project folder `root` a repository of its own when an enclosing
/// folder holds one. The default temporary directory is outside any
/// repository, so a normal run leaves the folder bare and still covers
/// `codeflow init` into a folder with no repository.
pub fn own_repository(root: &Path) {
    // Ancestry is judged on the physical path: a `TMPDIR` reached through a
    // link into a repository's folder has no `.git` among its lexical parents.
    let physical = root
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot resolve fixture {}: {error}", root.display()));
    let enclosed = physical
        .ancestors()
        .skip(1)
        .any(|ancestor| ancestor.join(".git").symlink_metadata().is_ok());
    if !enclosed || physical.join(".git").symlink_metadata().is_ok() {
        return;
    }
    let out = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
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
