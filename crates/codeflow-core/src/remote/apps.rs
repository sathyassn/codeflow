//! `codeflow remote setup-apps`: create the authority and agent GitHub Apps
//! from their manifests, the `codeflow-authority` environment restricted to
//! the default branch with the authority App's key secret and id variable,
//! and keep the agent App's key in this computer's credential store
//! (TSK-1348, ADR-0091 Mode A).
//!
//! Every host write runs under the administrator's own `gh` login. The run
//! refuses a GitHub App installation token and a token without
//! Administration write before it creates anything, and the first write,
//! the environment, needs Administration write itself. Each App takes two
//! clicks: "Create GitHub App" on GitHub's manifest page and "Install" on
//! the printed link. Every write is reported with what undoes it.
//!
//! Private keys never reach a file: the authority key goes to `gh secret
//! set` on standard input, and the agent key to the credential store on
//! standard input ([`super::agent_key`]).

use std::collections::BTreeMap;
use std::io::{Read, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::agent_key::{
    app_as_itself, call_failure, committer_for_slug, redact, signed_app_jwt, AgentApp, AppHttp,
    BotCommitter, CallFailure, KeyStore, Signer, API_ROOT,
};
use super::status_is_404;

/// The environment that holds the authority App's key; only the default
/// branch may deploy to it.
pub const AUTHORITY_ENVIRONMENT: &str = "codeflow-authority";
/// The environment secret holding the authority App's private key.
pub const AUTHORITY_KEY_SECRET: &str = "CODEFLOW_AUTHORITY_APP_KEY";
/// The environment variable holding the authority App's id.
pub const AUTHORITY_ID_VARIABLE: &str = "CODEFLOW_AUTHORITY_APP_ID";
/// GitHub's limit on an App name's length.
pub const APP_NAME_LIMIT: usize = 34;

const AUTHORITY_MANIFEST: &str = include_str!("../../../../assets/base/ci/apps/authority-app.json");
const AGENT_MANIFEST: &str = include_str!("../../../../assets/base/ci/apps/agent-app.json");

/// A `gh` call: arguments and optional standard input, answering standard
/// output or the text of a failure.
pub type Gh<'a> = &'a dyn Fn(&[&str], Option<&str>) -> Result<String, String>;

/// Which of the two Apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    /// Checks write only; creates the `authority:` check runs.
    Authority,
    /// Contents and Pull requests write only; the agent sessions' identity.
    Agent,
}

impl AppKind {
    /// "authority" or "agent".
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::Agent => "agent",
        }
    }

    fn template(self) -> &'static str {
        match self {
            Self::Authority => AUTHORITY_MANIFEST,
            Self::Agent => AGENT_MANIFEST,
        }
    }
}

/// The App's manifest for `repository` (`owner/name`), with GitHub sending
/// the browser to `redirect_url` once the App is created.
///
/// # Errors
///
/// When the shipped manifest is not a JSON object.
pub fn manifest(
    kind: AppKind,
    name: &str,
    repository: &str,
    redirect_url: &str,
) -> Result<Value, String> {
    let mut manifest: Value = serde_json::from_str(kind.template()).map_err(|error| {
        format!(
            "the shipped {} App manifest is not JSON: {error}",
            kind.label()
        )
    })?;
    let home = format!("https://github.com/{repository}");
    let Some(fields) = manifest.as_object_mut() else {
        return Err(format!(
            "the shipped {} App manifest is not an object",
            kind.label()
        ));
    };
    fields.insert("name".to_string(), json!(name));
    fields.insert("url".to_string(), json!(home));
    fields.insert(
        "hook_attributes".to_string(),
        json!({"url": home, "active": false}),
    );
    fields.insert("redirect_url".to_string(), json!(redirect_url));
    Ok(manifest)
}

/// The permissions the created App must show: the manifest's, plus the
/// Metadata read GitHub adds to every App.
///
/// # Errors
///
/// When the shipped manifest has no permission map.
pub fn expected_permissions(kind: AppKind) -> Result<BTreeMap<String, String>, String> {
    let manifest = manifest(kind, "", "", "")?;
    let mut permissions: BTreeMap<String, String> = manifest
        .get("default_permissions")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "the shipped {} App manifest has no permissions",
                kind.label()
            )
        })?
        .iter()
        .map(|(name, level)| (name.clone(), level.as_str().unwrap_or_default().to_string()))
        .collect();
    permissions.insert("metadata".to_string(), "read".to_string());
    Ok(permissions)
}

/// The repository the Apps are set up for, and who runs the setup.
#[derive(Clone, Debug)]
pub struct Target {
    /// `owner/name`.
    pub repository: String,
    /// The owning account's login.
    pub owner: String,
    /// Whether the owner is an organization.
    pub organization: bool,
    /// The default branch.
    pub default_branch: String,
    /// Whether the repository is private.
    pub private: bool,
    /// The login `gh` acts as.
    pub login: String,
}

impl Target {
    /// Where the manifest is posted: the account's or the organization's
    /// new App page.
    #[must_use]
    pub fn new_app_url(&self) -> String {
        if self.organization {
            format!(
                "https://github.com/organizations/{}/settings/apps/new",
                self.owner
            )
        } else {
            "https://github.com/settings/apps/new".to_string()
        }
    }

    /// The App's settings page where it is deleted.
    #[must_use]
    pub fn app_settings_url(&self, slug: &str) -> String {
        self.app_page_url(slug, "/advanced")
    }

    /// A page of the App's settings: `tail` is empty or starts with `/`.
    #[must_use]
    pub fn app_page_url(&self, slug: &str, tail: &str) -> String {
        app_page(self.organization.then_some(self.owner.as_str()), slug, tail)
    }
}

/// A page of an App's settings, under `organization` when one owns it.
pub(crate) fn app_page(organization: Option<&str>, slug: &str, tail: &str) -> String {
    match organization {
        Some(owner) => {
            format!("https://github.com/organizations/{owner}/settings/apps/{slug}{tail}")
        }
        None => format!("https://github.com/settings/apps/{slug}{tail}"),
    }
}

/// The page where an App is installed.
#[must_use]
pub fn install_url(slug: &str) -> String {
    format!("https://github.com/apps/{slug}/installations/new")
}

/// Takes the manifest to GitHub in the administrator's browser and brings
/// back the code GitHub returns once the App is created.
pub trait ManifestBrowser {
    /// Where GitHub sends the browser after "Create GitHub App".
    fn redirect_url(&self, target: &Target) -> String;

    /// The code GitHub returned for this manifest, its `state` checked.
    ///
    /// # Errors
    ///
    /// When no code arrives, or its state does not match.
    fn code(
        &mut self,
        kind: AppKind,
        post_url: &str,
        manifest: &Value,
        state: &str,
    ) -> Result<String, String>;
}

/// How a just-created App is asked, as itself, where its webhook points:
/// its JSON Web Token is signed with the key GitHub returned and sent to
/// `GET /app/hook/config`.
pub struct AppAuth<'a> {
    /// Signs the App's JSON Web Token.
    pub signer: &'a dyn Signer,
    /// Sends the call.
    pub http: &'a dyn AppHttp,
    /// Seconds since 1970, for the token's times.
    pub now: u64,
}

/// What to set up.
#[derive(Clone, Debug, Default)]
pub struct SetupOptions {
    /// The authority App's name; default `codeflow-authority-<repo>`.
    pub authority_name: Option<String>,
    /// The agent App's name; default `codeflow-agent-<repo>`.
    pub agent_name: Option<String>,
    /// Whether to set up the agent App.
    pub agent: bool,
}

/// One write made on the host or in the credential store, with what undoes
/// it.
#[derive(Clone, Debug)]
pub struct HostWrite {
    /// What was written.
    pub what: String,
    /// The command or step that undoes it.
    pub undo: String,
}

/// What a run did.
#[derive(Debug, Default)]
pub struct SetupReport {
    /// What the run found and did, in order.
    pub lines: Vec<String>,
    /// Every write made, in order.
    pub writes: Vec<HostWrite>,
    /// Why the run stopped, when it did.
    pub failure: Option<String>,
}

impl SetupReport {
    /// The report for the terminal: the lines, then every write with its
    /// undo. The failure is the caller's to print.
    #[must_use]
    pub fn render(&self) -> crate::git::TextBlock {
        let mut out = crate::git::TextBlock::new();
        for line in &self.lines {
            let _ = writeln!(out, "{line}");
        }
        if self.writes.is_empty() {
            let _ = writeln!(out, "writes: none");
        } else {
            let _ = writeln!(out, "writes, each with what undoes it:");
            for write in &self.writes {
                let _ = writeln!(out, "  - {}", write.what);
                let _ = writeln!(out, "    undo: {}", write.undo);
            }
        }
        out
    }
}

/// Run the setup. The report lists every write made, also when the run
/// stops part way.
pub fn setup(
    gh: Gh<'_>,
    browser: &mut dyn ManifestBrowser,
    store: &dyn KeyStore,
    auth: &AppAuth<'_>,
    options: &SetupOptions,
) -> SetupReport {
    let mut report = SetupReport::default();
    if let Err(why) = run_setup(gh, browser, store, auth, options, &mut report) {
        report.failure = Some(why);
    }
    report
}

fn run_setup(
    gh: Gh<'_>,
    browser: &mut dyn ManifestBrowser,
    store: &dyn KeyStore,
    auth: &AppAuth<'_>,
    options: &SetupOptions,
    report: &mut SetupReport,
) -> Result<(), String> {
    let target = preflight(gh)?;
    report.lines.push(format!(
        "repository: {} ({}, {}, default branch {}); acting as {}",
        target.repository,
        if target.organization {
            "organization"
        } else {
            "personal account"
        },
        if target.private { "private" } else { "public" },
        target.default_branch,
        target.login
    ));
    let authority_name = app_name(
        AppKind::Authority,
        options.authority_name.as_deref(),
        &target,
    )?;
    let agent_name = if options.agent {
        store.available().map_err(|why| {
            format!(
                "refused: the agent App's key needs a credential store, and {why}; run with --no-agent to set up the authority App alone"
            )
        })?;
        Some(app_name(
            AppKind::Agent,
            options.agent_name.as_deref(),
            &target,
        )?)
    } else {
        None
    };
    environment(gh, &target, report)?;
    authority(gh, browser, auth, &target, &authority_name, report)?;
    match agent_name {
        Some(name) => agent(gh, browser, store, auth, &target, &name, report)?,
        None => report
            .lines
            .push("agent App: not set up (--no-agent)".to_string()),
    }
    Ok(())
}

/// The App's name: the one given, or `codeflow-<kind>-<repo>`, checked
/// against GitHub's limit.
fn app_name(kind: AppKind, given: Option<&str>, target: &Target) -> Result<String, String> {
    let repo = target.repository.split('/').nth(1).unwrap_or_default();
    let name = given.map_or_else(
        || format!("codeflow-{}-{}", kind.label(), repo.to_lowercase()),
        str::to_string,
    );
    let flag = format!("--{}-name", kind.label());
    if name.is_empty() || name.chars().count() > APP_NAME_LIMIT {
        return Err(format!(
            "refused: the {} App's name '{name}' is not 1 to {APP_NAME_LIMIT} characters, GitHub's limit; choose one with {flag}",
            kind.label()
        ));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!(
            "refused: the {} App's name '{name}' may hold only letters, digits and hyphens; choose one with {flag}",
            kind.label()
        ));
    }
    Ok(name)
}

/// `owner/name` with GitHub's characters only.
#[must_use]
pub fn valid_repository(repository: &str) -> bool {
    let part = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(repository.split_once('/'), Some((owner, name)) if part(owner) && part(name))
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

fn forbidden(error: &str) -> bool {
    error.contains("403")
        || error.contains("Resource not accessible")
        || error.contains("Must have admin rights")
}

/// Who runs the setup and on what, before anything is written: a GitHub
/// App token and an account without the repository's administrator role
/// are refused.
fn preflight(gh: Gh<'_>) -> Result<Target, String> {
    let user = gh(&["api", "user"], None).map_err(|error| {
        if error.contains("Resource not accessible by integration") {
            "refused: gh is using a GitHub App installation token, such as the agent App's; run the setup under a repository administrator's own `gh auth login`".to_string()
        } else {
            format!(
                "refused: cannot tell which account gh acts as ({}); run the setup under a repository administrator's own `gh auth login`",
                first_line(&error)
            )
        }
    })?;
    let user: Value = serde_json::from_str(&user)
        .map_err(|_| "refused: gh's account answer is not JSON".to_string())?;
    let login = user
        .get("login")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if login.is_empty()
        || login.ends_with("[bot]")
        || user.get("type").and_then(Value::as_str) == Some("Bot")
    {
        return Err(format!(
            "refused: gh acts as '{login}', a GitHub App or bot; run the setup under a repository administrator's own `gh auth login`"
        ));
    }
    let view = gh(&["repo", "view", "--json", "nameWithOwner"], None).map_err(|error| {
        format!(
            "cannot identify the GitHub repository: {}",
            first_line(&error)
        )
    })?;
    let repository = serde_json::from_str::<Value>(&view)
        .ok()
        .and_then(|value| {
            value
                .get("nameWithOwner")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|name| valid_repository(name))
        .ok_or_else(|| {
            "cannot identify the GitHub repository: gh repo view gave no owner/name".to_string()
        })?;
    let repo = gh(&["api", &format!("repos/{repository}")], None)
        .map_err(|error| format!("cannot read {repository}: {}", first_line(&error)))?;
    let repo: Value =
        serde_json::from_str(&repo).map_err(|_| format!("cannot read {repository}: not JSON"))?;
    if repo.pointer("/permissions/admin").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "refused: the setup needs Administration write on {repository}, and {login} is not shown as its administrator; run it under a repository administrator's own `gh auth login`"
        ));
    }
    let owner = repo
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let default_branch = repo
        .get("default_branch")
        .and_then(Value::as_str)
        .filter(|branch| !branch.is_empty())
        .ok_or_else(|| format!("{repository} has no default branch yet; push it first"))?
        .to_string();
    Ok(Target {
        organization: repo.pointer("/owner/type").and_then(Value::as_str) == Some("Organization"),
        private: repo.get("private").and_then(Value::as_bool).unwrap_or(true),
        repository,
        owner,
        default_branch,
        login,
    })
}

