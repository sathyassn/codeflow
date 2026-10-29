//! Which text rule judges a shell call (TSK-397 design D, A2).
//!
//! The exec guard has two outcomes. **Baseline** is the line's rule. It
//! applies when no tightening trigger below is found in the harness
//! configuration this module reads; that is not a proof that the sandbox
//! contains the call. **Tightened** adds the strict branch, which judges
//! every word after a program the guard cannot read through. The tightened
//! outcome keeps every baseline finding, so neither outcome allows what the
//! baseline refuses.
//!
//! What is read. A Claude call is judged against the project its session
//! started in, which Claude Code exports to every hook as
//! `CLAUDE_PROJECT_DIR`; a Claude call without it tightens. Any other call
//! is judged against the repository of its working directory, each
//! directory between that and the repository root that holds harness
//! settings, and the session project when the environment names one. A
//! linked worktree adds its main checkout, found through Git's worktree
//! metadata; when that cannot be established the call tightens. Each
//! project is judged on its own and the triggers are combined, so one
//! loosened project tightens the call.
//!
//! Every unreadable, malformed or non-regular file tightens. An absent file
//! contributes nothing, except that a call identified as Codex or Grok
//! tightens when no configuration of that harness is present. What this
//! module cannot read (a `--settings` file, server-managed settings, MDM
//! policy, a Codex launch flag, and the session of a call whose harness is
//! not identified) gets the baseline; TSK-1052 tracks failing closed there.
//!
//! Harnesses are identified by a union rule: the guard reads the layers of
//! every harness whose shape the payload does not contradict. Only a key
//! unique to one harness excludes the others; shared keys never identify.

use std::collections::{BTreeMap, HashSet};
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
    /// Every harness: the reading for a payload that names none.
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

    /// The harnesses a payload's top-level keys do not contradict. Grok
    /// alone sends the camelCase `toolName`; Codex sends `turn_id` and
    /// `model` together; Claude Code sends `prompt_id` and `scratchpad_dir`
    /// together (TSK-397 canary, Claude Code 2.1.312). A payload carrying
    /// the unique keys of more than one harness contradicts itself and is
    /// read against all of them.
    #[must_use]
    pub fn from_keys(keys: &[String]) -> Self {
        let has = |key: &str| keys.iter().any(|k| k == key);
        let grok = has("toolName");
        let codex = has("turn_id") && has("model");
        let claude = has("prompt_id") && has("scratchpad_dir");
        if u8::from(grok) + u8::from(codex) + u8::from(claude) != 1 {
            return Self::ALL;
        }
        Self {
            claude: !(grok || codex),
            codex: !(grok || claude),
            grok: !(codex || claude),
        }
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
    /// Claude Code's managed settings file and its drop-in directory.
    pub managed_file: Option<PathBuf>,
    pub managed_dir: Option<PathBuf>,
    /// Native Windows, where Claude Code and Grok Build have no sandbox.
    pub native_windows: bool,
}

/// The debug-build variable that moves the managed settings location, so a
/// test is not judged by the host's own managed policy. A release build
/// never reads it.
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
        let managed_base = managed_base();
        #[cfg(debug_assertions)]
        let managed_base = env_path(TEST_MANAGED_DIR).or(managed_base);
        Self {
            session,
            cwd: cwd.to_path_buf(),
            root: root.to_path_buf(),
            home: crate::portable_path::user_home(),
            claude_config_dir: env_path("CLAUDE_CONFIG_DIR"),
            codex_home: env_path("CODEX_HOME"),
            managed_file: managed_base
                .as_ref()
                .map(|base| base.join("managed-settings.json")),
            managed_dir: managed_base.map(|base| base.join("managed-settings.d")),
            native_windows: cfg!(windows),
        }
    }
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
    HARNESS_FILES.iter().any(|file| {
        !matches!(
            crate::absence::symlink_metadata_optional(&dir.join(file)),
            Ok(None)
        )
    })
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

/// The project directories a call may run under, and why any could not be
/// established.
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

    /// Add the main checkout of each linked worktree among the projects,
    /// when it holds settings.
    fn with_main_checkouts(mut self) -> Self {
        for dir in self.dirs.clone() {
            match main_checkout(&dir) {
                Ok(Some(main)) if holds_settings(&main) => self.push(main),
                Ok(_) => {}
                Err(why) => self.unresolved.push(format!(
                    "the main checkout of the worktree at `{}` cannot be established ({why})",
                    dir.display()
                )),
            }
        }
        self
    }
}

