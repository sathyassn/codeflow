//! Which text rule judges a shell call (TSK-397 design D, A2), by
//! attestation.
//!
//! The exec guard has two outcomes. **Baseline** is the line's rule. It
//! applies only when this module can attest the configuration the call's
//! harness runs under: the payload identifies one harness, every
//! configuration layer that harness reads is read, and each layer's
//! containment settings equal the shipped preset or a narrower form listed
//! here. **Tightened** adds the strict branch, which judges every word after
//! a program the guard cannot read through. Anything unreadable, unknown,
//! ambiguous, customised or unattested tightens, with a reason that names
//! what to change. The tightened outcome keeps every baseline finding.
//!
//! An earlier revision listed loosening settings and tightened on each one.
//! Three review rounds each found a loosening the list lacked, in formats
//! the guard does not own, so the list is replaced by the attestation.
//!
//! What is attested, per harness:
//!
//! - **Claude Code**: the session project (`CLAUDE_PROJECT_DIR`, required),
//!   its repository root and a linked worktree's main checkout, each with
//!   its shared and local settings; the user settings; the managed settings
//!   file and drop-ins. In every layer the `sandbox` and `permissions`
//!   objects may hold only the keys listed in `claude_layer`, each with
//!   its shipped value or a narrower one; together the layers must enable a
//!   fail-closed sandbox and carry the floor. Other top-level keys (model,
//!   hooks, status line and the like) configure no containment and are not
//!   judged. Managed preferences (a macOS profile) and a cached
//!   server-managed file are not attested and tighten when present.
//! - **Codex**: the user configuration, profile files beside it, and the
//!   project configuration when the user configuration trusts the project.
//!   Every top-level key must be known; containment keys must hold shipped
//!   values; a shipped permission profile may only add denies or narrow an
//!   access. System and managed configuration files tighten when present.
//! - **Grok Build**: the profile selected by `GROK_SANDBOX` or the user
//!   configuration's `[sandbox] profile` must be a shipped profile, and its
//!   definition in the user and project profile files must equal the
//!   shipped one or narrow it. Grok's default is no sandbox.
//!
//! What no file or variable shows is not read: launch flags (`--settings`,
//! `--sandbox`, `-c`, `--profile`), server-managed settings that are not
//! cached on disk, and unmarked unsandboxed retries. TSK-1052 fails closed
//! there for a command that runs code after a same-line write.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::security::actions;

/// The harnesses whose settings a call is judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harnesses {
    pub claude: bool,
    pub codex: bool,
    pub grok: bool,
}

impl Harnesses {
    /// A payload that identifies no single harness. Its configuration
    /// cannot be attested, so it tightens; every harness is read so the
    /// reasons are complete.
    pub const ALL: Self = Self {
        claude: true,
        codex: true,
        grok: true,
    };
    /// A call identified as Claude Code's.
    pub const CLAUDE: Self = Self {
        claude: true,
        codex: false,
        grok: false,
    };
    /// A call identified as Codex's.
    pub const CODEX: Self = Self {
        claude: false,
        codex: true,
        grok: false,
    };
    /// A call identified as Grok Build's.
    pub const GROK: Self = Self {
        claude: false,
        codex: false,
        grok: true,
    };

    /// The harness a payload's top-level keys identify: Grok alone sends
    /// the camelCase `toolName`; Codex sends `turn_id` and `model`
    /// together; Claude Code sends `prompt_id` and `scratchpad_dir`
    /// together (TSK-397 canary, Claude Code 2.1.312). A payload with the
    /// keys of none or of several is [`Self::ALL`].
    #[must_use]
    pub fn from_keys(keys: &[String]) -> Self {
        let has = |key: &str| keys.iter().any(|k| k == key);
        let grok = has("toolName");
        let codex = has("turn_id") && has("model");
        let claude = has("prompt_id") && has("scratchpad_dir");
        match (claude, codex, grok) {
            (true, false, false) => Self::CLAUDE,
            (false, true, false) => Self::CODEX,
            (false, false, true) => Self::GROK,
            _ => Self::ALL,
        }
    }

    fn identified(self) -> bool {
        self == Self::CLAUDE || self == Self::CODEX || self == Self::GROK
    }
}

/// Where the guard reads harness configuration for one call.
#[derive(Debug, Clone)]
pub struct Host {
    /// The project the harness reports the session started in
    /// (`CLAUDE_PROJECT_DIR`), when it reports one.
    pub session: Option<PathBuf>,
    /// The call's working directory.
    pub cwd: PathBuf,
    /// The repository root of the call's working directory.
    pub root: PathBuf,
    /// The user's home directory.
    pub home: Option<PathBuf>,
    /// `CLAUDE_CONFIG_DIR` when set: Claude Code's user settings directory.
    pub claude_config_dir: Option<PathBuf>,
    /// `CODEX_HOME` when set.
    pub codex_home: Option<PathBuf>,
    /// `GROK_HOME` when set.
    pub grok_home: Option<PathBuf>,
    /// `GROK_SANDBOX` when set: the Grok profile the environment selects.
    pub grok_sandbox: Option<String>,
    /// Claude Code's managed settings file and its drop-in directory.
    pub managed_file: Option<PathBuf>,
    pub managed_dir: Option<PathBuf>,
    /// Claude Code managed preference files, which this module does not
    /// attest and which tighten when present.
    pub claude_unattested: Vec<PathBuf>,
    /// Codex system and managed configuration files, likewise.
    pub codex_unattested: Vec<PathBuf>,
    /// Native Windows, where Claude Code and Grok Build have no sandbox.
    pub native_windows: bool,
}

/// The debug-build variable that moves the managed and system
/// configuration locations, so a test is not judged by the host's own
/// policy. A release build never reads it.
pub const TEST_MANAGED_DIR: &str = "CODEFLOW_TEST_CLAUDE_MANAGED_DIR";

/// The variable Claude Code exports to every hook: the directory its
/// session started in.
pub const CLAUDE_PROJECT_DIR: &str = "CLAUDE_PROJECT_DIR";

impl Host {
    /// The host a hook process sees for a call in `cwd`, whose repository
    /// root is `root`.
    #[must_use]
    pub fn for_hook(cwd: &Path, root: &Path) -> Self {
        let session = env_path(CLAUDE_PROJECT_DIR);
        Self::with_session(session, cwd, root)
    }

    /// The host `codeflow doctor` sees: a session started at `root`.
    #[must_use]
    pub fn for_session(root: &Path) -> Self {
        Self::with_session(Some(root.to_path_buf()), root, root)
    }

