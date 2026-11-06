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
//!
//! `agent-token --committer` prints the App's committer values, confirmed
//! with GitHub, so that a session records the App as committer. Commits keep
//! their human author. `import-agent-key` stores another computer's key from
//! a file the developer owns, which it reads and never deletes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::apps;

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
        // An empty bearer is a public read: no credential is sent.
        let authorization = if bearer.is_empty() {
            String::new()
        } else {
            format!("Authorization: Bearer {bearer}\n")
        };
        let headers = format!(
            "{authorization}Accept: application/vnd.github+json\nX-GitHub-Api-Version: 2022-11-28\nUser-Agent: codeflow\n"
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

/// The App GitHub shows when asked as the App itself. Only the holder of the
/// key gets an answer. It takes the client id and key, not an [`AgentApp`],
/// because an import has no entry yet. Errors are redacted of the key and the
/// App's JSON Web Token.
pub(crate) fn app_as_itself(
    client_id: &str,
    pem: &str,
    signer: &dyn Signer,
    http: &dyn AppHttp,
    now: u64,
) -> Result<Value, String> {
    let jwt =
        signed_app_jwt(client_id, pem, signer, now).map_err(|error| redact(&error, &[pem]))?;
    let answer = http
        .call("GET", &format!("{API}/app"), &jwt, None)
        .map_err(|error| redact(&error, &[pem, jwt.as_str()]))?;
    serde_json::from_str(&answer).map_err(|_| "GitHub's answer for the App is not JSON".to_string())
}

/// The agent App's identity as a git committer. Neither value is a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BotCommitter {
    /// `<slug>[bot]`.
    pub name: String,
    /// `<bot user id>+<slug>[bot]@users.noreply.github.com`.
    pub email: String,
}

/// Lower-case letters, digits and hyphens: what GitHub makes a slug of, and
/// safe in the shell and settings lines these values end up in.
fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The committer values of the App `slug`, read from its public bot user
/// with no credentials. The id in the email is the bot user's, which is not
/// the App's id. The brackets are percent-encoded because curl reads a raw
/// `[bot]` as a pattern.
///
/// # Errors
///
/// When the slug is not one, or GitHub does not show exactly that bot user.
pub(crate) fn committer_for_slug(slug: &str, http: &dyn AppHttp) -> Result<BotCommitter, String> {
    if !valid_slug(slug) {
        return Err(format!(
            "the App's slug '{slug}' is not lower-case letters, digits and hyphens"
        ));
    }
    let name = format!("{slug}[bot]");
    let answer = http
        .call("GET", &format!("{API}/users/{slug}%5Bbot%5D"), "", None)
        .map_err(|error| format!("GitHub does not show the bot user {name}: {error}"))?;
    let user: Value = serde_json::from_str(&answer)
        .map_err(|_| format!("GitHub's answer for the bot user {name} is not JSON"))?;
    let id = user.get("id").and_then(Value::as_u64).filter(|id| *id > 0);
    match id {
        Some(id)
            if user.get("login").and_then(Value::as_str) == Some(name.as_str())
                && user.get("type").and_then(Value::as_str) == Some("Bot") =>
        {
            let email = format!("{id}+{name}@users.noreply.github.com");
            Ok(BotCommitter { name, email })
        }
        _ => Err(format!(
            "GitHub's answer for {name} is not that bot user, so its committer values cannot be confirmed"
        )),
    }
}

/// The committer values of `repository`'s agent App. The slug comes from the
/// App's own answer, confirmed against the stored entry.
///
/// # Errors
///
/// When there is no entry, GitHub does not answer as that App, or the bot
/// user cannot be confirmed. Nothing is guessed.
pub fn bot_committer(
    store: &dyn KeyStore,
    signer: &dyn Signer,
    http: &dyn AppHttp,
    repository: &str,
    now: u64,
) -> Result<BotCommitter, String> {
    let app = load_app(store, repository)?;
    let answer = app_as_itself(&app.client_id, &app.pem, signer, http, now)?;
    let id = answer.get("id").and_then(Value::as_u64);
    let slug = answer.get("slug").and_then(Value::as_str);
    if id != Some(app.app_id) || slug != Some(app.slug.as_str()) {
        return Err(format!(
            "GitHub answered with another App than the entry for {repository} names ({} id {})",
            app.slug, app.app_id
        ));
    }
    committer_for_slug(&app.slug, http).map_err(|error| redact(&error, &[app.pem.as_str()]))
}

