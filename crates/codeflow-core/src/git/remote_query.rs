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

use crate::hooks::limits::{self, Limit};

const fn key_of(limit: Limit) -> &'static str {
    match limit.key {
        Some(key) => key,
        None => panic!("a policy-set limit names its key"),
    }
}

/// The policy key that sets how long one `ls-remote` attempt may take.
pub const TIMEOUT_KEY: &str = key_of(limits::REMOTE_QUERY_TIMEOUT);
/// The policy key that sets how many attempts a timed-out `ls-remote` gets.
pub const ATTEMPTS_KEY: &str = key_of(limits::REMOTE_QUERY_ATTEMPTS);
/// The policy key that sets how long a fetch of the objects a judge needs
/// may take.
pub const FETCH_TIMEOUT_KEY: &str = key_of(limits::REMOTE_FETCH_TIMEOUT);
/// The shipped [`TIMEOUT_KEY`] value, declared with its reason in
/// [`limits::REMOTE_QUERY_TIMEOUT`].
pub const DEFAULT_TIMEOUT_SECONDS: u64 = limits::REMOTE_QUERY_TIMEOUT.default;
/// The shipped [`ATTEMPTS_KEY`] value ([`limits::REMOTE_QUERY_ATTEMPTS`]).
pub const DEFAULT_ATTEMPTS: u64 = limits::REMOTE_QUERY_ATTEMPTS.default;
/// The shipped [`FETCH_TIMEOUT_KEY`] value ([`limits::REMOTE_FETCH_TIMEOUT`]).
pub const DEFAULT_FETCH_TIMEOUT_SECONDS: u64 = limits::REMOTE_FETCH_TIMEOUT.default;

/// Whose limits a query runs under, which its timeout message names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The policy keys, read from the trusted policy.
    Policy,
    /// A session guard's own lookup wait, which no policy key changes.
    Guard,
}

/// How long each remote question may take and how many attempts a timeout
/// allows (TSK-1301). On the pre-push and `codeflow ci` paths the values
/// come from `git.remote_query_timeout_seconds`,
/// `git.remote_query_attempts` and `git.remote_fetch_timeout_seconds` in
/// the policy the caller trusts: the default branch's committed policy,
/// never a pushed head's. A session guard runs inside a harness hook
/// timeout, so it uses [`QueryLimits::guard`] instead and fails closed
/// when the remote does not answer within that wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryLimits {
    timeout: Duration,
    attempts: u64,
    fetch_timeout: Duration,
    source: Source,
}

impl Default for QueryLimits {
    fn default() -> Self {
        Self::from_values(
            DEFAULT_TIMEOUT_SECONDS,
            DEFAULT_ATTEMPTS,
            DEFAULT_FETCH_TIMEOUT_SECONDS,
        )
    }
}

impl QueryLimits {
    /// The limits a policy sets. The loader refuses a value below 1, so
    /// the floor here only keeps a hand-built policy from asking nothing.
    #[must_use]
    pub fn from_git(policy: &crate::hooks::policy::GitPolicy) -> Self {
        Self::from_values(
            policy.remote_query_timeout_seconds,
            policy.remote_query_attempts,
            policy.remote_fetch_timeout_seconds,
        )
    }

