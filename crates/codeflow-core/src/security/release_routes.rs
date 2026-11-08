//! Release routes through `gh` and the GitHub API (TSK-1346).
//!
//! The release family in the action table refuses `gh release` and tag
//! pushes. This reader adds the routes that publish without them: a
//! workflow dispatch of a release workflow, a repository dispatch a release
//! workflow listens for, a pending deployment approval, a release or tag
//! creation through the API, a deployment write, the re-enabling of a
//! release workflow, a change to a variable or secret a release workflow
//! reads, and a write to the Actions policies, environments, rulesets or
//! branch protection that hold the remote floors. `gh`, `curl`, `wget`,
//! `HTTPie` and xh are read. Each returns the form it names, and every doubt
//! refuses.
//!
//! A re-run of a release run is not judged here: a run id names no
//! workflow the guard can read without the network. The release workflow
//! refuses its own re-runs instead (`docs/releasing.md`).
//!
//! A release workflow is a workflow file at the policy authority (the
//! remote's default branch, [`crate::hooks::landed_policy`]) that runs on
//! `workflow_dispatch` or `repository_dispatch` and whose run can publish:
//! its token can write contents, packages, deployments, pages, attestations
//! or an OIDC token, or its permissions are left to the repository default,
//! or it deploys to an environment, calls a reusable workflow, or reads a
//! secret other than `GITHUB_TOKEN`. A file the guard cannot read or parse
//! counts as a release workflow.
use std::path::Path;

/// Where a call runs and which repository `gh` defaults to.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    cwd: Option<&'a Path>,
    /// `GH_REPO` as the line or the session sets it.
    gh_repo: Option<&'a str>,
    /// The repository a call acts on when its path names none.
    target: &'a Target,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(cwd: Option<&'a Path>, gh_repo: Option<&'a str>) -> Self {
        Self {
            cwd,
            gh_repo,
            target: &Target::Literal,
        }
    }
}

/// The repository a call acts on when its path names none, or names it
/// with gh's `{owner}` and `{repo}` placeholders.
enum Target {
    /// gh's current repository: no `-R` and no `GH_REPO`.
    Current,
    /// The repository every `-R`, `--repo` and `GH_REPO` names.
    Named(String),
    /// Why the guard cannot tell which repository gh acts on.
    Unresolved(String),
    /// A client that does not expand placeholders.
    Literal,
}

/// The repository a `gh` call acts on: every `-R`, `--repo` and `GH_REPO`
/// must name the same one.
fn gh_target(args: &[String], gh_repo: Option<&str>) -> Target {
    let mut named = Vec::new();
    for (at, word) in args.iter().enumerate() {
        let (name, attached) = split_option(word, &["-R"]);
        if matches!(name.as_str(), "-R" | "--repo") {
            match attached.or_else(|| args.get(at + 1).cloned()) {
                Some(repo) => named.push(repo),
                None => return Target::Unresolved("gives `-R` with no repository".into()),
            }
        }
    }
    named.extend(gh_repo.map(str::to_owned));
    let key = |repo: &str| github_slug(repo).unwrap_or_else(|| repo.to_ascii_lowercase());
    let Some(first) = named.first() else {
        return Target::Current;
    };
    if named.iter().all(|repo| key(repo) == key(first)) {
        Target::Named(first.clone())
    } else {
        let shown: Vec<String> = named.iter().map(|r| crate::git::display_text(r)).collect();
        Target::Unresolved(format!(
            "names repositories {} through `-R` and `GH_REPO`, which disagree",
            shown.join(", ")
        ))
    }
}

/// The refusal for `gh` arguments, or `None` when the call is not a release
/// route. `args` are the words after `gh`; `sub` starts at its subcommand.
pub(crate) fn gh(args: &[String], sub: &[String], ctx: Ctx<'_>) -> Option<String> {
    let target = gh_target(args, ctx.gh_repo);
    let ctx = Ctx {
        target: &target,
        ..ctx
    };
    let action = sub.get(1).map(String::as_str);
    match (sub.first().map(String::as_str), action) {
        (Some("workflow"), Some("run")) => workflow_run(&sub[2..], ctx),
        (Some("workflow"), Some("enable")) => workflow_enable(&sub[2..], ctx),
        (Some(kind @ ("variable" | "secret")), Some(verb @ ("set" | "delete"))) => {
            store_command(kind, verb, &sub[2..], ctx)
        }
        (Some("api"), _) => api(&sub[1..], ctx),
        _ => None,
    }
}

/// `curl` options that take the next word as their value, besides those
/// [`curl`] reads.
const CURL_VALUES: &[&str] = &[
    "-o",
    "--output",
    "-u",
    "--user",
    "-A",
    "--user-agent",
    "-e",
    "--referer",
    "-b",
    "--cookie",
    "-c",
    "--cookie-jar",
    "-w",
    "--write-out",
    "-m",
    "--max-time",
    "--connect-timeout",
    "-x",
    "--proxy",
    "-U",
    "--proxy-user",
    "-E",
    "--cert",
    "--key",
    "--cacert",
    "--capath",
    "-r",
    "--range",
    "-C",
    "--continue-at",
    "-D",
    "--dump-header",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--limit-rate",
    "--max-filesize",
    "--interface",
    "--retry",
    "--retry-delay",
    "--retry-max-time",
    "-z",
    "--time-cond",
    "--oauth2-bearer",
    "--url-query",
    "--variable",
    "-Y",
    "--speed-limit",
    "-y",
    "--speed-time",
];

/// The refusal for a `curl` call that writes to a GitHub API release route.
/// A write whose URL, host or path the guard cannot read is refused.
pub(crate) fn curl(args: &[String], cwd: Option<&Path>) -> Option<String> {
    let ctx = Ctx::new(cwd, None);
    let mut method: Option<String> = None;
    let mut implied: Option<&'static str> = None;
    let mut get = false;
    let mut glob = true;
    let mut redirected: Option<&'static str> = None;
    let mut body = Body::default();
    let mut urls = Vec::new();
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        let (name, attached) = split_option(word, &["-X", "-d", "-F", "-T", "-H", "-K"]);
        if attached.is_none() && CURL_VALUES.contains(&name.as_str()) {
            at += 1;
            continue;
        }
        let mut value = || {
            attached.clone().or_else(|| {
                at += 1;
                args.get(at - 1).cloned()
            })
        };
        match name.as_str() {
            "-K" | "--config" => {
                value();
                redirected = Some("reads its options from a file");
            }
            "--connect-to" | "--resolve" | "--request-target" => {
                value();
                redirected = Some("sends the request to a host or path other than its URL names");
            }
            "-H" | "--header" => {
                let header = value().unwrap_or_default();
                if header.starts_with('@')
                    || header
                        .get(..5)
                        .is_some_and(|h| h.eq_ignore_ascii_case("host:"))
                {
                    redirected = Some("sets the Host header or reads headers from a file");
                }
            }
            "-g" | "--globoff" => glob = false,
            "-X" | "--request" => method = value(),
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii"
            | "--data-urlencode" | "--json" => {
                implied = Some("POST");
                body.text(value());
            }
            "-F" | "--form" | "--form-string" => {
                implied = Some("POST");
                value();
                body.unknown = true;
            }
            "-T" | "--upload-file" => {
                implied = Some("PUT");
                value();
                body.unknown = true;
            }
            "--url" => urls.extend(value()),
            "-G" | "--get" => get = true,
            "-I" | "--head" => method = Some("HEAD".into()),
            _ if word.starts_with('-') => {}
            _ => urls.push(word.clone()),
        }
    }
    let method = match (method, get) {
        (Some(m), _) => m.to_ascii_uppercase(),
        (None, true) => "GET".into(),
        (None, false) => implied.unwrap_or("GET").into(),
    };
    if !writes(&method) {
        return None;
    }
    let form = format!("curl -X {method}");
    if let Some(why) = redirected {
        return Some(format!(
            "{form} {why}, so the guard cannot tell whether it reaches a release route"
        ));
    }
    urls.iter()
        .find_map(|url| route_of(&form, url, glob, &method, &body, ctx))
}

