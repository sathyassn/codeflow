//! The configured and the effective identity of a remote (TSK-1184).
//!
//! The policy authority asks a remote whether it holds any ref with a
//! hermetic query of the URL written in `remote.<name>.url`
//! ([`super::remote_query::advertises_any_ref`]), which applies no
//! `insteadOf`. Git itself fetches and pushes through the URL its rewrites
//! produce. When the two differ, an empty raw endpoint can stand in for a
//! populated authority, so the comparison here comes first and any doubt
//! refuses. It reads the effective URLs as the fetch guard does
//! (`hooks/ref_authority.rs`, `git remote get-url`), so includes and every
//! configuration scope count.

use std::path::Path;

use super::remote_query::{display_url, QueryLimits, SAFE_PROTOCOLS};

/// The transport git selects for `url`, by the name `GIT_ALLOW_PROTOCOL`
/// uses: `<helper>::<address>` names a remote helper; `scheme://` names its
/// scheme, with `git+ssh` and `ssh+git` counted as `ssh`; a local path is
/// `file`, and the scp-like `host:path` form is `ssh`.
#[must_use]
pub fn transport(url: &str) -> String {
    let scheme_len = url
        .char_indices()
        .take_while(|&(at, c)| {
            c.is_ascii_alphanumeric() || (at > 0 && matches!(c, '+' | '-' | '.'))
        })
        .count();
    let rest = &url[scheme_len..];
    if scheme_len > 0 && rest.starts_with("::") {
        return url[..scheme_len].to_string();
    }
    if scheme_len > 0 && rest.starts_with("://") {
        return match &url[..scheme_len] {
            "git+ssh" | "ssh+git" => "ssh".into(),
            other => other.into(),
        };
    }
    if is_local_path(url) {
        "file".into()
    } else {
        "ssh".into()
    }
}

/// Whether git reads `url` as a local path: no `:` before the first `/`,
/// except a Windows drive letter. Called only for URLs with no scheme.
fn is_local_path(url: &str) -> bool {
    match url.find(':') {
        None => true,
        Some(colon) => {
            url[..colon].contains('/')
                || (cfg!(windows) && colon == 1 && url.as_bytes()[0].is_ascii_alphabetic())
        }
    }
}

/// `url` as messages show it: control characters escaped and userinfo
/// hidden.
fn shown(url: &str) -> String {
    display_url(&super::display_text(url)).into_owned()
}

/// Refuse a `url` whose transport is outside [`SAFE_PROTOCOLS`], naming the
/// transport and the list. The list is never widened to admit it.
///
/// # Errors
///
/// The transport is not on the hermetic allow-list.
pub fn require_safe_protocol(url: &str) -> Result<(), String> {
    let transport = transport(url);
    if SAFE_PROTOCOLS
        .split(':')
        .any(|allowed| allowed == transport)
    {
        return Ok(());
    }
    Err(format!(
        "{} uses the {} protocol, outside the hermetic allow-list {SAFE_PROTOCOLS}; the list is never widened to ask it",
        shown(url),
        super::display_text(&transport)
    ))
}

/// Every URL git resolves for remote `name` in the repository at `root`
/// (`git remote get-url --all`, or with `--push`): git applies `insteadOf`,
/// `pushInsteadOf`, `pushurl` and include files at every scope, so these
/// are the identities a fetch or a push would use. Location variables cannot
/// point the read at another repository, and a configuration read that never
/// ends (a fifo behind an include) is cut off at `limits`.
///
/// # Errors
///
/// Git could not run, failed, ran out of time, or printed text that is not
/// UTF-8.
pub fn effective_urls(
    root: &Path,
    name: &str,
    push: bool,
    limits: QueryLimits,
) -> Result<Vec<String>, String> {
    let mut command = git_at(root);
    command.args(["remote", "get-url", "--all"]);
    if push {
        command.arg("--push");
    }
    command.args(["--", name]);
    let answer = super::remote_query::bounded_text(command, limits.timeout())?;
    let lines: Vec<String> = answer
        .strip_suffix('\n')
        .unwrap_or(&answer)
        .split('\n')
        .map(str::to_string)
        .collect();
    if lines.iter().all(String::is_empty) {
        return Err("git named no URL".into());
    }
    Ok(lines)
}

/// A git process that reads the repository at `root` with the configuration
/// git itself would apply there: location variables cannot point it at
/// another repository, and inherited configuration is kept on purpose. The
/// deprecated `GIT_CONFIG` file goes, as in [`super::judging_command`]: only
/// `git config` reads it, so under it the configuration listing would not
/// be what a fetch or push applies, and a rewrite or transport setting
/// could hide from the checks here (TSK-327 AC-6).
fn git_at(root: &Path) -> std::process::Command {
    let mut command = super::command();
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG",
    ] {
        command.env_remove(variable);
    }
    command.arg("-C").arg(root).env("LC_ALL", "C");
    command
}

/// One configuration entry: where git read it, its key as git prints it
/// (section and variable lowercased, subsection as written) and its value.
/// The text is decoded lossily so that an unrelated entry that is not UTF-8
/// cannot stop the read; `exact` says whether all three fields were valid
/// UTF-8, and a check that acts on an entry's text refuses one that is not.
struct Entry {
    origin: String,
    key: String,
    value: String,
    exact: bool,
}

/// Every configuration entry git applies in the repository at `root`, in
/// the view [`effective_urls`] reads, with the file each came from.
fn entries(root: &Path, limits: QueryLimits) -> Result<Vec<Entry>, String> {
    let mut command = git_at(root);
    command.args(["config", "--null", "--show-origin", "--list"]);
    parse_entries(&super::remote_query::bounded_raw(
        command,
        limits.timeout(),
    )?)
}

/// `git config --null --show-origin --list` output: `origin NUL key LF
/// value NUL` per entry; a key with no value has no LF. Without
/// `--show-origin` (`with_origin` false) each entry is `key LF value NUL`.
fn parse_listing(bytes: &[u8], with_origin: bool) -> Result<Vec<Entry>, String> {
    // A field that is not UTF-8 keeps only its display spelling and is
    // marked inexact, so a check that acts on it refuses.
    let text = |field: &[u8]| {
        let name = super::GitName::from_bytes(field);
        match name.rule_text() {
            Ok(text) => (text.to_string(), true),
            Err(_) => (name.display().to_string(), false),
        }
    };
    let mut fields = bytes.split(|&byte| byte == 0);
    let mut entries = Vec::new();
    loop {
        let (origin, origin_exact) = if with_origin {
            match fields.next() {
                Some(origin) if !origin.is_empty() => text(origin),
                _ => break,
            }
        } else {
            (String::new(), true)
        };
        let item = match fields.next() {
            Some(item) if !item.is_empty() => item,
            _ if with_origin => return Err("git config listed an origin with no entry".into()),
            _ => break,
        };
        let (key, value) = match item.iter().position(|&byte| byte == b'\n') {
            Some(at) => (&item[..at], &item[at + 1..]),
            None => (item, &item[item.len()..]),
        };
        let (key, key_exact) = text(key);
        let (value, value_exact) = text(value);
        entries.push(Entry {
            origin,
            key,
            value,
            exact: origin_exact && key_exact && value_exact,
        });
    }
    Ok(entries)
}

fn parse_entries(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    parse_listing(bytes, true)
}

/// The settings of a `remote.<name>` or `remote.<url>` section that replace
/// or redirect the transport while the URL stays as written: a remote
/// helper in place of git's transport, and the programs the remote end
/// runs, which on local and SSH transports choose the repository that
/// answers.
pub const TRANSPORT_SETTINGS: &[&str] = &["vcs", "uploadpack", "receivepack"];

/// Refuse when `section` (`remote.<name>` or `remote.<url>`) sets any of
/// [`TRANSPORT_SETTINGS`] in the configuration git applies at `root`, at
/// any scope or through any include: a hermetic question of the URL would
/// then not ask what a later fetch or push reaches. The refusal names the
/// setting and its value (the helper or program). Owned by TSK-1184. The
/// pre-push destination check (TSK-327) does not call it: a URL given to
/// `git ls-remote` reads every setting of its `remote.<url>` section, so
/// that check refuses any of them through the stricter [`url_section_in`].
///
/// # Errors
///
/// `section` is not a remote section, the configuration cannot be read, or
/// the section replaces or redirects the transport.
pub fn require_native_transport(
    root: &Path,
    section: &str,
    limits: QueryLimits,
) -> Result<(), String> {
    let subsection = section
        .get(..7)
        .filter(|head| head.eq_ignore_ascii_case("remote."))
        .and_then(|_| section.get(7..))
        .filter(|rest| !rest.is_empty())
        .ok_or_else(|| format!("{} is not a remote section", super::display_text(section)))?;
    let entries =
        entries(root, limits).map_err(|why| format!("the configuration cannot be read ({why})"))?;
    refuse_transport_settings(&entries, subsection)
}

