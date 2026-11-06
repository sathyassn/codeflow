//! The agent App's private key: kept in the operating system's credential
//! store and used only to mint short-lived installation tokens (TSK-1348
//! AC-8).
//!
//! The key never reaches a file. `codeflow remote setup-apps` hands it to
//! the store on standard input. `codeflow remote agent-token` reads it back
//! into memory, signs the App's JSON Web Token with `openssl` reading the
//! key from standard input, and sends that token to GitHub with `curl`
//! reading the authorization header from standard input, so neither the key
//! nor the JSON Web Token appears in a file or a process argument list. Only
//! the installation token, which expires after one hour, leaves the command,
//! and every error is redacted of the key and the JSON Web Token.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The credential store service name under which the agent App's entry is
/// kept; the account is the repository, `owner/name`.
pub const AGENT_KEY_SERVICE: &str = "codeflow-agent-app";

/// GitHub's REST API root.
const API: &str = "https://api.github.com";

/// What the store keeps for one repository's agent App: its ids and its
/// private key. `Debug` never prints the key.
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentApp {
    /// The App's numeric id.
    pub app_id: u64,
    /// The App's client id, the JSON Web Token's issuer.
    pub client_id: String,
    /// The App's slug; its bot login is `<slug>[bot]`.
    pub slug: String,
    /// The App's private key, PEM.
    pub pem: String,
}

impl std::fmt::Debug for AgentApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentApp")
            .field("app_id", &self.app_id)
            .field("client_id", &self.client_id)
            .field("slug", &self.slug)
            .field("pem", &"[redacted]")
            .finish()
    }
}

/// A place outside the file system that keeps one secret per repository.
pub trait KeyStore {
    /// The store's name, for messages: "the macOS login keychain".
    fn describe(&self) -> String;

    /// Whether the store can be used here.
    ///
    /// # Errors
    ///
    /// Why it cannot, naming what is missing.
    fn available(&self) -> Result<(), String>;

    /// Whether an entry for `account` exists.
    ///
    /// # Errors
    ///
    /// When the store cannot be read.
    fn exists(&self, account: &str) -> Result<bool, String>;

    /// Keep `secret` for `account`.
    ///
    /// # Errors
    ///
    /// When the store refuses or cannot be reached.
    fn store(&self, account: &str, secret: &str) -> Result<(), String>;

    /// The secret kept for `account`.
    ///
    /// # Errors
    ///
    /// When there is none, or the store cannot be read.
    fn load(&self, account: &str) -> Result<String, String>;

    /// The command that removes the entry for `account`.
    fn undo(&self, account: &str) -> String;
}

/// This computer's credential store: the login keychain on macOS, the
/// Secret Service (`secret-tool`) on other Unix systems. Windows has none
/// yet, so the agent App is refused there before anything is created.
#[must_use]
pub fn os_store() -> Box<dyn KeyStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(Keychain::login())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(SecretService)
    }
    #[cfg(not(unix))]
    {
        Box::new(Unsupported)
    }
}

/// The environment a helper is started from: the variables `codeflow`
/// itself was given, or a fixed list in a test.
type Inherited = Vec<(OsString, OsString)>;

fn inherited(fixed: Option<&Inherited>) -> Inherited {
    fixed
        .cloned()
        .unwrap_or_else(|| std::env::vars_os().collect())
}

/// Start a helper that sees a key or a token with an empty environment plus
/// only the variables in `keep`. A variable that changes what the helper
/// loads or reads (`PATH`, `DYLD_INSERT_LIBRARIES`, `LD_PRELOAD`,
/// `LD_LIBRARY_PATH`, `OPENSSL_CONF`, `CURL_HOME`, `HOME`) never reaches it,
/// and the helper itself is an absolute path, never found through `PATH`.
fn confine(command: &mut Command, inherited: &[(OsString, OsString)], keep: &[&str]) {
    command.env_clear();
    for (name, value) in inherited {
        let loader = name
            .to_str()
            .is_none_or(|name| name.starts_with("DYLD_") || name.starts_with("LD_"));
        if !loader && keep.iter().any(|kept| name == kept) {
            command.env(name, value);
        }
    }
}

/// A helper's output, for a message only: it is shown, never compared, and
/// every caller that may echo a key passes it through [`redact`].
fn helper_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn run(command: &mut Command, input: Option<&[u8]>) -> Result<Output, String> {
    command.stdin(Stdio::null());
    match input {
        Some(data) => crate::git::output_with_input(command, data),
        None => command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    }
    .map_err(|error| format!("cannot run {}: {error}", command.get_program().display()))
}