/// The refusal for a write by an HTTP client to `url`.
fn route_of(
    form: &str,
    url: &str,
    glob: bool,
    method: &str,
    body: &Body,
    ctx: Ctx<'_>,
) -> Option<String> {
    match destination(url, glob) {
        Err(why) => Some(format!("{form} {why}")),
        Ok(Dest::Other) => None,
        Ok(Dest::Api(path)) => api_route(form, method, &path, body, ctx),
    }
}

/// `wget` options that take the next word as their value.
const WGET_VALUES: &[&str] = &[
    "-O",
    "-o",
    "-a",
    "-e",
    "-P",
    "-t",
    "-T",
    "-w",
    "-U",
    "-Q",
    "-l",
    "-A",
    "-R",
    "-D",
    "-I",
    "-X",
    "-i",
    "-B",
    "--header",
    "--user",
    "--password",
    "--method",
    "--post-data",
    "--post-file",
    "--body-data",
    "--body-file",
    "--output-document",
    "--output-file",
    "--append-output",
    "--execute",
    "--directory-prefix",
    "--tries",
    "--timeout",
    "--wait",
    "--user-agent",
    "--quota",
    "--level",
    "--accept",
    "--reject",
    "--domains",
    "--include-directories",
    "--exclude-directories",
    "--input-file",
    "--base",
    "--http-user",
    "--http-password",
    "--referer",
    "--load-cookies",
    "--save-cookies",
    "--ca-certificate",
    "--certificate",
    "--private-key",
];

/// `HTTPie` and xh options that take the next word as their value.
const HTTPIE_VALUES: &[&str] = &[
    "-a",
    "--auth",
    "-A",
    "--auth-type",
    "--session",
    "--session-read-only",
    "-o",
    "--output",
    "--cert",
    "--cert-key",
    "--cert-key-pass",
    "--verify",
    "--proxy",
    "--timeout",
    "--pretty",
    "-s",
    "--style",
    "-p",
    "--print",
    "-P",
    "--history-print",
    "--max-redirects",
    "--default-scheme",
    "--format-options",
    "--response-charset",
    "--response-mime",
    "--ssl",
    "--ciphers",
    "--boundary",
    "--raw",
    "--bearer",
    "--resolve",
    "--interface",
    "--http-version",
];

/// A JSON string value without its quotes; other JSON stays as written.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// The refusal for a `wget`, `HTTPie` (`http`, `https`) or xh (`xh`, `xhs`)
/// call that writes to a GitHub API release route.
pub(crate) fn http_client(name: &str, args: &[String], cwd: Option<&Path>) -> Option<String> {
    let ctx = Ctx::new(cwd, None);
    let wget = name == "wget";
    let values = if wget { WGET_VALUES } else { HTTPIE_VALUES };
    let mut method: Option<String> = None;
    let mut implied = false;
    let mut redirected: Option<&'static str> = None;
    let mut body = Body::default();
    let mut positional = Vec::new();
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if !word.starts_with('-') || word == "-" {
            positional.push(word.clone());
            continue;
        }
        let (option, attached) = split_option(word, &[]);
        let value = if attached.is_some() {
            attached
        } else if values.contains(&option.as_str()) {
            at += 1;
            args.get(at - 1).cloned()
        } else {
            None
        };
        match option.as_str() {
            "--method" => method = value,
            "--post-data" | "--body-data" => {
                implied = true;
                body.text(value);
            }
            "--post-file" | "--body-file" | "--raw" => {
                implied = true;
                if option == "--raw" {
                    body.text(value);
                } else {
                    body.unknown = true;
                }
            }
            "-f" | "--form" | "--multipart" if !wget => implied = true,
            "-i" | "--input-file" | "-e" | "--execute" | "--config" if wget => {
                redirected = Some("reads its URLs or settings from a file or command");
            }
            "--header"
                if value
                    .as_deref()
                    .and_then(|h| h.get(..5))
                    .is_some_and(|h| h.eq_ignore_ascii_case("host:")) =>
            {
                redirected = Some("sets the Host header");
            }
            _ => {}
        }
    }
    let mut positional = positional.into_iter();
    let url = if wget {
        positional.next()
    } else {
        let first = positional.next();
        // HTTPie and xh read a first word of letters as the method when a
        // URL follows it.
        match first {
            Some(word) if word.bytes().all(|b| b.is_ascii_alphabetic()) => {
                match positional.next() {
                    Some(url) => {
                        method = method.or(Some(word));
                        Some(url)
                    }
                    None => Some(word),
                }
            }
            other => other,
        }
    };
    if !wget {
        implied |= httpie_items(positional, &mut body, &mut redirected);
    }
    let method = method.map_or_else(
        || if implied { "POST".into() } else { "GET".into() },
        |m| m.to_ascii_uppercase(),
    );
    if !writes(&method) {
        return None;
    }
    let form = if wget {
        format!("wget --method {method}")
    } else {
        format!("{name} {method}")
    };
    if let Some(why) = redirected {
        return Some(format!(
            "{form} {why}, so the guard cannot tell whether it reaches a release route"
        ));
    }
    route_of(&form, &url?, false, &method, &body, ctx)
}

/// Read `HTTPie` and xh request items into `body`; whether any sends a body.
/// A `Host` header item sets `redirected`.
fn httpie_items(
    items: impl Iterator<Item = String>,
    body: &mut Body,
    redirected: &mut Option<&'static str>,
) -> bool {
    let mut implied = false;
    for item in items {
        if item
            .split_once(':')
            .is_some_and(|(key, rest)| key.eq_ignore_ascii_case("host") && !rest.starts_with('='))
        {
            *redirected = Some("sets the Host header");
        } else if item.contains(":=@")
            || item.contains("=@")
            || item.contains('@') && !item.contains('=')
        {
            implied = true;
            body.unknown = true;
        } else if let Some((key, value)) = item.split_once(":=") {
            implied = true;
            body.fields
                .push((key.to_owned(), unquote(value).to_owned()));
        } else if item.contains("==") {
            // A query parameter, not a body field.
        } else if let Some((key, value)) = item.split_once('=') {
            implied = true;
            body.fields.push((key.to_owned(), value.to_owned()));
        }
    }
    implied
}

fn writes(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD")
}

