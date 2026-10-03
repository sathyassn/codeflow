//! Infrastructure health checks.
//!
//! The doctor core: a small registry of named checks that run concurrently
//! and report pass/warn/fail with timing. The CLI layer renders results and
//! adds the enforcement-matrix view (charter section 3.1). Checks verify the
//! v2 surface only: the binary's hook subcommands, harness availability,
//! `.codeflow/` config validity and writability, and network reachability.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::DoctorError;
use crate::model_qualification;
use crate::remedy::{self, Finding, Remedy};
use crate::scaffold::manifest::{Ownership, RegionFormat};
use crate::scaffold::region;
use crate::scaffold::sha256_hex;
use crate::scaffold::state::InstalledManifest;

mod ci_pin;

/// Outcome of a health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    /// A warning, with the step that clears it (SPC-013 R-80).
    Warn(Remedy),
    /// A state doctor cannot read, with the manual step that confirms it
    /// (TSK-147 AC-1). Never fails and never warns.
    Note(Remedy),
}

impl Status {
    /// Whether this is a warning.
    #[must_use]
    pub fn is_warn(&self) -> bool {
        matches!(self, Self::Warn(_))
    }
}

/// Result of a single doctor check.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub message: String,
    #[serde(serialize_with = "duration_millis::serialize")]
    pub duration: Duration,
}

/// Serialization helper for Duration as milliseconds.
mod duration_millis {
    use serde::Serializer;
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        s.serialize_u64(ms)
    }
}

/// The user's home directory (`HOME`, else `USERPROFILE`), or the current
/// directory when neither is set.
fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// Callback to locate an executable by name.
type LookPathFn = fn(&str) -> Result<String, String>;

/// Callback to read an environment variable.
type EnvVarFn = fn(&str) -> Option<String>;

/// Callback to execute a command with arguments.
type ExecCommandFn = fn(&str, &[&str]) -> Result<String, String>;

/// Callback to execute a command with UTF-8 stdin.
type ExecCommandStdinFn = fn(&str, &[&str], &str) -> Result<String, String>;

/// Configuration for doctor checks.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Project root directory.
    pub project_dir: String,
    /// Locates executables. Returns Ok(path) or Err.
    pub look_path: Option<LookPathFn>,
    /// Runs a command and returns its output.
    pub exec_command: Option<ExecCommandFn>,
    /// Runs a command with text on stdin and returns its output.
    pub exec_command_stdin: Option<ExecCommandStdinFn>,
    /// User-owned `CodeFlow` home holding the personal catalog overlay and the
    /// recorded canary observations. `None` reads neither; the CLI supplies
    /// `CODEFLOW_HOME`. Never derived from `qualification_dir`.
    pub codeflow_home: Option<PathBuf>,
    /// User-owned qualified-binding directory. `None` loads no binding
    /// records; the CLI supplies `CODEFLOW_HOME/qualified-bindings`.
    pub qualification_dir: Option<PathBuf>,
    /// The home directory whose `.codex/` and `.grok/` hold the harnesses'
    /// trust records. `None` is the user's home (Codex honours
    /// `CODEX_HOME`); tests pass a directory of their own.
    pub harness_home: Option<PathBuf>,
    /// Reads the harnesses' environment variables (`CODEX_HOME`,
    /// `GROK_HOME`, `GROK_FOLDER_TRUST`); `None` reads the process
    /// environment.
    pub env_var: Option<EnvVarFn>,
}

impl Options {
    fn env(&self, name: &str) -> Option<String> {
        self.env_var
            .map_or_else(|| std::env::var(name).ok(), |read| read(name))
    }

    /// Where Codex keeps its state: `CODEX_HOME`, else `~/.codex`.
    fn codex_home(&self) -> PathBuf {
        match &self.harness_home {
            Some(home) => home.join(".codex"),
            None => self
                .env("CODEX_HOME")
                .filter(|home| !home.is_empty())
                .map_or_else(|| user_home().join(".codex"), PathBuf::from),
        }
    }

    /// Where Grok keeps its state: `GROK_HOME` when set and not empty, else
    /// `~/.grok` (xai-dirs `resolve_grok_home`).
    fn grok_home(&self) -> PathBuf {
        match &self.harness_home {
            Some(home) => home.join(".grok"),
            None => self
                .env("GROK_HOME")
                .filter(|home| !home.is_empty())
                .map_or_else(|| user_home().join(".grok"), PathBuf::from),
        }
    }

    fn do_look_path(&self, name: &str) -> Result<String, String> {
        if let Some(f) = self.look_path {
            f(name)
        } else {
            which::which(name)
                .map(|p| p.to_string_lossy().to_string())
                .map_err(|e| e.to_string())
        }
    }

    fn do_exec(&self, cmd: &str, args: &[&str]) -> Result<String, String> {
        if let Some(f) = self.exec_command {
            f(cmd, args)
        } else {
            let output = crate::git::process(cmd)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }
    }

    fn do_exec_bounded(
        &self,
        cmd: &str,
        args: &[&str],
        timeout: Duration,
    ) -> Result<String, String> {
        if let Some(f) = self.exec_command {
            f(cmd, args)
        } else {
            run_bounded_command(cmd, args, timeout)
        }
    }

    fn do_exec_stdin(&self, cmd: &str, args: &[&str], stdin: &str) -> Result<String, String> {
        if let Some(f) = self.exec_command_stdin {
            f(cmd, args, stdin)
        } else {
            let mut child = crate::git::process(cmd)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| error.to_string())?;
            child
                .stdin
                .as_mut()
                .ok_or_else(|| "command stdin was not piped".to_string())?
                .write_all(stdin.as_bytes())
                .map_err(|error| error.to_string())?;
            let output = child
                .wait_with_output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }
    }
}

/// Hook subcommands that must be functional (charter section 3.3): the
/// three Claude-layer hooks plus the five git-hook shims.
const HOOK_SUBCOMMANDS: &[(&str, &str)] = &[
    ("hook", "git-guard"),
    ("hook", "session-orient"),
    ("hook", "session-summary"),
    ("git-hook", "pre-commit"),
    ("git-hook", "commit-msg"),
    ("git-hook", "pre-merge-commit"),
    ("git-hook", "reference-transaction"),
    ("git-hook", "pre-push"),
];

/// Shipped-asset path (`src` in the scaffold manifest) of the CI workflow;
/// used to locate the installed workflow's dest in the installed-file record,
/// so a relocated workflow is still found.
const CI_ASSET_SRC: &str = "ci/codeflow-ci.yml";

/// Canonical dest for the CI workflow when the installed manifest is silent.
const CI_DEFAULT_DEST: &str = ".github/workflows/codeflow-ci.yml";

/// Sentinels of the placeholder install steps older scaffolds shipped: the
/// GitHub workflow's loud `::error::`, and the copy-in platform files'
/// comment. Current templates install the target-pinned release instead.
const CI_PLACEHOLDER_MARKS: [&str; 2] = [
    "install step is an unwired PLACEHOLDER",
    "PLACEHOLDER: install the codeflow binary",
];
const MAX_SETTINGS_BYTES: u64 = 16 * 1024 * 1024;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION_PROBE_OUTPUT_BYTES: usize = 64 * 1024;
const VERSION_PROBE_POLL: Duration = Duration::from_millis(10);

/// Ordered list of all check names.
const CHECK_NAMES: &[&str] = &[
    "hooks",
    "claude",
    "codex",
    "grok",
    "config",
    "permissions",
    "policy-source",
    "network",
    "delegates",
    "model-bindings",
    "delegate-roundtrip",
    "repo-integrity",
    "ci-perimeter",
    "managed-drift",
    "customization",
    "instructions",
    "reading",
    "test-config",
    "id-registry",
    "adopter-fit",
];

/// Return the ordered list of all available check names.
#[must_use]
pub fn check_names() -> Vec<&'static str> {
    CHECK_NAMES.to_vec()
}

/// Type alias for individual check functions.
type CheckFn = fn(&Options) -> CheckResult;

/// Build the check registry mapping names to functions.
fn check_registry() -> HashMap<&'static str, CheckFn> {
    let mut m: HashMap<&'static str, CheckFn> = HashMap::new();
    m.insert("hooks", check_hooks);
    m.insert("policy-source", check_policy_source);
    m.insert("claude", check_claude);
    m.insert("codex", check_codex);
    m.insert("grok", check_grok);
    m.insert("config", check_config);
    m.insert("permissions", check_permissions);
    m.insert("network", check_network);
    m.insert("delegates", check_delegates);
    m.insert("model-bindings", check_model_bindings);
    m.insert("delegate-roundtrip", check_delegate_roundtrip);
    m.insert("repo-integrity", check_repo_integrity);
    m.insert("ci-perimeter", check_ci_perimeter);
    m.insert("managed-drift", check_managed_drift);
    m.insert("customization", check_customization);
    m.insert("instructions", check_instructions);
    m.insert("reading", check_reading);
    m.insert("test-config", check_test_config);
    m.insert("id-registry", check_id_registry);
    m.insert("adopter-fit", check_adopter_fit);
    m
}

/// Run all checks concurrently and return results in canonical order.
///
/// Uses `tokio::task::spawn_blocking` for concurrent execution. Individual
/// check failures are captured in the returned `CheckResult` items.
pub async fn run_all(opts: &Options) -> Vec<CheckResult> {
    let registry = check_registry();
    let mut handles = Vec::with_capacity(CHECK_NAMES.len());

    for &name in CHECK_NAMES {
        let f = registry[name];
        let opts_clone = opts.clone();
        handles.push(tokio::task::spawn_blocking(move || f(&opts_clone)));
    }

    let mut results = Vec::with_capacity(CHECK_NAMES.len());
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => results.push(CheckResult {
                name: "unknown".to_string(),
                status: Status::Fail,
                message: format!("task join error: {e}"),
                duration: Duration::ZERO,
            }),
        }
    }
    results
}

/// Run a single named check.
///
/// # Errors
///
/// Returns `DoctorError::CheckNotFound` if the name is not in the registry.
pub fn run_check(name: &str, opts: &Options) -> Result<CheckResult, DoctorError> {
    let registry = check_registry();
    let f = registry
        .get(name)
        .ok_or_else(|| DoctorError::CheckNotFound(name.to_string()))?;
    Ok(f(opts))
}

// ---------------------------------------------------------------------------
// Individual check functions
// ---------------------------------------------------------------------------

fn check_hooks(opts: &Options) -> CheckResult {
    let start = Instant::now();

    let Ok(codeflow_bin) = opts.do_look_path("codeflow") else {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: format!(
                "codeflow binary not found in PATH; {}; then codeflow update",
                crate::hooks::landed_policy::INSTALL
            ),
            duration: start.elapsed(),
        };
    };

    if opts
        .do_exec(&codeflow_bin, &["git-hook", "capabilities"])
        .map_or(true, |s| s.trim() != "hooks 3")
    {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: format!(
                "codeflow binary is older than hook contract 3; {}; then codeflow update",
                crate::hooks::landed_policy::INSTALL
            ),
            duration: start.elapsed(),
        };
    }
    let mut failing = Vec::new();
    for &(group, subcommand) in HOOK_SUBCOMMANDS {
        if opts
            .do_exec(&codeflow_bin, &[group, subcommand, "--help"])
            .is_err()
        {
            failing.push(format!("{group} {subcommand}"));
        }
    }

    if !failing.is_empty() {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: format!(
                "{} hook subcommand(s) not responding: {}",
                failing.len(),
                failing.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    // Responding subcommands prove the binary answers — not that git will
    // call it. A fresh clone keeps the committed shims but loses the local
    // `core.hooksPath` wiring, leaving zero local git gates behind a green
    // doctor. Resolve the ACTIVE hooks dir the same way orient's gates line
    // does and warn when the shims are not what git runs.
    let root = Path::new(&opts.project_dir);
    match hooks_wiring(root) {
        Wiring::Read => {
            if let Some(warning) = git_dir_hooks_finding(root) {
                return CheckResult {
                    name: "hooks".into(),
                    status: Status::Warn(warning.remedy),
                    message: warning.text,
                    duration: start.elapsed(),
                };
            }
        }
        Wiring::Broken(warning) => {
            return CheckResult {
                name: "hooks".into(),
                status: Status::Warn(warning.remedy),
                message: warning.text,
                duration: start.elapsed(),
            };
        }
        Wiring::Unverified(note) => {
            return CheckResult {
                name: "hooks".into(),
                status: Status::Note(note.remedy),
                message: note.text,
                duration: start.elapsed(),
            };
        }
    }

    CheckResult {
        name: "hooks".into(),
        status: Status::Pass,
        message: format!("all {} hook subcommands functional", HOOK_SUBCOMMANDS.len()),
        duration: start.elapsed(),
    }
}

/// Hooks in the repository's own hooks folder that git stopped running when
/// `core.hooksPath` was set to the codeflow shims (`pre-commit install`
/// output, or hooks written by hand). Init reported them as a choice; this
/// keeps the choice visible until the adopter makes it.
fn git_dir_hooks_finding(root: &Path) -> Option<Finding> {
    use crate::scaffold::detect::{self, CODEFLOW_HOOKS_PATH};
    if detect::configured_hooks_path(root).as_deref() != Some(CODEFLOW_HOOKS_PATH) {
        return None;
    }
    let found = detect::git_dir_hooks(root)?;
    Some(Finding::new(
        format!(
            "git does not run the hooks in {} while core.hooksPath = {CODEFLOW_HOOKS_PATH}: {}",
            found.dir,
            found.names.join(", ")
        ),
        remedy::DOCTOR_GIT_DIR_HOOKS.with(&[("path", &found.dir)]),
    ))
}

/// What reading the hook files shows about git calling the codeflow shims.
enum Wiring {
    /// Nothing to verify (no shims, not a git repository), or git's active
    /// hooks dir is the shims dir itself.
    Read,
    /// A hook git runs cannot call its shim: missing, not executable, or no
    /// shim named outside a comment. Reading proves this negative.
    Broken(Finding),
    /// Another manager's hooks are executable and name every shim. Reading
    /// cannot prove they run it (TSK-147 round 4 F6), so this is a note
    /// that names the git event which confirms it.
    Unverified(Finding),
}

/// Whether git's active hooks dir (resolved like `orient`: `core.hooksPath`,
/// else the common dir's `hooks/`) runs the codeflow shims the repo ships.
/// It flags, it never guesses: a recorded `git_hooks = "unwired"` (another
/// hook manager owned the hooks at init, deliberately not clobbered) gets
/// its own remedy text.
fn hooks_wiring(root: &Path) -> Wiring {
    use crate::scaffold::detect::CODEFLOW_HOOKS_PATH;
    use crate::scaffold::state::{ProjectState, GIT_HOOKS_UNWIRED};

    let shims = root.join(CODEFLOW_HOOKS_PATH);
    if !shims.join("pre-commit").exists() {
        return Wiring::Read; // no scaffolded shims — nothing to wire
    }
    // Prefer the configured string: relative `.codeflow/git-hooks` is the
    // contract so each worktree uses its own shims. An absolute path (often
    // the main checkout) is a Warn even if the files happen to exist.
    if let Some(configured) = crate::scaffold::detect::configured_hooks_path(root) {
        if configured == CODEFLOW_HOOKS_PATH {
            return Wiring::Read;
        }
        if std::path::Path::new(&configured).is_absolute() {
            return Wiring::Broken(Finding::new(
                format!(
                    "core.hooksPath is absolute ({configured}); the project-relative `{CODEFLOW_HOOKS_PATH}` lets each worktree use its own shims"
                ),
                remedy::DOCTOR_HOOKS_PATH.with(&[("hooks", CODEFLOW_HOOKS_PATH)]),
            ));
        }
    }
    let Some(active) = crate::hooks::orient::git_hooks_dir(root) else {
        return Wiring::Read;
    };
    let wired = match (active.canonicalize(), shims.canonicalize()) {
        (Ok(a), Ok(s)) => a == s,
        _ => active == shims,
    };
    if wired {
        return Wiring::Read;
    }
    let path = active
        .strip_prefix(root)
        .unwrap_or(&active)
        .display()
        .to_string();
    let uncalled = shims_not_called(&active, &shims);
    if uncalled.is_empty() {
        return Wiring::Unverified(Finding::new(
            format!(
                "the hooks git runs in {path} are executable and name every codeflow shim (wiring not verified: reading a hook cannot show that it runs the shim)"
            ),
            remedy::DOCTOR_HOOK_WIRING_UNSEEN.with(&[("path", &path)]),
        ));
    }

    let recorded_unwired = ProjectState::exists(root)
        && ProjectState::load(root).is_ok_and(|s| s.git_hooks == GIT_HOOKS_UNWIRED);
    Wiring::Broken(if recorded_unwired {
        Finding::new(
            format!(
                "codeflow shims are not git's active hooks (recorded git_hooks = \"unwired\": another hook manager owns {path}), and its hooks do not call: {}",
                uncalled.join(", ")
            ),
            remedy::DOCTOR_HOOK_MANAGER.with(&[("path", &path), ("hooks", &uncalled.join(", "))]),
        )
    } else {
        Finding::new(
            format!(
                "hook subcommands respond, but the codeflow shims are not git's active hooks (fresh clone?), and the hooks git runs do not call: {}",
                uncalled.join(", ")
            ),
            remedy::DOCTOR_HOOKS_PATH.with(&[("hooks", CODEFLOW_HOOKS_PATH)]),
        )
    })
}

/// The shims in `shims` that git's active hooks in `active` do not call,
/// each with why. A hook calls its shim when git runs it, which needs the
/// file to be executable (githooks(5): a hook that is not executable is
/// ignored), and a live line of it, not a comment, names
/// `.codeflow/git-hooks/<name>`. An empty result proves nothing more: a
/// named shim may still never run, which only a git event shows.
fn shims_not_called(active: &Path, shims: &Path) -> Vec<String> {
    use crate::scaffold::detect::CODEFLOW_HOOKS_PATH;
    let mut names: Vec<String> = std::fs::read_dir(shims)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let hook = active.join(&name);
            let why = match std::fs::read_to_string(&hook) {
                Err(_) => "no hook",
                Ok(_) if !crate::scaffold::detect::is_executable(&hook) => "not executable",
                Ok(text) => {
                    let call = format!("{CODEFLOW_HOOKS_PATH}/{name}");
                    if text.lines().any(|line| shell_code(line).contains(&call)) {
                        return None;
                    }
                    "no live call"
                }
            };
            Some(format!("{name} ({why})"))
        })
        .collect()
}

/// A shell line without its comment: from a `#` that starts a word.
fn shell_code(line: &str) -> &str {
    let mut previous = ' ';
    for (at, c) in line.char_indices() {
        if c == '#' && previous.is_whitespace() {
            return &line[..at];
        }
        previous = c;
    }
    line
}

fn check_claude(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_look_path("claude") {
        Ok(_) => CheckResult {
            name: "claude".into(),
            status: Status::Pass,
            message: "claude CLI found".into(),
            duration: start.elapsed(),
        },
        // Warn, not fail: git hooks + CI are the canonical enforcement
        // plane (charter section 9); codeflow works without a harness.
        Err(_) => CheckResult {
            name: "claude".into(),
            status: Status::Warn(
                remedy::DOCTOR_TOOL_MISSING
                    .with(&[("tool", "the claude CLI"), ("check", "claude")]),
            ),
            message: "claude CLI not found in PATH (Claude-layer hooks inactive)".into(),
            duration: start.elapsed(),
        },
    }
}

