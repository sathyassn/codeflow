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
    command
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "--all"]);
    if push {
        command.arg("--push");
    }
    command.args(["--", name]).env("LC_ALL", "C");
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

/// Check that remote `name`, configured as `raw`, is the remote git fetches
/// from and pushes to, and that its transport is on the hermetic allow-list;
/// then `raw` is the identity the hermetic query may ask. Each failure is a
/// refusal: a transport outside the list, an effective URL that cannot be
/// read, and one that differs from `raw`, each named with both identities.
///
/// # Errors
///
/// The reason the identity is not verified, in display form.
pub fn verify(root: &Path, name: &str, raw: &str, limits: QueryLimits) -> Result<(), String> {
    let remedy = format!(
        "the operator inspects git config --show-origin --get-regexp '^(url|include|includeif|remote\\.{name})\\.'"
    );
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
    fn userinfo_is_hidden_in_refusals() {
        let error = require_safe_protocol("ftp://user:FAKE@h/r").unwrap_err();
        assert!(!error.contains("FAKE"), "{error}");
    }
}