/// Deployment from selected branches only, as GitHub writes and shows it.
fn restricted_policy() -> Value {
    json!({"protected_branches": false, "custom_branch_policies": true})
}

/// The environment body: deployment from selected branches only, keeping
/// any reviewers, self-review rule, wait timer and administrator bypass it
/// already has, so a run never removes a protection rule or changes the
/// bypass.
fn environment_body(existing: Option<&Value>) -> Value {
    let mut body = json!({"deployment_branch_policy": restricted_policy()});
    if let Some(bypass) = existing.and_then(|environment| environment.get("can_admins_bypass")) {
        body["can_admins_bypass"] = bypass.clone();
    }
    let rules = existing
        .and_then(|environment| environment.get("protection_rules"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for rule in rules {
        match rule.get("type").and_then(Value::as_str) {
            Some("wait_timer") => {
                body["wait_timer"] = rule.get("wait_timer").cloned().unwrap_or(json!(0));
            }
            Some("required_reviewers") => {
                if let Some(prevent) = rule.get("prevent_self_review") {
                    body["prevent_self_review"] = prevent.clone();
                }
                let reviewers: Vec<Value> = rule
                    .get("reviewers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|reviewer| {
                        json!({
                            "type": reviewer.get("type").cloned().unwrap_or(Value::Null),
                            "id": reviewer.pointer("/reviewer/id").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect();
                body["reviewers"] = json!(reviewers);
            }
            _ => {}
        }
    }
    body
}

/// Create or restrict `codeflow-authority` to the default branch. This is
/// the first write, and it needs Administration write. An environment that
/// is already restricted gets no write.
fn environment(gh: Gh<'_>, target: &Target, report: &mut SetupReport) -> Result<(), String> {
    let repository = &target.repository;
    let path = format!("repos/{repository}/environments/{AUTHORITY_ENVIRONMENT}");
    let existing = match gh(&["api", &path], None) {
        Ok(text) => Some(serde_json::from_str::<Value>(&text).map_err(|_| {
            format!("the environment {AUTHORITY_ENVIRONMENT} could not be read: not JSON")
        })?),
        Err(error) if status_is_404(&error) => None,
        Err(error) => {
            return Err(format!(
                "cannot read the environment {AUTHORITY_ENVIRONMENT}: {}",
                first_line(&error)
            ))
        }
    };
    let restricted = restricted_policy();
    let shown_policy = existing
        .as_ref()
        .and_then(|environment| environment.get("deployment_branch_policy"));
    if shown_policy == Some(&restricted) {
        report.lines.push(format!(
            "environment {AUTHORITY_ENVIRONMENT}: already deploys from selected branches only"
        ));
        return branch_policy(gh, target, &path, report);
    }
    let free_note = if target.private {
        "; a private repository on GitHub Free has no environment secrets, so it uses Mode B (docs/policy-reference.md \"Mode A setup\")"
    } else {
        ""
    };
    let body = environment_body(existing.as_ref());
    gh(&["api", "-X", "PUT", &path, "--input", "-"], Some(&body.to_string())).map_err(|error| {
        if forbidden(&error) || status_is_404(&error) {
            format!(
                "refused: writing the environment {AUTHORITY_ENVIRONMENT} needs Administration write on {repository}, which this token lacks ({}){free_note}",
                first_line(&error)
            )
        } else {
            format!(
                "cannot write the environment {AUTHORITY_ENVIRONMENT}: {}{free_note}",
                first_line(&error)
            )
        }
    })?;
    report.writes.push(match &existing {
        None => HostWrite {
            what: format!("created the environment {AUTHORITY_ENVIRONMENT}, deployments from selected branches only"),
            undo: format!("gh api -X DELETE {path}"),
        },
        Some(_) => HostWrite {
            what: format!(
                "set the environment {AUTHORITY_ENVIRONMENT} to deploy from selected branches only (was {})",
                shown_policy.map_or_else(|| "any branch".to_string(), Value::to_string)
            ),
            undo: format!(
                "set its deployment branches back in the repository's Settings, Environments, {AUTHORITY_ENVIRONMENT}"
            ),
        },
    });
    branch_policy(gh, target, &path, report)
}

/// Exactly one deployment branch policy: the default branch.
fn branch_policy(
    gh: Gh<'_>,
    target: &Target,
    path: &str,
    report: &mut SetupReport,
) -> Result<(), String> {
    let policies = format!("{path}/deployment-branch-policies");
    let list = gh(&["api", &format!("{policies}?per_page=100")], None).map_err(|error| {
        format!(
            "cannot read the deployment branch policies: {}",
            first_line(&error)
        )
    })?;
    let list: Value = serde_json::from_str(&list)
        .map_err(|_| "the deployment branch policies are not JSON".to_string())?;
    let entries = list
        .get("branch_policies")
        .and_then(Value::as_array)
        .ok_or_else(|| "the deployment branch policies answer has no list".to_string())?;
    // One page is read; more policies than it holds are refused, never read
    // as "restricted to the default branch".
    let total = list.get("total_count").and_then(Value::as_u64);
    if total.is_none_or(|total| total > entries.len() as u64) {
        return Err(format!(
            "the environment {AUTHORITY_ENVIRONMENT} has more deployment branch policies than one page of 100 shows, or GitHub did not say how many; remove those that are not {} with `gh api -X DELETE {policies}/<id>` and run again",
            target.default_branch
        ));
    }
    let is_default = |entry: &&Value| {
        entry.get("name").and_then(Value::as_str) == Some(target.default_branch.as_str())
            && entry
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind == "branch")
    };
    let others: Vec<String> = entries
        .iter()
        .filter(|entry| !is_default(entry))
        .map(|entry| {
            format!(
                "'{}' (id {})",
                entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                entry.get("id").map_or_else(String::new, Value::to_string)
            )
        })
        .collect();
    if !others.is_empty() {
        return Err(format!(
            "the environment {AUTHORITY_ENVIRONMENT} also admits {}, so it is not restricted to {}; remove each with `gh api -X DELETE {policies}/<id>` and run again",
            others.join(", "),
            target.default_branch
        ));
    }
    if entries.iter().any(|entry| is_default(&entry)) {
        report.lines.push(format!(
            "environment {AUTHORITY_ENVIRONMENT}: deploys only from {}",
            target.default_branch
        ));
        return Ok(());
    }
    let body = json!({"name": target.default_branch, "type": "branch"});
    let answer = gh(
        &["api", "-X", "POST", &policies, "--input", "-"],
        Some(&body.to_string()),
    )
    .map_err(|error| {
        format!(
            "cannot restrict the environment to {}: {}",
            target.default_branch,
            first_line(&error)
        )
    })?;
    let id = serde_json::from_str::<Value>(&answer)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
        .map_or_else(|| "<id>".to_string(), |id| id.to_string());
    report.writes.push(HostWrite {
        what: format!(
            "restricted the environment {AUTHORITY_ENVIRONMENT} to the branch {} (policy {id})",
            target.default_branch
        ),
        undo: format!("gh api -X DELETE {policies}/{id}"),
    });
    Ok(())
}

/// What GitHub returns when it converts a manifest code. `Debug` never
/// prints the key; the client and webhook secrets are dropped.
struct Converted {
    id: u64,
    slug: String,
    client_id: String,
    pem: String,
}

impl std::fmt::Debug for Converted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Converted {{ id: {}, slug: {} }}", self.id, self.slug)
    }
}

/// A manifest code: letters, digits, `-` and `_`.
fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 200
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Create one App: the manifest goes to GitHub through `browser`, and the
/// code it brings back is converted. The created App must be the one
/// asked for (name, owner, permissions), or it is reported for deletion
/// and its key dropped.
fn create_app(
    gh: Gh<'_>,
    browser: &mut dyn ManifestBrowser,
    auth: &AppAuth<'_>,
    target: &Target,
    kind: AppKind,
    name: &str,
    report: &mut SetupReport,
) -> Result<Converted, String> {
    let label = kind.label();
    let state = uuid::Uuid::new_v4().simple().to_string();
    let manifest = manifest(
        kind,
        name,
        &target.repository,
        &browser.redirect_url(target),
    )?;
    let code = browser.code(kind, &target.new_app_url(), &manifest, &state)?;
    if !valid_code(&code) {
        return Err(format!(
            "the code returned for the {label} App is not a manifest code"
        ));
    }
    let answer = gh(
        &[
            "api",
            "-X",
            "POST",
            &format!("app-manifests/{code}/conversions"),
        ],
        None,
    )
    .map_err(|error| {
        format!(
            "GitHub did not convert the {label} App's code: {}",
            first_line(&error)
        )
    })?;
    let answer: Value = serde_json::from_str(&answer)
        .map_err(|_| format!("GitHub's answer for the {label} App is not JSON"))?;
    let text = |field: &str| {
        answer
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let (Some(id), Some(slug)) = (answer.get("id").and_then(Value::as_u64), text("slug")) else {
        return Err(format!(
            "GitHub's answer for the {label} App has no id or slug; check the account's Apps"
        ));
    };
    report.writes.push(HostWrite {
        what: format!("created the {label} GitHub App {slug} (id {id})"),
        undo: format!(
            "delete it at {} (\"Delete GitHub App\", a web step)",
            target.app_settings_url(&slug)
        ),
    });
    let (Some(client_id), Some(pem)) = (text("client_id"), text("pem")) else {
        return Err(format!(
            "GitHub's answer for the {label} App {slug} has no client id or key; delete the App and run again"
        ));
    };
    let owner = answer
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let created_name = text("name").unwrap_or_default();
    let permissions: BTreeMap<String, String> = answer
        .get("permissions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, level)| (name.clone(), level.as_str().unwrap_or_default().to_string()))
        .collect();
    let expected = expected_permissions(kind)?;
    let mut wrong = Vec::new();
    if !owner.eq_ignore_ascii_case(&target.owner) {
        wrong.push(format!("owner '{owner}', not '{}'", target.owner));
    }
    if created_name != name {
        wrong.push(format!("name '{created_name}', not '{name}'"));
    }
    if permissions != expected {
        wrong.push(format!(
            "permissions {}, not {}",
            json!(permissions),
            json!(expected)
        ));
    }
    wrong.extend(event_problems(&answer));
    if wrong.is_empty() {
        wrong.extend(hook_problems(auth, target, &client_id, &pem));
    }
    if !wrong.is_empty() {
        return Err(format!(
            "the {label} App GitHub created is not the one asked for ({}); its key was dropped, so delete it at {} and run again",
            wrong.join("; "),
            target.app_settings_url(&slug)
        ));
    }
    Ok(Converted {
        id,
        slug,
        client_id,
        pem,
    })
}

/// What is wrong with an App's events: it must subscribe to none, and a
/// webhook GitHub reports must be inactive.
fn event_problems(app: &Value) -> Vec<String> {
    let mut wrong = Vec::new();
    match app.get("events").and_then(Value::as_array) {
        Some(events) if events.is_empty() => {}
        Some(events) => wrong.push(format!("events {}, not none", json!(events))),
        None => wrong.push("no events list, so its events cannot be confirmed".to_string()),
    }
    if app
        .pointer("/hook_attributes/active")
        .and_then(Value::as_bool)
        == Some(true)
    {
        wrong.push("an active webhook".to_string());
    }
    wrong
}

/// What is wrong with a just-created App's webhook, asked as the App: it
/// must point at the repository page the manifest named, or nowhere, so no
/// delivery leaves for a server the administrator did not choose.
fn hook_problems(auth: &AppAuth<'_>, target: &Target, client_id: &str, pem: &str) -> Vec<String> {
    let home = format!("https://github.com/{}", target.repository);
    let jwt = match signed_app_jwt(client_id, pem, auth.signer, auth.now) {
        Ok(jwt) => jwt,
        Err(error) => {
            return vec![format!(
                "a webhook that can be checked (its key did not sign: {})",
                redact(&error, &[pem])
            )]
        }
    };
    let secrets = [pem, jwt.as_str()];
    match auth
        .http
        .call("GET", &format!("{API_ROOT}/app/hook/config"), &jwt, None)
    {
        Ok(answer) => {
            let url = serde_json::from_str::<Value>(&answer)
                .ok()
                .and_then(|value| value.get("url").and_then(Value::as_str).map(str::to_string));
            match url {
                Some(url) if url.is_empty() || url == home => Vec::new(),
                Some(url) => vec![format!("a webhook to '{url}', not {home}")],
                None => vec!["a webhook configuration GitHub did not describe".to_string()],
            }
        }
        Err(error) if call_failure(&error) == CallFailure::Status(404) => Vec::new(),
        Err(error) => vec![format!(
            "a webhook that can be checked ({})",
            first_line(&redact(&error, &secrets))
        )],
    }
}

/// What `gh api apps/<slug>` gave.
enum AppLookup {
    /// GitHub showed the App.
    Shown(Value),
    /// An HTTP 404: a private App this login cannot read, or a deleted App.
    /// This login cannot tell them apart.
    NotShown,
    /// Anything else, as an item for the "missing" list.
    Failed(String),
}

/// Read an App under the administrator's login.
fn lookup_app(gh: Gh<'_>, kind: AppKind, slug: &str) -> AppLookup {
    let label = kind.label();
    match gh(&["api", &format!("apps/{slug}")], None) {
        Ok(answer) => serde_json::from_str(&answer).map_or_else(
            |_| {
                AppLookup::Failed(format!(
                    "the {label} App {slug}: GitHub's answer is not JSON"
                ))
            },
            AppLookup::Shown,
        ),
        Err(error) if status_is_404(&error) => AppLookup::NotShown,
        Err(error) => AppLookup::Failed(format!(
            "the {label} App {slug} on GitHub ({})",
            first_line(&error)
        )),
    }
}

/// What is missing or wrong in `app`, an App GitHub showed: its id,
/// permissions and events must be those the manifest asks for.
pub(crate) fn app_problems(kind: AppKind, slug: &str, id: Option<u64>, app: &Value) -> Vec<String> {
    let label = kind.label();
    let mut wrong = Vec::new();
    let found = app.get("id").and_then(Value::as_u64);
    if id.is_none() || found != id {
        wrong.push(format!(
            "the {label} App {slug} with the id kept here ({}; GitHub shows {})",
            id.map_or_else(|| "none".to_string(), |id| id.to_string()),
            found.map_or_else(|| "none".to_string(), |id| id.to_string())
        ));
    }
    let permissions: BTreeMap<String, String> = app
        .get("permissions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, level)| (name.clone(), level.as_str().unwrap_or_default().to_string()))
        .collect();
    match expected_permissions(kind) {
        Ok(expected) if expected == permissions => {}
        Ok(expected) => wrong.push(format!(
            "the {label} App {slug} with permissions {} (it has {})",
            json!(expected),
            json!(permissions)
        )),
        Err(why) => wrong.push(why),
    }
    wrong.extend(
        event_problems(app)
            .into_iter()
            .map(|why| format!("the {label} App {slug} without {why}")),
    );
    wrong
}

/// The lines for an authority App this login cannot read: what was checked,
/// what was not and why, and where to look.
fn unverified_authority_lines(
    target: &Target,
    slug: &str,
    id: u64,
    replace: &str,
) -> Result<Vec<String>, String> {
    let permissions = expected_permissions(AppKind::Authority)?
        .iter()
        .map(|(name, level)| format!("{name} {level}"))
        .collect::<Vec<_>>()
        .join(" and ");
    Ok(vec![
        "authority App: present but not verified, nothing created".to_string(),
        format!(
            "  checked: the variable {AUTHORITY_ID_VARIABLE} ({id}) and the secret {AUTHORITY_KEY_SECRET} (its name only; the value cannot be read)"
        ),
        format!(
            "  not checked: the App {slug} itself. GitHub's API answered 404, and it answers 404 both for a private App this login cannot read and for a deleted App, so this run cannot tell them apart"
        ),
        format!(
            "  check it yourself: {} should show App ID {id}, and {} should show only {permissions}, with no events",
            target.app_page_url(slug, ""),
            target.app_page_url(slug, "/permissions")
        ),
        format!("  {replace}"),
    ])
}

/// The authority App, its key secret and id variable.
fn authority(
    gh: Gh<'_>,
    browser: &mut dyn ManifestBrowser,
    auth: &AppAuth<'_>,
    target: &Target,
    name: &str,
    report: &mut SetupReport,
) -> Result<(), String> {
    let repository = &target.repository;
    let environment = format!("repos/{repository}/environments/{AUTHORITY_ENVIRONMENT}");
    let variable = format!("{environment}/variables/{AUTHORITY_ID_VARIABLE}");
    match gh(&["api", &variable], None) {
        Ok(text) => {
            let value = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .get("value")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            return existing_authority(gh, target, name, &value, &variable, report);
        }
        Err(error) if status_is_404(&error) => {}
        Err(error) => {
            return Err(format!(
                "cannot read {AUTHORITY_ID_VARIABLE} in {AUTHORITY_ENVIRONMENT}: {}",
                first_line(&error)
            ))
        }
    }
    let app = create_app(gh, browser, auth, target, AppKind::Authority, name, report)?;
    let lost = |what: &str, error: &str| {
        format!(
            "the authority App {} was created, but {what}: {}; its key is not kept, so delete the App at {} and run again",
            app.slug,
            first_line(&redact(error, &[app.pem.as_str()])),
            target.app_settings_url(&app.slug)
        )
    };
    gh(
        &[
            "secret",
            "set",
            AUTHORITY_KEY_SECRET,
            "--env",
            AUTHORITY_ENVIRONMENT,
            "--repo",
            repository,
        ],
        Some(&app.pem),
    )
    .map_err(|error| {
        lost(
            &format!("its key could not be stored as {AUTHORITY_KEY_SECRET}"),
            &error,
        )
    })?;
    report.writes.push(HostWrite {
        what: format!("stored the authority App's key as the secret {AUTHORITY_KEY_SECRET} in {AUTHORITY_ENVIRONMENT}"),
        undo: format!("gh secret delete {AUTHORITY_KEY_SECRET} --env {AUTHORITY_ENVIRONMENT} --repo {repository}"),
    });
    let body = json!({"name": AUTHORITY_ID_VARIABLE, "value": app.id.to_string()});
    gh(
        &["api", "-X", "POST", &format!("{environment}/variables"), "--input", "-"],
        Some(&body.to_string()),
    )
    .map_err(|error| {
        format!(
            "the authority App {} and its key secret are in place, but {AUTHORITY_ID_VARIABLE} could not be stored: {}; set it to {} with `gh api -X POST {environment}/variables -f name={AUTHORITY_ID_VARIABLE} -f value={}`",
            app.slug,
            first_line(&error),
            app.id,
            app.id
        )
    })?;
    report.writes.push(HostWrite {
        what: format!(
            "stored the authority App's id {} as the variable {AUTHORITY_ID_VARIABLE} in {AUTHORITY_ENVIRONMENT}",
            app.id
        ),
        undo: format!("gh api -X DELETE {variable}"),
    });
    report.lines.push(format!(
        "authority App {} (id {}) created; install it on {repository} only (\"Only select repositories\"): {}",
        app.slug,
        app.id,
        install_url(&app.slug)
    ));
    report.lines.push(format!(
        "then, once the policy workflow is on the default branch: codeflow remote protect --check-app {} --lines \"integration/**\"",
        app.id
    ));
    Ok(())
}

/// An authority App already recorded by its id variable: it counts as set
/// up only when the key secret exists (its name; the value is never read)
/// and the App GitHub shows has that id, the manifest's permissions and no
/// events. An App GitHub answers 404 for is reported as present but not
/// verified when the variable and the secret hold, because its key is not
/// local and a private App looks the same as a deleted one. Otherwise the
/// run stops, naming what is present and what is missing.
fn existing_authority(
    gh: Gh<'_>,
    target: &Target,
    name: &str,
    value: &str,
    variable: &str,
    report: &mut SetupReport,
) -> Result<(), String> {
    let repository = &target.repository;
    let environment = format!("repos/{repository}/environments/{AUTHORITY_ENVIRONMENT}");
    let mut present = vec![format!("{AUTHORITY_ID_VARIABLE} '{value}'")];
    let mut missing = Vec::new();
    match gh(
        &[
            "api",
            &format!("{environment}/secrets/{AUTHORITY_KEY_SECRET}"),
        ],
        None,
    ) {
        Ok(_) => present.push(format!("the secret {AUTHORITY_KEY_SECRET}")),
        Err(error) if status_is_404(&error) => {
            missing.push(format!("the secret {AUTHORITY_KEY_SECRET}"));
        }
        Err(error) => missing.push(format!(
            "the secret {AUTHORITY_KEY_SECRET} (cannot read it: {})",
            first_line(&error)
        )),
    }
    let slug = name.to_lowercase();
    let id = value.parse::<u64>().ok();
    let replace = format!(
        "to set it up again, delete that App, then `gh api -X DELETE {variable}` and `gh secret delete {AUTHORITY_KEY_SECRET} --env {AUTHORITY_ENVIRONMENT} --repo {repository}`, and run again"
    );
    match lookup_app(gh, AppKind::Authority, &slug) {
        AppLookup::Shown(app) => {
            let problems = app_problems(AppKind::Authority, &slug, id, &app);
            if problems.is_empty() {
                present.push(format!("the authority App {slug} as the manifest asks"));
            }
            missing.extend(problems);
        }
        AppLookup::Failed(why) => missing.push(why),
        AppLookup::NotShown => match id.filter(|id| *id > 0) {
            // Nothing else will check the variable, so it must hold an id.
            None => missing.push(format!("{AUTHORITY_ID_VARIABLE} holding an App id")),
            Some(id) if missing.is_empty() => {
                report
                    .lines
                    .extend(unverified_authority_lines(target, &slug, id, &replace)?);
                return Ok(());
            }
            Some(_) => {}
        },
    }
    if missing.is_empty() {
        report.lines.push(format!(
            "authority App: already set up ({}), nothing created; {replace}",
            present.join(", ")
        ));
        Ok(())
    } else {
        Err(format!(
            "the authority App is not fully set up; present: {}; missing: {}; {replace}",
            present.join(", "),
            missing.join(", ")
        ))
    }
}

/// The lines a project adds to its harness settings, for the three
/// supported harnesses. They are printed; no settings file is edited.
fn harness_lines(committer: &BotCommitter) -> Vec<String> {
    let BotCommitter { name, email } = committer;
    vec![
        format!("committer: {name} <{email}>"),
        "add these lines to your project settings and commit them; this command edits no settings file, and `codeflow baseline approve` admits the change".to_string(),
        String::new(),
        "Claude Code, inside \"env\" in .claude/settings.json:".to_string(),
        format!("  \"GIT_COMMITTER_NAME\": \"{name}\","),
        format!("  \"GIT_COMMITTER_EMAIL\": \"{email}\""),
        String::new(),
        "Codex, at the end of .codex/config.toml:".to_string(),
        "  [shell_environment_policy.set]".to_string(),
        format!("  GIT_COMMITTER_NAME = \"{name}\""),
        format!("  GIT_COMMITTER_EMAIL = \"{email}\""),
        String::new(),
        "Grok Build has no project environment setting; set these in the shell that launches it:".to_string(),
        format!("  export GIT_COMMITTER_NAME='{name}'"),
        format!("  export GIT_COMMITTER_EMAIL='{email}'"),
    ]
}

/// Report the committer lines of the App `slug`. A bot user GitHub does not
/// show yet is said so, and a call that got no such answer says its own cause;
/// the run neither guesses the values nor stops.
fn report_committer(auth: &AppAuth<'_>, slug: &str, report: &mut SetupReport) {
    match committer_for_slug(slug, auth.http) {
        Ok(committer) => report.lines.extend(harness_lines(&committer)),
        Err(why)
            if matches!(
                call_failure(&why),
                CallFailure::Status(404) | CallFailure::Unclear
            ) =>
        {
            report.lines.push(format!(
                "committer lines: not printed, because GitHub does not show {slug}[bot] yet; install the App and run `codeflow remote setup-apps` again"
            ));
        }
        Err(why) => report.lines.push(format!(
            "committer lines: not printed: {}; run `codeflow remote setup-apps` again",
            first_line(&why)
        )),
    }
}

/// An agent App already kept in the credential store: it counts as set up
/// only when the entry reads as an App with a key and GitHub, asked as the
/// App with that key, shows that id and slug, the repository's owner, the
/// manifest's permissions and no events.
fn existing_agent(
    store: &dyn KeyStore,
    auth: &AppAuth<'_>,
    target: &Target,
    report: &mut SetupReport,
) -> Result<(), String> {
    let repository = &target.repository;
    let replace = format!(
        "to set it up again, delete that App, run `{}`, and run again",
        store.undo(repository)
    );
    let entry = store
        .load(repository)
        .ok()
        .and_then(|raw| serde_json::from_str::<AgentApp>(&raw).ok())
        .filter(|app| app.pem.contains("PRIVATE KEY") && !app.slug.is_empty());
    let Some(app) = entry else {
        return Err(format!(
            "the agent App is not fully set up; present: an entry for {repository} in {}; missing: an agent App and key that entry can be read as; {replace}",
            store.describe()
        ));
    };
    let answer = app_as_itself(
        &app.client_id,
        &app.pem,
        auth.signer,
        auth.http,
        auth.now,
    )
    .map_err(|why| {
        format!(
            "the agent App is not fully set up; present: its key for {repository} in {}; missing: the agent App {} answering as itself ({}); after a network or GitHub failure, run again; otherwise {replace}",
            store.describe(),
            app.slug,
            first_line(&why)
        )
    })?;
    let mut problems = app_problems(AppKind::Agent, &app.slug, Some(app.app_id), &answer);
    let shown_slug = answer
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if shown_slug != app.slug {
        problems.push(format!(
            "the agent App {} (GitHub shows slug '{shown_slug}')",
            app.slug
        ));
    }
    let owner = answer
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !owner.eq_ignore_ascii_case(&target.owner) {
        problems.push(format!(
            "an agent App owned by '{}' (GitHub shows owner '{owner}')",
            target.owner
        ));
    }
    if problems.is_empty() {
        report.lines.push(format!(
            "agent App: already set up (its key for {repository} in {}, and the agent App {} as the manifest asks, read as the App itself through GET /app), nothing created; {replace}",
            store.describe(),
            app.slug
        ));
        report_committer(auth, &app.slug, report);
        Ok(())
    } else {
        Err(format!(
            "the agent App is not fully set up; present: its key for {repository} in {}; missing: {}; {replace}",
            store.describe(),
            problems.join(", ")
        ))
    }
}

/// The agent App, its key kept in the credential store.
fn agent(
    gh: Gh<'_>,
    browser: &mut dyn ManifestBrowser,
    store: &dyn KeyStore,
    auth: &AppAuth<'_>,
    target: &Target,
    name: &str,
    report: &mut SetupReport,
) -> Result<(), String> {
    let repository = &target.repository;
    if store.exists(repository)? {
        return existing_agent(store, auth, target, report);
    }
    let app = create_app(gh, browser, auth, target, AppKind::Agent, name, report)?;
    let entry = AgentApp {
        app_id: app.id,
        client_id: app.client_id.clone(),
        slug: app.slug.clone(),
        pem: app.pem.clone(),
    };
    let entry = serde_json::to_string(&entry)
        .map_err(|error| format!("cannot encode the agent App entry: {error}"))?;
    store.store(repository, &entry).map_err(|why| {
        format!(
            "the agent App {} was created, but {} did not keep its key ({}); the key is not kept, so delete the App at {} and run again",
            app.slug,
            store.describe(),
            redact(&why, &[app.pem.as_str(), entry.as_str()]),
            target.app_settings_url(&app.slug)
        )
    })?;
    report.writes.push(HostWrite {
        what: format!(
            "kept the agent App's key for {repository} in {}",
            store.describe()
        ),
        undo: store.undo(repository),
    });
    report.lines.push(format!(
        "agent App {} (id {}) created; install it on {repository} only (\"Only select repositories\"): {}",
        app.slug,
        app.id,
        install_url(&app.slug)
    ));
    report.lines.push(format!(
        "agent sessions act as {}[bot] with a token from `codeflow remote agent-token`",
        app.slug
    ));
    report_committer(auth, &app.slug, report);
    Ok(())
}

/// Escape text for an HTML attribute or element.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// The page that posts the manifest to GitHub on load, with a button in
/// case scripts are off.
#[must_use]
pub fn manifest_page(kind: AppKind, post_url: &str, manifest: &Value, state: &str) -> String {
    let label = kind.label();
    let action = escape_html(&format!("{post_url}?state={state}"));
    let value = escape_html(&manifest.to_string());
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head><meta charset=\"utf-8\"><title>Create the CodeFlow {label} GitHub App</title></head>\n<body>\n<h1>Create the CodeFlow {label} GitHub App</h1>\n<p>This page sends the App's manifest to GitHub. On GitHub, check the name and the permissions, then click \"Create GitHub App\".</p>\n<form id=\"manifest\" method=\"post\" action=\"{action}\">\n<input type=\"hidden\" name=\"manifest\" value=\"{value}\">\n<button type=\"submit\">Continue to GitHub</button>\n</form>\n<script>document.getElementById(\"manifest\").submit();</script>\n</body>\n</html>\n"
    )
}

/// The code in a redirect (`.../callback?code=..&state=..` or the full
/// address), with its state checked.
///
/// # Errors
///
/// When the state is missing or different, or there is no valid code.
pub fn callback_code(address: &str, state: &str) -> Result<String, String> {
    let query = address.split_once('?').map_or("", |(_, query)| query);
    let query = query.split('#').next().unwrap_or_default();
    let mut code = None;
    let mut returned = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", value)) => code = Some(value),
            Some(("state", value)) => returned = Some(value),
            _ => {}
        }
    }
    if returned != Some(state) {
        return Err(
            "the address GitHub returned does not carry this run's state; start the step again"
                .to_string(),
        );
    }
    match code {
        Some(code) if valid_code(code) => Ok(code.to_string()),
        _ => Err("the address GitHub returned carries no manifest code".to_string()),
    }
}

/// Serves the manifest page on a loopback port and receives GitHub's
/// redirect on the same port. The page is served only at a path with a
/// random part, printed to the terminal.
pub struct LoopbackBrowser {
    listener: TcpListener,
    port: u16,
    wait: Duration,
    announce: Box<dyn Fn(&str)>,
}

impl LoopbackBrowser {
    /// Listen on a free port of 127.0.0.1.
    ///
    /// # Errors
    ///
    /// When no port can be opened.
    pub fn bind(wait: Duration, announce: Box<dyn Fn(&str)>) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot listen on 127.0.0.1: {error}; run with --paste"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot listen on 127.0.0.1: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("cannot read the listening port: {error}"))?
            .port();
        Ok(Self {
            listener,
            port,
            wait,
            announce,
        })
    }
}

/// The request target of an HTTP GET, read from `stream`.
fn request_target(stream: &mut TcpStream) -> Option<String> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    while buffer.len() < 8192 && !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    // The request line, read as bytes up to its CRLF; a target that is not
    // UTF-8 is no request this page answers.
    let line = buffer.split(|byte| *byte == b'\n').next()?;
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let target = line
        .strip_prefix(b"GET ")?
        .split(|byte| *byte == b' ')
        .next()?;
    String::from_utf8(target.to_vec()).ok()
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl ManifestBrowser for LoopbackBrowser {
    fn redirect_url(&self, _: &Target) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    fn code(
        &mut self,
        kind: AppKind,
        post_url: &str,
        manifest: &Value,
        state: &str,
    ) -> Result<String, String> {
        let page = format!("/{}", uuid::Uuid::new_v4().simple());
        let html = manifest_page(kind, post_url, manifest, state);
        (self.announce)(&format!(
            "{} App: open http://127.0.0.1:{}{page} in your browser, check the name and permissions on GitHub, and click \"Create GitHub App\" (waiting up to {} minutes)",
            kind.label(),
            self.port,
            self.wait.as_secs() / 60
        ));
        let deadline = Instant::now() + self.wait;
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => match request_target(&mut stream) {
                    Some(target) if target == page => {
                        respond(&mut stream, "200 OK", "text/html", &html);
                    }
                    Some(target) if target.starts_with("/callback?") => {
                        match callback_code(&target, state) {
                            Ok(code) => {
                                respond(
                                    &mut stream,
                                    "200 OK",
                                    "text/plain",
                                    "GitHub created the App. Return to the terminal.\n",
                                );
                                return Ok(code);
                            }
                            Err(why) => respond(&mut stream, "400 Bad Request", "text/plain", &why),
                        }
                    }
                    _ => respond(&mut stream, "404 Not Found", "text/plain", "not found\n"),
                },
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "no answer from GitHub for the {} App within {} minutes; run again",
                            kind.label(),
                            self.wait.as_secs() / 60
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(format!("the local listener failed: {error}")),
            }
        }
    }
}

