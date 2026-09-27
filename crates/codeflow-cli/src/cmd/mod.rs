//! CLI subcommand handlers. Each module owns its arg types and run function
//! so `main.rs` stays a thin dispatcher (charter §3.1).

pub mod ci;
pub mod delegate;
pub mod doctor;
pub mod estimate;
pub mod git_hook;
pub mod hook;
pub mod integrate;
pub mod new;
pub mod orient;
pub mod policy;
pub mod portal;
pub mod present;
mod push_set;
pub mod recall;
pub mod remote;
pub mod status;
pub mod test;
pub mod validate;
pub mod work;

use std::path::PathBuf;

use codeflow_core::hooks::{
    any_blocking, PolicyLevel, Violation, HUMAN_OVERRIDE_ENV, INTEGRATE_TOKEN_ENV,
};
use codeflow_core::registry;

/// `true` when the `codeflow integrate` gate-context token is present in the
/// environment (charter §6.2 / D9).
#[must_use]
pub fn integrate_token_present() -> bool {
    std::env::var(INTEGRATE_TOKEN_ENV).is_ok_and(|v| !v.is_empty())
}

/// `true` when a human's `CODEFLOW_HUMAN_OVERRIDE=1` is present (ADR-0007).
/// Honored only by the git-client hook plane; the git-guard never consults it
/// and blocks in-session attempts to set it.
#[must_use]
pub fn human_override_present() -> bool {
    std::env::var(HUMAN_OVERRIDE_ENV).is_ok_and(|v| v == "1")
}

/// Project root for hook evaluation: the repo containing `start`, or `start`
/// itself when not in a repository (policy then falls back to defaults).
#[must_use]
pub fn project_root(start: &std::path::Path) -> PathBuf {
    codeflow_core::hooks::RepoInfo::discover(start).map_or_else(|| start.to_path_buf(), |i| i.root)
}

/// Print violations and notes for one enforcement plane; return the exit
/// code (`block_code` when any violation is block-level, else 0).
#[must_use]
pub fn render_outcome(
    plane: &str,
    violations: &[Violation],
    notes: &[String],
    block_code: i32,
) -> i32 {
    for note in notes {
        eprintln!("codeflow {plane}: {note}");
    }
    for v in violations {
        eprintln!("{}", v.render(plane));
    }
    if any_blocking(violations) {
        block_code
    } else {
        if violations.iter().any(|v| v.level == PolicyLevel::Warn) {
            eprintln!("codeflow {plane}: warnings only — proceeding");
        }
        0
    }
}

/// Cheap per-command registry touch (charter §7 layer 3): when the current
/// working directory is inside an initialized codeflow repo, upsert its
/// entry in `~/.codeflow/registry.json`. Never blocks or fails the actual
/// command — registry maintenance is a side effect, not a gate.
pub fn touch_registry_best_effort() {
    let Some(home) = registry::codeflow_home() else {
        return;
    };
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if let Some(root) = registry::find_repo_root(&cwd) {
        if let Err(e) = registry::touch_registry(&home, &root) {
            eprintln!("warning: registry touch failed: {e}");
        }
    }
}

/// Walk up from the current directory to the nearest git repository root.
/// Falls back to the current directory when none is found (commands that
/// don't need git still work there).
pub(crate) fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.clone();
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return cwd,
        }
    }
}
