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

/// Run `command` with `input` on its stdin and capture its output.
///
/// The child's stdin is written from its own thread while stdout and stderr
/// are drained here, and stdin is closed when the input is written, so a
/// child that answers before it has read everything (`git check-ignore -v
/// --stdin` writes a record per path it reads) cannot fill an output pipe
/// and deadlock against this process's write. The command's stdout and stderr
/// are captured whatever the caller set.
///
/// A child that exits without reading all of `input` closes the pipe; the
/// resulting broken pipe is not itself the verdict, so the caller reads the
/// exit status. Any other write error, or a panicked writer, is returned.
///
/// # Errors
///
/// Returns the error when the child cannot start, its output cannot be read,
/// or its stdin cannot be written for a reason other than the child closing
/// it.
pub fn output_with_input(
    command: &mut std::process::Command,
    input: &[u8],
) -> std::io::Result<std::process::Output> {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("child stdin was not piped"))?;
    let input = input.to_vec();
    // `stdin` drops with the thread, which closes the pipe and ends the input.
    // A thread that cannot be created is an error, not a panic: the child is
    // stopped and reaped first, so none is left running.
    let writer = match std::thread::Builder::new().spawn(move || stdin.write_all(&input)) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let output = child.wait_with_output();
    let written = writer
        .join()
        .map_err(|_| std::io::Error::other("stdin writer panicked"))?;
    let output = output?;
    match written {
        Ok(()) => Ok(output),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(output),
        Err(error) => Err(error),
    }
}

pub use ci::{wait_for_ci_green, CiOutcome, CiWaitConfig, CiWaitError};
pub use conflict::{attempt_rebase, check_merge_conflicts, ConflictResult, RebaseResult};

#[cfg(test)]
mod tests {
    use super::{output_with_input, runs_git};
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

    /// Run `work` on its own thread and fail, instead of hanging, when it
    /// does not finish within a minute.
    #[cfg(unix)]
    fn bounded<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("the child exchange finished instead of deadlocking on a pipe")
    }

    #[cfg(unix)]
    #[test]
    fn output_larger_than_a_pipe_does_not_block_the_input() {
        // `cat` answers as it reads, so a writer that sends all 4 MiB before
        // reading blocks once the output pipe fills.
        let input: Vec<u8> = (b'a'..=b'w').cycle().take(4 * 1024 * 1024).collect();
        let expected = input.clone();
        let out = bounded(move || {
            output_with_input(&mut std::process::Command::new("cat"), &input).unwrap()
        });
        assert!(out.status.success());
        assert_eq!(out.stdout, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_stops_reading_is_judged_by_its_exit_status() {
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = bounded(move || {
            output_with_input(&mut std::process::Command::new("true"), &input).unwrap()
        });
        assert!(out.status.success());
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = bounded(move || {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "exit 3"]);
            output_with_input(&mut command, &input).unwrap()
        });
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn a_program_that_cannot_start_is_an_error() {
        let mut command = std::process::Command::new("/nonexistent/codeflow-test-program");
        assert!(output_with_input(&mut command, b"x").is_err());
    }
}
