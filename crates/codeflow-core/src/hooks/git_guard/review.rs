//! Review approvals and dismissals from an agent session (TSK-1348 AC-19).
//!
//! Any token with Pull requests write can approve a pull request or dismiss a
//! review on GitHub: the PR App's user token, the agent App's installation
//! token and the developer's own login alike. The guard therefore refuses
//! every approval and dismissal it can parse, whatever token the command
//! carries or the guard can see: `gh pr review --approve`, and a `gh api` or
//! `curl` request that posts an `APPROVE` review or event or puts a review
//! dismissal, REST or GraphQL. It also refuses what it cannot judge: a review
//! or GraphQL request whose body it cannot read, a writing request whose
//! address it cannot settle, and a `curl` command that reads a config file or
//! a variable. Other programs, commands it does not parse, and calls made
//! outside a guarded session are not covered; GitHub still accepts them.

use super::{GitPolicy, Violation};
use serde_json::Value;

/// What a command does to a review, when it does either, or why the guard
/// cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Approve,
    Dismiss,
    /// A review or GraphQL request whose body cannot be read, so it may
    /// approve.
    Unread,
    /// A writing request to an address the guard cannot settle.
    Unsettled,
    /// A `curl` command that reads a config file or a variable.
    Opaque,
}

fn violation(policy: &GitPolicy, act: Act, how: &str) -> Violation {
    let what = match act {
        Act::Approve => "approves a pull request",
        Act::Dismiss => "dismisses a pull request review",
        Act::Unread => {
            "sends a review or GraphQL request whose body the guard cannot read, so it may approve or dismiss"
        }
        Act::Unsettled => {
            "sends a writing request to an address the guard cannot settle (a `..` above the root, a percent escape in the path, or a curl glob), so it may approve or dismiss"
        }
        Act::Opaque => {
            "reads a curl config file or variable the guard does not read (`-K`, `--config`, `--variable`, `--expand-*`), so it may approve or dismiss"
        }
    };
    Violation::new(
        "git.pr_review_approval",
        policy.pr_review_approval,
        format!(
            "{how} {what}; agent sessions never approve or dismiss reviews, whatever token they use (policy key git.pr_review_approval)"
        ),
        crate::remedy::PR_REVIEW_APPROVAL.remedy(),
    )
}

/// `gh pr review` flags that take the next word as their value.
const GH_REVIEW_VALUE_FLAGS: &[&str] = &["--body", "-b", "--body-file", "-F", "--repo", "-R"];

/// `gh pr review <rest>`: `--approve`, `-a`, or `a` inside a cluster of
/// short flags (pflag reads `-ca` as `-c -a`) approves.
pub(super) fn check_gh_pr_review(rest: &[&str], policy: &GitPolicy, out: &mut Vec<Violation>) {
    if !policy.pr_review_approval.is_active() {
        return;
    }
    let mut at = 0;
    while at < rest.len() {
        let word = rest[at];
        if word == "--" {
            break;
        }
        if GH_REVIEW_VALUE_FLAGS.contains(&word) {
            at += 2;
            continue;
        }
        if word == "--approve" || word.starts_with("--approve=") || short_cluster_approves(word) {
            out.push(violation(policy, Act::Approve, "`gh pr review --approve`"));
            return;
        }
        at += 1;
    }
}

/// A short flag cluster such as `-a` or `-ca`; a value flag (`-b`, `-F`,
/// `-R`) ends it, since the rest of the word is its value.
fn short_cluster_approves(word: &str) -> bool {
    let Some(letters) = word.strip_prefix('-') else {
        return false;
    };
    if letters.starts_with('-') {
        return false;
    }
    for letter in letters.chars() {
        match letter {
            'a' => return true,
            'b' | 'F' | 'R' => return false,
            _ => {}
        }
    }
    false
}

/// Which review endpoint a request names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    /// `pulls/{n}/reviews/{id}/dismissals`.
    Dismissal,
    /// `pulls/{n}/reviews` or `pulls/{n}/reviews/{id}/events`.
    Review,
    /// `graphql`.
    GraphQl,
}