    fn with_session(session: Option<PathBuf>, cwd: &Path, root: &Path) -> Self {
        let home = crate::portable_path::user_home();
        let test = test_managed_dir();
        let (claude_unattested, codex_unattested) = match &test {
            Some(dir) => (
                vec![dir.join("com.anthropic.claudecode.plist")],
                codex_system(&dir.join("codex")),
            ),
            None => (
                claude_preferences(home.as_deref()),
                if cfg!(unix) {
                    codex_system(Path::new("/etc/codex"))
                } else {
                    Vec::new()
                },
            ),
        };
        let managed_base = test.or_else(managed_base);
        Self {
            session,
            cwd: cwd.to_path_buf(),
            root: root.to_path_buf(),
            home,
            claude_config_dir: env_path("CLAUDE_CONFIG_DIR"),
            codex_home: env_path("CODEX_HOME"),
            grok_home: env_path("GROK_HOME"),
            grok_sandbox: grok_selection(std::env::var_os("GROK_SANDBOX")),
            managed_file: managed_base
                .as_ref()
                .map(|base| base.join("managed-settings.json")),
            managed_dir: managed_base.map(|base| base.join("managed-settings.d")),
            claude_unattested,
            codex_unattested,
            native_windows: cfg!(windows),
        }
    }
}

/// The Grok profile a `GROK_SANDBOX` value selects. An empty value counts as
/// unset. A value that is not UTF-8 names no shipped profile, so it is kept
/// as a selection that never matches one and the call tightens.
#[must_use]
pub fn grok_selection(value: Option<std::ffi::OsString>) -> Option<String> {
    let value = value.filter(|value| !value.is_empty())?;
    Some(match value.into_string() {
        Ok(name) => name,
        Err(_) => "(a value that is not UTF-8)".to_string(),
    })
}

/// A non-empty environment value as a path; an empty one counts as unset,
/// as the harnesses read it.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Claude Code's managed settings directory for this platform.
fn managed_base() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Library/Application Support/ClaudeCode"))
    } else if cfg!(windows) {
        Some(PathBuf::from(r"C:\Program Files\ClaudeCode"))
    } else if cfg!(unix) {
        Some(PathBuf::from("/etc/claude-code"))
    } else {
        None
    }
}

/// The test location of the managed and system configuration, in a debug
/// build only.
fn test_managed_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        env_path(TEST_MANAGED_DIR)
    } else {
        None
    }
}

/// Claude Code's managed preference files on macOS.
fn claude_preferences(home: Option<&Path>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if cfg!(target_os = "macos") {
        let preferences = Path::new("/Library/Managed Preferences");
        files.push(preferences.join("com.anthropic.claudecode.plist"));
        if let Some(user) = home.and_then(Path::file_name) {
            files.push(
                preferences
                    .join(user)
                    .join("com.anthropic.claudecode.plist"),
            );
        }
    }
    files
}

/// Codex's system and managed configuration files in `dir`.
fn codex_system(dir: &Path) -> Vec<PathBuf> {
    ["config.toml", "managed_config.toml", "requirements.toml"]
        .iter()
        .map(|name| dir.join(name))
        .collect()
}

/// The settings files whose presence makes a directory a project a session
/// may have started in.
const HARNESS_FILES: &[&str] = &[
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".codex/config.toml",
    ".grok/sandbox.toml",
];

/// Whether `dir` holds a harness settings file. A file whose presence
/// cannot be read counts as present, so it is read and reported.
fn holds_settings(dir: &Path) -> bool {
    HARNESS_FILES.iter().any(|file| present(&dir.join(file)))
}

/// Whether anything, readable or not, is at `path`.
fn present(path: &Path) -> bool {
    !matches!(crate::absence::symlink_metadata_optional(path), Ok(None))
}

/// The working tree root of the repository holding `dir`: `Ok(None)`
/// outside a repository.
fn repository_root(dir: &Path) -> Result<Option<PathBuf>, String> {
    match git2::Repository::discover(dir) {
        Ok(repo) => Ok(repo.workdir().map(Path::to_path_buf)),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(error.message().to_string()),
    }
}

/// The main checkout of the linked worktree that holds `dir`: `Ok(None)`
/// outside a repository or in a main checkout. Git records the main
/// working tree as the parent of a common directory named `.git`, or as
/// that directory's `core.worktree`; a separate git directory without
/// `core.worktree` does not record it. The answer must point back: the
/// main checkout's `.git` is the common directory, or a file naming it.
fn main_checkout(dir: &Path) -> Result<Option<PathBuf>, String> {
    let repo = match git2::Repository::discover(dir) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error.message().to_string()),
    };
    if !repo.is_worktree() {
        return Ok(None);
    }
    let common = repo.commondir().to_path_buf();
    let config =
        git2::Config::open(&common.join("config")).map_err(|error| error.message().to_string())?;
    let configured = match config.get_path("core.worktree") {
        Ok(path) => Some(common.join(path)),
        Err(error) if error.code() == git2::ErrorCode::NotFound => None,
        Err(error) => return Err(error.message().to_string()),
    };
    let main = match configured {
        Some(main) => main,
        None if common.file_name().is_some_and(|name| name == ".git") => common
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "the common git directory has no parent".to_string())?,
        None => {
            return Err(format!(
                "the common git directory `{}` does not record its main working tree",
                common.display()
            ))
        }
    };
    let back = main.join(".git");
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let points_back = same(&back, &common)
        || read_text(&back).parsed().is_some_and(|text| {
            text.strip_prefix("gitdir:")
                .map(str::trim)
                .is_some_and(|named| same(&main.join(named), &common))
        });
    if points_back {
        Ok(Some(main))
    } else {
        Err(format!(
            "`{}` does not point back to the common git directory",
            back.display()
        ))
    }
}

/// The project directories whose settings a harness may read, and why any
/// could not be established.
#[derive(Debug, Default)]
struct Projects {
    dirs: Vec<PathBuf>,
    unresolved: Vec<String>,
}

impl Projects {
    fn push(&mut self, dir: PathBuf) {
        if !self.dirs.contains(&dir) {
            self.dirs.push(dir);
        }
    }

    /// Add the main checkout of each linked worktree among the projects.
    fn with_main_checkouts(mut self) -> Self {
        for dir in self.dirs.clone() {
            match main_checkout(&dir) {
                Ok(Some(main)) => self.push(main),
                Ok(None) => {}
                Err(why) => self.unresolved.push(format!(
                    "the main checkout of the worktree at `{}` cannot be established ({why})",
                    dir.display()
                )),
            }
        }
        self
    }
}

/// The directories whose Claude settings a session started in `session`
/// may read: the session directory, its repository root and, for a linked
/// worktree, the main checkout. Claude Code reads shared settings from the
/// session directory and local settings from the repository root (or the
/// main checkout); every candidate is read, so a file Claude Code does not
/// read can only tighten.
fn claude_projects(session: &Path) -> Projects {
    let mut projects = Projects::default();
    projects.push(session.to_path_buf());
    match repository_root(session) {
        Ok(Some(root)) => projects.push(root),
        Ok(None) => {}
        Err(why) => projects.unresolved.push(format!(
            "the repository of the session project `{}` cannot be read ({why})",
            session.display()
        )),
    }
    projects.with_main_checkouts()
}

