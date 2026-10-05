//! The one place codeflow pipes a child's stdin.
//!
//! A child that answers while it reads (`git check-ignore -v --stdin` writes a
//! record per path, `git cat-file --batch` a blob per id) fills its output
//! pipe once the answer outgrows it. A caller that writes all of stdin first,
//! or reads one output stream while another fills, then blocks against the
//! child for good (issue 71). Every piped stdin therefore goes through this
//! module: the input is written from its own thread and closed when written.
//! A source scan (`crates/codeflow-core/tests/stdin_pipe_rule.rs`) fails when
//! a `.stdin(...)` call that pipes appears anywhere else in the crates.

use std::io::Write as _;
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;

/// The thread that writes a child's stdin.
#[derive(Debug)]
pub struct InputWriter(JoinHandle<std::io::Result<()>>);

impl InputWriter {
    /// Whether the input has been written, or the write has ended.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    /// Wait for the write and return how it ended. A broken pipe is not an
    /// error here: the child stopped reading, and its exit status is the
    /// verdict.
    ///
    /// # Errors
    ///
    /// Returns a write error other than a broken pipe, or the error for a
    /// panicked writer.
    pub fn finish(self) -> std::io::Result<()> {
        match self.0.join() {
            Err(_) => Err(std::io::Error::other("stdin writer panicked")),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Ok(result) => result,
        }
    }
}

/// Start `command` with `input` written to its stdin from its own thread.
///
/// stdin is piped and closed once the input is written. The caller sets
/// stdout and stderr and owns draining them; [`output_with_input`] does both
/// for the common case. A thread that cannot be created is an error, not a
/// panic: the child is stopped and reaped first, so none is left running.
///
/// # Errors
///
/// Returns the error when the child cannot start or the writer thread cannot
/// be created.
pub fn spawn_with_input(
    command: &mut Command,
    input: Vec<u8>,
) -> std::io::Result<(Child, InputWriter)> {
    spawn_with_input_stopping(command, input, |child| {
        let _ = child.kill();
    })
}

/// [`spawn_with_input`], with `stop` run on the child when the writer thread
/// cannot be created, for a caller whose child has descendants to stop too
/// (a process group). The child is reaped after `stop`.
///
/// # Errors
///
/// Returns the error when the child cannot start or the writer thread cannot
/// be created.
pub fn spawn_with_input_stopping(
    command: &mut Command,
    input: Vec<u8>,
    stop: impl FnOnce(&mut Child),
) -> std::io::Result<(Child, InputWriter)> {
    let mut child = command.stdin(Stdio::piped()).spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        stop(&mut child);
        let _ = child.wait();
        return Err(std::io::Error::other("child stdin was not piped"));
    };
    // `stdin` drops with the thread, which closes the pipe and ends the input.
    match start_writer(move || stdin.write_all(&input)) {
        Ok(writer) => Ok((child, InputWriter(writer))),
        Err(error) => {
            stop(&mut child);
            let _ = child.wait();
            Err(error)
        }
    }
}

fn start_writer(
    write: impl FnOnce() -> std::io::Result<()> + Send + 'static,
) -> std::io::Result<JoinHandle<std::io::Result<()>>> {
    #[cfg(test)]
    if tests::FAIL_WRITER_START.with(std::cell::Cell::get) {
        return Err(std::io::Error::other("injected writer start failure"));
    }
    std::thread::Builder::new().spawn(write)
}

/// Run `command` with `input` on its stdin and capture its output.
///
/// stdin is written from its own thread while stdout and stderr are drained
/// here, so an answer larger than a pipe cannot deadlock against the write.
/// The command's stdout and stderr are captured whatever the caller set.
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
pub fn output_with_input(command: &mut Command, input: &[u8]) -> std::io::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let (child, writer) = spawn_with_input(command, input.to_vec())?;
    let output = child.wait_with_output();
    let written = writer.finish();
    let output = output?;
    written?;
    Ok(output)
}

#[cfg(test)]
pub(crate) mod tests {
    #[cfg(unix)]
    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// Makes this thread's next writer starts fail, for the tests.
        pub(crate) static FAIL_WRITER_START: Cell<bool> = const { Cell::new(false) };
    }

    #[cfg(unix)]
    #[test]
    fn a_writer_that_cannot_start_stops_and_reaps_the_child() {
        FAIL_WRITER_START.with(|flag| flag.set(true));
        let stopped = Cell::new(false);
        let mut command = Command::new("sleep");
        command.arg("30");
        let result = spawn_with_input_stopping(&mut command, vec![b'x'; 16], |child| {
            stopped.set(true);
            let _ = child.kill();
        });
        FAIL_WRITER_START.with(|flag| flag.set(false));
        assert!(result.unwrap_err().to_string().contains("injected"));
        assert!(stopped.get(), "the caller's stop ran on the child");
    }

    #[cfg(unix)]
    #[test]
    fn the_default_stop_kills_a_child_whose_writer_cannot_start() {
        FAIL_WRITER_START.with(|flag| flag.set(true));
        let started = std::time::Instant::now();
        let mut command = Command::new("sleep");
        command.arg("30");
        let result = spawn_with_input(&mut command, vec![b'x'; 16]);
        FAIL_WRITER_START.with(|flag| flag.set(false));
        assert!(result.is_err());
        // The child was killed and reaped, not waited out.
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
