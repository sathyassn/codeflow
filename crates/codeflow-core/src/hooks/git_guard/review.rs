//! Review approvals and dismissals from an agent session (TSK-1348 AC-19).
//!
//! Any token with Pull requests write can approve a pull request or dismiss a
//! review on GitHub: the PR App's user token, the agent App's installation
//! token and the developer's own login alike. The guard therefore refuses
//! every approval and dismissal it can parse, whatever token the command
//! carries or the guard can see: `gh pr review --approve`, and a `gh api` or
//! `curl` request that posts an `APPROVE` review or event or puts a review
//! dismissal. A REST review request whose body the guard cannot read refuses
//! too. Commands the guard does not parse, and calls made outside a guarded
//! session, are not covered; GitHub still accepts them.

use super::{GitPolicy, Violation};
use serde_json::Value;

/// What a command does to a review, when it does either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Approve,
    Dismiss,
    /// A review request whose body cannot be read, so it may approve.
    Unread,
}

fn violation(policy: &GitPolicy, act: Act, how: &str) -> Violation {
    let what = match act {
        Act::Approve => "approves a pull request",
        Act::Dismiss => "dismisses a pull request review",
        Act::Unread => {
            "sends a pull request review whose body the guard cannot read, so it may approve"
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

/// The path segments of an endpoint or URL, without scheme, host or query.
fn segments(endpoint: &str) -> Vec<&str> {
    let path = endpoint.split(['?', '#']).next().unwrap_or_default();
    let path = path
        .strip_prefix("https://")
        .or_else(|| path.strip_prefix("http://"))
        .map_or(path, |rest| {
            rest.split_once('/').map_or("", |(_, path)| path)
        });
    path.split('/').filter(|part| !part.is_empty()).collect()
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

fn endpoint_kind(endpoint: &str) -> Option<Endpoint> {
    let parts = segments(endpoint);
    if parts
        .last()
        .is_some_and(|last| last.eq_ignore_ascii_case("graphql"))
    {
        return Some(Endpoint::GraphQl);
    }
    let at = parts.iter().position(|part| *part == "pulls")?;
    if parts.get(at + 2) != Some(&"reviews") {
        return None;
    }
    match parts.get(at + 4) {
        Some(&"dismissals") => Some(Endpoint::Dismissal),
        None | Some(&"events") => Some(Endpoint::Review),
        Some(_) => None,
    }
}

/// One part of a request body: a named field (`gh api -f event=APPROVE`),
/// or a whole body (`--input`, `curl -d`). `None` is text the guard cannot
/// read: standard input, or a file it cannot open.
enum Body {
    Field(String, Option<String>),
    Whole(Option<String>),
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
        // A review mutation with an `APPROVE` event anywhere in the query or
        // its variables approves; a dismissal mutation dismisses. A query
        // the guard cannot read is not parsed.
        Endpoint::GraphQl => {
            let text: String = bodies
                .iter()
                .filter_map(|body| match body {
                    Body::Field(_, value) | Body::Whole(value) => value.as_deref(),
                })
                .collect::<Vec<_>>()
                .join("\n")
                .to_ascii_uppercase();
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

/// The text of a body read from `path`; `None` for standard input or a file
/// that cannot be read.
fn file_text(path: &str) -> Option<String> {
    if path == "-" {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// A `gh api` field `key=value`; with `-F`, a value `@file` is read from the
/// file.
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
    "-X",
    "--method",
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
                } else if !word.starts_with('-') && endpoint.is_none() {
                    endpoint = Some(word);
                }
                1
            }
        };
        at += skip;
    }
    if let Some(act) = endpoint
        .and_then(endpoint_kind)
        .and_then(|kind| judge(kind, &bodies))
    {
        out.push(violation(policy, act, "this `gh api` request"));
    }
}

/// `curl` options that take the next word as their value and carry no body
/// or URL.
const CURL_VALUE_FLAGS: &[&str] = &[
    "-X",
    "--request",
    "-H",
    "--header",
    "-o",
    "--output",
    "-u",
    "--user",
    "-A",
    "--user-agent",
    "-e",
    "--referer",
    "-w",
    "--write-out",
    "-m",
    "--max-time",
    "--connect-timeout",
    "-x",
    "--proxy",
    "-b",
    "--cookie",
    "-c",
    "--cookie-jar",
    "-K",
    "--config",
    "--cacert",
    "--cert",
    "--key",
    "--oauth2-bearer",
];

/// `curl` options whose value is a request body.
const CURL_BODY_FLAGS: &[&str] = &[
    "-d",
    "--data",
    "--data-raw",
    "--data-binary",
    "--data-ascii",
    "--data-urlencode",
    "--json",
    "-F",
    "--form",
];

/// A `curl` body value: `@file` is read from the file, except with
/// `--data-raw`; `name@file` with `--data-urlencode` and `name=@file` or
/// `name=<file` with `-F` read a file too.
fn curl_body(flag: &str, value: &str) -> Body {
    if flag == "--data-raw" {
        return Body::Whole(Some(value.to_string()));
    }
    let named_file = || match flag {
        "--data-urlencode" => value.split_once('@').map(|(_, path)| path),
        "-F" | "--form" => value
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

/// `curl <args>`: every URL it names is judged with every body it sends.
pub(super) fn check_curl(args: &[String], policy: &GitPolicy, out: &mut Vec<Violation>) {
    if !policy.pr_review_approval.is_active() {
        return;
    }
    let mut urls = Vec::new();
    let mut bodies = Vec::new();
    let mut at = 0;
    while at < args.len() {
        let word = args[at].as_str();
        let next = args.get(at + 1).map(String::as_str);
        if CURL_VALUE_FLAGS.contains(&word) {
            at += 2;
            continue;
        }
        if CURL_BODY_FLAGS.contains(&word) {
            bodies.push(curl_body(word, next.unwrap_or_default()));
            at += 2;
            continue;
        }
        if word == "--url" {
            urls.extend(next);
            at += 2;
            continue;
        }
        match word.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") && CURL_BODY_FLAGS.contains(&flag) => {
                bodies.push(curl_body(flag, value));
            }
            Some(("--url", value)) => urls.push(value),
            _ => {
                if let Some(value) = word.strip_prefix("-d").filter(|value| !value.is_empty()) {
                    bodies.push(curl_body("-d", value));
                } else if !word.starts_with('-') {
                    urls.push(word);
                }
            }
        }
        at += 1;
    }
    let act = urls
        .into_iter()
        .filter_map(endpoint_kind)
        .find_map(|kind| judge(kind, &bodies));
    if let Some(act) = act {
        out.push(violation(policy, act, "this `curl` request"));
    }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
