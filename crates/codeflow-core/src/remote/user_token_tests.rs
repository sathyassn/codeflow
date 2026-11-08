//! Tests for the PR App's user access token (TSK-1348 AC-17 and AC-18). Every
//! store, OAuth endpoint and GitHub answer here is a fake in memory; no test
//! reaches GitHub or a credential store.

use super::*;
use crate::remote::agent_key::tests::MemoryStore;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};

const REFRESH: &str = "ghr_refresh_one";
const ACCESS: &str = "ghu_access_one";
use crate::remote::agent_key::API_ROOT as API;

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

/// GitHub's REST answers by address, for `GET /user` and the paged
/// installation and repository lists; any other address answers 404. Records
/// each address and bearer.
struct FakeGitHub {
    answers: RefCell<BTreeMap<String, Result<String, String>>>,
    calls: RefCell<Vec<(String, String)>>,
}

impl FakeGitHub {
    /// The developer `dev`, one installation of the PR App with exactly its
    /// permissions, holding the recorded repository.
    fn good() -> Self {
        Self::empty()
            .user(Ok("dev"))
            .installations(&[installation(11, &perms())])
            .repositories(11, &[repo(42, "o/r")])
    }

    fn empty() -> Self {
        Self {
            answers: RefCell::new(BTreeMap::new()),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn user(self, login: Result<&str, &str>) -> Self {
        self.answer(
            &format!("{API}/user"),
            login
                .map(|login| json!({ "login": login }).to_string())
                .map_err(str::to_string),
        )
    }

    fn answer(self, url: &str, answer: Result<String, String>) -> Self {
        self.answers.borrow_mut().insert(url.to_string(), answer);
        self
    }

    /// `items` as GitHub pages them, 100 to a page.
    fn paged(self, url: &str, key: &str, items: &[Value]) -> Self {
        let total = items.len();
        let mut page = 1;
        let mut chunks: Vec<&[Value]> = items.chunks(PER_PAGE).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        for chunk in chunks {
            let body = json!({ "total_count": total, key: chunk }).to_string();
            self.answers
                .borrow_mut()
                .insert(format!("{url}?per_page=100&page={page}"), Ok(body));
            page += 1;
        }
        self
    }

    fn installations(self, items: &[Value]) -> Self {
        self.paged(&format!("{API}/user/installations"), "installations", items)
    }

    fn repositories(self, id: u64, items: &[Value]) -> Self {
        self.paged(
            &format!("{API}/user/installations/{id}/repositories"),
            "repositories",
            items,
        )
    }

    fn addresses(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }
}

impl AppHttp for FakeGitHub {
    fn call(
        &self,
        method: &str,
        url: &str,
        bearer: &str,
        body: Option<&str>,
    ) -> Result<String, String> {
        assert_eq!(method, "GET", "{url}");
        assert!(body.is_none());
        assert!(!bearer.is_empty());
        self.calls
            .borrow_mut()
            .push((url.to_string(), bearer.to_string()));
        self.answers.borrow().get(url).cloned().unwrap_or_else(|| {
            Err(format!(
                "GET {url} failed: curl: (22) The requested URL returned error: 404"
            ))
        })
    }
}

fn perms() -> Value {
    json!({"contents": "read", "metadata": "read", "pull_requests": "write"})
}

fn installation(id: u64, permissions: &Value) -> Value {
    json!({
        "id": id,
        "app_id": 7,
        "client_id": "Iv23client",
        "app_slug": "codeflow-pr-r",
        "account": {"login": "o", "id": 5, "type": "User"},
        "target_id": 5,
        "repository_selection": "selected",
        "permissions": permissions
    })
}

fn repo(id: u64, full_name: &str) -> Value {
    json!({"id": id, "full_name": full_name})
}

fn pr_app() -> PrApp {
    PrApp {
        slug: "codeflow-pr-r".to_string(),
        app_id: 7,
        client_id: "Iv23client".to_string(),
        repository_id: 42,
    }
}

const LOGIN_LINE: &str = "codeflow remote pr-login --repo o/r --app codeflow-pr-r --app-id 7 --client-id Iv23client --repository-id 42";

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
        app: pr_app(),
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

/// Run `pr-login` with a clock that moves by each wait.
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
    let result = pr_login(
        user_store,
        oauth,
        http,
        &Login {
            repository: "o/r",
            now: &now,
            wait: &wait,
            show: &show,
        },
        &pr_app(),
    );
    (result, shown.into_inner(), waits.into_inner())
}

fn check(github: &FakeGitHub) -> Result<(), String> {
    check_grant(github, ACCESS, &pr_app(), "o/r")
}

/// AC-17: the login line carries every id the grant check compares.
#[test]
fn the_login_line_carries_the_ids_recorded_at_setup() {
    assert_eq!(pr_app().login_line("o/r"), LOGIN_LINE);
    assert_eq!(
        authorize_again("o/r", Some(&pr_app())),
        format!("authorize again with `{LOGIN_LINE}`")
    );
}

/// AC-17: the device flow, polled as GitHub asks, sends no client secret,
/// checks the grant, and keeps it with the login GitHub names and the PR
/// App's ids; the code and address are shown once and no token is printed.
#[test]
fn pr_login_runs_the_device_flow_and_keeps_the_checked_grant() {
    let oauth = FakeOAuth::with(vec![
        Ok((200, device_code())),
        Ok((200, json!({"error": "authorization_pending"}))),
        Ok((200, json!({"error": "slow_down", "interval": 10}))),
        Ok((200, granted(ACCESS, REFRESH))),
    ]);
    let store = MemoryStore::default();
    let github = FakeGitHub::good();
    let (result, shown, waits) = login(&store, &oauth, &github);
    let lines = result.unwrap();
    assert!(
        lines[0].starts_with("authorized as dev for o/r"),
        "{lines:?}"
    );
    assert!(lines[1].contains("show dev as author"), "{lines:?}");
    assert!(lines[1].contains("cannot merge or push"), "{lines:?}");
    assert!(
        lines[1].contains("GH_TOKEN=$(codeflow remote pr-token --repo o/r)"),
        "{lines:?}"
    );
    assert_eq!(waits, [5, 5, 10]);
    assert_eq!(shown.len(), 1);
    assert!(shown[0].contains("open https://github.com/login/device and enter the code WDJB-MJHT"));
    let kept: UserGrant = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
    assert_eq!(
        (
            kept.login.as_str(),
            kept.app.clone(),
            kept.refresh_token.as_str()
        ),
        ("dev", pr_app(), REFRESH)
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
    assert_eq!(
        github.addresses(),
        [
            format!("{API}/user"),
            format!("{API}/user/installations?per_page=100&page=1"),
            format!("{API}/user/installations/11/repositories?per_page=100&page=1"),
        ]
    );
    for line in lines.iter().chain(shown.iter()) {
        assert!(
            !line.contains(ACCESS) && !line.contains(REFRESH) && !line.contains("device-secret"),
            "{line}"
        );
    }
}

/// AC-17: GitHub's `device_flow_disabled` names the switch and its pages, and
/// nothing is stored.
#[test]
fn pr_login_names_the_device_flow_switch_when_it_is_off() {
    let oauth = FakeOAuth::with(vec![Ok((
        200,
        json!({"error": "device_flow_disabled", "error_description": "Device flow must be explicitly enabled for this App"}),
    ))]);
    let store = MemoryStore::default();
    let (result, shown, _) = login(&store, &oauth, &FakeGitHub::good());
    let error = result.unwrap_err();
    assert!(
        error
            .contains("under \"Identifying and authorizing users\", select \"Enable Device Flow\""),
        "{error}"
    );
    assert!(
        error.contains("https://github.com/settings/apps/codeflow-pr-r for a personal App"),
        "{error}"
    );
    assert!(
        error.contains("https://github.com/organizations/o/settings/apps/codeflow-pr-r"),
        "{error}"
    );
    assert!(shown.is_empty());
    assert!(store.entries.borrow().is_empty());
}

/// A code nobody enters, a cancelled one, and an App that opted out of
/// expiring tokens each store nothing and say why.
#[test]
fn pr_login_stores_nothing_when_it_is_not_granted() {
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
        let (result, _, _) = login(&store, &oauth, &FakeGitHub::good());
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
    let (result, _, waits) = login(&store, &FakeOAuth::with(all), &FakeGitHub::good());
    let error = result.unwrap_err();
    assert!(error.contains("the code expired"), "{error}");
    assert!(error.contains(LOGIN_LINE), "{error}");
    assert_eq!(waits, [5, 5, 5]);
    assert!(store.entries.borrow().is_empty());
}

/// AC-18: a grant the check refuses is never stored, and the message names
/// what differs without the tokens.
#[test]
fn pr_login_stores_nothing_when_the_grant_check_refuses() {
    let oauth = FakeOAuth::with(vec![
        Ok((200, device_code())),
        Ok((200, granted(ACCESS, REFRESH))),
    ]);
    let store = MemoryStore::default();
    let widened = json!({"contents": "write", "metadata": "read", "pull_requests": "write"});
    let github = FakeGitHub::empty()
        .user(Ok("dev"))
        .installations(&[installation(11, &widened)]);
    let (result, _, _) = login(&store, &oauth, &github);
    let error = result.unwrap_err();
    assert!(error.contains("contents write, not read"), "{error}");
    assert!(error.ends_with("nothing was stored"), "{error}");
    assert!(
        !error.contains(ACCESS) && !error.contains(REFRESH),
        "{error}"
    );
    assert!(store.entries.borrow().is_empty());
}

/// AC-17: a token with time left is confirmed as the developer, checked, and
/// handed back with no renewal; `Debug` never prints it.
#[test]
fn a_fresh_token_is_checked_and_used_without_a_renewal() {
    let store = user_store_with(&grant(10_000, 1_000_000));
    let oauth = FakeOAuth::default();
    let github = FakeGitHub::good();
    let token = pr_token(&store, &oauth, &github, "o/r", 1_000).unwrap();
    assert_eq!(
        (token.token.as_str(), token.login.as_str()),
        (ACCESS, "dev")
    );
    assert!(oauth.forms.borrow().is_empty());
    assert!(!format!("{token:?}").contains(ACCESS));
    assert!(github
        .addresses()
        .contains(&format!("{API}/user/installations?per_page=100&page=1")));
}

/// AC-17: near its end the token is renewed silently, the new refresh token is
/// kept before the new access token is handed back, and the check runs on the
/// new token.
#[test]
fn a_renewal_keeps_the_new_refresh_token_before_it_hands_back_the_token() {
    let store = user_store_with(&grant(1_100, 1_000_000));
    let oauth = FakeOAuth::with(vec![Ok((
        200,
        granted("ghu_access_two", "ghr_refresh_two"),
    ))]);
    let github = FakeGitHub::good();
    let token = pr_token(&store, &oauth, &github, "o/r", 1_000).unwrap();
    assert_eq!(token.token, "ghu_access_two");
    let kept: UserGrant = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
    assert_eq!(kept.refresh_token, "ghr_refresh_two");
    assert_eq!(kept.access_token, "ghu_access_two");
    assert_eq!((kept.login.as_str(), kept.app.clone()), ("dev", pr_app()));
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
    assert!(github
        .calls
        .borrow()
        .iter()
        .all(|(_, bearer)| bearer == "ghu_access_two"));
    // A store that does not keep it: no token is handed back, and the message
    // says the saved grant is spent and gives the whole login line.
    let refusing = MemoryStore {
        refuse: true,
        ..user_store_with(&grant(1_100, 1_000_000))
    };
    let oauth = FakeOAuth::with(vec![Ok((
        200,
        granted("ghu_access_two", "ghr_refresh_two"),
    ))]);
    let error = pr_token(&refusing, &oauth, &FakeGitHub::good(), "o/r", 1_000).unwrap_err();
    assert!(
        error.contains("did not keep it, so the saved one no longer works"),
        "{error}"
    );
    assert!(error.contains(LOGIN_LINE), "{error}");
    assert!(
        !error.contains("ghu_access_two") && !error.contains("ghr_refresh_two"),
        "{error}"
    );
}

/// AC-17: a revoked token (401) leads to a renewal, which GitHub refuses: one
/// message naming `pr-login` with the recorded ids.
#[test]
fn a_revoked_or_expired_grant_says_to_authorize_again() {
    let store = user_store_with(&grant(10_000, 1_000_000));
    let oauth = FakeOAuth::with(vec![Ok((
        200,
        json!({"error": "bad_refresh_token", "error_description": "The refresh token passed is incorrect or expired."}),
    ))]);
    let github = FakeGitHub::empty().user(Err(
        "GET https://api.github.com/user failed: curl: (22) The requested URL returned error: 401",
    ));
    let error = pr_token(&store, &oauth, &github, "o/r", 1_000).unwrap_err();
    assert_eq!(
        error,
        format!("GitHub no longer accepts the saved PR App authorization for o/r (bad_refresh_token): it has expired or was revoked; authorize again with `{LOGIN_LINE}`")
    );
    // Past the refresh token's own end: no call at all.
    let store = user_store_with(&grant(500, 900));
    let oauth = FakeOAuth::default();
    let error = pr_token(&store, &oauth, &FakeGitHub::good(), "o/r", 1_000).unwrap_err();
    assert!(
        error.contains(&format!("has expired; authorize again with `{LOGIN_LINE}`")),
        "{error}"
    );
    assert!(oauth.forms.borrow().is_empty());
    // No grant yet.
    let error = pr_token(
        &MemoryStore::default(),
        &FakeOAuth::default(),
        &FakeGitHub::good(),
        "o/r",
        1_000,
    )
    .unwrap_err();
    assert!(error.contains("no PR App authorization for o/r"), "{error}");
    assert!(error.contains("codeflow remote pr-login"), "{error}");
}

/// AC-17: a network or GitHub failure names its cause, keeps the grant, and
/// never sends the developer to authorize again.
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
        let error = pr_token(&store, &oauth, &FakeGitHub::good(), "o/r", 1_000).unwrap_err();
        assert!(error.contains(said), "{error}");
        assert!(!error.contains(REFRESH) && !error.contains(ACCESS), "{error}");
        if said != "holds no user access token" {
            assert!(!error.contains("pr-login"), "{error}");
            assert_eq!(store.load("o/r").unwrap(), before);
        }
    }
    // `GET /user` failing for a reason that is not 401 is not a refusal.
    let store = user_store_with(&grant(10_000, 1_000_000));
    let github = FakeGitHub::empty().user(Err(
        "GET https://api.github.com/user failed: curl: (7) Failed to connect to api.github.com port 443",
    ));
    let error = pr_token(&store, &FakeOAuth::default(), &github, "o/r", 1_000).unwrap_err();
    assert!(error.contains("cannot reach GitHub"), "{error}");
    assert!(!error.contains("pr-login"), "{error}");
}

