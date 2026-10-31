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

/// `url` as it may be shown: the userinfo of a `scheme://` URL (a user, or
/// `user:token`) becomes `***`, keeping the scheme, host and path. A URL may
/// carry a credential, and every message and printed command can reach a
/// log or a transcript. Only git itself receives the real URL. Other forms,
/// such as a path or scp-style `host:path`, carry no userinfo and are
/// returned unchanged.
#[must_use]
pub fn display_url(url: &str) -> std::borrow::Cow<'_, str> {
    let Some(start) = url.find("://").map(|at| at + 3) else {
        return std::borrow::Cow::Borrowed(url);
    };
    let rest = &url[start..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    match authority.rfind('@') {
        Some(at) => std::borrow::Cow::Owned(format!("{}***{}", &url[..start], &rest[at..])),
        None => std::borrow::Cow::Borrowed(url),
    }
}

/// Redact URL userinfo in a diagnostic that may quote several remote URLs.
/// Keep the original text for protocol parsing; use this only for display.
#[must_use]
pub fn display_urls(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    let mut copied = 0;
    for (at, _) in text.match_indices("://") {
        let start = at + 3;
        if start < copied {
            continue;
        }
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '/' | '?' | '#'))
            .unwrap_or(rest.len());
        if let Some(userinfo) = rest[..end].rfind('@') {
            shown.push_str(&text[copied..start]);
            shown.push_str("***");
            copied = start + userinfo;
        }
    }
    shown.push_str(&text[copied..]);
    shown
}

/// `git ls-remote <args>`, never interactive and bounded by
/// [`LS_REMOTE_DEADLINE`] per attempt and [`MAX_ANSWER_BYTES`]. A default
/// advertisement (`--symref`) retries only timeouts, at most twice, with a
/// 100 ms backoff. Every other failure returns immediately.
///
/// # Errors
///
/// Returns why the remote could not be asked: git would not start, the
/// answer was too large or too slow, or `ls-remote` failed.
pub fn ls_remote(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut all = vec!["ls-remote"];
    all.extend_from_slice(args);
    let retries = if args.contains(&"--symref") { 2 } else { 0 };
    ls_remote_attempts(args, retries, || run(root, &all, LS_REMOTE_DEADLINE))
}

fn ls_remote_attempts(
    args: &[&str],
    retries: usize,
    mut attempt: impl FnMut() -> Result<String, Failure>,
) -> Result<String, String> {
    for number in 0..=retries {
        match attempt() {
            Err(Failure::Timeout(_)) if number < retries => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(Failure::Timeout(deadline)) => {
                return Err(format!(
                    "timeout: no answer within {}s per attempt ({} attempts)",
                    deadline.as_secs(),
                    number + 1
                ))
            }
            Ok(answer) if args.contains(&"--exit-code") && answer.is_empty() => {
                return Err(
                    "`git ls-remote --exit-code` returned success without a matching ref".into(),
                );
            }
            Ok(answer) => return Ok(answer),
            // With --exit-code, no matching refs is still a complete answer.
            Err(Failure::Exit(Some(2))) if args.contains(&"--exit-code") => {
                return Ok(String::new())
            }
            Err(Failure::Exit(_)) => {
                return Err(
                    "`git ls-remote` failed (unreachable, no credentials, or no such repository)"
                        .into(),
                )
            }
            Err(Failure::Authentication) => return Err("remote authentication failed".into()),
            Err(Failure::Other(why)) => return Err(why),
        }
    }
    unreachable!("the last attempt always returns")
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
            Failure::Exit(_) => format!("`git fetch {}` failed", display_url(url)),
            Failure::Authentication => "remote authentication failed".into(),
            Failure::Timeout(deadline) => format!("no answer within {}s", deadline.as_secs()),
            Failure::Other(why) => why,
        })
}

enum Failure {
    /// Git ran and exited unsuccessfully.
    Exit(Option<i32>),
    Authentication,
    Timeout(Duration),
    Other(String),
}

