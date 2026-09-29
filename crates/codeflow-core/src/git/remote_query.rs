//! Bounded, non-interactive questions to a git remote.
//!
//! The pre-push hook, `codeflow ci` and `task status` ask a destination what
//! it holds (`git ls-remote`) and, when the judge needs an object this clone
//! lacks, fetch it. Every call runs with no terminal, askpass or SSH password
//! prompt, within a deadline (then the whole process group is killed) and a
//! size bound, so a hook never hangs on a remote.

use std::io::Read as _;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// How long `ls-remote` may take to answer.
pub const LS_REMOTE_DEADLINE: Duration = Duration::from_secs(10);
/// How long a fetch of the objects the judge needs may take.
pub const FETCH_DEADLINE: Duration = Duration::from_secs(120);
/// How much a remote may say.
pub const MAX_ANSWER_BYTES: usize = 16 << 20;

/// `git ls-remote <args>`, never interactive and bounded by
/// [`LS_REMOTE_DEADLINE`] and [`MAX_ANSWER_BYTES`].
///
/// # Errors
///
/// Returns why the remote could not be asked: git would not start, the
/// answer was too large or too slow, or `ls-remote` failed.
pub fn ls_remote(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut all = vec!["ls-remote"];
    all.extend_from_slice(args);
    run(root, &all, LS_REMOTE_DEADLINE).map_err(|why| match why {
        Failure::Exit => {
            "`git ls-remote` failed (unreachable, no credentials, or no such repository)"
                .to_string()
        }
        Failure::Other(why) => why,
    })
}

/// Fetch `refs` from `url` into the object store only: no tracking ref,
/// tag or `FETCH_HEAD` is written, so nothing a later check reads as
/// authority changes.
///
/// # Errors
///
/// Returns why the fetch did not complete.
pub fn fetch_objects(root: &Path, url: &str, refs: &[&str]) -> Result<(), String> {
    let mut all = vec![
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-write-fetch-head",
        "--no-recurse-submodules",
        url,
    ];
    all.extend_from_slice(refs);
    run(root, &all, FETCH_DEADLINE)
        .map(|_| ())
        .map_err(|why| match why {
            Failure::Exit => format!("`git fetch {url}` failed"),
            Failure::Other(why) => why,
        })
}

enum Failure {
    /// Git ran and exited unsuccessfully.
    Exit,
    Other(String),
}

fn run(root: &Path, args: &[&str], deadline: Duration) -> Result<String, Failure> {
    let mut command = crate::git::command();
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false")
        .env("SSH_ASKPASS", "false")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_SSH_COMMAND", batch_ssh_command(root))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| Failure::Other(format!("could not start git: {error}")))?;
    let Some(stdout) = child.stdout.take() else {
        kill_group(&mut child);
        return Err(Failure::Other("could not read its answer".to_string()));
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout
            .take(MAX_ANSWER_BYTES as u64 + 1)
            .read_to_end(&mut bytes);
        let _ = sender.send(read.map(|_| bytes));
    });
    let until = Instant::now() + deadline;
    let timed_out = || Failure::Other(format!("no answer within {}s", deadline.as_secs()));
    let bytes = loop {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(bytes)) => break bytes,
            Ok(Err(error)) => {
                kill_group(&mut child);
                return Err(Failure::Other(format!(
                    "reading its answer failed: {error}"
                )));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < until => {}
            Err(_) => {
                kill_group(&mut child);
                return Err(timed_out());
            }
        }
    };
    if bytes.len() > MAX_ANSWER_BYTES {
        kill_group(&mut child);
        return Err(Failure::Other(format!(
            "its answer is over {} MiB",
            MAX_ANSWER_BYTES >> 20
        )));
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill_group(&mut child);
                return Err(timed_out());
            }
        }
    };
    if !status.success() {
        return Err(Failure::Exit);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The SSH command git would use, with password and host-key prompts off.
/// A configured command is kept: `GIT_SSH_COMMAND`, then `core.sshCommand`,
/// then `GIT_SSH`, then `ssh`.
fn batch_ssh_command(root: &Path) -> String {
    let configured = std::env::var("GIT_SSH_COMMAND")
        .ok()
        .filter(|command| !command.trim().is_empty())
        .or_else(|| {
            crate::git::command()
                .arg("-C")
                .arg(root)
                .args(["config", "--get", "core.sshCommand"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .filter(|command| !command.is_empty())
        })
        .or_else(|| {
            std::env::var("GIT_SSH")
                .ok()
                .filter(|program| !program.is_empty())
                .map(|program| format!("'{}'", program.replace('\'', "'\\''")))
        })
        .unwrap_or_else(|| "ssh".to_string());
    format!("{configured} -o BatchMode=yes")
}

/// Kill a child and, on Unix, the process group it leads (an SSH or
/// credential helper it started), then reap it.
fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: the child leads its own process group (`process_group(0)`),
        // whose id is its pid; SIGKILL to it is best-effort.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