fn refuse_transport_settings(entries: &[Entry], subsection: &str) -> Result<(), String> {
    for entry in entries {
        let Some((section, variable)) = entry
            .key
            .strip_prefix("remote.")
            .and_then(|rest| rest.rsplit_once('.'))
        else {
            continue;
        };
        if section == subsection && TRANSPORT_SETTINGS.contains(&variable) {
            return Err(format!(
                "remote.{}.{variable} is set to {} ({}), which replaces or redirects the transport while the URL stays the same, so a question of the URL would not ask what git reaches",
                super::display_text(subsection),
                super::display_text(&entry.value),
                super::display_text(&entry.origin)
            ));
        }
    }
    Ok(())
}

/// The `remote.<url>` section in `listed`, the output of `git config -z
/// --list` (each entry is the key, then a newline and the value or nothing,
/// then NUL), named for a message when any key sets it. `git ls-remote
/// <url>` and `git fetch <url>` read `url` as a remote name first, so every
/// setting of that section counts (its URL, push URL, helper, programs or
/// fetch mapping), not only [`TRANSPORT_SETTINGS`]: this is stricter than
/// [`require_native_transport`], which refuses only those three settings.
/// Keys are compared as bytes; the subsection keeps its case, as
/// git compares it. Owned with the transport check (TSK-1184); the pre-push
/// destination check (TSK-327) calls it.
#[must_use]
pub fn url_section_in(listed: &[u8], url: &str) -> Option<String> {
    for entry in listed.split(|byte| *byte == 0) {
        let key = match entry.iter().position(|byte| *byte == b'\n') {
            Some(at) => &entry[..at],
            None => entry,
        };
        let Some(rest) = key.strip_prefix(b"remote.") else {
            continue;
        };
        let subsection = rest
            .iter()
            .rposition(|byte| *byte == b'.')
            .map(|dot| &rest[..dot]);
        if subsection == Some(url.as_bytes()) {
            return Some(format!("a remote configured under the name {}", shown(url)));
        }
    }
    None
}

/// The keys a path-conditional include may not set: URL rewrites, remote
/// settings, further includes, and the SSH client git runs
/// (`core.sshCommand`, `ssh.*`), each of which can change what a later fetch
/// reaches without changing the URL read here.
const PATH_CONDITIONAL_REFUSED: &[&str] = &[
    "url.",
    "remote.",
    "include.",
    "includeif.",
    "core.sshcommand",
    "ssh.",
];

/// Refuse when the configuration includes a file on a `gitdir` condition
/// and that file sets a URL rewrite, a remote setting, a further include or
/// an SSH client setting ([`PATH_CONDITIONAL_REFUSED`]).
/// Whether git applies such a file depends on the path a command reaches
/// the repository by (a symbolic link, `GIT_DIR` as written), which this
/// read cannot know, so its answer would not bind a later fetch or push.
/// Each included file is read within the caller's `limits`.
fn refuse_path_conditional_remote_settings(
    entries: &[Entry],
    root: &Path,
    limits: QueryLimits,
) -> Result<(), String> {
    for entry in entries {
        let Some(condition) = entry
            .key
            .strip_prefix("includeif.")
            .and_then(|rest| rest.strip_suffix(".path"))
        else {
            continue;
        };
        let shown_condition = super::display_text(condition);
        // OS text rule (issue 79): an include whose condition, path or
        // origin is not UTF-8 cannot be judged exactly, so it refuses.
        if !entry.exact {
            return Err(format!(
                "a configuration file is included on the condition {shown_condition} from {}, and that entry is not valid UTF-8, so whether it changes the remote cannot be checked",
                super::display_text(&entry.origin)
            ));
        }
        if !(condition.starts_with("gitdir:") || condition.starts_with("gitdir/i:")) {
            continue;
        }
        let file = included_path(entry, root).ok_or_else(|| {
            format!(
                "a configuration file is included on the condition {shown_condition} from {}, and its path cannot be resolved here, so whether it changes the remote cannot be checked",
                super::display_text(&entry.origin)
            )
        })?;
        match crate::absence::symlink_metadata_optional(&file) {
            // Git skips an include that names no file; absence is proven
            // through the ancestors of the absolute path git reads.
            Ok(None) => continue,
            Ok(Some(_)) => {}
            Err(error) => {
                return Err(format!(
                    "the configuration file {} included on the condition {shown_condition} cannot be inspected: {error}",
                    file.display()
                ))
            }
        }
        let mut command = super::command();
        command
            .args(["config", "--null", "--no-includes", "--list", "--file"])
            .arg(&file)
            .env("LC_ALL", "C");
        let listed = super::remote_query::bounded_raw(command, limits.timeout())
            .and_then(|bytes| parse_listing(&bytes, false))
            .map_err(|why| {
                format!(
                    "the configuration file {} included on the condition {shown_condition} cannot be read ({why})",
                    file.display()
                )
            })?;
        if let Some(found) = listed.iter().find(|found| {
            PATH_CONDITIONAL_REFUSED
                .iter()
                .any(|prefix| found.key.starts_with(prefix))
        }) {
            return Err(format!(
                "the configuration file {} is included on the condition {shown_condition} and sets {}; whether git applies it depends on the path a command reaches the repository by, so the URL read here may not be the one a later fetch or push uses",
                file.display(),
                super::display_text(&found.key)
            ));
        }
    }
    Ok(())
}

/// The file an include entry names, resolved as git resolves it: `~/`
/// under the home directory, a relative path beside the file that holds
/// the entry. Git run with `-C root` names a repository file relative to
/// `root` (`file:.git/config`), so a relative holder resolves against
/// `root`, never against this process's directory. `None` when git's
/// answer cannot be reproduced here.
fn included_path(entry: &Entry, root: &Path) -> Option<std::path::PathBuf> {
    let value = entry.value.as_str();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return std::env::var_os("HOME").map(|home| Path::new(&home).join(rest));
    }
    if value.starts_with('~') {
        return None;
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    // `root` is the directory git ran in; joining keeps an absolute holder.
    let holder = root.join(entry.origin.strip_prefix("file:")?);
    holder.parent().map(|dir| dir.join(path))
}

/// The `ssh -G` settings that choose where an SSH connection goes: the host
/// it reaches, its port and user, the key name it checks, and any program
/// or host it goes through.
pub const SSH_ENDPOINT_KEYS: &[&str] = &[
    "hostname",
    "port",
    "user",
    "hostkeyalias",
    "proxycommand",
    "proxyjump",
];

/// The options [`super::remote_query::advertises_any_ref`] gives `ssh`
/// (its `HERMETIC_SSH` after the program name): no configuration file.
const HERMETIC_SSH_OPTIONS: &[&str] = &["-F", "none", "-o", "BatchMode=yes"];

/// Refuse when, for an SSH `url`, the endpoint a later git fetch or push
/// reaches can differ from the one the hermetic query asks. The query runs
/// plain `ssh` with no configuration file; git runs the program
/// `GIT_SSH_COMMAND`, `core.sshCommand` or `GIT_SSH` names, or `ssh` with
/// the user's SSH configuration (`HostName`, `Port`, `User`,
/// `ProxyCommand`, `ProxyJump` and the like). A configured program is
/// refused, since what it reaches cannot be read. Then, because `ssh` runs
/// a `Match exec` command while it reads its configuration, every SSH
/// configuration file `ssh` could read (the system files, `~/.ssh/config`
/// under `HOME` and under the account's home, and every file their
/// includes can name) is read as text first, fail-closed
/// (`refuse_ssh_commands`); `[policy_authority] accept_ssh_configuration`
/// in `~/.codeflow/authority/config.toml` under the account's home from the
/// password database, off by default, skips that read. Only then both views are resolved with `ssh -G`, which
/// connects nowhere, and every setting in [`SSH_ENDPOINT_KEYS`] must agree.
/// Any doubt refuses. Other transports pass. Owned by TSK-1184; the
/// pre-push destination check (TSK-327) can call it for the URL it asks,
/// from the push's view of the configuration.
///
/// # Errors
///
/// The configuration cannot be read, a program is configured, the URL
/// names no host this check can pass to `ssh`, an SSH configuration file
/// is refused by the text check, `ssh -G` fails, or the two endpoints
/// differ.
pub fn require_hermetic_ssh_endpoint(
    root: &Path,
    url: &str,
    limits: QueryLimits,
) -> Result<(), String> {
    if transport(url) != "ssh" {
        return Ok(());
    }
    let entries =
        entries(root, limits).map_err(|why| format!("the configuration cannot be read ({why})"))?;
    ssh_endpoint_agrees(&entries, url, limits)
}

fn ssh_endpoint_agrees(entries: &[Entry], url: &str, limits: QueryLimits) -> Result<(), String> {
    if transport(url) != "ssh" {
        return Ok(());
    }
    refuse_ssh_program_choice(entries, |name| std::env::var_os(name))?;
    let target = ssh_target(url)?;
    ssh_views_agree(&SshView::discover()?, &target, url, limits)
}

/// Where a relative `Include` in an SSH configuration file resolves: a
/// user file's in `~/.ssh` of every home `ssh` may expand `~` to, a system
/// file's in the system directory. Nested files keep their top file's base.
#[derive(Clone)]
enum IncludeBase {
    User,
    System(std::path::PathBuf),
}