/// The call's repository root and each directory between the working
/// directory and that root that holds settings, from the root down.
fn call_dirs(host: &Host) -> Vec<PathBuf> {
    let mut dirs = vec![host.root.clone()];
    if host.cwd.starts_with(&host.root) {
        let mut between: Vec<PathBuf> = host
            .cwd
            .ancestors()
            .take_while(|dir| *dir != host.root)
            .filter(|dir| holds_settings(dir))
            .map(Path::to_path_buf)
            .collect();
        between.reverse();
        dirs.extend(between);
    }
    dirs
}

/// Which rule judges a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The line's rule.
    Baseline,
    /// The baseline and the strict branch, with the first reason that
    /// selected it.
    Tightened(String),
}

impl Outcome {
    /// The reason the strict branch's findings name, if tightened.
    #[must_use]
    pub fn trigger(&self) -> Option<&str> {
        match self {
            Self::Baseline => None,
            Self::Tightened(trigger) => Some(trigger),
        }
    }
}

/// Every reason the call is tightened, in a fixed order: the policy
/// reasons the caller passes, the retry flag, then what the configuration
/// does not attest. Empty means the baseline. The hook and `codeflow
/// doctor` both decide through this function.
#[must_use]
pub fn reasons(host: &Host, harnesses: Harnesses, retry: bool, policy: &[String]) -> Vec<String> {
    let mut found = policy.to_vec();
    if retry {
        found.push(
            "the call asks to run outside the sandbox (`dangerouslyDisableSandbox`)".to_string(),
        );
    }
    found.extend(triggers(host, harnesses));
    found
}

/// The outcome for one call: the first of [`reasons`], or the baseline.
#[must_use]
pub fn outcome(host: &Host, harnesses: Harnesses, retry: bool, policy: &[String]) -> Outcome {
    reasons(host, harnesses, retry, policy)
        .into_iter()
        .next()
        .map_or(Outcome::Baseline, Outcome::Tightened)
}

/// What keeps the configuration of a call from being attested, in a fixed
/// order. Empty means it is attested.
#[must_use]
pub fn triggers(host: &Host, harnesses: Harnesses) -> Vec<String> {
    let mut found = Vec::new();
    if !harnesses.identified() {
        found.push(
            "the call's payload does not identify one harness (Claude Code, Codex or Grok Build), so the configuration it runs under cannot be attested"
                .to_string(),
        );
    }
    if host.native_windows && (harnesses.claude || harnesses.grok) {
        found.push("native Windows has no sandbox under Claude Code or Grok Build".to_string());
    }
    if harnesses.claude {
        claude_attest(host, &mut found);
    }
    if harnesses.codex {
        codex_attest(host, &mut found);
    }
    if harnesses.grok {
        grok_attest(host, &mut found);
    }
    let mut seen = std::collections::HashSet::new();
    found.retain(|reason| seen.insert(reason.clone()));
    found
}

/// A settings file as read.
enum Read<T> {
    Absent,
    Unreadable(String),
    Parsed(T),
}

impl<T> Read<T> {
    fn parsed(self) -> Option<T> {
        match self {
            Self::Parsed(value) => Some(value),
            Self::Absent | Self::Unreadable(_) => None,
        }
    }
}

/// The most bytes read from one settings file.
const READ_LIMIT: u64 = 16 * 1024 * 1024;

/// A settings file's text. It must be a regular file (a link to one is
/// followed); it is opened without blocking, so a FIFO cannot stall the
/// hook, checked again on the open handle, and read up to the limit.
fn read_text(path: &Path) -> Read<String> {
    use std::io::Read as _;
    match crate::absence::metadata_optional(path) {
        Ok(None) => return Read::Absent,
        Err(error) => return Read::Unreadable(error.to_string()),
        Ok(Some(meta)) if !meta.is_file() => {
            return Read::Unreadable("not a regular file".to_string());
        }
        Ok(Some(_)) => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) => return Read::Unreadable(error.to_string()),
    };
    match file.metadata() {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Read::Unreadable("not a regular file".to_string()),
        Err(error) => return Read::Unreadable(error.to_string()),
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(READ_LIMIT + 1).read_to_end(&mut bytes) {
        return Read::Unreadable(error.to_string());
    }
    if bytes.len() as u64 > READ_LIMIT {
        return Read::Unreadable(format!("larger than {READ_LIMIT} bytes"));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Read::Parsed(text),
        Err(_) => Read::Unreadable("not UTF-8".to_string()),
    }
}

// ---------------------------------------------------------------- Claude

/// One Claude settings file, read.
struct Layer {
    name: String,
    value: Value,
}

/// The shipped Claude presets, each as `codeflow update` writes it.
fn shipped_claude() -> Vec<Value> {
    [
        include_str!("../../../../assets/base/settings/default.json"),
        include_str!("../../../../assets/base/settings/acceptEdits.json"),
        include_str!("../../../../assets/base/settings/bypass-sandboxed.json"),
    ]
    .iter()
    .filter_map(|text| serde_json::from_str::<Value>(text).ok())
    .map(|mut value| {
        actions::table().apply_to_claude_preset(&mut value);
        value
    })
    .collect()
}

/// Every string a shipped preset holds at `pointer`.
fn shipped_strings(pointer: &str) -> Vec<String> {
    shipped_claude()
        .iter()
        .filter_map(|preset| preset.pointer(pointer).and_then(Value::as_array))
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

/// The Claude settings files a session in `host.session` may read, read;
/// each unreadable or malformed one, and each unattested host file, is a
/// reason in `found`.
fn claude_layers(host: &Host, found: &mut Vec<String>) -> Vec<Layer> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let user_dir = host
        .claude_config_dir
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".claude")));
    if let Some(dir) = &user_dir {
        paths.push(dir.join("settings.json"));
        let cached = dir.join("remote-settings.json");
        if present(&cached) {
            found.push(format!(
                "server-managed Claude settings are cached at `{}`; the guard does not attest them",
                cached.display()
            ));
        }
    }
    if let Some(session) = &host.session {
        let projects = claude_projects(session);
        found.extend(projects.unresolved);
        for dir in projects.dirs {
            paths.push(dir.join(".claude/settings.json"));
            paths.push(dir.join(".claude/settings.local.json"));
        }
    }
    if let Some(file) = &host.managed_file {
        paths.push(file.clone());
    }
    if let Some(dir) = &host.managed_dir {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let mut files: Vec<PathBuf> = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            if path.extension().is_some_and(|ext| ext == "json") {
                                files.push(path);
                            }
                        }
                        Err(error) => found.push(format!(
                            "the managed settings directory `{}` cannot be listed ({error})",
                            dir.display()
                        )),
                    }
                }
                files.sort();
                paths.extend(files);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => found.push(format!(
                "the managed settings directory `{}` cannot be listed ({error})",
                dir.display()
            )),
        }
    }
    let mut layers = Vec::new();
    for path in paths {
        let name = format!("`{}`", path.display());
        if layers.iter().any(|layer: &Layer| layer.name == name) {
            continue;
        }
        match read_text(&path) {
            Read::Absent => {}
            Read::Unreadable(why) => {
                found.push(format!("the Claude settings {name} cannot be read ({why})"));
            }
            Read::Parsed(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => layers.push(Layer { name, value }),
                Ok(_) | Err(_) => {
                    found.push(format!("the Claude settings {name} are not a JSON object"));
                }
            },
        }
    }
    layers
}