/// The agent App entry kept for `repository`. A parse failure drops serde's
/// text, which can echo the entry.
fn load_app(store: &dyn KeyStore, repository: &str) -> Result<AgentApp, String> {
    store.available()?;
    let raw = store.load(repository)?;
    serde_json::from_str(&raw).map_err(|_| {
        format!(
            "the agent App entry for {repository} in {} is not readable; remove it ({}) and run `codeflow remote setup-apps` again",
            store.describe(),
            store.undo(repository)
        )
    })
}

/// The largest key file an import reads: an RSA key is about 2 KiB.
const KEY_FILE_LIMIT: u64 = 16 * 1024;

/// What is wrong with a key file's mode and owner, by the store's own rule:
/// only the effective user may reach it.
#[cfg(unix)]
fn key_file_access_problem(path: &Path, mode: u32, owner: u32, me: u32) -> Option<String> {
    let shown = path.display();
    if owner != me {
        return Some(format!(
            "refused: the key file {shown} belongs to another user; store a key file you own"
        ));
    }
    (mode & 0o077 != 0).then(|| {
        format!("refused: others can read the key file {shown}; run `chmod 600 {shown}` and run this again")
    })
}

/// The key in `path`, a regular file only the effective user can reach.
/// Errors name the path and the reason, never the contents.
#[cfg(unix)]
fn read_key_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt as _;
    let shown = path.display();
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot read the key file {shown}: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "refused: the key file {shown} is not a regular file"
        ));
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    if let Some(problem) = key_file_access_problem(path, metadata.mode(), metadata.uid(), me) {
        return Err(problem);
    }
    let bytes = crate::bounded_file::read_bounded_regular(path, KEY_FILE_LIMIT).map_err(|_| {
        format!(
            "refused: the key file {shown} is not a regular file of at most {KEY_FILE_LIMIT} bytes"
        )
    })?;
    match String::from_utf8(bytes) {
        Ok(pem) if pem.contains("PRIVATE KEY") => Ok(pem),
        _ => Err(format!("refused: {shown} does not hold a private key")),
    }
}

/// No platform without a credential store gets this far.
#[cfg(not(unix))]
fn read_key_file(path: &Path) -> Result<String, String> {
    Err(format!(
        "refused: this platform cannot check who can read {}",
        path.display()
    ))
}

/// What `import_agent_key` is given.
#[derive(Clone, Copy, Debug)]
pub struct KeyImport<'a> {
    /// The repository, `owner/name`.
    pub repository: &'a str,
    /// The App's client id, from its settings page.
    pub client_id: &'a str,
    /// The private key file the developer downloaded.
    pub key_file: &'a Path,
}

/// The owner, the slug and the page of the App that answered as itself, once
/// it is confirmed as `repository`'s agent App.
fn confirmed_agent_app(answer: &Value, repository: &str) -> Result<(u64, String, String), String> {
    let owner = repository.split('/').next().unwrap_or_default();
    let slug = answer
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = answer.get("id").and_then(Value::as_u64);
    let shown_owner = answer
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut wrong = apps::app_problems(apps::AppKind::Agent, &slug, id, answer);
    if !shown_owner.eq_ignore_ascii_case(owner) {
        wrong.push(format!(
            "an App owned by '{owner}' (it is owned by '{shown_owner}')"
        ));
    }
    if !valid_slug(&slug) {
        wrong.push(format!("a usable slug (GitHub shows '{slug}')"));
    }
    match id.filter(|id| *id > 0) {
        Some(id) if wrong.is_empty() => {
            let organization =
                answer.pointer("/owner/type").and_then(Value::as_str) == Some("Organization");
            let page = apps::app_page(organization.then_some(shown_owner), &slug, "");
            Ok((id, slug, page))
        }
        Some(_) => Err(format!(
            "refused: this key does not belong to {repository}'s agent App; expected {}; nothing stored",
            wrong.join(", ")
        )),
        None => Err("refused: GitHub's answer for the App has no id; nothing stored".to_string()),
    }
}