/// The transports [`advertises_any_ref`] may use. `GIT_ALLOW_PROTOCOL`
/// overrides every `protocol.*.allow` setting, so no configuration enables
/// `ext::` or another remote helper, and `git://`, whose proxy is a
/// configured command, is left out.
pub const SAFE_PROTOCOLS: &str = "file:http:https:ssh";

/// The SSH command [`advertises_any_ref`] runs: plain `ssh`, reading no
/// configuration file and never prompting.
const HERMETIC_SSH: &str = "ssh -F none -o BatchMode=yes";

/// The only inherited variables [`advertises_any_ref`] passes on: program
/// lookup, the home that holds SSH keys and known hosts, the SSH agent,
/// temporary space, the Windows system variables, and network proxies,
/// which name a host, never a command.
pub const KEPT_ENVIRONMENT: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "SystemRoot",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SSH_AUTH_SOCK",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// Whether the repository at `url` advertises any ref. The guards and the
/// commit hooks ask it on every command while a remote's tracking
/// namespace is empty, to tell a remote that holds nothing yet from one
/// whose tracking refs are missing here (TSK-1077), so the question runs
/// no command any configuration names. It runs outside the repository,
/// with no global or system configuration (so no credential helper,
/// `core.sshCommand`, `insteadOf` or upload command applies), plain `ssh`
/// that reads no configuration file, only [`SAFE_PROTOCOLS`] and git's own
/// upload command, and a clean environment ([`KEPT_ENVIRONMENT`]), so no
/// inherited git, SSH or proxy-command variable applies.
/// A remote that needs credentials or a configured SSH setup therefore
/// cannot be asked, and the caller refuses with the fetch route.
///
/// `url` is the remote's configured URL; a relative local path is resolved
/// against `root`, as git resolves it there. A URL git could read as an
/// option is refused.
///
/// # Errors
///
/// Returns why the remote could not be asked.
pub fn advertises_any_ref(root: &Path, url: &str) -> Result<bool, String> {
    if url.is_empty() || url.starts_with('-') {
        return Err("the remote URL is empty or starts with '-'".into());
    }
    let resolved = if is_local_path(url) && Path::new(url).is_relative() {
        root.join(url).into_os_string()
    } else {
        std::ffi::OsString::from(url)
    };
    // An empty directory outside any repository: git reads no repository
    // configuration, and discovery stops at its parent.
    let outside = tempfile::tempdir()
        .map_err(|error| format!("cannot create a directory to ask from: {error}"))?;
    let ceiling = outside
        .path()
        .parent()
        .map_or_else(|| outside.path().to_path_buf(), Path::to_path_buf);
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let args = ["ls-remote", "--upload-pack=git-upload-pack"];
    let answer = ls_remote_attempts(&args, 0, || {
        // A clean environment: no inherited git, SSH or proxy-command
        // variable (`GIT_NAMESPACE` would select which refs a local remote
        // advertises, review round 02 F4); only what finding the programs,
        // the user's SSH agent and keys, and a network proxy need passes.
        let mut command = crate::git::judging_command();
        command.env_clear();
        for name in KEPT_ENVIRONMENT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .current_dir(outside.path())
            .args(args)
            .arg(&resolved)
            .env("GIT_CEILING_DIRECTORIES", &ceiling)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", null)
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_ALLOW_PROTOCOL", SAFE_PROTOCOLS)
            .env("GIT_SSH_COMMAND", HERMETIC_SSH)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "false")
            .env("SSH_ASKPASS", "false")
            .env("SSH_ASKPASS_REQUIRE", "never");
        bounded(command, LS_REMOTE_DEADLINE)
    })?;
    // Git ends each advertised ref with a newline; any other byte is a ref.
    Ok(answer.split('\n').any(|line| !line.is_empty()))
}