/// The `ssh` git would run and every configuration file it may read by
/// itself: the system files the program names, then `~/.ssh/config` under
/// each home in `homes`. `options` go before `-G` for the user's view;
/// production passes none. `accept` is the allow
/// [`accepts_ssh_configuration`] reads.
struct SshView {
    program: std::path::PathBuf,
    options: Vec<std::ffi::OsString>,
    files: Vec<(std::path::PathBuf, IncludeBase)>,
    homes: Vec<std::path::PathBuf>,
    accept: bool,
}

impl SshView {
    fn discover() -> Result<Self, String> {
        let program = ssh_on_path()?;
        let account = account_home()?;
        let accept = accepts_ssh_configuration(&account)?;
        let homes = ssh_homes(account, std::env::var_os("HOME"))?;
        let mut files: Vec<(std::path::PathBuf, IncludeBase)> = system_ssh_configs(&program)?
            .into_iter()
            .map(|file| {
                let base = file
                    .parent()
                    .map_or_else(|| file.clone(), Path::to_path_buf);
                (file, IncludeBase::System(base))
            })
            .collect();
        for home in &homes {
            files.push((home.join(".ssh/config"), IncludeBase::User));
        }
        Ok(Self {
            program,
            options: Vec::new(),
            files,
            homes,
            accept,
        })
    }
}

/// Every directory `ssh` may expand `~` to: `HOME` when it is set (as
/// `glob` with `GLOB_TILDE` expands it), and the account's home from the
/// password database (where `ssh` opens `~/.ssh/config`). `HOME` comes
/// first, so a refusal under it is found before the account's files are
/// read. A `HOME` that is empty or relative cannot be resolved and refuses.
fn ssh_homes(
    account: std::path::PathBuf,
    home: Option<std::ffi::OsString>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let Some(home) = home else {
        return Ok(vec![account]);
    };
    let home = std::path::PathBuf::from(home);
    if !home.is_absolute() {
        return Err(format!(
            "HOME is {}, which is not an absolute path, so the SSH configuration files ssh reads cannot be found",
            home.display()
        ));
    }
    if home == account {
        return Ok(vec![account]);
    }
    Ok(vec![home, account])
}

/// Where the SSH configuration allow lives under the account's home: the
/// operator-authority directory, which the shipped agent presets deny to
/// the harness sandbox and edit-guard refuses to edit.
const SSH_ALLOW_FILE: &str = ".codeflow/authority/config.toml";

/// The allow file's `[policy_authority]` table.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct SshAllowFile {
    policy_authority: SshAllowSection,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct SshAllowSection {
    accept_ssh_configuration: bool,
}

/// `[policy_authority] accept_ssh_configuration` in
/// `<account>/.codeflow/authority/config.toml`. `account` is the home from
/// the password database: neither `HOME` nor `CODEFLOW_HOME` is read, so
/// the command whose remote is judged cannot point this read at a file it
/// wrote. Off when the file does not exist. A file that is a link, not a
/// regular file, not owned by this account, writable by others, over the
/// size limit, or that cannot be read or parsed refuses.
fn accepts_ssh_configuration(account: &Path) -> Result<bool, String> {
    use std::io::Read as _;
    let path = account.join(SSH_ALLOW_FILE);
    let refuse = |why: String| {
        format!(
            "the SSH configuration allow {} {why}, so whether it accepts the SSH configuration is unknown",
            path.display()
        )
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(refuse(format!("cannot be opened ({e})"))),
    };
    let meta = file
        .metadata()
        .map_err(|e| refuse(format!("cannot be read ({e})")))?;
    if !meta.file_type().is_file() {
        return Err(refuse("is not a regular file".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let owner = unsafe { libc::geteuid() };
        if meta.uid() != owner || meta.mode() & 0o022 != 0 {
            return Err(refuse(
                "is not owned by this account or can be written by others".into(),
            ));
        }
    }
    if meta.len() > SSH_CONFIG_BYTES {
        return Err(refuse("is over 1 MiB".into()));
    }
    if let Some(entry) = misspelled_allow_entry(account)? {
        return Err(refuse(format!(
            "is reached through {}, spelled differently from {SSH_ALLOW_FILE}",
            entry.display()
        )));
    }
    let mut text = String::new();
    (&mut file)
        .take(SSH_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| refuse(format!("cannot be read ({e})")))?;
    if text.len() as u64 > SSH_CONFIG_BYTES {
        return Err(refuse("is over 1 MiB".into()));
    }
    toml::from_str::<SshAllowFile>(&text)
        .map(|allow| allow.policy_authority.accept_ssh_configuration)
        .map_err(|e| refuse(format!("cannot be parsed ({e})")))
}

/// The first entry on the way to the allow file whose stored name differs
/// from [`SSH_ALLOW_FILE`] only by case. On a volume that folds case, the
/// open above reaches `.codeflow/Authority/config.toml` too, a spelling a
/// guard matching the exact path may not have refused, so it does not count
/// as the allow.
fn misspelled_allow_entry(account: &Path) -> Result<Option<std::path::PathBuf>, String> {
    let mut dir = account.to_path_buf();
    for name in SSH_ALLOW_FILE.split('/') {
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            format!(
                "the directory {} cannot be listed ({e}), so the SSH configuration allow's spelling is unknown",
                dir.display()
            )
        })?;
        let mut exact = false;
        for entry in entries {
            let entry = entry.map_err(|e| {
                format!(
                    "the directory {} cannot be listed ({e}), so the SSH configuration allow's spelling is unknown",
                    dir.display()
                )
            })?;
            let found = entry.file_name();
            if found == name {
                exact = true;
            } else if found
                .to_str()
                .is_some_and(|found| found.eq_ignore_ascii_case(name))
            {
                return Ok(Some(dir.join(found)));
            }
        }
        if !exact {
            return Ok(Some(dir.join(name)));
        }
        dir.push(name);
    }
    Ok(None)
}

/// The `ssh` a shell finds first on `PATH`, as git's `ssh` resolves. An
/// empty or relative entry before it names a directory that depends on
/// where git runs, so it refuses.
fn ssh_on_path() -> Result<std::path::PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or("PATH is not set, so the ssh git runs is unknown")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() || dir.is_relative() {
            return Err(format!(
                "PATH names the directory {} relative to where git runs, so the ssh git runs is unknown",
                dir.display()
            ));
        }
        let candidate = dir.join(if cfg!(windows) { "ssh.exe" } else { "ssh" });
        let found = crate::absence::metadata_optional(&candidate).map_err(|error| {
            format!(
                "{} cannot be inspected ({error}), so the ssh git runs is unknown",
                candidate.display()
            )
        })?;
        if let Some(metadata) = found {
            #[cfg(unix)]
            let runnable = {
                use std::os::unix::fs::PermissionsExt as _;
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let runnable = metadata.is_file();
            if runnable {
                return Ok(candidate);
            }
        }
    }
    Err("no ssh was found on PATH".into())
}

/// The system configuration files `program` reads: the absolute paths
/// ending in `/ssh_config` that its file names (OpenSSH compiles its
/// system file path in). None found refuses: what it reads is unknown.
fn system_ssh_configs(program: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    const MOST: u64 = 64 << 20;
    let cannot = |why: String| {
        format!(
            "the system SSH configuration {} reads cannot be found ({why})",
            program.display()
        )
    };
    let size = std::fs::metadata(program)
        .map_err(|error| cannot(error.to_string()))?
        .len();
    if size > MOST {
        return Err(cannot("the program is larger than 64 MiB".into()));
    }
    let bytes = std::fs::read(program).map_err(|error| cannot(error.to_string()))?;
    let found = system_config_names(&bytes);
    if found.is_empty() {
        return Err(cannot("it names no ssh_config file".into()));
    }
    Ok(found)
}