    fn from_values(timeout_seconds: u64, attempts: u64, fetch_seconds: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_seconds.max(1)),
            attempts: attempts.max(1),
            fetch_timeout: Duration::from_secs(fetch_seconds.max(1)),
            source: Source::Policy,
        }
    }

    /// A session guard's limits: one attempt within
    /// [`GUARD_REMOTE_QUERY_SECS`](crate::hooks::guard_deadline::GUARD_REMOTE_QUERY_SECS),
    /// which fits inside the guard's own deadline and the harness hook
    /// timeout, whatever the policy keys say. A guard that cannot get an
    /// answer in time refuses; it never waits for the harness to kill it.
    #[must_use]
    pub fn guard() -> Self {
        let wait = Duration::from_secs(crate::hooks::guard_deadline::GUARD_REMOTE_QUERY_SECS);
        Self {
            timeout: wait,
            attempts: 1,
            fetch_timeout: wait,
            source: Source::Guard,
        }
    }

    /// The limits the local policy authority sets: the policy committed on
    /// the remote's default branch as last fetched
    /// ([`crate::hooks::landed_policy::load_local`], which never asks the
    /// remote).
    ///
    /// # Errors
    ///
    /// No default branch supplies a policy here: the remote's tracking refs
    /// are missing, the authority cannot be read, or the repository is in a
    /// labelled state with no remote, whose policy is `HEAD`'s and so the
    /// change's own. The caller decides whether the shipped defaults stand
    /// in.
    pub fn from_authority(root: &Path) -> Result<Self, String> {
        let landed = crate::hooks::landed_policy::load_local(root)?;
        if landed.authority.is_none() {
            return Err(format!(
                "no default branch supplies a policy: {}",
                landed.source
            ));
        }
        Ok(Self::from_git(&landed.policy.git))
    }

    /// [`Self::from_authority`], or the shipped defaults when the authority
    /// cannot be read. Never the working copy's policy, which the change
    /// being judged can write.
    #[must_use]
    pub fn from_authority_or_default(root: &Path) -> Self {
        Self::from_authority(root).unwrap_or_default()
    }

    /// Limits measured in any unit, for tests that scale the clock down.
    #[cfg(test)]
    #[must_use]
    pub fn scaled(timeout: Duration, attempts: u64) -> Self {
        Self {
            timeout,
            attempts,
            fetch_timeout: timeout,
            source: Source::Policy,
        }
    }

    /// How long one `ls-remote` attempt may take.
    #[must_use]
    pub fn timeout(self) -> Duration {
        self.timeout
    }

    /// How many attempts a timeout allows, the first included.
    #[must_use]
    pub fn attempts(self) -> u64 {
        self.attempts
    }

    /// How long a fetch of the objects a judge needs may take.
    #[must_use]
    pub fn fetch_timeout(self) -> Duration {
        self.fetch_timeout
    }

    /// What set a timed-out query's limits, for its message.
    fn set_by(self) -> String {
        match self.source {
            Source::Policy => {
                format!("{TIMEOUT_KEY} and {ATTEMPTS_KEY} in the policy this run reads set these")
            }
            Source::Guard => format!(
                "a session guard waits at most {}s for each such lookup, whatever {TIMEOUT_KEY} says, and its {}s deadline bounds the whole command inside the harness's {}s hook timeout",
                crate::hooks::guard_deadline::GUARD_REMOTE_QUERY_SECS,
                crate::hooks::guard_deadline::GUARD_DEADLINE_SECS,
                crate::hooks::guard_deadline::GUARD_HOOK_TIMEOUT_SECS,
            ),
        }
    }
}

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

/// The reason [`ls_remote`] and [`fetch_objects`] give when the remote
/// refused for want of a credential, so a caller can name the fix.
pub const AUTHENTICATION_FAILED: &str = "remote authentication failed";

/// `git ls-remote <args>`, never interactive and bounded by `limits` per
/// attempt and by [`MAX_ANSWER_BYTES`]. Only a timeout is retried, up to
/// the attempts `limits` allows, with a 100 ms backoff; every other
/// failure returns at once, and a destination that never answers still
/// refuses once the waits run out. A wait too large for the clock never runs
/// out.
///
/// # Errors
///
/// Returns why the remote could not be asked: git would not start, the
/// answer was too large or too slow, or `ls-remote` failed. A timeout
/// names the limits used, the time measured and the policy keys that set
/// them.
pub fn ls_remote(root: &Path, args: &[&str], limits: QueryLimits) -> Result<String, String> {
    ls_remote_with(root, args, limits, None)
}

/// As [`ls_remote`], sending `auth` with every request to its destination
/// (TSK-1252).
///
/// # Errors
///
/// As [`ls_remote`].
pub fn ls_remote_with(
    root: &Path,
    args: &[&str],
    limits: QueryLimits,
    auth: Option<&HttpAuth>,
) -> Result<String, String> {
    let mut all = vec!["ls-remote"];
    all.extend_from_slice(args);
    ls_remote_attempts(args, limits, || run(root, &all, limits.timeout, auth))
}

