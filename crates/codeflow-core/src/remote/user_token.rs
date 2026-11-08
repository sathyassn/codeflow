//! The developer's GitHub App user access token for the PR App (TSK-1348
//! AC-17 and AC-18), so a pull request an agent session opens or edits shows
//! the developer as its author.
//!
//! The PR App holds Pull requests write, Contents read and Metadata read
//! only, so its token can open and edit pull requests but cannot merge or
//! push. `codeflow remote pr-login` runs GitHub's device flow once: the
//! developer enters a code on GitHub, and the grant is kept in this
//! computer's credential store under its own service name. `codeflow remote
//! pr-token` prints the access token, renewing it with the refresh token when
//! it is near its end. GitHub stops accepting a refresh token once it is used,
//! so the renewed grant is stored before the new access token is printed, and
//! the caller holds [`refresh_lock`] so that two sessions never spend the same
//! refresh token. Only an authorization GitHub refuses, or one past its
//! expiry, sends the developer to `pr-login` again; a network or GitHub failure
//! names its own cause.
//!
//! A user access token has the permissions both the App and the user have,
//! and neither the device flow nor a refresh narrows them, so before a grant
//! is stored and before any token is printed, [`check_grant`] reads every
//! installation the token reaches and refuses unless each grants exactly the
//! PR App's three permissions and the repository recorded at setup is among
//! them. The check runs only here: a process of the same user that can reach
//! the credential store can read the grant and renew it without this command.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::agent_key::{
    self, call_failure, call_problem, redact, AppHttp, CallFailure, KeyStore, OAuthAnswer,
    OAuthHttp,
};

/// The credential store service under which a developer's PR App grant is
/// kept; the account is the repository, `owner/name`.
pub const PR_USER_SERVICE: &str = "codeflow-pr-user";

/// The permissions the PR App's installations must grant, exactly.
pub const PR_PERMISSIONS: [(&str, &str); 3] = [
    ("contents", "read"),
    ("metadata", "read"),
    ("pull_requests", "write"),
];

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// An access token this close to its end, in seconds, is renewed first.
const MARGIN: u64 = 300;

/// Items asked for per page when installations and repositories are read.
const PER_PAGE: usize = 100;

/// The most pages read of one list; a longer list cannot be checked, so it
/// refuses.
const MAX_PAGES: usize = 100;

/// The PR App as `setup-apps` recorded it: the ids the `pr-login` line
/// carries, which the grant check compares with GitHub's answers. None is
/// secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrApp {
    /// The App's slug, for its settings pages.
    pub slug: String,
    /// The App's id.
    pub app_id: u64,
    /// The App's client id, which the device flow and a renewal send.
    pub client_id: String,
    /// The id of the repository the App was set up for.
    pub repository_id: u64,
}

impl PrApp {
    /// The command a developer runs once to authorize the PR App for
    /// `repository`.
    #[must_use]
    pub fn login_line(&self, repository: &str) -> String {
        format!(
            "codeflow remote pr-login --repo {repository} --app {} --app-id {} --client-id {} --repository-id {}",
            self.slug, self.app_id, self.client_id, self.repository_id
        )
    }
}

/// What the store keeps for one repository: the developer's grant, with the
/// PR App it belongs to. `Debug` never prints a token.
#[derive(Clone, Serialize, Deserialize)]
pub struct UserGrant {
    /// The PR App the grant belongs to.
    pub app: PrApp,
    /// The GitHub login the grant acts as.
    pub login: String,
    /// The user access token.
    pub access_token: String,
    /// When the access token ends, in seconds since 1970.
    pub access_expires_at: u64,
    /// The refresh token, used once.
    pub refresh_token: String,
    /// When the refresh token ends, in seconds since 1970.
    pub refresh_expires_at: u64,
}

impl std::fmt::Debug for UserGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserGrant")
            .field("app", &self.app)
            .field("login", &self.login)
            .field("access_token", &"[redacted]")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_token", &"[redacted]")
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// A user access token ready to use. `Debug` never prints the token.
pub struct UserToken {
    /// The token.
    pub token: String,
    /// The GitHub login it acts as.
    pub login: String,
    /// When it ends, in seconds since 1970.
    pub expires_at: u64,
}

