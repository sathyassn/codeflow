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
    let mut all = vec!["ls-remote"];
    all.extend_from_slice(args);
    ls_remote_attempts(args, limits, || run(root, &all, limits.timeout))
}

/// Refuse, as a destination that cannot be verified, a query of `url` that
/// the local configuration would send somewhere other than `url` itself
/// (TSK-327). The pre-push hook is given the URL the push uses, after git
/// applied its rewrites; `git ls-remote <url>` and `git fetch <url>` read
/// the same configuration again and would apply a second rewrite, so the
/// answer could come from a repository the push never reaches. The check
/// reads the configuration the query itself reads (repository, worktree,
/// global and system files with their includes, through
/// [`crate::git::judging_command`], so no command-line overlay counts) and
/// refuses when any of these would apply to `url`:
///
/// - a `url.<base>.insteadOf` value that is a prefix of `url`, which git
///   rewrites before it connects; an empty value is a prefix of every URL;
/// - a `remote.<url>.*` section, which makes git read `url` as a configured
///   remote with its own URL, upload command or helper;
/// - a `remotes/<url>` or `branches/<url>` file, when `url` has no directory
///   separator and git could read it as a legacy remote name.
///
/// A configuration that cannot be read within the caller's `limits` refuses
/// too, so a stalled read never holds a guard past its lookup wait. Nothing
/// is rewritten here: the query either asks `url` exactly or does not ask.
///
/// # Errors
///
/// Returns what would redirect the query, or why the configuration could not
/// be read.
pub fn require_unredirected(root: &Path, url: &str, limits: QueryLimits) -> Result<(), String> {
    let shown = display_url(url);
    let refuse = |why: String| {
        format!(
            "the destination {shown} cannot be verified: {why}; the hook asks only the exact URL the push uses"
        )
    };
    // Git would read such a URL as an option of the query.
    if url.is_empty() || url.starts_with('-') {
        return Err(refuse("the URL is empty or starts with '-'".to_string()));
    }
    if let Some(overlay) = destination_overlay(root, url, limits).map_err(refuse)? {
        return Err(refuse(format!(
            "{overlay} would send the destination query somewhere other than this URL; remove it, or push to the URL it names"
        )));
    }
    Ok(())
}

/// What in the configuration at `root` would redirect a query of `url`, if
/// anything (see [`require_unredirected`]). The listing is read within
/// `limits` and [`MAX_ANSWER_BYTES`].
fn destination_overlay(
    root: &Path,
    url: &str,
    limits: QueryLimits,
) -> Result<Option<String>, String> {
    let mut command = crate::git::judging_command();
    command
        .arg("-C")
        .arg(root)
        .args(["config", "-z", "--list"])
        .env("LC_ALL", "C");
    let listed = bounded_raw(command, limits.timeout())
        .map_err(|why| format!("cannot read the git configuration: {why}"))?;
    if let Some(overlay) = overlay_in(&listed, url) {
        return Ok(Some(overlay));
    }
    legacy_remote_file(root, url)
}

/// [`destination_overlay`] over `git config -z --list` output: entries end
/// in NUL, and a key is followed by a newline and its value, or by nothing
/// for a key with no value. Section and variable names are lowercase there;
/// a subsection keeps its case, as git compares it. Any `remote.<url>`
/// setting is found by the shared
/// [`url_section_in`](super::remote_identity::url_section_in).
fn overlay_in(listed: &[u8], url: &str) -> Option<String> {
    for entry in listed.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let (key, value) = match entry.iter().position(|byte| *byte == b'\n') {
            Some(at) => (&entry[..at], Some(&entry[at + 1..])),
            None => (entry, None),
        };
        if let Some(base) = key
            .strip_prefix(b"url.")
            .and_then(|rest| rest.strip_suffix(b".insteadof"))
        {
            // Git refuses a valueless rewrite when it reads remotes, so the
            // query would not run as asked either. An empty value is a
            // prefix of every URL, and git applies it.
            let rewrites = value.is_none_or(|prefix| url.as_bytes().starts_with(prefix));
            if rewrites {
                // Shown only: the base is escaped and any userinfo hidden.
                let shown = crate::git::GitName::from_bytes(base).display().to_string();
                return Some(format!(
                    "the setting url.{}.insteadOf",
                    display_urls(&shown)
                ));
            }
        }
    }
    super::remote_identity::url_section_in(listed, url)
}