fn claude_attest(host: &Host, found: &mut Vec<String>) {
    if host.session.is_none() {
        found.push(format!(
            "the hook environment does not name the Claude Code session's project (`{CLAUDE_PROJECT_DIR}`), so the settings in force cannot be read"
        ));
    }
    for path in &host.claude_unattested {
        if present(path) {
            found.push(format!(
                "managed Claude preferences are present at `{}`; the guard does not attest them",
                path.display()
            ));
        }
    }
    let layers = claude_layers(host, found);
    for layer in &layers {
        claude_layer(layer, found);
    }
    for (pointer, key) in [
        ("/sandbox/enabled", "sandbox.enabled"),
        ("/sandbox/failIfUnavailable", "sandbox.failIfUnavailable"),
    ] {
        if !layers
            .iter()
            .any(|layer| layer.value.pointer(pointer) == Some(&Value::Bool(true)))
        {
            found.push(format!(
                "no Claude settings layer sets `{key}` to true; the shipped preset does"
            ));
        }
    }
    let deny = merged(&layers, "/permissions/deny");
    let deny_write = merged(&layers, "/sandbox/filesystem/denyWrite");
    let deny_read = merged(&layers, "/sandbox/filesystem/denyRead");
    for missing in missing_floor(&deny, &deny_write, &deny_read) {
        found.push(format!(
            "the Claude settings are missing the floor entry {missing}; `codeflow update` restores it"
        ));
    }
    let credentials = merged_credentials(&layers);
    for name in shipped_credential_denies() {
        if !credentials.contains(&name) {
            found.push(format!(
                "the Claude settings do not deny the credential variable `{name}` (`sandbox.credentials.envVars`); `codeflow update` restores it"
            ));
        }
    }
}