/// AC-17: error text redacts both tokens, even where a failure echoes them.
#[test]
fn error_text_never_holds_either_token() {
    let store = user_store_with(&grant(10_000, 1_000_000));
    let github = FakeGitHub::empty().user(Ok("dev")).answer(
        &format!("{API}/user/installations?per_page=100&page=1"),
        Err(format!(
            "GET failed for {ACCESS} after {REFRESH}: curl: (7) Failed to connect"
        )),
    );
    let error = pr_token(&store, &FakeOAuth::default(), &github, "o/r", 1_000).unwrap_err();
    assert!(error.contains("cannot reach GitHub"), "{error}");
    assert!(
        !error.contains(ACCESS) && !error.contains(REFRESH),
        "{error}"
    );
    assert!(!format!(
        "{:?}",
        serde_json::from_str::<UserGrant>(&grant(1, 2)).unwrap()
    )
    .contains(REFRESH));
}

/// AC-18: exactly the three permissions on the recorded App and repository
/// pass, whether the installation selects repositories or holds them all.
#[test]
fn the_grant_check_passes_the_exact_grant() {
    assert_eq!(check(&FakeGitHub::good()), Ok(()));
    let mut all = installation(11, &perms());
    all["repository_selection"] = json!("all");
    let github = FakeGitHub::empty()
        .installations(&[all])
        .repositories(11, &[repo(9, "o/other"), repo(42, "o/r")]);
    assert_eq!(check(&github), Ok(()));
    // An installation that gives no client id is judged by its App id.
    let mut bare = installation(11, &perms());
    bare.as_object_mut().unwrap().remove("client_id");
    let github = FakeGitHub::empty()
        .installations(&[bare])
        .repositories(11, &[repo(42, "o/r")]);
    assert_eq!(check(&github), Ok(()));
}

