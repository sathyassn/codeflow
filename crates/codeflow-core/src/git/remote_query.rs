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
        .env("GIT_SSH_COMMAND", batch_ssh_command(root)?)
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
    text_answer(&bytes)
}

/// The answer as text, one line per advertised ref.
///
/// OS text rule (issue 79): the advertised names are matched against declared
/// targets and release patterns, which need text, and a lossy decode would let
/// one name stand for another. A ref whose name is not valid UTF-8 is left out
/// of the answer: it can be neither a declared target nor a release or epic
/// line, so leaving it out only narrows what a caller can verify. The one name
/// that cannot be left out is the default branch (`ref: ... HEAD`), whose
/// answer is then a refusal, never another branch.
fn text_answer(bytes: &[u8]) -> Result<String, Failure> {
    let mut text = String::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        match std::str::from_utf8(line) {
            Ok(valid) => {
                text.push_str(valid);
                text.push('\n');
            }
            Err(_) if line.starts_with(b"ref: ") => {
                return Err(Failure::Other(
                    "the destination's default branch name is not valid UTF-8".to_string(),
                ));
            }
            Err(_) => {}
        }
    }
    Ok(text)
}

/// The SSH command git would use, with password and host-key prompts off.
/// A configured command is kept: `GIT_SSH_COMMAND`, then `core.sshCommand`,
/// then `GIT_SSH`, then `ssh`.
///
/// OS text rule (issue 79): the command is executed, so it must be exactly what
/// git would run. A value that is not valid UTF-8 cannot be passed on as text,
/// and a lossy spelling would run a different program, so the query refuses.
fn batch_ssh_command(root: &Path) -> Result<String, Failure> {
    let not_utf8 = |name: &str| {
        Failure::Other(format!(
            "{name} is not valid UTF-8, so the remote is not asked"
        ))
    };
    let text =
        |name: &str, value: std::ffi::OsString| value.into_string().map_err(|_| not_utf8(name));
    let from_env = |name: &str| -> Result<Option<String>, Failure> {
        match std::env::var_os(name) {
            Some(value) => Ok(Some(text(name, value)?)),
            None => Ok(None),
        }
    };
    // A value that is set is the command git would run, whatever it holds: an
    // empty one refuses below, as git refuses it, and never falls back to
    // another program.
    let mut configured = from_env("GIT_SSH_COMMAND")?;
    if configured.is_none() {
        let out = crate::git::command()
            .arg("-C")
            .arg(root)
            .args(["config", "--null", "--get", "core.sshCommand"])
            .output()
            .ok()
            .filter(|out| out.status.success());
        if let Some(out) = out {
            // `--null` frames the value with a NUL, so a value that ends in a
            // carriage return keeps it.
            let framed = out.stdout.strip_suffix(&[0]).unwrap_or(&out.stdout);
            let value = std::str::from_utf8(framed)
                .map_err(|_| not_utf8("core.sshCommand"))?
                .to_string();
            configured = Some(value);
        }
    }
    if configured.is_none() {
        configured = from_env("GIT_SSH")?.map(|program| {
            if program.is_empty() {
                program
            } else {
                format!("'{}'", program.replace('\'', "'\\''"))
            }
        });
    }
    if configured.as_deref().is_some_and(str::is_empty) {
        return Err(Failure::Other(
            "the configured ssh command is empty, so the remote is not asked".to_string(),
        ));
    }
    let configured = configured.unwrap_or_else(|| "ssh".to_string());
    Ok(format!("{configured} -o BatchMode=yes"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Review finding on issue 79: advertised ref names were decoded lossily,
    /// so a valid `caf` plus U+FFFD resolved to the tip of the distinct branch
    /// whose last byte is invalid. A name that is not valid UTF-8 is left out.
    #[test]
    fn an_advertised_name_that_is_not_utf8_is_left_out_and_never_aliased() {
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(
            dir.path(),
            &[b"refs/heads/caf\xe9", "refs/heads/caf\u{fffd}".as_bytes()],
        );
        let path = repo.workdir().unwrap().to_path_buf();
        let listed = ls_remote(&path, &["--heads", path.to_str().unwrap()]).unwrap();
        assert!(listed.contains("refs/heads/caf\u{fffd}\n"), "{listed}");
        assert_eq!(listed.matches("refs/heads/caf").count(), 1, "{listed}");
    }

    #[test]
    fn a_default_branch_that_is_not_utf8_refuses_the_answer() {
        let answer = text_answer(b"ref: refs/heads/caf\xe9\tHEAD\n");
        assert!(matches!(answer, Err(Failure::Other(ref why)) if why.contains("not valid UTF-8")));
    }

    /// The SSH command is executed, so one that is not valid UTF-8 refuses
    /// the query instead of running a lossy lookalike.
    #[test]
    fn a_configured_ssh_command_that_is_not_utf8_refuses_the_query() {
        if std::env::var_os("GIT_SSH_COMMAND").is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(dir.path(), &[]);
        let config = repo.path().join("config");
        let mut text = std::fs::read(&config).unwrap();
        text.extend_from_slice(b"[core]\n\tsshCommand = ssh \xff\n");
        std::fs::write(&config, text).unwrap();
        let error = batch_ssh_command(dir.path()).err().unwrap();
        assert!(
            matches!(error, Failure::Other(ref why) if why.contains("not valid UTF-8")),
            "refused"
        );
    }

    /// Round twelve on issue 79: a configured command that ends in a carriage
    /// return runs that program, not its sibling without it.
    #[test]
    fn a_configured_ssh_command_keeps_a_trailing_carriage_return() {
        if std::env::var_os("GIT_SSH_COMMAND").is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(dir.path(), &[]);
        let config = repo.path().join("config");
        let mut text = std::fs::read(&config).unwrap();
        text.extend_from_slice(b"[core]\n\tsshCommand = \"ssh-wrapper\r\"\n");
        std::fs::write(&config, text).unwrap();
        let command = batch_ssh_command(dir.path()).ok().unwrap();
        assert_eq!(command, "ssh-wrapper\r -o BatchMode=yes");
    }

    /// Round thirteen on issue 79: a configured command that is empty is
    /// refused, as git refuses it, and never replaced by `ssh`.
    #[test]
    fn an_empty_configured_ssh_command_refuses_and_never_falls_back() {
        if std::env::var_os("GIT_SSH_COMMAND").is_some() || std::env::var_os("GIT_SSH").is_some() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(dir.path(), &[]);
        let config = repo.path().join("config");
        let mut text = std::fs::read(&config).unwrap();
        text.extend_from_slice(b"[core]\n\tsshCommand =\n");
        std::fs::write(&config, text).unwrap();
        let error = batch_ssh_command(dir.path()).err().unwrap();
        assert!(
            matches!(error, Failure::Other(ref why) if why.contains("empty")),
            "refused"
        );
    }
}
