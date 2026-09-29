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
}
