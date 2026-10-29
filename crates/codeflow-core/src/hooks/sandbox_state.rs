//! Which text rule judges a shell call (TSK-397).
//!
//! The exec guard has two outcomes. **Strict** adds the strict branch, which
//! judges every word after a program the guard cannot read through. It is
//! the default and applies to every call unless the operator chose
//! otherwise. **Relaxed** is the line's rule. It applies only when every
//! policy source the hook reads sets `security.text_guard` to `relaxed`,
//! read from the protected remote-tracking refs: the call's repository and,
//! for a Claude Code call, the session's project. A working copy, a branch
//! and a repository without a remote never relax the rule.
//!
//! Relaxed is the operator's choice, not something the guard verifies. An
//! earlier revision chose it by attesting each harness's sandbox
//! configuration; every review round found another layer or key it did not
//! read, so that attestation was removed (operator, 2026-10-29). Under
//! relaxed, two facts the guard does read still select the strict rule: a
//! call marked `dangerouslyDisableSandbox`, and native Windows, where Claude
//! Code and Grok Build have no sandbox. The strict outcome keeps every
//! finding of the relaxed one.
//!
//! The Claude Code settings reader below decides nothing. `codeflow doctor`
//! uses it to check the state root against the write grants a session's
//! settings carry.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::landed_policy::LandedPolicy;
use super::policy::TextGuard;

/// Where a call runs, as the text-rule selection and doctor see it.
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
    /// Claude Code's managed settings file and its drop-in directory.
    pub managed_file: Option<PathBuf>,
    pub managed_dir: Option<PathBuf>,
    /// Native Windows, where Claude Code and Grok Build have no sandbox.
    pub native_windows: bool,
}

/// The debug-build variable that moves the managed settings location, so a
/// test is not judged by the host's own settings. A release build never
/// reads it.
pub const TEST_MANAGED_DIR: &str = "CODEFLOW_TEST_CLAUDE_MANAGED_DIR";

/// The variable Claude Code exports to every hook: the directory its
/// session started in.
pub const CLAUDE_PROJECT_DIR: &str = "CLAUDE_PROJECT_DIR";

impl Host {
    /// The host a hook process sees for a call in `cwd`, whose repository
    /// root is `root`.
    #[must_use]
    pub fn for_hook(cwd: &Path, root: &Path) -> Self {
        Self::with_session(env_path(CLAUDE_PROJECT_DIR), cwd, root)
    }

    /// The host `codeflow doctor` sees: a session started at `root`.
    #[must_use]
    pub fn for_session(root: &Path) -> Self {
        Self::with_session(Some(root.to_path_buf()), root, root)
    }

    fn with_session(session: Option<PathBuf>, cwd: &Path, root: &Path) -> Self {
        let managed_base = test_managed_dir().or_else(managed_base);
        Self {
            session,
            cwd: cwd.to_path_buf(),
            root: root.to_path_buf(),
            home: crate::portable_path::user_home(),
            claude_config_dir: env_path("CLAUDE_CONFIG_DIR"),
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

/// The test location of the managed settings, in a debug build only.
fn test_managed_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        env_path(TEST_MANAGED_DIR)
    } else {
        None
    }
}

/// Which rule judges a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The line's rule, by the operator's choice.
    Relaxed,
    /// The line's rule and the strict branch, with the first reason that
    /// selected it.
    Strict(String),
}

impl Outcome {
    /// The reason the strict branch's findings name, if strict.
    #[must_use]
    pub fn trigger(&self) -> Option<&str> {
        match self {
            Self::Relaxed => None,
            Self::Strict(reason) => Some(reason),
        }
    }
}