/// Writes the manifest page to a file in a private temporary folder and
/// reads back the address GitHub sent the browser to; GitHub returns to
/// the repository's page. The folder is removed when this is dropped.
pub struct PasteBrowser {
    dir: tempfile::TempDir,
    announce: Box<dyn Fn(&str)>,
    input: Box<dyn FnMut() -> Result<String, String>>,
}

impl PasteBrowser {
    /// A browser whose page goes under `scratch`.
    ///
    /// # Errors
    ///
    /// When the folder cannot be made.
    pub fn new(
        scratch: &Path,
        announce: Box<dyn Fn(&str)>,
        input: Box<dyn FnMut() -> Result<String, String>>,
    ) -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("codeflow-app-setup-")
            .tempdir_in(scratch)
            .map_err(|error| format!("cannot make a temporary folder: {error}"))?;
        Ok(Self {
            dir,
            announce,
            input,
        })
    }

    /// Where the page for `kind` is written.
    #[must_use]
    pub fn page_path(&self, kind: AppKind) -> PathBuf {
        self.dir.path().join(format!("{}-app.html", kind.label()))
    }
}

impl ManifestBrowser for PasteBrowser {
    fn redirect_url(&self, target: &Target) -> String {
        format!("https://github.com/{}", target.repository)
    }