/// The session project, with its main checkout.
fn session_projects(host: &Host) -> Projects {
    let mut projects = Projects::default();
    if let Some(session) = &host.session {
        projects.push(session.clone());
    }
    projects.with_main_checkouts()
}

/// The call's repository root, each directory between the working
/// directory and that root that holds settings, the session project when
/// named, and their main checkouts.
fn call_projects(host: &Host) -> Projects {
    let mut projects = Projects::default();
    if let Some(session) = &host.session {
        projects.push(session.clone());
    }
    projects.push(host.root.clone());
    if host.cwd.starts_with(&host.root) {
        for dir in host.cwd.ancestors() {
            if dir == host.root {
                break;
            }
            if holds_settings(dir) {
                projects.push(dir.to_path_buf());
            }
        }
    }
    projects.with_main_checkouts()
}

/// Which rule judges a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The line's rule.
    Baseline,
    /// The baseline and the strict branch, with the first trigger that
    /// selected it.
    Tightened(String),
}

impl Outcome {
    /// The trigger text the strict branch's findings name, if tightened.
    #[must_use]
    pub fn trigger(&self) -> Option<&str> {
        match self {
            Self::Baseline => None,
            Self::Tightened(trigger) => Some(trigger),
        }
    }
}

/// Every reason the call is tightened, in a fixed order: the policy, the
/// retry flag, then the configuration triggers. Empty means the baseline.
/// The hook and `codeflow doctor` both decide through this function.
#[must_use]
pub fn reasons(host: &Host, harnesses: Harnesses, retry: bool, policy_strict: bool) -> Vec<String> {
    let mut found = Vec::new();
    if policy_strict {
        found.push("the project policy sets `security.text_guard` to `strict`".to_string());
    }
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
pub fn outcome(host: &Host, harnesses: Harnesses, retry: bool, policy_strict: bool) -> Outcome {
    reasons(host, harnesses, retry, policy_strict)
        .into_iter()
        .next()
        .map_or(Outcome::Baseline, Outcome::Tightened)
}

/// Every configuration trigger that holds for a call, in a fixed order.
#[must_use]
pub fn triggers(host: &Host, harnesses: Harnesses) -> Vec<String> {
    let mut found = Vec::new();
    if host.native_windows && (harnesses.claude || harnesses.grok) {
        found.push("native Windows has no sandbox under Claude Code or Grok Build".to_string());
    }
    let every = call_projects(host);
    if harnesses.claude {
        let projects = if harnesses == Harnesses::CLAUDE && host.session.is_some() {
            session_projects(host)
        } else {
            if harnesses == Harnesses::CLAUDE {
                found.push(format!(
                    "the hook environment does not name the Claude Code session's project (`{CLAUDE_PROJECT_DIR}`), so the settings in force cannot be read"
                ));
            }
            call_projects(host)
        };
        found.extend(projects.unresolved.iter().cloned());
        for dir in &projects.dirs {
            claude_triggers(host, dir, &mut found);
        }
    }
    if harnesses.codex || harnesses.grok {
        found.extend(every.unresolved.iter().cloned());
    }
    if harnesses.codex {
        codex_triggers(host, &every.dirs, harnesses == Harnesses::CODEX, &mut found);
    }
    if harnesses.grok {
        grok_triggers(&every.dirs, harnesses == Harnesses::GROK, &mut found);
    }
    // One entry per trigger, though several layers may carry it.
    let mut seen = HashSet::new();
    found.retain(|trigger| seen.insert(trigger.clone()));
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

/// Where a Claude settings layer sits in the documented precedence, lowest
/// first. Two layers at the same rank that disagree make a value
/// uncomputable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    User,
    Project,
    Local,
    Managed,
}

struct Layer {
    rank: Rank,
    name: String,
    value: Value,
}

/// The Claude settings layers of a session started in `project`, read;
/// each unreadable or malformed one is a trigger.
fn claude_layers(host: &Host, project: &Path, found: &mut Vec<String>) -> Vec<Layer> {
    let mut paths: Vec<(Rank, PathBuf)> = Vec::new();
    let user_dir = host
        .claude_config_dir
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".claude")));
    if let Some(dir) = user_dir {
        paths.push((Rank::User, dir.join("settings.json")));
    }
    paths.push((Rank::Project, project.join(".claude/settings.json")));
    paths.push((Rank::Local, project.join(".claude/settings.local.json")));
    if let Some(file) = &host.managed_file {
        paths.push((Rank::Managed, file.clone()));
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
                paths.extend(files.into_iter().map(|file| (Rank::Managed, file)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => found.push(format!(
                "the managed settings directory `{}` cannot be listed ({error})",
                dir.display()
            )),
        }
    }
    let mut layers = Vec::new();
    for (rank, path) in paths {
        let name = format!("`{}`", path.display());
        match read_text(&path) {
            Read::Absent => {}
            Read::Unreadable(why) => {
                found.push(format!("the Claude settings {name} cannot be read ({why})"));
            }
            Read::Parsed(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => layers.push(Layer { rank, name, value }),
                Ok(_) | Err(_) => {
                    found.push(format!("the Claude settings {name} are not a JSON object"));
                }
            },
        }
    }
    layers
}

