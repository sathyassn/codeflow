//! Git operations: merge conflict detection, branch analysis, CI polling,
//! and the one constructor for every `git` process codeflow starts.
//!
//! Uses `git2` for native git operations where it can; CI polling shells to
//! the `gh` CLI.

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod ci;
pub mod conflict;
pub mod remote_query;

/// The variable a codeflow git-hook shim reads to run the codeflow binary
/// whose command started git, instead of the `codeflow` first on PATH
/// (SPC-013 R-85, amended). It is a dispatch hint, not authentication.
pub const HOOK_BINARY_ENV: &str = "CODEFLOW_HOOK_BINARY";

static CALLING_BINARY: OnceLock<PathBuf> = OnceLock::new();

/// Record `binary` as the codeflow binary this process is, so the git
/// processes it starts dispatch their hooks to it. The CLI calls this once
/// at startup with its own runtime path; a library caller that never does
/// keeps the shims' PATH lookup.
pub fn designate_calling_binary(binary: PathBuf) {
    let _ = CALLING_BINARY.set(binary);
}

/// A `git` process, with [`HOOK_BINARY_ENV`] set in its environment only,
/// overwriting any inherited value, when this process designated itself.
/// Every `git` codeflow starts is built here, so a hook git fires during a
/// codeflow command runs that same binary.
#[must_use]
pub fn command() -> std::process::Command {
    process("git")
}

/// A process for `program`. When `program` runs git (its last path part,
/// without an extension, is `git`), it is built as [`command`] builds git,
/// so a caller that holds the program name in a variable still gets the
/// hook dispatch; any other program is a plain `Command`.
#[must_use]
pub fn process(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let program = program.as_ref();
    let mut process = std::process::Command::new(program);
    if runs_git(program) {
        if let Some(binary) = CALLING_BINARY.get() {
            process.env(HOOK_BINARY_ENV, binary);
        }
    }
    process
}

fn runs_git(program: &std::ffi::OsStr) -> bool {
    let name = program.to_string_lossy();
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    name.split('.')
        .next()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("git"))
}

/// The short name of a reference, read lossily.
///
/// OS text rule (issue 79, `docs/architecture.md`): a branch name is matched
/// against valid-UTF-8 patterns and names (protected globs, task prefixes) and
/// shown, and a lossy spelling matches them exactly as the bytes do, so a
/// branch that is not valid UTF-8 must not read as detached or absent.
#[must_use]
pub fn reference_shorthand(reference: &git2::Reference<'_>) -> String {
    String::from_utf8_lossy(reference.shorthand_bytes()).into_owned()
}

/// A linked worktree as git lists it.
#[derive(Debug, Clone)]
pub struct LinkedWorktree {
    /// The administrative folder name, read lossily; it is shown, never
    /// looked up.
    pub name: String,
    /// The checkout, as git records it in the worktree's `gitdir` file.
    pub path: PathBuf,
}

/// Every linked worktree of `repo`, whatever its folder name holds.
///
/// OS text rule (issue 79, `docs/architecture.md`): a worktree whose
/// administrative folder name is not valid UTF-8 cannot be asked for by
/// `find_worktree`, and dropping it from a list would hide a checkout from a
/// guard or from cleanup. Its checkout is read from its `gitdir` file, which
/// is what `find_worktree` reads too.
#[must_use]
pub fn linked_worktrees(repo: &git2::Repository) -> Vec<LinkedWorktree> {
    let Ok(names) = repo.worktrees() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names.iter_bytes() {
        let lossy = String::from_utf8_lossy(name).into_owned();
        let path = match std::str::from_utf8(name) {
            Ok(valid) => repo
                .find_worktree(valid)
                .ok()
                .map(|worktree| worktree.path().to_path_buf()),
            Err(_) => gitdir_checkout(repo, name),
        };
        if let Some(path) = path {
            out.push(LinkedWorktree { name: lossy, path });
        }
    }
    out
}

/// The checkout named by `<common dir>/worktrees/<name>/gitdir`, which holds
/// the path of the checkout's `.git`.
fn gitdir_checkout(repo: &git2::Repository, name: &[u8]) -> Option<PathBuf> {
    let file = repo
        .commondir()
        .join("worktrees")
        .join(os_component(name))
        .join("gitdir");
    let text = std::fs::read(file).ok()?;
    let gitdir = os_path(text.trim_ascii_end());
    gitdir.parent().map(std::path::Path::to_path_buf)
}

/// One path component from bytes, exact where the platform allows it.
#[must_use]
pub fn os_component(name: &[u8]) -> PathBuf {
    os_path(name)
}

fn os_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

pub use ci::{wait_for_ci_green, CiOutcome, CiWaitConfig, CiWaitError};
pub use conflict::{attempt_rebase, check_merge_conflicts, ConflictResult, RebaseResult};

#[cfg(test)]
mod tests {
    use super::runs_git;
    use std::ffi::OsStr;

    #[test]
    fn a_program_runs_git_by_its_last_path_part() {
        for program in [
            "git",
            "GIT",
            "git.exe",
            "/usr/bin/git",
            r"C:\Git\cmd\git.exe",
        ] {
            assert!(runs_git(OsStr::new(program)), "{program}");
        }
        for program in ["gh", "gitleaks", "git-lfs", "/opt/git/bin/sh", "digit"] {
            assert!(!runs_git(OsStr::new(program)), "{program}");
        }
    }

    /// Review finding on issue 79: a linked worktree whose folder name is not
    /// valid UTF-8 stays in the list, with its checkout, instead of vanishing
    /// from guards and cleanup. Runs where the file system accepts the name,
    /// as on Linux.
    #[cfg(unix)]
    #[test]
    fn a_worktree_named_in_latin1_is_listed_with_its_checkout() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let admin = dir
            .path()
            .join(".git")
            .join("worktrees")
            .join(OsStr::from_bytes(b"caf\xe9"));
        if std::fs::create_dir_all(&admin).is_err() {
            return;
        }
        let checkout = dir.path().join("linked");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", checkout.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let listed = super::linked_worktrees(&repo);
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].name, "caf\u{fffd}");
        assert_eq!(listed[0].path, checkout);
    }
}
