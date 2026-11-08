//! Release routes through `gh` and the GitHub API (TSK-1346).
//!
//! The release family in the action table refuses `gh release` and tag
//! pushes. This reader adds the routes that publish without them: a
//! workflow dispatch of a release workflow, a repository dispatch a release
//! workflow listens for, a pending deployment approval, a release or tag
//! creation through the API, and a write to the Actions policies,
//! environments or rulesets that hold the remote floors. Each returns the
//! form it names, and every doubt refuses.
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

/// The refusal for `gh` arguments, or `None` when the call is not a release
/// route. `args` are the words after `gh`; `sub` starts at its subcommand.
pub(crate) fn gh(args: &[String], sub: &[String], cwd: Option<&Path>) -> Option<String> {
    match sub.first().map(String::as_str) {
        Some("workflow") if sub.get(1).is_some_and(|s| s == "run") => {
            workflow_run(args, &sub[2..], cwd)
        }
        Some("api") => api(&sub[1..], cwd),
        _ => None,
    }
}

/// The refusal for a `curl` call that writes to a GitHub API release route.
/// A URL held in a variable is not read: that route's floor is remote
/// (`docs/releasing.md`).
pub(crate) fn curl(args: &[String], cwd: Option<&Path>) -> Option<String> {
    let mut method: Option<String> = None;
    let mut implied: Option<&'static str> = None;
    let mut get = false;
    let mut body = Body::default();
    let mut urls = Vec::new();
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        let (name, attached) = split_option(word, &["-X", "-d", "-F", "-T"]);
        let mut value = || {
            attached.clone().or_else(|| {
                at += 1;
                args.get(at - 1).cloned()
            })
        };
        match name.as_str() {
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
    urls.iter().find_map(|url| {
        let path = api_path(url)?;
        judge(&format!("curl -X {method}"), &method, &path, &body, cwd)
    })
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

fn workflow_run(args: &[String], rest: &[String], cwd: Option<&Path>) -> Option<String> {
    let mut repo = repo_flag(args);
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
                "-R" | "--repo" => repo = value(),
                "-r" | "--ref" => reference = value(),
                "-f" | "-F" | "--field" | "--raw-field" => {
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
    if let Some(why) = authority.foreign(repo.as_deref()) {
        return Some(format!("{form} {why}"));
    }
    dispatch(form, &authority, &selector, reference.as_deref(), false)
}

/// The `-R`/`--repo` value given before the subcommand.
fn repo_flag(args: &[String]) -> Option<String> {
    let mut found = None;
    for (at, word) in args.iter().enumerate() {
        let (name, attached) = split_option(word, &["-R"]);
        if matches!(name.as_str(), "-R" | "--repo") {
            found = attached.or_else(|| args.get(at + 1).cloned());
        }
    }
    found
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
    let shown = crate::git::display_text(selector);
    if opaque(selector) {
        return Some(format!(
            "{form} names workflow {shown}, which the guard cannot resolve"
        ));
    }
    if selector.bytes().all(|b| b.is_ascii_digit()) {
        return Some(format!(
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
        return Some(format!(
            "{form} names workflow {shown}, which matches no workflow file at the policy authority {}",
            authority.shown
        ));
    }
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

fn api(rest: &[String], cwd: Option<&Path>) -> Option<String> {
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
        let (name, attached) = split_option(word, &["-X", "-f", "-F", "-H", "-q", "-t", "-p"]);
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
            | "--preview" | "--cache" => {
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
    if endpoint == "graphql" {
        return graphql(&body);
    }
    let path = api_path(&endpoint).unwrap_or(endpoint);
    judge(&format!("gh api -X {method}"), &method, &path, &body, cwd)
}

/// GraphQL mutations that create or move a tag, approve a deployment, or
/// change an environment or ruleset.
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
];

fn graphql(body: &Body) -> Option<String> {
    match body.field("query") {
        Field::Known(query) => MUTATIONS
            .iter()
            .find(|name| query.contains(**name))
            .map(|name| format!("gh api graphql runs the {name} mutation")),
        Field::Unknown => Some(
            "gh api graphql sends a query the guard cannot read, which may publish or approve a release"
                .into(),
        ),
        Field::Absent => None,
    }
}

/// The API path of a URL or endpoint on a GitHub API host, without host,
/// leading slash or query; `None` for another host.
fn api_path(url: &str) -> Option<String> {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest,
        None if url.starts_with("api.github.com") || url.starts_with("uploads.github.com") => url,
        None => return Some(url.trim_start_matches('/').to_owned()),
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = if matches!(host, "api.github.com" | "uploads.github.com") {
        path
    } else {
        path.strip_prefix("api/v3/")
            .or_else(|| path.strip_prefix("api/uploads/"))?
    };
    Some(path.split(['?', '#']).next().unwrap_or_default().to_owned())
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

fn judge(form: &str, method: &str, path: &str, body: &Body, cwd: Option<&Path>) -> Option<String> {
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
                cwd,
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
                    cwd,
                );
            }
            tail
        }
        Some("orgs") if segs.len() >= 2 => {
            judge_tail(form, method, &shown, None, &segs[2..], body, cwd)
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
    cwd: Option<&Path>,
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
    let repository = fits(tail, &["dispatches"], false);
    if workflow.is_none() && repository.is_none() {
        return None;
    }
    if workflow == Some(false) || repository == Some(false) {
        return unsure("dispatch a release workflow");
    }
    let authority = match Authority::read(cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    let (owner, name) = repo?;
    if owner.wild || name.wild {
        return refuse(
            "names a repository the guard cannot resolve, whose workflows it cannot read",
        );
    }
    if let Some(why) = authority.foreign(Some(&format!("{}/{}", owner.text, name.text))) {
        return Some(format!("{form} {shown} {why}"));
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

    /// Why `named` is not the authority's repository, or `None`.
    fn foreign(&self, named: Option<&str>) -> Option<String> {
        let named = named?;
        let shown = crate::git::display_text(named);
        if opaque(named) {
            return Some(format!(
                "names repository {shown}, which the guard cannot resolve, so it cannot read its workflows"
            ));
        }
        if named == "{owner}/{repo}" {
            return None;
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

/// `owner/name`, lower case, from a GitHub URL or a bare `owner/name`.
fn github_slug(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("git@github.com:")
        .or_else(|| url.split_once("github.com/").map(|(_, rest)| rest))
        .unwrap_or(url);
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !name.is_empty() && !owner.contains(':'))
        .then(|| format!("{owner}/{name}").to_ascii_lowercase())
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
            None => Flow {
                path,
                name: None,
                release: Some("the guard cannot read the file".into()),
                repository_dispatch: Some(Listen::Any),
            },
        });
    }
    Ok(flows)
}

/// Token scopes whose write lets a run publish.
const PUBLISHING: &[&str] = &[
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
    let unreadable = |path: String| Flow {
        path,
        name: None,
        release: Some("the guard cannot parse the file".into()),
        repository_dispatch: Some(Listen::Any),
    };
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
