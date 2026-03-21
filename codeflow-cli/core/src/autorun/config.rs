//! Parallel work configuration loading and validation.
//!
//! Reads `.codeflow/config/parallel-work/parallel-work-config.json` and provides
//! typed access to all parallel execution settings. All fields have defaults via
//! `#[serde(default)]` so the config file is optional.

use std::path::Path;

use serde::Deserialize;

use crate::error::AutorunError;

/// Relative path to the config file from the project root.
const CONFIG_PATH: &str = ".codeflow/config/parallel-work/parallel-work-config.json";

/// Top-level parallel work configuration with 4 sections.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ParallelWorkConfig {
    /// Worktree creation and lifecycle settings.
    pub worktree: WorktreeConfig,
    /// CRDT sync daemon settings.
    pub sync: SyncConfig,
    /// PR merge conflict handling settings.
    pub merge: MergeConfig,
    /// Loro CRDT claim system settings.
    pub claims: ClaimsConfig,
}

/// Worktree creation mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeMode {
    /// Create worktrees only for autorun sessions (default).
    Autorun,
    /// Create worktrees for every session including interactive.
    Always,
    /// Never create worktrees.
    Disabled,
}

impl<'de> Deserialize<'de> for WorktreeMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "autorun" => Ok(Self::Autorun),
            "always" => Ok(Self::Always),
            "disabled" => Ok(Self::Disabled),
            other => Err(serde::de::Error::custom(format!(
                "invalid worktree mode '{other}': expected autorun, always, or disabled"
            ))),
        }
    }
}

/// Worktree creation and lifecycle settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorktreeConfig {
    /// When to create worktrees: "autorun", "always", or "disabled".
    pub mode: WorktreeMode,
    /// Maximum concurrent worktrees.
    pub max_concurrent: usize,
    /// Directory for worktree creation (relative to project root).
    pub base_dir: String,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            mode: WorktreeMode::Autorun,
            max_concurrent: 3,
            base_dir: ".git-worktrees".to_string(),
        }
    }
}

/// CRDT sync daemon settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SyncConfig {
    /// Sync interval in seconds.
    pub interval_secs: u64,
    /// Auto-start daemon when more than one worktree is active.
    pub auto_start: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            auto_start: true,
        }
    }
}

/// PR merge conflict handling settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeConfig {
    /// Attempt git rebase on merge conflicts before PR creation.
    pub auto_rebase: bool,
    /// Use Loro-backed FIFO merge queue for PR ordering.
    pub queue_enabled: bool,
    /// Maximum rebase retries before giving up.
    pub max_rebase_attempts: usize,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            auto_rebase: true,
            queue_enabled: true,
            max_rebase_attempts: 3,
        }
    }
}

/// Loro CRDT claim system settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ClaimsConfig {
    /// Default scope_policy for tasks that do not specify one.
    pub default_scope_policy: String,
    /// Claim TTL in seconds (safety-net fallback).
    pub ttl_secs: u64,
    /// Persist coordination events to coordination-events.jsonl.
    pub capture_events: bool,
}

impl Default for ClaimsConfig {
    fn default() -> Self {
        Self {
            default_scope_policy: "soft".to_string(),
            ttl_secs: 4200,
            capture_events: true,
        }
    }
}

