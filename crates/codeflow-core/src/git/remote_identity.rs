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
/// another repository, and inherited configuration is kept on purpose.
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
    let text = |field: &[u8]| match std::str::from_utf8(field) {
        Ok(text) => (text.to_string(), true),
        Err(_) => (String::from_utf8_lossy(field).into_owned(), false),
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
/// setting and its value (the helper or program). Owned by TSK-1184; the
/// pre-push destination check (TSK-327) calls it for `remote.<url>`.
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

/// Refuse when the configuration includes a file on a `gitdir` condition
/// and that file sets a URL rewrite, a remote setting or a further include.
/// Whether git applies such a file depends on the path a command reaches
/// the repository by (a symbolic link, `GIT_DIR` as written), which this
/// read cannot know, so its answer would not bind a later fetch or push.
fn refuse_path_conditional_remote_settings(entries: &[Entry]) -> Result<(), String> {
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
        let file = included_path(entry).ok_or_else(|| {
            format!(
                "a configuration file is included on the condition {shown_condition} from {}, and its path cannot be resolved here, so whether it changes the remote cannot be checked",
                super::display_text(&entry.origin)
            )
        })?;
        match std::fs::symlink_metadata(&file) {
            // Git skips an include that names no file.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "the configuration file {} included on the condition {shown_condition} cannot be inspected: {error}",
                    file.display()
                ))
            }
            Ok(_) => {}
        }
        let mut command = super::command();
        command
            .args(["config", "--null", "--no-includes", "--list", "--file"])
            .arg(&file)
            .env("LC_ALL", "C");
        let listed = super::remote_query::bounded_raw(command, QueryLimits::guard().timeout())
            .and_then(|bytes| parse_listing(&bytes, false))
            .map_err(|why| {
                format!(
                    "the configuration file {} included on the condition {shown_condition} cannot be read ({why})",
                    file.display()
                )
            })?;
        if let Some(found) = listed.iter().find(|found| {
            ["url.", "remote.", "include.", "includeif."]
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
/// the entry. `None` when git's answer cannot be reproduced here.
fn included_path(entry: &Entry) -> Option<std::path::PathBuf> {
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
    let holder = entry.origin.strip_prefix("file:")?;
    Path::new(holder).parent().map(|dir| dir.join(path))
}

/// Check that remote `name`, configured as `raw`, is the remote git fetches
/// from and pushes to, and that its transport is on the hermetic allow-list;
/// then `raw` is the identity the hermetic query may ask. Each failure is a
/// refusal: a transport outside the list or replaced by a remote setting
/// ([`require_native_transport`]), a remote setting that a path-dependent
/// include may apply, an effective URL that cannot be read, and one that
/// differs from `raw`, each named with both identities.
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
    refuse_path_conditional_remote_settings(&entries).map_err(|why| format!("{why}; {remedy}"))?;
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
    require_safe_protocol(raw).map_err(|why| format!("{why}; {remedy}"))
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
        assert!(refuse_path_conditional_remote_settings(&listed).is_ok());
        let include =
            parse_entries(b"file:/r/.git/config\0includeif.gitdir:/x/.git.path\n/w\xff.config\0")
                .unwrap();
        let error = refuse_path_conditional_remote_settings(&include).unwrap_err();
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
        assert_eq!(
            included_path(&entry("file:/home/u/.gitconfig", "work.config")),
            Some(std::path::PathBuf::from("/home/u/work.config"))
        );
        assert_eq!(
            included_path(&entry("file:/home/u/.gitconfig", "/etc/w.config")),
            Some(std::path::PathBuf::from("/etc/w.config"))
        );
        assert_eq!(included_path(&entry("command line:", "rel.config")), None);
        assert_eq!(
            included_path(&entry("file:/h/.gitconfig", "~other/x")),
            None
        );
        assert_eq!(included_path(&entry("file:/h/.gitconfig", "")), None);
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
}