/// Whether git reads `url` as a local path: no `scheme://`, and no `:`
/// before the first `/` (the scp-like `host:path` form), except a Windows
/// drive letter.
fn is_local_path(url: &str) -> bool {
    if url.contains("://") {
        return false;
    }
    match url.find(':') {
        None => true,
        Some(colon) => {
            url[..colon].contains('/')
                || (cfg!(windows) && colon == 1 && url.as_bytes()[0].is_ascii_alphabetic())
        }
    }
}

fn run(root: &Path, args: &[&str], deadline: Duration) -> Result<String, Failure> {
    let mut command = crate::git::judging_command();
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false")
        .env("SSH_ASKPASS", "false")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GIT_SSH_COMMAND", batch_ssh_command(root)?);
    bounded(command, deadline)
}

/// Run `command` with no input, within `deadline` (then its whole process
/// group is killed) and [`MAX_ANSWER_BYTES`], and return its answer.
fn bounded(mut command: std::process::Command, deadline: Duration) -> Result<String, Failure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    // Read diagnostics only to classify authentication failures. Never return
    // remote stderr: a helper or URL could include credential material.
    let stderr = child.stderr.take();
    let diagnostics = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = match stderr {
            Some(stderr) => stderr.take(64 << 10).read_to_end(&mut bytes),
            None => Ok(0),
        };
        read.map(|_| bytes)
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout
            .take(MAX_ANSWER_BYTES as u64 + 1)
            .read_to_end(&mut bytes);
        let _ = sender.send(read.map(|_| bytes));
    });
    let until = Instant::now() + deadline;
    let timed_out = || Failure::Timeout(deadline);
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
    // A descendant may retain stderr after git exits. Keep git unreaped
    // until both pipes finish, so its group ID cannot be reused before
    // timeout cleanup, and keep the same deadline for this pipe too.
    while !diagnostics.is_finished() {
        if Instant::now() >= until {
            kill_group(&mut child);
            return Err(timed_out());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Unreadable diagnostics cannot prove an authentication failure, so a
    // failed exit is then reported as a plain one, which still refuses.
    let authentication = match diagnostics.join() {
        Ok(Ok(bytes)) => authentication_failed(&bytes),
        Ok(Err(_)) | Err(_) => false,
    };
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
        if authentication {
            return Err(Failure::Authentication);
        }
        return Err(Failure::Exit(status.code()));
    }
    text_answer(&bytes)
}

/// Whether git's diagnostics carry an authentication marker. The bytes are
/// matched as ASCII and never decoded: the text is not shown to anyone.
fn authentication_failed(diagnostics: &[u8]) -> bool {
    let bytes = diagnostics.to_ascii_lowercase();
    [
        "authentication failed",
        "permission denied",
        "could not read username",
        "terminal prompts disabled",
        "http basic: access denied",
        "returned error: 401",
    ]
    .iter()
    .any(|marker| {
        bytes
            .windows(marker.len())
            .any(|window| window == marker.as_bytes())
    })
}