/// Load parallel work config from the project directory.
///
/// Returns default config if the file does not exist. Validates constraints
/// after loading.
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` if the file exists but cannot be parsed,
/// or if validation fails.
pub fn load_config(project_dir: &Path) -> Result<ParallelWorkConfig, AutorunError> {
    let config_path = project_dir.join(CONFIG_PATH);

    let config = if config_path.exists() {
        let data = std::fs::read_to_string(&config_path).map_err(|e| {
            AutorunError::InvalidBatch(format!("reading {}: {e}", config_path.display()))
        })?;
        serde_json::from_str::<ParallelWorkConfig>(&data).map_err(|e| {
            AutorunError::InvalidBatch(format!("parsing {}: {e}", config_path.display()))
        })?
    } else {
        ParallelWorkConfig::default()
    };

    validate_config(&config)?;
    Ok(config)
}

/// Validate config constraints.
fn validate_config(config: &ParallelWorkConfig) -> Result<(), AutorunError> {
    if config.worktree.max_concurrent < 1 || config.worktree.max_concurrent > 10 {
        return Err(AutorunError::InvalidBatch(format!(
            "worktree.max_concurrent must be 1..=10, got {}",
            config.worktree.max_concurrent
        )));
    }
    if config.claims.ttl_secs < 60 {
        return Err(AutorunError::InvalidBatch(format!(
            "claims.ttl_secs must be >= 60, got {}",
            config.claims.ttl_secs
        )));
    }
    if config.merge.max_rebase_attempts < 1 {
        return Err(AutorunError::InvalidBatch(
            "merge.max_rebase_attempts must be >= 1".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Default tests --

    #[test]
    fn default_config_has_correct_values() {
        let cfg = ParallelWorkConfig::default();
        assert_eq!(cfg.worktree.mode, WorktreeMode::Autorun);
        assert_eq!(cfg.worktree.max_concurrent, 3);
        assert_eq!(cfg.worktree.base_dir, ".git-worktrees");
        assert_eq!(cfg.sync.interval_secs, 30);
        assert!(cfg.sync.auto_start);
        assert!(cfg.merge.auto_rebase);
        assert!(cfg.merge.queue_enabled);
        assert_eq!(cfg.merge.max_rebase_attempts, 3);
        assert_eq!(cfg.claims.default_scope_policy, "soft");
        assert_eq!(cfg.claims.ttl_secs, 4200);
        assert!(cfg.claims.capture_events);
    }

    // -- Config loading tests --

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg, ParallelWorkConfig::default());
    }

    #[test]
    fn load_config_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{
                "worktree": { "mode": "always", "max_concurrent": 5 },
                "claims": { "ttl_secs": 7200 }
            }"#,
        )
        .unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.worktree.mode, WorktreeMode::Always);
        assert_eq!(cfg.worktree.max_concurrent, 5);
        assert_eq!(cfg.worktree.base_dir, ".git-worktrees"); // default
        assert_eq!(cfg.claims.ttl_secs, 7200);
        assert_eq!(cfg.claims.default_scope_policy, "soft"); // default
    }

    #[test]
    fn load_config_partial_sections_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "sync": { "interval_secs": 10 } }"#,
        )
        .unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.sync.interval_secs, 10);
        // All other sections should be defaults.
        assert_eq!(cfg.worktree.max_concurrent, 3);
        assert_eq!(cfg.merge.max_rebase_attempts, 3);
        assert_eq!(cfg.claims.ttl_secs, 4200);
    }

    #[test]
    fn load_config_empty_json_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("parallel-work-config.json"), "{}").unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg, ParallelWorkConfig::default());
    }

    // -- Validation tests --

    #[test]
    fn validate_max_concurrent_zero_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.worktree.max_concurrent = 0;
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("max_concurrent"));
    }

    #[test]
    fn validate_max_concurrent_eleven_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.worktree.max_concurrent = 11;
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("max_concurrent"));
    }

    #[test]
    fn validate_max_concurrent_boundary_accepted() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.worktree.max_concurrent = 1;
        assert!(validate_config(&cfg).is_ok());
        cfg.worktree.max_concurrent = 10;
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_ttl_too_low_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.claims.ttl_secs = 59;
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("ttl_secs"));
    }

    #[test]
    fn validate_ttl_boundary_accepted() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.claims.ttl_secs = 60;
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_max_rebase_zero_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.merge.max_rebase_attempts = 0;
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("max_rebase_attempts"));
    }

    // -- Malformed config tests --

    #[test]
    fn load_config_malformed_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            "{{invalid json",
        )
        .unwrap();

        let err = load_config(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parsing"));
    }

    // -- WorktreeMode tests --

    #[test]
    fn worktree_mode_autorun() {
        let json = r#"{"mode": "autorun"}"#;
        let cfg: WorktreeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode, WorktreeMode::Autorun);
    }

    #[test]
    fn worktree_mode_always() {
        let json = r#"{"mode": "always"}"#;
        let cfg: WorktreeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode, WorktreeMode::Always);
    }

    #[test]
    fn worktree_mode_disabled() {
        let json = r#"{"mode": "disabled"}"#;
        let cfg: WorktreeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode, WorktreeMode::Disabled);
    }

    #[test]
    fn worktree_mode_invalid_rejected() {
        let json = r#"{"mode": "invalid"}"#;
        let result = serde_json::from_str::<WorktreeConfig>(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid"));
    }

    // -- Full config roundtrip --

    #[test]
    fn full_config_from_spec_example() {
        let json = r#"{
            "worktree": { "mode": "autorun", "max_concurrent": 3, "base_dir": ".git-worktrees" },
            "sync": { "interval_secs": 30, "auto_start": true },
            "merge": { "auto_rebase": true, "queue_enabled": true, "max_rebase_attempts": 3 },
            "claims": { "default_scope_policy": "soft", "ttl_secs": 4200, "capture_events": true }
        }"#;
        let cfg: ParallelWorkConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg, ParallelWorkConfig::default());
    }

    #[test]
    fn config_with_disabled_worktree_and_hard_policy() {
        let json = r#"{
            "worktree": { "mode": "disabled", "max_concurrent": 1 },
            "claims": { "default_scope_policy": "hard", "ttl_secs": 120 }
        }"#;
        let cfg: ParallelWorkConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.worktree.mode, WorktreeMode::Disabled);
        assert_eq!(cfg.worktree.max_concurrent, 1);
        assert_eq!(cfg.claims.default_scope_policy, "hard");
        assert_eq!(cfg.claims.ttl_secs, 120);
    }
}
