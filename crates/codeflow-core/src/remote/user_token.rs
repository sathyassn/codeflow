//! The developer's GitHub App user access token for the agent App (TSK-1348
//! AC-13 to AC-15), so a pull request an agent session opens shows the
//! developer as its author.
//!
//! `codeflow remote agent-login` runs GitHub's device flow once: the developer
//! enters a code on GitHub, and the grant is kept in this computer's
//! credential store under its own service name, apart from the App's key.
//! `codeflow remote agent-token --user` prints the access token, renewing it
//! with the refresh token when it is near its end. GitHub stops accepting a
//! refresh token once it is used, so the renewed grant is stored before the
//! new access token is printed, and the caller holds [`refresh_lock`] so that
//! two sessions never spend the same refresh token. Only an authorization
//! GitHub refuses, or one past its expiry, sends the developer to
//! `agent-login` again; a network or GitHub failure names its own cause.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::agent_key::{
    self, call_failure, call_problem, redact, AppHttp, CallFailure, KeyStore, OAuthAnswer,
    OAuthHttp,
};

/// The credential store service under which a developer's grant is kept; the
/// account is the repository, `owner/name`.
pub const USER_TOKEN_SERVICE: &str = "codeflow-agent-user";

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// An access token this close to its end, in seconds, is renewed first.
const MARGIN: u64 = 300;

/// What the store keeps for one repository: the developer's grant. `Debug`
/// never prints a token.
#[derive(Clone, Serialize, Deserialize)]
pub struct UserGrant {
    /// The agent App's client id, which the grant belongs to.
    pub client_id: String,
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
            .field("client_id", &self.client_id)
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

/// What `agent-login` needs besides its stores and transports: the clock, a
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

/// The command that authorizes again.
#[must_use]
pub fn authorize_again(repository: &str) -> String {
    format!("authorize again with `codeflow remote agent-login --repo {repository}`")
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
fn grant_from(value: &Value, client_id: &str, login: &str, now: u64) -> Result<UserGrant, String> {
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
            "GitHub gave a user access token with no refresh token and expiry, so it cannot be renewed: the agent App must keep expiring user access tokens on (its settings, \"Optional features\")"
                .to_string(),
        );
    };
    Ok(UserGrant {
        client_id: client_id.to_string(),
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

/// The App's settings pages, for the device flow switch: a personal App's and
/// an organization's.
fn settings_pages(owner: &str, slug: &str) -> String {
    format!(
        "{} for a personal App, or {} for an organization's",
        super::apps::app_page(None, slug, ""),
        super::apps::app_page(Some(owner), slug, "")
    )
}

/// Poll GitHub for the device flow's grant, as often as it allows, until the
/// code is entered, refused or past `deadline`.
fn poll(
    oauth: &dyn OAuthHttp,
    login: &Login<'_>,
    client_id: &str,
    device_code: &str,
    mut interval: u64,
    deadline: u64,
) -> Result<Value, String> {
    let expired = format!(
        "the code expired before it was entered; nothing was stored; run `codeflow remote agent-login --repo {}` again",
        login.repository
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
                    ("client_id", client_id),
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

/// Authorize the agent App for this developer through GitHub's device flow,
/// and keep the grant in `user_store`.
///
/// # Errors
///
/// When the agent App entry or the store cannot be read, GitHub refuses or
/// cannot be reached, the code expires, or the store does not keep the grant.
pub fn agent_login(
    app_store: &dyn KeyStore,
    user_store: &dyn KeyStore,
    oauth: &dyn OAuthHttp,
    http: &dyn AppHttp,
    login: &Login<'_>,
) -> Result<Vec<String>, String> {
    let repository = login.repository;
    let app = agent_key::load_app(app_store, repository)?;
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
                "the agent App {} has its device flow off: on its settings page ({}), under \"Identifying and authorizing users\", select \"Enable Device Flow\" and save, then run this again",
                app.slug,
                settings_pages(owner, &app.slug)
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
        "open {verification_uri} and enter the code {user_code} to let the agent App {} act for you on {repository}; this waits up to {} minutes",
        app.slug,
        lasts.div_ceil(60)
    ));
    let granted = poll(
        oauth,
        login,
        &app.client_id,
        device_code,
        interval,
        start.saturating_add(lasts),
    )?;
    let mut grant = grant_from(&granted, &app.client_id, "", (login.now)())?;
    let secrets = [grant.access_token.clone(), grant.refresh_token.clone()];
    let hide = |text: &str| redact(text, &[secrets[0].as_str(), secrets[1].as_str()]);
    grant.login = user_login(http, &grant.access_token).map_err(|error| {
        hide(&call_problem(
            &error,
            "the authorized login cannot be confirmed, and nothing was stored",
        ))
    })?;
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
            "`codeflow remote agent-token --user` prints a user access token: pull requests opened with it show {} as author, with the agent App {} as the app",
            grant.login, app.slug
        ),
    ])
}

