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

    /// Keep `secret` for `account` in place of any entry there, and read it
    /// back. A store that cannot replace an entry refuses.
    ///
    /// # Errors
    ///
    /// When the store refuses, cannot be reached, or does not give back the
    /// value it was handed.
    fn replace(&self, account: &str, secret: &str) -> Result<(), String> {
        let _ = (account, secret);
        Err(format!("{} cannot replace an entry", self.describe()))
    }
}

/// This computer's credential store: the login keychain on macOS, the
/// Secret Service (`secret-tool`) on other Unix systems. Windows has none
/// yet, so the agent App is refused there before anything is created.
#[must_use]
pub fn os_store() -> Box<dyn KeyStore> {
    os_store_for(AGENT_KEY_SERVICE)
}

/// This computer's credential store, keeping its entries under `service`.
#[must_use]
pub fn os_store_for(service: &'static str) -> Box<dyn KeyStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(Keychain::login_for(service))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(SecretService { service })
    }
    #[cfg(not(unix))]
    {
        let _ = service;
        Box::new(Unsupported)
    }
}

/// Whether a store gave back exactly what it was handed; the value itself
/// never reaches a message.
#[cfg(any(unix, test))]
fn kept(store: &dyn KeyStore, account: &str, secret: &str) -> Result<(), String> {
    let back = store.load(account)?;
    if back == secret {
        Ok(())
    } else {
        Err(format!(
            "{} did not give back the entry it was handed for {account}",
            store.describe()
        ))
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
/// loads, reads or trusts (`PATH`, `DYLD_INSERT_LIBRARIES`, `LD_PRELOAD`,
/// `LD_LIBRARY_PATH`, `OPENSSL_CONF`, `CURL_HOME`, `HOME`, `SSL_CERT_FILE`,
/// `CURL_CA_BUNDLE`) never reaches it, and the helper itself is an absolute
/// path, never found through `PATH`.
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
pub(crate) fn helper_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

pub(crate) fn run(command: &mut Command, input: Option<&[u8]>) -> Result<Output, String> {
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
    service: &'static str,
}

#[cfg(target_os = "macos")]
impl Keychain {
    /// The user's default (login) keychain, for the agent App's key.
    #[must_use]
    pub fn login() -> Self {
        Self::login_for(AGENT_KEY_SERVICE)
    }

    /// The user's default (login) keychain, keeping entries under `service`.
    #[must_use]
    pub fn login_for(service: &'static str) -> Self {
        Self {
            keychain: None,
            service,
        }
    }

    /// A named keychain file, for a scratch keychain in tests.
    #[must_use]
    pub fn at(keychain: &Path) -> Self {
        Self {
            keychain: Some(keychain.to_path_buf()),
            service: AGENT_KEY_SERVICE,
        }
    }

    fn find(&self, account: &str, password: bool) -> Command {
        let mut command = Command::new("/usr/bin/security");
        command.args(["find-generic-password", "-s", self.service, "-a", account]);
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
/// standard input, never in the argument list `ps` shows. With `update`, `-U`
/// replaces an entry already there.
#[cfg(any(target_os = "macos", test))]
fn keychain_add_line(
    service: &str,
    account: &str,
    secret: &str,
    keychain: Option<&Path>,
    update: bool,
) -> String {
    let hex = secret.bytes().fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    let keychain = keychain.map_or_else(String::new, |path| format!(" {}", path.display()));
    let update = if update { " -U" } else { "" };
    format!(
        "add-generic-password -s {service} -a {account} -l {service}{update} -X {hex}{keychain}\n"
    )
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
        let line = keychain_add_line(
            self.service,
            account,
            secret,
            self.keychain.as_deref(),
            false,
        );
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
            return Err(keychain_load_failure(&output, account, &self.describe()));
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
        format!(
            "security delete-generic-password -s {} -a {account}{keychain}",
            self.service
        )
    }

    fn replace(&self, account: &str, secret: &str) -> Result<(), String> {
        let line = keychain_add_line(
            self.service,
            account,
            secret,
            self.keychain.as_deref(),
            true,
        );
        let output = run(
            Command::new("/usr/bin/security").arg("-i"),
            Some(line.as_bytes()),
        )?;
        kept(self, account, secret).map_err(|why| {
            redact(
                &format!("{why}: {}", helper_text(&output.stderr)),
                &[secret],
            )
        })
    }
}

/// Why `security find-generic-password -w` gave no key. Only exit 44
/// (errSecItemNotFound) means there is no entry, as in `exists`; any other
/// status is a store failure, such as a locked keychain or a refused prompt.
/// Standard output can hold the key, so it never reaches a message.
#[cfg(any(target_os = "macos", test))]
fn keychain_load_failure(output: &Output, account: &str, store: &str) -> String {
    let detail = helper_text(&output.stderr);
    if output.status.code() == Some(44) {
        format!("no agent App entry for {account} in {store} ({detail})")
    } else {
        format!("cannot read {store} ({}): {detail}", output.status)
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

/// The arguments that ask `secret-tool` whether an entry exists: `search`,
/// never `lookup`. `lookup` exits 1 for an error and for no entry alike, and
/// also gives no value and no error when an unlock leaves an entry locked
/// (libsecret `on_lookup_unlocked`). `search` with no `--unlock` lists locked
/// entries too and prompts no one (libsecret `load_items`).
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn search_args<'a>(service: &'a str, account: &'a str) -> [&'a str; 5] {
    ["search", "service", service, "account", account]
}

/// Whether `secret-tool search` shows an entry. It exits 0 for an answer and
/// 1 for an error (libsecret `secret_tool_action_search`), so only exit 0
/// with nothing on either stream proves there is none; any other failure is
/// an error, never "absent", as for the keychain. Standard output can hold
/// the secret, so it never reaches a message.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn search_answer(output: &Output, store: &str) -> Result<bool, String> {
    let unread = || {
        format!(
            "cannot read {store} ({}): {}",
            output.status,
            helper_text(&output.stderr)
        )
    };
    if !output.status.success() {
        Err(unread())
    } else if !output.stdout.is_empty() {
        Ok(true)
    } else if output.stderr.is_empty() {
        Ok(false)
    } else {
        Err(unread())
    }
}

/// Why `secret-tool lookup` gave no key. The lookup exits 1 for an error and
/// for no entry alike, and gives no value and no error for an entry left
/// locked (libsecret `on_lookup_unlocked`). So anything it wrote is the cause,
/// and with nothing written `search` decides: only a search that finds
/// nothing means there is no entry. Standard output can hold the key, so it
/// never reaches a message.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn lookup_failure(
    lookup: &Output,
    search: Result<bool, String>,
    account: &str,
    store: &str,
) -> String {
    let detail = helper_text(&lookup.stderr);
    if !detail.is_empty() {
        return format!("cannot read {store} ({}): {detail}", lookup.status);
    }
    match search {
        Ok(false) => format!("no agent App entry for {account} in {store}"),
        Ok(true) => format!(
            "{store} holds an entry for {account} but did not give its value; unlock the keyring and run this again"
        ),
        Err(why) => why,
    }
}

/// The freedesktop Secret Service, through `secret-tool`, keeping entries
/// under `service`.
#[cfg(all(unix, not(target_os = "macos")))]
pub struct SecretService {
    service: &'static str,
}

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
            Self::command()?.args(search_args(self.service, account)),
            None,
        )?;
        search_answer(&output, &self.describe())
    }

    fn store(&self, account: &str, secret: &str) -> Result<(), String> {
        let output = run(
            Self::command()?.args([
                "store",
                &format!("--label={}", self.service),
                "service",
                self.service,
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
            Self::command()?.args(["lookup", "service", self.service, "account", account]),
            None,
        )?;
        if !output.status.success() || output.stdout.is_empty() {
            let search = self.exists(account);
            return Err(lookup_failure(&output, search, account, &self.describe()));
        }
        String::from_utf8(output.stdout).map_err(|_| format!("the entry for {account} is not text"))
    }

    fn undo(&self, account: &str) -> String {
        format!(
            "secret-tool clear service {} account {account}",
            self.service
        )
    }

    /// `secret-tool store` replaces an entry with the same attributes, so a
    /// replace is a store read back.
    fn replace(&self, account: &str, secret: &str) -> Result<(), String> {
        self.store(account, secret)?;
        kept(self, account, secret)
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
        Err("this platform has no credential store CodeFlow can keep the agent App's key or a GitHub authorization in yet (macOS keychain or Linux Secret Service)".to_string())
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

/// The variables curl may keep: only those that choose a proxy. A session
/// behind a proxy, such as a sandboxed one, reaches GitHub through them.
///
/// The call to GitHub is HTTPS, so curl asks a proxy for a tunnel and runs TLS
/// to GitHub through it, and the token in the headers never reaches the proxy.
/// That holds only while curl's certificate trust is its own: a variable that
/// changes it, such as `SSL_CERT_FILE` or `CURL_CA_BUNDLE`, would let a proxy
/// answer as GitHub and read the token, so none may be added here.
const CURL_PROXY_VARIABLES: [&str; 8] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];

/// `/usr/bin/curl -q`, with the headers on standard input (`-H @-`). The
/// program is never found through `PATH`; `-q` comes first, so no `.curlrc`
/// is read, and curl's environment holds only the proxy variables, so no
/// `CURL_HOME` or `HOME` can add a trace that would record the App's JSON Web
/// Token or the installation token, and nothing changes which certificates
/// curl trusts.
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

    /// `/usr/bin/curl` in its confined environment, with no arguments yet.
    pub(crate) fn program(&self) -> Command {
        let mut command = Command::new("/usr/bin/curl");
        confine(
            &mut command,
            &inherited(self.inherited.as_ref()),
            &CURL_PROXY_VARIABLES,
        );
        command
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
        let mut command = self.program();
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

/// GitHub's answer to an OAuth request: the HTTP status and the body, which
/// can hold a token and so never reaches a message.
pub struct OAuthAnswer {
    /// The HTTP status.
    pub status: u16,
    /// The response body.
    pub body: String,
}

/// A form POST to GitHub's OAuth endpoints (`github.com/login/...`), which
/// answer JSON when asked and carry their secrets in the body.
pub trait OAuthHttp {
    /// GitHub's answer to `form` posted to `url`.
    ///
    /// # Errors
    ///
    /// When no answer came: the transport failed, or the answer had no status.
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<OAuthAnswer, String>;
}

/// `form` as `application/x-www-form-urlencoded`, every byte outside the
/// unreserved set percent-encoded.
fn form_body(form: &[(&str, &str)]) -> String {
    let encode = |text: &str| {
        text.bytes().fold(String::new(), |mut out, byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                out.push(char::from(byte));
            } else {
                use std::fmt::Write as _;
                let _ = write!(out, "%{byte:02X}");
            }
            out
        })
    };
    form.iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

impl CurlHttp {
    /// The curl command for an OAuth POST: the form, which holds a device code
    /// or a refresh token, goes on standard input (`--data-binary @-`), never
    /// in the argument list; the status is written after the body.
    fn oauth_command(&self, url: &str) -> Command {
        let mut command = self.program();
        command.args([
            "-q",
            "-sS",
            "--max-time",
            "30",
            "-X",
            "POST",
            "-H",
            "Accept: application/json",
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "-H",
            "User-Agent: codeflow",
            "--data-binary",
            "@-",
            "-w",
            "\n%{http_code}",
            url,
        ]);
        command
    }
}

impl OAuthHttp for CurlHttp {
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<OAuthAnswer, String> {
        let secrets: Vec<&str> = form.iter().map(|(_, value)| *value).collect();
        let output = run(
            &mut self.oauth_command(url),
            Some(form_body(form).as_bytes()),
        )?;
        if !output.status.success() {
            return Err(redact(
                &format!("POST {url} failed: {}", helper_text(&output.stderr)),
                &secrets,
            ));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| format!("the answer to POST {url} is not UTF-8"))?;
        let (body, status) = text
            .rsplit_once('\n')
            .and_then(|(body, status)| Some((body, status.trim().parse::<u16>().ok()?)))
            .ok_or_else(|| format!("the answer to POST {url} has no HTTP status"))?;
        Ok(OAuthAnswer {
            status,
            body: body.to_string(),
        })
    }
}

/// What a failed [`AppHttp`] call shows about its cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallFailure {
    /// No answer came from GitHub: DNS, connection, proxy, TLS or a timeout.
    Unreachable,
    /// GitHub answered with this HTTP status.
    Status(u16),
    /// Anything else, which says nothing about GitHub.
    Unclear,
}

/// The curl exit codes that mean no answer came from GitHub: a proxy or host
/// not resolved (5, 6), no connection (7), a broken or empty exchange (16, 18,
/// 52, 55, 56, 92), a timeout (28), too many redirects (47), TLS (35, 60) and
/// a proxy handshake (97).
const CURL_NO_ANSWER: [&str; 14] = [
    "5", "6", "7", "16", "18", "28", "35", "47", "52", "55", "56", "60", "92", "97",
];

/// The cause of a failed call. `curl` prints its own line after the response
/// body, so the last such line is the real answer, and a body that mentions a
/// status or imitates curl does not count. A proxy that refuses a tunnel is
/// exit 56, never exit 22, so its status does not read as GitHub's.
pub(crate) fn call_failure(error: &str) -> CallFailure {
    let Some(at) = error.rfind("curl: (") else {
        return CallFailure::Unclear;
    };
    let Some((code, text)) = error[at + "curl: (".len()..].split_once(')') else {
        return CallFailure::Unclear;
    };
    if code == "22" {
        let status = text
            .strip_prefix(" The requested URL returned error: ")
            .unwrap_or_default();
        let digits = status
            .find(|next: char| !next.is_ascii_digit())
            .unwrap_or(status.len());
        match status[..digits].parse() {
            Ok(status) if digits == 3 => CallFailure::Status(status),
            _ => CallFailure::Unclear,
        }
    } else if CURL_NO_ANSWER.contains(&code) {
        CallFailure::Unreachable
    } else {
        CallFailure::Unclear
    }
}

/// A failed call by its cause, for a status with no meaning of its own to the
/// caller. `unknown` says what the failure leaves open. Callers redact it.
pub(crate) fn call_problem(error: &str, unknown: &str) -> String {
    match call_failure(error) {
        CallFailure::Unreachable => format!("cannot reach GitHub, so {unknown}: {error}"),
        CallFailure::Status(401) => format!(
            "GitHub rejected the request (check the key, the client id and this computer's clock), so {unknown}: {error}"
        ),
        CallFailure::Status(403) => format!("GitHub forbids this request, so {unknown}: {error}"),
        CallFailure::Status(status @ 500..=599) => {
            format!("GitHub failed with {status}, so {unknown}: {error}")
        }
        CallFailure::Status(status) => format!("GitHub answered {status}, so {unknown}: {error}"),
        CallFailure::Unclear => format!("cannot tell why the request failed, so {unknown}: {error}"),
    }
}

/// Why the installation lookup failed: only a 404 says the App is not
/// installed.
fn installation_problem(slug: &str, repository: &str, error: &str) -> String {
    if call_failure(error) == CallFailure::Status(404) {
        format!(
            "the agent App {slug} has no installation on {repository} that GitHub shows: {error}"
        )
    } else {
        call_problem(
            error,
            &format!("it is not known whether the agent App {slug} is installed on {repository}"),
        )
    }
}

/// Why the request for an installation token failed: only a 4xx from GitHub
/// is a refusal.
fn token_problem(error: &str) -> String {
    match call_failure(error) {
        CallFailure::Status(400..=499) => {
            format!("GitHub refused the installation token: {error}")
        }
        _ => call_problem(error, "no installation token was made"),
    }
}

/// Why `import_agent_key` could not prove a key. A 401 is GitHub rejecting the
/// signed token, and a 404 is GitHub knowing no such App; anything else says
/// nothing against the key.
fn key_check_problem(error: &str) -> String {
    match call_failure(error) {
        CallFailure::Status(401 | 404) => {
            format!("refused: GitHub did not accept this key for that client id: {error}")
        }
        _ => call_problem(error, "this key is not proved"),
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
        .map_err(|error| {
            if call_failure(&error) == CallFailure::Status(404) {
                format!("GitHub does not show the bot user {name}: {error}")
            } else {
                call_problem(&error, &format!("the bot user {name} cannot be confirmed"))
            }
        })?;
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
pub(crate) fn load_app(store: &dyn KeyStore, repository: &str) -> Result<AgentApp, String> {
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
    let answer = app_as_itself(client_id, &pem, signer, http, now)
        .map_err(|error| format!("{}; nothing stored", key_check_problem(&error)))?;
    let (app_id, slug, page) = confirmed_agent_app(&answer, repository)?;
    let jwt = signed_app_jwt(client_id, &pem, signer, now)
        .map_err(|error| redact(&error, &[pem.as_str()]))?;
    let secrets = [pem.as_str(), jwt.as_str()];
    let installation = http
        .call(
            "GET",
            &format!("{API}/repos/{repository}/installation"),
            &jwt,
            None,
        )
        .map_err(|error| {
            redact(
                &format!(
                    "{}; nothing stored",
                    installation_problem(&slug, repository, &error)
                ),
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
/// the repository, GitHub refuses or fails, or GitHub cannot be reached. The
/// message names which.
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
        .call(
            "GET",
            &format!("{API}/repos/{repository}/installation"),
            &jwt,
            None,
        )
        .map_err(|error| {
            let problem = installation_problem(&app.slug, repository, &error);
            let problem = if call_failure(&error) == CallFailure::Status(404) {
                format!(
                    "{problem}; install it at https://github.com/apps/{}/installations/new",
                    app.slug
                )
            } else {
                problem
            };
            redact(&problem, &secrets)
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
        .map_err(|error| redact(&token_problem(&error), &secrets))?;
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
        fn replace(&self, account: &str, secret: &str) -> Result<(), String> {
            self.store(account, secret)
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

        let refused = http(
            Ok(r#"{"id":77}"#),
            Err("POST failed: curl: (22) The requested URL returned error: 403"),
        );
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
        let line = keychain_add_line(AGENT_KEY_SERVICE, "o/r", "{\"pem\":\"x y\"}", None, false);
        assert!(line.starts_with("add-generic-password -s codeflow-agent-app -a o/r "));
        assert!(!line.contains("pem"));
        let hex = line.split(" -X ").nth(1).unwrap().trim();
        assert_eq!(decode_hex(hex).as_deref(), Some("{\"pem\":\"x y\"}"));
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
    }

    /// AC-17: a renewed grant replaces the entry in place (`-U`), under the
    /// service it is given; a first store does not.
    #[test]
    fn keychain_line_replaces_only_when_asked() {
        let line = keychain_add_line("codeflow-pr-user", "o/r", "{}", None, true);
        assert!(
            line.starts_with(
                "add-generic-password -s codeflow-pr-user -a o/r -l codeflow-pr-user -U -X "
            ),
            "{line}"
        );
        let line = keychain_add_line(AGENT_KEY_SERVICE, "o/r", "{}", None, false);
        assert!(!line.contains(" -U"), "{line}");
    }

    /// A store kept only when it gives the value back: a store that answers
    /// something else is refused, and the message holds neither value.
    #[test]
    fn kept_compares_the_value_read_back() {
        let store = MemoryStore::default();
        store.store("o/r", "ghr_new").unwrap();
        assert!(kept(&store, "o/r", "ghr_new").is_ok());
        let error = kept(&store, "o/r", "ghr_other").unwrap_err();
        assert!(
            !error.contains("ghr_new") && !error.contains("ghr_other"),
            "{error}"
        );
    }

    /// AC-17: the OAuth form holds a device code or a refresh token, so it
    /// goes on standard input and never into the argument list `ps` shows.
    #[test]
    fn oauth_command_carries_no_form_value() {
        let http = CurlHttp {
            inherited: Some(Vec::new()),
        };
        let command = http.oauth_command("https://github.com/login/oauth/access_token");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args.first().map(String::as_str), Some("-q"));
        let data = args.iter().position(|arg| arg == "--data-binary").unwrap();
        assert_eq!(args[data + 1], "@-");
        assert!(args.iter().any(|arg| arg == "Accept: application/json"));
        assert!(!args.iter().any(|arg| arg.contains("refresh_token")));
        assert_eq!(
            form_body(&[("client_id", "Iv1"), ("refresh_token", "a+b c/=")]),
            "client_id=Iv1&refresh_token=a%2Bb%20c%2F%3D"
        );
    }

    /// AC-17: the real curl posts the form from standard input and reads
    /// GitHub's status and body, a 4xx included, from a local listener.
    #[cfg(unix)]
    #[test]
    fn curl_posts_the_oauth_form_and_reads_the_status() {
        use std::io::{Read as _, Write as _};
        if CurlHttp::discover().is_err() {
            eprintln!("/usr/bin/curl not installed; skipped");
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            // Until the headers and the whole body have come.
            loop {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if body.len() >= length || read == 0 {
                        break;
                    }
                }
                if read == 0 {
                    break;
                }
            }
            let body = br#"{"error":"authorization_pending"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            stream.write_all(body).unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        let http = CurlHttp {
            inherited: Some(Vec::new()),
        };
        let answer = http
            .post_form(
                &format!("http://127.0.0.1:{port}/login/oauth/access_token"),
                &[("client_id", "Iv1"), ("device_code", "a+b c")],
            )
            .unwrap();
        let request = server.join().unwrap();
        assert_eq!(answer.status, 400);
        assert_eq!(answer.body, r#"{"error":"authorization_pending"}"#);
        assert!(
            request.starts_with("POST /login/oauth/access_token "),
            "{request}"
        );
        assert!(request.contains("Accept: application/json"), "{request}");
        assert!(
            request.ends_with("\r\n\r\nclient_id=Iv1&device_code=a%2Bb%20c"),
            "{request}"
        );
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

    /// Review F2: a `.curlrc` (in `HOME`, `CURL_HOME` or the XDG folder) that
    /// sets a proxy and a trace does not reach the call that carries the
    /// App's token; the control shows the same environment does apply it to a
    /// plain curl. Of the environment, curl keeps exactly the proxy
    /// variables: nothing that changes which certificates it trusts, what it
    /// loads or what it reads. A proxy with its own certificate authority
    /// therefore cannot read the call, and `a_proxy_gets_a_tunnel_and_no_token`
    /// shows what one does see.
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
        let elsewhere = home.path().join("elsewhere");
        let mut inherited = variables(&[
            ("HOME", home.path()),
            ("CURL_HOME", home.path()),
            ("XDG_CONFIG_HOME", home.path()),
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

        for name in [
            "CURL_CA_BUNDLE",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "NODE_EXTRA_CA_CERTS",
            "REQUESTS_CA_BUNDLE",
            "PATH",
            "OPENSSL_CONF",
            "SSLKEYLOGFILE",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
        ] {
            inherited.extend(variables(&[(name, &elsewhere)]));
        }
        for name in CURL_PROXY_VARIABLES {
            inherited.extend(variables(&[(name, Path::new("http://127.0.0.1:9"))]));
        }
        let kept = |inherited: Inherited| {
            let http = CurlHttp {
                inherited: Some(inherited),
            };
            let mut names: Vec<_> = http
                .program()
                .get_envs()
                .filter(|(_, value)| value.is_some())
                .map(|(name, _)| name.to_string_lossy().to_string())
                .collect();
            names.sort();
            names
        };
        let mut wanted = CURL_PROXY_VARIABLES.map(str::to_string).to_vec();
        wanted.sort();
        assert_eq!(kept(inherited.clone()), wanted);

        // With the proxy variables set aside, the same call goes straight to
        // the listener: the `.curlrc` proxy is not applied, and neither is
        // its trace.
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
        inherited.retain(|(name, _)| !CURL_PROXY_VARIABLES.iter().any(|proxy| name == proxy));
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

    /// What a proxy gets from the real curl, by a local listener standing in
    /// for one. It is asked for a tunnel, with no token in that request, and
    /// the next bytes are the start of TLS, so the token never crosses in the
    /// clear. A proxy that refuses the tunnel is a network failure, even when
    /// its status is one GitHub could have sent. The host is `.invalid` and
    /// the listener gives up, so a curl that ignored the proxy reaches no one
    /// and fails this test without hanging.
    #[cfg(unix)]
    #[test]
    fn a_proxy_gets_a_tunnel_and_no_token() {
        use std::io::{Read as _, Write as _};
        if CurlHttp::discover().is_err() {
            eprintln!("/usr/bin/curl not installed; skipped");
            return;
        }
        let through = |answer: &'static [u8]| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let server = std::thread::spawn(move || {
                let give_up = std::time::Instant::now() + std::time::Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(_) if std::time::Instant::now() > give_up => {
                            return (Vec::new(), Vec::new());
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                let mut seen = Vec::new();
                let mut chunk = [0_u8; 4096];
                while !seen.windows(4).any(|end| end == b"\r\n\r\n") {
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0, "the request ended early");
                    seen.extend_from_slice(&chunk[..read]);
                }
                stream.write_all(answer).unwrap();
                // After a tunnel opens, curl speaks first.
                let read = stream.read(&mut chunk).unwrap_or(0);
                (seen, chunk[..read].to_vec())
            });
            let proxy = format!("http://127.0.0.1:{port}");
            let http = CurlHttp {
                inherited: Some(variables(&[("HTTPS_PROXY", Path::new(&proxy))])),
            };
            let error = http
                .call(
                    "GET",
                    "https://github.invalid/app",
                    "jwt-not-for-a-proxy",
                    None,
                )
                .unwrap_err();
            let (request, tunnel) = server.join().unwrap();
            (String::from_utf8_lossy(&request).to_string(), tunnel, error)
        };

        let (request, tunnel, error) = through(b"HTTP/1.1 200 Connection Established\r\n\r\n");
        assert!(
            request.starts_with("CONNECT github.invalid:443 "),
            "{request}"
        );
        assert!(!request.contains("jwt-not-for-a-proxy"), "{request}");
        assert!(
            !request.to_lowercase().contains("authorization"),
            "{request}"
        );
        assert_eq!(
            tunnel.first(),
            Some(&0x16),
            "not a TLS handshake: {tunnel:?}"
        );
        let text = String::from_utf8_lossy(&tunnel);
        assert!(!text.contains("jwt-not-for-a-proxy"), "{text}");
        assert_eq!(call_failure(&error), CallFailure::Unreachable, "{error}");

        for answer in [
            &b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"[..],
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
        ] {
            let (request, tunnel, error) = through(answer);
            assert!(!request.contains("jwt-not-for-a-proxy"), "{request}");
            assert!(tunnel.is_empty(), "{tunnel:?}");
            assert_eq!(call_failure(&error), CallFailure::Unreachable, "{error}");
        }
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
            (
                routes(
                    Ok(&good),
                    Err("curl: (22) The requested URL returned error: 404"),
                ),
                "no installation on o/r",
            ),
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

    /// A store that cannot say whether it holds an entry, as a locked keyring
    /// or a failed D-Bus call leaves it. Nothing may write to it.
    pub(crate) struct UnclearStore;
    impl KeyStore for UnclearStore {
        fn describe(&self) -> String {
            "an unclear store".to_string()
        }
        fn available(&self) -> Result<(), String> {
            Ok(())
        }
        fn exists(&self, _: &str) -> Result<bool, String> {
            Err("the keyring did not answer".to_string())
        }
        fn store(&self, _: &str, _: &str) -> Result<(), String> {
            panic!("wrote to a store that could not answer");
        }
        fn load(&self, _: &str) -> Result<String, String> {
            panic!("read a store that could not answer");
        }
        fn undo(&self, account: &str) -> String {
            format!("forget {account}")
        }
    }

    #[test]
    fn import_stops_when_the_store_cannot_answer() {
        let http = Routes::new(&[]);
        let missing = Path::new("/nonexistent/never-read.pem");
        let error = import(&UnclearStore, &http, &signs(), missing).unwrap_err();
        assert!(error.contains("did not answer"), "{error}");
        assert!(http.calls.borrow().is_empty());
    }

    /// A real exit status and real output, from a shell standing in for
    /// `secret-tool`, which this host does not have, or for `security`,
    /// which no test runs.
    #[cfg(unix)]
    fn stub_tool(script: &str) -> Output {
        Command::new("/bin/sh")
            .args(["-c", script])
            .output()
            .unwrap()
    }

    /// What curl writes when it never gets an answer from GitHub, whatever
    /// the cause, and each is the network's fault: a proxy's own 403 is not
    /// GitHub's.
    const NO_ANSWER: [&str; 5] = [
        "GET failed: curl: (6) Could not resolve host: api.github.com",
        "GET failed: curl: (7) Failed to connect to api.github.com port 443",
        "GET failed: curl: (28) Operation timed out after 30002 milliseconds",
        "GET failed: curl: (35) TLS connect error",
        "GET failed: curl: (56) CONNECT tunnel failed, response 403",
    ];

    fn github_said(status: &str) -> String {
        format!(
            r#"GET failed: {{"message":"Not Found"}}curl: (22) The requested URL returned error: {status}"#
        )
    }

    /// Only curl's own last line decides, on every route: GitHub's answer
    /// comes before it and can say anything.
    #[test]
    fn a_failed_call_is_read_from_curls_own_line() {
        for text in NO_ANSWER {
            assert_eq!(call_failure(text), CallFailure::Unreachable, "{text}");
        }
        assert_eq!(call_failure(&github_said("404")), CallFailure::Status(404));
        assert_eq!(
            call_failure(&github_said("404 Not Found")),
            CallFailure::Status(404)
        );
        assert_eq!(call_failure(&github_said("502")), CallFailure::Status(502));
        for text in [
            github_said("4040"),
            github_said("40"),
            github_said(""),
            "404".to_string(),
            "GET failed: Could not resolve host".to_string(),
            "cannot run /usr/bin/curl: no such file".to_string(),
            "curl: (x) odd".to_string(),
            // curl's own trouble, not the network's.
            "curl: (3) URL rejected: Bad hostname".to_string(),
            "curl: (23) Failure writing output to destination".to_string(),
        ] {
            assert_eq!(call_failure(&text), CallFailure::Unclear, "{text}");
        }
        // A body that imitates curl comes before the real line.
        assert_eq!(
            call_failure(
                "GET failed: curl: (22) The requested URL returned error: 404curl: (28) timed out"
            ),
            CallFailure::Unreachable
        );
    }

    /// The real curl's words for each class, from a local listener and a
    /// closed port: the classes above are read from what it prints.
    #[cfg(unix)]
    #[test]
    fn the_real_curl_failures_keep_their_class() {
        use std::io::{Read as _, Write as _};
        if CurlHttp::discover().is_err() {
            eprintln!("/usr/bin/curl not installed; skipped");
            return;
        }
        let http = CurlHttp {
            inherited: Some(Vec::new()),
        };
        for status in [401_u16, 403, 404, 422, 500, 503] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                let body = r#"{"message":"returned error: 404"}"#;
                let answer = format!(
                    "HTTP/1.1 {status} Whatever\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(answer.as_bytes()).unwrap();
            });
            let error = http
                .call("GET", &format!("http://127.0.0.1:{port}/app"), "b", None)
                .unwrap_err();
            server.join().unwrap();
            assert_eq!(call_failure(&error), CallFailure::Status(status), "{error}");
        }
        let closed = http
            .call("GET", "http://127.0.0.1:1/app", "b", None)
            .unwrap_err();
        assert_eq!(call_failure(&closed), CallFailure::Unreachable, "{closed}");
        let unknown = http
            .call("GET", "http://no-such-host.invalid/app", "b", None)
            .unwrap_err();
        assert_eq!(
            call_failure(&unknown),
            CallFailure::Unreachable,
            "{unknown}"
        );
    }

    fn mint_with(http: &FakeHttp) -> String {
        mint(&stored(), &signs(), http, "o/r", 1_000_000).unwrap_err()
    }

    /// The installation lookup, which `mint` and an import share: only a 404
    /// says the App is not installed. Each other class names its own cause.
    fn lookup_cases() -> Vec<(String, &'static str)> {
        let mut cases: Vec<_> = NO_ANSWER
            .iter()
            .map(|text| ((*text).to_string(), "cannot reach GitHub"))
            .collect();
        cases.extend([
            (github_said("401"), "rejected the request"),
            (github_said("403"), "forbids"),
            (github_said("500"), "GitHub failed"),
            // The body says "Not Found"; the status is what GitHub answered.
            (github_said("503"), "GitHub failed"),
            (github_said("422"), "answered 422"),
            ("an odd failure".to_string(), "cannot tell why"),
        ]);
        cases
    }

    #[test]
    fn mint_says_no_installation_only_for_a_404() {
        for (error, named) in lookup_cases() {
            let shown = mint_with(&http(Err(&error), Ok("{}")));
            assert!(shown.contains(named), "{error}: {shown}");
            assert!(!shown.contains("has no installation"), "{error}: {shown}");
            assert!(!shown.contains("installations/new"), "{error}: {shown}");
            assert!(shown.contains("not known whether"), "{error}: {shown}");
        }
        let shown = mint_with(&http(Err(&github_said("404")), Ok("{}")));
        assert!(
            shown.contains("has no installation on o/r")
                && shown.contains("apps/codeflow-agent-r/installations/new"),
            "{shown}"
        );
    }

    #[test]
    fn mint_calls_only_a_github_4xx_a_refusal() {
        let installed = Ok(r#"{"id":77}"#);
        for status in ["403", "404", "422"] {
            let shown = mint_with(&http(installed, Err(&github_said(status))));
            assert!(
                shown.contains("GitHub refused the installation token"),
                "{shown}"
            );
        }
        for (error, named) in NO_ANSWER
            .iter()
            .map(|text| ((*text).to_string(), "cannot reach GitHub"))
            .chain([
                (github_said("500"), "GitHub failed"),
                (github_said("503"), "GitHub failed"),
                ("an odd failure".to_string(), "no installation token"),
            ])
        {
            let shown = mint_with(&http(installed, Err(&error)));
            assert!(shown.contains(named), "{error}: {shown}");
            assert!(!shown.contains("refused"), "{error}: {shown}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn import_says_no_installation_only_for_a_404_and_stores_nothing() {
        let (_dir, path) = key_file();
        let app = app_answer(&json!({}));
        let routes = |error: &str| Routes::new(&[("/app", Ok(&app)), (INSTALLATION, Err(error))]);
        for (error, named) in lookup_cases() {
            let store = MemoryStore::default();
            let shown = import(&store, &routes(&error), &signs(), &path).unwrap_err();
            assert!(shown.contains(named), "{error}: {shown}");
            assert!(!shown.contains("has no installation"), "{error}: {shown}");
            assert!(shown.ends_with("nothing stored"), "{error}: {shown}");
            assert!(store.entries.borrow().is_empty(), "{error}");
            assert!(!shown.contains("secretkeymaterial"), "{shown}");
        }
    }

    /// The check of the key itself: the class comes from the inner error, so
    /// a network failure says nothing against the key, and a 401 says the key
    /// was rejected.
    #[cfg(unix)]
    #[test]
    fn import_takes_the_key_checks_class_from_the_failure() {
        let (_dir, path) = key_file();
        let rejected = "did not accept this key";
        let cases = NO_ANSWER
            .iter()
            .map(|text| ((*text).to_string(), "cannot reach GitHub"))
            .chain([
                (github_said("401"), rejected),
                (github_said("404"), rejected),
                (github_said("403"), "forbids"),
                (github_said("500"), "GitHub failed"),
                ("an odd failure".to_string(), "cannot tell why"),
            ]);
        for (error, named) in cases {
            let store = MemoryStore::default();
            let http = Routes::new(&[("/app", Err(&error))]);
            let shown = import(&store, &http, &signs(), &path).unwrap_err();
            assert!(shown.contains(named), "{error}: {shown}");
            assert!(shown.ends_with("nothing stored"), "{error}: {shown}");
            assert!(!shown.contains("or could not be reached"), "{shown}");
            if named != rejected {
                assert!(!shown.contains(rejected), "{error}: {shown}");
            }
            assert!(store.entries.borrow().is_empty(), "{error}");
        }
    }

    /// The public read of the bot user: only a 404 says GitHub does not show
    /// it.
    #[test]
    fn a_bot_user_read_says_missing_only_for_a_404() {
        let read = |error: &str| {
            let http = Routes::new(&[(USER_PATH, Err(error))]);
            committer_for_slug("codeflow-agent-r", &http).unwrap_err()
        };
        let missing = read(&github_said("404"));
        assert!(missing.contains("does not show the bot user"), "{missing}");
        let cases = NO_ANSWER
            .iter()
            .map(|text| ((*text).to_string(), "cannot reach GitHub"))
            .chain([
                (github_said("403"), "forbids"),
                (github_said("502"), "GitHub failed"),
            ]);
        for (error, named) in cases {
            let shown = read(&error);
            assert!(shown.contains(named), "{error}: {shown}");
            assert!(!shown.contains("does not show"), "{error}: {shown}");
        }
    }

    /// `security` exits 44 for no such entry, as [`KeyStore::exists`] reads
    /// it. Any other status, such as a locked keychain or a refused prompt,
    /// is a store failure with its cause.
    #[cfg(unix)]
    #[test]
    fn a_keychain_read_proves_absence_only_by_exit_44() {
        let failure =
            |script: &str| keychain_load_failure(&stub_tool(script), "o/r", "the keychain");
        let absent = failure("printf 'item not found\\n' >&2; exit 44");
        assert!(absent.contains("no agent App entry for o/r"), "{absent}");
        for script in [
            "printf 'User interaction is not allowed.\\n' >&2; exit 36",
            "printf 'keychain cannot be found\\n' >&2; exit 1",
            "exit 51",
            "kill -9 $$",
        ] {
            let shown = failure(script);
            assert!(
                shown.contains("cannot read the keychain"),
                "{script}: {shown}"
            );
            assert!(!shown.contains("no agent App entry"), "{script}: {shown}");
        }
        assert!(failure("printf 'not allowed\\n' >&2; exit 36").contains("not allowed"));
        // Standard output can hold the key, and it never reaches a message.
        let shown = failure("printf 'hunter2\\n'; exit 36");
        assert!(!shown.contains("hunter2"), "{shown}");
    }

    /// `secret-tool lookup` exits 1 for an error and for no entry alike, and
    /// gives no value and no error for an entry left locked, so the search
    /// decides when the lookup itself names no cause.
    #[cfg(unix)]
    #[test]
    fn a_store_read_says_missing_only_when_the_search_finds_nothing() {
        let failure = |script: &str, search: Result<bool, String>| {
            lookup_failure(&stub_tool(script), search, "o/r", "the store")
        };
        for script in ["exit 1", "exit 0"] {
            let absent = failure(script, Ok(false));
            assert!(absent.contains("no agent App entry for o/r"), "{absent}");
            let locked = failure(script, Ok(true));
            assert!(locked.contains("holds an entry for o/r"), "{locked}");
            assert!(locked.contains("unlock"), "{locked}");
            assert!(!locked.contains("no agent App entry"), "{locked}");
            let unread = failure(script, Err("cannot read the store: bus".to_string()));
            assert_eq!(unread, "cannot read the store: bus");
        }
        let bus = "printf 'secret-tool: Cannot autolaunch D-Bus\\n' >&2; exit 1";
        for search in [Ok(false), Ok(true), Err("the search failed".to_string())] {
            let shown = failure(bus, search);
            assert!(shown.contains("cannot read the store"), "{shown}");
            assert!(shown.contains("Cannot autolaunch D-Bus"), "{shown}");
            assert!(!shown.contains("no agent App entry"), "{shown}");
        }
        let shown = failure("printf 'hunter2\\n'; exit 1", Ok(false));
        assert!(!shown.contains("hunter2"), "{shown}");
    }

    /// `secret-tool search` exits 0 for an answer and 1 for an error
    /// (libsecret `tool/secret-tool.c`, `secret_tool_action_search`), so only
    /// a clean, empty answer proves there is no entry.
    #[cfg(unix)]
    #[test]
    fn a_store_search_proves_absence_only_by_a_clean_empty_answer() {
        let answer = |script: &str| search_answer(&stub_tool(script), "the store");
        assert_eq!(answer("exit 0"), Ok(false));
        assert_eq!(answer("printf '[/1]\\nlabel = k\\n'"), Ok(true));
        // A locked item is listed, and its unread secret goes to stderr.
        assert_eq!(
            answer("printf '[/1]\\n'; printf 'secret-tool: locked\\n' >&2"),
            Ok(true)
        );
        for script in [
            "printf 'secret-tool: Cannot autolaunch D-Bus\\n' >&2; exit 1",
            "exit 1",
            "exit 2",
            "printf 'usage\\n' >&2; exit 2",
            "printf '[/1]\\nsecret = hunter2\\n'; exit 1",
            "printf 'a warning\\n' >&2; exit 0",
            "kill -9 $$",
        ] {
            let error = answer(script).expect_err(script);
            assert!(error.contains("the store"), "{script}: {error}");
            assert!(!error.contains("hunter2"), "{script}: {error}");
        }
    }

    /// The search lists a locked entry without a prompt, which `lookup`
    /// cannot: with no `--unlock`, nothing asks the person at the screen.
    #[test]
    fn the_store_search_never_unlocks() {
        let args = search_args(AGENT_KEY_SERVICE, "o/r");
        assert_eq!(args[0], "search");
        assert!(!args.contains(&"--unlock"), "{args:?}");
        assert_eq!(args[1..], ["service", AGENT_KEY_SERVICE, "account", "o/r"]);
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