/// The words in `bytes`, split on bytes that are not printable ASCII or
/// are a space, that are absolute paths ending in `/ssh_config`.
fn system_config_names(bytes: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = Vec::new();
    for word in bytes.split(|byte| !byte.is_ascii_graphic()) {
        if word.starts_with(b"/") && word.ends_with(b"/ssh_config") {
            // Every byte is printable ASCII, so each is one character.
            let text: String = word.iter().map(|byte| char::from(*byte)).collect();
            let path = std::path::PathBuf::from(text);
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The account's home directory, where OpenSSH looks for `~/.ssh/config`
/// whatever `HOME` says. A relative entry would name a directory that
/// depends on where the command runs, so it refuses.
#[cfg(unix)]
pub(crate) fn account_home() -> Result<std::path::PathBuf, String> {
    use std::os::unix::ffi::OsStrExt as _;
    let mut buffer = vec![0_u8; 1 << 16];
    // SAFETY: an all-zero `passwd` is a valid value of a plain C struct;
    // `getpwuid_r` fills it.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the call and `buffer` outlives
    // every use of the strings `entry` points into.
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &raw mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut result,
        )
    };
    if code != 0 || result.is_null() || entry.pw_dir.is_null() {
        return Err("the account's home directory cannot be read, so its SSH configuration cannot be checked".into());
    }
    // SAFETY: `pw_dir` is a NUL-terminated string inside `buffer`.
    let dir = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
    let dir = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
    if !dir.is_absolute() {
        return Err(format!(
            "the account's home directory is {}, which is not an absolute path, so its SSH configuration cannot be checked",
            dir.display()
        ));
    }
    Ok(dir)
}

#[cfg(not(unix))]
pub(crate) fn account_home() -> Result<std::path::PathBuf, String> {
    Err(
        "the SSH configuration files cannot be located on this platform, so the check refuses an SSH remote"
            .into(),
    )
}

/// Most nested `Include` levels, files and bytes per file read while
/// proving a configuration runs no command; past them it refuses.
const SSH_INCLUDE_DEPTH: usize = 16;
const SSH_CONFIG_FILES: usize = 256;
const SSH_CONFIG_BYTES: u64 = 1 << 20;

/// Keywords whose value `ssh` runs as a command at some point. The text
/// check refuses them wherever they appear, since this reader does not
/// prove which of them `ssh -G` reaches.
const SSH_COMMAND_KEYWORDS: &[&str] = &["proxycommand", "knownhostscommand", "localcommand"];

/// What an adopter does about a refusal of their SSH configuration.
const SSH_CONFIGURATION_REMEDY: &str = "remove that line or move it out of the files ssh reads by default; or fetch the remote once it holds its default branch; or, to let ssh read this account's SSH configuration as it is, set accept_ssh_configuration = true under [policy_authority] in ~/.codeflow/authority/config.toml, which the operator edits and the shipped agent presets deny to agents";

/// Compare the endpoint `view` resolves for `target` with the hermetic one.
/// Unless `view.accept`, every configuration file is first proven by
/// [`refuse_ssh_commands`] to give `ssh` nothing to run while it reads it.
fn ssh_views_agree(
    view: &SshView,
    target: &SshTarget,
    url: &str,
    limits: QueryLimits,
) -> Result<(), String> {
    if !view.accept {
        let mut read = 0;
        for (file, base) in &view.files {
            refuse_ssh_commands(file, base, &view.homes, 0, &mut read)?;
        }
    }
    let configured = ssh_endpoint(&view.program, &view.options, target, limits)?;
    let hermetic: Vec<std::ffi::OsString> = HERMETIC_SSH_OPTIONS.iter().map(Into::into).collect();
    let hermetic = ssh_endpoint(&view.program, &hermetic, target, limits)?;
    let differ: Vec<String> = SSH_ENDPOINT_KEYS
        .iter()
        .filter_map(|key| {
            let mine = configured.get(*key);
            let asked = hermetic.get(*key);
            (mine != asked).then(|| {
                format!(
                    "{key} {} where the hermetic question uses {}",
                    shown_setting(mine),
                    shown_setting(asked)
                )
            })
        })
        .collect();
    if differ.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the SSH configuration git uses for {} reaches a different endpoint ({}) from the one the hermetic question asks, so its answer would not cover a later fetch or push",
        shown(url),
        differ.join("; ")
    ))
}

/// Refuse, fail-closed, unless `file` and every file its `Include` lines
/// can name are proven to give `ssh` no command to run while it reads them.
/// This is not a parser of `ssh_config`: it reads more than `ssh` does and
/// refuses more than `ssh` would run, and that over-refusal is accepted.
///
/// Every line that does not start with `#` is split into words, double and
/// single quotes grouping and then dropped, backslashes dropped, and each
/// word split again at `=`, all compared in lower case. A line refuses
/// when any word contains `exec`, is `match`, is a command keyword
/// ([`SSH_COMMAND_KEYWORDS`]), or is `permitlocalcommand` not followed by
/// `no`; or when a quote does not close. An `include` line is followed into
/// every file it can name (see [`included_ssh_files`]). A missing file
/// holds nothing. A file that is not a readable UTF-8 regular file within
/// [`SSH_CONFIG_BYTES`], an `Include` that cannot be resolved, or more than
/// [`SSH_INCLUDE_DEPTH`] levels or [`SSH_CONFIG_FILES`] files refuses.
fn refuse_ssh_commands(
    file: &Path,
    base: &IncludeBase,
    homes: &[std::path::PathBuf],
    depth: usize,
    read: &mut usize,
) -> Result<(), String> {
    let cannot = |why: String| {
        format!(
            "the SSH configuration {} cannot be read in full ({why}), so it cannot be shown to run no command while ssh reads it and ssh is not asked to read it: {SSH_CONFIGURATION_REMEDY}",
            file.display()
        )
    };
    let Some(metadata) =
        crate::absence::metadata_optional(file).map_err(|error| cannot(error.to_string()))?
    else {
        return Ok(());
    };
    *read += 1;
    if depth > SSH_INCLUDE_DEPTH || *read > SSH_CONFIG_FILES {
        return Err(cannot(format!(
            "it is past {SSH_INCLUDE_DEPTH} include levels or {SSH_CONFIG_FILES} files"
        )));
    }
    if !metadata.is_file() {
        return Err(cannot("it is not a regular file".into()));
    }
    if metadata.len() > SSH_CONFIG_BYTES {
        return Err(cannot("it is larger than 1 MiB".into()));
    }
    let bytes = std::fs::read(file).map_err(|error| cannot(error.to_string()))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| cannot("it is not UTF-8 text".into()))?;
    for (number, line) in text.split('\n').enumerate() {
        // `ssh` separates words with space, tab and carriage return, as here.
        let line = line.trim_matches([' ', '\t', '\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let refuse = |what: &str| {
            format!(
                "the SSH configuration {} line {} {what}, and ssh may run a command while it reads its configuration, so ssh is not asked to read it: {SSH_CONFIGURATION_REMEDY}",
                file.display(),
                number + 1
            )
        };
        let words = ssh_words(line).ok_or_else(|| refuse("has a quote that does not close"))?;
        let tokens: Vec<String> = words
            .iter()
            .flat_map(|word| {
                word.replace('\\', "")
                    .split('=')
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<_>>()
            })
            .filter(|token| !token.is_empty())
            .collect();
        if tokens.iter().any(|token| token.contains("exec")) {
            return Err(refuse(
                "has the word exec (a Match exec criterion runs a command)",
            ));
        }
        for (at, token) in tokens.iter().enumerate() {
            if token == "match" {
                return Err(refuse(
                    "has a Match block, whose criteria this check does not evaluate",
                ));
            }
            if SSH_COMMAND_KEYWORDS.contains(&token.as_str()) {
                return Err(refuse(&format!("names the command setting {token}")));
            }
            if token == "permitlocalcommand" && tokens.get(at + 1).map(String::as_str) != Some("no")
            {
                return Err(refuse("may allow a local command (permitlocalcommand)"));
            }
        }
        if tokens.first().map(String::as_str) != Some("include") {
            continue;
        }
        let at = |why: &str| cannot(format!("line {} has an Include {why}", number + 1));
        let mut patterns: Vec<String> = Vec::new();
        if let Some((_, first)) = words[0].split_once('=') {
            patterns.push(first.to_string());
        }
        patterns.extend(words[1..].iter().cloned());
        for pattern in patterns.iter().filter(|pattern| !pattern.is_empty()) {
            if pattern.starts_with('#') {
                break;
            }
            for child in included_ssh_files(pattern, base, homes).map_err(|why| at(&why))? {
                refuse_ssh_commands(&child, base, homes, depth + 1, read)?;
            }
        }
    }
    Ok(())
}

/// The words of an `ssh_config` line, split on space, tab and carriage
/// return, with double or single quotes grouping and removed; `None` for a
/// quote that does not close.
fn ssh_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in text.chars() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if matches!(c, ' ' | '\t' | '\r') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

/// Every file an `Include` `pattern` can name, sorted as `glob` sorts them.
/// A leading `~/` is tried under each of `homes`; a relative path is in
/// `~/.ssh` of each home for a user file, or in the system directory for a
/// system file. `*` and `?` match within the last component, skipping dot
/// files unless the pattern starts with a dot. Anything this reader cannot
/// expand as `ssh` does (an environment or `%` token, `~user`, a bracket, a
/// backslash, a wildcard in a directory) is an error.
fn included_ssh_files(
    pattern: &str,
    base: &IncludeBase,
    homes: &[std::path::PathBuf],
) -> Result<Vec<std::path::PathBuf>, String> {
    let shown_pattern = super::display_text(pattern);
    if pattern.contains(['$', '%', '[', '\\']) {
        return Err(format!("{shown_pattern} this reader cannot expand"));
    }
    let paths: Vec<std::path::PathBuf> = if let Some(rest) = pattern.strip_prefix("~/") {
        homes.iter().map(|home| home.join(rest)).collect()
    } else if pattern.starts_with('~') {
        return Err(format!("{shown_pattern} naming another account's home"));
    } else if Path::new(pattern).is_absolute() {
        vec![std::path::PathBuf::from(pattern)]
    } else {
        match base {
            IncludeBase::System(dir) => vec![dir.join(pattern)],
            IncludeBase::User => homes
                .iter()
                .map(|home| home.join(".ssh").join(pattern))
                .collect(),
        }
    };
    let mut found = Vec::new();
    for path in paths {
        found.extend(matching_files(&path, &shown_pattern)?);
    }
    Ok(found)
}