/// The option name and an attached value: `-XPOST`, `-X=POST` and
/// `--method=POST` give `("-X", Some("POST"))` and the like.
fn split_option(word: &str, shorts: &[&str]) -> (String, Option<String>) {
    if let Some((name, value)) = word.split_once('=').filter(|_| word.starts_with("--")) {
        return (name.to_owned(), Some(value.to_owned()));
    }
    for short in shorts {
        if let Some(rest) = word.strip_prefix(short).filter(|r| !r.is_empty()) {
            let rest = rest.strip_prefix('=').unwrap_or(rest);
            return ((*short).to_owned(), Some(rest.to_owned()));
        }
    }
    (word.to_owned(), None)
}

/// A request body as far as the guard can read it.
#[derive(Default)]
struct Body {
    fields: Vec<(String, String)>,
    /// Some part comes from a file, stdin or text the guard cannot parse.
    unknown: bool,
}

enum Field {
    Known(String),
    Unknown,
    Absent,
}

impl Body {
    fn text(&mut self, text: Option<String>) {
        let Some(text) = text else { return };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) if !text.starts_with('@') => {
                for (key, value) in map {
                    match value {
                        serde_json::Value::String(s) => self.fields.push((key, s)),
                        other => self.fields.push((key, other.to_string())),
                    }
                }
            }
            _ => self.unknown = true,
        }
    }

    fn field(&self, key: &str) -> Field {
        match self.fields.iter().rev().find(|(k, _)| k == key) {
            Some((_, value)) if !opaque(value) => Field::Known(value.clone()),
            Some(_) => Field::Unknown,
            None if self.unknown => Field::Unknown,
            None => Field::Absent,
        }
    }
}

/// Text holding a shell expansion the guard did not resolve.
fn opaque(text: &str) -> bool {
    text.contains('$') || text.contains('`')
}

fn workflow_run(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let cwd = ctx.cwd;
    let mut reference = None;
    let mut selector = None;
    let mut at = 0;
    let mut options = true;
    while let Some(word) = rest.get(at) {
        at += 1;
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && word.starts_with('-') {
            let (name, attached) = split_option(word, &["-R", "-r", "-f", "-F"]);
            let mut value = || {
                attached.clone().or_else(|| {
                    at += 1;
                    rest.get(at - 1).cloned()
                })
            };
            match name.as_str() {
                "-r" | "--ref" => reference = value(),
                "-R" | "--repo" | "-f" | "-F" | "--field" | "--raw-field" => {
                    value();
                }
                _ => {}
            }
            continue;
        }
        if selector.is_none() {
            selector = Some(word.clone());
        }
    }
    let form = "gh workflow run";
    let Some(selector) = selector else {
        return Some(format!(
            "{form} names no workflow, so gh would pick one interactively; name a workflow that is not a release workflow"
        ));
    };
    let authority = match Authority::read(cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some(why) = authority.target(ctx.target) {
        return Some(format!("{form} {why}"));
    }
    dispatch(form, &authority, &selector, reference.as_deref(), false)
}

/// A `gh` subcommand's operands and its options.
type Parts = (Vec<String>, Vec<(String, Option<String>)>);

/// The operands and options of a `gh` subcommand whose value options are
/// `values`.
fn operands(rest: &[String], values: &[&str]) -> Parts {
    let mut positional = Vec::new();
    let mut options = Vec::new();
    let mut at = 0;
    let mut open = true;
    while let Some(word) = rest.get(at) {
        at += 1;
        if open && word == "--" {
            open = false;
            continue;
        }
        if open && word.starts_with('-') && word != "-" {
            let shorts: Vec<&str> = values.iter().copied().filter(|v| v.len() == 2).collect();
            let (name, attached) = split_option(word, &shorts);
            let value = if attached.is_some() {
                attached
            } else if values.contains(&name.as_str()) {
                at += 1;
                rest.get(at - 1).cloned()
            } else {
                None
            };
            options.push((name, value));
            continue;
        }
        positional.push(word.clone());
    }
    (positional, options)
}

/// `gh workflow enable`: re-enabling a release workflow lets its triggers
/// start release runs again.
fn workflow_enable(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let form = "gh workflow enable";
    let (positional, _) = operands(rest, &["-R", "--repo"]);
    let Some(selector) = positional.first() else {
        return Some(format!(
            "{form} names no workflow, so gh would pick one interactively; name a workflow that is not a release workflow"
        ));
    };
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some(why) = authority.target(ctx.target) {
        return Some(format!("{form} {why}"));
    }
    enable(form, &authority, selector, false)
}

fn enable(form: &str, authority: &Authority, selector: &str, api: bool) -> Option<String> {
    match matching(form, authority, selector, api) {
        Err(why) => Some(why),
        Ok(flows) => flows.iter().find(|flow| flow.release.is_some()).map(|flow| {
            format!(
                "{form} enables release workflow {} ({}), whose triggers would start release runs again",
                crate::git::display_text(&flow.path),
                flow.release.as_deref().unwrap_or_default()
            )
        }),
    }
}

/// `gh variable set|delete` and `gh secret set|delete`.
fn store_command(kind: &str, verb: &str, rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let form = format!("gh {kind} {verb}");
    let values = [
        "-R",
        "--repo",
        "-e",
        "--env",
        "-o",
        "--org",
        "-v",
        "--visibility",
        "-r",
        "--repos",
        "-b",
        "--body",
        "-f",
        "--env-file",
        "-a",
        "--app",
    ];
    let (positional, options) = operands(rest, &values);
    let option = |names: &[&str]| {
        options
            .iter()
            .find(|(name, _)| names.contains(&name.as_str()))
            .map(|(_, value)| value.clone().unwrap_or_default())
    };
    // Dependabot and Codespaces secrets and a user's Codespaces secrets
    // never reach an Actions run.
    if option(&["-a", "--app"]).is_some_and(|app| !app.eq_ignore_ascii_case("actions"))
        || options
            .iter()
            .any(|(name, _)| name == "-u" || name == "--user")
    {
        return None;
    }
    let org = option(&["-o", "--org"]).is_some();
    let mut names: Vec<Field> = positional
        .iter()
        .take(1)
        .map(|name| {
            if opaque(name) {
                Field::Unknown
            } else {
                Field::Known(name.clone())
            }
        })
        .collect();
    if option(&["-f", "--env-file"]).is_some() || names.is_empty() {
        names.push(Field::Unknown);
    }
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(&form, &why)),
    };
    if !org {
        if let Some(why) = authority.target(ctx.target) {
            return Some(format!("{form} {why}"));
        }
    }
    let store = if kind == "secret" {
        Store::Secrets
    } else {
        Store::Vars
    };
    store_write(store, &names, &authority).map(|why| format!("{form} {why}"))
}

fn unreadable(form: &str, why: &str) -> String {
    format!(
        "{form}: the workflows at the policy authority cannot be read ({why}), so the guard cannot tell a release workflow from another"
    )
}