    fn code(
        &mut self,
        kind: AppKind,
        post_url: &str,
        manifest: &Value,
        state: &str,
    ) -> Result<String, String> {
        let path = self.page_path(kind);
        std::fs::write(&path, manifest_page(kind, post_url, manifest, state))
            .map_err(|error| format!("cannot write the manifest page: {error}"))?;
        (self.announce)(&format!(
            "{} App: open {} in your browser, check the name and permissions on GitHub, and click \"Create GitHub App\"; then paste here the full address GitHub opens (it holds code= and state=)",
            kind.label(),
            path.display()
        ));
        let line = (self.input)();
        let _ = std::fs::remove_file(&path);
        callback_code(line?.trim(), state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::agent_key::tests::{MemoryStore, UnclearStore};
    use std::cell::RefCell;
    use std::io::{Read, Write};

    // A made-up key: the header is split so no source line holds a key block.
    const AUTHORITY_PEM: &str = concat!(
        "-----BEGIN RSA PRIVATE",
        " KEY-----\nAUTHORITYsecretKEYmaterial\n-----END RSA PRIVATE",
        " KEY-----\n"
    );
    // A made-up key: the header is split so no source line holds a key block.
    const AGENT_PEM: &str = concat!(
        "-----BEGIN RSA PRIVATE",
        " KEY-----\nAGENTsecretKEYmaterial\n-----END RSA PRIVATE",
        " KEY-----\n"
    );

    /// A recorded host: each call is matched by the start of its joined
    /// arguments; every call and its input is kept.
    struct Host {
        answers: Vec<(String, Result<String, String>)>,
        calls: RefCell<Vec<(String, Option<String>)>>,
    }

    impl Host {
        fn new() -> Self {
            Self {
                answers: Vec::new(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn answer(mut self, call: &str, answer: Result<&str, &str>) -> Self {
            self.answers.insert(
                0,
                (
                    call.to_string(),
                    answer.map(str::to_string).map_err(str::to_string),
                ),
            );
            self
        }

        fn gh(&self, args: &[&str], input: Option<&str>) -> Result<String, String> {
            let joined = args.join(" ");
            self.calls
                .borrow_mut()
                .push((joined.clone(), input.map(str::to_string)));
            // A recorded call matches the whole call, or its start up to a
            // space or a query, so `api repos/o/r` never answers a deeper
            // path.
            self.answers
                .iter()
                .find(|(call, _)| {
                    joined.strip_prefix(call.as_str()).is_some_and(|rest| {
                        rest.is_empty() || rest.starts_with(' ') || rest.starts_with('?')
                    })
                })
                .map_or_else(
                    || Err(format!("unexpected call: {joined}")),
                    |(_, answer)| answer.clone(),
                )
        }

        fn writes(&self) -> Vec<String> {
            self.calls
                .borrow()
                .iter()
                .filter(|(call, _)| call.contains("-X") || call.starts_with("secret"))
                .map(|(call, _)| call.clone())
                .collect()
        }
    }

    fn conversion(slug: &str, name: &str, owner: &str, permissions: &str, pem: &str) -> String {
        json!({
            "id": if slug.contains("agent") { 202 } else { 101 },
            "slug": slug,
            "name": name,
            "client_id": format!("Iv1.{slug}"),
            "client_secret": "client-secret-not-kept",
            "webhook_secret": null,
            "pem": pem,
            "owner": {"login": owner, "type": "User"},
            "permissions": serde_json::from_str::<Value>(permissions).unwrap(),
            "events": [],
        })
        .to_string()
    }

    /// Signs anything: the webhook check only needs a token to send.
    struct OkSigner;
    impl Signer for OkSigner {
        fn sign_rs256(&self, pem: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            assert!(pem == AUTHORITY_PEM || pem == AGENT_PEM, "an unknown key");
            Ok(vec![7; 8])
        }
    }

    /// Answers what a run asks without `gh`: `GET /app/hook/config` and
    /// `GET /app` as the App would, and the public bot user. Each call's
    /// path and bearer are kept.
    struct HookHttp {
        hook: Result<String, String>,
        app: Result<String, String>,
        bot: Result<String, String>,
        calls: RefCell<Vec<(String, String)>>,
    }
    impl HookHttp {
        fn repository_page() -> Self {
            Self::hook(Ok(
                r#"{"content_type":"json","url":"https://github.com/o/r"}"#.to_string(),
            ))
        }

        fn hook(hook: Result<String, String>) -> Self {
            Self {
                hook,
                app: Ok(agent_answer(&json!({}))),
                bot: Ok(BOT_USER.to_string()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn app(app: Result<String, String>) -> Self {
            Self {
                app,
                ..Self::repository_page()
            }
        }

        fn bot(bot: Result<String, String>) -> Self {
            Self {
                bot,
                ..Self::repository_page()
            }
        }
    }
    impl AppHttp for HookHttp {
        fn call(
            &self,
            method: &str,
            url: &str,
            bearer: &str,
            body: Option<&str>,
        ) -> Result<String, String> {
            assert_eq!((method, body), ("GET", None));
            let path = url.strip_prefix("https://api.github.com").unwrap();
            self.calls
                .borrow_mut()
                .push((path.to_string(), bearer.to_string()));
            match path {
                "/app/hook/config" => self.hook.clone(),
                "/app" => self.app.clone(),
                "/users/codeflow-agent-r%5Bbot%5D" => self.bot.clone(),
                other => panic!("unexpected call: {other}"),
            }
        }
    }

    /// The public answer for the agent App's bot user.
    const BOT_USER: &str = r#"{"login":"codeflow-agent-r[bot]","id":353475153,"type":"Bot"}"#;

    /// `GET /app` for the agent App, with `changes` over its fields.
    fn agent_answer(changes: &Value) -> String {
        let mut app = json!({
            "id": 202,
            "slug": "codeflow-agent-r",
            "owner": {"login": "o", "type": "User"},
            "permissions": {"contents": "write", "pull_requests": "write", "metadata": "read"},
            "events": [],
        });
        for (key, value) in changes.as_object().unwrap() {
            app[key] = value.clone();
        }
        app.to_string()
    }

    /// The exact failure the operator's live run logged for a private App
    /// under a `gh` login: the response body, then `gh`'s line, joined with
    /// no separator.
    const LIVE_404: &str = r#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest/apps/apps#get-an-app","status":"404"}gh: Not Found (HTTP 404)"#;

    fn auth<'a>(signer: &'a OkSigner, http: &'a HookHttp) -> AppAuth<'a> {
        AppAuth {
            signer,
            http,
            now: 1_800_000_000,
        }
    }

    /// The App GitHub shows for `slug` under the administrator's login.
    fn shown_app(id: u64, permissions: &str) -> String {
        json!({
            "id": id,
            "permissions": serde_json::from_str::<Value>(permissions).unwrap(),
            "events": [],
        })
        .to_string()
    }

    /// The agent App entry a first run keeps.
    fn agent_entry() -> String {
        serde_json::to_string(&AgentApp {
            app_id: 202,
            client_id: "Iv1.codeflow-agent-r".to_string(),
            slug: "codeflow-agent-r".to_string(),
            pem: AGENT_PEM.to_string(),
        })
        .unwrap()
    }

    /// A host where both Apps were set up by an earlier run.
    fn set_up_host() -> Host {
        fresh_host()
            .answer(
                "api repos/o/r/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID",
                Ok(r#"{"name":"CODEFLOW_AUTHORITY_APP_ID","value":"101"}"#),
            )
            .answer(
                "api repos/o/r/environments/codeflow-authority/deployment-branch-policies",
                Ok(r#"{"total_count":1,"branch_policies":[{"id":9,"name":"main","type":"branch"}]}"#),
            )
            .answer(
                "api repos/o/r/environments/codeflow-authority/secrets/CODEFLOW_AUTHORITY_APP_KEY",
                Ok(r#"{"name":"CODEFLOW_AUTHORITY_APP_KEY"}"#),
            )
            .answer(
                "api apps/codeflow-authority-r",
                Ok(&shown_app(101, r#"{"checks":"write","metadata":"read"}"#)),
            )
    }

    /// [`set_up_host`] with the authority App private, as the operator's
    /// live run found it, and the environment restricted by the earlier run.
    fn private_host() -> Host {
        set_up_host()
            .answer("api apps/codeflow-authority-r", Err(LIVE_404))
            .answer(
                "api repos/o/r/environments/codeflow-authority",
                Ok(&restricted_environment(&json!({}))),
            )
    }

    /// The environment as GitHub shows it once restricted, with `changes`.
    fn restricted_environment(changes: &Value) -> String {
        let mut environment = json!({
            "name": "codeflow-authority",
            "can_admins_bypass": false,
            "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true},
            "protection_rules": [{"type": "branch_policy"}],
        });
        for (key, value) in changes.as_object().unwrap() {
            environment[key] = value.clone();
        }
        environment.to_string()
    }

    /// A personal repository `o/r`, administrator `o`, no environment yet.
    fn fresh_host() -> Host {
        Host::new()
            .answer("api user", Ok(r#"{"login":"o","type":"User"}"#))
            .answer("repo view", Ok(r#"{"nameWithOwner":"o/r"}"#))
            .answer(
                "api repos/o/r",
                Ok(r#"{"default_branch":"main","private":false,"owner":{"login":"o","type":"User"},"permissions":{"admin":true}}"#),
            )
            .answer("api repos/o/r/environments/codeflow-authority", Err("gh: Not Found (HTTP 404)"))
            .answer("api -X PUT repos/o/r/environments/codeflow-authority", Ok("{}"))
            .answer(
                "api repos/o/r/environments/codeflow-authority/deployment-branch-policies",
                Ok(r#"{"total_count":0,"branch_policies":[]}"#),
            )
            .answer(
                "api -X POST repos/o/r/environments/codeflow-authority/deployment-branch-policies",
                Ok(r#"{"id":9,"name":"main","type":"branch"}"#),
            )
            .answer(
                "api repos/o/r/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID",
                Err("gh: Not Found (HTTP 404)"),
            )
            .answer(
                "api -X POST app-manifests/authoritycode/conversions",
                Ok(&conversion(
                    "codeflow-authority-r",
                    "codeflow-authority-r",
                    "o",
                    r#"{"checks":"write","metadata":"read"}"#,
                    AUTHORITY_PEM,
                )),
            )
            .answer(
                "api -X POST app-manifests/agentcode/conversions",
                Ok(&conversion(
                    "codeflow-agent-r",
                    "codeflow-agent-r",
                    "o",
                    r#"{"contents":"write","pull_requests":"write","metadata":"read"}"#,
                    AGENT_PEM,
                )),
            )
            .answer("secret set CODEFLOW_AUTHORITY_APP_KEY", Ok(""))
            .answer(
                "api -X POST repos/o/r/environments/codeflow-authority/variables",
                Ok("{}"),
            )
    }

    /// A browser that returns a code per App and keeps the manifests.
    #[derive(Default)]
    struct Clicks {
        manifests: Vec<(String, Value)>,
    }

    impl ManifestBrowser for Clicks {
        fn redirect_url(&self, _: &Target) -> String {
            "http://127.0.0.1:1/callback".to_string()
        }
        fn code(
            &mut self,
            kind: AppKind,
            post_url: &str,
            manifest: &Value,
            state: &str,
        ) -> Result<String, String> {
            assert_eq!(state.len(), 32);
            self.manifests
                .push((post_url.to_string(), manifest.clone()));
            Ok(format!("{}code", kind.label()))
        }
    }

    fn both() -> SetupOptions {
        SetupOptions {
            agent: true,
            ..SetupOptions::default()
        }
    }

    fn run(host: &Host, browser: &mut dyn ManifestBrowser, store: &dyn KeyStore) -> SetupReport {
        run_hook(host, browser, store, &HookHttp::repository_page())
    }

    fn run_hook(
        host: &Host,
        browser: &mut dyn ManifestBrowser,
        store: &dyn KeyStore,
        hook: &HookHttp,
    ) -> SetupReport {
        setup(
            &|args, input| host.gh(args, input),
            browser,
            store,
            &auth(&OkSigner, hook),
            &both(),
        )
    }

    #[test]
    fn agent_manifest_holds_exactly_contents_and_pull_requests_write() {
        let expected: BTreeMap<String, String> = [
            ("contents", "write"),
            ("metadata", "read"),
            ("pull_requests", "write"),
        ]
        .into_iter()
        .map(|(name, level)| (name.to_string(), level.to_string()))
        .collect();
        assert_eq!(expected_permissions(AppKind::Agent).unwrap(), expected);
        for absent in [
            "administration",
            "workflows",
            "environments",
            "deployments",
            "statuses",
            "checks",
            "actions",
            "secrets",
        ] {
            assert!(!expected.contains_key(absent), "{absent}");
        }
        let authority: BTreeMap<String, String> = [("checks", "write"), ("metadata", "read")]
            .into_iter()
            .map(|(name, level)| (name.to_string(), level.to_string()))
            .collect();
        assert_eq!(expected_permissions(AppKind::Authority).unwrap(), authority);
    }

    #[test]
    fn manifest_names_the_repository_and_never_takes_events() {
        for kind in [AppKind::Authority, AppKind::Agent] {
            let manifest = manifest(kind, "n", "o/r", "http://127.0.0.1:5/callback").unwrap();
            assert_eq!(manifest["name"], "n");
            assert_eq!(manifest["url"], "https://github.com/o/r");
            assert_eq!(manifest["redirect_url"], "http://127.0.0.1:5/callback");
            assert_eq!(manifest["public"], false);
            assert_eq!(manifest["hook_attributes"]["active"], false);
            assert_eq!(manifest["default_events"], json!([]));
        }
    }

    #[test]
    fn setup_creates_environment_secret_variable_and_both_apps() {
        let host = fresh_host();
        let store = MemoryStore::default();
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &store);
        assert_eq!(report.failure, None, "{}", report.render().into_string());
        assert_eq!(
            host.writes(),
            vec![
                "api -X PUT repos/o/r/environments/codeflow-authority --input -",
                "api -X POST repos/o/r/environments/codeflow-authority/deployment-branch-policies --input -",
                "api -X POST app-manifests/authoritycode/conversions",
                "secret set CODEFLOW_AUTHORITY_APP_KEY --env codeflow-authority --repo o/r",
                "api -X POST repos/o/r/environments/codeflow-authority/variables --input -",
                "api -X POST app-manifests/agentcode/conversions",
            ]
        );
        let calls = host.calls.borrow();
        let input = |prefix: &str| {
            calls
                .iter()
                .find(|(call, _)| call.starts_with(prefix))
                .and_then(|(_, input)| input.clone())
                .unwrap()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&input("api -X PUT")).unwrap(),
            json!({"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&input(
                "api -X POST repos/o/r/environments/codeflow-authority/deployment-branch-policies"
            ))
            .unwrap(),
            json!({"name": "main", "type": "branch"})
        );
        assert_eq!(input("secret set"), AUTHORITY_PEM);
        assert_eq!(
            serde_json::from_str::<Value>(&input(
                "api -X POST repos/o/r/environments/codeflow-authority/variables"
            ))
            .unwrap(),
            json!({"name": "CODEFLOW_AUTHORITY_APP_ID", "value": "101"})
        );
        // The manifests went to the personal account's page.
        assert_eq!(clicks.manifests.len(), 2);
        assert!(clicks
            .manifests
            .iter()
            .all(|(url, _)| url == "https://github.com/settings/apps/new"));
        assert_eq!(
            clicks.manifests[0].1["default_permissions"],
            json!({"checks": "write"})
        );
        // The agent key is in the store, never in the report.
        let entry: AgentApp = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
        assert_eq!(
            (entry.app_id, entry.slug.as_str(), entry.pem.as_str()),
            (202, "codeflow-agent-r", AGENT_PEM)
        );
        let shown = format!("{}\n{report:?}", report.render().into_string());
        assert!(
            shown.contains("https://github.com/apps/codeflow-authority-r/installations/new"),
            "{shown}"
        );
        assert!(
            shown.contains("https://github.com/apps/codeflow-agent-r/installations/new"),
            "{shown}"
        );
        assert!(
            !shown.contains("secretKEYmaterial") && !shown.contains("client-secret"),
            "{shown}"
        );
        // Every write is listed with its undo.
        assert_eq!(report.writes.len(), 7, "{shown}");
        assert!(report.writes.iter().all(|write| !write.undo.is_empty()));
        assert!(
            shown.contains("gh api -X DELETE repos/o/r/environments/codeflow-authority\n"),
            "{shown}"
        );
        assert!(
            shown.contains("https://github.com/settings/apps/codeflow-agent-r/advanced"),
            "{shown}"
        );
        assert!(shown.contains("forget o/r"), "{shown}");
    }

    #[test]
    fn organization_manifests_go_to_the_organization_page() {
        let host = fresh_host().answer(
            "api repos/o/r",
            Ok(r#"{"default_branch":"trunk","private":false,"owner":{"login":"o","type":"Organization"},"permissions":{"admin":true}}"#),
        );
        let host = host.answer(
            "api repos/o/r/environments/codeflow-authority/deployment-branch-policies",
            Ok(r#"{"total_count":1,"branch_policies":[{"id":3,"name":"trunk","type":"branch"}]}"#),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        assert_eq!(report.failure, None, "{}", report.render().into_string());
        assert_eq!(
            clicks.manifests[0].0,
            "https://github.com/organizations/o/settings/apps/new"
        );
        assert!(report.render().into_string().contains(
            "https://github.com/organizations/o/settings/apps/codeflow-authority-r/advanced"
        ));
        // The existing default-branch policy is kept, not written again.
        assert!(!host
            .writes()
            .iter()
            .any(|call| call.contains("deployment-branch-policies")));
    }

    #[test]
    fn refuses_an_app_installation_token_before_any_write() {
        let host = fresh_host().answer(
            "api user",
            Err("gh: Resource not accessible by integration (HTTP 403)"),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("GitHub App installation token"),
            "{failure}"
        );
        assert!(
            host.writes().is_empty() && clicks.manifests.is_empty() && report.writes.is_empty()
        );

        let bot = fresh_host().answer(
            "api user",
            Ok(r#"{"login":"codeflow-agent-r[bot]","type":"Bot"}"#),
        );
        let report = run(&bot, &mut Clicks::default(), &MemoryStore::default());
        assert!(report.failure.unwrap().contains("a GitHub App or bot"));
        assert!(bot.writes().is_empty());
    }

    #[test]
    fn refuses_a_token_without_administration_write_naming_it() {
        // Not the repository's administrator.
        let host = fresh_host().answer(
            "api repos/o/r",
            Ok(r#"{"default_branch":"main","private":false,"owner":{"login":"o","type":"User"},"permissions":{"admin":false,"push":true}}"#),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("needs Administration write on o/r"),
            "{failure}"
        );
        assert!(host.writes().is_empty() && clicks.manifests.is_empty());

        // An administrator's token narrowed to no Administration permission:
        // the host refuses the first write, and no App is created.
        let host = fresh_host().answer(
            "api -X PUT repos/o/r/environments/codeflow-authority",
            Err("gh: Resource not accessible by personal access token (HTTP 403)"),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("needs Administration write on o/r, which this token lacks"),
            "{failure}"
        );
        assert!(clicks.manifests.is_empty() && report.writes.is_empty());
    }

    #[test]
    fn refuses_an_environment_that_admits_another_branch() {
        let host = fresh_host().answer(
            "api repos/o/r/environments/codeflow-authority/deployment-branch-policies",
            Ok(r#"{"total_count":2,"branch_policies":[{"id":3,"name":"main","type":"branch"},{"id":4,"name":"integration/*","type":"branch"}]}"#),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("'integration/*' (id 4)")
                && failure.contains("not restricted to main"),
            "{failure}"
        );
        assert!(clicks.manifests.is_empty());
    }

    #[test]
    fn an_existing_environment_keeps_its_reviewers_and_timer() {
        let existing = json!({
            "name": "codeflow-authority",
            "deployment_branch_policy": null,
            "protection_rules": [
                {"type": "wait_timer", "wait_timer": 5},
                {"type": "required_reviewers", "prevent_self_review": true,
                 "reviewers": [{"type": "User", "reviewer": {"id": 42, "login": "o"}}]},
                {"type": "branch_policy"}
            ]
        });
        assert_eq!(
            environment_body(Some(&existing)),
            json!({
                "deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true},
                "wait_timer": 5,
                "prevent_self_review": true,
                "reviewers": [{"type": "User", "id": 42}]
            })
        );
        let host = fresh_host().answer(
            "api repos/o/r/environments/codeflow-authority",
            Ok(&existing.to_string()),
        );
        let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
        assert_eq!(report.failure, None);
        assert!(
            report.writes[0].what.contains("(was null)"),
            "{:?}",
            report.writes[0]
        );
    }

    #[test]
    fn an_app_that_differs_from_the_manifest_is_reported_and_its_key_dropped() {
        let host = fresh_host().answer(
            "api -X POST app-manifests/authoritycode/conversions",
            Ok(&conversion(
                "codeflow-authority-r",
                "codeflow-authority-r",
                "someone-else",
                r#"{"checks":"write","metadata":"read","administration":"write"}"#,
                AUTHORITY_PEM,
            )),
        );
        let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("owner 'someone-else'") && failure.contains("administration"),
            "{failure}"
        );
        assert!(!failure.contains("secretKEYmaterial"));
        assert!(!host.writes().iter().any(|call| call.starts_with("secret")));
        let last = report.writes.last().unwrap();
        assert!(last.what.contains("created the authority GitHub App"));
        assert!(last
            .undo
            .contains("https://github.com/settings/apps/codeflow-authority-r/advanced"));
    }

    /// More branch policies than one page holds are refused, never read
    /// as "restricted to the default branch" (review nit 2).
    #[test]
    fn a_second_page_of_branch_policies_is_refused() {
        let host = fresh_host().answer(
            "api repos/o/r/environments/codeflow-authority/deployment-branch-policies",
            Ok(r#"{"total_count":101,"branch_policies":[{"id":3,"name":"main","type":"branch"}]}"#),
        );
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("more deployment branch policies"),
            "{failure}"
        );
        assert!(clicks.manifests.is_empty());
    }

    /// F3: a second run calls the Apps set up only when the secret exists
    /// and each App GitHub shows has the id kept, the manifest's
    /// permissions and no events.
    #[test]
    fn a_second_run_creates_nothing_once_everything_is_in_place() {
        let store = MemoryStore::default();
        store.store("o/r", &agent_entry()).unwrap();
        let host = set_up_host();
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &store);
        assert_eq!(report.failure, None, "{}", report.render().into_string());
        assert!(clicks.manifests.is_empty());
        assert!(host
            .writes()
            .iter()
            .all(|call| !call.contains("app-manifests")));
        let shown = report.render().into_string();
        assert!(
            shown.contains("authority App: already set up")
                && shown.contains("the secret CODEFLOW_AUTHORITY_APP_KEY")
                && shown.contains("agent App: already set up"),
            "{shown}"
        );
    }

    /// F3: the id variable alone is not "already set up". A missing secret,
    /// or an App with other permissions, stops the run non-zero naming what
    /// is present and what is missing.
    #[test]
    fn a_partial_authority_setup_is_reported_not_called_set_up() {
        let cases = [
            (
                set_up_host().answer(
                    "api repos/o/r/environments/codeflow-authority/secrets/CODEFLOW_AUTHORITY_APP_KEY",
                    Err("gh: Not Found (HTTP 404)"),
                ),
                "missing: the secret CODEFLOW_AUTHORITY_APP_KEY",
            ),
            (
                set_up_host().answer(
                    "api apps/codeflow-authority-r",
                    Ok(&shown_app(101, r#"{"checks":"write","administration":"write","metadata":"read"}"#)),
                ),
                "with permissions",
            ),
            (
                set_up_host().answer(
                    "api apps/codeflow-authority-r",
                    Ok(&shown_app(999, r#"{"checks":"write","metadata":"read"}"#)),
                ),
                "GitHub shows 999",
            ),
        ];
        for (host, missing) in cases {
            let store = MemoryStore::default();
            store.store("o/r", &agent_entry()).unwrap();
            let mut clicks = Clicks::default();
            let report = run(&host, &mut clicks, &store);
            let failure = report.failure.clone().unwrap();
            assert!(
                failure.contains("the authority App is not fully set up")
                    && failure.contains("present: CODEFLOW_AUTHORITY_APP_ID '101'")
                    && failure.contains(missing),
                "{failure}"
            );
            assert!(!report.render().into_string().contains("already set up"));
            assert!(clicks.manifests.is_empty());
        }
    }

    /// F3: a credential-store entry that is not an agent App with a key, or
    /// an App GitHub shows with other events, is not "already set up".
    #[test]
    fn a_partial_agent_setup_is_reported_not_called_set_up() {
        let unreadable = MemoryStore::default();
        unreadable.store("o/r", "{}").unwrap();
        let report = run(&set_up_host(), &mut Clicks::default(), &unreadable);
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("the agent App is not fully set up")
                && failure.contains("missing: an agent App and key"),
            "{failure}"
        );
        let store = MemoryStore::default();
        store.store("o/r", &agent_entry()).unwrap();
        let hook = HookHttp::app(Ok(agent_answer(&json!({"events": ["push"]}))));
        let report = run_hook(&set_up_host(), &mut Clicks::default(), &store, &hook);
        let failure = report.failure.unwrap();
        assert!(failure.contains(r#"events ["push"]"#), "{failure}");
        assert!(!failure.contains("secretKEYmaterial"), "{failure}");
    }

    /// The operator's live defect: a private authority App answered 404 and
    /// the run stopped before the agent App. It now goes on, says what it
    /// checked, and never says "already set up" for an App nobody read.
    #[test]
    fn a_private_authority_app_does_not_stop_the_agent_app() {
        let host = private_host();
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &MemoryStore::default());
        let shown = report.render().into_string();
        assert_eq!(report.failure, None, "{shown}");
        assert_eq!(clicks.manifests.len(), 1, "{shown}");
        assert_eq!(
            clicks.manifests[0].1["default_permissions"],
            json!({"contents": "write", "pull_requests": "write"})
        );
        for line in [
            "authority App: present but not verified, nothing created\n",
            "  checked: the variable CODEFLOW_AUTHORITY_APP_ID (101) and the secret CODEFLOW_AUTHORITY_APP_KEY (its name only; the value cannot be read)\n",
            "  not checked: the App codeflow-authority-r itself. GitHub's API answered 404, and it answers 404 both for a private App this login cannot read and for a deleted App, so this run cannot tell them apart\n",
            "  check it yourself: https://github.com/settings/apps/codeflow-authority-r should show App ID 101, and https://github.com/settings/apps/codeflow-authority-r/permissions should show only checks write and metadata read, with no events\n",
            "  to set it up again, delete that App, then `gh api -X DELETE repos/o/r/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID` and `gh secret delete CODEFLOW_AUTHORITY_APP_KEY --env codeflow-authority --repo o/r`, and run again\n",
        ] {
            assert!(shown.contains(line), "{line}\n{shown}");
        }
        assert!(!shown.contains("authority App: already set up"), "{shown}");
    }

    /// An organization's pages are under the organization.
    #[test]
    fn a_private_organization_app_names_the_organization_pages() {
        let host = private_host().answer(
            "api repos/o/r",
            Ok(r#"{"default_branch":"main","private":false,"owner":{"login":"o","type":"Organization"},"permissions":{"admin":true}}"#),
        );
        let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
        let shown = report.render().into_string();
        assert!(
            shown.contains("https://github.com/organizations/o/settings/apps/codeflow-authority-r should show")
                && shown.contains("https://github.com/organizations/o/settings/apps/codeflow-authority-r/permissions should show"),
            "{shown}"
        );
    }

    /// The whole rerun on a private App: the agent App is read with its own
    /// key, and `gh` is never asked for it.
    #[test]
    fn a_rerun_verifies_the_agent_app_with_its_own_key() {
        let store = MemoryStore::default();
        store.store("o/r", &agent_entry()).unwrap();
        let host = private_host().answer("api apps/codeflow-agent-r", Err(LIVE_404));
        let hook = HookHttp::repository_page();
        let mut clicks = Clicks::default();
        let report = run_hook(&host, &mut clicks, &store, &hook);
        let shown = report.render().into_string();
        assert_eq!(report.failure, None, "{shown}");
        assert!(clicks.manifests.is_empty());
        assert!(
            shown.contains("agent App: already set up")
                && shown.contains("read as the App itself through GET /app"),
            "{shown}"
        );
        assert!(!host
            .calls
            .borrow()
            .iter()
            .any(|(call, _)| call == "api apps/codeflow-agent-r"));
        let calls = hook.calls.borrow();
        let (_, bearer) = calls.iter().find(|(path, _)| path == "/app").unwrap();
        assert_eq!(bearer.split('.').count(), 3, "a JSON Web Token");
        // The public bot user is read with no credentials.
        let (_, public) = calls.iter().find(|(path, _)| path != "/app").unwrap();
        assert_eq!(public, "");
    }

    /// With the App unreadable, the variable and the secret are all that is
    /// left to check, so each one still stops the run.
    #[test]
    fn a_private_app_with_a_bad_variable_or_missing_secret_stops() {
        let variable =
            "api repos/o/r/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID";
        let secret =
            "api repos/o/r/environments/codeflow-authority/secrets/CODEFLOW_AUTHORITY_APP_KEY";
        let cases = [
            (
                private_host().answer(variable, Ok(r#"{"value":""}"#)),
                "CODEFLOW_AUTHORITY_APP_ID",
            ),
            (
                private_host().answer(variable, Ok(r#"{"value":"0"}"#)),
                "CODEFLOW_AUTHORITY_APP_ID",
            ),
            (
                private_host().answer(variable, Ok(r#"{"value":"not-a-number"}"#)),
                "CODEFLOW_AUTHORITY_APP_ID",
            ),
            (
                private_host().answer(secret, Err("gh: Not Found (HTTP 404)")),
                "missing: the secret CODEFLOW_AUTHORITY_APP_KEY",
            ),
            (
                private_host().answer(secret, Err("gh: Server Error (HTTP 500)")),
                "the secret CODEFLOW_AUTHORITY_APP_KEY (cannot read it",
            ),
        ];
        for (host, named) in cases {
            let mut clicks = Clicks::default();
            let report = run(&host, &mut clicks, &MemoryStore::default());
            let shown = report.render().into_string();
            let failure = report.failure.unwrap();
            assert!(
                failure.contains("the authority App is not fully set up")
                    && failure.contains("missing: ")
                    && failure.contains(named),
                "{failure}"
            );
            assert!(!shown.contains("not verified"), "{shown}");
            assert!(!shown.contains("already set up"), "{shown}");
            assert!(clicks.manifests.is_empty());
        }
    }

    /// Only a 404 status leads to "carry on". Everything else stops, also a
    /// failure whose body happens to hold "404".
    #[test]
    fn only_a_real_404_lets_the_run_continue() {
        for answer in [
            Err("gh: Bad credentials (HTTP 401)"),
            Err("gh: Forbidden (HTTP 403)"),
            Err("gh: API rate limit exceeded (HTTP 429)"),
            Err("gh: Bad Gateway (HTTP 502)"),
            Err(r#"{"message":"upstream said 404"}gh: Bad Gateway (HTTP 502)"#),
            Err(r#"{"message":"upstream said (HTTP 404)"}gh: Bad Gateway (HTTP 502)"#),
            Err("Not Found: dial tcp: i/o timeout"),
            Ok("<html>not json</html>"),
        ] {
            let host = private_host().answer("api apps/codeflow-authority-r", answer);
            let mut clicks = Clicks::default();
            let report = run(&host, &mut clicks, &MemoryStore::default());
            let shown = report.render().into_string();
            let failure = report
                .failure
                .unwrap_or_else(|| panic!("{answer:?}\n{shown}"));
            assert!(
                failure.contains("the authority App is not fully set up"),
                "{failure}"
            );
            assert!(!shown.contains("not verified"), "{answer:?}\n{shown}");
            assert!(clicks.manifests.is_empty(), "{answer:?}");
        }
        // gh may add a line after its status, such as a scope hint.
        let host = private_host().answer(
            "api apps/codeflow-authority-r",
            Err("gh: Not Found (HTTP 404)\nThis API operation needs a scope"),
        );
        let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
        assert_eq!(report.failure, None);
    }

    /// Failures that name 404 or Not Found without being one.
    const NOT_A_404: [&str; 3] = [
        r#"{"message":"upstream said 404"}gh: Bad Gateway (HTTP 502)"#,
        r#"{"message":"Not Found"}gh: Bad Gateway (HTTP 502)"#,
        "Not Found: dial tcp: i/o timeout",
    ];

    /// Reading an environment as absent writes it and drops
    /// `can_admins_bypass`, so only a real 404 may do that.
    #[test]
    fn only_a_real_404_means_no_environment() {
        for answer in NOT_A_404 {
            let host =
                fresh_host().answer("api repos/o/r/environments/codeflow-authority", Err(answer));
            let mut clicks = Clicks::default();
            let report = run(&host, &mut clicks, &MemoryStore::default());
            let failure = report.failure.unwrap_or_else(|| panic!("{answer}"));
            assert!(failure.contains("cannot read the environment"), "{failure}");
            assert!(host.writes().is_empty(), "{answer}: {:?}", host.writes());
            assert!(clicks.manifests.is_empty(), "{answer}");
        }
    }

    /// Reading the id variable as absent creates an App and replaces the key
    /// secret, so only a real 404 may do that.
    #[test]
    fn only_a_real_404_means_no_id_variable() {
        for answer in NOT_A_404 {
            let host = fresh_host().answer(
                "api repos/o/r/environments/codeflow-authority/variables/CODEFLOW_AUTHORITY_APP_ID",
                Err(answer),
            );
            let mut clicks = Clicks::default();
            let report = run(&host, &mut clicks, &MemoryStore::default());
            let failure = report.failure.unwrap_or_else(|| panic!("{answer}"));
            assert!(
                failure.contains("cannot read CODEFLOW_AUTHORITY_APP_ID"),
                "{failure}"
            );
            assert!(clicks.manifests.is_empty(), "{answer}");
            assert!(
                !host.writes().iter().any(|call| call.starts_with("secret")),
                "{answer}"
            );
        }
    }

    /// A secret read that fails is reported as unread, not as a missing one.
    #[test]
    fn only_a_real_404_means_no_key_secret() {
        for answer in NOT_A_404 {
            let host = private_host().answer(
                "api repos/o/r/environments/codeflow-authority/secrets/CODEFLOW_AUTHORITY_APP_KEY",
                Err(answer),
            );
            let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
            let failure = report.failure.unwrap_or_else(|| panic!("{answer}"));
            assert!(failure.contains("(cannot read it"), "{failure}");
        }
    }

    /// `GET /app/hook/config` goes through curl, whose failure ends with its
    /// own status line, so that line decides, whatever the body says.
    #[test]
    fn only_curls_own_404_means_no_webhook() {
        let failed = |body: &str, status: &str| {
            Err(format!(
                "GET https://api.github.com/app/hook/config failed: {body}curl: (22) The requested URL returned error: {status}"
            ))
        };
        for hook in [
            failed(r#"{"message":"upstream said 404"}"#, "502"),
            failed(r#"{"message":"Not Found"}"#, "502"),
            failed(r#"{"message":"returned error: 404"}"#, "502"),
            failed("", "4040"),
            Err("GET failed: curl: (22) The requested URL returned error: 404curl: (28) Operation timed out".to_string()),
            Err("GET failed: curl: (7) Not Found: dial tcp".to_string()),
            Err("GET failed: curl: (28) Operation timed out".to_string()),
        ] {
            let host = fresh_host();
            let shown = format!("{hook:?}");
            let report = run_hook(
                &host,
                &mut Clicks::default(),
                &MemoryStore::default(),
                &HookHttp::hook(hook),
            );
            let failure = report.failure.unwrap_or_else(|| panic!("{shown}"));
            assert!(
                failure.contains("a webhook that can be checked"),
                "{failure}"
            );
            assert!(!host.writes().iter().any(|call| call.starts_with("secret")));
        }
        for status in ["404", "404 Not Found"] {
            let report = run_hook(
                &fresh_host(),
                &mut Clicks::default(),
                &MemoryStore::default(),
                &HookHttp::hook(failed(r#"{"message":"Not Found"}"#, status)),
            );
            assert_eq!(report.failure, None, "{status}");
        }
    }

    /// A locked keyring or a failed store call is not "no entry": no second
    /// agent App is created and nothing is stored.
    #[test]
    fn a_store_that_cannot_answer_creates_no_agent_app() {
        let host = fresh_host();
        let mut clicks = Clicks::default();
        let report = run(&host, &mut clicks, &UnclearStore);
        let failure = report.failure.unwrap();
        assert!(failure.contains("did not answer"), "{failure}");
        assert!(
            !host.writes().iter().any(|call| call.contains("agentcode")),
            "{:?}",
            host.writes()
        );
        assert!(!clicks
            .manifests
            .iter()
            .any(|(_, manifest)| manifest["name"].as_str().unwrap_or("").contains("agent")));
    }

    /// The agent App's own answer decides: a refused key, a deleted App, a
    /// failed read or an answer that differs is never "already set up", and
    /// the text holds neither the key nor the token, whatever the transport
    /// echoes.
    #[test]
    fn an_agent_app_that_fails_its_self_check_stops() {
        let echo = |what: &str| Err(format!("GET failed: {what} key {AGENT_PEM}"));
        let cases = [
            (HookHttp::app(echo("401")), "answering as itself"),
            (HookHttp::app(echo("404")), "answering as itself"),
            (HookHttp::app(echo("timed out")), "answering as itself"),
            (HookHttp::app(echo("502")), "run again"),
            (HookHttp::app(Ok("not json".to_string())), "not JSON"),
            (
                HookHttp::app(Ok(agent_answer(&json!({"id": 999})))),
                "GitHub shows 999",
            ),
            (
                HookHttp::app(Ok(agent_answer(&json!({"slug": "another-app"})))),
                "slug 'another-app'",
            ),
            (
                HookHttp::app(Ok(agent_answer(&json!({"owner": {"login": "someone"}})))),
                "owner 'someone'",
            ),
            (
                HookHttp::app(Ok(agent_answer(
                    &json!({"permissions": {"contents": "write", "administration": "write"}}),
                ))),
                "with permissions",
            ),
            (
                HookHttp::app(Ok(agent_answer(&json!({"events": ["push"]})))),
                r#"events ["push"]"#,
            ),
        ];
        for (hook, named) in cases {
            let store = MemoryStore::default();
            store.store("o/r", &agent_entry()).unwrap();
            let report = run_hook(&set_up_host(), &mut Clicks::default(), &store, &hook);
            let shown = report.render().into_string();
            let failure = report.failure.unwrap();
            assert!(
                failure.contains("the agent App is not fully set up") && failure.contains(named),
                "{named}: {failure}"
            );
            assert!(!shown.contains("agent App: already set up"), "{shown}");
            assert!(
                !failure.contains("secretKEYmaterial") && !failure.contains("PRIVATE KEY"),
                "{failure}"
            );
        }
    }

    /// The operator's log showed `writes: none` beside a PUT: a second run
    /// wrote to the environment each time, and could reset its bypass.
    #[test]
    fn a_second_run_writes_nothing_to_a_restricted_environment() {
        let store = MemoryStore::default();
        store.store("o/r", &agent_entry()).unwrap();
        let host = private_host();
        let report = run(&host, &mut Clicks::default(), &store);
        let shown = report.render().into_string();
        assert_eq!(report.failure, None, "{shown}");
        assert_eq!(host.writes(), Vec::<String>::new());
        assert!(report.writes.is_empty(), "{shown}");
        assert!(
            shown.contains("already deploys from selected branches only"),
            "{shown}"
        );
    }

    /// A write that is needed carries the bypass value GitHub showed, so it
    /// cannot turn the administrator bypass back on or off.
    #[test]
    fn a_needed_environment_write_keeps_can_admins_bypass() {
        for bypass in [false, true] {
            let existing = json!({
                "can_admins_bypass": bypass,
                "deployment_branch_policy": null,
                "protection_rules": [],
            });
            let host = fresh_host().answer(
                "api repos/o/r/environments/codeflow-authority",
                Ok(&existing.to_string()),
            );
            let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
            assert_eq!(report.failure, None);
            let calls = host.calls.borrow();
            let (_, body) = calls
                .iter()
                .find(|(call, _)| call.starts_with("api -X PUT"))
                .unwrap();
            let body: Value = serde_json::from_str(body.as_ref().unwrap()).unwrap();
            assert_eq!(body["can_admins_bypass"], json!(bypass));
        }
    }

    /// The lines a project pastes into its harness settings, exact, on a
    /// first run and on a verified rerun; the JSON and TOML forms hold the
    /// values unchanged.
    #[test]
    fn setup_prints_committer_lines_for_each_harness() {
        let name = "codeflow-agent-r[bot]";
        let email = "353475153+codeflow-agent-r[bot]@users.noreply.github.com";
        let expected = format!(
            "committer: {name} <{email}>\nadd these lines to your project settings and commit them; this command edits no settings file, and `codeflow baseline approve` admits the change\n\nClaude Code, inside \"env\" in .claude/settings.json:\n  \"GIT_COMMITTER_NAME\": \"{name}\",\n  \"GIT_COMMITTER_EMAIL\": \"{email}\"\n\nCodex, at the end of .codex/config.toml:\n  [shell_environment_policy.set]\n  GIT_COMMITTER_NAME = \"{name}\"\n  GIT_COMMITTER_EMAIL = \"{email}\"\n\nGrok Build has no project environment setting; set these in the shell that launches it:\n  export GIT_COMMITTER_NAME='{name}'\n  export GIT_COMMITTER_EMAIL='{email}'\n"
        );
        let first = run(
            &fresh_host(),
            &mut Clicks::default(),
            &MemoryStore::default(),
        );
        let shown = first.render().into_string();
        assert!(shown.contains(&expected), "{shown}");
        let store = MemoryStore::default();
        store.store("o/r", &agent_entry()).unwrap();
        let again = run(&private_host(), &mut Clicks::default(), &store);
        let rerun = again.render().into_string();
        assert!(rerun.contains(&expected), "{rerun}");

        let between = |from: &str, to: &str| {
            let start = shown.find(from).unwrap() + from.len();
            shown[start..start + shown[start..].find(to).unwrap()].to_string()
        };
        let claude: Value = serde_json::from_str(&format!(
            "{{{}}}",
            between(".claude/settings.json:\n", "\n\nCodex")
        ))
        .unwrap();
        assert_eq!(claude["GIT_COMMITTER_NAME"], name);
        assert_eq!(claude["GIT_COMMITTER_EMAIL"], email);
        let codex: toml::Value = between(".codex/config.toml:\n", "\n\nGrok")
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n")
            .parse()
            .unwrap();
        let set = &codex["shell_environment_policy"]["set"];
        assert_eq!(set["GIT_COMMITTER_NAME"].as_str(), Some(name));
        assert_eq!(set["GIT_COMMITTER_EMAIL"].as_str(), Some(email));
    }

    /// The lines are printed, never written: the two settings files are
    /// byte-identical after a run.
    #[test]
    fn setup_edits_no_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude/settings.json");
        let codex = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&claude, "{\"env\":{}}\n").unwrap();
        std::fs::write(&codex, "[shell_environment_policy]\n").unwrap();
        let before = (
            std::fs::read(&claude).unwrap(),
            std::fs::read(&codex).unwrap(),
        );
        let host = fresh_host();
        let report = run(&host, &mut Clicks::default(), &MemoryStore::default());
        assert_eq!(report.failure, None);
        assert!(report.render().into_string().contains("GIT_COMMITTER_NAME"));
        assert_eq!(
            before,
            (
                std::fs::read(&claude).unwrap(),
                std::fs::read(&codex).unwrap()
            )
        );
        assert!(host
            .calls
            .borrow()
            .iter()
            .all(|(call, _)| !call.contains("settings.json") && !call.contains("config.toml")));
    }

    /// GitHub may not show the bot user yet. The run neither guesses the
    /// values nor stops, and a later run prints them.
    #[test]
    fn an_unreadable_bot_user_prints_no_lines_and_does_not_stop() {
        for bot in [
            Err("GET failed: 404".to_string()),
            Ok(r#"{"login":"someone","id":5,"type":"User"}"#.to_string()),
        ] {
            let report = run_hook(
                &fresh_host(),
                &mut Clicks::default(),
                &MemoryStore::default(),
                &HookHttp::bot(bot),
            );
            let shown = report.render().into_string();
            assert_eq!(report.failure, None, "{shown}");
            assert!(
                shown.contains("committer lines: not printed, because GitHub does not show codeflow-agent-r[bot] yet; install the App and run `codeflow remote setup-apps` again"),
                "{shown}"
            );
            assert!(!shown.contains("GIT_COMMITTER"), "{shown}");
        }
    }

    /// A failure that says nothing about the bot user is not reported as a
    /// bot user GitHub does not show, and no one is told to install the App.
    #[test]
    fn a_bot_user_github_did_not_answer_for_is_not_called_missing() {
        for (error, named) in [
            (
                "GET failed: curl: (6) Could not resolve host: api.github.com",
                "cannot reach GitHub",
            ),
            (
                "GET failed: curl: (22) The requested URL returned error: 502",
                "GitHub failed",
            ),
        ] {
            let report = run_hook(
                &fresh_host(),
                &mut Clicks::default(),
                &MemoryStore::default(),
                &HookHttp::bot(Err(error.to_string())),
            );
            let shown = report.render().into_string();
            assert_eq!(report.failure, None, "{shown}");
            assert!(shown.contains("committer lines: not printed"), "{shown}");
            assert!(shown.contains(named), "{shown}");
            assert!(!shown.contains("does not show"), "{shown}");
            assert!(!shown.contains("install the App and run"), "{shown}");
        }
    }

    /// F4: a created App with the expected permissions but events, an
    /// active webhook, or a webhook to another server is refused: its key
    /// is dropped, the run stops non-zero and the delete page is named.
    #[test]
    fn an_app_with_events_or_a_foreign_webhook_is_refused() {
        let with = |extra: Value| {
            let mut answer: Value = serde_json::from_str(&conversion(
                "codeflow-authority-r",
                "codeflow-authority-r",
                "o",
                r#"{"checks":"write","metadata":"read"}"#,
                AUTHORITY_PEM,
            ))
            .unwrap();
            for (key, value) in extra.as_object().unwrap() {
                answer[key] = value.clone();
            }
            fresh_host().answer(
                "api -X POST app-manifests/authoritycode/conversions",
                Ok(&answer.to_string()),
            )
        };
        let cases = [
            (
                with(json!({"events": ["push"]})),
                HookHttp::repository_page(),
                "events [\"push\"]",
            ),
            (
                with(
                    json!({"hook_attributes": {"url": "https://evil.example/hook", "active": true}}),
                ),
                HookHttp::repository_page(),
                "an active webhook",
            ),
            (
                fresh_host(),
                HookHttp::hook(Ok(r#"{"url":"https://evil.example/hook"}"#.to_string())),
                "a webhook to 'https://evil.example/hook'",
            ),
            (
                fresh_host(),
                HookHttp::hook(Err(format!("GET failed: 500 {AUTHORITY_PEM}"))),
                "a webhook that can be checked",
            ),
        ];
        for (host, hook, why) in cases {
            let report = run_hook(
                &host,
                &mut Clicks::default(),
                &MemoryStore::default(),
                &hook,
            );
            let failure = report.failure.unwrap();
            assert!(
                failure.contains(why)
                    && failure.contains("its key was dropped")
                    && failure
                        .contains("https://github.com/settings/apps/codeflow-authority-r/advanced"),
                "{failure}"
            );
            assert!(!failure.contains("secretKEYmaterial"), "{failure}");
            assert!(!host.writes().iter().any(|call| call.starts_with("secret")));
        }
    }

    /// A store that refuses and echoes what it was given, as a helper that
    /// prints its standard input would.
    struct EchoStore;
    impl KeyStore for EchoStore {
        fn describe(&self) -> String {
            "an echoing store".to_string()
        }
        fn available(&self) -> Result<(), String> {
            Ok(())
        }
        fn exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn store(&self, _: &str, secret: &str) -> Result<(), String> {
            Err(format!("refused: {secret}"))
        }
        fn load(&self, _: &str) -> Result<String, String> {
            Err("none".to_string())
        }
        fn undo(&self, account: &str) -> String {
            format!("forget {account}")
        }
    }

    /// When the store refuses after the agent App is created, the run stops
    /// non-zero, the key is not reported kept, and a helper's echo of the
    /// key is redacted; so is an echo from `gh secret set` (review nit 1).
    #[test]
    fn a_refused_key_write_stops_the_run_and_never_shows_the_key() {
        let refusing = MemoryStore {
            refuse: true,
            ..MemoryStore::default()
        };
        let report = run(&fresh_host(), &mut Clicks::default(), &refusing);
        let failure = report.failure.unwrap();
        assert!(
            failure.contains("did not keep its key") && failure.contains("the key is not kept"),
            "{failure}"
        );
        assert!(!report
            .writes
            .iter()
            .any(|write| write.what.contains("kept the agent App")));
        let report = run(&fresh_host(), &mut Clicks::default(), &EchoStore);
        let failure = report.failure.unwrap();
        assert!(
            !failure.contains("secretKEYmaterial") && !failure.contains("PRIVATE KEY"),
            "{failure}"
        );
        let echo = format!("gh: echoed {AUTHORITY_PEM}");
        let host = fresh_host().answer("secret set CODEFLOW_AUTHORITY_APP_KEY", Err(&echo));
        let failure = run(&host, &mut Clicks::default(), &MemoryStore::default())
            .failure
            .unwrap();
        assert!(
            failure.contains("its key is not kept")
                && !failure.contains("secretKEYmaterial")
                && !failure.contains("PRIVATE KEY"),
            "{failure}"
        );
    }

    #[test]
    fn app_names_are_checked_before_anything_is_written() {
        let host = fresh_host();
        let options = SetupOptions {
            agent: true,
            authority_name: Some("x".repeat(35)),
            agent_name: None,
        };
        let report = setup(
            &|args, input| host.gh(args, input),
            &mut Clicks::default(),
            &MemoryStore::default(),
            &auth(&OkSigner, &HookHttp::repository_page()),
            &options,
        );
        assert!(report.failure.unwrap().contains("--authority-name"));
        assert!(host.writes().is_empty());
    }

    /// The paste route through a real temporary page: after a run that
    /// succeeds and after one whose key write fails, nothing is left in
    /// the scratch folder and no file anywhere under it held a key.
    #[test]
    fn no_key_or_page_file_is_left_after_success_or_a_failed_write() {
        for fail_secret in [false, true] {
            let scratch = tempfile::tempdir().unwrap();
            let host = if fail_secret {
                fresh_host().answer("secret set CODEFLOW_AUTHORITY_APP_KEY", Err("gh: HTTP 500"))
            } else {
                fresh_host()
            };
            let pages: std::rc::Rc<RefCell<Vec<PathBuf>>> = std::rc::Rc::default();
            let seen = pages.clone();
            let root = scratch.path().to_path_buf();
            let mut browser = PasteBrowser::new(
                scratch.path(),
                Box::new(|_| {}),
                Box::new(move || {
                    // The administrator's paste: read the page to find the
                    // state, and return GitHub's redirect address.
                    let page = std::fs::read_dir(&root)
                        .unwrap()
                        .flat_map(|dir| std::fs::read_dir(dir.unwrap().path()).unwrap())
                        .map(|entry| entry.unwrap().path())
                        .next()
                        .unwrap();
                    let html = std::fs::read_to_string(&page).unwrap();
                    seen.borrow_mut().push(page.clone());
                    let state = html
                        .split("state=")
                        .nth(1)
                        .unwrap()
                        .split('"')
                        .next()
                        .unwrap()
                        .to_string();
                    let kind = if page.ends_with("agent-app.html") {
                        "agent"
                    } else {
                        "authority"
                    };
                    Ok(format!(
                        "https://github.com/o/r?code={kind}code&state={state}\n"
                    ))
                }),
            )
            .unwrap();
            let store = MemoryStore::default();
            let report = run(&host, &mut browser, &store);
            assert_eq!(
                report.failure.is_some(),
                fail_secret,
                "{:?}",
                report.failure
            );
            if fail_secret {
                let failure = report.failure.unwrap();
                assert!(
                    failure.contains("its key is not kept, so delete the App"),
                    "{failure}"
                );
                assert!(!failure.contains("secretKEYmaterial"));
            } else {
                assert_eq!(pages.borrow().len(), 2);
            }
            assert!(pages.borrow().iter().all(|page| !page.exists()));
            drop(browser);
            assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
        }
    }

    /// AC-4: a run in a repository changes no git identity, commit template
    /// or trailer setting, and leaves `git.ai_attribution` as it was: the
    /// repository's config and policy files are byte-identical afterwards,
    /// and no `gh` call touches git configuration.
    #[test]
    fn a_run_leaves_git_config_and_ai_attribution_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Human Author").unwrap();
            config.set_str("user.email", "human@example.com").unwrap();
            config.set_str("commit.template", ".gitmessage").unwrap();
            config
                .set_str("trailer.ifexists", "addIfDifferent")
                .unwrap();
        }
        let policy = dir.path().join(".codeflow/policy.json");
        std::fs::create_dir_all(policy.parent().unwrap()).unwrap();
        std::fs::write(
            &policy,
            r#"{ "schema_version": 1, "git": { "ai_attribution": "forbid" } }"#,
        )
        .unwrap();
        let config_path = dir.path().join(".git/config");
        let before = (
            std::fs::read(&config_path).unwrap(),
            std::fs::read(&policy).unwrap(),
        );
        let host = fresh_host();
        let report = setup(
            &|args, input| host.gh(args, input),
            &mut Clicks::default(),
            &MemoryStore::default(),
            &auth(&OkSigner, &HookHttp::repository_page()),
            &both(),
        );
        assert_eq!(report.failure, None);
        let after = (
            std::fs::read(&config_path).unwrap(),
            std::fs::read(&policy).unwrap(),
        );
        assert_eq!(before, after);
        assert!(host
            .calls
            .borrow()
            .iter()
            .all(|(call, _)| !call.contains("config") && !call.contains("trailer")));
    }

    #[test]
    fn callback_code_checks_the_state() {
        assert_eq!(
            callback_code("/callback?code=abc_1-2&state=s1", "s1").unwrap(),
            "abc_1-2"
        );
        assert_eq!(
            callback_code("https://github.com/o/r?state=s1&code=abc#x", "s1").unwrap(),
            "abc"
        );
        assert!(callback_code("/callback?code=abc&state=other", "s1").is_err());
        assert!(callback_code("/callback?code=abc", "s1").is_err());
        assert!(callback_code("/callback?code=../x&state=s1", "s1").is_err());
    }

    #[test]
    fn manifest_page_escapes_the_manifest() {
        let manifest = json!({"name": "a\"><script>"});
        let page = manifest_page(
            AppKind::Agent,
            "https://github.com/settings/apps/new",
            &manifest,
            "s1",
        );
        assert!(page.contains("action=\"https://github.com/settings/apps/new?state=s1\""));
        assert!(!page.contains("\"><script>\"") && page.contains("&quot;&gt;&lt;script&gt;"));
    }

    /// The loopback route end to end on a real port: the page is served
    /// only at its random path, a redirect with another state is refused,
    /// and the matching one returns the code.
    #[test]
    fn loopback_browser_serves_the_page_and_takes_the_matching_redirect() {
        let announced: std::sync::Arc<std::sync::Mutex<String>> = std::sync::Arc::default();
        let said = announced.clone();
        let mut browser = LoopbackBrowser::bind(
            Duration::from_secs(20),
            Box::new(move |line| *said.lock().unwrap() = line.to_string()),
        )
        .unwrap();
        let port = browser.port;
        let watcher = announced.clone();
        let client = std::thread::spawn(move || {
            let get = |path: &str| -> String {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut answer = String::new();
                stream.read_to_string(&mut answer).unwrap();
                answer
            };
            let page_url = loop {
                let line = watcher.lock().unwrap().clone();
                if let Some(at) = line.find("http://127.0.0.1:") {
                    break line[at..].split(' ').next().unwrap().to_string();
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let path = &page_url[page_url.find('/').unwrap() + 2..];
            let path = &path[path.find('/').unwrap()..];
            assert!(get("/").starts_with("HTTP/1.1 404"));
            let page = get(path);
            assert!(page.starts_with("HTTP/1.1 200") && page.contains("Create GitHub App"));
            let state = page
                .split("state=")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string();
            assert!(get("/callback?code=forged&state=wrong").starts_with("HTTP/1.1 400"));
            assert!(get(&format!("/callback?code=real&state={state}")).starts_with("HTTP/1.1 200"));
        });
        let code = browser
            .code(
                AppKind::Agent,
                "https://github.com/settings/apps/new",
                &json!({}),
                "0123456789abcdef0123456789abcdef",
            )
            .unwrap();
        client.join().unwrap();
        assert_eq!(code, "real");
    }
}
