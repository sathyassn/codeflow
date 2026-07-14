//! Environment detection: stack, existing git-hook managers, empty dirs.

use std::path::Path;

use super::gitutil;

/// Detected project stack, used for `{{STACK}}` and recorded in project.toml.
#[must_use]
pub fn detect_stack(root: &Path) -> &'static str {
    if root.join("Cargo.toml").exists() {
        "rust"
    } else if root.join("package.json").exists() {
        "node"
    } else if root.join("pyproject.toml").exists() {
        "python"
    } else {
        "unset"
    }
}

/// A pre-existing git-hook manager codeflow must not clobber (charter AC #2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookManager {
    Husky,
    Lefthook,
    /// `core.hooksPath` already points somewhere that is not ours.
    HooksPath(String),
}

impl std::fmt::Display for HookManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Husky => f.write_str("husky"),
            Self::Lefthook => f.write_str("lefthook"),
            Self::HooksPath(path) => write!(f, "core.hooksPath={path}"),
        }
    }
}

/// Path codeflow wires hooks into via `core.hooksPath`.
pub const CODEFLOW_HOOKS_PATH: &str = ".codeflow/git-hooks";

/// Detects an existing hook manager that owns this repo's hooks.
#[must_use]
pub fn detect_hook_manager(root: &Path) -> Option<HookManager> {
    if root.join(".husky").is_dir() {
        return Some(HookManager::Husky);
    }
    for f in [
        "lefthook.yml",
        ".lefthook.yml",
        "lefthook.toml",
        ".lefthook.toml",
    ] {
        if root.join(f).exists() {
            return Some(HookManager::Lefthook);
        }
    }
    if let Some(path) = gitutil::config_get(root, "core.hooksPath") {
        if path != CODEFLOW_HOOKS_PATH {
            return Some(HookManager::HooksPath(path));
        }
    }
    None
}

/// True when the directory contains nothing (ignoring nothing — a truly
/// empty dir, the canonical bootstrap-grace case).
#[must_use]
pub fn is_empty_dir(root: &Path) -> bool {
    std::fs::read_dir(root).map_or(true, |mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_detection() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_stack(dir.path()), "unset");
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_stack(dir.path()), "python");
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_stack(dir.path()), "node");
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_stack(dir.path()), "rust");
    }

    #[test]
    fn husky_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".husky")).unwrap();
        assert_eq!(detect_hook_manager(dir.path()), Some(HookManager::Husky));
    }

    #[test]
    fn empty_dir_detection() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_empty_dir(dir.path()));
        std::fs::write(dir.path().join("x"), "").unwrap();
        assert!(!is_empty_dir(dir.path()));
    }
}