/// A legacy `remotes/<url>` or `branches/<url>` file in the common
/// directory, which git reads when `url` is a valid remote name: not empty,
/// not `.` or `..`, and free of directory separators.
fn legacy_remote_file(root: &Path, url: &str) -> Result<Option<String>, String> {
    let separator = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    if url.is_empty() || url == "." || url == ".." || url.contains(separator) {
        return Ok(None);
    }
    let repo = git2::Repository::discover(root)
        .map_err(|error| format!("cannot open the repository: {}", error.message()))?;
    for folder in ["remotes", "branches"] {
        let file = repo.commondir().join(folder).join(url);
        // Absent only when proven through its ancestors; anything else that
        // stops the read refuses, since git might read the file.
        match crate::absence::symlink_metadata_optional(&file) {
            Ok(None) => {}
            Ok(Some(_)) => {
                return Ok(Some(format!(
                    "the legacy remote file {folder}/{}",
                    display_url(url)
                )))
            }
            Err(error) => {
                return Err(format!(
                    "cannot read the legacy remote file {folder}/{}: {error}",
                    display_url(url)
                ))
            }
        }
    }
    Ok(None)
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
/// nothing a later check reads as authority changes. A fetch the local
/// configuration would send elsewhere is refused before it runs
/// ([`require_unredirected`]).
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
    require_unredirected(root, url, limits)?;
    let mut all = vec![
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-write-fetch-head",
        "--no-recurse-submodules",
        url,
    ];
    all.extend_from_slice(refs);
    run(root, &all, limits.fetch_timeout)
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
        .env("GIT_SSH_COMMAND", batch_ssh_command(root, deadline)?);
    bounded(command, deadline)
}

/// Run `command` with no input, within `deadline` (then its whole process
/// group is killed) and [`MAX_ANSWER_BYTES`], and return its answer.
fn bounded(command: std::process::Command, deadline: Duration) -> Result<String, Failure> {
    text_answer(&bounded_bytes(command, deadline)?)
}

/// [`bounded`], returning the answer's bytes undecoded.
fn bounded_bytes(command: std::process::Command, deadline: Duration) -> Result<Vec<u8>, Failure> {
    let (status, bytes, diagnostics) = bounded_run(command, deadline)?;
    if !status.success() {
        if diagnostics.as_deref().is_some_and(authentication_failed) {
            return Err(Failure::Authentication);
        }
        return Err(Failure::Exit(status.code()));
    }
    Ok(bytes)
}

/// A bounded run's exit status, answer and diagnostics, when read.
type Ran = (std::process::ExitStatus, Vec<u8>, Option<Vec<u8>>);

/// Run `command` with no input, within `deadline` (then its whole process
/// group is killed) and [`MAX_ANSWER_BYTES`]: git's exit status, its
/// answer, and up to 64 KiB of its diagnostics when they could be read
/// before the stderr grace ended. Only a timeout, an oversized answer or a
/// failure to run or read is an error.
fn bounded_run(command: std::process::Command, deadline: Duration) -> Result<Ran, Failure> {
    bounded_run_input(command, None, deadline, Diagnostics::Brief)
}

/// How much of a bounded child's stderr is kept, and how long it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diagnostics {
    /// Up to 64 KiB, read for [`STDERR_GRACE`] after git exits: a helper
    /// git started can hold stderr open, and git's own stderr only
    /// classifies a failure.
    Brief,
    /// All of it, up to [`MAX_ANSWER_BYTES`], read within the deadline: a
    /// codeflow child prints its findings there.
    Full,
}

/// [`bounded_run`], with `input` written to git's stdin from its own thread
/// when given ([`crate::git::spawn_with_input_stopping`]), so a stdin-fed
/// read such as `cat-file --batch` has the same deadline and answer cap.
/// The write must end within the deadline too: git is reaped only after it
/// ends, so a descendant that keeps stdin open and never reads is ended
/// with git's group at the deadline. The writer is then joined for at most
/// [`STDERR_GRACE`] and otherwise left to its closed pipe, never waited on.
fn bounded_run_input(
    mut command: std::process::Command,
    input: Option<&[u8]>,
    deadline: Duration,
    diagnostics: Diagnostics,
) -> Result<Ran, Failure> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let started = match input {
        Some(input) => {
            crate::git::spawn_with_input_stopping(&mut command, input.to_vec(), |child| {
                kill_group(child);
            })
            .map(|(child, writer)| (child, Some(writer)))
        }
        None => command
            .stdin(Stdio::null())
            .spawn()
            .map(|child| (child, None)),
    };
    let (mut child, writer) =
        started.map_err(|error| Failure::Other(format!("could not start git: {error}")))?;
    let ran = bounded_wait(&mut child, writer.as_ref(), deadline, diagnostics);
    if let Some(writer) = writer {
        // An answer means the write already ended. Every failure above
        // ended git's group, which closes the pipe, so the write ends at
        // once unless a process outside the group holds it.
        let written = writer.finish_within(STDERR_GRACE);
        if let (Ok(_), Some(written)) = (&ran, written) {
            written
                .map_err(|error| Failure::Other(format!("writing its input failed: {error}")))?;
        }
    }
    ran
}