/// The effective value of `pointer` by precedence: the highest-ranked
/// layer that sets it. `Err` when two layers of that rank disagree.
fn effective<'a>(layers: &'a [Layer], pointer: &str) -> Result<Option<&'a Value>, ()> {
    let mut best: Option<(Rank, &Value)> = None;
    for layer in layers {
        let Some(value) = layer.value.pointer(pointer) else {
            continue;
        };
        match best {
            Some((rank, _)) if rank > layer.rank => {}
            Some((rank, current)) if rank == layer.rank && current != value => return Err(()),
            _ => best = Some((layer.rank, value)),
        }
    }
    Ok(best.map(|(_, value)| value))
}

/// Every string of the array at `pointer` across the layers; a value that
/// is not an array of strings is named in `found`.
fn merged(layers: &[Layer], pointer: &str, found: &mut Vec<String>) -> Vec<String> {
    let mut items = Vec::new();
    for layer in layers {
        match layer.value.pointer(pointer) {
            None => {}
            Some(Value::Array(values)) => {
                for value in values {
                    match value.as_str() {
                        Some(text) => items.push(text.to_string()),
                        None => found.push(format!(
                            "`{pointer}` in {} holds a value that is not a string",
                            layer.name
                        )),
                    }
                }
            }
            Some(_) => found.push(format!("`{pointer}` in {} is not an array", layer.name)),
        }
    }
    items
}

/// The `sandbox.filesystem.allowWrite` entries of every Claude layer of the
/// session project, and why any layer could not be read; `codeflow doctor`
/// checks the state root against them.
#[must_use]
pub fn claude_allow_write(host: &Host) -> (Vec<String>, Vec<String>) {
    let mut problems = Vec::new();
    let mut entries = Vec::new();
    let projects = session_projects(host);
    problems.extend(projects.unresolved.iter().cloned());
    for dir in &projects.dirs {
        let layers = claude_layers(host, dir, &mut problems);
        entries.extend(merged(
            &layers,
            "/sandbox/filesystem/allowWrite",
            &mut problems,
        ));
    }
    (entries, problems)
}