/// AC-18: any other permission, a missing one, a higher level, a second
/// installation with Contents write, or another App's ids refuses, naming
/// what differs and the installation's page.
#[test]
fn the_grant_check_refuses_any_other_grant() {
    for (permissions, said) in [
        (
            json!({"contents": "read", "metadata": "read", "pull_requests": "write", "checks": "write"}),
            "checks write, which the PR App must not have",
        ),
        (
            json!({"contents": "write", "metadata": "read", "pull_requests": "write"}),
            "contents write, not read",
        ),
        (
            json!({"contents": "read", "pull_requests": "write"}),
            "no metadata read",
        ),
        (
            json!({"contents": "read", "metadata": "read", "pull_requests": "read"}),
            "pull_requests read, not write",
        ),
    ] {
        let github = FakeGitHub::empty()
            .installations(&[installation(11, &permissions)])
            .repositories(11, &[repo(42, "o/r")]);
        let error = check(&github).unwrap_err();
        assert!(error.contains(said), "{said}: {error}");
        assert!(
            error.contains("installation 11 (https://github.com/settings/installations/11)"),
            "{error}"
        );
        assert!(
            error.contains("https://github.com/settings/apps/codeflow-pr-r/permissions"),
            "{error}"
        );
        assert!(error.contains("no token was printed"), "{error}");
    }
    let mut other = installation(12, &perms());
    other["app_id"] = json!(8);
    other["client_id"] = json!("Iv23other");
    other["account"] = json!({"login": "org", "id": 6, "type": "Organization"});
    let github = FakeGitHub::empty()
        .installations(&[installation(11, &perms()), other])
        .repositories(11, &[repo(42, "o/r")]);
    let error = check(&github).unwrap_err();
    assert!(error.contains("App id 8, not 7"), "{error}");
    assert!(
        error.contains("client id Iv23other, not Iv23client"),
        "{error}"
    );
    assert!(
        error.contains("https://github.com/organizations/org/settings/installations/12"),
        "{error}"
    );
    assert!(!error.contains("installation 11"), "{error}");
    // A second installation of the same App with Contents write refuses,
    // though the first is exact and holds the repository.
    let github = FakeGitHub::empty()
        .installations(&[
            installation(11, &perms()),
            installation(
                12,
                &json!({"contents": "write", "metadata": "read", "pull_requests": "write"}),
            ),
        ])
        .repositories(11, &[repo(42, "o/r")]);
    let error = check(&github).unwrap_err();
    assert!(
        error.contains("installation 12 (https://github.com/settings/installations/12): contents write, not read"),
        "{error}"
    );
    assert!(!error.contains("installation 11"), "{error}");
    // The App id alone, and the client id alone, differing each refuse.
    for (field, value, said) in [
        ("app_id", json!(8), "App id 8, not 7"),
        (
            "client_id",
            json!("Iv23other"),
            "client id Iv23other, not Iv23client",
        ),
    ] {
        let mut differs = installation(11, &perms());
        differs[field] = value;
        let github = FakeGitHub::empty()
            .installations(&[differs])
            .repositories(11, &[repo(42, "o/r")]);
        let error = check(&github).unwrap_err();
        assert!(error.contains(said), "{field}: {error}");
        assert!(error.contains("no token was printed"), "{field}: {error}");
    }
    // No installation at all.
    let error = check(&FakeGitHub::empty().installations(&[])).unwrap_err();
    assert!(error.contains("installed nowhere"), "{error}");
}