/// Codex harness wiring (ADR-0008). `.codex/hooks.json` binds the same
/// `codeflow hook` guards to an interactive codex session that
/// `.claude/settings.json` binds to Claude Code, but only after a one-time
/// trust step (`/hooks` inside codex). Doctor reads Codex's recorded trust
/// statically, and a static reading proves only that a hook does NOT run:
/// an untrusted, changed or disabled hook is a Warn with the one-time step.
/// A configuration that matches is a Note, "configured; runtime not
/// verified", naming the real hook event that verifies it; doctor never
/// passes it without an observed run (TSK-147 round 3). A linked worktree,
/// whose hooks Codex takes from the main checkout, is a note that doctor
/// cannot verify it. Warn, not fail: git hooks + CI bind a codex session
/// regardless (charter section 9); the in-session layer is fast feedback,
/// not the boundary.
fn check_codex(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let hooks_json = Path::new(&opts.project_dir)
        .join(".codex")
        .join("hooks.json");

    if !hooks_json.exists() {
        return CheckResult {
            name: "codex".into(),
            status: Status::Pass,
            message: "no .codex/hooks.json (codex in-session hook plane not scaffolded)".into(),
            duration: start.elapsed(),
        };
    }

    let presence = if opts.do_look_path("codex").is_ok() {
        "codex CLI found"
    } else {
        "codex CLI not found in PATH"
    };
    let approve =
        "run `/hooks` inside interactive codex once and approve and enable the CodeFlow hooks";
    let observe = "start codex in this project and confirm a real hook event ran, such as the CodeFlow session-orient context at session start";
    let root = Path::new(&opts.project_dir);
    let trust = if crate::hooks::RepoInfo::discover(root).is_some_and(|info| info.is_worktree) {
        Err("a linked worktree, whose project hooks codex takes from the main checkout".to_string())
    } else {
        codex_hook_trust(&hooks_json, &opts.codex_home())
    };
    let (status, message) = match trust {
        Ok((trusted, total)) if trusted == total => (
            Status::Note(remedy::DOCTOR_UNSEEN.with(&[("step", observe)])),
            format!(
                ".codex/hooks.json present, {presence}: codex has {trusted} of {total} hooks trusted and enabled (configured; runtime not verified)"
            ),
        ),
        Ok((trusted, total)) => (
            Status::Warn(remedy::DOCTOR_HARNESS_APPROVAL.with(&[("step", approve), ("check", "codex")])),
            format!(
                ".codex/hooks.json present, {presence}: codex runs {trusted} of {total} hooks; an untrusted, changed or disabled hook does not run (git hooks and CI enforce regardless)"
            ),
        ),
        Err(why) => (
            Status::Note(remedy::DOCTOR_UNSEEN.with(&[("step", approve)])),
            format!(".codex/hooks.json present, {presence}: codex trust not read ({why})"),
        ),
    };
    CheckResult {
        name: "codex".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// The hook events Codex reads from a hooks file (`HookEventsToml`, Codex
/// 0.157.1); it ignores any other key.
const CODEX_EVENTS: [&str; 12] = [
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
];

/// A Codex hooks file, read with Codex's own types (`HooksFile`,
/// `MatcherGroup`, `HookHandlerConfig` in `codex-rs/config`, 0.157.1), so a
/// file Codex refuses is never counted as a set of hooks.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct CodexHooksFile {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    hooks: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct CodexMatcherGroup {
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    hooks: Vec<CodexHandler>,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum CodexHandler {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default, rename = "commandWindows", alias = "command_windows")]
        command_windows: Option<String>,
        #[serde(default, rename = "timeout")]
        timeout_sec: Option<u64>,
        #[serde(default, rename = "async")]
        asynchronous: bool,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
        #[serde(default, rename = "additionalContextLimit")]
        additional_context_limit: Option<usize>,
    },
    #[serde(rename = "mcp_tool")]
    McpTool {
        server: String,
        tool: String,
        #[serde(default)]
        input: serde_json::Map<String, serde_json::Value>,
        #[serde(default, rename = "timeout")]
        timeout_sec: Option<u64>,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
    },
    #[serde(rename = "prompt")]
    Prompt {},
    #[serde(rename = "agent")]
    Agent {},
}

/// One `hooks.state` record as Codex reads it (`HookStateToml`): a record
/// whose fields have other types is skipped whole (`config_rules.rs`).
#[derive(serde::Deserialize, Default)]
struct CodexHookState {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    trusted_hash: Option<String>,
}

/// How many of the project's Codex hooks run as they are now, of how many.
/// Codex runs a project hook only when the user's `config.toml` (the only
/// layer allowed to write hook state) holds a readable
/// `hooks.state."<hooks.json>:<event>:<group>:<handler>"` record that does
/// not disable it and whose `trusted_hash` is `sha256:` and the SHA-256 of
/// the sorted-key JSON of `{event_name, matcher, hooks: [handler]}`, the
/// handler normalized to `type`, `command`, `timeout` and `async` (checked
/// against the hashes Codex 0.157.1 recorded; an absent matcher is left
/// out, as the TOML value Codex builds the identity through cannot hold
/// it). `Err` when the hooks file does not read as Codex reads it, wires no
/// hook, or holds a hook shape that hash does not cover.
fn codex_hook_trust(hooks_json: &Path, codex_home: &Path) -> Result<(usize, usize), String> {
    let text = std::fs::read_to_string(hooks_json).map_err(|e| format!("hooks.json: {e}"))?;
    let file: CodexHooksFile =
        serde_json::from_str(&text).map_err(|e| format!("hooks.json as codex reads it: {e}"))?;
    let config = match std::fs::read_to_string(codex_home.join("config.toml")) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_err(|e| format!("codex config.toml: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(format!("codex config.toml: {e}")),
    };
    let states = codex_hook_states(&config);
    // The keys name the file as Codex resolved it; on Windows that may be
    // the plain or the verbatim (`\\?\C:\`) canonical form.
    let canonical = hooks_json.canonicalize().ok();
    let paths: BTreeSet<String> = std::iter::once(hooks_json.to_path_buf())
        .chain(canonical.clone())
        .chain(canonical.map(crate::portable_path::without_verbatim))
        .map(|path| path.display().to_string())
        .collect();
    let (mut running, mut total) = (0, 0);
    for event in CODEX_EVENTS {
        let Some(groups) = file.hooks.get(event) else {
            continue;
        };
        let groups: Vec<CodexMatcherGroup> = serde_json::from_value(groups.clone())
            .map_err(|e| format!("hooks.json {event} as codex reads it: {e}"))?;
        let event_name = snake_case(event);
        for (g, group) in groups.iter().enumerate() {
            for (h, handler) in group.hooks.iter().enumerate() {
                total += 1;
                let hash = codex_hook_hash(event, group.matcher.as_deref(), handler)
                    .map_err(|why| format!("{event} hook {g}:{h} {why}"))?;
                let runs = paths.iter().any(|path| {
                    states
                        .get(&format!("{path}:{event_name}:{g}:{h}"))
                        .is_some_and(|state| {
                            state.enabled != Some(false)
                                && state.trusted_hash.as_deref() == Some(hash.as_str())
                        })
                });
                running += usize::from(runs);
            }
        }
    }
    if total == 0 {
        return Err("hooks.json wires no hook codex reads".into());
    }
    Ok((running, total))
}

/// The trust hash Codex records for one handler of an `event` group, over
/// the identity Codex hashes once it has normalized the hook (discovery.rs
/// `append_matcher_groups`, `normalize_command_hook` and `hook_hash`,
/// 36650394): no matcher on `UserPromptSubmit`, `Stop` and `Interrupt`
/// (common.rs `matcher_pattern_for_event`), and the timeout at its default,
/// floor or cap. `Err` for what Codex skips, a matcher that does not
/// compile or an empty command, and for a handler shape doctor cannot hash.
fn codex_hook_hash(
    event: &str,
    matcher: Option<&str>,
    handler: &CodexHandler,
) -> Result<String, String> {
    let matcher = match event {
        "UserPromptSubmit" | "Stop" | "Interrupt" => None,
        _ => matcher,
    };
    if let Some(pattern) = matcher {
        let plain = pattern.is_empty()
            || pattern == "*"
            || pattern
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '|');
        if !plain && regex::Regex::new(pattern).is_err() {
            return Err(format!(
                "has a matcher {pattern:?} codex rejects, so codex skips its group"
            ));
        }
    }
    let CodexHandler::Command {
        command,
        command_windows: None,
        timeout_sec,
        asynchronous,
        status_message: None,
        additional_context_limit: None,
    } = handler
    else {
        return Err("has a shape doctor cannot hash".into());
    };
    if command.trim().is_empty() {
        return Err("has an empty command, which codex skips".into());
    }
    let timeout = match event {
        "SessionEnd" | "Interrupt" => timeout_sec.unwrap_or(1).clamp(1, 3),
        _ => timeout_sec.unwrap_or(600).max(1),
    };
    let handler = format!(
        "{{\"async\":{asynchronous},\"command\":{},\"timeout\":{timeout},\"type\":\"command\"}}",
        serde_json::Value::from(command.as_str())
    );
    // Codex builds the identity through a TOML value, which cannot hold an
    // absent matcher: the key is left out (discovery.rs).
    let matcher = matcher.map_or_else(String::new, |matcher| {
        format!(",\"matcher\":{}", serde_json::Value::from(matcher))
    });
    let identity = format!(
        "{{\"event_name\":{},\"hooks\":[{handler}]{matcher}}}",
        serde_json::Value::from(snake_case(event).as_str()),
    );
    Ok(format!("sha256:{}", sha256_hex(identity.as_bytes())))
}

/// The user's hook state records as Codex merges them: each record read
/// whole and skipped when it does not read, keys trimmed, fields merged in
/// order.
fn codex_hook_states(config: &toml::Table) -> BTreeMap<String, CodexHookState> {
    let mut states: BTreeMap<String, CodexHookState> = BTreeMap::new();
    let Some(table) = config
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table)
    else {
        return states;
    };
    for (key, value) in table {
        let Ok(state) = value.clone().try_into::<CodexHookState>() else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let merged = states.entry(key.to_string()).or_default();
        if state.enabled.is_some() {
            merged.enabled = state.enabled;
        }
        if state.trusted_hash.is_some() {
            merged.trusted_hash = state.trusted_hash;
        }
    }
    states
}

/// `PreToolUse` as Codex keys it: `pre_tool_use`.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Grok Build in-session wiring. Project hooks live in `.grok/hooks/*.json`
/// (Grok also scans `.claude/settings.json` when compat is on) and load
/// only in a trusted folder: `/hooks-trust` (or `--trust`) records it in
/// `~/.grok/trusted_folders.toml`, which this check reads. An untrusted
/// folder is a Warn; a trusted or ungated one is a Note, "configured;
/// runtime not verified", since a static reading never proves a hook runs
/// (TSK-147 round 3). Warn, not fail: git hooks + CI bind a Grok session
/// regardless.
fn check_grok(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let hooks_dir = root.join(".grok").join("hooks");
    let has_project_hooks = hooks_dir.is_dir()
        && std::fs::read_dir(&hooks_dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            })
        });

    if !has_project_hooks {
        return CheckResult {
            name: "grok".into(),
            status: Status::Pass,
            message: "no .grok/hooks/*.json (grok in-session hook plane not scaffolded)".into(),
            duration: start.elapsed(),
        };
    }

    let presence = if opts.do_look_path("grok").is_ok() {
        "grok CLI found"
    } else {
        "grok CLI not found in PATH"
    };
    let trust =
        "run `/hooks-trust` in grok inside this project once (or start grok with `--trust`)";
    let observe = "start grok in this project and confirm a real hook event ran, such as the CodeFlow guard answering a shell command";
    let env = opts.env("GROK_FOLDER_TRUST");
    let (status, message) = match grok_folder_trust(root, &opts.grok_home(), env.as_deref()) {
        GrokTrust::Trusted => (
            Status::Note(remedy::DOCTOR_UNSEEN.with(&[("step", observe)])),
            format!(".grok/hooks present, {presence}: grok trusts this folder, so project hooks should load (configured; runtime not verified)"),
        ),
        GrokTrust::Ungated => (
            Status::Note(remedy::DOCTOR_UNSEEN.with(&[("step", observe)])),
            format!(".grok/hooks present, {presence}: grok folder trust is turned off, so project hooks should load ungated (configured; runtime not verified)"),
        ),
        GrokTrust::Untrusted => (
            Status::Warn(remedy::DOCTOR_HARNESS_APPROVAL.with(&[("step", trust), ("check", "grok")])),
            format!(
                ".grok/hooks present, {presence}: grok does not trust this folder, so its project hooks are skipped (git hooks and CI enforce regardless)"
            ),
        ),
        GrokTrust::Unverifiable(why) => (
            Status::Note(remedy::DOCTOR_UNSEEN.with(&[("step", trust)])),
            format!(".grok/hooks present, {presence}: grok trust not read ({why})"),
        ),
        GrokTrust::Unreadable(why) => {
            let step = format!("repair or remove {}, then {trust}", opts.grok_home().join("trusted_folders.toml").display());
            (
                Status::Warn(remedy::DOCTOR_HARNESS_APPROVAL.with(&[("step", &step), ("check", "grok")])),
                format!(
                    ".grok/hooks present, {presence}: grok cannot read its trust store, so it trusts no folder and skips project hooks ({why})"
                ),
            )
        }
    };
    CheckResult {
        name: "grok".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// Grok's decision for a project folder.
enum GrokTrust {
    Trusted,
    Untrusted,
    /// Grok cannot read its trust store, so it trusts no folder.
    Unreadable(String),
    /// Folder trust is turned off by `GROK_FOLDER_TRUST`, the user config or
    /// the managed config.
    Ungated,
    /// Grok decides from state doctor cannot reproduce.
    Unverifiable(String),
}

/// A Grok trust store as Grok reads it (`TrustDocument` and `FolderTrust`
/// in `xai-grok-workspace/src/trust.rs`): a store that does not read whole
/// is unreadable, and an unreadable store trusts nothing.
#[derive(serde::Deserialize, Default)]
struct GrokTrustDocument {
    #[serde(default)]
    folders: BTreeMap<String, GrokFolderTrust>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct GrokFolderTrust {
    trusted: bool,
    #[serde(default)]
    decided_at: Option<i64>,
}

/// A boolean as Grok reads one from the environment (`xai-grok-env`).
fn grok_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disabled" => Some(false),
        _ => None,
    }
}

/// Whether Grok gates project hooks on folder trust, in Grok's order:
/// `GROK_FOLDER_TRUST`, then `[folder_trust] enabled` in the user config,
/// then in the managed config, else on (`feature_enabled` in
/// `folder_trust.rs`). A config Grok cannot parse is skipped, as Grok skips
/// it. `Err` for a layer with `[[version_overrides]]`, which Grok applies
/// for its own version before it reads the setting (loader.rs
/// `load_config_file`). Doctor cannot see Grok's remote setting, which can
/// only turn the gate off, or whether the binary is a local build, which
/// never gates.
fn grok_gate_enabled(env: Option<&str>, grok_home: &Path) -> Result<bool, String> {
    if let Some(value) = env.and_then(grok_bool) {
        return Ok(value);
    }
    for name in ["config.toml", "managed_config.toml"] {
        let Some(table) = std::fs::read_to_string(grok_home.join(name))
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            continue;
        };
        if table.contains_key("version_overrides") {
            return Err(format!(
                "{name} has version_overrides, which grok applies for its own version"
            ));
        }
        if let Some(enabled) = table
            .get("folder_trust")
            .and_then(|trust| trust.get("enabled"))
            .and_then(toml::Value::as_bool)
        {
            return Ok(enabled);
        }
    }
    Ok(true)
}

fn canonical_or_owned(path: &Path) -> PathBuf {
    crate::portable_path::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A key Grok never lets decide: relative, a filesystem root or the home
/// directory (`is_unsafe_trust_root`).
fn grok_unsafe_root(path: &Path, home: Option<&Path>) -> bool {
    !path.is_absolute()
        || path.parent().is_none()
        || home == Some(canonical_or_owned(path).as_path())
}

/// The key Grok trusts a folder by (`workspace_key` in trust.rs, f0e3be11):
/// a linked worktree's main checkout, else its git working tree, else the
/// folder, and the folder itself where that key would be a root Grok never
/// trusts. A worktree under Grok's own `worktrees/` is keyed by its
/// registry, which the caller reports as unverifiable first.
fn grok_workspace_key(path: &Path, home: Option<&Path>) -> PathBuf {
    let derived = git2::Repository::discover(path)
        .ok()
        .and_then(|repo| {
            if repo.is_worktree() {
                let main = git2::Repository::open(repo.commondir()).ok();
                if let Some(main_dir) = main.as_ref().and_then(git2::Repository::workdir) {
                    if canonical_or_owned(&main_dir.join(".git"))
                        == canonical_or_owned(repo.commondir())
                    {
                        return Some(canonical_or_owned(main_dir));
                    }
                }
            }
            repo.workdir().map(canonical_or_owned)
        })
        .unwrap_or_else(|| canonical_or_owned(path));
    if grok_unsafe_root(&derived, home) {
        canonical_or_owned(path)
    } else {
        derived
    }
}

/// Read Grok's folder trust for `root` as Grok decides it: its workspace
/// key, the recorded keys as written, the longest key that covers it within
/// the same workspace decides, and a tie trusts only when every tied record
/// does (`TrustStore::is_trusted`). Keys that are relative, a filesystem
/// root or the home directory never count. What doctor cannot reproduce is
/// `Unverifiable`: a relative `GROK_HOME`, which Grok refuses for its store
/// and reads its config against its own working directory
/// (`default_path_in`), config patches, and Grok's managed worktrees.
fn grok_folder_trust(root: &Path, grok_home: &Path, env: Option<&str>) -> GrokTrust {
    if !grok_home.is_absolute() {
        return GrokTrust::Unverifiable(
            "GROK_HOME is relative, which grok resolves against its own working directory".into(),
        );
    }
    match grok_gate_enabled(env, grok_home) {
        Ok(false) => return GrokTrust::Ungated,
        Err(why) => return GrokTrust::Unverifiable(why),
        Ok(true) => {}
    }
    let path = grok_home.join("trusted_folders.toml");
    let document = match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<GrokTrustDocument>(&text) {
            Ok(document) => document,
            Err(e) => return GrokTrust::Unreadable(format!("trusted_folders.toml: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => GrokTrustDocument::default(),
        Err(e) => return GrokTrust::Unreadable(format!("trusted_folders.toml: {e}")),
    };
    let folder = canonical_or_owned(root);
    if folder.starts_with(canonical_or_owned(&grok_home.join("worktrees"))) {
        return GrokTrust::Unverifiable(
            "a grok worktree, whose trust key grok reads from its worktrees.db".into(),
        );
    }
    let home = crate::portable_path::canonicalize(&user_home()).ok();
    let home = home.as_deref();
    let workspace = |path: &Path| {
        grok_workspace_key(path.ancestors().find(|p| p.exists()).unwrap_or(path), home)
    };
    let query = canonical_or_owned(&grok_workspace_key(&folder, home));
    let query_id = workspace(&query);
    let mut best: Option<usize> = None;
    let mut trusted = false;
    for (key, record) in &document.folders {
        let key = Path::new(key);
        if grok_unsafe_root(key, home) || !query.starts_with(key) || workspace(key) != query_id {
            continue;
        }
        let depth = key.components().count();
        match best {
            Some(d) if depth < d => {}
            Some(d) if depth == d => trusted &= record.trusted,
            _ => {
                best = Some(depth);
                trusted = record.trusted;
            }
        }
    }
    if trusted {
        GrokTrust::Trusted
    } else {
        GrokTrust::Untrusted
    }
}

fn check_config(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = Path::new(&opts.project_dir).join(".codeflow");

    if !config_dir.is_dir() {
        return CheckResult {
            name: "config".into(),
            status: Status::Warn(remedy::DOCTOR_INIT.remedy()),
            message: ".codeflow/ directory not found".into(),
            duration: start.elapsed(),
        };
    }

    let mut invalid_files = Vec::new();
    if let Err(e) = walk_json_files(&config_dir, &mut invalid_files) {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("error scanning .codeflow/: {e}"),
            duration: start.elapsed(),
        };
    }

    if !invalid_files.is_empty() {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("invalid JSON config files: {}", invalid_files.join(", ")),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "config".into(),
        status: Status::Pass,
        message: "all .codeflow/ JSON files are valid".into(),
        duration: start.elapsed(),
    }
}

/// Validate the schema 5 catalog, the personal overlay, promoted binding
/// records and the project selection, then report illustrative resolutions
/// and the live drift signals that are observable without launching a model.
///
/// The check never claims to observe a selected model or reasoning effort.
/// Those values remain native-interactive evaluation evidence.
fn check_model_bindings(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = if opts.project_dir.is_empty() {
        Path::new(".")
    } else {
        Path::new(&opts.project_dir)
    };
    let inputs = match crate::model_catalog::load_catalog(root).and_then(|catalog| {
        crate::model_catalog::CatalogInputs::load(
            catalog,
            root,
            opts.codeflow_home.as_deref(),
            opts.qualification_dir.as_deref(),
        )
    }) {
        Ok(inputs) => inputs,
        Err(error) => return model_binding_result(start, Status::Fail, error),
    };
    let (report, warned) = match inputs.diagnostic_report() {
        Ok(report) => report,
        Err(error) => return model_binding_result(start, Status::Fail, error),
    };
    let catalog = match model_qualification::harness_catalog() {
        Ok(catalog) => catalog,
        Err(error) => {
            return model_binding_result(
                start,
                Status::Fail,
                format!("embedded harness catalog invalid: {error}"),
            )
        }
    };
    let (status, bindings) = binding_record_drift(opts, &inputs, &catalog);
    let status = match (status, warned) {
        // Catalog warnings (a designated version with no full-suite record,
        // a canary identity drift) clear by qualifying with /cf-evaluate-model.
        (Status::Pass | Status::Note(_), true) => Status::Warn(remedy::DOCTOR_REQUALIFY.remedy()),
        (status, _) => status,
    };
    model_binding_result(start, status, format!("{report}\n{bindings}"))
}

/// Harness-version and settings drift for the loaded binding records. An
/// actively selected record that drifted fails; any other drift warns.
fn binding_record_drift(
    opts: &Options,
    inputs: &crate::model_catalog::CatalogInputs,
    catalog: &BTreeMap<String, model_qualification::HarnessMetadata>,
) -> (Status, String) {
    let records = &inputs.catalog.bindings;
    if records.is_empty() {
        return (
            Status::Pass,
            "no promoted model bindings and no project selection; the managed catalog remains effective"
                .into(),
        );
    }
    let selected: Vec<&crate::model_catalog::BindingReference> = inputs
        .catalog
        .selection
        .iter()
        .flat_map(|selection| &selection.bindings)
        .collect();
    let (drift, unobservable) = observe_binding_drift(opts, records, catalog);
    let active_drift: Vec<&str> = drift
        .iter()
        .filter(|(id, _)| selected.iter().any(|entry| &entry.binding_id == id))
        .map(|(_, message)| message.as_str())
        .collect();
    if !active_drift.is_empty() {
        return (
            Status::Fail,
            format!(
                "active project model selection requires requalification: {}. No override is applied",
                active_drift.join("; ")
            ),
        );
    }
    if !drift.is_empty() {
        return (
            Status::Warn(remedy::DOCTOR_REQUALIFY.remedy()),
            format!(
                "binding requalification required: {}. Requested model/effort remain native-session observations, never inferred by doctor",
                drift
                    .iter()
                    .map(|(_, message)| message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        );
    }
    if !unobservable.is_empty() {
        return (
            Status::Note(remedy::DOCTOR_CANARY.remedy()),
            format!(
                "{} approved binding(s) are structurally valid; {}",
                records.len(),
                unobservable.join("; ")
            ),
        );
    }
    let selection = if selected.is_empty() {
        "no project selection is active".to_owned()
    } else {
        format!(
            "project selection: {}",
            selected
                .iter()
                .map(|entry| format!("{}={}", entry.role, entry.binding_id))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    (
        Status::Pass,
        format!(
            "{} approved binding(s) passed structural, harness-version, and declared-settings drift checks; {selection}; live model/effort still require native observation",
            records.len()
        ),
    )
}

fn model_binding_result(start: Instant, status: Status, message: impl Into<String>) -> CheckResult {
    CheckResult {
        name: "model-bindings".into(),
        status,
        message: message.into(),
        duration: start.elapsed(),
    }
}

fn observe_binding_drift(
    opts: &Options,
    records: &[model_qualification::QualifiedBinding],
    catalog: &BTreeMap<String, model_qualification::HarnessMetadata>,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut drift = Vec::new();
    let mut unobservable = Vec::new();
    for record in records {
        let harness = &catalog[&record.requested.harness];
        if let Some(probe_id) = &harness.version_probe {
            let probe = model_qualification::trusted_version_probe(probe_id)
                .expect("validated embedded harness probe");
            match opts.do_look_path(probe.command) {
                Ok(command) => {
                    match opts.do_exec_bounded(&command, probe.args, VERSION_PROBE_TIMEOUT) {
                        Ok(observed)
                            if probe_version(probe_id, observed.trim())
                                == Some(record.requested.harness_version.as_str()) => {}
                        Ok(observed) => drift.push((
                            record.binding_id.clone(),
                            format!(
                                "{} harness version changed (qualified {:?}, observed {:?})",
                                record.binding_id,
                                record.requested.harness_version,
                                observed.trim()
                            ),
                        )),
                        Err(error) => drift.push((
                            record.binding_id.clone(),
                            format!(
                                "{} harness version probe failed: {error}",
                                record.binding_id
                            ),
                        )),
                    }
                }
                Err(_) => drift.push((
                    record.binding_id.clone(),
                    format!(
                        "{} harness command {} is unavailable",
                        record.binding_id, probe.command
                    ),
                )),
            }
        } else {
            unobservable.push(format!(
                "{} {} version has no external probe",
                record.binding_id, record.requested.harness
            ));
        }
        for source in &record.settings_sources {
            match read_bounded(&source.path, MAX_SETTINGS_BYTES) {
                Ok(bytes) => {
                    let observed = format!("sha256:{}", sha256_hex(&bytes));
                    if observed != source.digest {
                        drift.push((
                            record.binding_id.clone(),
                            format!(
                                "{} settings changed at {}",
                                record.binding_id,
                                source.path.display()
                            ),
                        ));
                    }
                }
                Err(error) => drift.push((
                    record.binding_id.clone(),
                    format!(
                        "{} settings source {} cannot be checked: {error}",
                        record.binding_id,
                        source.path.display()
                    ),
                )),
            }
        }
    }
    (drift, unobservable)
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("open: {error}"))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read: {error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(format!("exceeds {max_bytes} byte limit"));
    }
    Ok(bytes)
}

fn probe_version<'a>(probe_id: &str, observed: &'a str) -> Option<&'a str> {
    let mut tokens = observed.split_ascii_whitespace();
    match probe_id {
        "claude-cli-version" => tokens.next(),
        "codex-cli-version" if tokens.next() == Some("codex-cli") => tokens.next(),
        "grok-cli-version" if tokens.next() == Some("grok") => tokens.next(),
        _ => None,
    }
}

struct ProbeCapture {
    bytes: Vec<u8>,
}

fn run_bounded_command(cmd: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = crate::git::process(cmd);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().map(spawn_probe_reader);
    let stderr = child.stderr.take().map(spawn_probe_reader);
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= timeout {
            terminate_process_tree(&mut child);
            let _ = child.wait();
            return Err(probe_timeout_message(timeout));
        }
        std::thread::sleep(VERSION_PROBE_POLL);
    };
    let stdout = receive_probe_reader(stdout, started, timeout).map_err(|error| {
        terminate_process_tree(&mut child);
        if error == "timed out" {
            probe_timeout_message(timeout)
        } else {
            error
        }
    })?;
    let stderr = receive_probe_reader(stderr, started, timeout).map_err(|error| {
        terminate_process_tree(&mut child);
        if error == "timed out" {
            probe_timeout_message(timeout)
        } else {
            error
        }
    })?;
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout.bytes).to_string())
    } else {
        Err(String::from_utf8_lossy(&stderr.bytes).to_string())
    }
}