fn claude_triggers(host: &Host, project: &Path, found: &mut Vec<String>) {
    let layers = claude_layers(host, project, found);
    for (pointer, key) in [
        ("/sandbox/enabled", "sandbox.enabled"),
        ("/sandbox/failIfUnavailable", "sandbox.failIfUnavailable"),
    ] {
        match effective(&layers, pointer) {
            Ok(Some(Value::Bool(true))) => {}
            Ok(_) => found.push(format!(
                "the Claude settings of `{}` do not set `{key}` to true",
                project.display()
            )),
            Err(()) => found.push(format!(
                "the Claude settings layers of `{}` disagree on `{key}`, so its value cannot be computed",
                project.display()
            )),
        }
    }
    for layer in &layers {
        if let Some(value) = layer.value.pointer("/sandbox/filesystem/disabled") {
            if value != &Value::Bool(false) {
                found.push(format!("{} sets `sandbox.filesystem.disabled`", layer.name));
            }
        }
        escape_settings(layer, found);
    }
    let excluded = merged(&layers, "/sandbox/excludedCommands", found);
    if !excluded.is_empty() {
        found.push(format!(
            "the Claude settings list `sandbox.excludedCommands` ({}), which run outside the sandbox",
            excluded.join(", ")
        ));
    }
    let deny = merged(&layers, "/permissions/deny", found);
    let deny_write = merged(&layers, "/sandbox/filesystem/denyWrite", found);
    let deny_read = merged(&layers, "/sandbox/filesystem/denyRead", found);
    for missing in missing_floor(&deny, &deny_write, &deny_read) {
        found.push(format!(
            "the Claude settings are missing the floor entry {missing}; `codeflow update` restores it"
        ));
    }
    let shipped_reads = shipped_allow_read();
    for entry in merged(&layers, "/sandbox/filesystem/allowRead", found) {
        if !shipped_reads.contains(&entry) {
            found.push(format!(
                "`sandbox.filesystem.allowRead` entry `{entry}` can reopen a read the secret floor denies, which covers `**/.env` and `**/*secret*` in every directory"
            ));
        }
    }
    let floor = floor_paths(host, project);
    let allow_write = merged(&layers, "/sandbox/filesystem/allowWrite", found);
    let allow = merged(&layers, "/permissions/allow", found);
    let directories = merged(&layers, "/permissions/additionalDirectories", found);
    let edit_allows = allow.iter().filter_map(|rule| {
        let rule = rule.trim();
        if rule == "Edit" || rule == "Write" {
            return Some("**".to_string());
        }
        rule.strip_prefix("Edit(")
            .or_else(|| rule.strip_prefix("Write("))
            .and_then(|inner| inner.strip_suffix(')'))
            .map(str::to_string)
    });
    for (kind, entry) in allow_write
        .iter()
        .map(|e| ("`sandbox.filesystem.allowWrite`", e.clone()))
        .chain(edit_allows.map(|e| ("an `Edit` allow rule", e)))
        .chain(
            directories
                .iter()
                .map(|e| ("`permissions.additionalDirectories`", e.clone())),
        )
    {
        if reaches_floor(&entry, host, project, &floor) {
            found.push(format!(
                "{kind} entry `{entry}` reaches a protected path the sandbox floor denies"
            ));
        }
    }
}