/// What an address is, once its path is settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Address {
    Review(Endpoint),
    Other,
    /// The guard cannot settle the path, so it may name a review endpoint.
    Unsettled,
}

/// The path segments of an endpoint or URL, without scheme, host or query,
/// with `.` and `..` resolved as GitHub resolves them. `None` when the path
/// cannot be settled: a `..` above the root, or a percent escape, which may
/// decode to a dot or a separator. With `globs`, as curl reads a URL, any
/// `{`, `}`, `[` or `]` cannot be settled either.
fn settled_segments(endpoint: &str, globs: bool) -> Option<Vec<&str>> {
    if globs && endpoint.contains(['{', '}', '[', ']']) {
        return None;
    }
    let path = endpoint.split(['?', '#']).next().unwrap_or_default();
    let path = path
        .strip_prefix("https://")
        .or_else(|| path.strip_prefix("http://"))
        .map_or(path, |rest| {
            rest.split_once('/').map_or("", |(_, path)| path)
        });
    if path.contains('%') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts)
}

fn address(endpoint: &str, globs: bool) -> Address {
    let Some(parts) = settled_segments(endpoint, globs) else {
        return Address::Unsettled;
    };
    if parts
        .last()
        .is_some_and(|last| last.eq_ignore_ascii_case("graphql"))
    {
        return Address::Review(Endpoint::GraphQl);
    }
    let is = |at: usize, name: &str| {
        parts
            .get(at)
            .is_some_and(|part| part.eq_ignore_ascii_case(name))
    };
    let Some(at) = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("pulls"))
    else {
        return Address::Other;
    };
    if !is(at + 2, "reviews") {
        return Address::Other;
    }
    match parts.get(at + 4) {
        Some(_) if is(at + 4, "dismissals") => Address::Review(Endpoint::Dismissal),
        Some(_) if is(at + 4, "events") => Address::Review(Endpoint::Review),
        None => Address::Review(Endpoint::Review),
        Some(_) => Address::Other,
    }
}

/// One part of a request body: a named field (`gh api -f event=APPROVE`),
/// or a whole body (`--input`, `curl -d`). `None` is text the guard cannot
/// read: standard input, or a file it cannot open.
enum Body {
    Field(String, Option<String>),
    Whole(Option<String>),
}

impl Body {
    fn unread(&self) -> bool {
        matches!(self, Self::Field(_, None) | Self::Whole(None))
    }
}

fn is_approve(text: &str) -> bool {
    text.trim().eq_ignore_ascii_case("APPROVE")
}

/// What a whole REST review body does: its JSON `event`, or, for a body
/// that is not JSON, any `APPROVE` in it.
fn whole_rest_body(text: &str) -> Option<Act> {
    let approves = match serde_json::from_str::<Value>(text) {
        Ok(value) => value
            .get("event")
            .and_then(Value::as_str)
            .is_some_and(is_approve),
        Err(_) => text.to_ascii_uppercase().contains("APPROVE"),
    };
    approves.then_some(Act::Approve)
}

/// Every string in a JSON value, keys included, decoded.
fn json_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| json_strings(item, out)),
        Value::Object(map) => {
            for (key, item) in map {
                out.push(key.clone());
                json_strings(item, out);
            }
        }
        _ => {}
    }
}

/// The text of a GraphQL body part as GitHub reads it: the raw text, and,
/// when it is JSON, every string in it decoded, so a `\u` escape hides
/// nothing.
fn graphql_text(text: &str, out: &mut Vec<String>) {
    out.push(text.to_string());
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        json_strings(&value, out);
    }
}