/// The files `path` names, expanding `*` and `?` in its last component.
fn matching_files(path: &Path, shown_pattern: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let (Some(dir), Some(name)) = (
        path.parent(),
        path.file_name().and_then(|name| name.to_str()),
    ) else {
        return Err(format!("{shown_pattern} with no file name"));
    };
    if dir.to_str().is_none_or(|dir| dir.contains(['*', '?'])) {
        return Err(format!(
            "{shown_pattern} with a wildcard outside its file name"
        ));
    }
    if !name.contains(['*', '?']) {
        return Ok(vec![path.to_path_buf()]);
    }
    let unreadable = |error: std::io::Error| {
        format!(
            "{shown_pattern} whose folder {} cannot be read ({error})",
            dir.display()
        )
    };
    let Some(_) = crate::absence::metadata_optional(dir).map_err(unreadable)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(unreadable)? {
        let file_name = entry.map_err(unreadable)?.file_name();
        let Some(candidate) = file_name.to_str() else {
            return Err(format!(
                "{shown_pattern} whose folder holds a name that is not UTF-8"
            ));
        };
        if (candidate.starts_with('.') && !name.starts_with('.'))
            || !wildcard_matches(name, candidate)
        {
            continue;
        }
        found.push(dir.join(candidate));
    }
    found.sort();
    Ok(found)
}