/// A user access token for `repository`, renewed first when it is near its
/// end. Call it while holding [`refresh_lock`].
///
/// # Errors
///
/// When there is no grant, GitHub refuses it or it has expired (the message
/// names `agent-login`), GitHub cannot be reached or fails (the message names
/// that cause and keeps the grant), or the store does not keep a renewed grant.
pub fn user_token(
    user_store: &dyn KeyStore,
    oauth: &dyn OAuthHttp,
    http: &dyn AppHttp,
    repository: &str,
    now: u64,
) -> Result<UserToken, String> {
    user_store.available()?;
    if !user_store.exists(repository)? {
        return Err(format!(
            "no GitHub authorization for {repository} on this computer; {}",
            authorize_again(repository)
        ));
    }
    let raw = user_store.load(repository)?;
    let grant: UserGrant = serde_json::from_str(&raw).map_err(|_| {
        format!(
            "the saved GitHub authorization for {repository} is not readable; {}",
            authorize_again(repository)
        )
    })?;
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
            "the GitHub authorization for {repository} on this computer has expired; {}",
            authorize_again(repository)
        ));
    }
    let answer = oauth
        .post_form(
            TOKEN_URL,
            &[
                ("client_id", grant.client_id.as_str()),
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
                "GitHub no longer accepts the saved authorization for {repository} ({error}): it has expired or was revoked; {}",
                authorize_again(repository)
            ));
        }
    };
    // From here the old refresh token is spent.
    let renewed = grant_from(&value, &grant.client_id, &grant.login, now)
        .map_err(|why| format!("{why}; {}", authorize_again(repository)))?;
    let entry = serde_json::to_string(&renewed)
        .map_err(|error| format!("cannot encode the authorization: {error}"))?;
    user_store.replace(repository, &entry).map_err(|why| {
        format!(
            "GitHub renewed the authorization for {repository}, but {} did not keep it, so the saved one no longer works: {}; {}",
            user_store.describe(),
            redact(
                &hide(&why),
                &[
                    renewed.access_token.as_str(),
                    renewed.refresh_token.as_str(),
                    entry.as_str()
                ]
            ),
            authorize_again(repository)
        )
    })?;
    Ok(UserToken {
        token: renewed.access_token,
        login: renewed.login,
        expires_at: renewed.access_expires_at,
    })
}

/// Hold this computer's lock for `repository`'s grant until the file is
/// dropped, so one renewal runs at a time: GitHub stops accepting a refresh
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
    let path = dir.join(format!("agent-user-{}.lock", repository.replace('/', "--")));
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
mod tests {
    use super::*;
    use crate::remote::agent_key::tests::MemoryStore;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    const REFRESH: &str = "ghr_refresh_one";
    const ACCESS: &str = "ghu_access_one";

    /// One posted form: the address and its fields.
    type Posted = (String, Vec<(String, String)>);

    /// An OAuth endpoint that answers from a queue and records each form.
    #[derive(Default)]
    struct FakeOAuth {
        answers: RefCell<VecDeque<Result<OAuthAnswer, String>>>,
        forms: RefCell<Vec<Posted>>,
    }

    impl FakeOAuth {
        fn with(answers: Vec<Result<(u16, Value), String>>) -> Self {
            let fake = Self::default();
            for answer in answers {
                fake.answers
                    .borrow_mut()
                    .push_back(answer.map(|(status, body)| OAuthAnswer {
                        status,
                        body: body.to_string(),
                    }));
            }
            fake
        }
    }

    impl OAuthHttp for FakeOAuth {
        fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<OAuthAnswer, String> {
            self.forms.borrow_mut().push((
                url.to_string(),
                form.iter()
                    .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                    .collect(),
            ));
            self.answers
                .borrow_mut()
                .pop_front()
                .expect("an OAuth call nobody expected")
        }
    }

    /// `GET /user`: who each token is, or why it fails.
    struct FakeUser(Result<&'static str, &'static str>);

    impl AppHttp for FakeUser {
        fn call(
            &self,
            method: &str,
            url: &str,
            bearer: &str,
            _: Option<&str>,
        ) -> Result<String, String> {
            assert_eq!((method, url), ("GET", "https://api.github.com/user"));
            assert!(!bearer.is_empty());
            self.0
                .map(|login| json!({"login": login}).to_string())
                .map_err(str::to_string)
        }
    }