/// The `allowRead` entries the shipped presets carry.
fn shipped_allow_read() -> Vec<String> {
    let shipped: Value = serde_json::from_str(include_str!(
        "../../../../assets/base/settings/default.json"
    ))
    .unwrap_or(Value::Null);
    shipped
        .pointer("/sandbox/filesystem/allowRead")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The settings that weaken containment, anywhere under `sandbox` in a
/// layer.
fn escape_settings(layer: &Layer, found: &mut Vec<String>) {
    fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
        let Value::Object(map) = value else {
            return;
        };
        for (key, child) in map {
            let here = format!("{path}.{key}");
            let set = match key.as_str() {
                "enableWeakerNestedSandbox"
                | "enableWeakerNetworkIsolation"
                | "allowNestedUserNamespaces"
                | "allowAllUnixSockets"
                | "allowAppleEvents" => child != &Value::Bool(false),
                "allowUnixSockets" => !matches!(child, Value::Array(items) if items.is_empty()),
                "ignoreViolations" => !child.is_null(),
                _ => false,
            };
            if set {
                out.push(here.clone());
            }
            walk(child, &here, out);
        }
    }
    let mut keys = Vec::new();
    if let Some(sandbox) = layer.value.get("sandbox") {
        walk(sandbox, "sandbox", &mut keys);
    }
    for key in keys {
        found.push(format!(
            "{} sets `{key}`, which weakens the sandbox",
            layer.name
        ));
    }
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

/// The protected paths as absolute paths for a session in `project`: the
/// enforcement paths, startup files and git configuration files.
fn floor_paths(host: &Host, project: &Path) -> Vec<PathBuf> {
    protected_edit_denies()
        .iter()
        .filter_map(|rule| actions::edit_deny_write_path(rule))
        .filter_map(|path| absolute(&path, host, project))
        .collect()
}

/// A settings path read as absolute: `~/` under the home, `//` and `/`
/// from the file system root, anything else under `base`.
fn absolute(entry: &str, host: &Host, base: &Path) -> Option<PathBuf> {
    if let Some(rest) = entry.strip_prefix("~/") {
        return host.home.as_ref().map(|home| home.join(rest));
    }
    if entry == "~" {
        return host.home.clone();
    }
    if let Some(rest) = entry.strip_prefix("//") {
        return Some(PathBuf::from("/").join(rest));
    }
    if entry.starts_with('/') {
        return Some(PathBuf::from(entry));
    }
    Some(base.join(entry.trim_start_matches("./")))
}

/// Whether an allow entry is at or under a protected path, or is a pattern
/// whose literal start could reach one. Every reading the settings
/// grammars give a path is tried: absolute, under the project and under the
/// home.
fn reaches_floor(entry: &str, host: &Host, project: &Path, floor: &[PathBuf]) -> bool {
    let glob_at = entry.find(['*', '?', '[', '{']);
    let literal = glob_at.map_or(entry, |at| &entry[..at]);
    let readings: Vec<PathBuf> = [
        absolute(literal, host, project),
        host.home
            .as_ref()
            .map(|home| home.join(literal.trim_start_matches('/'))),
        Some(project.join(literal.trim_start_matches('/'))),
    ]
    .into_iter()
    .flatten()
    .collect();
    readings.iter().any(|reading| {
        let reading = crate::security::outward::lexical_join(Path::new("/"), reading);
        floor.iter().any(|protected| {
            reading.starts_with(protected)
                || (glob_at.is_some() && protected.starts_with(&reading))
                || (glob_at.is_some() && literal_prefix_overlaps(&reading, protected))
        })
    })
}

/// Whether a pattern's literal start, cut inside a path component, can
/// still begin a protected path (`.cl*` reaches `.claude`).
fn literal_prefix_overlaps(reading: &Path, protected: &Path) -> bool {
    protected
        .as_os_str()
        .as_encoded_bytes()
        .starts_with(reading.as_os_str().as_encoded_bytes())
}

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

fn codex_triggers(host: &Host, projects: &[PathBuf], identified: bool, found: &mut Vec<String>) {
    let user_path = host
        .codex_home
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".codex")))
        .map(|dir| dir.join("config.toml"));
    let user = user_path
        .as_ref()
        .and_then(|path| read_toml(path, "Codex configuration", found).map(|t| (path.clone(), t)));
    let mut layered: Vec<(String, toml::Table)> = Vec::new();
    for dir in projects {
        let path = dir.join(".codex/config.toml");
        if let Some(table) = read_toml(&path, "Codex configuration", found) {
            if has_key(&table, "sandbox_mode") {
                found.push(format!(
                    "the Codex configuration `{}` sets `sandbox_mode`, which a permission profile does not compose with",
                    path.display()
                ));
            }
            let mut effective = user.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
            overlay(&mut effective, &table);
            layered.push((format!("`{}`", path.display()), effective));
        }
    }
    if let Some((path, table)) = &user {
        if has_key(table, "sandbox_mode") {
            found.push(format!(
                "the Codex configuration `{}` sets `sandbox_mode`, which a permission profile does not compose with",
                path.display()
            ));
        }
        if layered.is_empty() {
            layered.push((format!("`{}`", path.display()), table.clone()));
        }
    }
    if layered.is_empty() {
        if identified {
            found.push(
                "no Codex configuration file is present, so the shipped permission profile is not in force"
                    .to_string(),
            );
        }
        return;
    }
    for (name, table) in &layered {
        codex_profile_triggers(name, table, found);
    }
}

/// What a Codex filesystem rule grants, strictest first.
fn access_rank(access: &str) -> Option<u8> {
    match access {
        "deny" | "none" => Some(0),
        "read" => Some(1),
        "write" => Some(2),
        _ => None,
    }
}

/// A Codex permission profile's effective filesystem rules, keyed by scope
/// (empty, or a special root such as `:workspace_roots`) and path, and its
/// network settings, after `extends` is followed to a built-in profile.
#[derive(Default)]
struct Profile {
    filesystem: BTreeMap<(String, String), String>,
    network: BTreeMap<String, toml::Value>,
}

