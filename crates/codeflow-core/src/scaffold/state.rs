//! Consumer-project state: `.codeflow/project.toml` (user-owned,
//! schema-versioned) and `.codeflow/manifest.json` (the installed-file record
//! that makes `codeflow update` possible), plus the `.codeflow/.baseline/`
//! pristine-copy store used as the 3-way merge base.
//!
//! `project.toml` is user-owned: load/store round-trips through a
//! `toml::Table` so keys codeflow does not know about (e.g. a future
//! `orient.enabled = false`) survive every rewrite.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{Ownership, Tier};
use super::ScaffoldError;

pub const CODEFLOW_DIR: &str = ".codeflow";
pub const PROJECT_TOML: &str = ".codeflow/project.toml";
pub const INSTALLED_MANIFEST: &str = ".codeflow/manifest.json";
pub const BASELINE_DIR: &str = ".codeflow/.baseline";

/// How git hooks ended up wired at init.
pub const GIT_HOOKS_WIRED: &str = "wired";
pub const GIT_HOOKS_UNWIRED: &str = "unwired";

/// `.codeflow/project.toml` — the fields codeflow owns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub schema_version: u32,
    pub tier: Tier,
    pub scaffold_version: String,
    pub stack: String,
    pub areas: Vec<String>,
    /// Branch policy arming flag. Init writes `false`, makes the scaffold
    /// commit, then flips to `true` — the bootstrap grace that gives a brand
    /// new project its first hour with zero policy walls (charter AC #1).
    /// Git hooks and git-guard read this flag.
    pub policy_armed: bool,
    /// `"wired"` when codeflow set `core.hooksPath`; `"unwired"` when an
    /// existing hook manager (husky/lefthook/custom hooksPath) was detected
    /// and left alone — doctor surfaces this.
    pub git_hooks: String,
    pub permission_preset: String,
    #[serde(default)]
    pub product_one_liner: String,
}

impl ProjectState {
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(PROJECT_TOML)
    }

    #[must_use]
    pub fn exists(root: &Path) -> bool {
        Self::path(root).exists()
    }

    /// # Errors
    ///
    /// [`ScaffoldError::NotInitialized`] when the file is absent; otherwise
    /// IO/parse failures.
    pub fn load(root: &Path) -> Result<Self, ScaffoldError> {
        let path = Self::path(root);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ScaffoldError::NotInitialized
            } else {
                ScaffoldError::io(&path, e)
            }
        })?;
        toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })
    }

    /// Writes the state, preserving any keys in the existing file that this
    /// struct does not model (project.toml is user-owned).
    ///
    /// # Errors
    ///
    /// IO failures, or an existing file that is not valid TOML.
    pub fn store(&self, root: &Path) -> Result<(), ScaffoldError> {
        let path = Self::path(root);
        let ours = toml::Table::try_from(self).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        let merged = match std::fs::read_to_string(&path) {
            Ok(existing) => {
                let mut table: toml::Table =
                    toml::from_str(&existing).map_err(|e| ScaffoldError::InvalidState {
                        what: PROJECT_TOML.to_string(),
                        detail: e.to_string(),
                    })?;
                for (k, v) in ours {
                    table.insert(k, v);
                }
                table
            }
            Err(_) => ours,
        };
        let text = toml::to_string_pretty(&merged).map_err(|e| ScaffoldError::InvalidState {
            what: PROJECT_TOML.to_string(),
            detail: e.to_string(),
        })?;
        write_file(&path, text.as_bytes())
    }
}

/// One record in `.codeflow/manifest.json`.
///
/// `sha256` semantics by ownership class:
/// - `managed`: hash of the pristine shipped version (== the `.baseline/`
///   copy), NOT of the on-disk file when it carries user edits from a 3-way
///   merge — "file hash == recorded hash" means the user has not modified it;
/// - `managed-region` (markdown/hash): hash of the codeflow block only;
/// - `managed-region` (json): hash of the rendered shipped preset;
/// - `user-owned`: hash of the rendered shipped default (the user file is
///   never compared — only the defaults are diffed for new keys).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFile {
    pub src: String,
    pub ownership: Ownership,
    pub sha256: String,
    #[serde(default)]
    pub exec: bool,
}