fn probe_timeout_message(timeout: Duration) -> String {
    format!(
        "timed out after {} ms",
        u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
    )
}

fn terminate_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(process_group) = i32::try_from(child.id()) {
            // SAFETY: the child was placed in a new process group whose id is
            // its pid. SIGKILL is best-effort; child.kill remains a backstop.
            unsafe {
                libc::killpg(process_group, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        // Spawn without waiting so an unavailable or wedged helper cannot
        // extend the probe deadline. The direct-child kill below is immediate.
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    let _ = child.kill();
}

fn spawn_probe_reader<R: Read + Send + 'static>(
    mut pipe: R,
) -> std::sync::mpsc::Receiver<ProbeCapture> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut retained = Vec::with_capacity(VERSION_PROBE_OUTPUT_BYTES);
        let mut chunk = [0_u8; 8192];
        loop {
            let Ok(read) = pipe.read(&mut chunk) else {
                break;
            };
            if read == 0 {
                break;
            }
            retained.extend_from_slice(&chunk[..read]);
            if retained.len() > VERSION_PROBE_OUTPUT_BYTES {
                let excess = retained.len() - VERSION_PROBE_OUTPUT_BYTES;
                retained.drain(..excess);
            }
        }
        let _ = sender.send(ProbeCapture { bytes: retained });
    });
    receiver
}

fn receive_probe_reader(
    receiver: Option<std::sync::mpsc::Receiver<ProbeCapture>>,
    started: Instant,
    timeout: Duration,
) -> Result<ProbeCapture, String> {
    let receiver = receiver.ok_or_else(|| "version probe output was not piped".to_string())?;
    let remaining = timeout.saturating_sub(started.elapsed());
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            std::sync::mpsc::RecvTimeoutError::Timeout => "timed out".to_string(),
            std::sync::mpsc::RecvTimeoutError::Disconnected => {
                "version probe output capture failed".to_string()
            }
        })
}

/// Walk a directory tree and validate JSON files, recording paths relative
/// to `dir` for any that fail to parse.
fn walk_json_files(dir: &Path, invalid: &mut Vec<String>) -> Result<(), std::io::Error> {
    walk_json_files_inner(dir, dir, invalid)
}

fn walk_json_files_inner(
    root: &Path,
    dir: &Path,
    invalid: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_json_files_inner(root, &path, invalid)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let rel = path.strip_prefix(root).map_or_else(
                |_| path.to_string_lossy().to_string(),
                |p| p.to_string_lossy().to_string(),
            );
            match std::fs::read(&path) {
                Ok(data) => {
                    if serde_json::from_slice::<serde_json::Value>(&data).is_err() {
                        invalid.push(rel);
                    }
                }
                Err(e) => {
                    invalid.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    Ok(())
}

fn check_permissions(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = PathBuf::from(&opts.project_dir).join(".codeflow");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&config_dir) {
            if meta.permissions().mode() & 0o200 == 0 {
                return CheckResult {
                    name: "permissions".into(),
                    status: Status::Fail,
                    message: ".codeflow/ is not writable".into(),
                    duration: start.elapsed(),
                };
            }
        }
    }

    let _ = config_dir; // suppress unused warning on non-unix

    // A full gate also writes its locks and its evidence outside the
    // worktree (TSK-216); in a sandbox those are the writes that fail.
    let sandbox = if sandboxed(opts) {
        format!("; {SANDBOX_NOTE}")
    } else {
        String::new()
    };
    let unwritable = if opts.project_dir.is_empty() {
        Vec::new()
    } else {
        crate::testing::gate_guard::unwritable_gate_dirs(
            Path::new(&opts.project_dir),
            opts.codeflow_home.as_deref(),
        )
    };
    if !unwritable.is_empty() {
        let paths: Vec<String> = unwritable
            .iter()
            .map(|(dir, _)| format!("`{}`", dir.display()))
            .collect();
        let errors: Vec<String> = unwritable
            .iter()
            .map(|(dir, error)| format!("{}: {error}", dir.display()))
            .collect();
        return CheckResult {
            name: "permissions".into(),
            status: Status::Warn(remedy::DOCTOR_GATE_DIRS.with(&[("paths", &paths.join(", "))])),
            message: format!(
                "a full gate cannot write its lock or evidence directories, so `codeflow test --mode full` refuses or fails here ({}){sandbox}",
                errors.join("; ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "permissions".into(),
        status: Status::Pass,
        message: format!(
            "file permissions correct; the full gate's lock and evidence directories are writable{sandbox}"
        ),
        duration: start.elapsed(),
    }
}

/// Claude Code sets `SANDBOX_RUNTIME` in the commands its sandbox runs.
fn sandboxed(opts: &Options) -> bool {
    opts.env("SANDBOX_RUNTIME")
        .is_some_and(|value| !value.is_empty() && value != "0")
}

/// Reported context only: the marker says a sandbox launched the command,
/// not which restrictions apply or why a probe failed.
const SANDBOX_NOTE: &str = "SANDBOX_RUNTIME is set, so this likely runs in the Claude Code sandbox";

/// The first non-empty line of a probe's output, for quoting.
fn first_line(output: &str) -> &str {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
}

/// The HTTP status code `curl -w '%{http_code}'` printed, when it printed
/// one (`000` means no response).
fn http_status(output: &str) -> Option<u16> {
    output
        .trim()
        .trim_matches('\'')
        .parse()
        .ok()
        .filter(|code| *code != 0)
}

fn check_network(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let warn = |message: String, remedy: remedy::Remedy| CheckResult {
        name: "network".into(),
        status: Status::Warn(remedy),
        message,
        duration: start.elapsed(),
    };

    // The Claude Code sandbox carries traffic through its proxy and gives
    // no direct DNS, so there the probe is an HTTPS request, which the
    // proxy carries as it carries every other tool's (TSK-216).
    if sandboxed(opts) {
        if opts.do_look_path("curl").is_err() {
            return warn(
                format!("`curl` not found; skipping connectivity probe; {SANDBOX_NOTE}"),
                remedy::DOCTOR_TOOL_MISSING
                    .with(&[("tool", "the `curl` tool"), ("check", "network")]),
            );
        }
        // The status line decides: a proxy refusal or an upstream error
        // still exits 0 without `--fail`.
        return match opts.do_exec(
            "curl",
            &[
                "-sS",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "--max-time",
                "5",
                "https://github.com",
            ],
        ) {
            Ok(out) => match http_status(&out) {
                Some(code @ 200..=399) => CheckResult {
                    name: "network".into(),
                    status: Status::Pass,
                    message: format!(
                        "network connectivity OK over HTTPS (github.com answered HTTP {code}); {SANDBOX_NOTE}"
                    ),
                    duration: start.elapsed(),
                },
                Some(code) => warn(
                    format!(
                        "HTTPS request to github.com answered HTTP {code}, so connectivity is not confirmed; {SANDBOX_NOTE}"
                    ),
                    remedy::DOCTOR_NETWORK.remedy(),
                ),
                None => warn(
                    format!(
                        "HTTPS request to github.com returned no HTTP status ({}); {SANDBOX_NOTE}",
                        first_line(&out)
                    ),
                    remedy::DOCTOR_NETWORK.remedy(),
                ),
            },
            Err(error) => warn(
                format!(
                    "HTTPS request to github.com failed ({}); {SANDBOX_NOTE}",
                    first_line(&error)
                ),
                remedy::DOCTOR_NETWORK.remedy(),
            ),
        };
    }

    // The probe is `host`. If it isn't installed we cannot infer offline from its
    // absence — say so and skip, rather than implying the network is down.
    if opts.do_look_path("host").is_err() {
        return warn(
            "`host` not found; skipping connectivity probe".into(),
            remedy::DOCTOR_TOOL_MISSING.with(&[("tool", "the `host` tool"), ("check", "network")]),
        );
    }

    match opts.do_exec("host", &["-W", "2", "github.com"]) {
        Ok(_) => CheckResult {
            name: "network".into(),
            status: Status::Pass,
            message: "network connectivity OK".into(),
            duration: start.elapsed(),
        },
        Err(_) => warn(
            "network connectivity check failed (offline?)".into(),
            remedy::DOCTOR_NETWORK.remedy(),
        ),
    }
}

/// Bidirectional cross-vendor delegation readiness (ADR-0023). Optional by
/// design, so a missing prerequisite warns — never fails. The Claude-host lane
/// uses the official codex plugin; the Codex-host lane uses an interactive
/// Claude CLI in tmux. This check verifies inspectable prerequisites only:
/// live account/tool access still requires an interactive canary in each lane.
/// `agy` is informational because it has no sanctioned interactive lane.
fn check_delegates(opts: &Options) -> CheckResult {
    let start = Instant::now();

    let agy_note = if opts.do_look_path("agy").is_ok() {
        "; agy present (not a delegate tier — no sanctioned interactive lane)"
    } else {
        ""
    };

    // Gaps this machine can close on its own, and a sign-in, which only
    // the operator's own account can close.
    let mut gaps = Vec::new();
    let mut signed_out = false;
    // The shipped presets deny reading the Codex auth file in the sandbox,
    // so there a failed status that does not say "not logged in" leaves the
    // sign-in unconfirmed (TSK-216); it is quoted, never explained.
    let mut auth_unseen: Option<String> = None;

    match opts.do_look_path("codex") {
        Ok(codex_bin) => {
            if let Err(error) = opts.do_exec(&codex_bin, &["login", "status"]) {
                if sandboxed(opts) && !error.to_ascii_lowercase().contains("not logged in") {
                    auth_unseen = Some(first_line(&error).to_string());
                } else {
                    signed_out = true;
                }
            }
            if opts.do_exec(&codex_bin, &["mcp", "list"]).is_err() {
                gaps.push("Codex MCP inventory unavailable (run `codex mcp list`)".to_string());
            }
        }
        Err(_) => gaps.push("codex missing from PATH".to_string()),
    }

    match opts.do_look_path("claude") {
        Ok(claude_bin) => {
            if opts.do_exec(&claude_bin, &["mcp", "list"]).is_err() {
                gaps.push("Claude MCP inventory unavailable (run `claude mcp list`)".to_string());
            }

            match opts.do_exec(&claude_bin, &["plugin", "list", "--json"]) {
                Ok(json) if codex_plugin_enabled(&json) => {}
                Ok(_) => gaps.push(
                    "codex@openai-codex plugin not enabled (install it in Claude Code and run /codex:setup)"
                        .to_string(),
                ),
                Err(_) => gaps.push(
                    "Claude plugin inventory unavailable (run `claude plugin list --json`)"
                        .to_string(),
                ),
            }
        }
        Err(_) => gaps.push("claude missing from PATH".to_string()),
    }

    if opts.do_look_path("tmux").is_err() {
        gaps.push("tmux missing from PATH".to_string());
    }

    if let (Some(error), true) = (&auth_unseen, gaps.is_empty()) {
        return CheckResult {
            name: "delegates".into(),
            status: Status::Note(remedy::DOCTOR_SANDBOX_UNSEEN.with(&[
                ("check", "delegates"),
                ("what", "the Codex sign-in"),
            ])),
            message: format!(
                "Claude↔Codex prerequisites present; the Codex sign-in is unconfirmed: `codex login status` failed ({error}), and the settings preset denies sandboxed commands the Codex auth file; {SANDBOX_NOTE}{agy_note}"
            ),
            duration: start.elapsed(),
        };
    }
    if let Some(error) = &auth_unseen {
        gaps.push(format!(
            "Codex sign-in unconfirmed: `codex login status` failed ({error}); run `codeflow doctor --check delegates` outside the sandbox"
        ));
    }

    if gaps.is_empty() && !signed_out {
        CheckResult {
            name: "delegates".into(),
            status: Status::Pass,
            message: format!(
                "Claude↔Codex bidirectional prerequisites present (Codex auth/MCP + Claude plugin/MCP + tmux); retain live interactive canaries. Grok-hosted Claude/Codex lanes use Herdr and are not claimed complete by this check{agy_note}"
            ),
            duration: start.elapsed(),
        }
    } else {
        let remedy = if gaps.is_empty() {
            remedy::DOCTOR_DELEGATES_SIGN_IN.remedy()
        } else {
            remedy::DOCTOR_DELEGATES.remedy()
        };
        if signed_out {
            gaps.push("Codex auth unavailable (run `codex login`)".to_string());
        }
        CheckResult {
            name: "delegates".into(),
            status: Status::Warn(remedy),
            message: format!(
                "cross-vendor delegation is partially unavailable (optional): {}. Verify Claude auth with an interactive TTY canary; status output alone is not authoritative{agy_note}",
                gaps.join("; ")
            ),
            duration: start.elapsed(),
        }
    }
}

fn codex_plugin_enabled(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|value| {
            value.as_array().map(|plugins| {
                plugins.iter().any(|plugin| {
                    plugin.get("id").and_then(serde_json::Value::as_str)
                        == Some("codex@openai-codex")
                        && plugin
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false)
                })
            })
        })
        .unwrap_or(false)
}

/// Synthetic schema-v2 lifecycle through the installed binary. Unlike the
/// optional harness prerequisite check, this is a deterministic product
/// capability and therefore fails doctor when any transition is broken.
fn check_delegate_roundtrip(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let fail = |message: String| CheckResult {
        name: "delegate-roundtrip".into(),
        status: Status::Fail,
        message,
        duration: start.elapsed(),
    };
    let Ok(codeflow_bin) = opts.do_look_path("codeflow") else {
        return fail("codeflow binary not found in PATH".to_string());
    };
    let root = std::env::temp_dir().join(format!("codeflow-doctor-{}", ulid::Ulid::new()));
    if let Err(error) = create_private_roundtrip_root(&root) {
        return fail(format!("cannot create round-trip workspace: {error}"));
    }
    let state = root.join("state");
    let prompt = root.join("prompt.txt");
    if let Err(error) = std::fs::write(&prompt, b"doctor delegate round trip") {
        let _ = std::fs::remove_dir_all(&root);
        return fail(format!("cannot create round-trip prompt: {error}"));
    }
    let context = DelegateDoctorContext {
        opts,
        binary: &codeflow_bin,
        state: state.to_string_lossy().into_owned(),
        prompt: prompt.to_string_lossy().into_owned(),
    };
    let result = context.run();
    let _ = std::fs::remove_dir_all(&root);

    match result {
        Ok(()) => CheckResult {
            name: "delegate-roundtrip".into(),
            status: Status::Pass,
            message: "schema-v2 init → ready → arm → accepted → terminal round trip passed".into(),
            duration: start.elapsed(),
        },
        Err(error) => fail(error),
    }
}

fn create_private_roundtrip_root(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    let mut root_builder = std::fs::DirBuilder::new();
    #[cfg(not(unix))]
    let root_builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        root_builder.mode(0o700);
    }
    root_builder.create(root)
}

struct DelegateDoctorContext<'a> {
    opts: &'a Options,
    binary: &'a str,
    state: String,
    prompt: String,
}

impl DelegateDoctorContext<'_> {
    const PROMPT_ID: &'static str = "123e4567-e89b-12d3-a456-426614174000";

    fn run(&self) -> Result<(), String> {
        self.ready()?;
        self.turn()
    }

    fn ready(&self) -> Result<(), String> {
        self.step(
            &[
                "delegate",
                "init",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
            ],
            "",
            "init",
        )?;
        self.step(
            &[
                "hook",
                "delegate-turn",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
            ],
            r#"{"hook_event_name":"SessionStart","source":"startup","session_id":"doctor-session","cwd":"/tmp"}"#,
            "ready hook",
        )?;
        self.step(
            &[
                "delegate",
                "wait",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
                "--until",
                "ready",
                "--timeout-seconds",
                "1",
            ],
            "",
            "ready wait",
        )
    }

    fn turn(&self) -> Result<(), String> {
        self.step(
            &[
                "delegate",
                "arm",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
                "--turn-id",
                "turn-1",
                "--prompt-file",
                &self.prompt,
            ],
            "",
            "arm",
        )?;
        let accepted = format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"doctor-session","prompt_id":"{}","prompt":"doctor delegate round trip"}}"#,
            Self::PROMPT_ID
        );
        self.hook_and_wait(&accepted, "accepted", "accepted hook")?;
        let terminal = format!(
            r#"{{"hook_event_name":"Stop","session_id":"doctor-session","prompt_id":"{}","last_assistant_message":"round trip complete"}}"#,
            Self::PROMPT_ID
        );
        self.hook_and_wait(&terminal, "terminal", "terminal hook")
    }

    fn hook_and_wait(&self, payload: &str, until: &str, stage: &str) -> Result<(), String> {
        self.step(
            &[
                "hook",
                "delegate-turn",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
            ],
            payload,
            stage,
        )?;
        self.step(
            &[
                "delegate",
                "wait",
                "--run-id",
                "doctor",
                "--state-dir",
                &self.state,
                "--turn-id",
                "turn-1",
                "--until",
                until,
                "--timeout-seconds",
                "1",
            ],
            "",
            &format!("{until} wait"),
        )
    }

    fn step(&self, args: &[&str], stdin: &str, stage: &str) -> Result<(), String> {
        exec_doctor_step(self.opts, self.binary, args, stdin, stage)
    }
}