/// Judge a dispatch of the workflow `selector` at `reference`; `api` is
/// whether the selector came from an API path, which takes only a file name
/// or an id.
fn dispatch(
    form: &str,
    authority: &Authority,
    selector: &str,
    reference: Option<&str>,
    api: bool,
) -> Option<String> {
    let matches = match matching(form, authority, selector, api) {
        Ok(matches) => matches,
        Err(why) => return Some(why),
    };
    if let Some(flow) = matches.iter().find(|flow| flow.release.is_some()) {
        return Some(format!(
            "{form} dispatches release workflow {} ({}); a dry run is refused too, because the guard does not evaluate the workflow's expressions",
            crate::git::display_text(&flow.path),
            flow.release.as_deref().unwrap_or_default()
        ));
    }
    let reference = reference?;
    if authority.is_default(reference) {
        return None;
    }
    let shown_ref = crate::git::display_text(reference);
    for flow in matches {
        match authority.flow_at(reference, &flow.path) {
            Ok(Some(at_ref)) if at_ref.release.is_none() => {}
            Ok(Some(at_ref)) => {
                return Some(format!(
                    "{form} runs {} as ref {shown_ref} holds it, where it is a release workflow ({})",
                    crate::git::display_text(&flow.path),
                    at_ref.release.unwrap_or_default()
                ))
            }
            Ok(None) | Err(_) => {
                return Some(format!(
                    "{form} runs {} as ref {shown_ref} holds it, which the guard cannot read here; fetch that branch so its tracking ref holds the file",
                    crate::git::display_text(&flow.path)
                ))
            }
        }
    }
    None
}

/// The workflows at the authority that `selector` names, or the refusal
/// when it names none the guard can map.
fn matching<'a>(
    form: &str,
    authority: &'a Authority,
    selector: &str,
    api: bool,
) -> Result<Vec<&'a Flow>, String> {
    let shown = crate::git::display_text(selector);
    if opaque(selector) {
        return Err(format!(
            "{form} names workflow {shown}, which the guard cannot resolve"
        ));
    }
    if selector.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "{form} names workflow id {shown}, which the guard cannot map to a workflow file"
        ));
    }
    let wanted = selector.trim_start_matches("./");
    let matches: Vec<&Flow> = authority
        .flows
        .iter()
        .filter(|flow| {
            flow.path == wanted
                || flow.path.rsplit('/').next() == Some(wanted)
                || (!api
                    && flow
                        .name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(wanted)))
        })
        .collect();
    if matches.is_empty() {
        return Err(format!(
            "{form} names workflow {shown}, which matches no workflow file at the policy authority {}",
            authority.shown
        ));
    }
    Ok(matches)
}

fn api(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let mut method = None;
    let mut body = Body::default();
    let mut implied = false;
    let mut endpoint = None;
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at += 1;
        if !word.starts_with('-') || word == "-" {
            if endpoint.is_none() {
                endpoint = Some(word.clone());
            }
            continue;
        }
        let (name, attached) =
            split_option(word, &["-X", "-f", "-F", "-H", "-q", "-t", "-p", "-R"]);
        let mut value = || {
            attached.clone().or_else(|| {
                at += 1;
                rest.get(at - 1).cloned()
            })
        };
        match name.as_str() {
            "-X" | "--method" => method = value(),
            "-f" | "--raw-field" | "-F" | "--field" => {
                implied = true;
                let typed = matches!(name.as_str(), "-F" | "--field");
                match value().as_deref().and_then(|v| v.split_once('=')) {
                    Some((_, v)) if typed && v.starts_with('@') => body.unknown = true,
                    Some((k, v)) => body.fields.push((k.to_owned(), v.to_owned())),
                    None => body.unknown = true,
                }
            }
            "--input" => {
                implied = true;
                value();
                body.unknown = true;
            }
            "-H" | "--header" | "-q" | "--jq" | "-t" | "--template" | "--hostname" | "-p"
            | "--preview" | "--cache" | "-R" | "--repo" => {
                value();
            }
            _ => {}
        }
    }
    let method = method.map_or_else(
        || if implied { "POST".into() } else { "GET".into() },
        |m| m.to_ascii_uppercase(),
    );
    if !writes(&method) {
        return None;
    }
    let endpoint = endpoint?;
    let form = format!("gh api -X {method}");
    // gh sends a URL as written and joins any other endpoint to its host.
    let path = if endpoint.contains("://") {
        match destination(&endpoint, false) {
            Err(why) => return Some(format!("{form} {why}")),
            Ok(Dest::Other) => return None,
            Ok(Dest::Api(path)) => path,
        }
    } else {
        let path = endpoint.split(['?', '#']).next().unwrap_or_default();
        if let Some(why) = dot_segment(path) {
            let shown = crate::git::display_text(&endpoint);
            return Some(format!("{form} {shown}: {why}"));
        }
        path.trim_start_matches('/').to_owned()
    };
    api_route(&form, &method, &path, &body, ctx)
}

/// GraphQL mutations that create or move a tag, approve or create a
/// deployment, or change an environment, ruleset or branch protection rule.
const MUTATIONS: &[&str] = &[
    "createRef",
    "updateRef",
    "updateRefs",
    "approveDeployments",
    "createEnvironment",
    "updateEnvironment",
    "deleteEnvironment",
    "createRepositoryRuleset",
    "updateRepositoryRuleset",
    "deleteRepositoryRuleset",
    "createDeployment",
    "createBranchProtectionRule",
    "updateBranchProtectionRule",
    "deleteBranchProtectionRule",
];

/// A GraphQL write: refused when its query runs a mutation in
/// [`MUTATIONS`] or the guard cannot read it.
fn graphql(form: &str, body: &Body) -> Option<String> {
    match body.field("query") {
        Field::Known(query) => MUTATIONS
            .iter()
            .find(|name| query.contains(**name))
            .map(|name| format!("{form} graphql runs the {name} mutation")),
        Field::Unknown => Some(format!(
            "{form} graphql sends a query the guard cannot read, which may publish or approve a release"
        )),
        Field::Absent => None,
    }
}

/// Where a request goes, as the guard reads its URL.
enum Dest {
    /// A GitHub API host, with the API path: no host, leading slash or
    /// query.
    Api(String),
    /// Another host.
    Other,
}