/// AC-18: every page is read: a widened installation on page two refuses,
/// and the repository found only on a later page passes.
#[test]
fn the_grant_check_reads_every_page() {
    let mut installations: Vec<Value> = (1..=100).map(|id| installation(id, &perms())).collect();
    installations.push(installation(
        101,
        &json!({"contents": "read", "metadata": "read", "pull_requests": "write", "issues": "write"}),
    ));
    let github = FakeGitHub::empty().installations(&installations);
    let error = check(&github).unwrap_err();
    assert!(error.contains("installation 101"), "{error}");
    assert!(error.contains("issues write"), "{error}");
    assert!(github
        .addresses()
        .contains(&format!("{API}/user/installations?per_page=100&page=2")));

    let repositories: Vec<Value> = (1..=150)
        .map(|id| repo(id + 1_000, "o/x"))
        .chain([repo(42, "o/r")])
        .collect();
    let github = FakeGitHub::empty()
        .installations(&[installation(11, &perms())])
        .repositories(11, &repositories);
    assert_eq!(check(&github), Ok(()));
    assert!(github.addresses().contains(&format!(
        "{API}/user/installations/11/repositories?per_page=100&page=2"
    )));
}

/// AC-18: the repository is matched by the id recorded at setup, never by
/// name: a repository of the same name with another id refuses.
#[test]
fn the_grant_check_matches_the_repository_by_id() {
    let github = FakeGitHub::empty()
        .installations(&[installation(11, &perms()), installation(12, &perms())])
        .repositories(11, &[repo(43, "o/r")])
        .repositories(12, &[]);
    let error = check(&github).unwrap_err();
    assert!(
        error.contains("the repository id 42 recorded at setup is not among"),
        "{error}"
    );
}