/// Every string of the array at `pointer` across the layers. A value of
/// another shape is reported by [`claude_layer`].
fn merged(layers: &[Layer], pointer: &str) -> Vec<String> {
    layers
        .iter()
        .filter_map(|layer| layer.value.pointer(pointer).and_then(Value::as_array))
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

/// The variable names the layers deny through `sandbox.credentials`.
fn merged_credentials(layers: &[Layer]) -> Vec<String> {
    layers
        .iter()
        .filter_map(|layer| {
            layer
                .value
                .pointer("/sandbox/credentials/envVars")
                .and_then(Value::as_array)
        })
        .flatten()
        .filter(|entry| entry.get("mode").and_then(Value::as_str) == Some("deny"))
        .filter_map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

/// The credential variables every shipped preset denies.
fn shipped_credential_denies() -> Vec<String> {
    let presets = shipped_claude();
    let Some(first) = presets.first() else {
        return Vec::new();
    };
    let names = |preset: &Value| -> Vec<String> {
        preset
            .pointer("/sandbox/credentials/envVars")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|entry| entry.get("mode").and_then(Value::as_str) == Some("deny"))
            .filter_map(|entry| {
                entry
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    };
    names(first)
        .into_iter()
        .filter(|name| presets.iter().all(|preset| names(preset).contains(name)))
        .collect()
}

/// Attest one Claude layer's `sandbox` and `permissions` objects: every
/// key must be one listed here, with the shipped value or a narrower one.
fn claude_layer(layer: &Layer, found: &mut Vec<String>) {
    let name = &layer.name;
    let mut fail = |what: String| found.push(format!("the Claude settings {name}: {what}"));
    if let Some(sandbox) = layer.value.get("sandbox") {
        let Some(sandbox) = sandbox.as_object() else {
            fail("`sandbox` is not an object".to_string());
            return;
        };
        for (key, value) in sandbox {
            let at = format!("sandbox.{key}");
            match key.as_str() {
                "enabled" | "failIfUnavailable" => {
                    if value != &Value::Bool(true) {
                        fail(format!("`{at}` is not true, as the shipped preset sets it"));
                    }
                }
                "autoAllowBashIfSandboxed" | "allowUnsandboxedCommands" => {
                    if !value.is_boolean() {
                        fail(format!("`{at}` is not a boolean"));
                    }
                }
                "excludedCommands" => {
                    if value.as_array().is_none_or(|items| !items.is_empty()) {
                        fail(format!(
                            "`{at}` lists commands that run outside the sandbox; remove it"
                        ));
                    }
                }
                "enableWeakerNestedSandbox"
                | "enableWeakerNetworkIsolation"
                | "allowAppleEvents"
                | "allowNestedUserNamespaces" => {
                    if value != &Value::Bool(false) {
                        fail(format!("`{at}` weakens the sandbox; remove it"));
                    }
                }
                "filesystem" => claude_filesystem(value, &mut fail),
                "network" => claude_network(value, &mut fail),
                "credentials" => claude_credentials(value, &mut fail),
                _ => fail(format!(
                    "`{at}` is not a setting the shipped preset uses, so its effect is not attested; remove it"
                )),
            }
        }
    }
    if let Some(permissions) = layer.value.get("permissions") {
        let Some(permissions) = permissions.as_object() else {
            fail("`permissions` is not an object".to_string());
            return;
        };
        for (key, value) in permissions {
            let at = format!("permissions.{key}");
            match key.as_str() {
                // Deny wins over allow and ask in Claude Code, so neither
                // can reopen the floor.
                "allow" | "ask" | "deny" => {
                    if !strings(value) {
                        fail(format!("`{at}` is not an array of strings"));
                    }
                }
                "defaultMode" => {
                    if !value.is_string() {
                        fail(format!("`{at}` is not a string"));
                    }
                }
                "disableBypassPermissionsMode" => {}
                "additionalDirectories" => {
                    if value.as_array().is_none_or(|items| !items.is_empty()) {
                        fail(format!(
                            "`{at}` adds directories the sandbox can write; remove it"
                        ));
                    }
                }
                _ => fail(format!(
                    "`{at}` is not a setting the shipped preset uses, so its effect is not attested; remove it"
                )),
            }
        }
    }
}

/// Whether `value` is an array of strings.
fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

/// `sandbox.filesystem`: denies may grow; reads and writes may only be
/// ones a shipped preset grants.
fn claude_filesystem(value: &Value, fail: &mut impl FnMut(String)) {
    let Some(map) = value.as_object() else {
        fail("`sandbox.filesystem` is not an object".to_string());
        return;
    };
    for (key, value) in map {
        let at = format!("sandbox.filesystem.{key}");
        match key.as_str() {
            "denyRead" | "denyWrite" => {
                if !strings(value) {
                    fail(format!("`{at}` is not an array of strings"));
                }
            }
            "allowRead" | "allowWrite" => {
                if !strings(value) {
                    fail(format!("`{at}` is not an array of strings"));
                    continue;
                }
                let shipped = shipped_strings(&format!("/sandbox/filesystem/{key}"));
                for entry in value.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if !shipped.iter().any(|known| known == entry) {
                        fail(format!(
                            "`{at}` grants `{entry}`, which no shipped preset grants; remove it"
                        ));
                    }
                }
            }
            _ => fail(format!(
                "`{at}` is not a setting the shipped preset uses, so its effect is not attested; remove it"
            )),
        }
    }
}

/// `sandbox.network`: domain lists are egress rules and may be set; local
/// binding is a boolean; Unix sockets may not be opened.
fn claude_network(value: &Value, fail: &mut impl FnMut(String)) {
    let Some(map) = value.as_object() else {
        fail("`sandbox.network` is not an object".to_string());
        return;
    };
    for (key, value) in map {
        let at = format!("sandbox.network.{key}");
        match key.as_str() {
            "allowedDomains" | "deniedDomains" => {
                if !strings(value) {
                    fail(format!("`{at}` is not an array of strings"));
                }
            }
            "allowLocalBinding" => {
                if !value.is_boolean() {
                    fail(format!("`{at}` is not a boolean"));
                }
            }
            "allowUnixSockets" => {
                if value.as_array().is_none_or(|items| !items.is_empty()) {
                    fail(format!(
                        "`{at}` opens Unix sockets, which can reach a process outside the sandbox; remove it"
                    ));
                }
            }
            "allowAllUnixSockets" => {
                if value != &Value::Bool(false) {
                    fail(format!(
                        "`{at}` opens Unix sockets, which can reach a process outside the sandbox; remove it"
                    ));
                }
            }
            _ => fail(format!(
                "`{at}` is not a setting the shipped preset uses, so its effect is not attested; remove it"
            )),
        }
    }
}

/// `sandbox.credentials`: entries that deny a variable.
fn claude_credentials(value: &Value, fail: &mut impl FnMut(String)) {
    let Some(map) = value.as_object() else {
        fail("`sandbox.credentials` is not an object".to_string());
        return;
    };
    for (key, value) in map {
        let at = format!("sandbox.credentials.{key}");
        if key != "envVars" {
            fail(format!(
                "`{at}` is not a setting the shipped preset uses, so its effect is not attested; remove it"
            ));
            continue;
        }
        let denies = value.as_array().is_some_and(|items| {
            items.iter().all(|entry| {
                entry.as_object().is_some_and(|entry| {
                    entry.len() == 2
                        && entry.get("name").is_some_and(Value::is_string)
                        && entry.get("mode") == Some(&Value::String("deny".to_string()))
                })
            })
        });
        if !denies {
            fail(format!(
                "`{at}` holds an entry other than a `{{\"name\", \"mode\": \"deny\"}}` deny"
            ));
        }
    }
}

/// Every `sandbox.filesystem.allowWrite` and
/// `permissions.additionalDirectories` entry of the Claude layers a
/// session in `host.session` reads, with the file that holds it, and why
/// any layer could not be read; `codeflow doctor` checks the state root
/// against them.
#[must_use]
pub fn claude_write_grants(host: &Host) -> (Vec<(String, String)>, Vec<String>) {
    let mut problems = Vec::new();
    let layers = claude_layers(host, &mut problems);
    let mut grants = Vec::new();
    for layer in &layers {
        for pointer in [
            "/sandbox/filesystem/allowWrite",
            "/permissions/additionalDirectories",
        ] {
            for entry in layer
                .value
                .pointer(pointer)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                grants.push((entry.to_string(), layer.name.clone()));
            }
        }
    }
    (grants, problems)
}

/// The floor entries a merged configuration lacks: each enforcement path
/// needs its `Edit(...)` deny or its `denyWrite` entry, and each secret
/// class its `Read(...)` deny or its `denyRead` entry.
#[must_use]
pub fn missing_floor(deny: &[String], deny_write: &[String], deny_read: &[String]) -> Vec<String> {
    let table = actions::table();
    let mut missing = Vec::new();
    for rule in protected_edit_denies() {
        let written = actions::edit_deny_write_path(&rule)
            .is_some_and(|path| deny_write.iter().any(|entry| entry == &path));
        if !written && !deny.contains(&rule) {
            missing.push(format!("`{rule}`"));
        }
    }
    for rule in table.claude_read_denies() {
        let path = rule
            .strip_prefix("Read(")
            .and_then(|inner| inner.strip_suffix(')'));
        let read = path.is_some_and(|path| deny_read.iter().any(|entry| entry == path));
        if !read && !deny.contains(&rule) {
            missing.push(format!("`{rule}`"));
        }
    }
    for path in &table.sandbox_read_denies {
        let rule = format!("Read({path})");
        if !deny_read.contains(path) && !deny.contains(&rule) {
            missing.push(format!("`{path}` in `sandbox.filesystem.denyRead`"));
        }
    }
    missing
}

/// Every `Edit(...)` deny of the floor: the enforcement paths, the shell
/// startup files and the git configuration files of the action table.
fn protected_edit_denies() -> Vec<String> {
    let table = actions::table();
    table
        .claude_edit_denies
        .iter()
        .cloned()
        .chain(table.startup_paths.claude_edit_denies())
        .chain(table.git_config_paths.claude_edit_denies())
        .collect()
}

// ----------------------------------------------------------------- TOML

/// A TOML configuration file as read: absent, a reason it cannot be used,
/// or its table.
fn read_toml(path: &Path, what: &str, found: &mut Vec<String>) -> Option<toml::Table> {
    let name = format!("`{}`", path.display());
    match read_text(path) {
        Read::Absent => None,
        Read::Unreadable(why) => {
            found.push(format!("the {what} {name} cannot be read ({why})"));
            None
        }
        Read::Parsed(text) => {
            let Ok(table) = text.parse::<toml::Table>() else {
                found.push(format!("the {what} {name} is not valid TOML"));
                return None;
            };
            Some(table)
        }
    }
}

/// `over` laid onto `base`: a key `over` sets wins, tables merge.
fn overlay(base: &mut toml::Table, over: &toml::Table) {
    for (key, value) in over {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(more)) => overlay(inner, more),
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

// ---------------------------------------------------------------- Codex

/// Codex top-level keys that configure no containment: models, prompts,
/// display, history, telemetry, tools other than the shell, authentication
/// and project trust. Any value is attested.
const CODEX_FREE_KEYS: &[&str] = &[
    "agents",
    "allow_login_shell",
    "analytics",
    "approvals_reviewer",
    "apps",
    "auto_review",
    "background_terminal_max_timeout",
    "browser_use",
    "chatgpt_base_url",
    "check_for_update_on_startup",
    "cli_auth_credentials_store",
    "compact_prompt",
    "computer_use",
    "desktop",
    "developer_instructions",
    "disable_paste_burst",
    "experimental_compact_prompt_file",
    "experimental_use_unified_exec_tool",
    "feedback",
    "file_opener",
    "forced_chatgpt_workspace_id",
    "forced_login_method",
    "hide_agent_reasoning",
    "history",
    "hooks",
    "instructions",
    "log_dir",
    "marketplaces",
    "mcp_oauth_callback_port",
    "mcp_oauth_callback_url",
    "mcp_oauth_credentials_store",
    "mcp_optional_startup_grace_ms",
    "mcp_servers",
    "memories",
    "model",
    "model_auto_compact_token_limit",
    "model_auto_compact_token_limit_scope",
    "model_catalog_json",
    "model_context_window",
    "model_instructions_file",
    "model_provider",
    "model_providers",
    "model_reasoning_effort",
    "model_reasoning_summary",
    "model_supports_reasoning_summaries",
    "model_verbosity",
    "notice",
    "notify",
    "openai_base_url",
    "oss_provider",
    "otel",
    "personality",
    "plan_mode_reasoning_effort",
    "plugins",
    "profile",
    "project_doc_fallback_filenames",
    "project_doc_max_bytes",
    "projects",
    "review_model",
    "service_tier",
    "shell_environment_policy",
    "show_raw_agent_reasoning",
    "skills",
    "sqlite_home",
    "suppress_unstable_features_warning",
    "tool_output_token_limit",
    "tool_suggest",
    "tools",
    "tui",
    "web_search",
    "windows_wsl_setup_acknowledged",
];

/// Codex feature flags that configure no containment.
const CODEX_FREE_FEATURES: &[&str] = &[
    "apps",
    "code_mode",
    "context_management",
    "enable_request_compression",
    "fast_mode",
    "goals",
    "memories",
    "multi_agent",
    "personality",
    "prevent_idle_sleep",
    "remote_plugin",
    "rollout_budget",
    "shell_snapshot",
    "shell_tool",
    "skill_mcp_dependency_install",
    "ultrafast_mode",
    "unified_exec",
    "web_search",
    "web_search_cached",
    "web_search_request",
];

/// The shipped Codex configuration.
fn shipped_codex() -> toml::Table {
    include_str!("../../../../assets/base/codex/config.toml")
        .parse()
        .unwrap_or_default()
}

/// The shipped permission profile `name`, if there is one.
fn shipped_codex_profile(name: &str) -> Option<toml::Table> {
    shipped_codex()
        .get("permissions")
        .and_then(|permissions| permissions.get(name))
        .and_then(toml::Value::as_table)
        .cloned()
}

fn codex_attest(host: &Host, found: &mut Vec<String>) {
    for path in &host.codex_unattested {
        if present(path) {
            found.push(format!(
                "the Codex system configuration `{}` is present; the guard does not attest it",
                path.display()
            ));
        }
    }
    let user_dir = host
        .codex_home
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".codex")));
    let mut user = toml::Table::new();
    if let Some(dir) = &user_dir {
        let path = dir.join("config.toml");
        if let Some(table) = read_toml(&path, "Codex configuration", found) {
            codex_layer(&table, &format!("`{}`", path.display()), false, found);
            user = table;
        }
        // Profile files beside the user configuration, which `--profile`
        // lays over it.
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let mut files: Vec<PathBuf> = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => files.push(entry.path()),
                        // An entry that cannot be read could be a profile
                        // file, so the call tightens.
                        Err(error) => found.push(format!(
                            "an entry of the Codex home `{}` cannot be read ({error})",
                            dir.display()
                        )),
                    }
                }
                files.retain(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(".config.toml"))
                });
                files.sort();
                for path in files {
                    if let Some(table) = read_toml(&path, "Codex profile file", found) {
                        codex_layer(&table, &format!("`{}`", path.display()), true, found);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => found.push(format!(
                "the Codex home `{}` cannot be listed ({error})",
                dir.display()
            )),
        }
    }
    let trusted = codex_trusts(&user, &host.root);
    let mut effective = user.clone();
    let mut name = "the user Codex configuration".to_string();
    for dir in call_dirs(host) {
        let path = dir.join(".codex/config.toml");
        let Some(table) = read_toml(&path, "Codex configuration", found) else {
            continue;
        };
        codex_layer(&table, &format!("`{}`", path.display()), false, found);
        if !trusted {
            found.push(format!(
                "the user Codex configuration does not mark `{}` trusted, so Codex does not apply `{}`; trust the project in the user configuration",
                host.root.display(),
                path.display()
            ));
        }
        if trusted {
            overlay(&mut effective, &table);
            name = format!(
                "the Codex configuration in force at `{}`",
                host.root.display()
            );
        }
    }
    codex_effective(&effective, &name, found);
}