/// Read `url` as an HTTP client does: a scheme (`http` when none is
/// given), userinfo, host, port and path. The host is compared lower case
/// and without a trailing dot, whatever userinfo and port it carries. A URL
/// that clients could read differently is an error, which a write refuses:
/// a shell expansion or a byte outside printable ASCII in the scheme or
/// host, more than one `@`, a port that is not a number, a dot segment in
/// the path, or, when `glob` (curl without `--globoff`), braces or
/// brackets, which curl expands.
fn destination(url: &str, glob: bool) -> Result<Dest, String> {
    let shown = crate::git::display_text(url);
    let bad = |why: &str| {
        Err(format!(
            "sends to {shown}, which the guard cannot read: {why}"
        ))
    };
    if url.bytes().any(|b| !b.is_ascii_graphic()) || url.contains('\\') {
        return bad("it holds a space, a backslash or a byte outside printable ASCII");
    }
    if glob && url.contains(['{', '}', '[', ']']) {
        return bad("curl expands the braces or brackets in it");
    }
    let (scheme, rest) = if let Some(at) = url.find("://") {
        let scheme = &url[..at];
        if !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        {
            return bad("its scheme is not one the guard reads");
        }
        (scheme.to_ascii_lowercase(), &url[at + 3..])
    } else if url.starts_with(':') {
        // HTTPie and xh read `:port/path` as localhost.
        return Ok(Dest::Other);
    } else {
        let head = &url[..url.find(['/', '?', '#']).unwrap_or(url.len())];
        if head
            .split_once(':')
            .is_some_and(|(_, port)| !port.bytes().all(|b| b.is_ascii_digit()))
        {
            return bad("it names a scheme without `//`, which clients read differently");
        }
        ("http".to_owned(), url)
    };
    if !matches!(scheme.as_str(), "http" | "https") {
        return Ok(Dest::Other);
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.matches('@').count() > 1 {
        return bad("its userinfo holds an `@`, which clients split differently");
    }
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host_port.starts_with('[') {
        // An IPv6 address names no GitHub host.
        return Ok(Dest::Other);
    }
    let Some(host) = host_name(host_port) else {
        return bad("its host or port is not one the guard reads");
    };
    let path = tail.split(['?', '#']).next().unwrap_or_default();
    if let Some(why) = dot_segment(path) {
        return bad(why);
    }
    let path = path.trim_start_matches('/');
    let api = if matches!(host.as_str(), "api.github.com" | "uploads.github.com") {
        Some(path)
    } else if path == "api/graphql" {
        Some("graphql")
    } else {
        path.strip_prefix("api/v3/")
            .or_else(|| path.strip_prefix("api/uploads/"))
    };
    Ok(api.map_or(Dest::Other, |path| Dest::Api(path.to_owned())))
}

/// A host as clients compare it: without its port, lower case and without a
/// trailing dot. `None` for an empty host, a port that is not a number, or
/// a character a host name does not hold, a `%` or shell expansion
/// included.
fn host_name(host_port: &str) -> Option<String> {
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, ""));
    let host = host.strip_suffix('.').unwrap_or(host);
    (!host.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)))
    .then(|| host.to_ascii_lowercase())
}

/// Why `path` cannot be read: a `.` or `..` segment, raw or percent-encoded,
/// which one client resolves and another sends as written.
fn dot_segment(path: &str) -> Option<&'static str> {
    path.split('/')
        .any(|seg| {
            matches!(
                seg.to_ascii_lowercase().replace("%2e", ".").as_str(),
                "." | ".."
            )
        })
        .then_some("its path has a `.` or `..` segment, which clients resolve differently")
}

/// The refusal for a write to the API `path`: a GraphQL path is judged by
/// its query, and every path by its route.
fn api_route(form: &str, method: &str, path: &str, body: &Body, ctx: Ctx<'_>) -> Option<String> {
    let last = path.split(['?', '#']).next().unwrap_or_default();
    let last = last
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if last.eq_ignore_ascii_case("graphql") {
        if let Some(why) = graphql(form, body) {
            return Some(why);
        }
    }
    judge(form, method, path, body, ctx)
}

/// One path segment; `wild` when it holds a shell expansion the guard did
/// not resolve, which may stand for any segment or, last, for several.
struct Seg<'a> {
    text: &'a str,
    wild: bool,
}