/// What a request with these body parts does to a review at `kind`.
fn judge(kind: Endpoint, bodies: &[Body]) -> Option<Act> {
    match kind {
        Endpoint::Dismissal => Some(Act::Dismiss),
        Endpoint::Review => {
            for body in bodies {
                let act = match body {
                    Body::Field(key, Some(value)) if key == "event" => {
                        is_approve(value).then_some(Act::Approve)
                    }
                    Body::Field(key, None) if key == "event" => Some(Act::Unread),
                    Body::Field(..) => None,
                    Body::Whole(Some(text)) => whole_rest_body(text),
                    Body::Whole(None) => Some(Act::Unread),
                };
                if act.is_some() {
                    return act;
                }
            }
            None
        }
        // A body the guard cannot read may hold any mutation, so it refuses,
        // a harmless query from standard input included. Otherwise a review
        // mutation with an `APPROVE` event anywhere in the query or its
        // variables approves, and a dismissal mutation dismisses.
        Endpoint::GraphQl => {
            if bodies.iter().any(Body::unread) {
                return Some(Act::Unread);
            }
            let mut texts = Vec::new();
            for body in bodies {
                match body {
                    Body::Field(key, Some(value)) => {
                        texts.push(key.clone());
                        graphql_text(value, &mut texts);
                    }
                    Body::Whole(Some(text)) => graphql_text(text, &mut texts),
                    Body::Field(_, None) | Body::Whole(None) => {}
                }
            }
            let text = texts.join("\n").to_ascii_uppercase();
            if text.contains("DISMISSPULLREQUESTREVIEW") {
                Some(Act::Dismiss)
            } else if text.contains("PULLREQUESTREVIEW") && text.contains("APPROVE") {
                Some(Act::Approve)
            } else {
                None
            }
        }
    }
}

/// What a request does: the first act found at any address it names. An
/// address the guard cannot settle refuses when the request may write.
fn judge_addresses(addresses: &[Address], bodies: &[Body], writes: bool) -> Option<Act> {
    addresses.iter().find_map(|address| match address {
        Address::Review(kind) => judge(*kind, bodies),
        Address::Unsettled => writes.then_some(Act::Unsettled),
        Address::Other => None,
    })
}