/// A read credential for one destination: the hosted run's token, sent as
/// an HTTP `Authorization` header to that destination's URL only (TSK-1252).
///
/// Git receives it through its configuration environment for one process,
/// as `http.<url>.extraHeader`, so it is never written to git config, the
/// credential store or a file, and it never appears in a command line. The
/// header is scoped to the exact destination URL, and redirects are not
/// followed, so no other host receives it. `Debug` never shows the token.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpAuth {
    url: String,
    header: String,
}

impl std::fmt::Debug for HttpAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpAuth")
            .field("url", &self.url)
            .field("header", &"***")
            .finish()
    }
}

impl HttpAuth {
    /// The run's `token` for `url`, bound to the repository the run
    /// belongs to: `url` must be `<server>/<repository>` (or with `.git`),
    /// where `server` and `repository` are the values the hosted runner
    /// sets (`GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`), and it must use
    /// HTTPS. Plain HTTP is accepted only for a loopback host, where the
    /// token never leaves the machine.
    ///
    /// # Errors
    ///
    /// Returns why the token is not sent to `url`. No message contains the
    /// token.
    pub fn for_run(url: &str, token: &str, server: &str, repository: &str) -> Result<Self, String> {
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err("the token is empty or holds a character a token cannot hold".into());
        }
        let server = server.trim_end_matches('/');
        let repository = repository.trim_matches('/');
        if server.is_empty() || repository.is_empty() {
            return Err("the run names no server URL or repository".into());
        }
        let expected = format!("{server}/{repository}");
        if url != expected && url != format!("{expected}.git") {
            return Err(format!(
                "the destination {} is not this run's repository {expected}",
                display_url(url)
            ));
        }
        let host = url
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
            .unwrap_or_default();
        let loopback = host.rsplit_once(':').map_or(host, |(name, port)| {
            if port.bytes().all(|byte| byte.is_ascii_digit()) {
                name
            } else {
                host
            }
        });
        let secure = url.starts_with("https://")
            || (url.starts_with("http://")
                && matches!(loopback, "127.0.0.1" | "localhost" | "[::1]"));
        if !secure || host.contains('@') {
            return Err(format!(
                "the destination {} is not an HTTPS URL without user information",
                display_url(url)
            ));
        }
        use base64::Engine as _;
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{token}").as_bytes());
        Ok(Self {
            url: url.to_string(),
            header: format!("AUTHORIZATION: basic {basic}"),
        })
    }

    /// The destination URL this credential is for.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Give `command` the credential for this destination only: the
    /// header scoped to its URL, no redirect followed, and no inherited
    /// trace variable that could print request headers.
    fn apply(&self, command: &mut std::process::Command) {
        for name in [
            "GIT_TRACE",
            "GIT_TRACE_CURL",
            "GIT_TRACE_CURL_NO_DATA",
            "GIT_TRACE_PACKET",
            "GIT_CURL_VERBOSE",
            "GIT_TRACE2",
            "GIT_TRACE2_EVENT",
            "GIT_TRACE2_PERF",
        ] {
            command.env_remove(name);
        }
        command
            .env("GIT_TRACE_REDACT", "1")
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", format!("http.{}.extraHeader", self.url))
            .env("GIT_CONFIG_VALUE_0", &self.header)
            .env("GIT_CONFIG_KEY_1", "http.followRedirects")
            .env("GIT_CONFIG_VALUE_1", "false");
    }
}