/// Collect a started git's answer within `deadline`; see [`bounded_run`].
/// When `writer` feeds git's stdin, git is reaped only after the write
/// ends, within the same deadline. `mode` says how much of stderr is kept.
fn bounded_wait(
    child: &mut std::process::Child,
    writer: Option<&crate::git::InputWriter>,
    deadline: Duration,
    mode: Diagnostics,
) -> Result<Ran, Failure> {
    let Some(stdout) = child.stdout.take() else {
        kill_group(child);
        return Err(Failure::Other("could not read its answer".to_string()));
    };
    // The destination query reads diagnostics only to classify
    // authentication failures and never shows them: a helper or URL could
    // include credential material. Only `bounded_output` returns them, to a
    // caller that redacts URLs before showing any (the id sync).
    let stderr = child.stderr.take();
    let kept = match mode {
        Diagnostics::Brief => 64 << 10,
        Diagnostics::Full => MAX_ANSWER_BYTES as u64 + 1,
    };
    let diagnostics = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = match stderr {
            Some(stderr) => stderr.take(kept).read_to_end(&mut bytes),
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
                kill_group(child);
                return Err(Failure::Other(format!(
                    "reading its answer failed: {error}"
                )));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !expired() => {}
            Err(_) => {
                kill_group(child);
                return Err(timed_out());
            }
        }
    };
    if bytes.len() > MAX_ANSWER_BYTES {
        kill_group(child);
        return Err(Failure::Other(format!(
            "its answer is over {} MiB",
            MAX_ANSWER_BYTES >> 20
        )));
    }
    // Git's own exit, observed without reaping it: an unreaped git keeps
    // its process group ID reserved, so ending the group below can reach
    // only git's descendants.
    loop {
        match has_exited(child) {
            Ok(true) => break,
            Ok(false) if !expired() => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill_group(child);
                return Err(timed_out());
            }
        }
    }
    // Git can exit before its input is written when a descendant it started
    // keeps stdin open and never reads. Git stays unreaped until the write
    // ends, so its group can still be ended at the deadline.
    if let Some(writer) = writer {
        while !writer.is_finished() {
            if expired() {
                kill_group(child);
                return Err(timed_out());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    collect_diagnostics(child, diagnostics, bytes, until, deadline, mode)
}

/// The stderr reader of a bounded child.
type DiagnosticsReader = std::thread::JoinHandle<std::io::Result<Vec<u8>>>;

/// After git exited and its answer was read: read its stderr as `mode`
/// says, reap it and return the run; see [`bounded_wait`].
fn collect_diagnostics(
    child: &mut std::process::Child,
    diagnostics: DiagnosticsReader,
    bytes: Vec<u8>,
    until: Option<Instant>,
    deadline: Duration,
    mode: Diagnostics,
) -> Result<Ran, Failure> {
    // Git's stderr closes when it exits unless a descendant it started, such
    // as an SSH or credential helper, still holds it (TSK-389). Such a
    // helper gets a short grace and is then ended with git's group, so the
    // lookup costs one wait and leaves no process behind, never a timeout
    // and a retry. Its diagnostics are then not read, so a failed exit is
    // reported as a plain one, which still refuses. A codeflow child's
    // stderr holds its findings, so it is read to the end within the
    // deadline instead.
    if mode == Diagnostics::Full {
        while !diagnostics.is_finished() {
            if until.is_some_and(|until| Instant::now() >= until) {
                kill_group(child);
                return Err(Failure::Timeout(deadline));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let read = diagnostics.join();
        let status = child.wait();
        return match (read, status) {
            (Ok(Ok(stderr)), Ok(status)) if stderr.len() <= MAX_ANSWER_BYTES => {
                Ok((status, bytes, Some(stderr)))
            }
            (_, Err(error)) => Err(Failure::Other(format!(
                "could not read how it exited: {error}"
            ))),
            (Ok(Ok(_)), Ok(_)) => Err(Failure::Other(format!(
                "its error output is over {} MiB",
                MAX_ANSWER_BYTES >> 20
            ))),
            (Ok(Err(error)), _) => Err(Failure::Other(format!(
                "reading its error output failed: {error}"
            ))),
            (Err(_), _) => Err(Failure::Other(
                "reading its error output failed".to_string(),
            )),
        };
    }
    let grace = Instant::now() + STDERR_GRACE;
    while !diagnostics.is_finished() && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(10));
    }
    let diagnostics = if diagnostics.is_finished() {
        match diagnostics.join() {
            Ok(Ok(bytes)) => Some(bytes),
            Ok(Err(_)) | Err(_) => None,
        }
    } else {
        signal_group(child);
        None
    };
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            return Err(Failure::Other(format!(
                "could not read how git exited: {error}"
            )))
        }
    };
    Ok((status, bytes, diagnostics))
}

/// A bounded run that ended without git's full output: why, and whether
/// the deadline ran out, which a caller of a network command may treat as
/// an unreachable remote.
#[derive(Debug, PartialEq, Eq)]
pub struct Unanswered {
    /// The deadline ran out and git's process group was ended.
    pub timed_out: bool,
    /// Why there is no output, fit to show.
    pub reason: String,
}