/// AC-18: a page that is missing, short, unreadable or changes its count
/// refuses, so the check never judges part of a list.
#[test]
fn the_grant_check_refuses_a_list_it_cannot_read_whole() {
    let installations = format!("{API}/user/installations?per_page=100&page=1");
    let page = |total: u64, items: Vec<Value>| {
        Ok(json!({"total_count": total, "installations": items}).to_string())
    };
    for (github, said) in [
        (
            FakeGitHub::empty().answer(
                &installations,
                Err("GET failed: curl: (22) The requested URL returned error: 500".to_string()),
            ),
            "GitHub failed with 500",
        ),
        (
            FakeGitHub::empty().answer(&installations, Ok("<html>".to_string())),
            "is not a list it can be judged from",
        ),
        (
            FakeGitHub::empty()
                .answer(&installations, Ok(json!({"installations": []}).to_string())),
            "is not a list it can be judged from",
        ),
        (
            FakeGitHub::empty()
                .answer(&installations, page(2, vec![installation(1, &perms())]))
                .answer(
                    &format!("{API}/user/installations?per_page=100&page=2"),
                    page(2, vec![]),
                ),
            "stops at page 2 with 1 of 2 read",
        ),
        (
            FakeGitHub::empty()
                .answer(&installations, page(2, vec![installation(1, &perms())]))
                .answer(
                    &format!("{API}/user/installations?per_page=100&page=2"),
                    page(3, vec![installation(2, &perms())]),
                ),
            "changed while it was read",
        ),
        (
            FakeGitHub::empty().answer(
                &installations,
                page(
                    1,
                    vec![installation(1, &perms()), installation(2, &perms())],
                ),
            ),
            "is not a list it can be judged from",
        ),
        (
            FakeGitHub::empty().answer(&installations, page(2, vec![installation(1, &perms())])),
            "GitHub answered 404",
        ),
    ] {
        let error = check(&github).unwrap_err();
        assert!(error.contains(said), "{said}: {error}");
        assert!(error.contains("no token was printed"), "{error}");
    }
    // A repository page that cannot be read refuses too.
    let github = FakeGitHub::empty()
        .installations(&[installation(11, &perms())])
        .answer(
            &format!("{API}/user/installations/11/repositories?per_page=100&page=1"),
            Err("GET failed: curl: (22) The requested URL returned error: 403".to_string()),
        );
    let error = check(&github).unwrap_err();
    assert!(error.contains("GitHub forbids this request"), "{error}");
    assert!(error.contains("installation 11's repositories"), "{error}");
}