impl std::fmt::Debug for UserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserToken")
            .field("token", &"[redacted]")
            .field("login", &self.login)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// What `pr-login` needs besides its stores and transports: the clock, a
/// wait between polls, and where to show the code.
pub struct Login<'a> {
    /// The repository, `owner/name`.
    pub repository: &'a str,
    /// Seconds since 1970.
    pub now: &'a dyn Fn() -> u64,
    /// Wait this many seconds.
    pub wait: &'a dyn Fn(u64),
    /// Show a line to the developer.
    pub show: &'a dyn Fn(&str),
}

/// The command that authorizes again: the whole `pr-login` line when the PR
/// App is known, else where to find it.
#[must_use]
pub fn authorize_again(repository: &str, app: Option<&PrApp>) -> String {
    match app {
        Some(app) => format!("authorize again with `{}`", app.login_line(repository)),
        None => format!(
            "authorize with the `codeflow remote pr-login` line `codeflow remote setup-apps` printed for {repository}"
        ),
    }
}

/// GitHub's answer to an OAuth request: a grant, or a named refusal.
enum Answer {
    Granted(Value),
    Refused { error: String, value: Value },
}

/// Read an OAuth answer. An `error` field is GitHub's refusal whatever the
/// status; any other status that is not 200 says nothing about the grant.
fn read_answer(answer: &OAuthAnswer, what: &str) -> Result<Answer, String> {
    let value: Option<Value> = serde_json::from_str(&answer.body).ok();
    if let Some(value) = value.as_ref() {
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Ok(Answer::Refused {
                error: error.to_string(),
                value: value.clone(),
            });
        }
    }
    if answer.status != 200 {
        return Err(format!(
            "GitHub answered {} to {what}, with no OAuth error named",
            answer.status
        ));
    }
    value
        .map(Answer::Granted)
        .ok_or_else(|| format!("GitHub's answer to {what} is not JSON"))
}

/// GitHub's own words for a refusal, for a message.
fn refusal(error: &str, value: &Value) -> String {
    match value.get("error_description").and_then(Value::as_str) {
        Some(description) => format!("{error}: {description}"),
        None => error.to_string(),
    }
}