/// Run a git command within `deadline` and [`MAX_ANSWER_BYTES`], as the
/// destination query's own reads run, and return its output whatever the
/// exit status (TSK-327 pre-push id sync). Its stderr is at most 64 KiB, and
/// empty when a helper held it past the grace. The caller classifies a
/// failed exit; a timeout, an oversized answer or a failure to run is
/// [`Unanswered`], never an output.
///
/// # Errors
///
/// Why git gave no output within `deadline`.
pub fn bounded_output(
    command: std::process::Command,
    deadline: Duration,
) -> Result<std::process::Output, Unanswered> {
    output_of(bounded_run(command, deadline))
}

/// [`bounded_output`] for a stdin-fed git command, such as `cat-file
/// --batch` or `hash-object --stdin`: `input` is written from its own
/// thread, and the deadline and [`MAX_ANSWER_BYTES`] bound the whole
/// exchange (TSK-327 pre-push id sync).
///
/// # Errors
///
/// Why git gave no output within `deadline`.
pub fn bounded_output_with_input(
    command: std::process::Command,
    input: &[u8],
    deadline: Duration,
) -> Result<std::process::Output, Unanswered> {
    output_of(bounded_run_input(
        command,
        Some(input),
        deadline,
        Diagnostics::Brief,
    ))
}

/// Run a codeflow child process with `input` on its stdin, as the pre-push
/// hook runs `codeflow ci` (TSK-327), within `deadline`: the child leads
/// its own process group, and at the deadline the whole group is ended, so
/// every git command it started inside the group ends with it, a lazy
/// fetch from a stalled promisor included. Its stdout and its full stderr
/// (where it prints its findings) are returned, each up to
/// [`MAX_ANSWER_BYTES`], whatever the exit status.
///
/// # Errors
///
/// Why the child gave no output within `deadline`.
pub fn bounded_process_output(
    command: std::process::Command,
    input: &[u8],
    deadline: Duration,
) -> Result<std::process::Output, Unanswered> {
    match bounded_run_input(command, Some(input), deadline, Diagnostics::Full) {
        Err(Failure::Timeout(deadline)) => Err(Unanswered {
            timed_out: true,
            reason: format!("it gave no verdict within {}", seconds(deadline)),
        }),
        ran => output_of(ran),
    }
}

fn output_of(ran: Result<Ran, Failure>) -> Result<std::process::Output, Unanswered> {
    match ran {
        Ok((status, stdout, stderr)) => Ok(std::process::Output {
            status,
            stdout,
            stderr: stderr.unwrap_or_default(),
        }),
        Err(why) => Err(Unanswered {
            timed_out: matches!(why, Failure::Timeout(_)),
            reason: local_read_failure(why),
        }),
    }
}

fn local_read_failure(why: Failure) -> String {
    match why {
        Failure::Exit(_) | Failure::Authentication => "git failed".to_string(),
        Failure::Timeout(deadline) => format!("git gave no answer within {}", seconds(deadline)),
        Failure::Other(why) => why,
    }
}

/// [`bounded`] for a local git read in another module: the answer, or why
/// there is none, as text (TSK-1184 `remote_identity`).
pub(crate) fn bounded_text(
    command: std::process::Command,
    deadline: Duration,
) -> Result<String, String> {
    bounded(command, deadline).map_err(local_read_failure)
}

/// [`bounded_text`] with the answer's bytes undecoded, for a listing whose
/// unrelated entries need not be text (TSK-1184 `remote_identity`).
pub(crate) fn bounded_raw(
    command: std::process::Command,
    deadline: Duration,
) -> Result<Vec<u8>, String> {
    bounded_bytes(command, deadline).map_err(local_read_failure)
}

/// How a bounded local git read ended when it gave an answer or an exit
/// status the caller interprets, such as `remote get-url` exiting 2 for a
/// remote that is not configured.
#[derive(Debug, PartialEq, Eq)]
pub enum LocalAnswer {
    /// Git exited successfully with these bytes on stdout.
    Answer(Vec<u8>),
    /// Git ran and exited unsuccessfully with this code.
    Exit(Option<i32>),
}