/// The macOS keychain, through `/usr/bin/security`.
#[cfg(target_os = "macos")]
pub struct Keychain {
    keychain: Option<PathBuf>,
}

#[cfg(target_os = "macos")]
impl Keychain {
    /// The user's default (login) keychain.
    #[must_use]
    pub fn login() -> Self {
        Self { keychain: None }
    }

    /// A named keychain file, for a scratch keychain in tests.
    #[must_use]
    pub fn at(keychain: &Path) -> Self {
        Self {
            keychain: Some(keychain.to_path_buf()),
        }
    }

    fn find(&self, account: &str, password: bool) -> Command {
        let mut command = Command::new("/usr/bin/security");
        command.args([
            "find-generic-password",
            "-s",
            AGENT_KEY_SERVICE,
            "-a",
            account,
        ]);
        if password {
            command.arg("-w");
        }
        if let Some(keychain) = &self.keychain {
            command.arg(keychain);
        }
        command
    }
}

/// The `security -i` line that adds the entry: the secret travels as hex on
/// standard input, never in the argument list `ps` shows.
#[cfg(any(target_os = "macos", test))]
fn keychain_add_line(account: &str, secret: &str, keychain: Option<&Path>) -> String {
    let hex = secret.bytes().fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    let keychain = keychain.map_or_else(String::new, |path| format!(" {}", path.display()));
    format!("add-generic-password -s {AGENT_KEY_SERVICE} -a {account} -l {AGENT_KEY_SERVICE} -X {hex}{keychain}\n")
}

#[cfg(target_os = "macos")]
impl KeyStore for Keychain {
    fn describe(&self) -> String {
        match &self.keychain {
            None => "the macOS login keychain".to_string(),
            Some(path) => format!("the macOS keychain {}", path.display()),
        }
    }

    fn available(&self) -> Result<(), String> {
        if Path::new("/usr/bin/security").is_file() {
            Ok(())
        } else {
            Err("/usr/bin/security is missing".to_string())
        }
    }

    fn exists(&self, account: &str) -> Result<bool, String> {
        let output = run(&mut self.find(account, false), None)?;
        if output.status.success() {
            return Ok(true);
        }
        // `security` exits 44 (errSecItemNotFound) for no such entry; any
        // other failure is an error, never "absent".
        if output.status.code() == Some(44) {
            Ok(false)
        } else {
            Err(format!(
                "cannot read {}: {}",
                self.describe(),
                helper_text(&output.stderr)
            ))
        }
    }

    fn store(&self, account: &str, secret: &str) -> Result<(), String> {
        let line = keychain_add_line(account, secret, self.keychain.as_deref());
        let output = run(
            Command::new("/usr/bin/security").arg("-i"),
            Some(line.as_bytes()),
        )?;
        if self.exists(account)? {
            Ok(())
        } else {
            Err(redact(
                &format!(
                    "{} did not keep the entry: {}",
                    self.describe(),
                    helper_text(&output.stderr)
                ),
                &[secret],
            ))
        }
    }

    fn load(&self, account: &str) -> Result<String, String> {
        let output = run(&mut self.find(account, true), None)?;
        if !output.status.success() {
            return Err(format!(
                "no agent App entry for {account} in {} ({})",
                self.describe(),
                helper_text(&output.stderr)
            ));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| format!("the entry for {account} is not text"))?;
        // `security -w` ends its answer with one newline.
        let text = text.strip_suffix('\n').unwrap_or(text.as_str());
        // `security -w` prints data it judges not printable as hex.
        if text.starts_with('{') {
            Ok(text.to_string())
        } else {
            decode_hex(text).ok_or_else(|| format!("the entry for {account} is not readable"))
        }
    }

    fn undo(&self, account: &str) -> String {
        let keychain = self
            .keychain
            .as_ref()
            .map_or_else(String::new, |path| format!(" {}", path.display()));
        format!("security delete-generic-password -s {AGENT_KEY_SERVICE} -a {account}{keychain}")
    }
}

#[cfg(any(target_os = "macos", test))]
fn decode_hex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// The freedesktop Secret Service, through `secret-tool`.
#[cfg(all(unix, not(target_os = "macos")))]
pub struct SecretService;

