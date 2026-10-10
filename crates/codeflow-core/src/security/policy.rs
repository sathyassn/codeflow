//! Security scanner policy.
//!
//! Configuration consumed by the security modules. The git-guard hook and
//! git-hook shims load this from `.codeflow/policy.json` (charter section 6.1
//! is the single source of truth for all enforcement planes); every field
//! has a safe default so the scanner works with no config present.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Policy values read by the security modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityPolicy {
    /// Protected branch names or glob patterns (e.g. `release/*`).
    pub protected_branches: Vec<String>,
    /// Protected file paths or glob patterns (exact paths or `**` globs).
    pub protected_paths: Vec<String>,
    /// Managed scratch folders that must not be deleted or renamed.
    /// Empty by default; populated per repo when scratch space is managed.
    pub managed_tmp_folders: Vec<String>,
    /// Regex patterns identifying git network commands. Empty list means
    /// the built-in defaults apply.
    pub git_network_patterns: Vec<String>,
    /// Regex patterns identifying GitHub CLI commands. Empty list means
    /// the built-in defaults apply.
    pub github_cli_patterns: Vec<String>,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::defaults()
    }
}

impl SecurityPolicy {
    /// Built-in defaults, aligned with the charter section 6.1 policy schema.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            protected_branches: vec!["main".into(), "master".into()],
            protected_paths: vec![
                ".claude/settings.json".into(),
                ".claude/settings.local.json".into(),
                ".codeflow/policy.json".into(),
                ".codeflow/manifest.json".into(),
                ".codeflow/.baseline/**".into(),
                ".github/workflows/**".into(),
                ".git/hooks/**".into(),
            ],
            managed_tmp_folders: Vec::new(),
            git_network_patterns: Vec::new(),
            github_cli_patterns: Vec::new(),
        }
    }

    /// Read security policy; genuine absence retains built-in defaults.
    /// # Errors
    /// Existing policy bytes cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        match crate::hooks::policy::optional_text(path)? {
            Some(data) => serde_json::from_str(&data)
                .map_err(|error| format!("cannot parse security policy: {error}")),
            None => Ok(Self::defaults()),
        }
    }

    /// Return the protected branch list (names and glob patterns).
    #[must_use]
    pub fn protected_branch_list(&self) -> Vec<String> {
        self.protected_branches.clone()
    }

    /// Return all protected paths.
    #[must_use]
    pub fn all_protected_paths(&self) -> Vec<String> {
        self.protected_paths.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_protect_main_and_master() {
        let p = SecurityPolicy::defaults();
        assert_eq!(p.protected_branches, vec!["main", "master"]);
    }

    #[test]
    fn test_defaults_protect_settings_and_policy_files() {
        let p = SecurityPolicy::defaults();
        let paths = p.all_protected_paths();
        assert!(paths.contains(&".claude/settings.json".to_string()));
        assert!(paths.contains(&".codeflow/policy.json".to_string()));
    }

    #[test]
    fn test_defaults_no_managed_tmp_folders() {
        let p = SecurityPolicy::defaults();
        assert!(p.managed_tmp_folders.is_empty());
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = SecurityPolicy::load(&dir.path().join("nope.json")).unwrap();
        assert_eq!(p.protected_branches, vec!["main", "master"]);
    }

    #[test]
    fn test_load_parses_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(
            &path,
            r#"{"protected_branches": ["main", "release/*"], "protected_paths": ["conf/**"]}"#,
        )
        .unwrap();
        let p = SecurityPolicy::load(&path).unwrap();
        assert_eq!(p.protected_branches, vec!["main", "release/*"]);
        assert_eq!(p.all_protected_paths(), vec!["conf/**"]);
    }

    #[test]
    fn test_load_malformed_json_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(SecurityPolicy::load(&path).is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        let p = SecurityPolicy::defaults();
        let json = serde_json::to_string(&p).unwrap();
        let back: SecurityPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protected_branches, p.protected_branches);
        assert_eq!(back.protected_paths, p.protected_paths);
    }
}