    fn app_store() -> MemoryStore {
        let store = MemoryStore::default();
        store.entries.borrow_mut().insert(
            "o/r".to_string(),
            json!({"app_id": 7, "client_id": "Iv23client", "slug": "codeflow-agent-r", "pem": "not used"})
                .to_string(),
        );
        store
    }

    fn granted(access: &str, refresh: &str) -> Value {
        json!({
            "access_token": access,
            "expires_in": 28800,
            "refresh_token": refresh,
            "refresh_token_expires_in": 15_897_600,
            "token_type": "bearer",
            "scope": ""
        })
    }

    fn device_code() -> Value {
        json!({
            "device_code": "device-secret",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        })
    }

    fn grant(access_expires_at: u64, refresh_expires_at: u64) -> String {
        serde_json::to_string(&UserGrant {
            client_id: "Iv23client".to_string(),
            login: "dev".to_string(),
            access_token: ACCESS.to_string(),
            access_expires_at,
            refresh_token: REFRESH.to_string(),
            refresh_expires_at,
        })
        .unwrap()
    }

    fn user_store_with(entry: &str) -> MemoryStore {
        let store = MemoryStore::default();
        store
            .entries
            .borrow_mut()
            .insert("o/r".to_string(), entry.to_string());
        store
    }

    /// Run `agent-login` with a clock that moves by each wait.
    fn login(
        user_store: &MemoryStore,
        oauth: &FakeOAuth,
        http: &dyn AppHttp,
    ) -> (Result<Vec<String>, String>, Vec<String>, Vec<u64>) {
        let clock = Cell::new(1_000);
        let waits = RefCell::new(Vec::new());
        let shown = RefCell::new(Vec::new());
        let now = || clock.get();
        let wait = |seconds: u64| {
            waits.borrow_mut().push(seconds);
            clock.set(clock.get() + seconds);
        };
        let show = |line: &str| shown.borrow_mut().push(line.to_string());
        let result = agent_login(
            &app_store(),
            user_store,
            oauth,
            http,
            &Login {
                repository: "o/r",
                now: &now,
                wait: &wait,
                show: &show,
            },
        );
        (result, shown.into_inner(), waits.into_inner())
    }

    /// AC-13: the device flow, polled as GitHub asks, keeps the grant with
    /// the login GitHub names, and shows the code and address once.
    #[test]
    fn agent_login_runs_the_device_flow_and_keeps_the_grant() {
        let oauth = FakeOAuth::with(vec![
            Ok((200, device_code())),
            Ok((200, json!({"error": "authorization_pending"}))),
            Ok((200, json!({"error": "slow_down", "interval": 10}))),
            Ok((200, granted(ACCESS, REFRESH))),
        ]);
        let store = MemoryStore::default();
        let (result, shown, waits) = login(&store, &oauth, &FakeUser(Ok("dev")));
        let lines = result.unwrap();
        assert!(
            lines[0].starts_with("authorized as dev for o/r"),
            "{lines:?}"
        );
        assert!(lines[1].contains("show dev as author"), "{lines:?}");
        assert_eq!(waits, [5, 5, 10]);
        assert_eq!(shown.len(), 1);
        assert!(
            shown[0].contains("open https://github.com/login/device and enter the code WDJB-MJHT")
        );
        let kept: UserGrant = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
        assert_eq!(
            (
                kept.login.as_str(),
                kept.client_id.as_str(),
                kept.refresh_token.as_str()
            ),
            ("dev", "Iv23client", REFRESH)
        );
        assert_eq!(kept.access_expires_at, 1_020 + 28_800);
        let forms = oauth.forms.borrow();
        assert_eq!(forms[0].0, "https://github.com/login/device/code");
        assert_eq!(
            forms[0].1,
            [("client_id".to_string(), "Iv23client".to_string())]
        );
        assert_eq!(forms[1].0, "https://github.com/login/oauth/access_token");
        assert!(forms[1].1.contains(&(
            "grant_type".to_string(),
            "urn:ietf:params:oauth:grant-type:device_code".to_string()
        )));
        assert!(!forms
            .iter()
            .flat_map(|(_, form)| form)
            .any(|(name, _)| name == "client_secret"));
        for line in lines.iter().chain(shown.iter()) {
            assert!(
                !line.contains(ACCESS)
                    && !line.contains(REFRESH)
                    && !line.contains("device-secret"),
                "{line}"
            );
        }
    }

