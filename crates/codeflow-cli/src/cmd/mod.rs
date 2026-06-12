//! CLI subcommand implementations. `main.rs` stays a thin dispatcher;
//! everything with behavior lives here.

pub mod recall;
pub mod remote;

use codeflow_core::registry;

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