/// A method that may change something on the server.
fn writes(method: &str) -> bool {
    !(method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
}

/// The text of a body read from `path`; `None` for standard input or a file
/// that cannot be read.
fn file_text(path: &str) -> Option<String> {
    if path == "-" {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// A `gh api` field `key=value`; with `-F`, a value `@file` is read from the
/// file. `-f` never reads a file, so its `@` is literal text.
fn gh_field(text: &str, typed: bool) -> Body {
    let (key, value) = text.split_once('=').unwrap_or((text, ""));
    let value = match value.strip_prefix('@') {
        Some(path) if typed => file_text(path),
        _ => Some(value.to_string()),
    };
    Body::Field(key.to_string(), value)
}

/// `gh api` flags that take the next word as their value and carry no body.
const GH_API_VALUE_FLAGS: &[&str] = &[
    "-H",
    "--header",
    "--hostname",
    "-q",
    "--jq",
    "-t",
    "--template",
    "--cache",
    "-p",
    "--preview",
];

/// `gh api <rest>`.
pub(super) fn check_gh_api(rest: &[&str], policy: &GitPolicy, out: &mut Vec<Violation>) {
    if !policy.pr_review_approval.is_active() {
        return;
    }
    let mut endpoint = None;
    let mut bodies = Vec::new();
    let mut methods = Vec::new();
    let mut at = 0;
    while at < rest.len() {
        let word = rest[at];
        let next = rest.get(at + 1).copied();
        let inline = |name: &str| {
            word.strip_prefix(name)
                .and_then(|value| value.strip_prefix('='))
        };
        let skip = match word {
            _ if GH_API_VALUE_FLAGS.contains(&word) => 2,
            "-X" | "--method" => {
                methods.extend(next);
                2
            }
            "-f" | "--raw-field" => {
                bodies.push(gh_field(next.unwrap_or_default(), false));
                2
            }
            "-F" | "--field" => {
                bodies.push(gh_field(next.unwrap_or_default(), true));
                2
            }
            "--input" => {
                bodies.push(Body::Whole(next.and_then(file_text)));
                2
            }
            _ => {
                if let Some(value) = inline("--raw-field")
                    .or_else(|| word.strip_prefix("-f").filter(|value| !value.is_empty()))
                {
                    bodies.push(gh_field(value, false));
                } else if let Some(value) = inline("--field")
                    .or_else(|| word.strip_prefix("-F").filter(|value| !value.is_empty()))
                {
                    bodies.push(gh_field(value, true));
                } else if let Some(path) = inline("--input") {
                    bodies.push(Body::Whole(file_text(path)));
                } else if let Some(method) = inline("--method")
                    .or_else(|| word.strip_prefix("-X").filter(|value| !value.is_empty()))
                {
                    methods.push(method);
                } else if !word.starts_with('-') && endpoint.is_none() {
                    endpoint = Some(word);
                }
                1
            }
        };
        at += skip;
    }
    // `gh api` posts once it has a field or an input.
    let writing = !bodies.is_empty() || methods.iter().any(|method| writes(method));
    let addresses: Vec<Address> = endpoint
        .map(|word| address(word, false))
        .into_iter()
        .collect();
    if let Some(act) = judge_addresses(&addresses, &bodies, writing) {
        out.push(violation(policy, act, "this `gh api` request"));
    }
}

/// curl's short options that take a value: the rest of the word, or the
/// next word.
const CURL_SHORT_VALUE: &str = "AbcCdDEeFHKmoPQrtTuUwxXyYz";

/// curl's long options that take the next word as their value and carry no
/// body or URL.
const CURL_LONG_VALUE: &[&str] = &[
    "header",
    "output",
    "user",
    "user-agent",
    "referer",
    "write-out",
    "max-time",
    "connect-timeout",
    "proxy",
    "cookie",
    "cookie-jar",
    "cacert",
    "capath",
    "cert",
    "key",
    "oauth2-bearer",
    "dump-header",
    "range",
    "retry",
    "limit-rate",
    "resolve",
    "connect-to",
    "interface",
    "url-query",
    "trace",
    "trace-ascii",
    "stderr",
    "output-dir",
    "proxy-user",
    "continue-at",
    "time-cond",
];

/// What one curl option carries.
enum CurlOption<'a> {
    Body(&'a str, &'a str),
    Upload(&'a str),
    Url(&'a str),
    Method(&'a str),
    /// A config file or a variable, which the guard does not read.
    Opaque,
    Ignored,
}

/// Whether a long option takes a value, so the next word is not a URL.
fn curl_long_takes_value(name: &str) -> bool {
    CURL_LONG_VALUE.contains(&name)
        || CURL_LONG_BODY.contains(&name)
        || name.starts_with("expand-")
        || matches!(
            name,
            "config" | "variable" | "url" | "request" | "request-target" | "upload-file"
        )
}

/// curl's long options whose value is a request body.
const CURL_LONG_BODY: &[&str] = &[
    "data",
    "data-raw",
    "data-binary",
    "data-ascii",
    "data-urlencode",
    "json",
    "form",
    "form-string",
];

fn curl_long_option<'a>(name: &str, value: &'a str) -> CurlOption<'a> {
    match name {
        _ if CURL_LONG_BODY.contains(&name) => CurlOption::Body(curl_long_flag(name), value),
        "upload-file" => CurlOption::Upload(value),
        "url" | "request-target" => CurlOption::Url(value),
        "request" => CurlOption::Method(value),
        "config" | "variable" => CurlOption::Opaque,
        _ if name.starts_with("expand-") => CurlOption::Opaque,
        _ => CurlOption::Ignored,
    }
}

/// The flag spelling [`curl_body`] reads for a long body option.
fn curl_long_flag(name: &str) -> &'static str {
    match name {
        "data-raw" => "--data-raw",
        "form-string" => "--form-string",
        "data-urlencode" => "--data-urlencode",
        "form" => "-F",
        _ => "-d",
    }
}

fn curl_short_option(letter: char, value: &str) -> CurlOption<'_> {
    match letter {
        'd' => CurlOption::Body("-d", value),
        'F' => CurlOption::Body("-F", value),
        'T' => CurlOption::Upload(value),
        'X' => CurlOption::Method(value),
        'K' => CurlOption::Opaque,
        _ => CurlOption::Ignored,
    }
}

/// A `curl` body value: `@file` is read from the file, except with
/// `--data-raw` and `--form-string`; `name@file` with `--data-urlencode` and
/// `name=@file` or `name=<file` with `-F` read a file too.
fn curl_body(flag: &str, value: &str) -> Body {
    if flag == "--data-raw" || flag == "--form-string" {
        return Body::Whole(Some(value.to_string()));
    }
    let named_file = || match flag {
        "--data-urlencode" => value.split_once('@').map(|(_, path)| path),
        "-F" => value
            .split_once("=@")
            .or_else(|| value.split_once("=<"))
            .map(|(_, path)| path),
        _ => None,
    };
    match value.strip_prefix('@').or_else(named_file) {
        Some(path) => Body::Whole(file_text(path.split(';').next().unwrap_or(path))),
        None => Body::Whole(Some(value.to_string())),
    }
}

/// `curl <args>`: every URL it names, the request target and any URL an
/// upload completes are judged with every body it sends.
pub(super) fn check_curl(args: &[String], policy: &GitPolicy, out: &mut Vec<Violation>) {
    if !policy.pr_review_approval.is_active() {
        return;
    }
    let mut urls: Vec<String> = Vec::new();
    let mut bodies = Vec::new();
    let mut uploads: Vec<String> = Vec::new();
    let mut methods: Vec<String> = Vec::new();
    let mut opaque = false;
    let mut take = |option: CurlOption<'_>| match option {
        CurlOption::Body(flag, value) => bodies.push(curl_body(flag, value)),
        CurlOption::Upload(path) => uploads.push(path.to_string()),
        CurlOption::Url(url) => urls.push(url.to_string()),
        CurlOption::Method(method) => methods.push(method.to_string()),
        CurlOption::Opaque => opaque = true,
        CurlOption::Ignored => {}
    };
    let mut at = 0;
    while at < args.len() {
        let word = args[at].as_str();
        if let Some(long) = word.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            if curl_long_takes_value(name) {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        at += 1;
                        args.get(at).map_or("", String::as_str)
                    }
                };
                take(curl_long_option(name, value));
            }
        } else if let Some(letters) = word.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            // A cluster of short options; the first that takes a value takes
            // the rest of the word, or the next word.
            for (index, letter) in letters.char_indices() {
                if CURL_SHORT_VALUE.contains(letter) {
                    let rest = &letters[index + letter.len_utf8()..];
                    let value = if rest.is_empty() {
                        at += 1;
                        args.get(at).map_or("", String::as_str)
                    } else {
                        rest
                    };
                    take(curl_short_option(letter, value));
                    break;
                }
            }
        } else {
            take(CurlOption::Url(word));
        }
        at += 1;
    }
    if opaque {
        out.push(violation(policy, Act::Opaque, "this `curl` command"));
        return;
    }
    // An upload is a body, and completes a URL that ends in `/` with the
    // file's name.
    for path in &uploads {
        bodies.push(Body::Whole(file_text(path)));
        let name = path.rsplit('/').next().unwrap_or(path);
        let completed: Vec<String> = urls
            .iter()
            .filter(|url| {
                url.split(['?', '#'])
                    .next()
                    .is_some_and(|path| path.ends_with('/'))
            })
            .map(|url| {
                let (path, query) = url.split_at(url.find(['?', '#']).unwrap_or(url.len()));
                format!("{path}{name}{query}")
            })
            .collect();
        urls.extend(completed);
    }
    let writing = !bodies.is_empty() || methods.iter().any(|method| writes(method));
    let addresses: Vec<Address> = urls.iter().map(|url| address(url, true)).collect();
    if let Some(act) = judge_addresses(&addresses, &bodies, writing) {
        out.push(violation(policy, act, "this `curl` request"));
    }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