/// The answer as text, one line per advertised ref.
///
/// Every advertised ref must decode; otherwise callers cannot prove the
/// destination's complete ref set and the query refuses.
fn text_answer(bytes: &[u8]) -> Result<String, Failure> {
    String::from_utf8(bytes.to_vec())
        .map_err(|error| Failure::Other(format!("advertised refs are not valid UTF-8: {error}")))
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
            .map_err(|error| Failure::Other(format!("cannot read core.sshCommand: {error}")))?;
        if out.status.success() {
            // `--null` frames the value with a NUL, so a value that ends in a
            // carriage return keeps it.
            let framed = out.stdout.strip_suffix(&[0]).ok_or_else(|| {
                Failure::Other("cannot read core.sshCommand: missing NUL framing".into())
            })?;
            let value = std::str::from_utf8(framed)
                .map_err(|_| not_utf8("core.sshCommand"))?
                .to_string();
            configured = Some(value);
        } else if out.status.code() != Some(1) {
            return Err(Failure::Other(format!(
                "cannot read core.sshCommand: git config failed ({})",
                out.status
            )));
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

    #[test]
    fn diagnostics_redact_every_url_without_changing_other_text() {
        let text = "fatal: 'https://u:FAKE@one.invalid/x'\nTo ssh://FAKE@two.invalid\nhttps://three.invalid/u@path and git@four.invalid:path";
        assert_eq!(display_urls(text), "fatal: 'https://***@one.invalid/x'\nTo ssh://***@two.invalid\nhttps://three.invalid/u@path and git@four.invalid:path");
        assert_eq!(display_urls("plain diagnostic"), "plain diagnostic");
    }

    #[test]
    fn command_line_url_rewrite_cannot_redirect_judging_queries() {
        const CHILD: &str = "CODEFLOW_REMOTE_REWRITE_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = Path::new(&root);
            let destination = root.join("destination");
            let url = destination.to_str().unwrap();
            let answer = ls_remote(root, &[url, "refs/heads/expected"]).unwrap();
            let fetched = fetch_objects(root, url, &["refs/heads/expected"]);
            assert!(
                answer.contains("refs/heads/expected"),
                "query was redirected"
            );
            assert!(fetched.is_ok(), "fetch was redirected: {fetched:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let destination = dir.path().join("destination");
        let decoy = dir.path().join("decoy");
        crate::git::repo_with_refs(&destination, &[b"refs/heads/expected"]);
        crate::git::repo_with_refs(&decoy, &[b"refs/heads/decoy"]);
        let parameters = format!(
            "'url.{}.insteadOf={}'",
            decoy.display(),
            destination.display()
        );
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::remote_query::tests::command_line_url_rewrite_cannot_redirect_judging_queries",
                "--nocapture",
            ])
            .env(CHILD, dir.path())
            .env("GIT_CONFIG_PARAMETERS", parameters)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    fn inherited_stderr_keeps_the_query_within_its_deadline() {
        use std::os::unix::fs::PermissionsExt as _;
        if let Some(root) = std::env::var_os("CODEFLOW_STDERR_ONLY_CHILD") {
            let started = Instant::now();
            let result = run(
                Path::new(&root),
                &["ls-remote", "stub"],
                Duration::from_millis(200),
            );
            assert!(matches!(result, Err(Failure::Timeout(_))));
            assert!(started.elapsed() < Duration::from_secs(5));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        // Git exits and closes stdout while its descendant retains stderr.
        std::fs::write(&git, "#!/bin/sh\n/bin/sleep 60 >/dev/null &\nexit 0\n").unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = crate::git::process(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::remote_query::tests::inherited_stderr_keeps_the_query_within_its_deadline",
                "--nocapture",
            ])
            .env("CODEFLOW_STDERR_ONLY_CHILD", dir.path())
            .env("PATH", bin)
            .env("GIT_SSH_COMMAND", "ssh")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn display_url_hides_userinfo_and_keeps_scheme_host_and_path() {
        for (url, shown) in [
            (
                "https://u:FAKE_TOKEN_3b9@h.invalid/x.git",
                "https://***@h.invalid/x.git",
            ),
            ("https://FAKE_TOKEN_3b9@h.invalid", "https://***@h.invalid"),
            (
                "ssh://git@h.invalid:22/x.git?q=a@b",
                "ssh://***@h.invalid:22/x.git?q=a@b",
            ),
            ("https://a@b:FAKE@h.invalid/x", "https://***@h.invalid/x"),
            ("https://h.invalid/u@x.git", "https://h.invalid/u@x.git"),
            ("git@h.invalid:x.git", "git@h.invalid:x.git"),
            ("/srv/destination.git", "/srv/destination.git"),
        ] {
            assert_eq!(display_url(url), shown, "{url}");
        }
    }

    #[test]
    fn authentication_markers_match_raw_bytes_in_any_case() {
        assert!(authentication_failed(
            b"fatal: Authentication failed for 'https://example.invalid/'\n"
        ));
        assert!(authentication_failed(
            b"git@example.invalid: Permission denied (publickey).\xff\n"
        ));
        assert!(!authentication_failed(
            b"fatal: unable to access: Could not resolve host\n"
        ));
        assert!(!authentication_failed(b""));
    }

    #[test]
    fn default_lookup_retries_only_timeouts_and_stops_at_its_bound() {
        for (timeouts, expected, success) in [(0, 1, true), (1, 2, true), (3, 3, false)] {
            let mut calls = 0;
            let result = ls_remote_attempts(&["--symref"], 2, || {
                calls += 1;
                if calls <= timeouts {
                    Err(Failure::Timeout(LS_REMOTE_DEADLINE))
                } else {
                    Ok("answer".into())
                }
            });
            assert_eq!(calls, expected);
            assert_eq!(result.is_ok(), success);
        }
        let mut calls = 0;
        let result = ls_remote_attempts(&["--symref"], 2, || {
            calls += 1;
            Err(Failure::Authentication)
        });
        assert_eq!(calls, 1);
        assert!(result.unwrap_err().contains("authentication"));

        // A complete but unusable advertisement is a final answer, never
        // another network attempt, whether malformed or missing a default.
        for answer in [
            "malformed",
            "1111111111111111111111111111111111111111\trefs/heads/main\n",
        ] {
            let mut calls = 0;
            let result = ls_remote_attempts(&["--symref"], 2, || {
                calls += 1;
                Ok(answer.into())
            })
            .unwrap();
            assert!(crate::workgraph::release_line::from_advertisement("stub", &result).is_err());
            assert_eq!(calls, 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn r20_empty_remote_advertisement_is_not_proven_absence() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_R20_EMPTY_REMOTE";
        if let Some(root) = std::env::var_os(CHILD) {
            assert!(
                ls_remote(Path::new(&root), &["--exit-code", "origin", "refs/heads/x"]).is_err()
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        std::fs::write(
            &git,
            "#!/bin/sh\nif [ \"$3\" = config ]; then exit 1; fi\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::remote_query::tests::r20_empty_remote_advertisement_is_not_proven_absence",
                "--nocapture",
            ])
            .env(CHILD, dir.path())
            .env("PATH", bin)
            .env_remove("GIT_SSH_COMMAND")
            .env_remove("GIT_SSH")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn r20_failed_ssh_config_query_never_selects_default() {
        const CHILD: &str = "CODEFLOW_R20_SSH_QUERY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "git::remote_query::tests::r20_failed_ssh_config_query_never_selects_default",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env_remove("GIT_SSH_COMMAND")
                .env_remove("GIT_SSH")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        assert!(batch_ssh_command(dir.path()).is_ok());
        let path = repo.path().join("config");
        std::fs::write(path, "[broken\n").unwrap();
        assert!(
            batch_ssh_command(dir.path()).is_err(),
            "failed config must not choose ssh"
        );
    }

    #[test]
    fn r17_remote_no_matching_ref_is_an_empty_answer() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        let url = dir.path().to_str().unwrap();
        assert_eq!(
            ls_remote(dir.path(), &["--exit-code", url, "refs/heads/absent"]),
            Ok(String::new())
        );
        assert!(ls_remote(dir.path(), &["--exit-code", "./absent-repository"]).is_err());
        assert!(fetch_objects(dir.path(), url, &["refs/heads/absent"]).is_err());
    }

    /// Review finding on issue 79: advertised ref names were decoded lossily,
    /// so a valid `caf` plus U+FFFD resolved to the tip of the distinct branch
    /// whose last byte is invalid. An unreadable advertised set now refuses.
    #[test]
    fn an_advertised_name_that_is_not_utf8_refuses_the_query() {
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(
            dir.path(),
            &[b"refs/heads/caf\xe9", "refs/heads/caf\u{fffd}".as_bytes()],
        );
        let path = repo.workdir().unwrap().to_path_buf();
        assert!(ls_remote(&path, &["--heads", path.to_str().unwrap()]).is_err());
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
