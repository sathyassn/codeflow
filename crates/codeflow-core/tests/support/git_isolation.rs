//! Hermetic git for fixture tests (issue 104, TSK-282).
//!
//! A fixture project is a folder under the system temporary directory. When
//! `TMPDIR` points inside a git repository and the fixture has no repository
//! of its own, git discovers the enclosing one, and a fixture that runs
//! `git add`, `git commit` or `scaffold::init` changes that repository.
//! `GIT_CEILING_DIRECTORIES` stops git's search at the temporary directory, so
//! a fixture sees its own repository or none. libgit2's
//! `Repository::discover` ignores the variable, and so does the engine's own
//! check that git and libgit2 agree on whether a repository exists
//! (`hooks::repo::open`), so a fixture that reaches either must be given its
//! own repository: [`own_repository`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// Point git at empty global and system configuration and stop its repository
/// search at the temporary directory, for the whole test process. The engine
/// under test spawns its own `git` subprocesses, which inherit the process
/// environment, so a per-`Command` `.env()` cannot reach them. The mutation
/// is made race-free by doing it exactly once behind a `Once` that every test
/// synchronizes on before its first spawn: POSIX `setenv` concurrent with
/// `getenv` or a spawn is a data race, so repeated `set_var` from parallel
/// tests, even with identical values, is not benign.
pub fn isolate_git() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
        std::env::set_var("GIT_CEILING_DIRECTORIES", ceiling());
    });
}

/// The directories git must not search into: the temporary directory (as git
/// resolves it, links followed) after any ceiling already set.
fn ceiling() -> std::ffi::OsString {
    let temporary = std::env::temp_dir();
    let temporary = temporary.canonicalize().unwrap_or(temporary);
    let mut entries: Vec<PathBuf> = std::env::var_os("GIT_CEILING_DIRECTORIES")
        .map(|existing| std::env::split_paths(&existing).collect())
        .unwrap_or_default();
    entries.push(temporary);
    std::env::join_paths(entries).expect("the temporary directory has no path separator")
}

/// Give the fixture project folder `root` a repository of its own when an
/// enclosing folder already holds one, so git and libgit2 both stop at the
/// fixture. A temporary directory outside any repository (the default) is left
/// as the test wrote it, which keeps the paths that need no repository, such
/// as `scaffold::init` into a bare folder, covered in a normal run. Call
/// [`isolate_git`] first.
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
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet"])
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
