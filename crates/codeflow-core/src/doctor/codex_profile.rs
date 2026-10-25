//! Whether the installed Codex accepts the builder seat's permission profile
//! as the managed launch selects it (TSK-1003).
//!
//! Codex 0.163.0 and later load config twice for a TUI seat: once at startup
//! with no project layer (the user config plus the `-c` overrides, from
//! openai/codex#51872), and once for the session with the project's
//! `.codex/config.toml`. A launch that selects a profile defined only in the
//! project file stops in the first load, before the seat opens. The probe
//! asks the installed Codex to load its config both ways with the builder
//! overrides, using `codex features list`, which reads config and prints
//! the feature table without a model, a session or the sandbox.

use std::path::Path;
use std::time::Duration;

/// The `-c` overrides a Codex builder seat launches with, in order. The
/// posture table in cf-model-orchestrator's `resources/routing/transport.md`
/// is their documented home; a test keeps the two in step. The description
/// override gives the startup load a profile named `cf-builder` that grants
/// nothing; the session load merges it into the project's table, whose
/// description is the same value.
pub const CODEX_BUILDER_OVERRIDES: [&str; 2] = [
    "permissions.cf-builder.description=\"CodeFlow builder seat\"",
    "default_permissions=\"cf-builder\"",
];

/// How long one Codex config load may take before it counts as failed.
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// How one Codex config load with the builder overrides ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexLoad {
    /// Codex loaded its config and exited 0.
    Accepted,
    /// Codex ran and refused the config, with what it printed.
    Rejected(String),
    /// Codex did not start or did not finish in time, so it said nothing
    /// about the profile.
    NotRun(String),
}

/// What the installed Codex said about the builder overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexBuilderProbe {
    /// `codex --version`, first line, when it answered.
    pub version: Option<String>,
    /// The startup load, with no project config.
    pub startup: CodexLoad,
    /// The session load, in the project folder.
    pub project: CodexLoad,
}

/// Run the probe with the `codex` at `codex`, from `root` (the project) and
/// from an empty temporary folder (no project config). Nothing is written to
/// the project; the temporary folder is removed afterwards.
#[must_use]
pub fn probe_codex_builder_profile(root: &Path, codex: &Path) -> CodexBuilderProbe {
    let Some(program) = codex.to_str() else {
        let reason = "the codex path is not valid UTF-8".to_string();
        return CodexBuilderProbe {
            version: None,
            startup: CodexLoad::NotRun(reason.clone()),
            project: CodexLoad::NotRun(reason),
        };
    };
    let version = super::run_captured(program, &["--version"], LOAD_TIMEOUT, Some(""), None)
        .ok()
        .filter(|run| run.code == Some(0))
        .and_then(|run| {
            run.stdout
                .lines()
                .next()
                .map(|line| line.trim().to_string())
        })
        .filter(|line| !line.is_empty());
    let startup = match tempfile::tempdir() {
        Ok(empty) => load(program, empty.path()),
        Err(error) => CodexLoad::NotRun(format!("no temporary folder: {error}")),
    };
    let project = load(program, root);
    CodexBuilderProbe {
        version,
        startup,
        project,
    }
}

/// One config load with the builder overrides, in `cwd`.
fn load(program: &str, cwd: &Path) -> CodexLoad {
    let mut args = vec!["features", "list"];
    for value in CODEX_BUILDER_OVERRIDES {
        args.push("-c");
        args.push(value);
    }
    let run = match super::run_captured(program, &args, LOAD_TIMEOUT, Some(""), Some(cwd)) {
        Ok(run) => run,
        Err(reason) => return CodexLoad::NotRun(reason),
    };
    if run.code == Some(0) {
        return CodexLoad::Accepted;
    }
    // Codex prints a config error as a headline and its causes on later
    // lines, so the non-empty lines are joined and bounded.
    let text = if run.stderr.trim().is_empty() {
        &run.stdout
    } else {
        &run.stderr
    };
    let reason: String = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
        .chars()
        .take(240)
        .collect();
    if reason.is_empty() {
        return CodexLoad::Rejected(match run.code {
            Some(code) => format!("exit code {code}"),
            None => "ended by a signal".to_string(),
        });
    }
    CodexLoad::Rejected(reason)
}