/// A duration as messages show it: whole seconds without a fraction.
fn seconds(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

fn ls_remote_attempts(
    args: &[&str],
    limits: QueryLimits,
    mut attempt: impl FnMut() -> Result<String, Failure>,
) -> Result<String, String> {
    let started = Instant::now();
    let attempts = limits.attempts.max(1);
    for number in 1..=attempts {
        match attempt() {
            Err(Failure::Timeout(_)) if number < attempts => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(Failure::Timeout(deadline)) => {
                return Err(format!(
                    "timeout: no answer within {} per attempt ({number} {}, {:.1}s measured); {}",
                    seconds(deadline),
                    if number == 1 { "attempt" } else { "attempts" },
                    started.elapsed().as_secs_f64(),
                    limits.set_by(),
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
            Err(Failure::Authentication) => return Err(AUTHENTICATION_FAILED.into()),
            Err(Failure::Other(why)) => return Err(why),
        }
    }
    unreachable!("the last attempt always returns")
}

/// Fetch `refs` from `url` into the object store only, within the fetch
/// wait `limits` sets: no tracking ref, tag or `FETCH_HEAD` is written, so
/// nothing a later check reads as authority changes.
///
/// # Errors
///
/// Returns why the fetch did not complete.
pub fn fetch_objects(
    root: &Path,
    url: &str,
    refs: &[&str],
    limits: QueryLimits,
) -> Result<(), String> {
    fetch_objects_with(root, url, refs, limits, None)
}

/// As [`fetch_objects`], sending `auth` with every request when it is the
/// credential for `url` (TSK-1252).
///
/// # Errors
///
/// As [`fetch_objects`].
pub fn fetch_objects_with(
    root: &Path,
    url: &str,
    refs: &[&str],
    limits: QueryLimits,
    auth: Option<&HttpAuth>,
) -> Result<(), String> {
    let auth = auth.filter(|auth| auth.url() == url);
    let mut all = vec![
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-write-fetch-head",
        "--no-recurse-submodules",
        url,
    ];
    all.extend_from_slice(refs);
    run(root, &all, limits.fetch_timeout, auth)
        .map(|_| ())
        .map_err(|why| match why {
            Failure::Exit(_) => format!("`git fetch {}` failed", display_url(url)),
            Failure::Authentication => AUTHENTICATION_FAILED.into(),
            Failure::Timeout(deadline) => format!(
                "no answer within {}; {FETCH_TIMEOUT_KEY} in the policy this run reads sets this",
                seconds(deadline)
            ),
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
/// lookup, the SSH agent, temporary space, the Windows system variables,
/// and network proxies, which name a host, never a command. The home is
/// not passed on: git's HTTP transport reads a `.netrc` there whatever its
/// configuration says (review round 03 F5), so the question runs with an
/// empty one.
pub const KEPT_ENVIRONMENT: &[&str] = &[
    "PATH",
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
/// upload command, a clean environment ([`KEPT_ENVIRONMENT`]), so no
/// inherited git, SSH or proxy-command variable applies, and an empty home,
/// so no stored HTTP credential (`.netrc` or `_netrc`) is sent. Only SSH
/// keys, which prove possession without disclosing a secret, still apply.
/// A remote that needs other credentials or a configured SSH setup
/// therefore cannot be asked, and the caller refuses with the fetch route.
///
/// `url` is the remote's configured URL; a relative local path is resolved
/// against `root`, as git resolves it there. A URL git could read as an
/// option is refused.
///
/// # Errors
///
/// Returns why the remote could not be asked.
pub fn advertises_any_ref(root: &Path, url: &str, limits: QueryLimits) -> Result<bool, String> {
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
    let answer = ls_remote_attempts(&args, limits, || {
        // A clean environment: no inherited git, SSH or proxy-command
        // variable (`GIT_NAMESPACE` would select which refs a local remote
        // advertises, review round 02 F4); only what finding the programs,
        // the user's SSH agent and keys, and a network proxy need passes.
        // The home is the empty directory, so libcurl finds no `.netrc`
        // (review round 03 F5); OpenSSH finds keys through the account.
        let mut command = crate::git::judging_command();
        command.env_clear();
        for name in KEPT_ENVIRONMENT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .current_dir(outside.path())
            .env("HOME", outside.path())
            .env("USERPROFILE", outside.path())
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
        bounded(command, limits.timeout)
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

fn run(
    root: &Path,
    args: &[&str],
    deadline: Duration,
    auth: Option<&HttpAuth>,
) -> Result<String, Failure> {
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
    if let Some(auth) = auth {
        auth.apply(&mut command);
    }
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
    // A deadline past the clock's range never expires instead of
    // overflowing: the policy sets no ceiling on it, so a very large value
    // means effectively no limit, as the policy reference says (TSK-1301).
    let until = Instant::now().checked_add(deadline);
    let expired = || until.is_some_and(|until| Instant::now() >= until);
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
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !expired() => {}
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
        if expired() {
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
            Ok(None) if !expired() => std::thread::sleep(Duration::from_millis(20)),
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
            let answer =
                ls_remote(root, &[url, "refs/heads/expected"], QueryLimits::default()).unwrap();
            let fetched =
                fetch_objects(root, url, &["refs/heads/expected"], QueryLimits::default());
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
                None,
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

    /// TSK-1252 AC-3: the run's token is bound to the run's own repository
    /// over HTTPS (plain HTTP only on loopback), refuses a token that could
    /// inject a header, and never shows itself.
    #[test]
    fn the_run_token_binds_only_to_the_run_repository_over_https() {
        let token = "ghs_fakeToken123";
        let ok = HttpAuth::for_run(
            "https://github.com/owner/repo",
            token,
            "https://github.com",
            "owner/repo",
        )
        .unwrap();
        assert!(!format!("{ok:?}").contains(token));
        assert!(HttpAuth::for_run(
            "https://github.com/owner/repo.git",
            token,
            "https://github.com/",
            "owner/repo"
        )
        .is_ok());
        assert!(HttpAuth::for_run(
            "http://127.0.0.1:8080/owner/repo.git",
            token,
            "http://127.0.0.1:8080",
            "owner/repo"
        )
        .is_ok());
        for (url, server, repository, token) in [
            (
                "https://evil.example/owner/repo",
                "https://github.com",
                "owner/repo",
                token,
            ),
            (
                "https://github.com/other/repo",
                "https://github.com",
                "owner/repo",
                token,
            ),
            (
                "http://github.com/owner/repo",
                "http://github.com",
                "owner/repo",
                token,
            ),
            (
                "https://x@github.com/owner/repo",
                "https://x@github.com",
                "owner/repo",
                token,
            ),
            ("https://github.com/owner/repo", "", "", token),
            (
                "https://github.com/owner/repo",
                "https://github.com",
                "owner/repo",
                "",
            ),
            (
                "https://github.com/owner/repo",
                "https://github.com",
                "owner/repo",
                "a\r\nX: y",
            ),
        ] {
            let refused = HttpAuth::for_run(url, token, server, repository);
            assert!(refused.is_err(), "{url} {server} {repository}");
            if !token.is_empty() {
                assert!(!refused.unwrap_err().contains(token));
            }
        }
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

    /// TSK-1301: with no remote the local policy is `HEAD`'s, which the
    /// change being pushed can write, so it never sets the waits.
    #[test]
    fn a_policy_with_no_default_branch_never_sets_the_waits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/policy.json"),
            r#"{"git":{"remote_query_timeout_seconds":7777}}"#,
        )
        .unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".codeflow/policy.json")).unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let who = git2::Signature::now("t", "t@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &who, &who, "chore: x", &tree, &[])
            .unwrap();
        assert!(QueryLimits::from_authority(dir.path()).is_err());
        assert_eq!(
            QueryLimits::from_authority_or_default(dir.path()),
            QueryLimits::default()
        );
    }

    /// TSK-1301: a timeout names the limits used, the measured time and
    /// the policy keys that set them, with the attempt counted correctly.
    #[test]
    fn a_timeout_names_the_limits_the_measured_time_and_the_keys() {
        let limits = QueryLimits::default();
        let error = ls_remote_attempts(&["--symref"], limits, || {
            Err(Failure::Timeout(limits.timeout()))
        })
        .unwrap_err();
        assert!(
            error.starts_with("timeout: no answer within 30s per attempt (3 attempts, "),
            "{error}"
        );
        assert!(
            error.contains(
                "s measured); git.remote_query_timeout_seconds and git.remote_query_attempts"
            ),
            "{error}"
        );
        let guard = ls_remote_attempts(&[], QueryLimits::guard(), || {
            Err(Failure::Timeout(Duration::from_millis(250)))
        })
        .unwrap_err();
        assert!(
            guard.contains("within 0.2s per attempt (1 attempt, "),
            "{guard}"
        );
        assert!(
            guard.contains(
                "a session guard waits at most 10s for each such lookup, whatever git.remote_query_timeout_seconds says, and its 20s deadline bounds the whole command"
            ),
            "{guard}"
        );
    }

    /// TSK-1301: the destination probe against a fake `git` whose answer
    /// takes as long as GitHub's measured 11.3 s, with every time scaled
    /// down 50 times so the test stays fast. The old fixed limit (10 s,
    /// three attempts) refuses that answer; the shipped default (30 s)
    /// takes it; the largest values the policy accepts neither wrap nor
    /// panic.
    #[cfg(unix)]
    #[test]
    fn a_slow_destination_answers_within_the_policy_timeout() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_SLOW_DESTINATION_CHILD";
        const SCALE: u32 = 50;
        if let Some(root) = std::env::var_os(CHILD) {
            let root = Path::new(&root);
            let args = ["--symref", "stub", "HEAD", "refs/heads/*"];
            let old = QueryLimits::scaled(Duration::from_secs(10) / SCALE, 3);
            let refused = ls_remote(root, &args, old).unwrap_err();
            assert!(refused.contains("(3 attempts, "), "{refused}");
            assert!(refused.contains(TIMEOUT_KEY), "{refused}");
            let shipped = QueryLimits::default();
            let new = QueryLimits::scaled(shipped.timeout() / SCALE, shipped.attempts());
            let answer = ls_remote(root, &args, new).unwrap();
            assert!(answer.contains("refs/heads/main"), "{answer}");
            let widest = QueryLimits::from_values(u64::MAX, u64::MAX, u64::MAX);
            assert!(ls_remote(root, &args, widest).is_ok());
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        // 11.3 s / 50 = 0.226 s.
        std::fs::write(
            &git,
            "#!/bin/sh\nsleep 0.226\nprintf 'ref: refs/heads/main\\tHEAD\\n1111111111111111111111111111111111111111\\trefs/heads/main\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let out = crate::git::process(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::remote_query::tests::a_slow_destination_answers_within_the_policy_timeout",
                "--nocapture",
            ])
            .env(CHILD, dir.path())
            .env("PATH", path)
            .env("GIT_SSH_COMMAND", "ssh")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn default_lookup_retries_only_timeouts_and_stops_at_its_bound() {
        for (timeouts, expected, success) in [(0, 1, true), (1, 2, true), (3, 3, false)] {
            let mut calls = 0;
            let result = ls_remote_attempts(&["--symref"], QueryLimits::default(), || {
                calls += 1;
                if calls <= timeouts {
                    Err(Failure::Timeout(QueryLimits::default().timeout()))
                } else {
                    Ok("answer".into())
                }
            });
            assert_eq!(calls, expected);
            assert_eq!(result.is_ok(), success);
        }
        let mut calls = 0;
        let result = ls_remote_attempts(&["--symref"], QueryLimits::default(), || {
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
            let result = ls_remote_attempts(&["--symref"], QueryLimits::default(), || {
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
            assert!(ls_remote(
                Path::new(&root),
                &["--exit-code", "origin", "refs/heads/x"],
                QueryLimits::default()
            )
            .is_err());
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
            ls_remote(
                dir.path(),
                &["--exit-code", url, "refs/heads/absent"],
                QueryLimits::default()
            ),
            Ok(String::new())
        );
        assert!(ls_remote(
            dir.path(),
            &["--exit-code", "./absent-repository"],
            QueryLimits::default()
        )
        .is_err());
        assert!(fetch_objects(
            dir.path(),
            url,
            &["refs/heads/absent"],
            QueryLimits::default()
        )
        .is_err());
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
        assert!(ls_remote(
            &path,
            &["--heads", path.to_str().unwrap()],
            QueryLimits::default()
        )
        .is_err());
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