fn resolve_profile(table: &toml::Table, name: &str) -> Result<Profile, String> {
    let profiles = table
        .get("permissions")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "no `permissions` table defines it".to_string())?;
    let mut chain = Vec::new();
    let mut current = name.to_string();
    let mut seen = HashSet::new();
    while !current.starts_with(':') {
        if !seen.insert(current.clone()) {
            return Err(format!("`extends` loops through `{current}`"));
        }
        let profile = profiles
            .get(&current)
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("profile `{current}` is not defined"))?;
        chain.push(profile);
        current = match profile.get("extends") {
            Some(toml::Value::String(next)) => next.clone(),
            Some(_) => {
                return Err(format!(
                    "profile `{current}` has an `extends` that is not a string"
                ))
            }
            None => return Err(format!("profile `{current}` extends no built-in profile")),
        };
    }
    if current != ":workspace" && current != ":read-only" {
        return Err(format!(
            "the profile chain ends at `{current}`, not `:workspace` or `:read-only`"
        ));
    }
    let mut resolved = Profile::default();
    for profile in chain.iter().rev() {
        if let Some(filesystem) = profile.get("filesystem") {
            let filesystem = filesystem
                .as_table()
                .ok_or_else(|| "a `filesystem` entry is not a table".to_string())?;
            overlay_rules(&mut resolved.filesystem, "", filesystem)?;
        }
        if let Some(network) = profile.get("network") {
            let network = network
                .as_table()
                .ok_or_else(|| "a `network` entry is not a table".to_string())?;
            for (key, value) in network {
                if key != "domains" {
                    resolved.network.insert(key.clone(), value.clone());
                }
            }
        }
    }
    Ok(resolved)
}

fn overlay_rules(
    rules: &mut BTreeMap<(String, String), String>,
    scope: &str,
    table: &toml::Table,
) -> Result<(), String> {
    for (key, value) in table {
        match value {
            toml::Value::String(access) if access_rank(access).is_some() => {
                rules.insert((scope.to_string(), key.clone()), access.clone());
            }
            toml::Value::Table(inner) if scope.is_empty() && key.starts_with(':') => {
                overlay_rules(rules, key, inner)?;
            }
            toml::Value::Integer(_) if key == "glob_scan_max_depth" => {}
            _ => {
                return Err(format!(
                    "the filesystem entry `{key}` has a value this guard does not model"
                ))
            }
        }
    }
    Ok(())
}

/// Codex filesystem rules keyed by scope and path, with their access.
type CodexRules = BTreeMap<(String, String), String>;

/// The shipped Codex profiles: what `cf-guard` denies or keeps read-only,
/// and every write `cf-guard` or `cf-builder` grants.
fn shipped_codex() -> (CodexRules, HashSet<(String, String)>) {
    let shipped: toml::Table = include_str!("../../../../assets/base/codex/config.toml")
        .parse()
        .unwrap_or_default();
    let guard = resolve_profile(&shipped, "cf-guard").unwrap_or_default();
    let builder = resolve_profile(&shipped, "cf-builder").unwrap_or_default();
    let protected = guard
        .filesystem
        .iter()
        .filter(|(_, access)| access.as_str() != "write")
        .map(|(key, access)| (key.clone(), access.clone()))
        .collect();
    let writes = guard
        .filesystem
        .iter()
        .chain(builder.filesystem.iter())
        .filter(|(_, access)| access.as_str() == "write")
        .map(|(key, _)| key.clone())
        .collect();
    (protected, writes)
}

/// The network keys whose `true` widens what a shipped profile allows.
const CODEX_NETWORK_WIDENERS: &[&str] = &[
    "allow_local_binding",
    "allow_upstream_proxy",
    "dangerously_allow_non_loopback_proxy",
    "dangerously_allow_all_unix_sockets",
];