#[cfg(all(unix, not(target_os = "macos")))]
impl SecretService {
    /// `/usr/bin/secret-tool`, never one found through `PATH`, with only the
    /// variables that reach the session bus. It is handed the key on
    /// standard input when it stores it.
    fn command() -> Result<Command, String> {
        if !Path::new("/usr/bin/secret-tool").is_file() {
            return Err("/usr/bin/secret-tool (libsecret) is not installed".to_string());
        }
        let mut command = Command::new("/usr/bin/secret-tool");
        confine(
            &mut command,
            &inherited(None),
            &["DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"],
        );
        Ok(command)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl KeyStore for SecretService {
    fn describe(&self) -> String {
        "the Secret Service keyring".to_string()
    }

    fn available(&self) -> Result<(), String> {
        Self::command().map(|_| ())
    }

    fn exists(&self, account: &str) -> Result<bool, String> {
        let output = run(
            Self::command()?.args(["lookup", "service", AGENT_KEY_SERVICE, "account", account]),
            None,
        )?;
        Ok(output.status.success() && !output.stdout.is_empty())
    }

    fn store(&self, account: &str, secret: &str) -> Result<(), String> {
        let output = run(
            Self::command()?.args([
                "store",
                &format!("--label={AGENT_KEY_SERVICE}"),
                "service",
                AGENT_KEY_SERVICE,
                "account",
                account,
            ]),
            Some(secret.as_bytes()),
        )?;
        if output.status.success() {
            Ok(())
        } else {
            Err(redact(
                &format!(
                    "secret-tool did not keep the entry: {}",
                    helper_text(&output.stderr)
                ),
                &[secret],
            ))
        }
    }

    fn load(&self, account: &str) -> Result<String, String> {
        let output = run(
            Self::command()?.args(["lookup", "service", AGENT_KEY_SERVICE, "account", account]),
            None,
        )?;
        if !output.status.success() || output.stdout.is_empty() {
            return Err(format!(
                "no agent App entry for {account} in {}",
                self.describe()
            ));
        }
        String::from_utf8(output.stdout).map_err(|_| format!("the entry for {account} is not text"))
    }

    fn undo(&self, account: &str) -> String {
        format!("secret-tool clear service {AGENT_KEY_SERVICE} account {account}")
    }
}

/// A platform with no supported store: everything is refused.
#[cfg(not(unix))]
pub struct Unsupported;

#[cfg(not(unix))]
impl KeyStore for Unsupported {
    fn describe(&self) -> String {
        "no credential store".to_string()
    }

    fn available(&self) -> Result<(), String> {
        Err("this platform has no credential store CodeFlow can keep the agent App's key in yet (macOS keychain or Linux Secret Service)".to_string())
    }

    fn exists(&self, _: &str) -> Result<bool, String> {
        Err(self.available().unwrap_err())
    }

    fn store(&self, _: &str, _: &str) -> Result<(), String> {
        Err(self.available().unwrap_err())
    }

    fn load(&self, _: &str) -> Result<String, String> {
        Err(self.available().unwrap_err())
    }

    fn undo(&self, _: &str) -> String {
        String::new()
    }
}

/// Signs data with an RSA key, SHA-256 (RS256).
pub trait Signer {
    /// The signature of `data` under `pem`.
    ///
    /// # Errors
    ///
    /// When signing fails.
    fn sign_rs256(&self, pem: &str, data: &[u8]) -> Result<Vec<u8>, String>;
}

/// `/usr/bin/openssl dgst -sha256 -sign /dev/stdin`: the key on standard
/// input, the data (not secret) in a file in a private temporary folder.
/// The program is never found through `PATH`, and it runs with an empty
/// environment, so `OPENSSL_CONF` or a preloaded library cannot reach the
/// process that reads the key.
pub struct OpensslSigner {
    scratch: PathBuf,
    inherited: Option<Inherited>,
}

impl OpensslSigner {
    /// Use `/usr/bin/openssl`, with temporary folders under `scratch`.
    ///
    /// # Errors
    ///
    /// When `/usr/bin/openssl` is not installed.
    pub fn discover(scratch: &Path) -> Result<Self, String> {
        if !Path::new("/usr/bin/openssl").is_file() {
            return Err("/usr/bin/openssl is not installed".to_string());
        }
        Ok(Self {
            scratch: scratch.to_path_buf(),
            inherited: None,
        })
    }
}

impl Signer for OpensslSigner {
    fn sign_rs256(&self, pem: &str, data: &[u8]) -> Result<Vec<u8>, String> {
        let dir = tempfile::Builder::new()
            .prefix("codeflow-jwt-")
            .tempdir_in(&self.scratch)
            .map_err(|error| format!("cannot make a temporary folder: {error}"))?;
        let path = dir.path().join("signing-input");
        std::fs::write(&path, data)
            .map_err(|error| format!("cannot write the signing input: {error}"))?;
        let mut command = Command::new("/usr/bin/openssl");
        confine(&mut command, &inherited(self.inherited.as_ref()), &[]);
        command
            .args(["dgst", "-sha256", "-sign", "/dev/stdin"])
            .arg(&path);
        let output = run(&mut command, Some(pem.as_bytes()))?;
        if output.status.success() && !output.stdout.is_empty() {
            Ok(output.stdout)
        } else {
            Err(redact(
                &format!(
                    "openssl could not sign the App's token: {}",
                    helper_text(&output.stderr)
                ),
                &[pem],
            ))
        }
    }
}

/// An HTTPS call to GitHub's API with a bearer token.
pub trait AppHttp {
    /// The response body of `method url`, sent with `bearer`.
    ///
    /// # Errors
    ///
    /// On a transport failure or a status that is not success.
    fn call(
        &self,
        method: &str,
        url: &str,
        bearer: &str,
        body: Option<&str>,
    ) -> Result<String, String>;
}

/// `/usr/bin/curl -q`, with the headers on standard input (`-H @-`). The
/// program is never found through `PATH`; `-q` comes first, so no `.curlrc`
/// is read, and curl runs with an empty environment, so no `CURL_HOME`,
/// `HOME` or proxy variable can add a proxy or a trace that would record
/// the App's JSON Web Token or the installation token.
pub struct CurlHttp {
    inherited: Option<Inherited>,
}

impl CurlHttp {
    /// Use `/usr/bin/curl`.
    ///
    /// # Errors
    ///
    /// When `/usr/bin/curl` is not installed.
    pub fn discover() -> Result<Self, String> {
        if !Path::new("/usr/bin/curl").is_file() {
            return Err("/usr/bin/curl is not installed".to_string());
        }
        Ok(Self { inherited: None })
    }
}

impl AppHttp for CurlHttp {
    fn call(
        &self,
        method: &str,
        url: &str,
        bearer: &str,
        body: Option<&str>,
    ) -> Result<String, String> {
        let headers = format!(
            "Authorization: Bearer {bearer}\nAccept: application/vnd.github+json\nX-GitHub-Api-Version: 2022-11-28\nUser-Agent: codeflow\n"
        );
        let mut command = Command::new("/usr/bin/curl");
        confine(&mut command, &inherited(self.inherited.as_ref()), &[]);
        command.args([
            "-q",
            "-sS",
            "--fail-with-body",
            "--max-time",
            "30",
            "-X",
            method,
            "-H",
            "@-",
        ]);
        if let Some(body) = body {
            command.args(["--data-binary", body]);
        }
        command.arg(url);
        let output = run(&mut command, Some(headers.as_bytes()))?;
        if output.status.success() {
            String::from_utf8(output.stdout)
                .map_err(|_| format!("the answer to {method} {url} is not UTF-8"))
        } else {
            Err(redact(
                &format!(
                    "{method} {url} failed: {}{}",
                    helper_text(&output.stdout),
                    helper_text(&output.stderr)
                ),
                &[bearer],
            ))
        }
    }
}

/// A minted installation token. `Debug` never prints the token.
pub struct Minted {
    /// The installation token.
    pub token: String,
    /// When GitHub expires it, as GitHub wrote it.
    pub expires_at: String,
}

impl std::fmt::Debug for Minted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Minted")
            .field("token", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// `text` with every secret, every line of the key and any line naming a
/// private key replaced, so an error can be shown.
#[must_use]
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        out = out.replace(secret, "[redacted]");
        for line in secret
            .lines()
            .map(str::trim)
            .filter(|line| line.len() >= 16)
        {
            out = out.replace(line, "[redacted]");
        }
    }
    out.lines()
        .map(|line| {
            if line.contains("PRIVATE KEY") {
                "[redacted]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The App's JSON Web Token: RS256, issued 60 seconds in the past, valid
/// for nine minutes (GitHub accepts at most ten), issuer the client id.
fn app_jwt(app: &AgentApp, signer: &dyn Signer, now: u64) -> Result<String, String> {
    signed_app_jwt(&app.client_id, &app.pem, signer, now)
}

/// [`app_jwt`] for an App known by its client id and key, such as one
/// `setup-apps` has just created.
pub(crate) fn signed_app_jwt(
    client_id: &str,
    pem: &str,
    signer: &dyn Signer,
    now: u64,
) -> Result<String, String> {
    let header = b64url(json!({"alg": "RS256", "typ": "JWT"}).to_string().as_bytes());
    let claims = json!({"iat": now.saturating_sub(60), "exp": now + 540, "iss": client_id});
    let input = format!("{header}.{}", b64url(claims.to_string().as_bytes()));
    let signature = signer.sign_rs256(pem, input.as_bytes())?;
    Ok(format!("{input}.{}", b64url(&signature)))
}

/// GitHub's REST API root, for callers in this module's parent.
pub(crate) const API_ROOT: &str = API;

/// Mint an installation token of the agent App for `repository`
/// (`owner/name`), scoped to that repository. The key is read from `store`
/// and never leaves this function; errors are redacted of the key and the
/// App's JSON Web Token.
///
/// # Errors
///
/// When there is no entry, the key cannot sign, the App is not installed on
/// the repository, or GitHub refuses.
pub fn mint(
    store: &dyn KeyStore,
    signer: &dyn Signer,
    http: &dyn AppHttp,
    repository: &str,
    now: u64,
) -> Result<Minted, String> {
    let Some((_, name)) = repository.split_once('/') else {
        return Err(format!("'{repository}' is not owner/name"));
    };
    store.available()?;
    let raw = store.load(repository)?;
    let app: AgentApp = serde_json::from_str(&raw).map_err(|_| {
        format!(
            "the agent App entry for {repository} in {} is not readable; remove it ({}) and run `codeflow remote setup-apps` again",
            store.describe(),
            store.undo(repository)
        )
    })?;
    let jwt = app_jwt(&app, signer, now).map_err(|error| redact(&error, &[&app.pem]))?;
    let secrets = [app.pem.as_str(), jwt.as_str()];
    let installation = http
        .call("GET", &format!("{API}/repos/{repository}/installation"), &jwt, None)
        .map_err(|error| {
            redact(
                &format!(
                    "the agent App {} has no installation on {repository} that GitHub shows: {error}; install it at https://github.com/apps/{}/installations/new",
                    app.slug, app.slug
                ),
                &secrets,
            )
        })?;
    let id = serde_json::from_str::<Value>(&installation)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
        .ok_or_else(|| format!("GitHub's answer for the {repository} installation has no id"))?;
    let body = json!({"repositories": [name]}).to_string();
    let answer = http
        .call(
            "POST",
            &format!("{API}/app/installations/{id}/access_tokens"),
            &jwt,
            Some(&body),
        )
        .map_err(|error| {
            redact(
                &format!("GitHub refused the installation token: {error}"),
                &secrets,
            )
        })?;
    let answer: Value = serde_json::from_str(&answer)
        .map_err(|_| "GitHub's installation token answer is not JSON".to_string())?;
    let token = answer
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitHub's answer carries no token".to_string())?
        .to_string();
    let expires_at = answer
        .get("expires_at")
        .and_then(Value::as_str)
        .unwrap_or("in one hour")
        .to_string();
    Ok(Minted { token, expires_at })
}

/// Whether a git credential request (Git's `key=value` lines on standard
/// input) is for `https://github.com`. Any other host, including one an
/// `insteadOf` rewrite points at, gets no answer and no token is minted.
#[must_use]
pub fn credential_request_is_github(request: &str) -> bool {
    let mut protocol = None;
    let mut host = None;
    for line in request.split('\n') {
        match line.strip_suffix('\r').unwrap_or(line).split_once('=') {
            Some(("protocol", value)) => protocol = Some(value),
            Some(("host", value)) => host = Some(value),
            _ => {}
        }
    }
    protocol == Some("https") && host == Some("github.com")
}

/// What a git credential helper prints for `get`: the installation token
/// as the password of the `x-access-token` user.
#[must_use]
pub fn git_credential_answer(token: &str) -> String {
    format!("username=x-access-token\npassword={token}\n")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    // A made-up key: the header is split so no source line holds a key block.
    const PEM: &str = concat!(
        "-----BEGIN RSA PRIVATE",
        " KEY-----\nMIIEowIBAAKCAQEAsecretkeymaterialline1\nZZZZsecretkeymaterialline2ZZZZ\n-----END RSA PRIVATE",
        " KEY-----\n"
    );

    /// A store that keeps entries in memory.
    #[derive(Default)]
    pub(crate) struct MemoryStore {
        pub(crate) entries: RefCell<BTreeMap<String, String>>,
        pub(crate) refuse: bool,
    }

    impl KeyStore for MemoryStore {
        fn describe(&self) -> String {
            "a test store".to_string()
        }
        fn available(&self) -> Result<(), String> {
            Ok(())
        }
        fn exists(&self, account: &str) -> Result<bool, String> {
            Ok(self.entries.borrow().contains_key(account))
        }
        fn store(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.refuse {
                return Err("the test store refuses".to_string());
            }
            self.entries
                .borrow_mut()
                .insert(account.to_string(), secret.to_string());
            Ok(())
        }
        fn load(&self, account: &str) -> Result<String, String> {
            self.entries
                .borrow()
                .get(account)
                .cloned()
                .ok_or_else(|| "none".to_string())
        }
        fn undo(&self, account: &str) -> String {
            format!("forget {account}")
        }
    }

    struct FakeSigner(Result<Vec<u8>, String>);
    impl Signer for FakeSigner {
        fn sign_rs256(&self, pem: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            assert_eq!(pem, PEM);
            self.0.clone()
        }
    }

    /// Answers by method; records the bearer it was sent.
    struct FakeHttp {
        installation: Result<String, String>,
        token: Result<String, String>,
        bearers: RefCell<Vec<String>>,
    }
    impl AppHttp for FakeHttp {
        fn call(
            &self,
            method: &str,
            url: &str,
            bearer: &str,
            body: Option<&str>,
        ) -> Result<String, String> {
            self.bearers.borrow_mut().push(bearer.to_string());
            if method == "GET" {
                assert_eq!(url, "https://api.github.com/repos/o/r/installation");
                self.installation.clone()
            } else {
                assert_eq!(
                    url,
                    "https://api.github.com/app/installations/77/access_tokens"
                );
                assert_eq!(body, Some(r#"{"repositories":["r"]}"#));
                self.token.clone()
            }
        }
    }

    fn stored() -> MemoryStore {
        let store = MemoryStore::default();
        let app = AgentApp {
            app_id: 5,
            client_id: "Iv1.client".to_string(),
            slug: "codeflow-agent-r".to_string(),
            pem: PEM.to_string(),
        };
        store
            .store("o/r", &serde_json::to_string(&app).unwrap())
            .unwrap();
        store
    }

    fn http(installation: Result<&str, &str>, token: Result<&str, &str>) -> FakeHttp {
        FakeHttp {
            installation: installation.map(str::to_string).map_err(str::to_string),
            token: token.map(str::to_string).map_err(str::to_string),
            bearers: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn mint_returns_only_the_installation_token() {
        let store = stored();
        let http = http(
            Ok(r#"{"id":77}"#),
            Ok(r#"{"token":"ghs_minted","expires_at":"2026-11-05T20:00:00Z"}"#),
        );
        let minted = mint(
            &store,
            &FakeSigner(Ok(vec![1, 2, 3])),
            &http,
            "o/r",
            1_000_000,
        )
        .unwrap();
        assert_eq!(minted.token, "ghs_minted");
        assert_eq!(minted.expires_at, "2026-11-05T20:00:00Z");
        let shown = format!("{minted:?}");
        assert!(!shown.contains("ghs_minted") && !shown.contains("secretkeymaterial"));
        // The JSON Web Token: issuer the client id, inside GitHub's ten
        // minutes, and the same bearer for both calls.
        let bearers = http.bearers.borrow();
        assert_eq!(bearers.len(), 2);
        assert_eq!(bearers[0], bearers[1]);
        let claims = bearers[0].split('.').nth(1).unwrap();
        let claims: Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(claims)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["iss"], "Iv1.client");
        let span = claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap();
        assert!(span <= 600, "{span}");
        assert!(!bearers[0].contains("secretkeymaterial"));
        assert_eq!(
            git_credential_answer(&minted.token),
            "username=x-access-token\npassword=ghs_minted\n"
        );
    }

    #[test]
    fn mint_errors_never_carry_the_key_or_the_app_token() {
        // A transport that echoes what it was sent, key and bearer.
        struct Echo;
        impl AppHttp for Echo {
            fn call(
                &self,
                _: &str,
                _: &str,
                bearer: &str,
                _: Option<&str>,
            ) -> Result<String, String> {
                Err(format!(
                    "401 for Authorization: Bearer {bearer}; key was {PEM}"
                ))
            }
        }
        let store = stored();
        let error = mint(&store, &FakeSigner(Ok(vec![9; 8])), &Echo, "o/r", 1_000_000).unwrap_err();
        assert!(error.contains("[redacted]"), "{error}");
        assert!(!error.contains("secretkeymaterial"), "{error}");
        assert!(!error.contains("PRIVATE KEY"), "{error}");
        let header = b64url(json!({"alg": "RS256", "typ": "JWT"}).to_string().as_bytes());
        assert!(!error.contains(&header), "{error}");

        let refused = http(Ok(r#"{"id":77}"#), Err("403 nope"));
        let error = mint(
            &store,
            &FakeSigner(Ok(vec![9; 8])),
            &refused,
            "o/r",
            1_000_000,
        )
        .unwrap_err();
        assert!(error.contains("refused the installation token"), "{error}");
        assert!(!error.contains("secretkeymaterial"), "{error}");

        let error = mint(
            &store,
            &FakeSigner(Err(format!("bad key {PEM}"))),
            &http(Ok("{}"), Ok("{}")),
            "o/r",
            1_000_000,
        )
        .unwrap_err();
        assert!(!error.contains("secretkeymaterial"), "{error}");
        assert!(!error.contains("PRIVATE KEY"), "{error}");
    }

    #[test]
    fn mint_names_a_missing_entry_and_never_shows_the_entry() {
        let error = mint(
            &MemoryStore::default(),
            &FakeSigner(Ok(vec![1])),
            &http(Ok("{}"), Ok("{}")),
            "o/r",
            1,
        )
        .unwrap_err();
        assert_eq!(error, "none");
        let store = MemoryStore::default();
        store.store("o/r", "not json secretkeymaterial").unwrap();
        let error = mint(
            &store,
            &FakeSigner(Ok(vec![1])),
            &http(Ok("{}"), Ok("{}")),
            "o/r",
            1,
        )
        .unwrap_err();
        assert!(
            error.contains("not readable") && !error.contains("secretkeymaterial"),
            "{error}"
        );
    }

    #[test]
    fn agent_app_debug_hides_the_key() {
        let app = AgentApp {
            app_id: 1,
            client_id: "c".to_string(),
            slug: "s".to_string(),
            pem: PEM.to_string(),
        };
        assert!(!format!("{app:?}").contains("secretkeymaterial"));
    }

    #[test]
    fn keychain_line_carries_the_secret_on_stdin_as_hex_only() {
        let line = keychain_add_line("o/r", "{\"pem\":\"x y\"}", None);
        assert!(line.starts_with("add-generic-password -s codeflow-agent-app -a o/r "));
        assert!(!line.contains("pem"));
        let hex = line.split(" -X ").nth(1).unwrap().trim();
        assert_eq!(decode_hex(hex).as_deref(), Some("{\"pem\":\"x y\"}"));
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
    }

    /// The real signer, with a throwaway key made for this test: it signs
    /// with the key on standard input and leaves no file behind, after a
    /// signature and after a refused key.
    #[cfg(unix)]
    #[test]
    fn openssl_signer_leaves_no_file_after_success_or_failure() {
        let scratch = tempfile::tempdir().unwrap();
        let Ok(signer) = OpensslSigner::discover(scratch.path()) else {
            eprintln!("openssl not installed; skipped");
            return;
        };
        let pem = throwaway_key();
        let signature = signer.sign_rs256(&pem, b"header.payload").unwrap();
        assert_eq!(signature.len(), 256);
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
        let error = signer
            .sign_rs256("not a key", b"header.payload")
            .unwrap_err();
        assert!(error.contains("could not sign"), "{error}");
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    /// A 2048-bit RSA key made for one test.
    #[cfg(unix)]
    fn throwaway_key() -> String {
        let made = Command::new("/usr/bin/openssl")
            .args(["genrsa", "2048"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(made.status.success());
        String::from_utf8(made.stdout).unwrap()
    }

    fn variables(pairs: &[(&str, &Path)]) -> Inherited {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), value.as_os_str().to_os_string()))
            .collect()
    }

    /// Review F1: the helper that reads the key keeps only the variables it
    /// is given leave to keep, and never a loader variable.
    #[test]
    fn confine_passes_on_only_the_listed_variables() {
        let path = Path::new("/x");
        let inherited = variables(&[
            ("PATH", path),
            ("HOME", path),
            ("OPENSSL_CONF", path),
            ("DYLD_INSERT_LIBRARIES", path),
            ("LD_PRELOAD", path),
            ("DBUS_SESSION_BUS_ADDRESS", path),
        ]);
        let mut command = Command::new("/usr/bin/true");
        confine(
            &mut command,
            &inherited,
            &["DBUS_SESSION_BUS_ADDRESS", "LD_PRELOAD"],
        );
        let kept: Vec<_> = command
            .get_envs()
            .filter_map(|(name, value)| {
                value.map(|value| (name.to_os_string(), value.to_os_string()))
            })
            .collect();
        assert_eq!(
            kept,
            vec![(
                OsString::from("DBUS_SESSION_BUS_ADDRESS"),
                OsString::from("/x")
            )]
        );
    }

    /// Review F1: an `openssl` placed first on `PATH`, with `OPENSSL_CONF`
    /// and loader variables set, never runs and never sees the key; the
    /// control shows that a bare `openssl` under that `PATH` runs the fake.
    #[cfg(unix)]
    #[test]
    fn a_path_prefixed_openssl_never_receives_the_key() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = tempfile::tempdir().unwrap();
        if OpensslSigner::discover(scratch.path()).is_err() {
            eprintln!("/usr/bin/openssl not installed; skipped");
            return;
        }
        let fake = tempfile::tempdir().unwrap();
        let caught = fake.path().join("caught");
        let script = fake.path().join("openssl");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ncat > '{}'\nexit 0\n", caught.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let control = Command::new("openssl")
            .env_clear()
            .env("PATH", fake.path())
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(control.success() && caught.exists());
        std::fs::remove_file(&caught).unwrap();
        let evil = fake.path().join("evil");
        let signer = OpensslSigner {
            scratch: scratch.path().to_path_buf(),
            inherited: Some(variables(&[
                ("PATH", fake.path()),
                ("OPENSSL_CONF", &evil),
                ("DYLD_INSERT_LIBRARIES", &evil),
                ("LD_PRELOAD", &evil),
            ])),
        };
        let signature = signer
            .sign_rs256(&throwaway_key(), b"header.payload")
            .unwrap();
        assert_eq!(signature.len(), 256);
        assert!(!caught.exists(), "the PATH-prefixed openssl ran");
    }

    /// Review F2: a `.curlrc` (in `HOME`, `CURL_HOME` or the XDG folder)
    /// that sets a proxy and a trace, and proxy variables, do not reach the
    /// call that carries the App's token. The control shows the same
    /// environment does apply them to a plain curl.
    #[cfg(unix)]
    #[test]
    fn curl_applies_no_curlrc_proxy_or_trace() {
        use std::io::{Read as _, Write as _};
        if CurlHttp::discover().is_err() {
            eprintln!("/usr/bin/curl not installed; skipped");
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let trace = home.path().join("trace");
        let rc = format!(
            "proxy = \"http://127.0.0.1:9\"\ntrace = \"{}\"\n",
            trace.display()
        );
        std::fs::write(home.path().join(".curlrc"), &rc).unwrap();
        std::fs::write(home.path().join("curlrc"), &rc).unwrap();
        let proxy = Path::new("http://127.0.0.1:9");
        let inherited = variables(&[
            ("HOME", home.path()),
            ("CURL_HOME", home.path()),
            ("XDG_CONFIG_HOME", home.path()),
            ("http_proxy", proxy),
            ("ALL_PROXY", proxy),
        ]);
        let control = Command::new("/usr/bin/curl")
            .env_clear()
            .envs(inherited.iter().cloned())
            .args(["-sS", "--max-time", "5", "http://127.0.0.1:1/control"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!control.status.success());
        assert!(trace.exists(), "the control curl did not read the curlrc");
        std::fs::remove_file(&trace).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let http = CurlHttp {
            inherited: Some(inherited),
        };
        let answer = http
            .call(
                "GET",
                &format!("http://127.0.0.1:{port}/app"),
                "jwt-not-for-a-trace",
                None,
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(answer, "ok");
        assert!(!trace.exists(), "curl applied the curlrc trace");
    }

    /// Review nit 5: the git credential helper answers only
    /// `https://github.com`, so an `insteadOf` rewrite to another host
    /// gets nothing.
    #[test]
    fn the_credential_helper_answers_only_github() {
        assert!(credential_request_is_github(
            "protocol=https\nhost=github.com\npath=o/r.git\n\n"
        ));
        assert!(credential_request_is_github(
            "protocol=https\r\nhost=github.com\r\n"
        ));
        for other in [
            "protocol=https\nhost=evil.example\n",
            "protocol=http\nhost=github.com\n",
            "host=github.com\n",
            "protocol=https\nhost=github.com.evil.example\n",
            "",
        ] {
            assert!(!credential_request_is_github(other), "{other:?}");
        }
    }
}