/// A grant from a token answer. Without a refresh token the access token
/// cannot be renewed, which GitHub does only for an App that keeps
/// expiring user tokens.
fn grant_from(value: &Value, app: &PrApp, login: &str, now: u64) -> Result<UserGrant, String> {
    let text = |field: &str| {
        value
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let seconds = |field: &str| value.get(field).and_then(Value::as_u64);
    let access_token = text("access_token")
        .ok_or_else(|| "GitHub's answer holds no user access token".to_string())?;
    let (Some(refresh_token), Some(access_in), Some(refresh_in)) = (
        text("refresh_token"),
        seconds("expires_in"),
        seconds("refresh_token_expires_in"),
    ) else {
        return Err(
            "GitHub gave a user access token with no refresh token and expiry, so it cannot be renewed: the PR App must keep expiring user access tokens on (its settings, \"Optional features\")"
                .to_string(),
        );
    };
    Ok(UserGrant {
        app: app.clone(),
        login: login.to_string(),
        access_token,
        access_expires_at: now.saturating_add(access_in),
        refresh_token,
        refresh_expires_at: now.saturating_add(refresh_in),
    })
}

/// The GitHub login a user access token acts as, read with `GET /user`.
fn user_login(http: &dyn AppHttp, token: &str) -> Result<String, String> {
    let answer = http.call("GET", &format!("{}/user", agent_key::API_ROOT), token, None)?;
    serde_json::from_str::<Value>(&answer)
        .ok()
        .and_then(|value| {
            value
                .get("login")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|login| !login.is_empty())
        .ok_or_else(|| "GitHub's answer to GET /user names no login".to_string())
}

/// The App's settings pages: a personal App's and an organization's.
fn settings_pages(owner: &str, slug: &str, tail: &str) -> String {
    format!(
        "{} for a personal App, or {} for an organization's",
        super::apps::app_page(None, slug, tail),
        super::apps::app_page(Some(owner), slug, tail)
    )
}

/// Every item of a paged GitHub list (`total_count` and the items under
/// `key`), read `PER_PAGE` at a time until `stop` holds for an item or all
/// `total_count` items are read. A page that cannot be read, is not that
/// shape, or comes back empty before the count is reached refuses, so a list
/// is never judged from part of it.
fn walk(
    http: &dyn AppHttp,
    token: &str,
    url: &str,
    key: &str,
    what: &str,
    stop: &dyn Fn(&Value) -> bool,
) -> Result<Vec<Value>, String> {
    let mut items: Vec<Value> = Vec::new();
    let mut total: Option<u64> = None;
    for page in 1..=MAX_PAGES {
        let address = format!("{url}?per_page={PER_PAGE}&page={page}");
        let answer = http.call("GET", &address, token, None).map_err(|error| {
            call_problem(
                &error,
                &format!("{what} could not be read (page {page}), so no token was printed"),
            )
        })?;
        let unreadable = || {
            format!("GitHub's answer for {what} (page {page}) is not a list it can be judged from, so no token was printed")
        };
        let value: Value = serde_json::from_str(&answer).map_err(|_| unreadable())?;
        let count = value
            .get("total_count")
            .and_then(Value::as_u64)
            .ok_or_else(unreadable)?;
        if total.is_some_and(|total| total != count) {
            return Err(format!(
                "{what} changed while it was read, so no token was printed; run it again"
            ));
        }
        total = Some(count);
        let list = value
            .get(key)
            .and_then(Value::as_array)
            .filter(|list| list.len() <= PER_PAGE)
            .ok_or_else(unreadable)?;
        let found = list.iter().any(stop);
        items.extend(list.iter().cloned());
        let read = u64::try_from(items.len()).unwrap_or(u64::MAX);
        if read > count {
            return Err(unreadable());
        }
        if found || read == count {
            return Ok(items);
        }
        if list.is_empty() {
            return Err(format!(
                "GitHub's answer for {what} stops at page {page} with {read} of {count} read, so no token was printed"
            ));
        }
    }
    Err(format!(
        "{what} runs past {MAX_PAGES} pages, more than can be checked, so no token was printed"
    ))
}

/// What differs between an installation's permissions and the PR App's
/// exact three.
fn permission_problems(permissions: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
    let Some(permissions) = permissions else {
        return vec!["no permissions GitHub describes".to_string()];
    };
    let mut wrong = Vec::new();
    for (name, level) in &PR_PERMISSIONS {
        match permissions.get(*name).and_then(Value::as_str) {
            Some(found) if found == *level => {}
            Some(found) => wrong.push(format!("{name} {found}, not {level}")),
            None => wrong.push(format!("no {name} {level}")),
        }
    }
    for (name, level) in permissions {
        if !PR_PERMISSIONS.iter().any(|(allowed, _)| allowed == name) {
            wrong.push(format!(
                "{name} {}, which the PR App must not have",
                level.as_str().unwrap_or("(unreadable)")
            ));
        }
    }
    wrong
}

/// An installation's settings page, where its owner approves permissions.
fn installation_page(installation: &Value, id: u64) -> String {
    let account = installation
        .pointer("/account/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if installation
        .pointer("/account/type")
        .and_then(Value::as_str)
        == Some("Organization")
    {
        format!("https://github.com/organizations/{account}/settings/installations/{id}")
    } else {
        format!("https://github.com/settings/installations/{id}")
    }
}

/// Check the grant behind `token` before it is stored or printed (AC-18):
/// every installation the token reaches, on every page, must belong to the
/// PR App recorded at setup and grant exactly its three permissions, and the
/// repository id recorded at setup must be among an installation's
/// repositories. Ids come from GitHub's answers; no owner or name from the
/// working copy is used. A widened App stops the next print; a token already
/// printed is not recalled.
///
/// # Errors
///
/// When anything differs, or a page cannot be read whole.
pub fn check_grant(
    http: &dyn AppHttp,
    token: &str,
    app: &PrApp,
    repository: &str,
) -> Result<(), String> {
    let owner = repository.split('/').next().unwrap_or_default();
    let api = agent_key::API_ROOT;
    let installations = walk(
        http,
        token,
        &format!("{api}/user/installations"),
        "installations",
        "the PR App's installations",
        &|_| false,
    )?;
    if installations.is_empty() {
        return Err(format!(
            "the PR App {} is installed nowhere this login can reach, so no token was printed; install it: {}",
            app.slug,
            super::apps::install_url(&app.slug)
        ));
    }
    let mut problems = Vec::new();
    let mut ids = Vec::new();
    for installation in &installations {
        let Some(id) = installation.get("id").and_then(Value::as_u64) else {
            problems.push("an installation GitHub gives no id for".to_string());
            continue;
        };
        ids.push(id);
        let mut wrong = Vec::new();
        let app_id = installation.get("app_id").and_then(Value::as_u64);
        if app_id != Some(app.app_id) {
            wrong.push(format!(
                "App id {}, not {}",
                app_id.map_or_else(|| "none".to_string(), |id| id.to_string()),
                app.app_id
            ));
        }
        if let Some(client_id) = installation.get("client_id").and_then(Value::as_str) {
            if client_id != app.client_id {
                wrong.push(format!("client id {client_id}, not {}", app.client_id));
            }
        }
        wrong.extend(permission_problems(
            installation.get("permissions").and_then(Value::as_object),
        ));
        if !wrong.is_empty() {
            problems.push(format!(
                "installation {id} ({}): {}",
                installation_page(installation, id),
                wrong.join(", ")
            ));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "the PR App's grant is not the one CodeFlow allows, so no token was printed: {}; the App's permissions are on {}, and an installation's owner approves them on its page",
            problems.join("; "),
            settings_pages(owner, &app.slug, "/permissions")
        ));
    }
    let target = app.repository_id;
    let is_target =
        |repository: &Value| repository.get("id").and_then(Value::as_u64) == Some(target);
    for id in ids {
        let repositories = walk(
            http,
            token,
            &format!("{api}/user/installations/{id}/repositories"),
            "repositories",
            &format!("installation {id}'s repositories"),
            &is_target,
        )?;
        if repositories.iter().any(is_target) {
            return Ok(());
        }
    }
    Err(format!(
        "the repository id {target} recorded at setup is not among the repositories the PR App {} reaches for this login, so no token was printed; install the App on that repository, or run `codeflow remote setup-apps` again if the repository was replaced",
        app.slug
    ))
}

/// Poll GitHub for the device flow's grant, as often as it allows, until the
/// code is entered, refused or past `deadline`.
fn poll(
    oauth: &dyn OAuthHttp,
    login: &Login<'_>,
    app: &PrApp,
    device_code: &str,
    mut interval: u64,
    deadline: u64,
) -> Result<Value, String> {
    let expired = format!(
        "the code expired before it was entered; nothing was stored; run `{}` again",
        app.login_line(login.repository)
    );
    loop {
        (login.wait)(interval);
        if (login.now)() >= deadline {
            return Err(expired);
        }
        let answer = oauth
            .post_form(
                TOKEN_URL,
                &[
                    ("client_id", app.client_id.as_str()),
                    ("device_code", device_code),
                    ("grant_type", DEVICE_GRANT),
                ],
            )
            .map_err(|error| call_problem(&error, "the authorization was not completed"))?;
        match read_answer(&answer, "the authorization request")? {
            Answer::Granted(value) => return Ok(value),
            Answer::Refused { error, .. } if error == "authorization_pending" => {}
            Answer::Refused { error, value } if error == "slow_down" => {
                interval = value
                    .get("interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(interval + 5);
            }
            Answer::Refused { error, .. }
                if error == "expired_token" || error == "token_expired" =>
            {
                return Err(expired);
            }
            Answer::Refused { error, .. } if error == "access_denied" => {
                return Err(
                    "the authorization was cancelled on GitHub; nothing was stored".to_string(),
                );
            }
            Answer::Refused { error, value } => {
                return Err(format!(
                    "GitHub refused the authorization: {}; nothing was stored",
                    refusal(&error, &value)
                ));
            }
        }
    }
}

/// Authorize the PR App for this developer through GitHub's device flow,
/// check the grant (AC-18), and keep it in `user_store`. No client secret is
/// used: the device flow needs none.
///
/// # Errors
///
/// When the store cannot answer, GitHub refuses or cannot be reached, the
/// code expires, the grant check refuses, or the store does not keep the
/// grant.
pub fn pr_login(
    user_store: &dyn KeyStore,
    oauth: &dyn OAuthHttp,
    http: &dyn AppHttp,
    login: &Login<'_>,
    app: &PrApp,
) -> Result<Vec<String>, String> {
    let repository = login.repository;
    user_store.available()?;
    let owner = repository.split('/').next().unwrap_or_default();
    let start = (login.now)();
    let answer = oauth
        .post_form(DEVICE_CODE_URL, &[("client_id", app.client_id.as_str())])
        .map_err(|error| call_problem(&error, "no device code was issued"))?;
    let value = match read_answer(&answer, "the device code request")? {
        Answer::Granted(value) => value,
        Answer::Refused { error, .. } if error == "device_flow_disabled" => {
            return Err(format!(
                "the PR App {} has its device flow off: on its settings page ({}), under \"Identifying and authorizing users\", select \"Enable Device Flow\" and save, then run this again",
                app.slug,
                settings_pages(owner, &app.slug, "")
            ));
        }
        Answer::Refused { error, value } => {
            return Err(format!(
                "GitHub refused the device code request: {}",
                refusal(&error, &value)
            ));
        }
    };
    let field = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default();
    let (device_code, user_code, verification_uri) = (
        field("device_code"),
        field("user_code"),
        field("verification_uri"),
    );
    if device_code.is_empty() || user_code.is_empty() || verification_uri.is_empty() {
        return Err("GitHub's device code answer lacks the code or the address".to_string());
    }
    let lasts = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(900);
    let interval = value.get("interval").and_then(Value::as_u64).unwrap_or(5);
    (login.show)(&format!(
        "open {verification_uri} and enter the code {user_code} to let the PR App {} open and edit pull requests for you on {repository}; this waits up to {} minutes",
        app.slug,
        lasts.div_ceil(60)
    ));
    let granted = poll(
        oauth,
        login,
        app,
        device_code,
        interval,
        start.saturating_add(lasts),
    )?;
    let mut grant = grant_from(&granted, app, "", (login.now)())?;
    let secrets = [grant.access_token.clone(), grant.refresh_token.clone()];
    let hide = |text: &str| redact(text, &[secrets[0].as_str(), secrets[1].as_str()]);
    grant.login = user_login(http, &grant.access_token).map_err(|error| {
        hide(&call_problem(
            &error,
            "the authorized login cannot be confirmed, and nothing was stored",
        ))
    })?;
    check_grant(http, &grant.access_token, app, repository)
        .map_err(|why| format!("{}; nothing was stored", hide(&why)))?;
    let entry = serde_json::to_string(&grant)
        .map_err(|error| format!("cannot encode the authorization: {error}"))?;
    user_store.replace(repository, &entry).map_err(|why| {
        format!(
            "GitHub authorized {}, but {} did not keep the authorization: {}",
            grant.login,
            user_store.describe(),
            redact(&hide(&why), &[entry.as_str()])
        )
    })?;
    Ok(vec![
        format!(
            "authorized as {} for {repository}; the grant is kept in {} (remove it: {})",
            grant.login,
            user_store.describe(),
            user_store.undo(repository)
        ),
        format!(
            "agent sessions open and edit pull requests with `GH_TOKEN=$(codeflow remote pr-token --repo {repository})`: they show {} as author, and the token cannot merge or push",
            grant.login
        ),
    ])
}

/// The PR App's user access token for `repository`, renewed first when it is
/// near its end, and printed only after [`check_grant`] passes. Call it while
/// holding [`refresh_lock`].
///
/// # Errors
///
/// When there is no grant, GitHub refuses it or it has expired (the message
/// names `pr-login`), GitHub cannot be reached or fails (the message names
/// that cause and keeps the grant), the store does not keep a renewed grant,
/// or the grant check refuses.
pub fn pr_token(
    user_store: &dyn KeyStore,
    oauth: &dyn OAuthHttp,
    http: &dyn AppHttp,
    repository: &str,
    now: u64,
) -> Result<UserToken, String> {
    user_store.available()?;
    if !user_store.exists(repository)? {
        return Err(format!(
            "no PR App authorization for {repository} on this computer; {}",
            authorize_again(repository, None)
        ));
    }
    let raw = user_store.load(repository)?;
    let grant: UserGrant = serde_json::from_str(&raw).map_err(|_| {
        format!(
            "the saved PR App authorization for {repository} is not readable; {}",
            authorize_again(repository, None)
        )
    })?;
    let again = authorize_again(repository, Some(&grant.app));
    let token = current_token(user_store, oauth, http, repository, now, &grant, &again)?;
    let hide = |text: &str| {
        redact(
            text,
            &[
                grant.access_token.as_str(),
                grant.refresh_token.as_str(),
                token.token.as_str(),
            ],
        )
    };
    check_grant(http, &token.token, &grant.app, repository).map_err(|why| hide(&why))?;
    Ok(token)
}

/// The grant's access token when it has time left and GitHub still takes it,
/// else a renewed one, stored before it is handed back.
fn current_token(
    user_store: &dyn KeyStore,
    oauth: &dyn OAuthHttp,
    http: &dyn AppHttp,
    repository: &str,
    now: u64,
    grant: &UserGrant,
    again: &str,
) -> Result<UserToken, String> {
    let hide = |text: &str| {
        redact(
            text,
            &[grant.access_token.as_str(), grant.refresh_token.as_str()],
        )
    };
    if now.saturating_add(MARGIN) < grant.access_expires_at {
        match user_login(http, &grant.access_token) {
            Ok(login) => {
                return Ok(UserToken {
                    token: grant.access_token.clone(),
                    login,
                    expires_at: grant.access_expires_at,
                });
            }
            // GitHub no longer takes it; a refresh decides whether the grant
            // still stands.
            Err(error) if call_failure(&error) == CallFailure::Status(401) => {}
            Err(error) => {
                return Err(hide(&call_problem(
                    &error,
                    "the saved user token cannot be confirmed; the authorization is unchanged",
                )));
            }
        }
    }
    if now >= grant.refresh_expires_at {
        return Err(format!(
            "the PR App authorization for {repository} on this computer has expired; {again}"
        ));
    }
    let answer = oauth
        .post_form(
            TOKEN_URL,
            &[
                ("client_id", grant.app.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", grant.refresh_token.as_str()),
            ],
        )
        .map_err(|error| {
            hide(&call_problem(
                &error,
                "the user token was not renewed; the authorization is unchanged",
            ))
        })?;
    let value = match read_answer(&answer, "the renewal").map_err(|why| hide(&why))? {
        Answer::Granted(value) => value,
        Answer::Refused { error, .. } => {
            return Err(format!(
                "GitHub no longer accepts the saved PR App authorization for {repository} ({error}): it has expired or was revoked; {again}"
            ));
        }
    };
    // From here the old refresh token is spent.
    let renewed = grant_from(&value, &grant.app, &grant.login, now)
        .map_err(|why| format!("{}; {again}", hide(&why)))?;
    let entry = serde_json::to_string(&renewed)
        .map_err(|error| format!("cannot encode the authorization: {error}"))?;
    user_store.replace(repository, &entry).map_err(|why| {
        format!(
            "GitHub renewed the PR App authorization for {repository}, but {} did not keep it, so the saved one no longer works: {}; {again}",
            user_store.describe(),
            redact(
                &hide(&why),
                &[
                    renewed.access_token.as_str(),
                    renewed.refresh_token.as_str(),
                    entry.as_str()
                ]
            )
        )
    })?;
    Ok(UserToken {
        token: renewed.access_token,
        login: renewed.login,
        expires_at: renewed.access_expires_at,
    })
}

/// Hold this computer's lock for `repository`'s PR App grant until the file
/// is dropped, so one renewal runs at a time: GitHub stops accepting a refresh
/// token once it is used, and a second session spending it would be refused.
/// The lock file, under `locks` in the codeflow home (`~/.codeflow`), holds
/// nothing.
///
/// # Errors
///
/// When the lock file cannot be made or locked.
pub fn refresh_lock(codeflow_home: &Path, repository: &str) -> Result<std::fs::File, String> {
    use fs2::FileExt;
    let dir = codeflow_home.join("locks");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    let path = dir.join(format!("pr-user-{}.lock", repository.replace('/', "--")));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    file.lock_exclusive()
        .map_err(|error| format!("cannot lock {}: {error}", path.display()))?;
    Ok(file)
}

#[cfg(test)]
#[path = "user_token_tests.rs"]
mod tests;