/// Whether the user configuration marks the project at `root` trusted.
/// Codex loads a project's `.codex/` layers only then.
fn codex_trusts(user: &toml::Table, root: &Path) -> bool {
    let Some(projects) = user.get("projects").and_then(toml::Value::as_table) else {
        return false;
    };
    let mut keys = vec![root.to_path_buf()];
    if let Ok(canonical) = root.canonicalize() {
        keys.push(canonical);
    }
    // A root that is not UTF-8 cannot be a TOML key, so it is never
    // trusted, and the project layer is not read as applied.
    keys.iter().filter_map(|key| key.to_str()).any(|key| {
        projects
            .get(key)
            .and_then(|entry| entry.get("trust_level"))
            .and_then(toml::Value::as_str)
            == Some("trusted")
    })
}

/// Attest one Codex layer: every key is known, and each containment key
/// holds its shipped value or a narrower one. `profile_file` marks a file
/// `--profile` lays over the user configuration.
fn codex_layer(table: &toml::Table, name: &str, profile_file: bool, found: &mut Vec<String>) {
    let mut problems: Vec<String> = Vec::new();
    for (key, value) in table {
        let mut fail = |what: String| problems.push(what);
        match key.as_str() {
            "approval_policy" => {
                if value.as_str() != Some("never") {
                    fail(
                        "`approval_policy` is not `never`, so an escalation can be approved"
                            .to_string(),
                    );
                }
            }
            "default_permissions" => {
                if !matches!(value.as_str(), Some("cf-guard" | "cf-builder")) {
                    fail(
                        "`default_permissions` does not select `cf-guard` or `cf-builder`"
                            .to_string(),
                    );
                }
            }
            "permissions" => codex_permissions(value, &mut fail),
            "features" => codex_features(value, &mut fail),
            "windows" => {
                let shipped = shipped_codex().get("windows").cloned();
                if Some(value) != shipped.as_ref() {
                    fail("`windows` differs from the shipped `[windows]` table".to_string());
                }
            }
            "profiles" => match value.as_table() {
                Some(profiles) => {
                    for (profile, inner) in profiles {
                        match inner.as_table() {
                            Some(inner) if !inner.contains_key("profiles") => codex_layer(
                                inner,
                                &format!("{name} profile `{profile}`"),
                                true,
                                found,
                            ),
                            _ => fail(format!("profile `{profile}` is not a table of settings")),
                        }
                    }
                }
                None => fail("`profiles` is not a table".to_string()),
            },
            "sandbox_mode" | "sandbox_workspace_write" => {
                fail(format!(
                    "`{key}` replaces the permission profile; remove it"
                ));
            }
            "project_root_markers" => fail(
                "`project_root_markers` changes which project layers Codex loads; remove it"
                    .to_string(),
            ),
            "projects" if profile_file => fail("a profile sets `projects`".to_string()),
            key if CODEX_FREE_KEYS.contains(&key) => {}
            _ => fail(format!(
                "`{key}` is not a key the guard knows, so its effect is not attested; remove it"
            )),
        }
    }
    found.extend(
        problems
            .into_iter()
            .map(|what| format!("the Codex configuration {name}: {what}")),
    );
}