/// Run a local git read within `deadline` and [`MAX_ANSWER_BYTES`], as the
/// destination query's own reads run, for a caller that reads meaning into
/// git's exit status (TSK-327 hook, guard and CI destination reads). A
/// timeout, an oversized answer, a failure to run or a failed exit with an
/// authentication marker is `Err`, so it can never be taken for a proven
/// absence.
///
/// # Errors
///
/// Why git gave no answer or exit status within `deadline`.
pub fn bounded_local(
    command: std::process::Command,
    deadline: Duration,
) -> Result<LocalAnswer, String> {
    match bounded_bytes(command, deadline) {
        Ok(bytes) => Ok(LocalAnswer::Answer(bytes)),
        Err(Failure::Exit(code)) => Ok(LocalAnswer::Exit(code)),
        Err(why) => Err(local_read_failure(why)),
    }
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
/// A configured command is kept, selected by git's own precedence:
/// `GIT_SSH_COMMAND`, then `core.sshCommand`, then `GIT_SSH`, then `ssh`
/// (TSK-327 AC-3). `core.sshCommand` is read as the push read it, through
/// [`push_view`]: the files every git command reads, and the command-line
/// configuration (`git -c`) git hands the hook, which the push connected
/// with. The one selected command is passed on as `GIT_SSH_COMMAND`, which
/// git prefers to every other selector, so no unselected transport runs and
/// the query connects with the program the push used. When the selection
/// cannot be read, the query refuses.
///
/// OS text rule (issue 79): the command is executed, so it must be exactly what
/// git would run. A value that is not valid UTF-8 cannot be passed on as text,
/// and a lossy spelling would run a different program, so the query refuses.
/// The configuration read runs within `deadline`, as the query does.
fn batch_ssh_command(root: &Path, deadline: Duration) -> Result<String, Failure> {
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
        let mut command = push_view();
        command
            .arg("-C")
            .arg(root)
            .args(["config", "--null", "--get", "core.sshCommand"]);
        match bounded_bytes(command, deadline) {
            Ok(answer) => {
                // `--null` frames the value with a NUL, so a value that ends
                // in a carriage return keeps it.
                let framed = answer.strip_suffix(&[0]).ok_or_else(|| {
                    Failure::Other("cannot read core.sshCommand: missing NUL framing".into())
                })?;
                let value = std::str::from_utf8(framed)
                    .map_err(|_| not_utf8("core.sshCommand"))?
                    .to_string();
                configured = Some(value);
            }
            // Git exits 1 when the key is not set.
            Err(Failure::Exit(Some(1))) => {}
            Err(why) => {
                return Err(Failure::Other(format!(
                    "cannot read core.sshCommand: {}",
                    local_read_failure(why)
                )))
            }
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

/// A `git` that reads configuration as the push did: a
/// [`crate::git::judging_command`] given back the command-line configuration
/// this process inherited (`git -c`, `--config-env`), which git hands the
/// hook from the push. Only for reading settings the push connected with;
/// the destination query itself never runs in this view, since a
/// command-line rewrite was already applied to the URL the hook is given.
fn push_view() -> std::process::Command {
    let mut command = crate::git::judging_command();
    for (key, setting) in std::env::vars_os() {
        if crate::git::is_command_line_config(&key) {
            command.env(key, setting);
        }
    }
    command
}

/// The command-line setting the push connected with that the destination
/// query does not apply and that could change how it connects: a key in the
/// `http`, `protocol`, `remote`, `ssh` or `transfer` sections, or
/// `core.gitProxy` (TSK-327). The query reads no command-line configuration
/// except `core.sshCommand`, which selects its ssh program, and a command-line
/// rewrite has already been applied to the URL the hook is given, so under
/// such a setting the query and the push could get different answers for
/// the same URL. Read through git, which parses the command line.
///
/// # Errors
///
/// Returns why the command-line configuration could not be read within
/// `limits`.
pub fn command_line_transport(root: &Path, limits: QueryLimits) -> Result<Option<String>, String> {
    let mut command = push_view();
    command
        .arg("-C")
        .arg(root)
        .args(["config", "-z", "--list", "--show-scope"])
        .env("LC_ALL", "C");
    let listed = bounded_raw(command, limits.timeout())
        .map_err(|why| format!("cannot read the command-line git configuration: {why}"))?;
    Ok(transport_in(&listed))
}

/// [`command_line_transport`] over `git config -z --list --show-scope`
/// output: each entry is its scope and a NUL, then the key with a newline
/// and its value, or the key alone, and a NUL. Keys are lowercase there.
fn transport_in(listed: &[u8]) -> Option<String> {
    const SECTIONS: [&[u8]; 5] = [b"http", b"protocol", b"remote", b"ssh", b"transfer"];
    let mut fields = listed.split(|byte| *byte == 0);
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        if scope != b"command" {
            continue;
        }
        let key = entry
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        let section = key.split(|byte| *byte == b'.').next().unwrap_or_default();
        if SECTIONS.contains(&section) || key == b"core.gitproxy" {
            // Shown only: the key is escaped and any userinfo in a URL
            // subsection hidden.
            let shown = crate::git::GitName::from_bytes(key).display().to_string();
            return Some(format!("the command-line setting {}", display_urls(&shown)));
        }
    }
    None
}

/// How long a helper may hold git's stderr open after git has exited before
/// it is ended with git's process group (TSK-389).
const STDERR_GRACE: Duration = Duration::from_millis(250);

/// Whether the child has exited, without reaping it on Unix, so its process
/// group ID cannot be reused while the group may still be signalled.
fn has_exited(child: &mut std::process::Child) -> std::io::Result<bool> {
    #[cfg(unix)]
    {
        let pid: libc::id_t = child.id();
        // SAFETY: a zeroed `siginfo_t` is a valid value to pass for `waitid`
        // to fill; `WNOWAIT` leaves the child waitable, so the later `wait`
        // still reaps it and reads its status.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let done = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &raw mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if done != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // With `WNOHANG`, a child that has not exited leaves `info` zeroed.
        Ok(info.si_signo != 0)
    }
    #[cfg(not(unix))]
    {
        child.try_wait().map(|status| status.is_some())
    }
}

/// On Unix, SIGKILL to the process group the child leads (an SSH or
/// credential helper it started); best-effort, and the child is not reaped.
fn signal_group(child: &std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: the child leads its own process group (`process_group(0)`),
        // whose id is its pid, and it is not reaped yet, so the id is still
        // its group's; SIGKILL to it is best-effort.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = child;
}

/// Kill a child and, on Unix, the process group it leads (an SSH or
/// credential helper it started), then reap it.
fn kill_group(child: &mut std::process::Child) {
    signal_group(child);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TSK-327: a stdin-fed read has the same deadline and answer cap as
    /// every other bounded read. A child that never reads its input (1 MiB,
    /// more than a pipe holds, so the writer blocks) and never answers is
    /// ended at the deadline with its group, and the blocked writer ends
    /// with it. A child that answers more than the cap is refused. A child
    /// that reads its input and answers is returned whole.
    #[cfg(unix)]
    #[test]
    fn a_stdin_fed_read_ends_at_its_deadline_and_cap() {
        let shell = |script: &str| {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", script]);
            command
        };
        let input = vec![b'x'; 1 << 20];
        let started = Instant::now();
        let stalled = bounded_output_with_input(shell("sleep 30"), &input, Duration::from_secs(1));
        let took = started.elapsed();
        let why = stalled.unwrap_err();
        assert!(why.timed_out, "{why:?}");
        assert!(why.reason.contains("within 1s"), "{why:?}");
        assert!(took < Duration::from_secs(10), "took {took:?}");
        // The parent exits at once, closing its stdout and stderr, while a
        // descendant keeps stdin open and never reads; the input outgrows
        // the pipe, so only ending the group frees the writer.
        let started = Instant::now();
        let held = bounded_output_with_input(
            shell("exec 3<&0; sleep 30 <&3 >/dev/null 2>&1 & exit 0"),
            &input,
            Duration::from_secs(1),
        );
        let took = started.elapsed();
        let why = held.unwrap_err();
        assert!(why.timed_out, "{why:?}");
        assert!(took < Duration::from_secs(10), "took {took:?}");
        let oversized = bounded_output_with_input(
            shell("cat >/dev/null; head -c 17000000 /dev/zero"),
            &input,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(!oversized.timed_out && oversized.reason.contains("over 16 MiB"));
        let answered =
            bounded_output_with_input(shell("wc -c"), &input, Duration::from_secs(30)).unwrap();
        assert!(answered.status.success());
        assert_eq!(String::from_utf8_lossy(&answered.stdout).trim(), "1048576");
    }

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

    /// TSK-389 (TSK-327 AC-7 and AC-8): a helper that keeps git's stderr
    /// open after git has exited costs one wait, well inside the deadline,
    /// and is ended with git's group. Before, the query waited the whole
    /// deadline for that pipe and reported a timeout, which `ls_remote`
    /// then retried. A git that fails still refuses, in the same one wait.
    #[cfg(unix)]
    #[test]
    fn a_helper_holding_stderr_costs_one_wait_and_is_ended() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_STDERR_HELPER_CHILD";
        const DEADLINE: Duration = Duration::from_secs(10);
        if let Some(root) = std::env::var_os(CHILD) {
            let root = Path::new(&root);
            for (exit, answered) in [("0", true), ("128", false)] {
                std::fs::write(root.join("exit"), exit).unwrap();
                let started = Instant::now();
                let result = run(root, &["ls-remote", "stub"], DEADLINE);
                let took = started.elapsed();
                assert!(took < Duration::from_secs(3), "took {took:?}");
                if answered {
                    assert!(matches!(result, Ok(ref answer) if answer.is_empty()));
                } else {
                    assert!(matches!(result, Err(Failure::Exit(Some(128)))));
                }
                let helper: i32 = std::fs::read_to_string(root.join("helper.pid"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                // The helper was signalled with git's group. Its new parent
                // reaps it shortly; until then it is a zombie that `kill`
                // still finds, so the check waits for it to go.
                let gone = Instant::now() + Duration::from_secs(5);
                // SAFETY: signal 0 only asks whether the process exists.
                while unsafe { libc::kill(helper, 0) } == 0 {
                    assert!(Instant::now() < gone, "helper {helper} still runs");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let git = bin.join("git");
        // Git answers, starts a helper that keeps stderr, and exits.
        std::fs::write(
            &git,
            format!(
                "#!/bin/sh\n/bin/sleep 60 >/dev/null </dev/null &\necho $! > '{root}/helper.pid'\nexit $(/bin/cat '{root}/exit')\n",
                root = dir.path().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = crate::git::process(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::remote_query::tests::a_helper_holding_stderr_costs_one_wait_and_is_ended",
                "--nocapture",
            ])
            .env(CHILD, dir.path())
            .env("PATH", bin)
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

    /// The ssh selection cases: (`GIT_SSH_COMMAND`, `core.sshCommand`,
    /// `GIT_SSH`, command-line overlay, deprecated file) and the one run.
    #[cfg(unix)]
    const SSH_SELECTION_CASES: [([bool; 5], &str); 13] = [
        ([false, false, false, false, false], "ssh"),
        ([true, false, false, false, false], "rec-command"),
        ([false, true, false, false, false], "rec-config"),
        ([false, false, true, false, false], "rec-program"),
        ([true, true, false, false, false], "rec-command"),
        ([true, false, true, false, false], "rec-command"),
        ([false, true, true, false, false], "rec-config"),
        ([true, true, true, false, false], "rec-command"),
        ([false, false, false, true, false], "rec-overlay"),
        ([false, true, false, true, false], "rec-overlay"),
        ([true, false, false, true, false], "rec-command"),
        ([false, false, false, false, true], "ssh"),
        ([false, true, false, false, true], "rec-config"),
    ];

    /// TSK-327 AC-3: the query runs only the transport git's precedence
    /// selects among `GIT_SSH_COMMAND`, `core.sshCommand` and `GIT_SSH`,
    /// for each one alone, each pair and all three, and plain `ssh` with
    /// none. A `core.sshCommand` given as a command-line overlay is the
    /// push's own, so it selects as git selects it, over the repository's.
    /// One named only in the deprecated file that `git config` alone reads
    /// selects nothing, since the push never read it.
    #[cfg(unix)]
    #[test]
    fn the_query_runs_only_the_selected_ssh_transport() {
        use std::os::unix::fs::PermissionsExt as _;
        const CHILD: &str = "CODEFLOW_SSH_SELECTION_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            let limits = QueryLimits::scaled(Duration::from_secs(20), 1);
            let asked = ls_remote(Path::new(&root), &["ssh://example.invalid/x.git"], limits);
            assert!(asked.is_err(), "every recorder refuses: {asked:?}");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = dir.path().join("ran.log");
        for name in [
            "rec-command",
            "rec-config",
            "rec-program",
            "rec-overlay",
            "rec-file",
            "ssh",
        ] {
            let script = bin.join(name);
            std::fs::write(
                &script,
                format!("#!/bin/sh\necho {name} >> '{}'\nexit 255\n", log.display()),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let file = dir.path().join("deprecated-file");
        std::fs::write(&file, "[core]\n\tsshCommand = rec-file\n").unwrap();
        for ([command, config, program, overlay, deprecated], expected) in SSH_SELECTION_CASES {
            let root = tempfile::tempdir().unwrap();
            let repo = git2::Repository::init(root.path()).unwrap();
            if config {
                repo.config()
                    .unwrap()
                    .set_str("core.sshCommand", "rec-config")
                    .unwrap();
            }
            let _ = std::fs::remove_file(&log);
            let mut child = crate::git::process(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "git::remote_query::tests::the_query_runs_only_the_selected_ssh_transport",
                    "--nocapture",
                ])
                .env(CHILD, root.path())
                .env("PATH", &path)
                .env("HOME", dir.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env_remove("GIT_SSH_COMMAND")
                .env_remove("GIT_SSH")
                .env_remove("GIT_SSH_VARIANT")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_CONFIG_COUNT");
            if command {
                child.env("GIT_SSH_COMMAND", "rec-command");
            }
            if program {
                child.env("GIT_SSH", bin.join("rec-program"));
            }
            if overlay {
                child.env("GIT_CONFIG_PARAMETERS", "'core.sshCommand=rec-overlay'");
            }
            if deprecated {
                child.env("GIT_CONFIG", &file);
            }
            let out = child.output().unwrap();
            assert!(
                out.status.success(),
                "{expected}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let ran = std::fs::read_to_string(&log).unwrap_or_default();
            let ran: std::collections::BTreeSet<&str> = ran.lines().collect();
            assert_eq!(
                ran,
                std::collections::BTreeSet::from([expected]),
                "case {:?}",
                (command, config, program, overlay, deprecated)
            );
        }
    }

    /// TSK-327 AC-1: every configuration that would send a destination
    /// query elsewhere is named; a rewrite of another URL, a pushInsteadOf
    /// and a remote under another name are not.
    #[test]
    fn a_redirecting_setting_is_named_and_others_are_not() {
        let url = "/srv/dest.git";
        let listed = |entries: &[&str]| entries.join("\0").into_bytes();
        for (entries, named) in [
            (vec!["url./srv/decoy.git.insteadof\n/srv/dest"], true),
            (vec!["url./srv/decoy.git.insteadof\n/srv/dest.git"], true),
            (vec!["url.x.insteadof"], true),
            (vec!["remote./srv/dest.git.uploadpack\nevil"], true),
            (vec!["url./srv/decoy.git.insteadof\n"], true),
            (vec!["url./srv/decoy.git.insteadof\n/srv/other"], false),
            (
                vec!["url./srv/decoy.git.pushinsteadof\n/srv/dest.git"],
                false,
            ),
            (
                vec![
                    "remote.origin.url\n/srv/dest.git",
                    "remote.pushdefault\norigin",
                ],
                false,
            ),
            (vec!["remote./SRV/DEST.GIT.url\nx"], false),
        ] {
            assert_eq!(
                overlay_in(&listed(&entries), url).is_some(),
                named,
                "{entries:?}"
            );
        }
    }

    /// TSK-327: a command-line setting that could change how the query
    /// connects is named; other command-line settings and every file scope
    /// are not.
    #[test]
    fn a_command_line_transport_setting_is_named() {
        let listed = |entries: &[&str]| entries.join("\0").into_bytes();
        for (entries, named) in [
            (vec!["command", "http.proxy\nhttp://proxy.invalid"], true),
            (
                vec!["command", "http.https://h.invalid/.extraheader\nx"],
                true,
            ),
            (vec!["command", "protocol.allow\nalways"], true),
            (vec!["command", "remote.origin.uploadpack\nx"], true),
            (vec!["command", "ssh.variant\nsimple"], true),
            (vec!["command", "transfer.fsckobjects"], true),
            (vec!["command", "core.gitproxy\nx"], true),
            (
                vec![
                    "command",
                    "maintenance.auto\nfalse",
                    "command",
                    "gc.auto\n0",
                ],
                false,
            ),
            (vec!["command", "core.sshcommand\nx"], false),
            (vec!["command", "url./srv/a.insteadof\n/srv/b"], false),
            (vec!["local", "http.proxy\nhttp://proxy.invalid"], false),
            (vec!["global", "remote.origin.url\n/srv/a"], false),
        ] {
            assert_eq!(
                transport_in(&listed(&entries)).is_some(),
                named,
                "{entries:?}"
            );
        }
    }

    /// TSK-327 AC-1: the check reads the repository's own configuration
    /// and its legacy remote files, and a clean repository passes.
    #[test]
    fn a_rewrite_in_the_repository_refuses_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let dest = dir.path().join("dest.git");
        let url = dest.to_str().unwrap();
        // The developer's own files could name this path only by accident.
        if require_unredirected(dir.path(), url, QueryLimits::guard()).is_err() {
            return;
        }
        repo.config()
            .unwrap()
            .set_str(
                &format!("url.{}/decoy.git.insteadOf", dir.path().display()),
                url,
            )
            .unwrap();
        let refused = require_unredirected(dir.path(), url, QueryLimits::guard()).unwrap_err();
        assert!(refused.contains("insteadOf"), "{refused}");
        assert!(refused.contains("cannot be verified"), "{refused}");
        let clean = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(clean.path()).unwrap();
        assert!(require_unredirected(clean.path(), "dest", QueryLimits::guard()).is_ok());
        std::fs::create_dir_all(repo.path().join("remotes")).unwrap();
        std::fs::write(
            repo.path().join("remotes").join("dest"),
            "URL: /elsewhere\n",
        )
        .unwrap();
        let refused = require_unredirected(clean.path(), "dest", QueryLimits::guard()).unwrap_err();
        assert!(refused.contains("remotes/dest"), "{refused}");
        assert!(require_unredirected(clean.path(), "./dest", QueryLimits::guard()).is_ok());
        let refused = require_unredirected(clean.path(), "--upload-pack=x", QueryLimits::guard())
            .unwrap_err();
        assert!(refused.contains("starts with '-'"), "{refused}");
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
        assert!(batch_ssh_command(dir.path(), QueryLimits::guard().timeout()).is_ok());
        let path = repo.path().join("config");
        std::fs::write(path, "[broken\n").unwrap();
        assert!(
            batch_ssh_command(dir.path(), QueryLimits::guard().timeout()).is_err(),
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
        let error = batch_ssh_command(dir.path(), QueryLimits::guard().timeout())
            .err()
            .unwrap();
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
        let command = batch_ssh_command(dir.path(), QueryLimits::guard().timeout())
            .ok()
            .unwrap();
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
        let error = batch_ssh_command(dir.path(), QueryLimits::guard().timeout())
            .err()
            .unwrap();
        assert!(
            matches!(error, Failure::Other(ref why) if why.contains("empty")),
            "refused"
        );
    }
}