    /// AC-15: GitHub's `device_flow_disabled` names the switch and its page,
    /// and nothing is stored.
    #[test]
    fn agent_login_names_the_device_flow_switch_when_it_is_off() {
        let oauth = FakeOAuth::with(vec![Ok((
            200,
            json!({"error": "device_flow_disabled", "error_description": "Device flow must be explicitly enabled for this App"}),
        ))]);
        let store = MemoryStore::default();
        let (result, shown, _) = login(&store, &oauth, &FakeUser(Ok("dev")));
        let error = result.unwrap_err();
        assert!(
            error.contains(
                "under \"Identifying and authorizing users\", select \"Enable Device Flow\""
            ),
            "{error}"
        );
        assert!(
            error.contains("https://github.com/settings/apps/codeflow-agent-r for a personal App"),
            "{error}"
        );
        assert!(
            error.contains("https://github.com/organizations/o/settings/apps/codeflow-agent-r"),
            "{error}"
        );
        assert!(shown.is_empty());
        assert!(store.entries.borrow().is_empty());
    }

    /// A code nobody enters, a cancelled one, and an App that opted out of
    /// expiring tokens each store nothing and say why.
    #[test]
    fn agent_login_stores_nothing_when_it_is_not_granted() {
        for (answers, said) in [
            (vec![json!({"error": "expired_token"})], "the code expired"),
            (vec![json!({"error": "token_expired"})], "the code expired"),
            (
                vec![json!({"error": "access_denied"})],
                "cancelled on GitHub",
            ),
            (
                vec![json!({"access_token": ACCESS, "token_type": "bearer"})],
                "keep expiring user access tokens on",
            ),
        ] {
            let mut all = vec![Ok((200, device_code()))];
            all.extend(answers.into_iter().map(|answer| Ok((200, answer))));
            let oauth = FakeOAuth::with(all);
            let store = MemoryStore::default();
            let (result, _, _) = login(&store, &oauth, &FakeUser(Ok("dev")));
            let error = result.unwrap_err();
            assert!(error.contains(said), "{error}");
            assert!(!error.contains(ACCESS), "{error}");
            assert!(store.entries.borrow().is_empty(), "{error}");
        }
        // Pending for longer than the code lasts.
        let mut all = vec![Ok((
            200,
            json!({"device_code": "d", "user_code": "U-1", "verification_uri": "https://github.com/login/device", "expires_in": 12, "interval": 5}),
        ))];
        all.extend((0..3).map(|_| Ok((200, json!({"error": "authorization_pending"})))));
        let store = MemoryStore::default();
        let (result, _, waits) = login(&store, &FakeOAuth::with(all), &FakeUser(Ok("dev")));
        assert!(result.unwrap_err().contains("the code expired"));
        assert_eq!(waits, [5, 5, 5]);
        assert!(store.entries.borrow().is_empty());
    }

    /// AC-13: a token with time left is checked as the developer and printed,
    /// with no renewal.
    #[test]
    fn a_fresh_token_is_used_without_a_renewal() {
        let store = user_store_with(&grant(10_000, 1_000_000));
        let oauth = FakeOAuth::default();
        let token = user_token(&store, &oauth, &FakeUser(Ok("dev")), "o/r", 1_000).unwrap();
        assert_eq!(
            (token.token.as_str(), token.login.as_str()),
            (ACCESS, "dev")
        );
        assert!(oauth.forms.borrow().is_empty());
        assert!(!format!("{token:?}").contains(ACCESS));
    }