/// Store this computer's own key for `repository`'s agent App. The key is
/// proved first: GitHub must answer as the App, show it as the repository's
/// agent App, and show it installed there. Nothing is stored before that,
/// and the file is only read. Returns the lines to print, the last of which
/// says the file is still there.
///
/// # Errors
///
/// Why the key was refused; the text holds neither the key nor the App's
/// JSON Web Token.
pub fn import_agent_key(
    store: &dyn KeyStore,
    signer: &dyn Signer,
    http: &dyn AppHttp,
    import: &KeyImport<'_>,
    now: u64,
) -> Result<Vec<String>, String> {
    let KeyImport {
        repository,
        client_id,
        key_file,
    } = *import;
    if !apps::valid_repository(repository) {
        return Err(format!("'{repository}' is not owner/name"));
    }
    store.available()?;
    // Before the file is read: a keychain add over an entry keeps the old key
    // and reports success.
    if store.exists(repository)? {
        return Err(format!(
            "refused: {} already holds an entry for {repository}; to replace it, run `{}` and run this again",
            store.describe(),
            store.undo(repository)
        ));
    }
    let pem = read_key_file(key_file)?;
    let answer = app_as_itself(client_id, &pem, signer, http, now).map_err(|error| {
        format!("refused: GitHub did not accept this key for that client id, or could not be reached: {error}; nothing stored")
    })?;
    let (app_id, slug, page) = confirmed_agent_app(&answer, repository)?;
    let jwt = signed_app_jwt(client_id, &pem, signer, now)
        .map_err(|error| redact(&error, &[pem.as_str()]))?;
    let secrets = [pem.as_str(), jwt.as_str()];
    let installation = http
        .call("GET", &format!("{API}/repos/{repository}/installation"), &jwt, None)
        .map_err(|error| {
            redact(
                &format!("refused: the agent App {slug} has no installation on {repository} that GitHub shows: {error}; nothing stored"),
                &secrets,
            )
        })?;
    serde_json::from_str::<Value>(&installation)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
        .ok_or_else(|| {
            format!("refused: GitHub's answer for the {repository} installation has no id; nothing stored")
        })?;
    let app = AgentApp {
        app_id,
        client_id: client_id.to_string(),
        slug: slug.clone(),
        pem: pem.clone(),
    };
    let entry =
        serde_json::to_string(&app).map_err(|_| "cannot encode the agent App entry".to_string())?;
    store
        .store(repository, &entry)
        .map_err(|why| redact(&why, &[pem.as_str(), jwt.as_str(), entry.as_str()]))?;
    let kept = store
        .load(repository)
        .ok()
        .and_then(|raw| serde_json::from_str::<AgentApp>(&raw).ok())
        .is_some_and(|read| {
            (
                read.app_id,
                read.client_id.as_str(),
                read.slug.as_str(),
                read.pem.as_str(),
            ) == (app_id, client_id, slug.as_str(), pem.as_str())
        });
    if !kept {
        return Err(format!(
            "{} did not keep this key for {repository}; remove what it holds (`{}`) and run this again",
            store.describe(),
            store.undo(repository)
        ));
    }
    let path = key_file.display();
    Ok(vec![
        format!(
            "agent App {slug}: key kept for {repository} in {}",
            store.describe()
        ),
        format!(
            "the key file is still at {path}; delete it yourself (`rm {path}`), and if it ever reached a synced folder, a shared disk or a backup, delete this key on {page} and generate a new one"
        ),
    ])
}

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
    let app = load_app(store, repository)?;
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

    /// Answers by path; a path it does not know is a test failure. Each
    /// call's path and bearer are kept. Every refusal repeats the bearer and
    /// the key, as a transport that echoes what it was sent would.
    struct Routes {
        answers: BTreeMap<String, Result<String, String>>,
        calls: RefCell<Vec<(String, String)>>,
    }

    impl Routes {
        fn new(routes: &[(&str, Result<&str, &str>)]) -> Self {
            Self {
                answers: routes
                    .iter()
                    .map(|(path, answer)| {
                        (
                            (*path).to_string(),
                            answer.map(str::to_string).map_err(str::to_string),
                        )
                    })
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl AppHttp for Routes {
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
            let answer = self
                .answers
                .get(path)
                .unwrap_or_else(|| panic!("unexpected call: {path}"))
                .clone();
            answer.map_err(|why| format!("{why} for Bearer {bearer}; key was {PEM}"))
        }
    }

    const USER_PATH: &str = "/users/codeflow-agent-r%5Bbot%5D";
    const BOT: &str = r#"{"login":"codeflow-agent-r[bot]","id":353475153,"type":"Bot"}"#;

    /// `GET /app` for the stored agent App, with `changes` over its fields.
    fn app_answer(changes: &Value) -> String {
        let mut app = json!({
            "id": 5,
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

    fn committer_of(http: &Routes) -> Result<BotCommitter, String> {
        bot_committer(&stored(), &FakeSigner(Ok(vec![1])), http, "o/r", 1_000_000)
    }

    #[test]
    fn bot_committer_names_the_app_from_github() {
        let app = app_answer(&json!({}));
        let http = Routes::new(&[("/app", Ok(&app)), (USER_PATH, Ok(BOT))]);
        assert_eq!(
            committer_of(&http).unwrap(),
            BotCommitter {
                name: "codeflow-agent-r[bot]".to_string(),
                email: "353475153+codeflow-agent-r[bot]@users.noreply.github.com".to_string(),
            }
        );
        let calls = http.calls.borrow();
        assert_eq!(calls[0].0, "/app");
        assert_eq!(calls[0].1.split('.').count(), 3, "the App's own token");
        // The public read: brackets encoded, no credentials.
        assert_eq!(calls[1], (USER_PATH.to_string(), String::new()));
        assert!(!calls[1].0.contains('['));
    }

    #[test]
    fn bot_committer_refuses_what_it_cannot_confirm() {
        let good = app_answer(&json!({}));
        let other_id = app_answer(&json!({"id": 6}));
        let other_slug = app_answer(&json!({"slug": "another-app"}));
        let user = |login: &str, id: Value, kind: &str| {
            json!({"login": login, "id": id, "type": kind}).to_string()
        };
        let cases: Vec<(String, Result<String, String>)> = vec![
            (other_id, Ok(BOT.to_string())),
            (other_slug, Ok(BOT.to_string())),
            (good.clone(), Err("404".to_string())),
            (good.clone(), Err("403 rate limit".to_string())),
            (good.clone(), Ok("not json".to_string())),
            (good.clone(), Ok(user("someone[bot]", json!(5), "Bot"))),
            (
                good.clone(),
                Ok(user("codeflow-agent-r[bot]", json!(5), "User")),
            ),
            (
                good.clone(),
                Ok(user("codeflow-agent-r[bot]", json!(0), "Bot")),
            ),
            (
                good.clone(),
                Ok(user("codeflow-agent-r[bot]", Value::Null, "Bot")),
            ),
            (
                good,
                Ok(r#"{"login":"codeflow-agent-r[bot]","type":"Bot"}"#.to_string()),
            ),
        ];
        for (app, bot) in cases {
            let mut http = Routes::new(&[("/app", Ok(&app))]);
            http.answers.insert(USER_PATH.to_string(), bot);
            let error = committer_of(&http).unwrap_err();
            assert!(!error.contains("secretkeymaterial"), "{error}");
        }
        // No entry, and a refused key, say so and hold no key or token.
        assert!(bot_committer(
            &MemoryStore::default(),
            &FakeSigner(Ok(vec![1])),
            &Routes::new(&[]),
            "o/r",
            1
        )
        .is_err());
        let http = Routes::new(&[("/app", Err("401"))]);
        let error = committer_of(&http).unwrap_err();
        assert!(
            error.contains("401")
                && !error.contains("secretkeymaterial")
                && !error.contains(&http.calls.borrow()[0].1),
            "{error}"
        );
        // A slug that could not be a slug never reaches the network or a shell.
        for slug in ["", "UPPER", "a b", "a;b", "a[bot]", "a/b", "$(x)"] {
            let http = Routes::new(&[]);
            assert!(committer_for_slug(slug, &http).is_err(), "{slug}");
            assert!(http.calls.borrow().is_empty(), "{slug}");
        }
    }

    /// One request to a local listener, as curl sends it.
    #[cfg(unix)]
    fn request_for(bearer: &'static str) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            String::from_utf8_lossy(&request[..read]).to_string()
        });
        let http = CurlHttp {
            inherited: Some(Vec::new()),
        };
        http.call(
            "GET",
            &format!("http://127.0.0.1:{port}/users/a%5Bbot%5D"),
            bearer,
            None,
        )
        .unwrap();
        server.join().unwrap()
    }

    /// The public read sends no credential, and the encoded brackets reach
    /// the server as written; the control shows a bearer is sent otherwise.
    #[cfg(unix)]
    #[test]
    fn curl_sends_no_authorization_for_an_empty_bearer() {
        if CurlHttp::discover().is_err() {
            eprintln!("/usr/bin/curl not installed; skipped");
            return;
        }
        let public = request_for("");
        assert!(public.starts_with("GET /users/a%5Bbot%5D "), "{public}");
        assert!(!public.to_lowercase().contains("authorization"), "{public}");
        let signed = request_for("a-bearer");
        assert!(
            signed.contains("Authorization: Bearer a-bearer"),
            "{signed}"
        );
    }

    const INSTALLATION: &str = "/repos/o/r/installation";

    /// A key file only the owner can read, and the folder that holds it.
    #[cfg(unix)]
    fn key_file() -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.private-key.pem");
        std::fs::write(&path, PEM).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        (dir, path)
    }

    fn import(
        store: &dyn KeyStore,
        http: &dyn AppHttp,
        signer: &dyn Signer,
        path: &Path,
    ) -> Result<Vec<String>, String> {
        import_agent_key(
            store,
            signer,
            http,
            &KeyImport {
                repository: "o/r",
                client_id: "Iv1.client",
                key_file: path,
            },
            1_000_000,
        )
    }

    fn signs() -> FakeSigner {
        FakeSigner(Ok(vec![1, 2, 3]))
    }

    fn good_routes(app: &str) -> Routes {
        Routes::new(&[("/app", Ok(app)), (INSTALLATION, Ok(r#"{"id":77}"#))])
    }

    #[cfg(unix)]
    #[test]
    fn import_builds_the_entry_from_the_apps_own_answer() {
        let (_dir, path) = key_file();
        let store = MemoryStore::default();
        let app = app_answer(&json!({}));
        let http = good_routes(&app);
        let lines = import(&store, &http, &signs(), &path).unwrap();
        let entry: AgentApp = serde_json::from_str(&store.load("o/r").unwrap()).unwrap();
        assert_eq!(
            (
                entry.app_id,
                entry.client_id.as_str(),
                entry.slug.as_str(),
                entry.pem.as_str()
            ),
            (5, "Iv1.client", "codeflow-agent-r", PEM)
        );
        assert_eq!(
            lines[0],
            "agent App codeflow-agent-r: key kept for o/r in a test store"
        );
        assert!(!lines.join("\n").contains("secretkeymaterial"));
        // The stored entry mints.
        let minted = mint(&store, &signs(), &http_for_mint(), "o/r", 1_000_000).unwrap();
        assert_eq!(minted.token, "ghs_minted");
    }

    fn http_for_mint() -> FakeHttp {
        http(
            Ok(r#"{"id":77}"#),
            Ok(r#"{"token":"ghs_minted","expires_at":"2026-11-05T20:00:00Z"}"#),
        )
    }

    /// An organization's App is deleted and rotated on the organization's
    /// page.
    #[cfg(unix)]
    #[test]
    fn import_names_the_organizations_page() {
        let (_dir, path) = key_file();
        let app = app_answer(&json!({"owner": {"login": "o", "type": "Organization"}}));
        let lines = import(&MemoryStore::default(), &good_routes(&app), &signs(), &path).unwrap();
        assert!(
            lines[1].contains("https://github.com/organizations/o/settings/apps/codeflow-agent-r "),
            "{}",
            lines[1]
        );
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_what_does_not_verify_and_stores_nothing() {
        let (_dir, path) = key_file();
        let good = app_answer(&json!({}));
        let authority =
            app_answer(&json!({"permissions": {"checks": "write", "metadata": "read"}}));
        let events = app_answer(&json!({"events": ["push"]}));
        let owner = app_answer(&json!({"owner": {"login": "someone", "type": "User"}}));
        let slug = app_answer(&json!({"slug": "Odd Slug"}));
        let no_id = app_answer(&json!({"id": null}));
        let routes = |app: Result<&str, &str>, installed: Result<&str, &str>| {
            Routes::new(&[("/app", app), (INSTALLATION, installed)])
        };
        let installed = Ok(r#"{"id":77}"#);
        let cases = [
            (routes(Err("401"), installed), "401"),
            (routes(Err("404"), installed), "404"),
            (routes(Err("timed out"), installed), "timed out"),
            (routes(Ok("not json"), installed), "not JSON"),
            (routes(Ok(&owner), installed), "someone"),
            (routes(Ok(&authority), installed), "with permissions"),
            (routes(Ok(&events), installed), "events"),
            (routes(Ok(&slug), installed), "Odd Slug"),
            (routes(Ok(&no_id), installed), "nothing stored"),
            (routes(Ok(&good), Err("404")), "no installation on o/r"),
            (routes(Ok(&good), Ok("{}")), "has no id"),
        ];
        for (http, named) in cases {
            let store = MemoryStore::default();
            let error = import(&store, &http, &signs(), &path).unwrap_err();
            assert!(error.contains(named), "{named}: {error}");
            assert!(store.entries.borrow().is_empty(), "{named}");
            assert!(!error.contains("secretkeymaterial"), "{error}");
            assert!(!error.contains("PRIVATE KEY"), "{error}");
            for (_, bearer) in http.calls.borrow().iter() {
                assert!(!error.contains(bearer.as_str()), "{error}");
            }
        }
        // The key does not sign.
        let store = MemoryStore::default();
        let error = import(
            &store,
            &Routes::new(&[]),
            &FakeSigner(Err(format!("bad key {PEM}"))),
            &path,
        )
        .unwrap_err();
        assert!(store.entries.borrow().is_empty());
        assert!(!error.contains("secretkeymaterial"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_an_existing_entry_before_reading_the_file() {
        let store = stored();
        let before = store.load("o/r").unwrap();
        let missing = Path::new("/nonexistent/never-read.pem");
        let http = Routes::new(&[]);
        let error = import(&store, &http, &signs(), missing).unwrap_err();
        assert!(
            error.contains("already holds an entry") && error.contains("forget o/r"),
            "{error}"
        );
        assert!(
            !error.contains("never-read"),
            "the file was reached: {error}"
        );
        assert!(http.calls.borrow().is_empty());
        assert_eq!(store.load("o/r").unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_an_unsafe_key_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let (dir, path) = key_file();
        let shared = |mode: u32| {
            let file = dir.path().join(format!("mode-{mode:o}.pem"));
            std::fs::write(&file, PEM).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
            file
        };
        let private = |name: &str, bytes: &[u8]| {
            let file = dir.path().join(name);
            std::fs::write(&file, bytes).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            file
        };
        let link = dir.path().join("link.pem");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let fifo = dir.path().join("fifo.pem");
        let made = Command::new("/usr/bin/mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success());
        let cases = [
            (shared(0o644), "chmod 600"),
            (shared(0o640), "chmod 600"),
            (link, "not a regular file"),
            (dir.path().to_path_buf(), "not a regular file"),
            (fifo, "not a regular file"),
            (
                private("large.pem", &vec![b'a'; 16 * 1024 + 1]),
                "at most 16384 bytes",
            ),
            (private("plain.pem", b"no key here"), "does not hold"),
            (private("binary.pem", &[0xff, 0xfe]), "does not hold"),
        ];
        for (file, named) in cases {
            let store = MemoryStore::default();
            let http = Routes::new(&[]);
            let error = import(&store, &http, &signs(), &file).unwrap_err();
            assert!(error.contains(named), "{}: {error}", file.display());
            assert!(error.contains(&file.display().to_string()), "{error}");
            assert!(!error.contains("secretkeymaterial"), "{error}");
            assert!(http.calls.borrow().is_empty());
            assert!(store.entries.borrow().is_empty());
        }
    }

    /// A file owned by another user cannot be made without root, so the
    /// owner rule is tested on the check itself.
    #[cfg(unix)]
    #[test]
    fn a_key_file_of_another_user_is_refused() {
        let path = Path::new("/k.pem");
        let problem = key_file_access_problem(path, 0o100_600, 501, 502).unwrap();
        assert!(problem.contains("belongs to another user"), "{problem}");
        assert_eq!(key_file_access_problem(path, 0o100_600, 501, 501), None);
        assert_eq!(key_file_access_problem(path, 0o100_400, 501, 501), None);
        assert!(key_file_access_problem(path, 0o100_604, 501, 501).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn import_leaves_the_key_file_in_place() {
        use std::os::unix::fs::MetadataExt as _;
        let (_dir, path) = key_file();
        let before = (
            std::fs::read(&path).unwrap(),
            path.metadata().unwrap().mode(),
        );
        let app = app_answer(&json!({}));
        let lines = import(&MemoryStore::default(), &good_routes(&app), &signs(), &path).unwrap();
        let shown = path.display().to_string();
        assert_eq!(
            lines.last().unwrap(),
            &format!("the key file is still at {shown}; delete it yourself (`rm {shown}`), and if it ever reached a synced folder, a shared disk or a backup, delete this key on https://github.com/settings/apps/codeflow-agent-r and generate a new one")
        );
        assert_eq!(
            before,
            (
                std::fs::read(&path).unwrap(),
                path.metadata().unwrap().mode()
            )
        );
        let refused = Routes::new(&[("/app", Err("401"))]);
        import(&MemoryStore::default(), &refused, &signs(), &path).unwrap_err();
        assert_eq!(
            before,
            (
                std::fs::read(&path).unwrap(),
                path.metadata().unwrap().mode()
            )
        );
    }

    /// A store that reports success and keeps something else, as a keychain
    /// add over an old entry does.
    struct ForgetfulStore;
    impl KeyStore for ForgetfulStore {
        fn describe(&self) -> String {
            "a forgetful store".to_string()
        }
        fn available(&self) -> Result<(), String> {
            Ok(())
        }
        fn exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn store(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn load(&self, _: &str) -> Result<String, String> {
            Ok(r#"{"app_id":1,"client_id":"old","slug":"old","pem":"old"}"#.to_string())
        }
        fn undo(&self, account: &str) -> String {
            format!("forget {account}")
        }
    }

    #[cfg(unix)]
    #[test]
    fn import_reports_a_store_that_kept_something_else() {
        let (_dir, path) = key_file();
        let app = app_answer(&json!({}));
        let error = import(&ForgetfulStore, &good_routes(&app), &signs(), &path).unwrap_err();
        assert!(
            error.contains("did not keep this key") && error.contains("forget o/r"),
            "{error}"
        );
        assert!(!error.contains("secretkeymaterial"), "{error}");
        // A store that refuses and echoes its input shows nothing of it.
        let refusing = MemoryStore {
            refuse: true,
            ..MemoryStore::default()
        };
        let error = import(&refusing, &good_routes(&app), &signs(), &path).unwrap_err();
        assert!(error.contains("refuses"), "{error}");
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