/// Whether `text` matches `pattern`, where `*` is any run and `?` any one
/// character.
fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut resume: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            resume = Some((p, t));
            p += 1;
        } else if let Some((star, at)) = resume {
            p = star + 1;
            t = at + 1;
            resume = Some((star, at + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

fn shown_setting(value: Option<&String>) -> String {
    value.map_or_else(|| "nothing".into(), |value| super::display_text(value))
}

/// Refuse when the SSH program git would run is configured: whatever it
/// connects to cannot be read here. `GIT_SSH_COMMAND` and `GIT_SSH` come
/// from `lookup` (the environment a later git inherits); `core.sshCommand`
/// and an `ssh.variant` other than `ssh` or `auto` from the configuration.
fn refuse_ssh_program_choice(
    entries: &[Entry],
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<(), String> {
    for variable in ["GIT_SSH_COMMAND", "GIT_SSH"] {
        if lookup(variable).is_some() {
            return Err(format!(
                "{variable} is set, so git would run a chosen SSH program whose endpoint cannot be read here"
            ));
        }
    }
    for entry in entries {
        let chosen = match entry.key.as_str() {
            "core.sshcommand" => true,
            "ssh.variant" => !matches!(entry.value.as_str(), "ssh" | "auto"),
            _ => false,
        };
        if chosen {
            return Err(format!(
                "{} is set to {} ({}), so git would run a chosen SSH program whose endpoint cannot be read here",
                entry.key,
                super::display_text(&entry.value),
                super::display_text(&entry.origin)
            ));
        }
    }
    Ok(())
}

/// The arguments git passes `ssh` for `url`: an optional `-p <port>`, then
/// `[user@]host`. A form this check cannot read as git does refuses.
#[derive(Debug, PartialEq, Eq)]
struct SshTarget {
    port: Option<String>,
    destination: String,
}

fn ssh_target(url: &str) -> Result<SshTarget, String> {
    let cannot = || {
        format!(
            "the SSH address in {} cannot be read as git reads it, so its endpoint cannot be checked",
            shown(url)
        )
    };
    let (authority, scheme_form) = match url.find("://") {
        Some(at) => {
            let rest = &url[at + 3..];
            (&rest[..rest.find('/').unwrap_or(rest.len())], true)
        }
        None => (&url[..url.find(':').ok_or_else(cannot)?], false),
    };
    let (user, hostport) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };
    let (host, port) = if !scheme_form {
        (hostport, None)
    } else if let Some(inner) = hostport.strip_prefix('[') {
        let (host, after) = inner.split_once(']').ok_or_else(cannot)?;
        (host, after.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (hostport, None),
        }
    };
    let port = port.filter(|port| !port.is_empty());
    let plain = |text: &str| {
        !text.is_empty()
            && !text.starts_with('-')
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
    };
    if !plain(host)
        || user.is_some_and(|user| !plain(user))
        || port.is_some_and(|port| !port.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(cannot());
    }
    Ok(SshTarget {
        port: port.map(str::to_string),
        destination: match user {
            Some(user) => format!("{user}@{host}"),
            None => host.to_string(),
        },
    })
}

/// The endpoint settings `ssh -G` resolves for `target` with `options`
/// before the target. `ssh -G` prints the configuration and connects
/// nowhere.
fn ssh_endpoint(
    program: &Path,
    options: &[std::ffi::OsString],
    target: &SshTarget,
    limits: QueryLimits,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut command = std::process::Command::new(program);
    command.args(options).arg("-G");
    if let Some(port) = &target.port {
        command.arg("-p").arg(port);
    }
    command.arg(&target.destination).env("LC_ALL", "C");
    let hermetic: Vec<std::ffi::OsString> = HERMETIC_SSH_OPTIONS.iter().map(Into::into).collect();
    let view = if options == hermetic.as_slice() {
        "no SSH configuration"
    } else {
        "the user's SSH configuration"
    };
    let answer = super::remote_query::bounded_text(command, limits.timeout())
        .map_err(|why| format!("ssh could not resolve its settings with {view} ({why})"))?;
    let endpoint = parse_ssh_settings(&answer);
    if !endpoint.contains_key("hostname") {
        return Err(format!("ssh resolved no host name with {view}"));
    }
    Ok(endpoint)
}

/// The [`SSH_ENDPOINT_KEYS`] in `ssh -G` output, one `key value` per line
/// split on the newline and the first space it prints; a value of `none`
/// is no setting.
fn parse_ssh_settings(answer: &str) -> std::collections::BTreeMap<String, String> {
    answer
        .split('\n')
        .filter_map(|line| line.split_once(' '))
        .filter(|(key, value)| SSH_ENDPOINT_KEYS.contains(key) && *value != "none")
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Check that remote `name`, configured as `raw`, is the remote git fetches
/// from and pushes to, and that its transport is on the hermetic allow-list;
/// then `raw` is the identity the hermetic query may ask. Each failure is a
/// refusal: a transport outside the list or replaced by a remote setting
/// ([`require_native_transport`]), a remote setting that a path-dependent
/// include may apply, an effective URL that cannot be read, one that
/// differs from `raw`, each named with both identities, and, for an SSH
/// remote, an SSH client or configuration that reaches another endpoint
/// ([`require_hermetic_ssh_endpoint`]).
///
/// # Errors
///
/// The reason the identity is not verified, in display form.
pub fn verify(root: &Path, name: &str, raw: &str, limits: QueryLimits) -> Result<(), String> {
    let remedy = format!(
        "the operator inspects git config --show-origin --get-regexp '^(url|include|includeif|remote\\.{name})\\.'"
    );
    let entries = entries(root, limits).map_err(|why| {
        format!(
            "the configuration git applies for {name} cannot be read ({}); {remedy}",
            super::display_text(&why)
        )
    })?;
    refuse_transport_settings(&entries, name).map_err(|why| format!("{why}; {remedy}"))?;
    refuse_path_conditional_remote_settings(&entries, root, limits)
        .map_err(|why| format!("{why}; {remedy}"))?;
    for (push, verb) in [(false, "fetches from"), (true, "pushes to")] {
        let urls = effective_urls(root, name, push, limits).map_err(|why| {
            format!(
                "the URL git {verb} for {name} cannot be read ({}), so it cannot be compared with remote.{name}.url; {remedy}",
                super::display_text(&why)
            )
        })?;
        for url in &urls {
            require_safe_protocol(url)
                .map_err(|why| format!("{name} resolves to a URL git {verb}: {why}; {remedy}"))?;
        }
        if urls.as_slice() != [raw] {
            let effective: Vec<String> = urls.iter().map(|url| shown(url)).collect();
            return Err(format!(
                "remote.{name}.url is {} but git {verb} {}; an insteadOf, pushInsteadOf or pushurl setting, or an included configuration file, makes them differ, so asking the configured URL would not ask the remote git uses; {remedy}",
                shown(raw),
                effective.join(", ")
            ));
        }
    }
    require_safe_protocol(raw).map_err(|why| format!("{why}; {remedy}"))?;
    ssh_endpoint_agrees(&entries, raw, limits).map_err(|why| {
        format!("{why}; the operator compares the host's resolved SSH settings with and without the SSH configuration files, or fetches the remote once it holds its default branch")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transports_follow_git_selection() {
        for (url, expected) in [
            ("ext::sh -c touch% x", "ext"),
            ("git://127.0.0.1:9/repo.git", "git"),
            ("https://example.invalid/r.git", "https"),
            ("http://example.invalid/r.git", "http"),
            ("ssh://git@example.invalid/r.git", "ssh"),
            ("git+ssh://example.invalid/r.git", "ssh"),
            ("ssh+git://example.invalid/r.git", "ssh"),
            ("file:///srv/r.git", "file"),
            ("ftp://example.invalid/r.git", "ftp"),
            ("git@example.invalid:owner/r.git", "ssh"),
            ("/srv/r.git", "file"),
            ("../r.git", "file"),
            ("./a:b", "file"),
            ("fd::17", "fd"),
        ] {
            assert_eq!(transport(url), expected, "{url}");
        }
    }

    #[test]
    fn the_allow_list_is_named_and_never_widened() {
        for url in ["ext::sh -c x", "git://h/r", "ftp://h/r", "fd::3"] {
            let error = require_safe_protocol(url).unwrap_err();
            assert!(error.contains(SAFE_PROTOCOLS), "{error}");
            assert!(
                error.contains(&format!("the {} protocol", transport(url))),
                "{error}"
            );
        }
        for url in ["https://h/r", "ssh://h/r", "h:r", "/srv/r", "file:///srv/r"] {
            assert!(require_safe_protocol(url).is_ok(), "{url}");
        }
    }

    /// AC-2: when git cannot name the effective URL (here, no repository at
    /// all), the identity is not verified and the caller refuses.
    #[test]
    fn an_unreadable_effective_url_is_not_verified() {
        let dir = tempfile::tempdir().unwrap();
        let error = verify(dir.path(), "origin", "/srv/r.git", QueryLimits::guard()).unwrap_err();
        assert!(error.contains("cannot be read"), "{error}");
    }

    /// AC-2: the hermetic query keeps its transports and inherited
    /// variables; this task adds a check before it and widens nothing.
    #[test]
    fn the_hermetic_query_environment_is_unchanged() {
        assert_eq!(SAFE_PROTOCOLS, "file:http:https:ssh");
        assert_eq!(
            super::super::remote_query::KEPT_ENVIRONMENT,
            &[
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
            ]
        );
    }

    #[test]
    fn listings_parse_with_and_without_origins() {
        let listed = parse_entries(
            b"file:/r/.git/config\0remote.origin.url\n/srv/r\0command line:\0core.bare\0",
        )
        .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].origin, "file:/r/.git/config");
        assert_eq!(listed[0].key, "remote.origin.url");
        assert_eq!(listed[0].value, "/srv/r");
        assert_eq!(listed[1].key, "core.bare");
        assert_eq!(listed[1].value, "");
        let plain = parse_listing(b"url.a.insteadof\nb\0", false).unwrap();
        assert_eq!(plain[0].key, "url.a.insteadof");
        assert!(
            parse_entries(b"file:/x\0").is_err(),
            "an origin with no entry"
        );
    }

    /// An unrelated entry that is not UTF-8 is read lossily and never stops
    /// the checks; an include entry that is not UTF-8 refuses.
    #[test]
    fn entries_that_are_not_utf8_refuse_only_where_acted_on() {
        let listed = parse_entries(
            b"file:/r/.git/config\0remote.fixture.url\next::x\xff\0file:/r/.git/config\0remote.origin.url\n/srv/r\0",
        )
        .unwrap();
        assert_eq!(listed.len(), 2);
        assert!(!listed[0].exact);
        assert!(listed[1].exact);
        assert!(refuse_transport_settings(&listed, "origin").is_ok());
        assert!(refuse_path_conditional_remote_settings(
            &listed,
            Path::new("/r"),
            QueryLimits::guard()
        )
        .is_ok());
        let include =
            parse_entries(b"file:/r/.git/config\0includeif.gitdir:/x/.git.path\n/w\xff.config\0")
                .unwrap();
        let error = refuse_path_conditional_remote_settings(
            &include,
            Path::new("/r"),
            QueryLimits::guard(),
        )
        .unwrap_err();
        assert!(error.contains("not valid UTF-8"), "{error}");
    }

    #[test]
    fn include_paths_resolve_as_git_resolves_them() {
        let entry = |origin: &str, value: &str| Entry {
            origin: origin.into(),
            key: "includeif.gitdir:/x/.git.path".into(),
            value: value.into(),
            exact: true,
        };
        let root = Path::new("/srv/repo");
        assert_eq!(
            included_path(&entry("file:/home/u/.gitconfig", "work.config"), root),
            Some(std::path::PathBuf::from("/home/u/work.config"))
        );
        // Git run with `-C root` names the repository file relative to
        // root; the include resolves beside it, never in this process's
        // directory (TSK-1184 delta finding 1).
        assert_eq!(
            included_path(&entry("file:.git/config", "rel.config"), root),
            Some(std::path::PathBuf::from("/srv/repo/.git/rel.config"))
        );
        assert_eq!(
            included_path(&entry("file:/home/u/.gitconfig", "/etc/w.config"), root),
            Some(std::path::PathBuf::from("/etc/w.config"))
        );
        assert_eq!(
            included_path(&entry("command line:", "rel.config"), root),
            None
        );
        assert_eq!(
            included_path(&entry("file:/h/.gitconfig", "~other/x"), root),
            None
        );
        assert_eq!(included_path(&entry("file:/h/.gitconfig", ""), root), None);
    }

    #[test]
    fn transport_settings_refuse_only_their_section() {
        let entries = [
            Entry {
                origin: "file:/r/.git/config".into(),
                key: "remote.other.vcs".into(),
                value: "touchy".into(),
                exact: true,
            },
            Entry {
                origin: "file:/r/.git/config".into(),
                key: "remote.origin.prune".into(),
                value: "true".into(),
                exact: true,
            },
        ];
        assert!(refuse_transport_settings(&entries, "origin").is_ok());
        let error = refuse_transport_settings(&entries, "other").unwrap_err();
        assert!(error.contains("remote.other.vcs"), "{error}");
        assert!(error.contains("touchy"), "{error}");
    }

    #[test]
    fn userinfo_is_hidden_in_refusals() {
        let error = require_safe_protocol("ftp://user:FAKE@h/r").unwrap_err();
        assert!(!error.contains("FAKE"), "{error}");
    }

    /// Delta finding 2: the arguments git gives `ssh`, and every form this
    /// check cannot pass on as git does refuses.
    #[test]
    fn ssh_targets_follow_git_and_refuse_the_unreadable() {
        let target = |port: Option<&str>, destination: &str| SshTarget {
            port: port.map(str::to_string),
            destination: destination.into(),
        };
        for (url, expected) in [
            ("ssh://127.0.0.1/r.git", target(None, "127.0.0.1")),
            (
                "ssh://git@127.0.0.1:2222/r.git",
                target(Some("2222"), "git@127.0.0.1"),
            ),
            ("git+ssh://localhost:/r.git", target(None, "localhost")),
            ("ssh://[::1]:22/r.git", target(Some("22"), "::1")),
            ("git@127.0.0.1:owner/r.git", target(None, "git@127.0.0.1")),
            ("localhost:r.git", target(None, "localhost")),
        ] {
            assert_eq!(ssh_target(url).unwrap(), expected, "{url}");
        }
        for url in [
            "ssh://-oProxyCommand=x/r",
            "ssh://127.0.0.1:22x/r",
            "ssh://127.0.0.1%2e/r",
            "[127.0.0.1:22]:r",
            "ssh://a b@127.0.0.1/r",
            "ssh:///r",
        ] {
            assert!(ssh_target(url).is_err(), "{url}");
        }
    }

    /// Delta finding 2: a chosen SSH program refuses, from the environment
    /// or the configuration; `ssh.variant` of `ssh` or `auto` does not.
    #[test]
    fn a_chosen_ssh_program_refuses() {
        let none = |_: &str| None;
        let entry = |key: &str, value: &str| Entry {
            origin: "file:/r/.git/config".into(),
            key: key.into(),
            value: value.into(),
            exact: true,
        };
        assert!(refuse_ssh_program_choice(&[], none).is_ok());
        for variable in ["GIT_SSH_COMMAND", "GIT_SSH"] {
            let error = refuse_ssh_program_choice(&[], |name: &str| {
                (name == variable).then(|| "wrapper".into())
            })
            .unwrap_err();
            assert!(error.contains(variable), "{error}");
        }
        let error =
            refuse_ssh_program_choice(&[entry("core.sshcommand", "ssh -i k")], none).unwrap_err();
        assert!(error.contains("core.sshcommand"), "{error}");
        let error = refuse_ssh_program_choice(&[entry("ssh.variant", "plink")], none).unwrap_err();
        assert!(error.contains("ssh.variant"), "{error}");
        for variant in ["ssh", "auto"] {
            assert!(refuse_ssh_program_choice(&[entry("ssh.variant", variant)], none).is_ok());
        }
    }

    #[test]
    fn ssh_settings_keep_only_endpoint_keys() {
        let parsed = parse_ssh_settings(
            "user me\nhostname h.invalid\nport 22\nproxycommand none\nidentityfile ~/.ssh/id\nproxyjump j.invalid\n",
        );
        assert_eq!(
            parsed.get("hostname").map(String::as_str),
            Some("h.invalid")
        );
        assert_eq!(
            parsed.get("proxyjump").map(String::as_str),
            Some("j.invalid")
        );
        assert!(!parsed.contains_key("proxycommand"));
        assert!(!parsed.contains_key("identityfile"));
    }

    /// Delta finding 2, with the real `ssh -G` and a fixture configuration
    /// file (never the user's): a `Host` block that moves the host and port
    /// is a different endpoint from the hermetic view, and one that changes
    /// nothing on the endpoint is the same.
    #[test]
    fn ssh_configuration_that_moves_the_endpoint_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let moved = dir.path().join("moved");
        std::fs::write(
            &moved,
            "Host 127.0.0.1\n\tHostName 127.0.0.2\n\tPort 2222\n\tProxyJump 127.0.0.3\n",
        )
        .unwrap();
        let same = dir.path().join("same");
        std::fs::write(&same, "Host 127.0.0.1\n\tServerAliveInterval 30\n").unwrap();
        let target = ssh_target("ssh://127.0.0.1/r.git").unwrap();
        let limits = QueryLimits::guard();
        let Some(program) = fixture_ssh() else {
            return;
        };
        let hermetic: Vec<std::ffi::OsString> =
            HERMETIC_SSH_OPTIONS.iter().map(Into::into).collect();
        let hermetic = ssh_endpoint(&program, &hermetic, &target, limits).unwrap();
        let view = |file: &Path| {
            ssh_endpoint(
                &program,
                &fixture_view(&program, file).options,
                &target,
                limits,
            )
            .unwrap()
        };
        let moved_view = view(&moved);
        assert_eq!(
            moved_view.get("hostname").map(String::as_str),
            Some("127.0.0.2")
        );
        assert_ne!(moved_view.get("port"), hermetic.get("port"));
        assert!(moved_view.contains_key("proxyjump"));
        assert_eq!(
            hermetic.get("hostname").map(String::as_str),
            Some("127.0.0.1")
        );
        let same_view = view(&same);
        for key in SSH_ENDPOINT_KEYS {
            assert_eq!(same_view.get(*key), hermetic.get(*key), "{key}");
        }
        let url = "ssh://127.0.0.1/r.git";
        let error =
            ssh_views_agree(&fixture_view(&program, &moved), &target, url, limits).unwrap_err();
        assert!(error.contains("different endpoint"), "{error}");
        ssh_views_agree(&fixture_view(&program, &same), &target, url, limits).unwrap();
    }

    /// The `ssh` on `PATH`, or `None` (the test is skipped) on a host with
    /// none.
    fn fixture_ssh() -> Option<std::path::PathBuf> {
        match ssh_on_path() {
            Ok(program) => Some(program),
            Err(why) => {
                eprintln!("skipped: no ssh on this host ({why})");
                None
            }
        }
    }

    /// A view that reads only the fixture `file` with `program`, as
    /// production reads the system and account files.
    fn fixture_view(program: &Path, file: &Path) -> SshView {
        let dir = file.parent().unwrap().to_path_buf();
        SshView {
            program: program.to_path_buf(),
            options: vec!["-F".into(), file.into()],
            files: vec![(file.to_path_buf(), IncludeBase::System(dir.clone()))],
            homes: vec![dir],
            accept: false,
        }
    }

    /// A stand-in `ssh` in `dir` that records every call in a marker file
    /// and prints the same endpoint for every view; the marker is returned.
    #[cfg(unix)]
    fn recording_ssh(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let asked = dir.join("ssh-was-asked");
        let program = dir.join("recording-ssh");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\n: > '{}'\nprintf 'hostname 127.0.0.1\\nport 22\\n'\n",
                asked.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (program, asked)
    }

    /// Delta findings 3 and 4: `ssh` runs a `Match exec` command while it
    /// reads its configuration, and it removes double quotes from the
    /// keyword, so `"Match" exec`, `Ma"tch" exec` and an `In"clude"` of a
    /// file with `Match exec` all run it. For each, the real `ssh` on the
    /// fixture shows the hazard; then the check refuses, the command never
    /// runs, and `ssh` is never asked to read that configuration.
    #[cfg(unix)]
    #[test]
    fn a_match_exec_configuration_refuses_and_its_command_never_runs() {
        let Some(program) = fixture_ssh() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("command-ran");
        let run = format!("exec \"touch {}\"", marker.display());
        let inner = dir.path().join("inner.conf");
        std::fs::write(&inner, format!("Match {run}\n")).unwrap();
        let (stand_in, asked) = recording_ssh(dir.path());
        let target = ssh_target("ssh://127.0.0.1/r.git").unwrap();
        for (body, names) in [
            (
                format!("Host *\n\tServerAliveInterval 30\nMatch {run}\n"),
                "line 3",
            ),
            (format!("\"Match\" {run}\n"), "line 1"),
            (format!("Ma\"tch\" {run}\n"), "line 1"),
            (format!("In\"clude\" {}\n", inner.display()), "inner.conf"),
        ] {
            let config = dir.path().join("config");
            std::fs::write(&config, &body).unwrap();
            // The check at dbf3f2116 let these through to ssh, as here.
            ssh_endpoint(
                &program,
                &fixture_view(&program, &config).options,
                &target,
                QueryLimits::guard(),
            )
            .unwrap();
            assert!(
                marker.exists(),
                "{body}: ssh ran the command while resolving"
            );
            std::fs::remove_file(&marker).unwrap();

            let error = ssh_views_agree(
                &fixture_view(&stand_in, &config),
                &target,
                "ssh://127.0.0.1/r.git",
                QueryLimits::guard(),
            )
            .unwrap_err();
            assert!(
                error.contains("exec") && error.contains(names),
                "{body}: {error}"
            );
            assert!(error.contains("accept_ssh_configuration"), "{error}");
            assert!(!marker.exists(), "{body}: the command ran");
            assert!(!asked.exists(), "{body}: ssh was asked to read it");
        }
    }

    /// The fail-closed classes beyond `exec`: command keywords, any Match
    /// block, a quote that does not close, and an include that cannot be
    /// read. `PermitLocalCommand no` and plain settings pass.
    #[test]
    fn command_settings_match_blocks_and_unreadable_text_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let base = IncludeBase::System(dir.path().to_path_buf());
        let homes = [dir.path().to_path_buf()];
        let top = dir.path().join("config");
        let scan = |body: &str| {
            std::fs::write(&top, body).unwrap();
            refuse_ssh_commands(&top, &base, &homes, 0, &mut 0)
        };
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        for (body, names) in [
            ("Host a\n\tProxyCommand nc %h %p\n", "proxycommand"),
            (
                "Host a\n\tKnownHostsCommand /bin/true\n",
                "knownhostscommand",
            ),
            ("LocalCommand=true\n", "localcommand"),
            ("PermitLocalCommand yes\n", "permitlocalcommand"),
            ("Match host 127.0.0.1\n\tPort 2222\n", "Match block"),
            ("Host \"a\n", "does not close"),
            ("Include folder\n", "not a regular file"),
        ] {
            let error = scan(body).unwrap_err();
            assert!(error.contains(names), "{body}: {error}");
            assert!(error.contains("accept_ssh_configuration"), "{error}");
        }
        scan("Host a\n\tPermitLocalCommand no\n\tPort 22 # trailing\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let locked = dir.path().join("locked.conf");
            std::fs::write(&locked, "Host b\n").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            // Skipped when this process can read it anyway (root).
            if std::fs::read(&locked).is_err() {
                let error = scan("Include locked.conf\n").unwrap_err();
                assert!(error.contains("cannot be read in full"), "{error}");
            }
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    /// Includes are followed as `ssh` follows them: relative to the base,
    /// `~/` under each home, and wildcards in the file name; an include this
    /// reader cannot resolve, or one too deep, refuses.
    #[test]
    fn included_ssh_configuration_is_read_or_refused() {
        let dir = tempfile::tempdir().unwrap();
        let base_dir = dir.path().join("base");
        let home = dir.path().join("home");
        std::fs::create_dir_all(base_dir.join("conf.d")).unwrap();
        std::fs::create_dir_all(home.join("more")).unwrap();
        let base = IncludeBase::System(base_dir.clone());
        let homes = [home.clone()];
        let top = base_dir.join("config");
        let scan = |body: &str| {
            std::fs::write(&top, body).unwrap();
            refuse_ssh_commands(&top, &base, &homes, 0, &mut 0)
        };
        std::fs::write(base_dir.join("conf.d/a.conf"), "Host a\n\tPort 1\n").unwrap();
        std::fs::write(base_dir.join("conf.d/.hidden.conf"), "Match exec x\n").unwrap();
        std::fs::write(home.join("more/b"), "Host b\n").unwrap();
        scan("Include conf.d/*.conf \"~/more/b\" missing/none\n").unwrap();
        std::fs::write(
            base_dir.join("conf.d/z.conf"),
            "Host z\n  ProxyJump j\n  sessiontype=EXEC\n",
        )
        .unwrap();
        let error = scan("Include=conf.d/*.conf\n").unwrap_err();
        assert!(
            error.contains("z.conf") && error.contains("exec"),
            "{error}"
        );
        std::fs::remove_file(base_dir.join("conf.d/z.conf")).unwrap();
        for refused in [
            "Include ${HOME}/x\n",
            "Include %d/x\n",
            "Include ~other/x\n",
            "Include */x\n",
            "Include conf.d/[ab].conf\n",
        ] {
            let error = scan(refused).unwrap_err();
            assert!(
                error.contains("cannot be read in full"),
                "{refused}: {error}"
            );
        }
        std::fs::write(base_dir.join("loop"), "Include loop\n").unwrap();
        let error = scan("Include loop\n").unwrap_err();
        assert!(error.contains("include levels"), "{error}");
        std::fs::write(&top, [0xff_u8, 0xfe]).unwrap();
        let error = refuse_ssh_commands(&top, &base, &homes, 0, &mut 0).unwrap_err();
        assert!(error.contains("UTF-8"), "{error}");
        refuse_ssh_commands(&base_dir.join("absent"), &base, &homes, 0, &mut 0).unwrap();
    }

    /// Delta finding 4 (F2): `ssh` expands `~` and a user file's relative
    /// includes with `HOME` when it is set, and opens `~/.ssh/config` from
    /// the account's home. With the two different and `Match exec` only
    /// under `HOME`, the check still finds it.
    #[test]
    fn a_home_that_differs_from_the_account_home_is_read_too() {
        let dir = tempfile::tempdir().unwrap();
        let account = dir.path().join("account");
        let home = dir.path().join("home");
        std::fs::create_dir_all(account.join(".ssh/config.d")).unwrap();
        std::fs::create_dir_all(home.join(".ssh/config.d")).unwrap();
        let marker = dir.path().join("command-ran");
        std::fs::write(account.join(".ssh/config"), "Include config.d/*.conf\n").unwrap();
        std::fs::write(
            home.join(".ssh/config.d/a.conf"),
            format!("Match exec \"touch {}\"\n", marker.display()),
        )
        .unwrap();
        let homes = ssh_homes(account.clone(), Some(home.clone().into())).unwrap();
        assert_eq!(homes, [home.clone(), account.clone()]);
        let error = refuse_ssh_commands(
            &account.join(".ssh/config"),
            &IncludeBase::User,
            &homes,
            0,
            &mut 0,
        )
        .unwrap_err();
        assert!(
            error.contains("a.conf") && error.contains("exec"),
            "{error}"
        );
        assert!(!marker.exists());
        // Only the account's home: the file under HOME is never named.
        refuse_ssh_commands(
            &account.join(".ssh/config"),
            &IncludeBase::User,
            std::slice::from_ref(&account),
            0,
            &mut 0,
        )
        .unwrap();
        assert_eq!(
            ssh_homes(account.clone(), None).unwrap(),
            std::slice::from_ref(&account)
        );
        assert_eq!(
            ssh_homes(account.clone(), Some(account.clone().into())).unwrap(),
            std::slice::from_ref(&account)
        );
        assert!(ssh_homes(account, Some("relative".into())).is_err());
    }

    /// The allow, off by default, refuses a configuration the text check
    /// refuses; on, the same configuration goes to `ssh -G` and passes.
    #[cfg(unix)]
    #[test]
    fn the_accept_setting_passes_a_refused_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::write(&config, "Match host 127.0.0.1\n\tServerAliveInterval 30\n").unwrap();
        let (stand_in, asked) = recording_ssh(dir.path());
        let target = ssh_target("ssh://127.0.0.1/r.git").unwrap();
        let mut view = fixture_view(&stand_in, &config);
        let error = ssh_views_agree(
            &view,
            &target,
            "ssh://127.0.0.1/r.git",
            QueryLimits::guard(),
        )
        .unwrap_err();
        assert!(error.contains("Match block"), "{error}");
        assert!(!asked.exists());
        view.accept = true;
        ssh_views_agree(
            &view,
            &target,
            "ssh://127.0.0.1/r.git",
            QueryLimits::guard(),
        )
        .unwrap();
        assert!(asked.exists());
    }

    /// The allow is read only from the account home's authority file. The
    /// user config a `CODEFLOW_HOME` or `HOME` prefix selects is never read,
    /// and a file that is a link, writable by others or unparsable refuses.
    #[test]
    fn the_allow_is_read_only_from_the_account_authority_file() {
        let account = tempfile::tempdir().unwrap();
        let account = account.path();
        let allow = "[policy_authority]\naccept_ssh_configuration = true\n";
        assert!(!accepts_ssh_configuration(account).unwrap());
        // The old place: the user config under the codeflow home.
        std::fs::create_dir_all(account.join(".codeflow")).unwrap();
        std::fs::write(account.join(".codeflow/config.toml"), allow).unwrap();
        assert!(!accepts_ssh_configuration(account).unwrap());
        let authority = account.join(".codeflow/authority");
        std::fs::create_dir_all(&authority).unwrap();
        let file = authority.join("config.toml");
        std::fs::write(&file, "[policy_authority]\n").unwrap();
        assert!(!accepts_ssh_configuration(account).unwrap());
        std::fs::write(&file, allow).unwrap();
        assert!(accepts_ssh_configuration(account).unwrap());
        std::fs::write(&file, "[policy_authority\n").unwrap();
        let error = accepts_ssh_configuration(account).unwrap_err();
        assert!(error.contains("cannot be parsed"), "{error}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::write(&file, allow).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
            let error = accepts_ssh_configuration(account).unwrap_err();
            assert!(error.contains("written by others"), "{error}");
            std::fs::remove_file(&file).unwrap();
            let elsewhere = account.join("elsewhere.toml");
            std::fs::write(&elsewhere, allow).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
            let error = accepts_ssh_configuration(account).unwrap_err();
            assert!(error.contains("cannot be opened"), "{error}");
            std::fs::remove_file(&file).unwrap();
            std::fs::create_dir(&file).unwrap();
            let error = accepts_ssh_configuration(account).unwrap_err();
            assert!(error.contains("not a regular file"), "{error}");
        }
    }

    /// An allow under a different case of the authority directory never
    /// counts: on a volume that folds case the open reaches it and the
    /// spelling check refuses; elsewhere it is not found and the allow is off.
    #[test]
    fn an_allow_spelled_in_another_case_never_counts() {
        let allow = "[policy_authority]\naccept_ssh_configuration = true\n";
        for spelling in [".codeflow/Authority", ".Codeflow/authority"] {
            let account = tempfile::tempdir().unwrap();
            let dir = account.path().join(spelling);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), allow).unwrap();
            match accepts_ssh_configuration(account.path()) {
                Ok(accepted) => assert!(!accepted, "{spelling}"),
                Err(error) => assert!(error.contains("spelled differently"), "{error}"),
            }
        }
        let account = tempfile::tempdir().unwrap();
        let dir = account.path().join(".codeflow/authority");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Config.toml"), allow).unwrap();
        match accepts_ssh_configuration(account.path()) {
            Ok(accepted) => assert!(!accepted),
            Err(error) => assert!(error.contains("spelled differently"), "{error}"),
        }
    }

    #[test]
    fn system_configuration_names_come_from_the_program() {
        let found = system_config_names(
            b"\x00junk\x00/etc/ssh/ssh_config\x00/etc/ssh/ssh_config\x00relative/ssh_config /opt/x/ssh_config\x01/etc/ssh/sshd_config",
        );
        assert_eq!(
            found,
            [
                std::path::PathBuf::from("/etc/ssh/ssh_config"),
                std::path::PathBuf::from("/opt/x/ssh_config")
            ]
        );
        assert!(system_config_names(b"no paths here").is_empty());
        assert!(wildcard_matches("*.conf", "a.conf"));
        assert!(wildcard_matches("a?c*", "abcde"));
        assert!(!wildcard_matches("*.conf", "a.cfg"));
    }

    /// Delta finding 2: a path-conditional include that sets the SSH client
    /// is refused by class, since the URL comparison would not move.
    #[test]
    fn a_path_conditional_ssh_setting_refuses() {
        let dir = tempfile::tempdir().unwrap();
        for (body, key) in [
            ("[core]\n\tsshCommand = ssh -F /tmp/x\n", "core.sshcommand"),
            ("[ssh]\n\tvariant = plink\n", "ssh.variant"),
        ] {
            std::fs::write(dir.path().join("ssh.config"), body).unwrap();
            let include = [Entry {
                origin: format!("file:{}/.git/config", dir.path().display()),
                key: "includeif.gitdir:/elsewhere/.git.path".into(),
                value: dir.path().join("ssh.config").to_str().unwrap().into(),
                exact: true,
            }];
            let error =
                refuse_path_conditional_remote_settings(&include, dir.path(), QueryLimits::guard())
                    .unwrap_err();
            assert!(error.contains(key), "{error}");
        }
    }
}