/// Whether `tail` can name `pattern` (`*` is any one segment); `open` lets
/// the tail run longer. Returns `Some(true)` for a certain match,
/// `Some(false)` for one only an unresolved segment allows.
fn fits(tail: &[Seg<'_>], pattern: &[&str], open: bool) -> Option<bool> {
    let mut certain = true;
    for (at, token) in pattern.iter().enumerate() {
        let seg = tail.get(at)?;
        if seg.wild {
            if *token != "*" {
                certain = false;
            }
            if at + 1 == tail.len() {
                return Some(certain && at + 1 == pattern.len());
            }
            continue;
        }
        if *token != "*" && seg.text != *token {
            return None;
        }
    }
    (open || tail.len() == pattern.len()).then_some(certain)
}

fn judge(form: &str, method: &str, path: &str, body: &Body, ctx: Ctx<'_>) -> Option<String> {
    // An empty inner segment is a variable the line's environment left
    // empty; the command's own shell may not, so it stands for any segment.
    let segs: Vec<Seg<'_>> = path
        .trim_matches('/')
        .split('/')
        .map(|text| Seg {
            text,
            wild: text.is_empty() || opaque(text),
        })
        .collect();
    let shown = crate::git::display_text(path);
    if segs.first().is_some_and(|s| s.wild) {
        return Some(format!(
            "{form} {shown}: the guard cannot read the endpoint, which may publish or approve a release"
        ));
    }
    match segs.first().map(|s| s.text) {
        Some("repos") if segs.len() >= 3 => {
            let tail = judge_tail(
                form,
                method,
                &shown,
                Some((&segs[1], &segs[2])),
                &segs[3..],
                body,
                ctx,
            );
            // An unresolved owner may hold `owner/name` in one word, so the
            // path after it is read from one segment earlier too.
            if tail.is_none() && (segs[1].wild || segs[2].wild) {
                return judge_tail(
                    form,
                    method,
                    &shown,
                    Some((&segs[1], &segs[1])),
                    &segs[2..],
                    body,
                    ctx,
                );
            }
            tail
        }
        Some("orgs") if segs.len() >= 2 => {
            judge_tail(form, method, &shown, None, &segs[2..], body, ctx)
        }
        _ => None,
    }
}

fn judge_tail(
    form: &str,
    method: &str,
    shown: &str,
    repo: Option<(&Seg<'_>, &Seg<'_>)>,
    tail: &[Seg<'_>],
    body: &Body,
    ctx: Ctx<'_>,
) -> Option<String> {
    let org = repo.is_none();
    let unsure = |what: &str| {
        Some(format!(
            "{form} {shown}: an unresolved part of the path may make it {what}"
        ))
    };
    let refuse = |what: &str| Some(format!("{form} {shown} {what}"));
    for (pattern, open, what, orgs_too) in FLOORS {
        if org && !orgs_too {
            continue;
        }
        match fits(tail, pattern, *open) {
            Some(true) => return refuse(what),
            Some(false) => return unsure(what),
            None => {}
        }
    }
    match fits(tail, &["git", "refs"], false) {
        Some(true) if method == "POST" => {
            return match body.field("ref") {
                Field::Known(r) if r.starts_with("refs/heads/") => None,
                Field::Known(r) => refuse(&format!(
                    "creates ref {}, which is not a branch",
                    crate::git::display_text(&r)
                )),
                _ => refuse("creates a ref the guard cannot read, which may be a release tag"),
            }
        }
        Some(false) => return unsure("create a release tag"),
        _ => {}
    }
    if let Some(certain) = fits(tail, &["git", "refs", "*"], true) {
        if !certain || tail.get(2).is_some_and(|s| s.text != "heads") {
            return unsure("move a release tag");
        }
    }
    let workflow = fits(tail, &["actions", "workflows", "*", "dispatches"], false);
    let enabling = fits(tail, &["actions", "workflows", "*", "enable"], false);
    let repository = fits(tail, &["dispatches"], false);
    let store = store_route(tail, body);
    if workflow.is_none() && enabling.is_none() && repository.is_none() && store.is_none() {
        return None;
    }
    if [workflow, enabling, repository].contains(&Some(false))
        || matches!(store, Some((_, _, false)))
    {
        return unsure("dispatch, enable or configure a release workflow");
    }
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some((owner, name)) = repo {
        if owner.wild || name.wild {
            return refuse(
                "names a repository the guard cannot resolve, whose workflows it cannot read",
            );
        }
        if let Some(why) = authority.path_repo(owner.text, name.text, ctx.target) {
            return Some(format!("{form} {shown} {why}"));
        }
    }
    if let Some((store, names, _)) = store {
        return store_write(store, &names, &authority).map(|why| format!("{form} {shown} {why}"));
    }
    // Workflow and repository dispatches name one repository.
    repo?;
    if enabling.is_some() {
        return enable(form, &authority, tail[2].text, true);
    }
    if workflow.is_some() {
        // The workflow is judged first, whatever the body holds.
        let selector = tail[2].text;
        if let Some(why) = dispatch(form, &authority, selector, None, true) {
            return Some(why);
        }
        return match body.field("ref") {
            Field::Known(r) => dispatch(form, &authority, selector, Some(&r), true),
            Field::Unknown => refuse("dispatches at a ref the guard cannot read"),
            Field::Absent => None,
        };
    }
    repository_dispatch(&authority, body).map(|what| format!("{form} {shown} {what}"))
}

/// An Actions variable or secret write: what it writes, the names it
/// changes, and whether the path is certain.
fn store_route(tail: &[Seg<'_>], body: &Body) -> Option<(Store, Vec<Field>, bool)> {
    let segment_name = |seg: &Seg<'_>| {
        if seg.wild {
            Field::Unknown
        } else {
            Field::Known(seg.text.to_owned())
        }
    };
    if let Some(certain) = fits(tail, &["actions", "variables"], false) {
        return Some((Store::Vars, vec![body.field("name")], certain));
    }
    if let Some(certain) = fits(tail, &["actions", "variables", "*"], false) {
        // A PATCH can rename the variable, so the body's name counts too.
        let mut names = vec![segment_name(&tail[2])];
        if !matches!(body.field("name"), Field::Absent) {
            names.push(body.field("name"));
        }
        return Some((Store::Vars, names, certain));
    }
    if let Some(certain) = fits(tail, &["actions", "secrets", "*"], false) {
        return Some((Store::Secrets, vec![segment_name(&tail[2])], certain));
    }
    None
}

/// Whether a write to `names` changes what a release workflow reads.
fn store_write(store: Store, names: &[Field], authority: &Authority) -> Option<String> {
    let what = match store {
        Store::Vars => "variable",
        Store::Secrets => "secret",
    };
    for flow in authority.flows.iter().filter(|flow| flow.release.is_some()) {
        let reads = match store {
            Store::Vars => &flow.reads.vars,
            Store::Secrets => &flow.reads.secrets,
        };
        let path = crate::git::display_text(&flow.path);
        for name in names {
            match name {
                Field::Known(name) if reads.covers(name) => {
                    return Some(format!(
                        "changes {what} {}, which release workflow {path} reads",
                        crate::git::display_text(name)
                    ))
                }
                Field::Unknown if reads.any() => {
                    return Some(format!(
                        "changes a {what} the guard cannot name, and release workflow {path} reads {what}s"
                    ))
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Store {
    Vars,
    Secrets,
}

/// Remote floors and deployment approval, refused whatever the repository:
/// the path pattern, whether the path may run longer, what it does, and
/// whether the organization path is refused too.
const FLOORS: &[(&[&str], bool, &str, bool)] = &[
    (
        &["actions", "policies"],
        true,
        "writes an Actions policy, which holds the actor and event floors",
        true,
    ),
    (
        &["rulesets"],
        true,
        "writes a ruleset, which holds the release tag floor",
        true,
    ),
    (
        &["environments"],
        true,
        "writes an environment, which holds the required-reviewer floor",
        false,
    ),
    (
        &["actions", "runs", "*", "pending_deployments"],
        false,
        "approves or rejects a pending deployment",
        false,
    ),
    (
        &["actions", "runs", "*", "deployment_protection_rule"],
        false,
        "answers a deployment protection rule",
        false,
    ),
    (
        &["releases"],
        true,
        "creates, publishes or changes a release",
        false,
    ),
    (&["git", "tags"], false, "creates a tag object", false),
    (
        &["deployments"],
        true,
        "creates or changes a deployment, which starts the runs that listen for deployment events",
        false,
    ),
    (
        &["branches", "*", "protection"],
        true,
        "writes branch protection, which holds the push floor",
        false,
    ),
    (
        &["git", "refs", "tags"],
        true,
        "creates, moves or deletes a release tag",
        false,
    ),
];

/// What a repository dispatch with `body` would start, when a release
/// workflow at the authority listens for it.
fn repository_dispatch(authority: &Authority, body: &Body) -> Option<String> {
    let listeners: Vec<(&Flow, &Listen)> = authority
        .flows
        .iter()
        .filter(|flow| flow.release.is_some())
        .filter_map(|flow| flow.repository_dispatch.as_ref().map(|l| (flow, l)))
        .collect();
    let (first, _) = listeners.first()?;
    let Field::Known(event) = body.field("event_type") else {
        return Some(format!(
            "sends a repository dispatch whose event type the guard cannot read, and release workflow {} listens for repository dispatches",
            crate::git::display_text(&first.path)
        ));
    };
    listeners
        .iter()
        .find(|(_, listen)| match listen {
            Listen::Any => true,
            Listen::Types(types) => types.contains(&event),
        })
        .map(|(flow, _)| {
            format!(
                "sends repository dispatch {}, which release workflow {} listens for",
                crate::git::display_text(&event),
                crate::git::display_text(&flow.path)
            )
        })
}

/// The repository dispatch types a workflow listens for.
#[derive(Debug)]
enum Listen {
    Any,
    Types(Vec<String>),
}

/// One workflow file at the authority.
#[derive(Debug)]
struct Flow {
    path: String,
    name: Option<String>,
    /// Why it is a release workflow, or `None`.
    release: Option<String>,
    /// The repository dispatch types it listens for, if it listens.
    repository_dispatch: Option<Listen>,
    /// The variables and secrets it reads.
    reads: Reads,
}

impl Flow {
    /// A file the guard cannot read counts as a release workflow that
    /// listens to every repository dispatch and reads every stored value.
    fn unreadable(path: String, why: &str) -> Self {
        Self {
            path,
            name: None,
            release: Some(why.to_owned()),
            repository_dispatch: Some(Listen::Any),
            reads: Reads::ALL,
        }
    }
}

/// The variables and secrets a workflow reads.
#[derive(Debug)]
struct Reads {
    vars: Names,
    secrets: Names,
}

impl Reads {
    const ALL: Self = Self {
        vars: Names::All,
        secrets: Names::All,
    };

    /// What `text` reads: `vars.NAME` and `secrets.NAME`, or every name
    /// when it indexes or serializes the context or passes secrets on with
    /// `secrets: inherit`.
    fn of(text: &str) -> Self {
        // YAML and expression blanks: space, tab and line ends.
        let squeezed: String = text
            .chars()
            .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
            .collect();
        let whole = |context: &str| {
            squeezed.contains(&format!("{context}["))
                || squeezed.contains(&format!("toJSON({context})"))
                || squeezed.contains(&format!("tojson({context})"))
        };
        let names = |context: &str, all: bool| {
            if all {
                return Names::All;
            }
            let prefix = format!("{context}.");
            let mut found = Vec::new();
            let mut at = 0;
            while let Some(offset) = text[at..].find(&prefix) {
                let start = at + offset;
                at = start + prefix.len();
                let before = text[..start].chars().next_back();
                if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let name: String = text[at..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() && !name.eq_ignore_ascii_case("GITHUB_TOKEN") {
                    found.push(name.to_ascii_uppercase());
                }
            }
            found.sort();
            found.dedup();
            Names::Some(found)
        };
        Self {
            vars: names("vars", whole("vars")),
            secrets: names(
                "secrets",
                whole("secrets") || squeezed.contains("secrets:inherit"),
            ),
        }
    }
}

/// Names a workflow reads from one context.
#[derive(Debug)]
enum Names {
    All,
    Some(Vec<String>),
}

impl Names {
    /// GitHub stores variable and secret names in upper case.
    fn covers(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Some(names) => names.iter().any(|n| n.eq_ignore_ascii_case(name)),
        }
    }

    fn any(&self) -> bool {
        match self {
            Self::All => true,
            Self::Some(names) => !names.is_empty(),
        }
    }
}

/// The workflows at the policy authority and what names that authority.
struct Authority {
    repo: git2::Repository,
    flows: Vec<Flow>,
    /// The remote and default branch; `None` in the labelled states.
    remote: Option<(String, String)>,
    /// `owner/name` of the remote, when its URL names a GitHub repository.
    slug: Option<String>,
    shown: String,
}

impl Authority {
    fn read(cwd: Option<&Path>) -> Result<Self, String> {
        let cwd = cwd.ok_or("the command's directory is not known")?;
        let repo = crate::hooks::repo::open(cwd)?.ok_or("no repository here")?;
        let root = repo.workdir().unwrap_or(repo.path()).to_path_buf();
        let landed = crate::hooks::landed_policy::load(&root)?;
        let commit = landed.commit.ok_or("no committed authority")?;
        let remote = landed.authority.as_ref().and_then(|authority| {
            let rest = authority.reference.strip_prefix("refs/remotes/")?;
            let (remote, branch) = rest.split_once('/')?;
            Some((remote.to_owned(), branch.to_owned()))
        });
        let slug = remote
            .as_ref()
            .and_then(|(name, _)| repo.find_remote(name).ok())
            .and_then(|remote| remote.url().ok().and_then(github_slug));
        let flows = {
            let tree = repo
                .find_commit(commit)
                .and_then(|commit| commit.tree())
                .map_err(|e| e.to_string())?;
            flows_in(&repo, &tree)?
        };
        Ok(Self {
            repo,
            flows,
            remote,
            slug,
            shown: landed.source,
        })
    }

    /// Why the repository `target` names is not the authority's, or
    /// `None`.
    fn target(&self, target: &Target) -> Option<String> {
        match target {
            Target::Named(named) => self.foreign(named),
            Target::Unresolved(why) => Some(why.clone()),
            Target::Current => self.current(),
            Target::Literal => None,
        }
    }

    /// Why the repository an API path names, `owner/name` or gh's
    /// `{owner}` and `{repo}` placeholders, is not the authority's or
    /// disagrees with `-R` and `GH_REPO`, or `None`.
    fn path_repo(&self, owner: &str, name: &str, target: &Target) -> Option<String> {
        let placeholder = owner == "{owner}" || name == "{repo}";
        if placeholder && !matches!(target, Target::Literal) {
            // gh fills both from the repository it acts on, which must be
            // the authority's; a literal part must then match it too.
            if let Some(why) = self.target(target) {
                return Some(why);
            }
            let slug = self.slug.as_deref().unwrap_or_default();
            let (slug_owner, slug_name) = slug.split_once('/').unwrap_or_default();
            let owner = if owner == "{owner}" {
                slug_owner
            } else {
                owner
            };
            let name = if name == "{repo}" { slug_name } else { name };
            return self.foreign(&format!("{owner}/{name}"));
        }
        let named = format!("{owner}/{name}");
        if let Target::Named(flag) | Target::Unresolved(flag) = target {
            let key = |repo: &str| github_slug(repo).unwrap_or_else(|| repo.to_ascii_lowercase());
            if matches!(target, Target::Unresolved(_)) || key(flag) != key(&named) {
                return Some(format!(
                    "names repository {} in its path, which `-R` or `GH_REPO` contradicts ({})",
                    crate::git::display_text(&named),
                    crate::git::display_text(flag)
                ));
            }
        }
        self.foreign(&named)
    }

    /// Why gh's current repository may not be the authority's: every
    /// GitHub remote of this repository must name the authority's
    /// repository, since gh picks among them by rules the guard does not
    /// repeat.
    fn current(&self) -> Option<String> {
        let unsure = |why: String| {
            Some(format!(
                "acts on gh's current repository, which the guard cannot tell is the policy authority's: {why}"
            ))
        };
        let names = match self.repo.remotes() {
            Ok(names) => names,
            Err(e) => return unsure(format!("the remotes cannot be read ({})", e.message())),
        };
        let mut github = 0;
        for name in &names {
            let Ok(Some(name)) = name else {
                return unsure("a remote name is not UTF-8".into());
            };
            let url = self
                .repo
                .find_remote(name)
                .ok()
                .and_then(|r| r.url().ok().map(str::to_owned));
            let Some(url) = url else {
                return unsure(format!(
                    "remote {} has no readable URL",
                    crate::git::display_text(name)
                ));
            };
            if !url.to_ascii_lowercase().contains("github") {
                continue;
            }
            github += 1;
            if self.slug.is_none() || github_slug(&url) != self.slug {
                return unsure(format!(
                    "remote {} names another repository",
                    crate::git::display_text(name)
                ));
            }
        }
        (github == 0).then(|| {
            "acts on gh's current repository, and no remote here names a GitHub repository".into()
        })
    }

    /// Why `named` is not the authority's repository, or `None`.
    fn foreign(&self, named: &str) -> Option<String> {
        let shown = crate::git::display_text(named);
        if opaque(named) {
            return Some(format!(
                "names repository {shown}, which the guard cannot resolve, so it cannot read its workflows"
            ));
        }
        let wanted = github_slug(named).unwrap_or_else(|| named.to_ascii_lowercase());
        (self.slug.as_deref() != Some(wanted.as_str())).then(|| {
            format!(
                "names repository {shown}, whose workflows the guard cannot read; only the repository of the policy authority is read"
            )
        })
    }

    fn is_default(&self, reference: &str) -> bool {
        let reference = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        self.remote
            .as_ref()
            .is_some_and(|(_, branch)| branch == reference)
    }

    /// The workflow `path` as the remote's tracking ref for `reference`
    /// holds it.
    fn flow_at(&self, reference: &str, path: &str) -> Result<Option<Flow>, String> {
        let (remote, _) = self.remote.as_ref().ok_or("no remote")?;
        let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        let tree = self
            .repo
            .find_reference(&format!("refs/remotes/{remote}/{branch}"))
            .and_then(|r| r.peel_to_tree())
            .map_err(|e| e.to_string())?;
        Ok(flows_in(&self.repo, &tree)?
            .into_iter()
            .find(|flow| flow.path == path))
    }
}

/// `owner/name`, lower case, from a github.com URL (`https://`,
/// `ssh://` or scp-style `git@github.com:`), gh's `HOST/OWNER/REPO` or a
/// bare `owner/name`. The host is compared as [`host_name`] reads it, so
/// a look-alike such as `evilgithub.com` names no GitHub repository.
fn github_slug(text: &str) -> Option<String> {
    let github = |host: &str| {
        host_name(host).is_some_and(|h| matches!(h.as_str(), "github.com" | "www.github.com"))
    };
    let path = if let Some((_, after)) = text.split_once("://") {
        let (authority, path) = after.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        if authority.matches('@').count() > 1 || !github(host) {
            return None;
        }
        path
    } else if let Some((user_host, path)) = text.split_once(':') {
        let host = user_host.rsplit_once('@').map_or(user_host, |(_, h)| h);
        if user_host.contains('/') || !github(host) {
            return None;
        }
        path
    } else {
        match text.split('/').collect::<Vec<_>>().as_slice() {
            [_, _] => text,
            [host, _, _] if github(host) => text.split_once('/')?.1,
            _ => return None,
        }
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let part = |p: &str| {
        !matches!(p, "" | "." | "..")
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    };
    (part(owner) && part(name)).then(|| format!("{owner}/{name}").to_ascii_lowercase())
}

const WORKFLOWS: &str = ".github/workflows";
const READ_LIMIT: usize = 1024 * 1024;

fn flows_in(repo: &git2::Repository, tree: &git2::Tree<'_>) -> Result<Vec<Flow>, String> {
    let dir = match tree.get_path(Path::new(WORKFLOWS)) {
        Ok(entry) => entry,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let dir = dir
        .to_object(repo)
        .and_then(|o| o.peel_to_tree())
        .map_err(|e| format!("{WORKFLOWS} is not a directory: {e}"))?;
    let mut flows = Vec::new();
    for entry in &dir {
        let Ok(name) = entry.name() else {
            return Err(format!("{WORKFLOWS} holds a name that is not UTF-8"));
        };
        let extension = Path::new(name).extension().and_then(|e| e.to_str());
        if !extension
            .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
        {
            continue;
        }
        let path = format!("{WORKFLOWS}/{name}");
        let text = (entry.kind() == Some(git2::ObjectType::Blob) && entry.filemode() != 0o120_000)
            .then(|| repo.find_blob(entry.id()).ok())
            .flatten()
            .filter(|blob| blob.size() <= READ_LIMIT)
            .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok());
        flows.push(match text {
            Some(text) => classify(path, &text),
            None => Flow::unreadable(path, "the guard cannot read the file"),
        });
    }
    Ok(flows)
}

/// Token scopes whose write lets a run publish, or start or unblock a run
/// that publishes: `actions` dispatches and re-runs workflows, and
/// `administration` changes rulesets, environments and branch protection.
const PUBLISHING: &[&str] = &[
    "actions",
    "administration",
    "contents",
    "packages",
    "deployments",
    "pages",
    "attestations",
    "id-token",
];

/// Whether permissions write a publishing scope: `None` when unset.
fn permissions_publish(value: Option<&serde_yaml::Value>) -> Option<bool> {
    use serde_yaml::Value;
    Some(match value? {
        Value::String(s) => s != "read-all",
        Value::Mapping(map) => map.iter().any(|(key, level)| {
            key.as_str().is_none_or(|k| PUBLISHING.contains(&k))
                && level.as_str().is_none_or(|l| l != "read" && l != "none")
        }),
        Value::Null => return None,
        _ => true,
    })
}

fn classify(path: String, text: &str) -> Flow {
    use serde_yaml::Value;
    let unreadable = |path| Flow::unreadable(path, "the guard cannot parse the file");
    let Ok(Value::Mapping(doc)) = serde_yaml::from_str::<Value>(text) else {
        return unreadable(path);
    };
    let name = doc.get("name").and_then(Value::as_str).map(str::to_owned);
    let on = doc
        .get("on")
        .or_else(|| doc.get(Value::Bool(true)))
        .cloned()
        .unwrap_or(Value::Null);
    let events: Vec<(String, Value)> = match on {
        Value::String(event) => vec![(event, Value::Null)],
        Value::Sequence(list) => list
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| (s.to_owned(), Value::Null)))
            .collect(),
        Value::Mapping(map) => map
            .into_iter()
            .filter_map(|(k, v)| k.as_str().map(|s| (s.to_owned(), v)))
            .collect(),
        _ => return unreadable(path),
    };
    let event = |name: &str| events.iter().find(|(e, _)| e == name).map(|(_, v)| v);
    // A types value the guard cannot read listens for any type.
    let repository_dispatch =
        event("repository_dispatch").map(|config| match config.get("types") {
            Some(Value::Sequence(list)) if list.iter().all(|v| v.as_str().is_some()) => {
                Listen::Types(
                    list.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect(),
                )
            }
            Some(Value::String(one)) => Listen::Types(vec![one.clone()]),
            _ => Listen::Any,
        });
    if event("workflow_dispatch").is_none() && repository_dispatch.is_none() {
        return Flow {
            path,
            name,
            release: None,
            repository_dispatch: None,
            reads: Reads::of(text),
        };
    }
    let mut reasons = Vec::new();
    let top = permissions_publish(doc.get("permissions"));
    if top == Some(true) {
        reasons.push("its token can write a publishing scope".to_owned());
    }
    match doc.get("jobs") {
        Some(Value::Mapping(jobs)) => {
            for (job_name, job) in jobs {
                let job_name = job_name.as_str().unwrap_or("?");
                let own = permissions_publish(job.get("permissions"));
                if own == Some(true) {
                    reasons.push(format!("job {job_name} can write a publishing scope"));
                } else if own.is_none() && top.is_none() {
                    reasons.push(format!(
                        "job {job_name} takes the repository's default token permissions"
                    ));
                }
                if job.get("uses").is_some() {
                    reasons.push(format!("job {job_name} calls a reusable workflow"));
                }
                if job.get("environment").is_some() {
                    reasons.push(format!("job {job_name} deploys to an environment"));
                }
            }
        }
        _ => reasons.push("its jobs cannot be read".into()),
    }
    let mut rest = text;
    while let Some(at) = rest.find("secrets.") {
        rest = &rest[at + "secrets.".len()..];
        let secret: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !secret.eq_ignore_ascii_case("GITHUB_TOKEN") {
            reasons.push(format!("it reads secret {secret}"));
            break;
        }
    }
    reasons.dedup();
    Flow {
        path,
        name,
        release: (!reasons.is_empty()).then(|| {
            reasons.truncate(2);
            reasons.join("; ")
        }),
        repository_dispatch,
        reads: Reads::of(text),
    }
}

/// Whether a bare push refspec part has the shape of a release tag: the
/// `v*` pattern of this repository's `release tags` ruleset, or a leading
/// digit as version tags elsewhere take. The guard cannot read an adopter's
/// ruleset, so these shapes stand for every adopter.
pub(crate) fn release_tag_shaped(part: &str) -> bool {
    !part.starts_with("refs/")
        && part != "HEAD"
        && part
            .chars()
            .next()
            .is_some_and(|c| c == 'v' || c.is_ascii_digit())
}

#[cfg(test)]
#[path = "release_routes_tests.rs"]
mod tests;