/// Judge the effective Codex configuration `table` by what its default
/// profile grants, not by the profile's name.
fn codex_profile_triggers(name: &str, table: &toml::Table, found: &mut Vec<String>) {
    match table.get("approval_policy").and_then(toml::Value::as_str) {
        Some("never") => {}
        other => found.push(format!(
            "the Codex approval policy in {name} is {} rather than `never`, so an escalation can be approved",
            other.map_or_else(|| "unset".to_string(), |v| format!("`{v}`"))
        )),
    }
    let Some(profile_name) = table
        .get("default_permissions")
        .and_then(toml::Value::as_str)
    else {
        found.push(format!(
            "the Codex configuration {name} selects no default permission profile"
        ));
        return;
    };
    let profile = match resolve_profile(table, profile_name) {
        Ok(profile) => profile,
        Err(why) => {
            found.push(format!(
                "the Codex permission profile `{profile_name}` in {name} cannot be judged: {why}"
            ));
            return;
        }
    };
    let (protected, writes) = shipped_codex();
    for ((scope, path), access) in &protected {
        let shown = if scope.is_empty() {
            format!("`{path}`")
        } else {
            format!("`{path}` under `{scope}`")
        };
        let have = profile.filesystem.get(&(scope.clone(), path.clone()));
        let weaker = match have {
            None => true,
            Some(have) => access_rank(have) > access_rank(access),
        };
        if weaker {
            found.push(format!(
                "the Codex permission profile `{profile_name}` in {name} does not keep the shipped rule {shown} = `{access}`"
            ));
        }
    }
    for ((scope, path), access) in &profile.filesystem {
        if access == "write" && !writes.contains(&(scope.clone(), path.clone())) {
            found.push(format!(
                "the Codex permission profile `{profile_name}` in {name} grants a write the shipped profiles do not: `{path}`"
            ));
        }
    }
    for key in CODEX_NETWORK_WIDENERS {
        match profile.network.get(*key) {
            None | Some(toml::Value::Boolean(false)) => {}
            Some(_) => found.push(format!(
                "the Codex permission profile `{profile_name}` in {name} sets `network.{key}`"
            )),
        }
    }
}

/// Whether `key` appears in the table at any depth.
fn has_key(table: &toml::Table, key: &str) -> bool {
    table.iter().any(|(name, value)| {
        name == key || value.as_table().is_some_and(|inner| has_key(inner, key))
    })
}

/// The shipped Grok profiles.
fn shipped_grok() -> toml::Table {
    include_str!("../../../../assets/base/grok/sandbox.toml")
        .parse()
        .unwrap_or_default()
}

/// The string members of `profile`'s array `list`, or why it is not an
/// array of strings.
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

fn grok_triggers(projects: &[PathBuf], identified: bool, found: &mut Vec<String>) {
    let shipped = shipped_grok();
    let shipped_profiles = shipped
        .get("profiles")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default();
    let mut any = false;
    for dir in projects {
        let path = dir.join(".grok/sandbox.toml");
        let present = crate::absence::symlink_metadata_optional(&path);
        if !matches!(present, Ok(None)) {
            any = true;
        }
        let Some(table) = read_toml(&path, "Grok sandbox profiles", found) else {
            continue;
        };
        let name = format!("`{}`", path.display());
        let profiles = table
            .get("profiles")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        for (profile, shipped) in &shipped_profiles {
            let Some(shipped) = shipped.as_table() else {
                continue;
            };
            let Some(have) = profiles.get(profile).and_then(toml::Value::as_table) else {
                found.push(format!(
                    "the Grok sandbox profiles {name} lack the shipped profile `{profile}`"
                ));
                continue;
            };
            if have.get("extends") != shipped.get("extends") {
                found.push(format!(
                    "the Grok profile `{profile}` in {name} does not extend what the shipped profile extends"
                ));
            }
            for list in ["read_only", "deny"] {
                let want = grok_list(shipped, list).unwrap_or_default();
                match grok_list(have, list) {
                    Err(why) => {
                        found.push(format!("the Grok profile `{profile}` in {name}: {why}"));
                    }
                    Ok(entries) => {
                        for entry in want {
                            if !entries.contains(&entry) {
                                found.push(format!(
                                    "the Grok profile `{profile}` in {name} lacks the shipped `{list}` entry `{entry}`"
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    if !any && identified {
        found.push(
            "no Grok sandbox profiles file (`.grok/sandbox.toml`) is present, so the shipped profiles are not in force"
                .to_string(),
        );
    }
}

#[cfg(test)]
#[path = "sandbox_state_tests.rs"]
mod tests;