/// `.codeflow/manifest.json` — what codeflow installed and at which version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledManifest {
    pub schema_version: u32,
    pub scaffold_version: String,
    /// dest path (project-root relative) → record.
    pub files: std::collections::BTreeMap<String, InstalledFile>,
}

impl InstalledManifest {
    #[must_use]
    pub fn new(scaffold_version: &str) -> Self {
        Self {
            schema_version: 1,
            scaffold_version: scaffold_version.to_string(),
            files: std::collections::BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(INSTALLED_MANIFEST)
    }

    /// Loads the record, or an empty one when absent.
    ///
    /// # Errors
    ///
    /// IO failures other than not-found, or invalid JSON.
    pub fn load_or_default(root: &Path, scaffold_version: &str) -> Result<Self, ScaffoldError> {
        let path = Self::path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(ScaffoldError::from),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self::new(scaffold_version))
            }
            Err(e) => Err(ScaffoldError::io(&path, e)),
        }
    }

    /// # Errors
    ///
    /// IO or serialization failures.
    pub fn store(&self, root: &Path) -> Result<(), ScaffoldError> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_file(&Self::path(root), text.as_bytes())
    }
}

/// Baseline (pristine shipped copy) storage under `.codeflow/.baseline/<dest>`.
pub struct Baseline;

impl Baseline {
    #[must_use]
    pub fn path(root: &Path, dest: &str) -> PathBuf {
        root.join(BASELINE_DIR).join(dest)
    }

    #[must_use]
    pub fn read(root: &Path, dest: &str) -> Option<String> {
        std::fs::read_to_string(Self::path(root, dest)).ok()
    }

    /// # Errors
    ///
    /// IO failures.
    pub fn write(root: &Path, dest: &str, content: &str) -> Result<(), ScaffoldError> {
        write_file(&Self::path(root, dest), content.as_bytes())
    }
}

/// Creates parent directories and writes `bytes` to `path`.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ScaffoldError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ScaffoldError::io(parent, e))?;
    }
    std::fs::write(path, bytes).map_err(|e| ScaffoldError::io(path, e))
}

/// Sets (or clears) the executable bit.
pub(crate) fn set_exec(path: &Path, exec: bool) -> Result<(), ScaffoldError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| ScaffoldError::io(path, e))?;
        let mut perms = meta.permissions();
        let mode = if exec {
            perms.mode() | 0o111
        } else {
            perms.mode() & !0o111
        };
        perms.set_mode(mode);
        std::fs::set_permissions(path, perms).map_err(|e| ScaffoldError::io(path, e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, exec);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_state_round_trip_preserves_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let state = ProjectState {
            schema_version: 1,
            tier: Tier::Standard,
            scaffold_version: "2.0.0".to_string(),
            stack: "rust".to_string(),
            areas: vec!["core".to_string()],
            policy_armed: true,
            git_hooks: GIT_HOOKS_WIRED.to_string(),
            permission_preset: "default".to_string(),
            product_one_liner: "demo".to_string(),
        };
        state.store(root).unwrap();

        // User adds a key codeflow does not model.
        let path = ProjectState::path(root);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\n[orient]\nenabled = false\n");
        std::fs::write(&path, text).unwrap();

        let mut loaded = ProjectState::load(root).unwrap();
        loaded.policy_armed = false;
        loaded.store(root).unwrap();

        let final_text = std::fs::read_to_string(&path).unwrap();
        assert!(final_text.contains("[orient]"), "foreign key survived");
        assert!(final_text.contains("policy_armed = false"));
    }
}