/// Why one policy source keeps the strict rule: it does not set
/// `security.text_guard` to `relaxed`, or it was not read from the
/// protected remote-tracking refs. `None` when it chooses relaxed. `whose`
/// names the source in the reason, such as "the project policy".
#[must_use]
pub fn policy_reason(authority: &LandedPolicy, whose: &str) -> Option<String> {
    if authority.policy.security.text_guard != TextGuard::Relaxed {
        return Some(format!(
            "{whose} keeps the default `security.text_guard: strict` (read from {})",
            authority.source
        ));
    }
    if !authority.remote_tracked {
        return Some(format!(
            "{whose} sets `security.text_guard: relaxed`, but it is read from {}, not from a protected remote-tracking ref, so it is not honoured",
            authority.source
        ));
    }
    None
}

/// Every reason the call takes the strict rule, in a fixed order: the
/// policy reasons the caller passes, the retry flag, then native Windows.
/// Empty means relaxed. The hook and `codeflow doctor` both decide through
/// this function.
#[must_use]
pub fn reasons(host: &Host, retry: bool, policy: &[String]) -> Vec<String> {
    let mut found = policy.to_vec();
    if retry {
        found.push(
            "the call asks to run outside the sandbox (`dangerouslyDisableSandbox`)".to_string(),
        );
    }
    if host.native_windows {
        found.push(
            "native Windows has no sandbox under Claude Code or Grok Build, and the guard does not establish which harness made the call"
                .to_string(),
        );
    }
    found
}

/// The outcome for one call: the first of [`reasons`], or relaxed.
#[must_use]
pub fn outcome(host: &Host, retry: bool, policy: &[String]) -> Outcome {
    reasons(host, retry, policy)
        .into_iter()
        .next()
        .map_or(Outcome::Relaxed, Outcome::Strict)
}

// ------------------------------------------------- Claude settings reader

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

/// The directories whose Claude settings a session started in `session`
/// may read: the session directory, its repository root and, for a linked
/// worktree, the main checkout; with why any could not be established.
fn claude_projects(session: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut dirs = vec![session.to_path_buf()];
    let mut unresolved = Vec::new();
    match repository_root(session) {
        Ok(Some(root)) if !dirs.contains(&root) => dirs.push(root),
        Ok(_) => {}
        Err(why) => unresolved.push(format!(
            "the repository of the session project `{}` cannot be read ({why})",
            session.display()
        )),
    }
    for dir in dirs.clone() {
        match main_checkout(&dir) {
            Ok(Some(main)) if !dirs.contains(&main) => dirs.push(main),
            Ok(_) => {}
            Err(why) => unresolved.push(format!(
                "the main checkout of the worktree at `{}` cannot be established ({why})",
                dir.display()
            )),
        }
    }
    (dirs, unresolved)
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
/// reader, checked again on the open handle, and read up to the limit.
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

/// One Claude settings file, read.
struct Layer {
    name: String,
    value: Value,
}

/// The Claude settings files a session in `host.session` may read, read;
/// each one that cannot be read or is not a JSON object is a problem in
/// `problems`.
fn claude_layers(host: &Host, problems: &mut Vec<String>) -> Vec<Layer> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let user_dir = host
        .claude_config_dir
        .clone()
        .or_else(|| host.home.as_ref().map(|home| home.join(".claude")));
    if let Some(dir) = &user_dir {
        paths.push(dir.join("settings.json"));
    }
    if let Some(session) = &host.session {
        let (dirs, unresolved) = claude_projects(session);
        problems.extend(unresolved);
        for dir in dirs {
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
                        Err(error) => problems.push(format!(
                            "the managed settings directory `{}` cannot be listed ({error})",
                            dir.display()
                        )),
                    }
                }
                files.sort();
                paths.extend(files);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => problems.push(format!(
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
                problems.push(format!("the Claude settings {name} cannot be read ({why})"));
            }
            Read::Parsed(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_object() => layers.push(Layer { name, value }),
                Ok(_) | Err(_) => {
                    problems.push(format!("the Claude settings {name} are not a JSON object"));
                }
            },
        }
    }
    layers
}

/// Each `sandbox.filesystem.allowWrite` and
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

#[cfg(test)]
#[path = "sandbox_state_tests.rs"]
mod tests;