/// `[features]`: the shipped flags keep their values; other flags must be
/// ones that configure no containment.
fn codex_features(value: &toml::Value, fail: &mut impl FnMut(String)) {
    let Some(features) = value.as_table() else {
        fail("`features` is not a table".to_string());
        return;
    };
    for (key, value) in features {
        match key.as_str() {
            "hooks" | "codex_hooks" | "network_proxy" => {
                if value.as_bool() != Some(true) {
                    fail(format!(
                        "`features.{key}` is not `true`, as the shipped configuration sets it"
                    ));
                }
            }
            key if CODEX_FREE_FEATURES.contains(&key) => {}
            _ => fail(format!(
                "`features.{key}` is not a feature the guard knows, so its effect is not attested; remove it"
            )),
        }
    }
}

/// `[permissions]`: each shipped profile may only add denies or narrow an
/// access; another profile is not attested and may not be selected.
fn codex_permissions(value: &toml::Value, fail: &mut impl FnMut(String)) {
    let Some(profiles) = value.as_table() else {
        fail("`permissions` is not a table".to_string());
        return;
    };
    for (name, profile) in profiles {
        let Some(shipped) = shipped_codex_profile(name) else {
            continue;
        };
        let Some(profile) = profile.as_table() else {
            fail(format!("`permissions.{name}` is not a table"));
            continue;
        };
        for (key, value) in profile {
            let at = format!("permissions.{name}.{key}");
            match key.as_str() {
                "extends" => {
                    if Some(value) != shipped.get("extends") {
                        fail(format!("`{at}` differs from the shipped profile"));
                    }
                }
                "description" => {}
                "filesystem" => {
                    let empty = toml::Table::new();
                    let shipped = shipped
                        .get("filesystem")
                        .and_then(toml::Value::as_table)
                        .unwrap_or(&empty);
                    codex_filesystem(value, shipped, &at, fail);
                }
                "network" => {
                    let empty = toml::Table::new();
                    let shipped = shipped
                        .get("network")
                        .and_then(toml::Value::as_table)
                        .unwrap_or(&empty);
                    codex_network(value, shipped, &at, fail);
                }
                _ => fail(format!(
                    "`{at}` is not in the shipped profile, so its effect is not attested; remove it"
                )),
            }
        }
    }
}

/// What a Codex filesystem access grants, strictest first.
fn access_rank(access: &str) -> Option<u8> {
    match access {
        "deny" | "none" => Some(0),
        "read" => Some(1),
        "write" => Some(2),
        _ => None,
    }
}

/// A profile's filesystem rules: a shipped entry may only narrow; any
/// other entry may only deny, since a more specific entry overrides a
/// broader one. A nested scope such as `:workspace_roots` follows the same
/// rule.
fn codex_filesystem(
    value: &toml::Value,
    shipped: &toml::Table,
    at: &str,
    fail: &mut impl FnMut(String),
) {
    let Some(table) = value.as_table() else {
        fail(format!("`{at}` is not a table"));
        return;
    };
    for (key, value) in table {
        match (key.as_str(), value) {
            ("glob_scan_max_depth", toml::Value::Integer(depth)) => {
                let floor = shipped
                    .get("glob_scan_max_depth")
                    .and_then(toml::Value::as_integer)
                    .unwrap_or(0);
                if *depth < floor {
                    fail(format!(
                        "`{at}.glob_scan_max_depth` is below the shipped {floor}, so deep denies stop applying"
                    ));
                }
            }
            (scope, toml::Value::Table(_)) if scope.starts_with(':') => {
                let empty = toml::Table::new();
                let inner = shipped
                    .get(scope)
                    .and_then(toml::Value::as_table)
                    .unwrap_or(&empty);
                codex_filesystem(value, inner, &format!("{at}.\"{scope}\""), fail);
            }
            (path, toml::Value::String(access)) => {
                let Some(rank) = access_rank(access) else {
                    fail(format!(
                        "`{at}` gives `{path}` the unknown access `{access}`"
                    ));
                    continue;
                };
                let allowed = shipped
                    .get(path)
                    .and_then(toml::Value::as_str)
                    .and_then(access_rank)
                    .unwrap_or(0);
                if rank > allowed {
                    fail(format!(
                        "`{at}` grants `{path}` = `{access}`, wider than the shipped profile; remove it"
                    ));
                }
            }
            (path, _) => fail(format!(
                "`{at}` holds `{path}` with a value the guard does not model"
            )),
        }
    }
}

/// A profile's network table: every key keeps its shipped value, except
/// that a domain may be denied or dropped.
fn codex_network(
    value: &toml::Value,
    shipped: &toml::Table,
    at: &str,
    fail: &mut impl FnMut(String),
) {
    let Some(table) = value.as_table() else {
        fail(format!("`{at}` is not a table"));
        return;
    };
    for (key, value) in table {
        if key == "domains" {
            let empty = toml::Table::new();
            let known = shipped
                .get("domains")
                .and_then(toml::Value::as_table)
                .unwrap_or(&empty);
            let Some(domains) = value.as_table() else {
                fail(format!("`{at}.domains` is not a table"));
                continue;
            };
            for (domain, rule) in domains {
                if rule.as_str() != Some("deny") && known.get(domain) != Some(rule) {
                    fail(format!(
                        "`{at}.domains` allows `{domain}`, which the shipped profile does not"
                    ));
                }
            }
        } else if shipped.get(key) != Some(value) {
            fail(format!("`{at}.{key}` differs from the shipped profile"));
        }
    }
}