    /// AC-13: near its end the token is renewed silently, and the new refresh
    /// token is kept before the new access token is handed back.
    #[test]
    fn a_renewal_keeps_the_new_refresh_token_before_it_hands_back_the_token() {
        let store = user_store_with(&grant(1_100, 1_000_000));
        let oauth = FakeOAuth::with(vec![Ok((
            200,
            granted("ghu_access_two", "ghr_refresh_two"),
        ))]);
        let token = user_token(&store, &oauth, &FakeUser(Ok("dev")), "o/r", 1_000).unwrap();
        assert_eq!(token.token, "ghu_access_two");
        let kept: UserGrant = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
        assert_eq!(kept.refresh_token, "ghr_refresh_two");
        assert_eq!(kept.access_token, "ghu_access_two");
        assert_eq!(kept.login, "dev");
        let forms = oauth.forms.borrow();
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].0, "https://github.com/login/oauth/access_token");
        assert_eq!(
            forms[0].1,
            [
                ("client_id".to_string(), "Iv23client".to_string()),
                ("grant_type".to_string(), "refresh_token".to_string()),
                ("refresh_token".to_string(), REFRESH.to_string()),
            ]
        );
        // A store that does not keep it: no token is handed back, and the
        // message says the saved grant is spent.
        let refusing = MemoryStore {
            refuse: true,
            ..user_store_with(&grant(1_100, 1_000_000))
        };
        let oauth = FakeOAuth::with(vec![Ok((
            200,
            granted("ghu_access_two", "ghr_refresh_two"),
        ))]);
        let error = user_token(&refusing, &oauth, &FakeUser(Ok("dev")), "o/r", 1_000).unwrap_err();
        assert!(
            error.contains("did not keep it, so the saved one no longer works"),
            "{error}"
        );
        assert!(
            error.contains("codeflow remote agent-login --repo o/r"),
            "{error}"
        );
        assert!(
            !error.contains("ghu_access_two") && !error.contains("ghr_refresh_two"),
            "{error}"
        );
    }

    /// AC-13: a revoked token (401) leads to a renewal, which GitHub refuses:
    /// one message naming `agent-login`.
    #[test]
    fn a_revoked_or_expired_grant_says_to_authorize_again() {
        let store = user_store_with(&grant(10_000, 1_000_000));
        let oauth = FakeOAuth::with(vec![Ok((
            200,
            json!({"error": "bad_refresh_token", "error_description": "The refresh token passed is incorrect or expired."}),
        ))]);
        let user = FakeUser(Err("GET https://api.github.com/user failed: curl: (22) The requested URL returned error: 401"));
        let error = user_token(&store, &oauth, &user, "o/r", 1_000).unwrap_err();
        assert_eq!(
            error,
            "GitHub no longer accepts the saved authorization for o/r (bad_refresh_token): it has expired or was revoked; authorize again with `codeflow remote agent-login --repo o/r`"
        );
        // Past the refresh token's own end: no call at all.
        let store = user_store_with(&grant(500, 900));
        let oauth = FakeOAuth::default();
        let error = user_token(&store, &oauth, &FakeUser(Ok("dev")), "o/r", 1_000).unwrap_err();
        assert!(
            error.contains(
                "has expired; authorize again with `codeflow remote agent-login --repo o/r`"
            ),
            "{error}"
        );
        assert!(oauth.forms.borrow().is_empty());
        // No grant yet.
        let error = user_token(
            &MemoryStore::default(),
            &FakeOAuth::default(),
            &FakeUser(Ok("dev")),
            "o/r",
            1_000,
        )
        .unwrap_err();
        assert!(error.contains("no GitHub authorization for o/r"), "{error}");
    }

    /// AC-13: a network or GitHub failure names its cause, keeps the grant,
    /// and never sends the developer to authorize again.
    #[test]
    fn a_failure_that_is_not_a_refusal_never_says_authorize_again() {
        let before = grant(1_100, 1_000_000);
        for (answer, said) in [
            (
                Err("POST https://github.com/login/oauth/access_token failed: curl: (6) Could not resolve host: github.com".to_string()),
                "cannot reach GitHub",
            ),
            (Ok((502, json!("Bad Gateway"))), "GitHub answered 502"),
            (Ok((200, json!("not an object"))), "holds no user access token"),
        ] {
            let store = user_store_with(&before);
            let oauth = FakeOAuth::with(vec![answer]);
            let error = user_token(&store, &oauth, &FakeUser(Ok("dev")), "o/r", 1_000).unwrap_err();
            assert!(error.contains(said), "{error}");
            assert!(!error.contains(REFRESH) && !error.contains(ACCESS), "{error}");
            if said != "holds no user access token" {
                assert!(!error.contains("agent-login"), "{error}");
                assert_eq!(store.load("o/r").unwrap(), before);
            }
        }
        // `GET /user` failing for a reason that is not 401 is not a refusal.
        let store = user_store_with(&grant(10_000, 1_000_000));
        let user = FakeUser(Err("GET https://api.github.com/user failed: curl: (7) Failed to connect to api.github.com port 443"));
        let error = user_token(&store, &FakeOAuth::default(), &user, "o/r", 1_000).unwrap_err();
        assert!(error.contains("cannot reach GitHub"), "{error}");
        assert!(!error.contains("agent-login"), "{error}");
    }

    /// AC-13: one renewal at a time: while one session holds the lock, a
    /// second cannot take it.
    #[test]
    fn the_refresh_lock_lets_one_holder_at_a_time() {
        use fs2::FileExt;
        let home = tempfile::tempdir().unwrap();
        let held = refresh_lock(home.path(), "o/r").unwrap();
        let path = home.path().join("locks/agent-user-o--r.lock");
        let second = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(second.try_lock_exclusive().is_err());
        drop(held);
        assert!(second.try_lock_exclusive().is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), b"");
    }
}