fn exec_doctor_step(
    opts: &Options,
    binary: &str,
    args: &[&str],
    stdin: &str,
    stage: &str,
) -> Result<(), String> {
    opts.do_exec_stdin(binary, args, stdin)
        .map(|_| ())
        .map_err(|error| format!("schema-v2 round trip failed at {stage}: {}", error.trim()))
}

/// Repository structural integrity (ADR-0007). Two failure signatures, both
/// reproduced from a `gh pr merge --delete-branch` / worktree mishap:
///
/// 1. `core.bare = true` on a repo that still has a working tree — the root
///    checkout got flipped to bare; remedy `git config core.bare false`.
/// 2. A protected branch checked out in a non-root (linked) worktree —
///    remedy: switch that worktree to its feature branch.
///
/// Passes otherwise, and stays quiet where it cannot determine repo state
/// (not a git repo, git unavailable): this check flags, it never guesses.
fn check_repo_integrity(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let pass = |msg: &str| CheckResult {
        name: "repo-integrity".into(),
        status: Status::Pass,
        message: msg.into(),
        duration: start.elapsed(),
    };
    let fail = |msg: String| CheckResult {
        name: "repo-integrity".into(),
        status: Status::Fail,
        message: msg,
        duration: start.elapsed(),
    };

    // Signal 1: core.bare on a repo that still holds a working tree. A working
    // checkout keeps its git dir under `.git`; a genuinely bare repo has none.
    let is_bare = opts
        .do_exec(
            "git",
            &[
                "-C",
                opts.project_dir.as_str(),
                "rev-parse",
                "--is-bare-repository",
            ],
        )
        .map(|s| s.trim() == "true")
        .unwrap_or(false);
    if is_bare && root.join(".git").exists() {
        return fail(
            "core.bare=true on a repo with a working tree — a merge/worktree mishap flipped it; run `git config core.bare false`".into(),
        );
    }

    // Signal 2: a protected branch checked out in a non-root worktree.
    let policy = crate::hooks::policy::Policy::load(&root).git;
    if let Ok(list) = opts.do_exec(
        "git",
        &[
            "-C",
            opts.project_dir.as_str(),
            "worktree",
            "list",
            "--porcelain",
        ],
    ) {
        let worktrees = parse_worktree_list(&list);
        // The main (root) worktree is listed first and may hold a protected
        // branch; only linked worktrees (the rest) must not.
        for wt in worktrees.iter().skip(1) {
            if let Some(branch) = &wt.branch {
                if policy.branch_is_protected(branch) {
                    return fail(format!(
                        "protected branch '{branch}' is checked out in a non-root worktree ({}) — switch that worktree to its feature branch",
                        wt.path
                    ));
                }
            }
        }
    }

    // The root checkout: its root branch, and in an umbrella its nested
    // repositories and linked worktrees (TSK-165).
    let here = if opts.project_dir.is_empty() {
        Path::new(".")
    } else {
        root.as_path()
    };
    let (lines, warns) = crate::root_checkout::doctor_lines(here, &policy, &|name| opts.env(name));
    let mut message = if warns {
        "repo layout: not bare, no protected branch in a linked worktree; the root checkout \
         needs attention"
            .to_string()
    } else {
        "repo layout healthy: not bare, no protected branch in a linked worktree".to_string()
    };
    for line in &lines {
        message.push_str("\n      ");
        message.push_str(line);
    }
    if warns {
        CheckResult {
            name: "repo-integrity".into(),
            status: Status::Warn(crate::remedy::DOCTOR_ROOT_CHECKOUT.remedy()),
            message,
            duration: start.elapsed(),
        }
    } else {
        pass(&message)
    }
}

/// One entry parsed from `git worktree list --porcelain`.
struct WorktreeEntry {
    path: String,
    /// Checked-out branch (short name), `None` when detached.
    branch: Option<String>,
}

fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            cur = Some(WorktreeEntry {
                path: path.to_string(),
                branch: None,
            });
        } else if let Some(refname) = line.strip_prefix("branch ") {
            if let Some(w) = cur.as_mut() {
                w.branch = Some(
                    refname
                        .strip_prefix("refs/heads/")
                        .unwrap_or(refname)
                        .to_string(),
                );
            }
        }
    }
    if let Some(w) = cur.take() {
        out.push(w);
    }
    out
}

/// Adopter fit (SPC-013 R-84, R-97, R-115): the effective PR-section level
/// and its origin, a kept PR template's pending decision, and the release
/// backend with any release tool that owns versions. WARN while a decision
/// is pending, while policy provenance is unreadable, or while the release
/// backend and a detected tool disagree; FAIL on an invalid backend.
fn check_adopter_fit(opts: &Options) -> CheckResult {
    use crate::hooks::adoption;
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let raw = adoption::raw_policy(&root);
    let policy = crate::hooks::Policy::load(&root);
    let level = adoption::pr_sections_effective(&raw, &policy.git);
    let mut parts = vec![format!(
        "git.pr_sections effective level {} ({})",
        level.level, level.origin
    )];
    let mut status = Status::Pass;
    if let Err(error) = &raw {
        status = Status::Warn(
            remedy::DOCTOR_POLICY_DECISION
                .with(&[("decision", "make it valid JSON so its provenance reads")]),
        );
        parts.push(format!(
            "policy provenance unreadable ({error}); the configured level stands and any kept PR template mapping is unresolved"
        ));
    }
    if let Some(pending) = adoption::pending_decision(&policy.git) {
        if !status.is_warn() {
            status = Status::Warn(remedy::DOCTOR_POLICY_DECISION.with(&[(
                "decision",
                "`git.pr_section_mapping.decided`: accepted, refused or custom",
            )]));
        }
        parts.push(pending);
    }
    match adoption::release_backend(&root) {
        Ok(backend) => {
            parts.push(format!("release.backend {backend}"));
            if let Some(finding) =
                adoption::release_backend_finding(backend, &adoption::detect_release_tools(&root))
            {
                if !status.is_warn() {
                    status = Status::Warn(remedy::DOCTOR_RELEASE_BACKEND.remedy());
                }
                parts.push(finding);
            }
        }
        Err(error) => {
            status = Status::Fail;
            parts.push(error);
        }
    }
    CheckResult {
        name: "adopter-fit".into(),
        status,
        message: parts.join("; "),
        duration: start.elapsed(),
    }
}

/// Perimeter honesty (charter section 9): the scaffolded CI workflow is the
/// authoritative, server-enforced perimeter, and branch protection can
/// require it. It installs the `codeflow` release the target branch pins,
/// checksum-verified (SPC-013 R-113, TSK-095), so this describes that pin:
/// the version CI installs, a lowered pin, and an upgrade that carries
/// `codeflow update` before its raised pin has landed (WARN, with the
/// two-step order). An older scaffold's placeholder install step WARNS as an
/// unarmed perimeter, and a CI file without a shipped pinned install left
/// unchanged passes with no claim about the pin (TSK-182). No CI file at all
/// passes cleanly: the repo opted out or predates the workflow. WARN only,
/// never a block.
fn check_ci_perimeter(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let github = ci_workflow_dest(&root);
    // The GitHub workflow `init` scaffolds, then the copy-in platform files
    // at the paths their platforms read.
    let found = [github.as_str(), ".gitlab-ci.yml", "bitbucket-pipelines.yml"]
        .into_iter()
        .find_map(|dest| {
            std::fs::read_to_string(root.join(dest))
                .ok()
                .map(|content| (dest.to_string(), content))
        });
    let Some((dest, content)) = found else {
        return CheckResult {
            name: "ci-perimeter".into(),
            status: Status::Pass,
            message: "no codeflow CI workflow found (nothing to arm)".into(),
            duration: start.elapsed(),
        };
    };

    if CI_PLACEHOLDER_MARKS
        .iter()
        .any(|mark| content.contains(mark))
    {
        return CheckResult {
            name: "ci-perimeter".into(),
            status: Status::Warn(remedy::DOCTOR_CI_PLACEHOLDER.with(&[("path", &dest)])),
            message: format!(
                "{dest} install step is still the PLACEHOLDER: the CI perimeter is not armed"
            ),
            duration: start.elapsed(),
        };
    }

    // The pin describes what CI runs only for a shipped pinned install left
    // unchanged; for any other install (a source build, its own installer,
    // an edited template) doctor makes no claim (TSK-182).
    if !ci_pin::recognized(&content) {
        return CheckResult {
            name: "ci-perimeter".into(),
            status: Status::Pass,
            message: format!(
                "{dest} does not carry the shipped target-pinned install unchanged, so doctor cannot verify how it installs codeflow, which version that is, or whether its checksum is checked"
            ),
            duration: start.elapsed(),
        };
    }

    let pin = ci_pin::report(&root);
    CheckResult {
        name: "ci-perimeter".into(),
        status: pin.status,
        message: format!("{dest}: {}", pin.message),
        duration: start.elapsed(),
    }
}

/// The shared id registry (SPC-013 R-10, R-21): damage, ids no ref can
/// place, and the assurance of the host. Reads fetched refs only; it never
/// fetches.
fn check_id_registry(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let result = |status: Status, message: String| CheckResult {
        name: "id-registry".into(),
        status,
        message,
        duration: start.elapsed(),
    };
    match crate::workgraph::durable_work_tracking_enabled(&root) {
        Ok(true) => {}
        Ok(false) => {
            return result(
                Status::Pass,
                "durable work tracking is off; no registry applies".into(),
            )
        }
        // Whether a registry applies is unknown, so it is not applicable
        // here; the enforcing surfaces (pre-commit, CI) refuse until the
        // state is repaired, and the warning keeps the cause visible.
        Err(error) => {
            return result(
                Status::Warn(remedy::DOCTOR_TRACKING_UNKNOWN.remedy()),
                format!("not applicable: durable-work tracking cannot be determined ({error})"),
            )
        }
    }
    let git = crate::ids::Git::new(&root);
    let report = match crate::ids::check::check(&git, None) {
        Ok(report) => report,
        Err(error) => return result(Status::Fail, format!("cannot read the registry: {error}")),
    };
    if !report.blocks.is_empty() {
        return result(Status::Fail, report.blocks.join("; "));
    }
    let mut notes: Vec<String> = report.info.iter().take(1).cloned().collect();
    notes.extend(report.warns.iter().cloned());
    let mut status = if report.warns.is_empty() {
        Status::Pass
    } else {
        Status::Warn(remedy::DOCTOR_ID_REGISTRY.remedy())
    };
    if git.has_remote(crate::ids::AUTHORITY) {
        let state = crate::ids::state::load(&git).unwrap_or_default();
        let last = state
            .last_verified
            .get(crate::ids::AUTHORITY)
            .map_or("none yet".to_string(), |sha| {
                crate::ids::ledger::short(sha).to_string()
            });
        if state.data_profile.contains_key(crate::ids::AUTHORITY) {
            notes.push(format!(
                "host data profile applied; last verified tip {last}"
            ));
        } else {
            if !status.is_warn() {
                status = Status::Warn(remedy::DOCTOR_REGISTRY_UNPROTECTED.remedy());
            }
            notes.push(format!(
                "reduced assurance: no host rules recorded for codeflow/registry (`codeflow remote protect` applies them); a rewrite is detected only against the last verified tip ({last})"
            ));
        }
    }
    result(status, notes.join("; "))
}

/// Where init placed the CI workflow: the installed-file record for the shipped
/// CI asset (robust to a relocated workflow), else the canonical dest.
fn ci_workflow_dest(root: &Path) -> String {
    InstalledManifest::load_or_default(root, "0")
        .ok()
        .and_then(|m| {
            m.files
                .iter()
                .find(|(_, f)| f.src == CI_ASSET_SRC)
                .map(|(dest, _)| dest.clone())
        })
        .unwrap_or_else(|| CI_DEFAULT_DEST.to_string())
}

