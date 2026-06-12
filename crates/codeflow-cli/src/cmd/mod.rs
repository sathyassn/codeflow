//! CLI subcommand handlers. Each module owns its arg types and run function
//! so `main.rs` stays a thin dispatcher (charter §3.1).

pub mod git_hook;
pub mod hook;
pub mod orient;

use std::path::PathBuf;

use codeflow_core::hooks::{any_blocking, PolicyLevel, Violation, INTEGRATE_TOKEN_ENV};

/// `true` when the `codeflow integrate` gate-context token is present in the
/// environment (charter §6.2 / D9).
#[must_use]
pub fn integrate_token_present() -> bool {
    std::env::var(INTEGRATE_TOKEN_ENV).is_ok_and(|v| !v.is_empty())
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