/// AC-18: a widened App stops the next print, even with a cached token.
#[test]
fn a_widened_app_stops_the_next_print() {
    let store = user_store_with(&grant(10_000, 1_000_000));
    let widened = json!({"contents": "read", "metadata": "read", "pull_requests": "write", "actions": "write"});
    let github = FakeGitHub::empty()
        .user(Ok("dev"))
        .installations(&[installation(11, &widened)])
        .repositories(11, &[repo(42, "o/r")]);
    let error = pr_token(&store, &FakeOAuth::default(), &github, "o/r", 1_000).unwrap_err();
    assert!(error.contains("actions write"), "{error}");
    assert!(!error.contains(ACCESS), "{error}");
}

/// AC-17: one renewal at a time: while one session holds the lock, a second
/// cannot take it, and the lock file stays empty.
#[test]
fn the_refresh_lock_lets_one_holder_at_a_time() {
    use fs2::FileExt;
    let home = tempfile::tempdir().unwrap();
    let held = refresh_lock(home.path(), "o/r").unwrap();
    let path = home.path().join("locks/pr-user-o--r.lock");
    let second = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(second.try_lock_exclusive().is_err());
    drop(held);
    assert!(second.try_lock_exclusive().is_ok());
    assert_eq!(std::fs::read(&path).unwrap(), b"");
}