/// Managed-region drift (charter §4.3.2): WARN when a marker-delimited managed
/// region has been hand-edited inside its `codeflow:managed` markers.
/// `codeflow update` regenerates that block from the shipped asset, so an
/// in-marker edit is silently lost on the next update — surfacing it here is
/// the honest signal. Reuses the installed-file record: the manifest stores a
/// sha256 of each managed region (the pristine shipped block); this recomputes
/// the current block's hash and flags any that no longer match. Content OUTSIDE
/// the markers is project-owned and never compared, and JSON settings merges
/// (no text markers; the record holds the shipped preset's hash, not the
/// on-disk block) are skipped. WARN only; stays quiet where it cannot read the
/// record or a file — it flags, it never guesses.
fn check_managed_drift(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let pass = |msg: String| CheckResult {
        name: "managed-drift".into(),
        status: Status::Pass,
        message: msg,
        duration: start.elapsed(),
    };

    let Ok(installed) = InstalledManifest::load_or_default(&root, "0") else {
        return pass("no readable installed manifest; drift check skipped".into());
    };

    let mut drifted: Vec<String> = Vec::new();
    for (dest, file) in &installed.files {
        if file.ownership != Ownership::ManagedRegion {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(root.join(dest)) else {
            continue; // file absent: presence is another check's concern
        };
        // Marker-delimited regions only (markdown/hash). A JSON settings merge
        // has no text markers, so extraction returns None and it is skipped —
        // its recorded hash is the shipped preset, not the on-disk block.
        let Some(block) = region::extract_block(&content, RegionFormat::Markdown)
            .or_else(|| region::extract_block(&content, RegionFormat::Hash))
        else {
            continue;
        };
        if sha256_hex(block.as_bytes()) != file.sha256 {
            drifted.push(dest.clone());
        }
    }

    if drifted.is_empty() {
        return pass("no managed-region drift (codeflow blocks match the record)".into());
    }
    CheckResult {
        name: "managed-drift".into(),
        status: Status::Warn(remedy::DOCTOR_MANAGED_DRIFT.remedy()),
        message: format!(
            "{} managed region(s) hand-edited inside codeflow markers; `codeflow update` will regenerate and lose these edits: {}",
            drifted.len(),
            drifted.join(", ")
        ),
        duration: start.elapsed(),
    }
}

/// Consuming-project onboarding health. Standard/full scaffolds intentionally
/// ship explicit placeholders rather than inventing product or architecture
/// facts. Once those documents exist, keep the reminder visible until the
/// project has reconciled them through `/cf-customize`. Minimal installs do not
/// ship the method documents, so absence of both is a clean not-applicable pass.
fn check_customization(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let product = root.join("docs/product.md");
    let architecture = root.join("docs/architecture.md");

    if !product.exists() && !architecture.exists() {
        return CheckResult {
            name: "customization".into(),
            status: Status::Pass,
            message: "method product/architecture docs not installed (customization check not applicable)"
                .into(),
            duration: start.elapsed(),
        };
    }

    let mut incomplete = Vec::new();
    for (path, sentinels) in [
        (
            "docs/product.md",
            &[
                "{{PRODUCT_PURPOSE}}",
                "{{PRODUCT_USERS}}",
                "{{PRODUCT_SCOPE}}",
                "{{PRODUCT_NON_GOALS}}",
            ][..],
        ),
        (
            "docs/architecture.md",
            &["{{ARCHITECTURE_OVERVIEW}}", "{{ARCHITECTURE_AREAS}}"][..],
        ),
    ] {
        match std::fs::read_to_string(root.join(path)) {
            Ok(content) if sentinels.iter().any(|sentinel| content.contains(sentinel)) => {
                incomplete.push(path.to_string());
            }
            Ok(_) => {}
            Err(_) => incomplete.push(format!("{path} (missing/unreadable)")),
        }
    }

    let agents = root.join("AGENTS.md");
    if agents.exists()
        && std::fs::read_to_string(&agents)
            .is_ok_and(|content| content.contains("<!-- Add project-specific notes here. -->"))
    {
        incomplete.push("AGENTS.md".to_string());
    }

    if incomplete.is_empty() {
        CheckResult {
            name: "customization".into(),
            status: Status::Pass,
            message: "consuming-project product, architecture, and agent context are customized"
                .into(),
            duration: start.elapsed(),
        }
    } else {
        CheckResult {
            name: "customization".into(),
            status: Status::Warn(
                remedy::DOCTOR_CUSTOMIZATION.with(&[("path", &incomplete.join(", "))]),
            ),
            message: format!(
                "consuming-project context still needs reconciliation: {}; `/cf-customize` verifies it against README, manifests, code, CI, harness settings, and live tools",
                incomplete.join(", ")
            ),
            duration: start.elapsed(),
        }
    }
}

/// Always-loaded instruction size (TSK-127). Codex concatenates the
/// instruction file of each directory from the project root down to its
/// working directory, reads at most 32 KiB of that chain and silently cuts
/// the rest. The project section sits last in each `AGENTS.md`, so an
/// oversized chain loses the adopter's own rules first. Every directory that
/// holds an instruction file is a possible working directory, so each one's
/// chain is measured. Warns, never fails: the fix is the project's call.
fn check_instructions(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let limit = crate::scaffold::rule_map::CODEX_INSTRUCTION_LIMIT_BYTES;
    let root = PathBuf::from(&opts.project_dir);
    let mut chains = Vec::new();
    instruction_chains(&root, Path::new(""), 0, 0, &mut chains);
    let over: Vec<&(String, usize)> = chains.iter().filter(|(_, bytes)| *bytes > limit).collect();
    let (status, message) = if chains.is_empty() {
        (
            Status::Pass,
            "no AGENTS.md (instruction size check not applicable)".to_string(),
        )
    } else if over.is_empty() {
        let (file, bytes) = chains
            .iter()
            .max_by_key(|(_, bytes)| *bytes)
            .expect("chains is not empty");
        (
            Status::Pass,
            format!(
                "{} is {bytes} bytes, {} under Codex's {limit}-byte instruction limit",
                chain_label(file),
                limit - bytes
            ),
        )
    } else {
        let listed: Vec<String> = over
            .iter()
            .map(|(file, bytes)| format!("{} is {bytes} bytes", chain_label(file)))
            .collect();
        (
            Status::Warn(remedy::DOCTOR_INSTRUCTIONS.remedy()),
            format!(
                "{}, over Codex's {limit}-byte instruction limit: Codex cuts the end of the chain, where the project section lives; move project detail into files the section points at",
                listed.join("; ")
            ),
        )
    };
    CheckResult {
        name: "instructions".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// Reading sizes (TSK-150): the always-read kernel (the managed block of
/// `AGENTS.md`), the per-task reading chain of the installed skills, and each
/// shipped skill, each against its guideline number in [`crate::reading`].
/// Sizes are reported, never failed: within every guideline is a pass that
/// states the numbers; above one is a warning that names moving detail
/// behind a trigger as the step that clears it. The Codex limit on the whole
/// `AGENTS.md` is the `instructions` check.
/// Where to edit to bring a reading measure within its guideline: a
/// skill's directory, the kernel's file, or the files the chain lists.
fn reading_location(subject: &str, skill_tree: &str) -> String {
    if subject.starts_with("kernel") {
        "the managed block of AGENTS.md".to_string()
    } else if crate::reading::SKILL_GUIDELINES
        .iter()
        .any(|(name, _)| *name == subject)
    {
        format!("{skill_tree}/{subject}/")
    } else {
        format!("the files of the {subject}")
    }
}

fn check_reading(opts: &Options) -> CheckResult {
    use crate::reading::{self, Inventory, Measure, SkillFiles};
    use crate::scaffold::rule_map;

    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let mut measures: Vec<Measure> = Vec::new();
    if let Some(block) = std::fs::read_to_string(root.join("AGENTS.md"))
        .ok()
        .as_deref()
        .and_then(rule_map::managed_block)
    {
        measures.push(Measure {
            subject: "kernel (AGENTS.md managed block)".to_string(),
            bytes: reading::authored_len(block.as_bytes()),
            guideline: rule_map::MANAGED_BLOCK_GUIDELINE_BYTES,
        });
    }
    let mut files = SkillFiles::new();
    let mut skill_tree = ".claude/skills";
    for tree in [".claude/skills", ".agents/skills"] {
        reading::load_skill_tree(&root.join(tree), &mut files);
        if !files.is_empty() {
            skill_tree = tree;
            break;
        }
    }
    let mut partial = None;
    if !files.is_empty() {
        let chain = reading::reading_chain(&files, &Inventory::SHIPPED);
        if !chain.files.is_empty() {
            if !chain.errors.is_empty() {
                partial = Some(chain.errors.len());
            }
            measures.push(chain.measure());
        }
        measures.extend(
            reading::skill_measures(&files)
                .into_iter()
                .filter(|measure| {
                    reading::SKILL_GUIDELINES
                        .iter()
                        .any(|(name, _)| *name == measure.subject)
                }),
        );
    }
    let listed = |items: Vec<&Measure>| {
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    };
    let note = partial.map_or_else(String::new, |edges| {
        format!(
            " ({edges} installed read edges differ from the shipped map, so the chain total may be partial)"
        )
    });
    let (over, within): (Vec<&Measure>, Vec<&Measure>) =
        measures.iter().partition(|measure| measure.over());
    let (status, message) = if measures.is_empty() {
        (
            Status::Pass,
            "no AGENTS.md managed block or installed skills (reading sizes not applicable)"
                .to_string(),
        )
    } else if over.is_empty() {
        (
            Status::Pass,
            format!("within guidelines: {}{note}", listed(within)),
        )
    } else {
        let subjects = over
            .iter()
            .map(|measure| reading_location(&measure.subject, skill_tree))
            .collect::<Vec<_>>()
            .join(", ");
        (
            Status::Warn(remedy::DOCTOR_READING.with(&[("path", &subjects)])),
            format!(
                "above guideline: {}. Sizes are guidelines, not failures: move detail behind a trigger (an index entry or a conditional read) to clear this, never cut a duty. Within: {}{note}",
                listed(over),
                listed(within)
            ),
        )
    };
    CheckResult {
        name: "reading".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

/// Directory depth past which the instruction walk stops.
const INSTRUCTION_WALK_DEPTH: usize = 16;

/// Collect `(instruction file, chain bytes)` for every directory under
/// `root` that holds a non-empty instruction file, where the chain is that
/// file plus the files of its ancestors up to `root`, as Codex loads them.
/// Hidden directories, build output and nested repositories or worktrees
/// (a directory holding `.git`) are not part of this project's chain;
/// symlinked directories are not followed.
fn instruction_chains(
    root: &Path,
    relative: &Path,
    depth: usize,
    inherited: usize,
    chains: &mut Vec<(String, usize)>,
) {
    let dir = root.join(relative);
    let mut total = inherited;
    if let Some((name, bytes)) = instruction_file(&dir) {
        total += bytes;
        chains.push((crate::portable_path::slashed(&relative.join(name)), total));
    }
    if depth >= INSTRUCTION_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut children: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name())
        .filter(|name| {
            let name = name.to_string_lossy();
            !name.starts_with('.') && !matches!(name.as_ref(), "target" | "node_modules")
        })
        .collect();
    children.sort();
    for child in children {
        let child_relative = relative.join(&child);
        if root.join(&child_relative).join(".git").exists() {
            continue;
        }
        instruction_chains(root, &child_relative, depth + 1, total, chains);
    }
}

/// The instruction file Codex reads in `dir`: a non-empty
/// `AGENTS.override.md` wins over `AGENTS.md`; empty files are skipped.
fn instruction_file(dir: &Path) -> Option<(&'static str, usize)> {
    ["AGENTS.override.md", "AGENTS.md"]
        .into_iter()
        .find_map(|name| {
            let bytes = std::fs::metadata(dir.join(name)).ok()?.len();
            (bytes > 0).then(|| (name, usize::try_from(bytes).unwrap_or(usize::MAX)))
        })
}

fn chain_label(file: &str) -> String {
    if Path::new(file)
        .parent()
        .is_some_and(|parent| parent.as_os_str().is_empty())
    {
        file.to_string()
    } else {
        format!("{file} with its parent instructions")
    }
}

/// Generic-testing config health. When `.codeflow/test-config.json` exists,
/// runs the testing engine's config-health checks (cwd existence, command
/// parsing, path safety, glob validity, runner probes, …) and WARNS with a
/// compact summary if any fail. Skips cleanly (Pass) when the file is absent —
/// a project need not configure the testing engine. WARN only, never a block:
/// the authoritative gate is `codeflow test` itself.
fn check_test_config(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = PathBuf::from(&opts.project_dir);
    let config_path = root.join(".codeflow").join("test-config.json");

    if !config_path.exists() {
        return CheckResult {
            name: "test-config".into(),
            status: Status::Pass,
            message: "no .codeflow/test-config.json (generic testing engine not configured)".into(),
            duration: start.elapsed(),
        };
    }

    let checks = crate::testing::doctor::run_all_checks(&root);
    let failures: Vec<String> = checks
        .iter()
        .filter(|c| c.status == crate::testing::doctor::CheckStatus::Fail)
        .map(|c| c.name.clone())
        .collect();
    let warnings: Vec<String> = checks
        .iter()
        .filter(|c| c.status == crate::testing::doctor::CheckStatus::Warn)
        .map(|c| c.name.clone())
        .collect();

    if failures.is_empty() && warnings.is_empty() {
        return CheckResult {
            name: "test-config".into(),
            status: Status::Pass,
            message: format!("test-config.json healthy ({} checks passed)", checks.len()),
            duration: start.elapsed(),
        };
    }

    if failures.is_empty() {
        return CheckResult {
            name: "test-config".into(),
            status: Status::Warn(remedy::DOCTOR_TEST_CONFIG.remedy()),
            message: format!(
                "{} test-config warning(s): {} — run `codeflow doctor --check test-config` for the aggregate result and inspect the config",
                warnings.len(),
                warnings.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "test-config".into(),
        status: Status::Warn(remedy::DOCTOR_TEST_CONFIG.remedy()),
        message: format!(
            "{} test-config health check(s) failed: {} — run `codeflow doctor --check test-config` for detail",
            failures.len(),
            failures.join(", ")
        ),
        duration: start.elapsed(),
    }
}

fn check_policy_source(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let (status, message) =
        match crate::hooks::landed_policy::diagnostic(Path::new(&opts.project_dir)) {
            Ok(message) => (Status::Pass, message),
            Err(message) => (Status::Fail, message),
        };
    CheckResult {
        name: "policy-source".into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    /// The step a warning names, or "" for any other status.
    fn warn_remedy(result: &CheckResult) -> &str {
        match &result.status {
            Status::Warn(remedy) => remedy,
            _ => "",
        }
    }

    use super::*;

    fn test_opts() -> Options {
        Options {
            look_path: Some(|_| Err("not found".into())),
            exec_command: Some(|_, _| Err("not available".into())),
            // Never the real user's harness state.
            harness_home: Some(PathBuf::from("/nonexistent/codeflow-test-home")),
            env_var: Some(|_| None),
            ..Options::default()
        }
    }

    #[test]
    fn id_registry_is_not_applicable_when_project_state_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        // A project.toml without the state fields `init` writes.
        std::fs::write(cf.join("project.toml"), "tier = \"standard\"\n").unwrap();
        let opts = Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            ..test_opts()
        };
        let result = check_id_registry(&opts);
        assert!(result.status.is_warn(), "{}", result.message);
        assert!(
            result.message.starts_with("not applicable:"),
            "{}",
            result.message
        );
    }

    #[test]
    fn id_registry_passes_when_tracking_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let opts = Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            ..test_opts()
        };
        let result = check_id_registry(&opts);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
    }

    #[test]
    fn test_check_names_count() {
        assert_eq!(check_names().len(), 20);
    }

    #[test]
    fn test_check_registry_has_all_entries() {
        let registry = check_registry();
        for &name in CHECK_NAMES {
            assert!(registry.contains_key(name), "missing check: {name}");
        }
    }

    fn write_binding(directory: &Path, observed_effort: &str) {
        std::fs::create_dir_all(directory).unwrap();
        let digest = format!("sha256:{}", "a".repeat(64));
        let record = serde_json::json!({
            "schema_version": 1,
            "binding_id": "orchid-approved-high",
            "provider": "anthropic",
            "lineage": "claude",
            "eligible_roles": ["primary", "reviewer", "claude-judgment-primary"],
            "qualified_at": "2026-07-25T10:00:00Z",
            "requested": {
                "model": "orchid-one-pin",
                "effort": "high",
                "harness": "claude-code",
                "harness_version": "2.1.220",
                "settings_digest": digest
            },
            "observed": {
                "model": "orchid-one-pin",
                "effort": observed_effort,
                "evidence": [{"kind": "session", "digest": digest}]
            },
            "qualification": {
                "run_id": "full-1",
                "suite": "full",
                "suite_digest": digest,
                "result_digest": digest,
                "codeflow_revision": "abc123"
            },
            "settings_sources": [],
            "approval": {
                "reviewer": "operator",
                "reviewed_at": "2026-07-25T10:00:00Z"
            }
        });
        std::fs::write(
            directory.join("orchid-approved-high.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn model_bindings_without_records_reports_the_managed_catalog_without_probing() {
        let directory = tempfile::tempdir().unwrap();
        let opts = Options {
            qualification_dir: Some(directory.path().join("qualified-bindings")),
            look_path: Some(|_| panic!("no binding record, so nothing to probe")),
            exec_command: Some(|_, _| panic!("doctor must not launch a model")),
            ..Options::default()
        };
        let result = check_model_bindings(&opts);
        // The managed roster is designated, not qualified: advisory, never a failure.
        assert!(result.status.is_warn(), "got: {}", result.message);
        for expected in [
            "illustrative: context-free, not a task's resolution",
            "designated version with no full-suite record",
            "no promoted model bindings and no project selection",
            "doctor did not launch a model",
        ] {
            assert!(result.message.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn model_bindings_pass_observable_version_drift_check() {
        let directory = tempfile::tempdir().unwrap();
        write_binding(directory.path(), "high");
        let mut opts = test_opts();
        opts.qualification_dir = Some(directory.path().to_path_buf());
        opts.look_path = Some(|name| {
            (name == "claude")
                .then(|| "/usr/local/bin/claude".into())
                .ok_or_else(|| "not found".into())
        });
        opts.exec_command = Some(|_, args| {
            assert_eq!(args, ["--version"]);
            Ok("2.1.220 (Claude Code)\n".into())
        });
        let result = check_model_bindings(&opts);
        assert!(result.status.is_warn(), "got: {}", result.message);
        assert!(result
            .message
            .contains("1 approved binding(s) passed structural, harness-version"));
        assert!(!result.message.contains("requalification"));
    }

    #[test]
    fn a_binding_whose_harness_has_no_version_probe_is_a_note() {
        // TSK-147: doctor cannot observe the codex-app version, so this is a
        // note naming the manual confirmation, never a warning that stays.
        let directory = tempfile::tempdir().unwrap();
        write_binding(directory.path(), "high");
        let path = directory.path().join("orchid-approved-high.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        record["binding_id"] = "codex-app-sol-high".into();
        record["provider"] = "openai".into();
        record["lineage"] = "codex".into();
        record["requested"]["model"] = "gpt-6-sol".into();
        record["requested"]["harness"] = "codex-app".into();
        record["requested"]["harness_version"] = "1.0.0".into();
        record["observed"]["model"] = "gpt-6-sol".into();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(
            directory.path().join("codex-app-sol-high.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        let mut opts = test_opts();
        opts.qualification_dir = Some(directory.path().to_path_buf());
        let result = check_model_bindings(&opts);
        // The unobservable binding alone is a note (DOCTOR_CANARY). The
        // managed schema 5 catalog adds its standing warning until each
        // designated version has a full-suite record (ADR-0069), and a
        // warning outranks a note, so the check warns and still names the
        // binding doctor cannot probe.
        assert!(
            result.status.is_warn(),
            "{:?}: {}",
            result.status,
            result.message
        );
        assert!(
            result.message.contains("no external probe"),
            "{}",
            result.message
        );
        assert!(
            result
                .message
                .contains("designated version with no full-suite record"),
            "{}",
            result.message
        );
    }

    fn write_project_selection(root: &Path, binding_id: &str) {
        let directory = root.join(".codeflow");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("model-selection.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "bindings": [{
                    "role": "claude-judgment-primary",
                    "binding_id": binding_id
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn model_bindings_reports_a_valid_active_project_selection() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("qualified-bindings");
        write_binding(&directory, "high");
        write_project_selection(workspace.path(), "orchid-approved-high");
        let mut opts = test_opts();
        opts.project_dir = workspace.path().to_string_lossy().into_owned();
        opts.qualification_dir = Some(directory);
        opts.look_path = Some(|name| {
            (name == "claude")
                .then(|| "/usr/local/bin/claude".into())
                .ok_or_else(|| "not found".into())
        });
        opts.exec_command = Some(|_, _| Ok("2.1.220 (Claude Code)".into()));
        let result = check_model_bindings(&opts);
        assert!(result.status.is_warn(), "got: {}", result.message);
        assert!(result
            .message
            .contains("project selection: claude-judgment-primary=orchid-approved-high"));
    }

    #[test]
    fn model_bindings_missing_active_record_fails_closed() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("qualified-bindings");
        std::fs::create_dir_all(&directory).unwrap();
        write_project_selection(workspace.path(), "missing");
        let mut opts = test_opts();
        opts.project_dir = workspace.path().to_string_lossy().into_owned();
        opts.qualification_dir = Some(directory);
        let result = check_model_bindings(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("missing binding record"));
    }

    #[test]
    fn model_bindings_active_drift_is_a_failure_not_a_warning() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("qualified-bindings");
        write_binding(&directory, "high");
        write_project_selection(workspace.path(), "orchid-approved-high");
        let mut opts = test_opts();
        opts.project_dir = workspace.path().to_string_lossy().into_owned();
        opts.qualification_dir = Some(directory);
        opts.look_path = Some(|name| {
            (name == "claude")
                .then(|| "/usr/local/bin/claude".into())
                .ok_or_else(|| "not found".into())
        });
        opts.exec_command = Some(|_, _| Ok("2.2.0 (Claude Code)".into()));
        let result = check_model_bindings(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("No override is applied"));
    }

    #[test]
    fn model_bindings_requested_observed_mismatch_fails() {
        let directory = tempfile::tempdir().unwrap();
        write_binding(directory.path(), "xhigh");
        let mut opts = test_opts();
        opts.qualification_dir = Some(directory.path().to_path_buf());
        let result = check_model_bindings(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("requested and observed"));
    }

    #[test]
    fn model_bindings_reports_harness_and_settings_drift_without_guessing_model() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("qualified-bindings");
        let settings = workspace.path().join("settings.json");
        std::fs::write(&settings, b"initial").unwrap();
        write_binding(&directory, "high");
        let record_path = directory.join("orchid-approved-high.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
        record["settings_sources"] = serde_json::json!([{
            "path": settings.to_string_lossy(),
            "digest": format!("sha256:{}", sha256_hex(b"initial"))
        }]);
        std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
        std::fs::write(&settings, b"changed").unwrap();

        let mut opts = test_opts();
        opts.qualification_dir = Some(directory);
        opts.look_path = Some(|name| {
            (name == "claude")
                .then(|| "/usr/local/bin/claude".into())
                .ok_or_else(|| "not found".into())
        });
        opts.exec_command = Some(|_, _| Ok("2.2.0 (Claude Code)".into()));
        let result = check_model_bindings(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("harness version changed"));
        assert!(result.message.contains("settings changed"));
        assert!(result.message.contains("never inferred by doctor"));
    }

    #[test]
    fn model_bindings_bounds_settings_source_reads() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("qualified-bindings");
        let settings = workspace.path().join("settings.json");
        let file = std::fs::File::create(&settings).unwrap();
        file.set_len(MAX_SETTINGS_BYTES + 1).unwrap();
        write_binding(&directory, "high");
        let record_path = directory.join("orchid-approved-high.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
        record["settings_sources"] = serde_json::json!([{
            "path": settings.to_string_lossy(),
            "digest": format!("sha256:{}", sha256_hex(b"unused"))
        }]);
        std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

        let mut opts = test_opts();
        opts.qualification_dir = Some(directory);
        opts.look_path = Some(|name| {
            (name == "claude")
                .then(|| "/usr/local/bin/claude".into())
                .ok_or_else(|| "not found".into())
        });
        opts.exec_command = Some(|_, _| Ok("2.1.220 (Claude Code)".into()));
        let result = check_model_bindings(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("exceeds 16777216 byte limit"));
    }

    #[test]
    fn probe_versions_use_the_allowlisted_banner_position() {
        assert_eq!(
            probe_version("claude-cli-version", "2.1.220 (Claude Code)"),
            Some("2.1.220")
        );
        assert_eq!(
            probe_version("codex-cli-version", "codex-cli 0.144.3"),
            Some("0.144.3")
        );
        assert_eq!(
            probe_version("grok-cli-version", "grok 1.0.13 (5e9a58528b76) [stable]"),
            Some("1.0.13")
        );
        assert_ne!(
            probe_version("claude-cli-version", "3.0.0 (compat 2.1.220)"),
            Some("2.1.220")
        );
        assert_eq!(probe_version("unknown", "1.0.0"), None);
    }

    #[cfg(unix)]
    #[test]
    fn version_probe_timeout_terminates_the_process_tree() {
        let started = Instant::now();
        let error =
            run_bounded_command("sh", &["-c", "sleep 2"], Duration::from_millis(50)).unwrap_err();
        assert!(error.contains("timed out after 50 ms"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn version_probe_timeout_bounds_inherited_output_pipes() {
        let started = Instant::now();
        let error = run_bounded_command(
            "sh",
            &["-c", "sleep 2 & printf 'ready\\n'"],
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.contains("timed out after 50 ms"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_run_check_unknown() {
        let opts = test_opts();
        let result = run_check("nonexistent", &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_check_claude_not_found_warns() {
        let opts = test_opts();
        let result = check_claude(&opts);
        assert!(result.status.is_warn());
    }

    #[test]
    fn test_check_claude_found() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "claude" {
                Ok("/usr/local/bin/claude".into())
            } else {
                Err("not found".into())
            }
        });
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_codex_not_scaffolded_passes_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_codex(&opts);
        assert_eq!(r.status, Status::Pass);
        assert!(
            r.message.contains("no .codex/hooks.json"),
            "got: {}",
            r.message
        );
    }

    /// The shipped `.codex/hooks.json`, and the `trusted_hash` values Codex
    /// 0.157.1 recorded in `~/.codex/config.toml` for it after `/hooks`
    /// approval (observed 2026-09-28; the hash does not depend on the path).
    const SHIPPED_CODEX_HOOKS: &str = include_str!("../../../../assets/base/codex/hooks.json");
    /// The shipped file as Codex 0.157.1 trusted it, before TSK-128 added
    /// the prompt hook.
    const OBSERVED_CODEX_HOOKS: &str = r#"{"hooks": {
      "PreToolUse": [{"matcher": "^(Bash|PowerShell)$", "hooks": [
        {"type": "command", "command": "codeflow hook git-guard", "timeout": 10},
        {"type": "command", "command": "codeflow hook exec-guard", "timeout": 10}]}],
      "SessionStart": [{"matcher": "startup|resume|clear|compact", "hooks": [
        {"type": "command", "command": "codeflow hook session-orient", "timeout": 10}]}]}}"#;
    const CODEX_TRUSTED: [(&str, &str); 3] = [
        (
            "pre_tool_use:0:0",
            "sha256:42d1067865f6352ae0975d92a473334f58d7748b8afb7a27e36b7dc834d204d3",
        ),
        (
            "pre_tool_use:0:1",
            "sha256:02d37e4136d171ce43056b705e61eade1ae3c7d97bfd1f497197413be24a42ce",
        ),
        (
            "session_start:0:0",
            "sha256:261fbd3855b8f99f89ac885329096492c3635babbedd779a4526fc46a586bf66",
        ),
    ];

    /// `path`'s canonical form, as a harness records it in a trust key (the
    /// plain drive form on Windows), escaped for a TOML basic string.
    fn toml_path(path: &Path) -> String {
        crate::portable_path::canonicalize(path)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "\\\\")
    }

    /// A project with `hooks` as its `.codex/hooks.json`, and a harness home
    /// whose Codex config trusts `trusted` handler keys at their hashes.
    fn codex_project(hooks: &str, trusted: &[(&str, &str)]) -> (tempfile::TempDir, Options) {
        use std::fmt::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".codex")).unwrap();
        std::fs::write(project.join(".codex/hooks.json"), hooks).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let hooks_path = toml_path(&project.join(".codex/hooks.json"));
        let mut config = String::new();
        for (key, hash) in trusted {
            let _ = writeln!(
                config,
                "[hooks.state.\"{hooks_path}:{key}\"]\ntrusted_hash = \"{hash}\"\n"
            );
        }
        std::fs::write(home.join(".codex/config.toml"), config).unwrap();
        let mut opts = test_opts();
        opts.project_dir = project.to_string_lossy().into_owned();
        opts.harness_home = Some(home);
        (dir, opts)
    }

    #[test]
    fn codex_trust_recorded_for_every_hook_is_configured_not_run() {
        // TSK-147: doctor reads the trust Codex records instead of warning
        // that it cannot; the hashes are Codex's own.
        let (_dir, opts) = codex_project(OBSERVED_CODEX_HOOKS, &CODEX_TRUSTED);
        let r = check_codex(&opts);
        assert!(is_configured(&r), "{:?} {}", r.status, r.message);
        assert!(r.message.contains("3 of 3"), "{}", r.message);
    }

    #[test]
    fn the_shipped_codex_hooks_are_all_hashable() {
        // Contract 3 changes every command, including the fifth edit guard;
        // none matches the three older approvals in this observed fixture.
        let (_dir, opts) = codex_project(SHIPPED_CODEX_HOOKS, &CODEX_TRUSTED);
        let r = check_codex(&opts);
        assert!(r.status.is_warn(), "{}", r.message);
        assert!(r.message.contains("0 of 5"), "{}", r.message);
    }

    #[test]
    fn codex_hooks_without_recorded_trust_warn_with_the_approval_step() {
        let (_dir, opts) = codex_project(OBSERVED_CODEX_HOOKS, &CODEX_TRUSTED[..1]);
        let r = check_codex(&opts);
        assert!(r.status.is_warn(), "{}", r.message);
        assert!(r.message.contains("1 of 3"), "{}", r.message);
        assert!(
            warn_remedy(&r).contains("`/hooks` inside interactive codex")
                && warn_remedy(&r).contains("codeflow doctor --check codex"),
            "{}",
            warn_remedy(&r)
        );
        // No Codex config at all: nothing trusted.
        let (dir, mut opts) = codex_project(SHIPPED_CODEX_HOOKS, &[]);
        opts.harness_home = Some(dir.path().join("elsewhere"));
        assert!(check_codex(&opts).status.is_warn());
    }

    #[test]
    fn a_codex_hook_changed_since_approval_warns() {
        let changed = OBSERVED_CODEX_HOOKS.replacen("\"timeout\": 10", "\"timeout\": 20", 1);
        let (_dir, opts) = codex_project(&changed, &CODEX_TRUSTED);
        let r = check_codex(&opts);
        assert!(r.status.is_warn(), "{}", r.message);
        assert!(r.message.contains("2 of 3"), "{}", r.message);
    }

    #[test]
    fn a_codex_hook_shape_doctor_cannot_hash_is_a_note() {
        let odd = r#"{"hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "x", "timeout": 5, "statusMessage": "s"}]}]}}"#;
        let (_dir, opts) = codex_project(odd, &[]);
        let r = check_codex(&opts);
        let Status::Note(remedy) = &r.status else {
            panic!("expected a note: {:?}", r.status);
        };
        assert!(remedy.contains("cannot verify"), "{remedy}");
        assert!(remedy.contains("`/hooks`"), "{remedy}");
    }

    /// `CODEX_TRUSTED` with each record's body replaced by `entry`, where
    /// `{hash}` is the record's own hash.
    fn codex_config_with(entry: &str) -> String {
        use std::fmt::Write as _;
        let mut config = String::new();
        for (key, hash) in CODEX_TRUSTED {
            let _ = writeln!(
                config,
                "[hooks.state.\"<path>:{key}\"]\n{}\n",
                entry.replace("{hash}", hash)
            );
        }
        config
    }

    fn rewrite_codex_config(opts: &Options, config: &str) {
        let project = Path::new(&opts.project_dir);
        let hooks = toml_path(&project.join(".codex/hooks.json"));
        let home = opts.harness_home.clone().unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            config.replace("<path>", &hooks),
        )
        .unwrap();
    }

    #[test]
    fn a_codex_trust_record_codex_rejects_is_not_trust() {
        // TSK-147 F2: Codex reads each state record as a whole and skips one
        // whose fields have the wrong type, so its hash trusts nothing.
        let (_dir, opts) = codex_project(OBSERVED_CODEX_HOOKS, &CODEX_TRUSTED);
        for entry in [
            "trusted_hash = \"{hash}\"\nenabled = \"yes\"",
            "trusted_hash = [\"{hash}\"]",
        ] {
            rewrite_codex_config(&opts, &codex_config_with(entry));
            let r = check_codex(&opts);
            assert!(r.status.is_warn(), "{entry}: {:?} {}", r.status, r.message);
            assert!(r.message.contains("0 of 3"), "{entry}: {}", r.message);
        }
    }

    #[test]
    fn a_disabled_codex_hook_does_not_pass() {
        // A trusted hook the user disabled does not run.
        let (_dir, opts) = codex_project(OBSERVED_CODEX_HOOKS, &CODEX_TRUSTED);
        rewrite_codex_config(
            &opts,
            &codex_config_with("trusted_hash = \"{hash}\"\nenabled = false"),
        );
        let r = check_codex(&opts);
        assert!(r.status.is_warn(), "{:?} {}", r.status, r.message);
    }

    #[test]
    fn a_codex_hooks_file_codex_cannot_read_is_a_note() {
        // Groups or handlers that are not lists are not an empty hook set.
        for hooks in [
            r#"{"hooks": {"PreToolUse": {"matcher": "x"}}}"#,
            r#"{"hooks": {"PreToolUse": [{"matcher": "x", "hooks": {"type": "command"}}]}}"#,
            r#"{"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "x", "timeout": 5, "async": "no"}]}]}}"#,
            r#"{"hooks": {}, "extra": 1}"#,
            r#"{"hooks": {}}"#,
        ] {
            let (_dir, opts) = codex_project(hooks, &CODEX_TRUSTED);
            let r = check_codex(&opts);
            assert!(
                matches!(r.status, Status::Note(_)),
                "{hooks}: {:?} {}",
                r.status,
                r.message
            );
        }
    }

    #[test]
    fn test_check_grok_not_scaffolded_passes_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_grok(&opts);
        assert_eq!(r.status, Status::Pass);
        assert!(
            r.message.contains("no .grok/hooks/*.json"),
            "got: {}",
            r.message
        );
    }

    /// A project with a Grok hook file, and a harness home whose folder
    /// trust store is `store` (none when `None`).
    fn grok_project(store: Option<&str>) -> (tempfile::TempDir, Options, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".grok/hooks")).unwrap();
        std::fs::write(project.join(".grok/hooks/codeflow.json"), "{}").unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        let root = toml_path(&project);
        if let Some(store) = store {
            let text = store.replace("<root>", &root);
            std::fs::write(home.join(".grok/trusted_folders.toml"), text).unwrap();
        }
        let mut opts = test_opts();
        opts.project_dir = project.to_string_lossy().into_owned();
        opts.harness_home = Some(home.clone());
        (dir, opts, home)
    }

    #[test]
    fn a_grok_folder_trusted_in_the_store_is_configured_not_run() {
        let (_dir, opts, _) = grok_project(Some(
            "[folders.\"<root>\"]\ntrusted = true\ndecided_at = 1\n",
        ));
        let r = check_grok(&opts);
        assert!(is_configured(&r), "{:?} {}", r.status, r.message);
    }

    #[test]
    fn an_untrusted_grok_folder_warns_with_the_trust_step() {
        for store in [
            None,
            Some("[folders.\"<root>\"]\ntrusted = false\ndecided_at = 1\n"),
            Some("[folders.\"/somewhere/else\"]\ntrusted = true\n"),
        ] {
            let (_dir, opts, _) = grok_project(store);
            let r = check_grok(&opts);
            assert!(r.status.is_warn(), "{store:?}: {}", r.message);
            assert!(
                warn_remedy(&r).contains("/hooks-trust")
                    && warn_remedy(&r).contains("codeflow doctor --check grok"),
                "{}",
                warn_remedy(&r)
            );
        }
    }

    #[test]
    fn grok_folder_trust_turned_off_is_configured_not_run() {
        let (_dir, opts, home) = grok_project(None);
        std::fs::write(
            home.join(".grok/config.toml"),
            "[folder_trust]\nenabled = false\n",
        )
        .unwrap();
        let r = check_grok(&opts);
        assert!(is_configured(&r), "{:?} {}", r.status, r.message);
    }

    fn grok_env_on(name: &str) -> Option<String> {
        (name == "GROK_FOLDER_TRUST").then(|| "1".to_string())
    }

    fn grok_env_off(name: &str) -> Option<String> {
        (name == "GROK_FOLDER_TRUST").then(|| "false".to_string())
    }

    #[test]
    fn grok_folder_trust_follows_grok_precedence() {
        // TSK-147 F3: env, then user config, then managed config, then on
        // (flags.rs resolve_bool_flag; folder_trust.rs feature_enabled).
        let off = "[folder_trust]\nenabled = false\n";
        let on = "[folder_trust]\nenabled = true\n";
        // The environment turning it on overrides a user config turning it off.
        let (_dir, mut opts, home) = grok_project(None);
        std::fs::write(home.join(".grok/config.toml"), off).unwrap();
        opts.env_var = Some(grok_env_on);
        assert!(check_grok(&opts).status.is_warn());
        // The environment turning it off, in any spelling grok reads.
        let (_dir, mut opts, _) = grok_project(None);
        opts.env_var = Some(grok_env_off);
        assert!(is_configured(&check_grok(&opts)));
        // The user config wins over the managed config.
        let (_dir, opts, home) = grok_project(None);
        std::fs::write(home.join(".grok/config.toml"), on).unwrap();
        std::fs::write(home.join(".grok/managed_config.toml"), off).unwrap();
        assert!(check_grok(&opts).status.is_warn());
        // The managed config applies when the user config is silent.
        let (_dir, opts, home) = grok_project(None);
        std::fs::write(home.join(".grok/managed_config.toml"), off).unwrap();
        assert!(is_configured(&check_grok(&opts)));
    }

    #[test]
    fn a_grok_trust_store_grok_rejects_trusts_nothing() {
        // TSK-147 F2: grok reads the store as a whole; a record with a field
        // of the wrong type makes it unreadable, and then nothing is trusted.
        for store in [
            "[folders.\"<root>\"]\ntrusted = true\ndecided_at = \"yesterday\"\n",
            "[folders.\"<root>\"]\ntrusted = \"yes\"\n",
            "folders = 1\n",
        ] {
            let (_dir, opts, _) = grok_project(Some(store));
            let r = check_grok(&opts);
            assert!(r.status.is_warn(), "{store}: {:?} {}", r.status, r.message);
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_grok_grant_under_another_spelling_of_the_folder_is_not_trust() {
        // Grok compares the canonical folder with the recorded key as
        // written, so a grant recorded under a symlink does not cover it.
        let (dir, mut opts, home) = grok_project(None);
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(dir.path().join("project"), &alias).unwrap();
        opts.project_dir = alias.to_string_lossy().into_owned();
        std::fs::write(
            home.join(".grok/trusted_folders.toml"),
            format!("[folders.\"{}\"]\ntrusted = true\n", alias.display()),
        )
        .unwrap();
        assert!(check_grok(&opts).status.is_warn());
    }

    #[test]
    fn a_grok_grant_on_a_filesystem_root_never_counts() {
        let (_dir, opts, _) = grok_project(Some("[folders.\"/\"]\ntrusted = true\n"));
        assert!(check_grok(&opts).status.is_warn());
    }

    #[test]
    fn grok_home_honours_its_environment_variable() {
        fn env(name: &str) -> Option<String> {
            (name == "GROK_HOME").then(|| "/elsewhere/grok".to_string())
        }
        let opts = Options {
            harness_home: None,
            env_var: Some(env),
            ..test_opts()
        };
        assert_eq!(opts.grok_home(), PathBuf::from("/elsewhere/grok"));
    }

    #[test]
    fn an_unreadable_grok_trust_store_trusts_nothing() {
        // Grok treats a store it cannot read as trusting no folder.
        let (_dir, opts, _) = grok_project(Some("not = [toml"));
        let r = check_grok(&opts);
        assert!(r.status.is_warn(), "{:?} {}", r.status, r.message);
        assert!(
            warn_remedy(&r).contains("repair or remove")
                && warn_remedy(&r).contains("/hooks-trust"),
            "{}",
            warn_remedy(&r)
        );
    }

    /// A trust configuration doctor finds matching: a note that the
    /// runtime is not verified (TSK-147 round 3 F2), never a pass.
    fn is_configured(result: &CheckResult) -> bool {
        matches!(result.status, Status::Note(_))
            && result.message.contains("configured; runtime not verified")
    }

    fn is_note(result: &CheckResult) -> bool {
        matches!(&result.status, Status::Note(remedy) if remedy.contains("cannot verify"))
    }

    #[test]
    fn a_grok_config_with_version_overrides_is_unverified() {
        // TSK-147 F3 round 2: grok applies `[[version_overrides]]` for its own
        // version before it reads `folder_trust.enabled` (loader.rs
        // load_config_file), which doctor cannot reproduce: no Pass.
        let patched = "[folder_trust]\nenabled = false\n\n[[version_overrides]]\n\
                       [version_overrides.folder_trust]\nenabled = true\n";
        for layer in ["config.toml", "managed_config.toml"] {
            let (_dir, opts, home) = grok_project(None);
            std::fs::write(home.join(".grok").join(layer), patched).unwrap();
            let r = check_grok(&opts);
            assert!(is_note(&r), "{layer}: {:?} {}", r.status, r.message);
        }
        // A user config that decides the gate without patches is read as
        // written, and the managed config's patches never apply then.
        let (_dir, opts, home) = grok_project(None);
        std::fs::write(
            home.join(".grok/config.toml"),
            "[folder_trust]\nenabled = false\n",
        )
        .unwrap();
        std::fs::write(home.join(".grok/managed_config.toml"), patched).unwrap();
        assert!(is_configured(&check_grok(&opts)));
        // The environment outranks every config layer.
        let (_dir, mut opts, home) = grok_project(None);
        std::fs::write(home.join(".grok/config.toml"), patched).unwrap();
        opts.env_var = Some(grok_env_off);
        assert!(is_configured(&check_grok(&opts)));
    }

    #[test]
    fn a_relative_grok_home_is_unverified() {
        // Grok refuses a relative trust-store home and reads its config
        // against its own working directory (trust.rs default_path_in).
        fn env(name: &str) -> Option<String> {
            (name == "GROK_HOME").then(|| "relative-grok".to_string())
        }
        let (_dir, mut opts, _) = grok_project(None);
        opts.harness_home = None;
        opts.env_var = Some(env);
        let r = check_grok(&opts);
        assert!(is_note(&r), "{:?} {}", r.status, r.message);
    }

    #[test]
    fn a_linked_worktree_takes_its_grok_trust_from_the_main_checkout() {
        // Grok keys a linked worktree by its main checkout (trust.rs
        // git_derived_workspace_key), so only a grant covering the main
        // checkout trusts it.
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("tracked"), "x").unwrap();
        git(&main, &["add", "tracked"]);
        git(
            &main,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.test",
                "commit",
                "-qm",
                "seed",
            ],
        );
        let linked = dir.path().join("linked");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "wt",
                linked.to_str().unwrap(),
            ],
        );
        std::fs::create_dir_all(linked.join(".grok/hooks")).unwrap();
        std::fs::write(linked.join(".grok/hooks/codeflow.json"), "{}").unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = linked.to_string_lossy().into_owned();
        opts.harness_home = Some(home.clone());
        let grant = |folder: &Path| {
            std::fs::write(
                home.join(".grok/trusted_folders.toml"),
                format!("[folders.\"{}\"]\ntrusted = true\n", toml_path(folder)),
            )
            .unwrap();
        };
        grant(&linked);
        let r = check_grok(&opts);
        assert!(r.status.is_warn(), "{:?} {}", r.status, r.message);
        grant(&main);
        let r = check_grok(&opts);
        assert!(is_configured(&r), "{:?} {}", r.status, r.message);
    }

    #[test]
    fn a_grok_managed_worktree_is_unverified() {
        // Grok keys a worktree under its home's worktrees/ through its
        // worktrees.db registry, which doctor does not read.
        let (dir, mut opts, home) = grok_project(None);
        let managed = home.join(".grok/worktrees/repo/wt");
        std::fs::create_dir_all(managed.join(".grok/hooks")).unwrap();
        std::fs::write(managed.join(".grok/hooks/codeflow.json"), "{}").unwrap();
        std::fs::write(
            home.join(".grok/trusted_folders.toml"),
            format!("[folders.\"{}\"]\ntrusted = true\n", toml_path(&managed)),
        )
        .unwrap();
        opts.project_dir = managed.to_string_lossy().into_owned();
        let r = check_grok(&opts);
        assert!(is_note(&r), "{:?} {}", r.status, r.message);
        drop(dir);
    }

    /// A project whose `.codex/hooks.json` wires `groups` under `event`, and
    /// a Codex config trusting its first handler at `hash`.
    fn codex_single(event: &str, groups: &str, hash: &str) -> CheckResult {
        let hooks = format!(r#"{{"hooks": {{"{event}": {groups}}}}}"#);
        let key = format!("{}:0:0", snake_case(event));
        let (_dir, opts) = codex_project(&hooks, &[(&key, hash)]);
        check_codex(&opts)
    }

    #[test]
    fn codex_hooks_are_hashed_after_codex_normalizes_them() {
        // TSK-147 F2 round 2, the reviewer's probes: each hash below is the
        // one doctor computed before Codex's normalization (discovery.rs
        // append_matcher_groups, normalize_command_hook), so none is trust.
        let ignored_matcher = codex_single(
            "UserPromptSubmit",
            r#"[{"matcher":"bogus","hooks":[{"type":"command","command":"codeflow hook session-orient","timeout":10}]}]"#,
            "sha256:8cae15bf3a1950e8e9b95a6945dd894e1c3eedd8ae9e72843f01e09e5c97e099",
        );
        assert!(
            ignored_matcher.status.is_warn(),
            "{}",
            ignored_matcher.message
        );
        let zero_timeout = codex_single(
            "PreToolUse",
            r#"[{"matcher":"^Bash$","hooks":[{"type":"command","command":"x","timeout":0}]}]"#,
            "sha256:de305438036e547f0be717966bc7968b34e3c7da64088816f6bddbab0f9ef872",
        );
        assert!(zero_timeout.status.is_warn(), "{}", zero_timeout.message);
        // Codex skips a group whose matcher does not compile and a hook
        // whose command is empty: doctor cannot say what then runs.
        let invalid_matcher = codex_single(
            "PreToolUse",
            r#"[{"matcher":"[","hooks":[{"type":"command","command":"x","timeout":10}]}]"#,
            "sha256:20a7b5a3325c6c3477af23788f7be368fcc8ed32b060cbb5dc84cc1af89ac28c",
        );
        assert!(is_note(&invalid_matcher), "{:?}", invalid_matcher.status);
        let empty_command = codex_single(
            "PreToolUse",
            r#"[{"matcher":"^Bash$","hooks":[{"type":"command","command":"   ","timeout":10}]}]"#,
            "sha256:218d05af2d3beb588aeb86a90ed64aef17344d31dc8bbd286883ed090ecefc77",
        );
        assert!(is_note(&empty_command), "{:?}", empty_command.status);
    }

    #[test]
    fn codex_normalization_gives_equivalent_hooks_one_hash() {
        // The identity Codex hashes: no matcher on UserPromptSubmit, Stop
        // and Interrupt; a missing or zero timeout at its default or floor.
        let hash = |event: &str, matcher: Option<&str>, timeout: Option<u64>| {
            let handler = CodexHandler::Command {
                command: "x".into(),
                command_windows: None,
                timeout_sec: timeout,
                asynchronous: false,
                status_message: None,
                additional_context_limit: None,
            };
            codex_hook_hash(event, matcher, &handler).unwrap()
        };
        for event in ["UserPromptSubmit", "Stop", "Interrupt"] {
            assert_eq!(
                hash(event, Some("bogus"), Some(2)),
                hash(event, None, Some(2))
            );
        }
        assert_ne!(
            hash("PreToolUse", Some("Bash"), Some(10)),
            hash("PreToolUse", None, Some(10))
        );
        assert_eq!(
            hash("PreToolUse", None, None),
            hash("PreToolUse", None, Some(600))
        );
        assert_eq!(
            hash("PreToolUse", None, Some(0)),
            hash("PreToolUse", None, Some(1))
        );
        assert_eq!(
            hash("SessionEnd", None, None),
            hash("SessionEnd", None, Some(1))
        );
        assert_eq!(
            hash("SessionEnd", None, Some(9)),
            hash("SessionEnd", None, Some(3))
        );
    }

    #[test]
    fn test_check_config_missing_dir_warns() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("policy.json"), "{}").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("bad.json"), "not json!!").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("invalid"));
        assert!(result.message.contains("bad.json"));
    }

    #[test]
    fn test_check_config_nested_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join(".codeflow").join(".baseline");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("settings.json"), "{ nope").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
    }

    #[test]
    fn test_customization_is_not_applicable_without_method_docs() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let result = check_customization(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("not applicable"));
    }

    #[test]
    fn test_customization_warns_on_shipped_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/product.md"),
            "# Product\n{{PRODUCT_PURPOSE}}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("docs/architecture.md"),
            "# Architecture\n{{ARCHITECTURE_AREAS}}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "<!-- Add project-specific notes here. -->\n",
        )
        .unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let result = check_customization(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("docs/product.md"));
        assert!(result.message.contains("docs/architecture.md"));
        assert!(result.message.contains("AGENTS.md"));
        assert!(result.message.contains("/cf-customize"));
    }

    #[test]
    fn test_customization_passes_after_reconciliation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/product.md"),
            "# Product\nUseful context\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("docs/architecture.md"),
            "# Architecture\nReal component map\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "Project commands: cargo test\n",
        )
        .unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let result = check_customization(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    /// A project with a small managed `AGENTS.md` and one installed skill
    /// whose `SKILL.md` is `skill_bytes` long.
    fn reading_project(skill: &str, skill_bytes: usize) -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# p\n\n<!-- codeflow:managed:begin scaffold=3.0.0 -->\nrules\n<!-- codeflow:managed:end -->\n",
        )
        .unwrap();
        let skill_dir = dir.path().join(".claude/skills").join(skill);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "x".repeat(skill_bytes)).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        (dir, opts)
    }

    #[test]
    fn test_reading_is_not_applicable_without_a_map_or_skills() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_reading(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(
            result.message.contains("not applicable"),
            "{}",
            result.message
        );
    }

    #[test]
    fn test_reading_reports_a_skill_at_its_guideline_as_information() {
        let guideline = crate::reading::skill_guideline("cf-herdr");
        let (_dir, opts) = reading_project("cf-herdr", guideline);
        let result = check_reading(&opts);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            result
                .message
                .contains(&format!("cf-herdr {guideline} of {guideline} bytes")),
            "{}",
            result.message
        );
        assert!(result.message.contains("kernel (AGENTS.md managed block)"));
    }

    #[test]
    fn test_reading_warns_above_a_guideline_and_names_the_step_that_clears_it() {
        let guideline = crate::reading::skill_guideline("cf-herdr");
        let (dir, opts) = reading_project("cf-herdr", 2 * guideline);
        let result = check_reading(&opts);
        assert!(result.status.is_warn(), "{}", result.message);
        assert!(result.message.starts_with(&format!(
            "above guideline: cf-herdr {} of {guideline} bytes",
            2 * guideline
        )));
        assert!(
            result.message.contains("move detail behind a trigger"),
            "{}",
            result.message
        );

        // A kernel past its guideline warns the same way.
        let big = format!(
            "<!-- codeflow:managed:begin -->\n{}\n<!-- codeflow:managed:end -->\n",
            "r".repeat(crate::scaffold::rule_map::MANAGED_BLOCK_GUIDELINE_BYTES)
        );
        std::fs::write(dir.path().join("AGENTS.md"), big).unwrap();
        std::fs::write(dir.path().join(".claude/skills/cf-herdr/SKILL.md"), "x").unwrap();
        let result = check_reading(&opts);
        assert!(result.status.is_warn(), "{}", result.message);
        assert!(
            result
                .message
                .starts_with("above guideline: kernel (AGENTS.md managed block)"),
            "{}",
            result.message
        );
    }

    #[test]
    fn test_reading_measures_the_installed_chain_and_ignores_adopter_skills() {
        let (dir, opts) = reading_project("team-release-notes", 100_000);
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base");
        let mut shipped = crate::reading::SkillFiles::new();
        for tree in ["agents/skills", "claude/skills"] {
            crate::reading::load_skill_tree(&assets.join(tree), &mut shipped);
        }
        for (path, text) in &shipped {
            let dest = dir.path().join(".claude/skills").join(path);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::write(dest, text).unwrap();
        }
        let result = check_reading(&opts);
        assert!(
            result.message.contains("per-task reading chain"),
            "{}",
            result.message
        );
        assert!(
            !result.message.contains("installed read edges differ"),
            "{}",
            result.message
        );
        assert!(
            !result.message.contains("team-release-notes"),
            "an adopter's own skill is not measured: {}",
            result.message
        );
        for (skill, _) in crate::reading::SKILL_GUIDELINES {
            assert!(result.message.contains(&format!("{skill} ")), "{skill}");
        }
    }

    #[test]
    fn test_instructions_warns_past_the_codex_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        assert_eq!(check_instructions(&opts).status, Status::Pass);

        std::fs::write(dir.path().join("AGENTS.md"), "x".repeat(32 * 1024)).unwrap();
        let at_limit = check_instructions(&opts);
        assert_eq!(at_limit.status, Status::Pass, "{}", at_limit.message);
        assert!(at_limit.message.contains("0 under"));

        std::fs::write(dir.path().join("AGENTS.md"), "x".repeat(32 * 1024 + 1)).unwrap();
        let over = check_instructions(&opts);
        assert!(over.status.is_warn());
        assert!(over.message.contains("32769 bytes"), "{}", over.message);
        assert!(over.message.contains("project section"));
    }

    /// Codex review probe (TSK-127 F1): an 18-byte root file and a
    /// 32,769-byte `nested/AGENTS.md` must warn, naming the nested chain.
    #[test]
    fn test_instructions_measures_nested_chains() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("AGENTS.md"), "x".repeat(18)).unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(
            dir.path().join("nested/AGENTS.md"),
            "x".repeat(32 * 1024 + 1),
        )
        .unwrap();
        let nested = check_instructions(&opts);
        assert!(nested.status.is_warn(), "{}", nested.message);
        assert!(
            nested
                .message
                .contains("nested/AGENTS.md with its parent instructions is 32787 bytes"),
            "{}",
            nested.message
        );

        // The chain is what counts: two files each under the limit.
        std::fs::write(dir.path().join("AGENTS.md"), "x".repeat(20 * 1024)).unwrap();
        std::fs::write(dir.path().join("nested/AGENTS.md"), "x".repeat(13 * 1024)).unwrap();
        assert!(check_instructions(&opts).status.is_warn());

        // An override replaces its directory's AGENTS.md in the chain.
        std::fs::write(dir.path().join("nested/AGENTS.override.md"), "short\n").unwrap();
        let overridden = check_instructions(&opts);
        assert_eq!(overridden.status, Status::Pass, "{}", overridden.message);

        // A nested repository or worktree is its own project root.
        std::fs::remove_file(dir.path().join("nested/AGENTS.override.md")).unwrap();
        std::fs::create_dir_all(dir.path().join("nested/.git")).unwrap();
        assert_eq!(check_instructions(&opts).status, Status::Pass);

        // No root file, oversized nested file: still found.
        std::fs::remove_dir_all(dir.path().join("nested/.git")).unwrap();
        std::fs::remove_file(dir.path().join("AGENTS.md")).unwrap();
        std::fs::write(
            dir.path().join("nested/AGENTS.md"),
            "x".repeat(32 * 1024 + 1),
        )
        .unwrap();
        assert!(check_instructions(&opts).status.is_warn());
    }

    /// git in a tempdir, isolated from the host config (mirrors orient's
    /// test helper — both surfaces resolve hook wiring the same way).
    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(out.status.success());
    }

    /// Options where the codeflow binary is found and every hook subcommand
    /// probe succeeds — isolating the wiring probe under test.
    fn hooks_opts(dir: &Path) -> Options {
        let mut opts = test_opts();
        opts.project_dir = dir.to_string_lossy().into_owned();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, args| {
            Ok(if args == ["git-hook", "capabilities"] {
                "hooks 3"
            } else {
                "help output"
            }
            .to_string())
        });
        opts
    }

    fn write_shims(root: &Path) {
        let shims = root.join(".codeflow/git-hooks");
        std::fs::create_dir_all(&shims).unwrap();
        std::fs::write(
            shims.join("pre-commit"),
            "#!/bin/sh\nexec codeflow git-hook pre-commit\n",
        )
        .unwrap();
    }

    #[test]
    fn test_check_hooks_no_binary() {
        let opts = test_opts();
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_hooks_all_pass() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_hooks(&hooks_opts(dir.path()));
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("functional"));
        assert!(result.message.contains('8'));
    }

    #[test]
    fn test_check_hooks_warns_when_shims_not_active() {
        // The fresh-clone signature: shims committed in the tree, but the
        // local core.hooksPath wiring is gone — subcommands respond, git
        // calls nothing.
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        write_shims(dir.path());
        let r = check_hooks(&hooks_opts(dir.path()));
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(
            warn_remedy(&r).contains("git config core.hooksPath .codeflow/git-hooks"),
            "remedy: {}",
            r.message
        );
    }

    #[test]
    fn test_check_hooks_passes_when_hookspath_wired() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        write_shims(dir.path());
        git(
            dir.path(),
            &["config", "core.hooksPath", ".codeflow/git-hooks"],
        );
        let r = check_hooks(&hooks_opts(dir.path()));
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_check_hooks_warns_when_hookspath_is_absolute() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        write_shims(dir.path());
        let absolute = dir.path().join(".codeflow/git-hooks");
        git(
            dir.path(),
            &[
                "config",
                "core.hooksPath",
                absolute.to_str().expect("utf-8 path"),
            ],
        );
        let r = check_hooks(&hooks_opts(dir.path()));
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(
            r.message.contains("absolute"),
            "expected absolute-path warning, got: {}",
            r.message
        );
        assert!(
            warn_remedy(&r).contains("git config core.hooksPath .codeflow/git-hooks"),
            "remedy: {}",
            r.message
        );
    }

    #[test]
    fn test_check_hooks_recorded_unwired_warns_with_manager_remedy() {
        // Another hook manager owned the hooks at init (recorded, deliberate:
        // init never clobbers) — the remedy is calling the shims from that
        // manager, not flipping core.hooksPath under it.
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        write_shims(dir.path());
        crate::scaffold::state::ProjectState {
            schema_version: 1,
            tier: crate::scaffold::manifest::Tier::Standard,
            scaffold_version: "2.0.0".into(),
            stack: "rust".into(),
            areas: vec!["core".into()],
            policy_armed: true,
            git_hooks: crate::scaffold::state::GIT_HOOKS_UNWIRED.into(),
            permission_preset: "acceptEdits".into(),
            product_one_liner: "x".into(),
            release_rules: None,
        }
        .store(dir.path())
        .unwrap();
        let r = check_hooks(&hooks_opts(dir.path()));
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(r.message.contains("hook manager"), "got: {}", r.message);
        assert!(
            !r.message.contains("git config core.hooksPath"),
            "must not tell the user to clobber their manager: {}",
            r.message
        );
    }

    /// A repo whose hooks another manager owns in `.husky`, each hook
    /// written by `hook` from its name, with the given mode.
    #[cfg(unix)]
    fn managed_hooks(dir: &Path, hook: impl Fn(&str) -> String, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        git(dir, &["init", "-b", "main"]);
        write_shims(dir);
        let husky = dir.join(".husky");
        std::fs::create_dir_all(&husky).unwrap();
        for entry in std::fs::read_dir(dir.join(".codeflow/git-hooks")).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            let path = husky.join(&name);
            std::fs::write(&path, hook(&name)).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        git(dir, &["config", "core.hooksPath", ".husky"]);
    }

    /// TSK-147 round 3 F6: a manager hook calls its shim only when git runs
    /// it (the file is executable) and the call is on a live line. The
    /// reviewer's fixtures: hooks that name the shim but are not
    /// executable, and executable hooks whose call is commented out.
    #[cfg(unix)]
    #[test]
    fn a_manager_hook_that_git_skips_or_that_comments_the_call_warns() {
        let live = |name: &str| format!("#!/bin/sh\n.codeflow/git-hooks/{name} \"$@\"\n");
        let commented =
            |name: &str| format!("#!/bin/sh\n# .codeflow/git-hooks/{name} \"$@\"\nexit 0\n");
        let trailing =
            |name: &str| format!("#!/bin/sh\nexit 0 # .codeflow/git-hooks/{name} \"$@\"\n");
        for (label, hook, mode, why) in [
            (
                "not executable",
                &live as &dyn Fn(&str) -> String,
                0o644,
                "not executable",
            ),
            ("commented", &commented, 0o755, "no live call"),
            ("trailing comment", &trailing, 0o755, "no live call"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            managed_hooks(dir.path(), hook, mode);
            let r = check_hooks(&hooks_opts(dir.path()));
            assert!(r.status.is_warn(), "{label}: {}", r.message);
            assert!(r.message.contains(why), "{label}: {}", r.message);
        }
    }

    /// TSK-147 round 4 F6: reading a manager's hook proves only the
    /// negative. An executable hook that names every shim may still never
    /// run it, so it is a note, "wiring not verified", naming the real git
    /// event that confirms it, and never a pass. The reviewer's probes: a
    /// call after `exit`, in a false branch, in an unused function, printed,
    /// passed as data, inside a here-document, a masked failure, and the
    /// straight-line control, which reading cannot tell apart from them.
    #[cfg(unix)]
    #[test]
    fn a_manager_hook_that_names_its_shim_is_a_note_never_a_pass() {
        type Probe<'a> = (&'a str, &'a dyn Fn(&str) -> String);
        let call = |name: &str| format!(".codeflow/git-hooks/{name} \"$@\"");
        let probes: [Probe; 8] = [
            ("after exit", &|n| {
                format!("#!/bin/sh\nexit 0\n{}\n", call(n))
            }),
            ("false branch", &|n| {
                format!("#!/bin/sh\nif false; then\n  {}\nfi\nexit 0\n", call(n))
            }),
            ("unused function", &|n| {
                format!("#!/bin/sh\nunused() {{\n  {}\n}}\nexit 0\n", call(n))
            }),
            ("echo only", &|n| {
                format!("#!/bin/sh\nprintf '%s\\n' '.codeflow/git-hooks/{n}'\nexit 0\n")
            }),
            ("colon data", &|n| {
                format!("#!/bin/sh\n: '.codeflow/git-hooks/{n}'\nexit 0\n")
            }),
            ("here document", &|n| {
                format!(
                    "#!/bin/sh\ncat >/dev/null <<'EOF'\n{}\nEOF\nexit 0\n",
                    call(n)
                )
            }),
            ("masked failure", &|n| {
                format!("#!/bin/sh\n{} || true\n", call(n))
            }),
            ("live control", &|n| format!("#!/bin/sh\n{}\n", call(n))),
        ];
        for (label, hook) in probes {
            let dir = tempfile::tempdir().unwrap();
            managed_hooks(dir.path(), hook, 0o755);
            let r = check_hooks(&hooks_opts(dir.path()));
            let Status::Note(remedy) = &r.status else {
                panic!("{label}: {:?} {}", r.status, r.message);
            };
            assert!(
                r.message.contains("wiring not verified"),
                "{label}: {}",
                r.message
            );
            assert!(r.message.contains(".husky"), "{label}: {}", r.message);
            let text = remedy.to_string();
            assert!(text.contains("git commit --allow-empty"), "{label}: {text}");
            assert!(text.contains("git.commit_format"), "{label}: {text}");
        }
    }

    #[test]
    fn test_check_hooks_no_shims_skips_wiring_probe() {
        // A repo without scaffolded shims has nothing to wire — flag, never
        // guess.
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let r = check_hooks(&hooks_opts(dir.path()));
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_check_hooks_some_failing() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, args| {
            // The contract is current; fail just the git-guard handler.
            if args == ["git-hook", "capabilities"] {
                return Ok("hooks 3".into());
            }
            if args.len() >= 2 && args[1] == "git-guard" {
                Err("unknown subcommand".into())
            } else {
                Ok("ok".into())
            }
        });
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not responding"));
        assert!(result.message.contains("hook git-guard"));
    }

    #[test]
    fn test_check_network_pass() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "host" {
                Ok("/usr/bin/host".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, _args| Ok("github.com has address".into()));
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_network_probe_fails_warns_offline() {
        // `host` present but the probe errs → offline (or unreachable) warning.
        let mut opts = test_opts(); // exec_command returns Err
        opts.look_path = Some(|name| {
            if name == "host" {
                Ok("/usr/bin/host".into())
            } else {
                Err("not found".into())
            }
        });
        let result = check_network(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("offline"));
    }

    #[test]
    fn test_check_network_host_absent_skips_not_offline() {
        // `host` not installed must NOT be reported as offline — it is a skipped
        // probe. test_opts()'s look_path errs for every name, including `host`.
        let opts = test_opts();
        let result = check_network(&opts);
        assert!(result.status.is_warn());
        assert!(result.message.contains("not found"));
        assert!(result.message.contains("skipping"));
        assert!(!result.message.contains("offline"));
    }

    #[test]
    fn test_check_permissions_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[cfg(unix)]
    #[test]
    fn test_check_permissions_readonly_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);

        // Restore so tempdir cleanup succeeds.
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not writable"));
    }

    fn in_sandbox(name: &str) -> Option<String> {
        (name == "SANDBOX_RUNTIME").then(|| "1".to_string())
    }

    /// TSK-216 AC-3: the permissions check names each full-gate directory
    /// this process cannot write, and says when it runs in the sandbox.
    #[test]
    fn test_check_permissions_names_unwritable_gate_directories() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".codeflow")).unwrap();
        git2::Repository::init(&project).unwrap();
        let mut opts = test_opts();
        opts.project_dir = project.to_string_lossy().to_string();

        opts.codeflow_home = Some(dir.path().join("home"));
        let result = check_permissions(&opts);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(result.message.contains("writable"), "{}", result.message);
        assert!(!result.message.contains("sandbox"), "{}", result.message);

        let blocked = dir.path().join("not-a-dir");
        std::fs::write(&blocked, "file").unwrap();
        opts.codeflow_home = Some(blocked.clone());
        opts.env_var = Some(in_sandbox);
        let result = check_permissions(&opts);
        let Status::Warn(remedy) = &result.status else {
            panic!("{:?} {}", result.status, result.message);
        };
        for dir in [blocked.join("locks"), blocked.join("gate-runs")] {
            let shown = dir.display().to_string();
            assert!(result.message.contains(&shown), "{}", result.message);
            assert!(remedy.to_string().contains(&shown), "{remedy}");
        }
        assert!(result.message.contains(SANDBOX_NOTE), "{}", result.message);
        assert!(remedy.to_string().contains("allowWrite"), "{remedy}");
    }

    /// TSK-216 AC-3: in the sandbox the network probe is an HTTPS request,
    /// which its proxy carries, never a direct DNS lookup, and it passes
    /// only on an HTTP 2xx or 3xx answer.
    #[test]
    fn test_check_network_in_the_sandbox_probes_https() {
        let mut opts = test_opts();
        opts.env_var = Some(in_sandbox);
        opts.look_path = Some(|name| Ok(format!("/usr/bin/{name}")));
        opts.exec_command = Some(|cmd, args| {
            if cmd == "curl" && args.contains(&"https://github.com") {
                Ok("200".into())
            } else {
                Err("no direct DNS in the sandbox".into())
            }
        });
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(result.message.contains("HTTP 200"), "{}", result.message);
        assert!(result.message.contains(SANDBOX_NOTE), "{}", result.message);

        // A proxy refusal still exits 0 and is not connectivity.
        opts.exec_command = Some(|_, _| Ok("403".into()));
        let result = check_network(&opts);
        assert!(result.status.is_warn(), "{}", result.message);
        assert!(result.message.contains("HTTP 403"), "{}", result.message);
        assert!(!result.message.contains("OK"), "{}", result.message);

        // No response at all.
        opts.exec_command = Some(|_, _| Ok("000".into()));
        let result = check_network(&opts);
        assert!(result.status.is_warn(), "{}", result.message);
        assert!(
            result.message.contains("no HTTP status"),
            "{}",
            result.message
        );

        // A transport failure quotes curl's error.
        opts.exec_command = Some(|_, _| Err("curl: (56) CONNECT tunnel failed\n".into()));
        let result = check_network(&opts);
        assert!(result.status.is_warn());
        assert!(
            result.message.contains("CONNECT tunnel failed"),
            "{}",
            result.message
        );
        assert!(result.message.contains(SANDBOX_NOTE), "{}", result.message);
    }

    /// Delegate prerequisites present, with `codex login status` answering
    /// `login`.
    fn delegates_with_login(
        login: fn(&str, &[&str]) -> Result<String, String>,
        sandbox: bool,
    ) -> CheckResult {
        let mut opts = test_opts();
        if sandbox {
            opts.env_var = Some(in_sandbox);
        } else {
            opts.env_var = Some(|_| None);
        }
        opts.look_path = Some(|name| match name {
            "codex" | "claude" | "tmux" => Ok(format!("/usr/local/bin/{name}")),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(login);
        check_delegates(&opts)
    }

    fn access_denied(_: &str, args: &[&str]) -> Result<String, String> {
        match args {
            ["login", "status"] => Err("Error: Permission denied (os error 1)\n".into()),
            ["plugin", "list", "--json"] => {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            }
            _ => Ok("ready".into()),
        }
    }

    fn signed_out(_: &str, args: &[&str]) -> Result<String, String> {
        match args {
            ["login", "status"] => Err("Not logged in\n".into()),
            ["plugin", "list", "--json"] => {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            }
            _ => Ok("ready".into()),
        }
    }

    /// TSK-216 AC-3: in the sandbox a failed `codex login status` that does
    /// not say it is signed out leaves the sign-in unconfirmed and quotes the
    /// failure; an explicit "Not logged in" is a sign-in to fix everywhere.
    #[test]
    fn test_check_delegates_in_the_sandbox_reports_auth_unreadable() {
        let result = delegates_with_login(access_denied, true);
        let Status::Note(remedy) = &result.status else {
            panic!("{:?} {}", result.status, result.message);
        };
        assert!(result.message.contains("unconfirmed"), "{}", result.message);
        assert!(
            result.message.contains("Permission denied"),
            "{}",
            result.message
        );
        assert!(
            !result.message.contains("`codex login`"),
            "{}",
            result.message
        );
        assert!(result.message.contains(SANDBOX_NOTE), "{}", result.message);
        assert!(
            remedy.to_string().contains("outside the sandbox"),
            "{remedy}"
        );

        // Signed out, said so: a sign-in to fix, sandbox or not.
        for sandbox in [true, false] {
            let result = delegates_with_login(signed_out, sandbox);
            assert!(
                result.status.is_warn(),
                "{:?} {}",
                result.status,
                result.message
            );
            assert!(
                result.message.contains("`codex login`"),
                "{}",
                result.message
            );
        }

        // Outside the sandbox any failure is a sign-in to fix.
        let result = delegates_with_login(access_denied, false);
        assert!(result.status.is_warn());
        assert!(
            result.message.contains("`codex login`"),
            "{}",
            result.message
        );
    }

    #[test]
    fn test_check_delegates_bidirectional_prerequisites_pass() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "claude" => Ok("/usr/local/bin/claude".into()),
            "tmux" => Ok("/usr/local/bin/tmux".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, args| {
            if args == ["plugin", "list", "--json"] {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            } else {
                Ok("ready".into())
            }
        });
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(
            result.message.contains("bidirectional prerequisites"),
            "got: {}",
            result.message
        );
        assert!(result.message.contains("interactive canaries"));
        assert!(
            result.message.contains("Grok-hosted"),
            "must not claim Grok-hosted lanes complete: {}",
            result.message
        );
    }

    #[test]
    fn test_check_delegates_all_tools_missing_warns_not_fails() {
        // Optional capability: absence is explicit but never a doctor failure.
        let opts = test_opts();
        let result = check_delegates(&opts);
        assert!(result.status.is_warn());
        assert!(
            result.message.contains("codex missing"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("claude missing"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("tmux missing"),
            "got: {}",
            result.message
        );
    }

    #[test]
    fn test_check_delegates_reports_auth_mcp_and_plugin_gaps() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "claude" => Ok("/usr/local/bin/claude".into()),
            "tmux" => Ok("/usr/local/bin/tmux".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|cmd, args| {
            if cmd.contains("claude") && args == ["plugin", "list", "--json"] {
                Ok(r#"[{"id":"codex@openai-codex","enabled":false}]"#.into())
            } else {
                Err("not ready".into())
            }
        });
        let result = check_delegates(&opts);
        assert!(result.status.is_warn());
        assert!(
            result.message.contains("codex login"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("Codex MCP"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("Claude MCP"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("plugin not enabled"),
            "got: {}",
            result.message
        );
        assert!(
            result.message.contains("interactive TTY"),
            "got: {}",
            result.message
        );
    }

    /// TSK-147 round 3 F5: a gap this machine closes gets the local
    /// remedy; only a sign-in, alone, gets the operator's account remedy.
    #[test]
    fn delegate_gaps_split_local_repair_from_the_sign_in() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "claude" => Ok("/usr/local/bin/claude".into()),
            "tmux" => Ok("/usr/local/bin/tmux".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, args| match args {
            ["login", "status"] => Err("signed out".into()),
            ["plugin", "list", "--json"] => {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            }
            _ => Ok("ready".into()),
        });
        let result = check_delegates(&opts);
        assert_eq!(
            result.status,
            Status::Warn(remedy::DOCTOR_DELEGATES_SIGN_IN.remedy()),
            "{}",
            result.message
        );
        assert!(result.message.contains("codex login"), "{}", result.message);

        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "claude" => Ok("/usr/local/bin/claude".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, args| match args {
            ["login", "status"] => Err("signed out".into()),
            ["plugin", "list", "--json"] => {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            }
            _ => Ok("ready".into()),
        });
        let result = check_delegates(&opts);
        assert_eq!(
            result.status,
            Status::Warn(remedy::DOCTOR_DELEGATES.remedy()),
            "{}",
            result.message
        );
        assert!(
            result.message.contains("tmux missing"),
            "{}",
            result.message
        );
    }

    #[test]
    fn test_codex_plugin_enabled_requires_exact_enabled_plugin() {
        assert!(codex_plugin_enabled(
            r#"[{"id":"codex@openai-codex","enabled":true}]"#
        ));
        assert!(!codex_plugin_enabled(
            r#"[{"id":"codex@openai-codex","enabled":false}]"#
        ));
        assert!(!codex_plugin_enabled(r#"[{"id":"other","enabled":true}]"#));
        assert!(!codex_plugin_enabled("not-json"));
    }

    #[test]
    fn test_check_delegates_agy_presence_is_informational() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "claude" => Ok("/usr/local/bin/claude".into()),
            "tmux" => Ok("/usr/local/bin/tmux".into()),
            "agy" => Ok("/usr/local/bin/agy".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, args| {
            if args == ["plugin", "list", "--json"] {
                Ok(r#"[{"id":"codex@openai-codex","enabled":true}]"#.into())
            } else {
                Ok("ready".into())
            }
        });
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(
            result.message.contains("agy present"),
            "got: {}",
            result.message
        );
    }

    #[test]
    fn test_delegate_roundtrip_passes_all_binary_steps() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            (name == "codeflow")
                .then(|| "/usr/local/bin/codeflow".to_string())
                .ok_or_else(|| "not found".to_string())
        });
        opts.exec_command_stdin = Some(|_, _, _| Ok(String::new()));
        let result = check_delegate_roundtrip(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("round trip passed"));
    }

    #[test]
    fn test_delegate_roundtrip_is_fail_severity() {
        let mut opts = test_opts();
        opts.look_path = Some(|_| Ok("/usr/local/bin/codeflow".to_string()));
        opts.exec_command_stdin = Some(|_, args, _| {
            if args.contains(&"arm") {
                Err("synthetic arm failure".to_string())
            } else {
                Ok(String::new())
            }
        });
        let result = check_delegate_roundtrip(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("failed at arm"));
    }

    #[cfg(unix)]
    #[test]
    fn test_delegate_roundtrip_workspace_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("roundtrip");
        create_private_roundtrip_root(&root).unwrap();
        assert_eq!(
            std::fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn test_repo_integrity_bare_with_working_tree_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("true\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Fail);
        assert!(r.message.contains("core.bare"), "got: {}", r.message);
        assert!(
            r.message.contains("git config core.bare false"),
            "remedy: {}",
            r.message
        );
    }

    #[test]
    fn test_repo_integrity_protected_in_linked_worktree_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("false\n".into())
            } else if args.contains(&"worktree") {
                // Root on a feature branch; a LINKED worktree holds main.
                Ok("worktree /repo/root\nHEAD aaa\nbranch refs/heads/feat/x\n\nworktree /repo/wt\nHEAD bbb\nbranch refs/heads/main\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Fail);
        assert!(r.message.contains("main"), "got: {}", r.message);
        assert!(r.message.contains("worktree"), "got: {}", r.message);
    }

    #[test]
    fn test_repo_integrity_healthy_passes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        opts.exec_command = Some(|_, args| {
            if args.contains(&"--is-bare-repository") {
                Ok("false\n".into())
            } else if args.contains(&"worktree") {
                // Root holds main (allowed); the linked worktree is a feature.
                Ok("worktree /repo/root\nHEAD aaa\nbranch refs/heads/main\n\nworktree /repo/wt\nHEAD bbb\nbranch refs/heads/feat/x\n".into())
            } else {
                Ok(String::new())
            }
        });
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_repo_integrity_non_repo_passes_quietly() {
        // git unavailable / not a repo → no signal → pass, never guess.
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts(); // exec_command returns Err
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_repo_integrity(&opts);
        assert_eq!(r.status, Status::Pass);
    }

    // --- ci-perimeter -------------------------------------------------------

    /// The shipped CI templates, as a project copies them.
    const SHIPPED_GITHUB_CI: &str = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
    const SHIPPED_GITHUB_POLICY: &str =
        include_str!("../../../../assets/base/ci/codeflow-policy.yml");
    const SHIPPED_GITLAB: &str = include_str!("../../../../assets/base/ci/.gitlab-ci.yml");
    const SHIPPED_BITBUCKET: &str =
        include_str!("../../../../assets/base/ci/bitbucket-pipelines.yml");
    const SHIPPED_GENERIC: &str = include_str!("../../../../assets/base/ci/ci-generic.sh");

    fn write_ci(root: &Path, body: &str) {
        let ci = root.join(CI_DEFAULT_DEST);
        std::fs::create_dir_all(ci.parent().unwrap()).unwrap();
        std::fs::write(&ci, body).unwrap();
    }

    /// A project pinning `1.2.3` whose CI file at `dest` holds `body`.
    fn perimeter(dest: &str, body: &str) -> CheckResult {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(dest);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        std::fs::write(
            dir.path().join(".codeflow/project.toml"),
            "scaffold_version = \"1.2.3\"\n",
        )
        .unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        check_ci_perimeter(&opts)
    }

    /// The report for a CI file doctor cannot verify: no pin, version or
    /// checksum claim, and it says so (TSK-182).
    fn assert_unverified(r: &CheckResult) {
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
        assert!(r.message.contains("cannot verify"), "got: {}", r.message);
        for claim in ["CI installs", "pins", "1.2.3", "verified against"] {
            assert!(!r.message.contains(claim), "{claim}: {}", r.message);
        }
    }

    /// `text` with the first line holding `from` replaced by `to`.
    fn edit_line(text: &str, from: &str, to: &str) -> String {
        let at = text.find(from).expect("the shipped line");
        format!("{}{to}{}", &text[..at], &text[at + from.len()..])
    }

    #[test]
    fn test_ci_perimeter_placeholder_warns() {
        let dir = tempfile::tempdir().unwrap();
        write_ci(
            dir.path(),
            "steps:\n  - name: Install codeflow\n    run: echo \"::error::codeflow install step is an unwired PLACEHOLDER\"\n",
        );
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert!(r.status.is_warn());
        assert!(r.message.contains("not armed"), "got: {}", r.message);
        assert!(
            r.message.contains("codeflow-ci.yml"),
            "names the file: {}",
            r.message
        );
    }

    #[test]
    fn test_ci_perimeter_wired_passes() {
        let r = perimeter(CI_DEFAULT_DEST, SHIPPED_GITHUB_CI);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
        // The target-side pin, not the old placeholder story (TSK-095).
        assert!(
            r.message
                .contains("the codeflow version the target branch pins (1.2.3 here)"),
            "got: {}",
            r.message
        );
        assert!(r.message.contains("sha256.sum"), "got: {}", r.message);
    }

    /// TSK-182 AC-4: every shipped template, as copied, keeps the TSK-095
    /// pin report.
    #[test]
    fn test_ci_perimeter_recognizes_every_shipped_template() {
        for (dest, body) in [
            (CI_DEFAULT_DEST, SHIPPED_GITHUB_CI),
            (CI_DEFAULT_DEST, SHIPPED_GITHUB_POLICY),
            (".gitlab-ci.yml", SHIPPED_GITLAB),
            ("bitbucket-pipelines.yml", SHIPPED_BITBUCKET),
            (".gitlab-ci.yml", SHIPPED_GENERIC),
        ] {
            let r = perimeter(dest, body);
            assert_eq!(r.status, Status::Pass, "{dest}: {}", r.message);
            assert!(
                r.message
                    .contains("the codeflow version the target branch pins (1.2.3 here)"),
                "{dest}: {}",
                r.message
            );
        }
    }

    #[test]
    fn test_ci_perimeter_warns_when_nothing_is_pinned() {
        let dir = tempfile::tempdir().unwrap();
        write_ci(dir.path(), SHIPPED_GITHUB_CI);
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(
            r.message.contains("no scaffold_version is pinned"),
            "got: {}",
            r.message
        );
    }

    #[test]
    fn test_ci_perimeter_finds_a_copied_in_platform_file() {
        let r = perimeter("bitbucket-pipelines.yml", SHIPPED_BITBUCKET);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
        assert!(
            r.message.starts_with("bitbucket-pipelines.yml"),
            "got: {}",
            r.message
        );
    }

    #[test]
    fn test_ci_perimeter_does_not_claim_the_pin_for_an_own_install() {
        let r = perimeter(
            CI_DEFAULT_DEST,
            "steps:\n  - name: Build codeflow from source\n    run: cargo install --path crates/codeflow-cli\n",
        );
        assert_unverified(&r);
    }

    /// TSK-182 AC-1: the TSK-095 review probe's workflow, which names
    /// `scaffold_version` only in a comment and installs nothing.
    #[test]
    fn test_ci_perimeter_does_not_claim_the_pin_from_a_comment() {
        let r = perimeter(
            CI_DEFAULT_DEST,
            "# scaffold_version is not used here\nname: external\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo no codeflow installed\n",
        );
        assert_unverified(&r);
    }

    /// TSK-182 AC-2: the review probe's workflow, which reads the pin from
    /// the head's own state, and the shipped install pointed at the head.
    #[test]
    fn test_ci_perimeter_does_not_claim_the_pin_for_a_head_read() {
        let r = perimeter(
            CI_DEFAULT_DEST,
            "name: custom\non: push\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: |\n          version=$(sed -n /scaffold_version/p .codeflow/project.toml)\n          echo \"$version\"\n",
        );
        assert_unverified(&r);
        let head = edit_line(
            SHIPPED_GITHUB_CI,
            "PIN_REF: ${{ github.event.pull_request.base.sha || github.sha }}",
            "PIN_REF: ${{ github.event.pull_request.head.sha || github.sha }}",
        );
        assert_unverified(&perimeter(CI_DEFAULT_DEST, &head));
        let head = edit_line(
            SHIPPED_GITLAB,
            "BASE=\"${CI_MERGE_REQUEST_TARGET_BRANCH_SHA:-}\"",
            "BASE=\"$CI_COMMIT_SHA\"",
        );
        assert_unverified(&perimeter(".gitlab-ci.yml", &head));
    }

    /// TSK-182 AC-3: a pinned install whose checksum comparison is removed.
    #[test]
    fn test_ci_perimeter_does_not_claim_an_unchecked_install() {
        let check = "          if [ \"$actual\" != \"$expected\" ]; then\n            echo \"::error::checksum mismatch for ${asset} (codeflow ${version}): expected ${expected}, got ${actual}; refusing it\"; exit 1\n          fi\n";
        assert!(SHIPPED_GITHUB_CI.contains(check));
        let unchecked = SHIPPED_GITHUB_CI.replace(check, "");
        assert_unverified(&perimeter(CI_DEFAULT_DEST, &unchecked));
    }

    /// TSK-182 AC-5: one edited line of the shared pinned run, or of the
    /// GitHub install, leaves the file unrecognized.
    #[test]
    fn test_ci_perimeter_does_not_claim_an_edited_template() {
        let edited = edit_line(
            SHIPPED_BITBUCKET,
            "codeflow_fail() { echo \"codeflow: error: $*\" >&2; exit 1; }",
            "codeflow_fail() { echo \"codeflow: error: $*\" >&2; }",
        );
        assert_unverified(&perimeter("bitbucket-pipelines.yml", &edited));
        let edited = edit_line(
            SHIPPED_GITHUB_CI,
            "asset=codeflow-cli-x86_64-unknown-linux-gnu.tar.xz",
            "asset=codeflow-cli-x86_64-unknown-linux-musl.tar.xz",
        );
        assert_unverified(&perimeter(CI_DEFAULT_DEST, &edited));
    }

    #[test]
    fn test_ci_perimeter_warns_on_an_old_copy_in_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".gitlab-ci.yml"),
            "    # PLACEHOLDER: install the codeflow binary onto PATH, e.g.\n",
        )
        .unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(r.message.contains("not armed"), "got: {}", r.message);
    }

    #[test]
    fn test_ci_perimeter_missing_file_skips_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_ci_perimeter(&opts);
        assert_eq!(r.status, Status::Pass);
        assert!(
            r.message.contains("no codeflow CI workflow"),
            "got: {}",
            r.message
        );
    }

    // --- managed-drift ------------------------------------------------------

    /// A single managed-region record whose sha256 is the hash of `block`,
    /// exactly as `codeflow update` records it.
    fn record_region(root: &Path, dest: &str, src: &str, sha_of: &str) {
        use crate::scaffold::state::InstalledFile;
        let mut m = InstalledManifest::new("2.0.0");
        m.files.insert(
            dest.to_string(),
            InstalledFile {
                src: src.to_string(),
                ownership: Ownership::ManagedRegion,
                sha256: sha256_hex(sha_of.as_bytes()),
                exec: false,
            },
        );
        m.store(root).unwrap();
    }

    const REGION_BLOCK: &str =
        "<!-- codeflow:managed:begin scaffold=2.0.0 -->\nrules\n<!-- codeflow:managed:end -->";

    #[test]
    fn test_managed_drift_clean_when_block_matches_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("AGENTS.md"),
            format!("# Mine\n\n{REGION_BLOCK}\n\ntail\n"),
        )
        .unwrap();
        record_region(root, "AGENTS.md", "AGENTS.md.tmpl", REGION_BLOCK);

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
    }

    #[test]
    fn test_managed_drift_flags_in_marker_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // On disk the region has been hand-edited INSIDE the markers.
        let edited =
            "<!-- codeflow:managed:begin scaffold=2.0.0 -->\nrules HAND EDITED\n<!-- codeflow:managed:end -->";
        std::fs::write(root.join("AGENTS.md"), format!("# Mine\n\n{edited}\n")).unwrap();
        // The record still holds the pristine block's hash.
        record_region(root, "AGENTS.md", "AGENTS.md.tmpl", REGION_BLOCK);

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert!(r.status.is_warn());
        assert!(
            r.message.contains("AGENTS.md"),
            "names the drifted file: {}",
            r.message
        );
        assert!(
            r.message.contains("codeflow update"),
            "explains the risk: {}",
            r.message
        );
    }

    #[test]
    fn test_managed_drift_ignores_outside_marker_edits_and_json_regions() {
        use crate::scaffold::state::InstalledFile;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Edits OUTSIDE the markers are project-owned and must NOT flag.
        std::fs::write(
            root.join("AGENTS.md"),
            format!("# Heavily customized intro\n\n{REGION_BLOCK}\n\nlots of my own notes\n"),
        )
        .unwrap();
        // A JSON managed-region (settings): no text markers, and its recorded
        // hash is the shipped preset, never the on-disk block — must be skipped
        // even though the sha deliberately does not match.
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{\"hooks\": {}}\n").unwrap();

        let mut m = InstalledManifest::new("2.0.0");
        m.files.insert(
            "AGENTS.md".into(),
            InstalledFile {
                src: "AGENTS.md.tmpl".into(),
                ownership: Ownership::ManagedRegion,
                sha256: sha256_hex(REGION_BLOCK.as_bytes()),
                exec: false,
            },
        );
        m.files.insert(
            ".claude/settings.json".into(),
            InstalledFile {
                src: "settings/default.json".into(),
                ownership: Ownership::ManagedRegion,
                sha256: "deadbeef".into(),
                exec: false,
            },
        );
        m.store(root).unwrap();

        let mut opts = test_opts();
        opts.project_dir = root.to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(
            r.status,
            Status::Pass,
            "outside-marker/JSON must not flag: {}",
            r.message
        );
    }

    #[test]
    fn test_managed_drift_no_manifest_passes_quietly() {
        // No installed manifest → nothing to compare → pass, never guess.
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_managed_drift(&opts);
        assert_eq!(r.status, Status::Pass);
    }

    // --- test-config --------------------------------------------------------

    fn write_test_config(root: &Path, body: &str) {
        let cf = root.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("test-config.json"), body).unwrap();
    }

    #[test]
    fn test_check_test_config_absent_passes_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_test_config(&opts);
        assert_eq!(r.status, Status::Pass);
        assert!(
            r.message.contains("no .codeflow/test-config.json"),
            "got: {}",
            r.message
        );
    }

    #[test]
    fn test_check_test_config_present_healthy_passes() {
        let dir = tempfile::tempdir().unwrap();
        write_test_config(dir.path(), r#"{"schema_version":"1.0","targets":[]}"#);
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_test_config(&opts);
        assert_eq!(r.status, Status::Pass, "got: {}", r.message);
        assert!(r.message.contains("healthy"), "got: {}", r.message);
    }

    #[test]
    fn test_check_test_config_present_unhealthy_warns() {
        let dir = tempfile::tempdir().unwrap();
        // Loadable config, but the target's cwd does not exist → cwd-exists FAIL.
        // `custom` runner emits no probe, so the check stays hermetic.
        write_test_config(
            dir.path(),
            r#"{"schema_version":"1.0","targets":[{"name":"t","runner":"custom","cwd":"nope","modes":{"full":{"command":"echo hi"}}}]}"#,
        );
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let r = check_test_config(&opts);
        assert!(r.status.is_warn(), "got: {}", r.message);
        assert!(r.message.contains("failed"), "got: {}", r.message);
        assert!(
            r.message.contains("codeflow doctor --check test-config"),
            "points to detail: {}",
            r.message
        );
    }

    #[test]
    fn test_check_test_config_structural_block_warns_as_unenforced() {
        let dir = tempfile::tempdir().unwrap();
        write_test_config(
            dir.path(),
            r#"{"schema_version":"1.0","targets":[{"name":"t","runner":"custom","modes":{"full":{"command":"true"}},"structural":{"source_glob":["src/**/*.rs"]}}]}"#,
        );
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().into_owned();
        let result = check_test_config(&opts);
        assert!(result.status.is_warn(), "got: {}", result.message);
        assert!(result.message.contains("structural-unenforced"));
    }

    #[test]
    fn test_status_serde() {
        let json = serde_json::to_string(&Status::Pass).unwrap();
        assert_eq!(json, "\"pass\"");
        // A warning serializes with the step that clears it (R-80).
        let warn = Status::Warn(crate::remedy::DOCTOR_INIT.remedy());
        let json = serde_json::to_string(&warn).unwrap();
        assert_eq!(
            json,
            "{\"warn\":\"run `codeflow init` in the project root\"}"
        );
    }

    #[test]
    fn test_check_result_serialization() {
        let result = CheckResult {
            name: "test".into(),
            status: Status::Pass,
            message: "ok".into(),
            duration: Duration::from_millis(42),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"duration\":42"));
    }

    #[tokio::test]
    async fn test_run_all_returns_all_results() {
        let opts = test_opts();
        let results = run_all(&opts).await;
        assert_eq!(results.len(), CHECK_NAMES.len());
    }

    #[test]
    fn test_run_check_valid_name() {
        let opts = test_opts();
        let result = run_check("claude", &opts).unwrap();
        assert_eq!(result.name, "claude");
    }
}