/// The configuration Codex applies must select a shipped profile, never
/// approve escalations, and keep the network proxy and hooks on.
fn codex_effective(table: &toml::Table, name: &str, found: &mut Vec<String>) {
    let mut fail = |what: &str| found.push(format!("{name} {what}"));
    if table.get("approval_policy").and_then(toml::Value::as_str) != Some("never") {
        fail("does not set `approval_policy = \"never\"`, as the shipped configuration does");
    }
    match table
        .get("default_permissions")
        .and_then(toml::Value::as_str)
    {
        Some(profile @ ("cf-guard" | "cf-builder")) => {
            let defined = table
                .get("permissions")
                .and_then(|permissions| permissions.get(profile))
                .and_then(toml::Value::as_table)
                .is_some_and(|have| {
                    shipped_codex_profile(profile).is_some_and(|shipped| {
                        shipped
                            .keys()
                            .all(|key| key == "description" || have.contains_key(key))
                            && covers(have, &shipped)
                    })
                });
            if !defined {
                fail("does not define the selected permission profile with every shipped entry");
            }
        }
        _ => fail("does not select the shipped permission profile (`default_permissions`)"),
    }
    let features = table.get("features").and_then(toml::Value::as_table);
    for flag in ["network_proxy", "hooks"] {
        let on = features.is_some_and(|features| {
            features.get(flag).and_then(toml::Value::as_bool) == Some(true)
                || (flag == "hooks"
                    && features.get("codex_hooks").and_then(toml::Value::as_bool) == Some(true))
        });
        if !on {
            fail(&format!(
                "does not set `features.{flag} = true`, as the shipped configuration does"
            ));
        }
    }
}

/// Whether every entry of `shipped`, at any depth, is present in `have`.
/// Values were checked layer by layer; this checks nothing was dropped.
fn covers(have: &toml::Table, shipped: &toml::Table) -> bool {
    shipped.iter().all(|(key, value)| {
        key == "description"
            || match (have.get(key), value) {
                (Some(toml::Value::Table(have)), toml::Value::Table(shipped)) => {
                    covers(have, shipped)
                }
                (Some(_), _) => true,
                (None, _) => false,
            }
    })
}

// ----------------------------------------------------------------- Grok

/// The shipped Grok profiles by name.
fn shipped_grok() -> toml::Table {
    include_str!("../../../../assets/base/grok/sandbox.toml")
        .parse::<toml::Table>()
        .ok()
        .and_then(|table| {
            table
                .get("profiles")
                .and_then(toml::Value::as_table)
                .cloned()
        })
        .unwrap_or_default()
}

/// The string members of the array `list` in `profile`, or why it is not
/// an array of strings.
fn grok_list(profile: &toml::Table, list: &str) -> Result<Vec<String>, String> {
    match profile.get(list) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("`{list}` holds a member that is not a string"))
            })
            .collect(),
        Some(_) => Err(format!("`{list}` is not an array")),
    }
}

fn grok_attest(host: &Host, found: &mut Vec<String>) {
    let shipped = shipped_grok();
    let grok_dir = host
        .grok_home
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".grok")));
    let mut selected = host
        .grok_sandbox
        .clone()
        .map(|name| (name, "`GROK_SANDBOX`".to_string()));
    if let Some(dir) = &grok_dir {
        let path = dir.join("config.toml");
        if let Some(config) = read_toml(&path, "Grok configuration", found) {
            if let Some(sandbox) = config.get("sandbox") {
                let shown = format!("`[sandbox]` in `{}`", path.display());
                match sandbox.as_table() {
                    Some(table) => {
                        for (key, value) in table {
                            match (key.as_str(), value.as_str()) {
                                ("profile", Some(name)) => {
                                    if selected.is_none() {
                                        selected = Some((name.to_string(), shown.clone()));
                                    }
                                }
                                _ => found.push(format!(
                                    "{shown} sets `{key}`, which the guard does not attest; remove it"
                                )),
                            }
                        }
                    }
                    None => found.push(format!("{shown} is not a table")),
                }
            }
        }
    }
    let Some((profile, source)) = selected else {
        found.push(
            "no Grok sandbox profile is selected where the guard can read it (`GROK_SANDBOX`, or `[sandbox] profile` in the user Grok configuration); Grok's default is no sandbox, and a `--sandbox` launch flag is not visible to the guard"
                .to_string(),
        );
        return;
    };
    if !shipped.contains_key(&profile) {
        found.push(format!(
            "{source} selects the Grok profile `{profile}`, not a shipped one (`cf-guard`, `cf-guard-worktree`)"
        ));
        return;
    }
    let mut files = Vec::new();
    if let Some(dir) = &grok_dir {
        files.push(dir.join("sandbox.toml"));
    }
    files.extend(
        call_dirs(host)
            .into_iter()
            .map(|dir| dir.join(".grok/sandbox.toml")),
    );
    let mut defined = false;
    for path in files {
        let Some(table) = read_toml(&path, "Grok sandbox profiles", found) else {
            continue;
        };
        let name = format!("`{}`", path.display());
        for key in table.keys().filter(|key| *key != "profiles") {
            found.push(format!(
                "the Grok sandbox profiles {name} set `{key}`, which the guard does not attest; remove it"
            ));
        }
        let profiles = table.get("profiles").and_then(toml::Value::as_table);
        for (each, shipped) in &shipped {
            let Some(have) = profiles.and_then(|profiles| profiles.get(each)) else {
                continue;
            };
            if *each == profile {
                defined = true;
            }
            match (have.as_table(), shipped.as_table()) {
                (Some(have), Some(shipped)) => grok_profile(have, shipped, each, &name, found),
                _ => found.push(format!(
                    "the Grok profile `{each}` in {name} is not a table"
                )),
            }
        }
    }
    if !defined {
        found.push(format!(
            "{source} selects the Grok profile `{profile}`, but no profiles file the guard reads defines it"
        ));
    }
}

/// Attest a Grok profile against the shipped one: the same `extends`,
/// every shipped `read_only` and `deny` entry, no write grant beyond the
/// shipped ones, and no other key.
fn grok_profile(
    have: &toml::Table,
    shipped: &toml::Table,
    profile: &str,
    name: &str,
    found: &mut Vec<String>,
) {
    let mut fail =
        |what: String| found.push(format!("the Grok profile `{profile}` in {name} {what}"));
    for (key, value) in have {
        match key.as_str() {
            "extends" => {
                if Some(value) != shipped.get("extends") {
                    fail("does not extend what the shipped profile extends".to_string());
                }
            }
            "restrict_network" => {
                if !value.is_bool() {
                    fail("sets `restrict_network` to a value that is not a boolean".to_string());
                }
            }
            "read_only" | "deny" | "read_write" => {}
            _ => fail(format!(
                "sets `{key}`, which the shipped profile does not, so its effect is not attested; remove it"
            )),
        }
    }
    if !have.contains_key("extends") {
        fail("does not extend what the shipped profile extends".to_string());
    }
    for list in ["read_only", "deny"] {
        let want = grok_list(shipped, list).unwrap_or_default();
        match grok_list(have, list) {
            Err(why) => fail(why),
            Ok(entries) => {
                for entry in want.iter().filter(|entry| !entries.contains(entry)) {
                    fail(format!("lacks the shipped `{list}` entry `{entry}`"));
                }
            }
        }
    }
    let allowed = grok_list(shipped, "read_write").unwrap_or_default();
    match grok_list(have, "read_write") {
        Err(why) => fail(why),
        Ok(entries) => {
            for entry in entries.iter().filter(|entry| !allowed.contains(entry)) {
                fail(format!(
                    "grants the write `{entry}` (`read_write`), which the shipped profile does not; remove it"
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "sandbox_state_tests.rs"]
mod tests;
