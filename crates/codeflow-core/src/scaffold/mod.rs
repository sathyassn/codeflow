//! The scaffold engine — `codeflow init` / `codeflow update` (charter §4, §10, D22).
//!
//! This is the trust-critical core of the product: it writes files into a
//! user's repository and must never destroy their work. Three invariants hold
//! everywhere in this module:
//!
//! 1. **Never clobber.** An existing file is only overwritten when codeflow
//!    can prove it is unmodified (hash matches the installed record) or the
//!    user passed `--force`. Conflicting updates land next to the file as
//!    `<path>.new`, never on top of it.
//! 2. **Never silent.** Every file the engine touched, skipped, merged, or
//!    refused to touch appears in the printed report.
//! 3. **Ownership classes drive behavior** (charter §4.3):
//!    fully-managed (hash + baseline + 3-way merge), managed-region (marked
//!    block / known JSON keys only), user-owned (write once; additive new-key
//!    sync for schema-versioned JSON).
//!
//! Assets are resolved through [`assets::AssetSource`] so the engine is
//! independent of how they are shipped: the CLI provides a rust-embed source
//! (disk in debug builds, embedded in release); tests provide a directory
//! source over fixtures.

pub mod assets;
pub mod detect;
pub mod init;
pub mod json_edit;
pub mod manifest;
pub mod portal;
pub mod pr_template;
pub mod prior_release;
pub mod region;
pub mod report;
pub mod rule_map;
pub mod settings_merge;
pub mod state;
pub mod template;
pub mod update;

mod gitutil;
mod hash;
pub(crate) mod version;

/// The starter ADR is useful only when `CodeFlow` is creating a project's first
/// decision record. Brownfield repositories already have an architecture and
/// an ADR sequence; adding another `ADR-0001` would make the doc graph invalid
/// and invent a decision after the fact.
const INITIAL_STACK_ADR: &str = "docs/decisions/ADR-0001-stack-choice.md";

fn should_skip_initial_stack_adr(
    root: &std::path::Path,
    dest: &str,
) -> Result<bool, ScaffoldError> {
    if dest != INITIAL_STACK_ADR || root.join(dest).exists() {
        return Ok(false);
    }

    let decisions = root.join("docs/decisions");
    let entries = match std::fs::read_dir(&decisions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ScaffoldError::io(&decisions, error)),
    };

    for entry in entries {
        let entry = entry.map_err(|error| ScaffoldError::io(&decisions, error))?;
        if !entry
            .file_type()
            .map_err(|error| ScaffoldError::io(entry.path(), error))?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("ADR-") && name.ends_with(".md") {
            return Ok(true);
        }
    }

    Ok(false)
}

pub use assets::{AssetSource, DirSource};
pub use hash::sha256_hex;
pub use init::{init, InitAnswers, InitOptions};
pub use manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest, Tier};
pub use report::{Action, FileReport, Report};
pub use update::{update, UpdateOptions};
pub use version::version_skew_warning;

/// Errors produced by the scaffold engine.
#[derive(Debug, thiserror::Error)]
pub enum ScaffoldError {
    #[error("io error on {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("scaffold manifest missing from assets ({0})")]
    ManifestMissing(String),
    #[error("scaffold manifest invalid: {0}")]
    ManifestInvalid(String),
    #[error("refusing to follow a symlink at {path} — scaffold IO stays beneath the repo root")]
    UnsafeSymlink { path: std::path::PathBuf },
    #[error("not a codeflow project (no .codeflow/project.toml) — run `codeflow init`")]
    NotInitialized,
    #[error("invalid {what}: {detail}")]
    InvalidState { what: String, detail: String },
    #[error("git: {0}")]
    Git(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl ScaffoldError {
    fn io(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
