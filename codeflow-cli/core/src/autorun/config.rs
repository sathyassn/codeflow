//! Parallel work configuration loading and validation.
//!
//! Reads `.codeflow/config/parallel-work/parallel-work-config.json` and provides
//! typed access to all parallel execution settings. All fields have defaults via
//! `#[serde(default)]` so the config file is optional.

use std::path::Path;

use serde::Deserialize;

use crate::error::AutorunError;

/// Default maximum concurrent worktrees when config is absent.
pub const DEFAULT_MAX_CONCURRENT: usize = 5;

/// Grace period (seconds) -- skip liveness check for recently created worktrees.
pub const WORKTREE_INIT_GRACE_SECS: u64 = 60;

/// Relative path to the config file from the project root.
const CONFIG_PATH: &str = ".codeflow/config/parallel-work/parallel-work-config.json";

/// Relative path to the local config override file (gitignored).
const LOCAL_CONFIG_PATH: &str = ".codeflow/config/parallel-work/parallel-work-config.local.json";

/// Top-level parallel work configuration with 5 sections.
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
    /// Autorun worker settings.
    pub autorun: AutorunConfig,
}

/// Autorun worker execution settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AutorunConfig {
    /// Worker timeout in seconds (default: 3600 = 60 minutes).
    pub worker_timeout_secs: u64,
    /// Behavior when a task is blocked: "skip_and_continue" or "fail".
    pub blocked_behavior: String,
    /// Directory for autorun reports (relative to project root).
    pub report_dir: String,
}

impl Default for AutorunConfig {
    fn default() -> Self {
        Self {
            worker_timeout_secs: 3600,
            blocked_behavior: "skip_and_continue".to_string(),
            report_dir: ".state/autorun/reports".to_string(),
        }
    }
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

/// Default shared files to symlink into worktrees.
fn default_shared_files() -> Vec<String> {
    vec![
        ".claude/settings.local.json".to_string(),
        ".codeflow/config/parallel-work/parallel-work-config.local.json".to_string(),
    ]
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
    /// Individual files from the main repo to symlink into worktrees.
    /// These are non-git-tracked files (local overrides) that need to be
    /// visible across worktrees. Paths are relative to project root.
    #[serde(default = "default_shared_files")]
    pub shared_files: Vec<String>,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            mode: WorktreeMode::Autorun,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            base_dir: crate::worktree::DEFAULT_BASE_DIR.to_string(),
            shared_files: default_shared_files(),
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
            interval_secs: crate::coordination::sync::DEFAULT_SYNC_INTERVAL_SECS,
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
/// Loading order (later overrides earlier):
/// 1. Defaults from `ParallelWorkConfig::default()`
/// 2. Project config from `parallel-work-config.json`
/// 3. Local config from `parallel-work-config.local.json` (gitignored)
/// 4. `CODEFLOW_WORKTREE_MODE` env var overrides `worktree.mode`
///
/// Returns default config if neither file exists. Validates constraints
/// after loading.
///
/// # Errors
///
/// Returns `AutorunError::InvalidBatch` if a file exists but cannot be parsed,
/// or if validation fails.
pub fn load_config(project_dir: &Path) -> Result<ParallelWorkConfig, AutorunError> {
    let env_mode = std::env::var("CODEFLOW_WORKTREE_MODE").ok();
    load_config_inner(project_dir, env_mode.as_deref())
}

/// Inner implementation that accepts an explicit mode override for testability.
///
/// Separating env var reading from the merge logic avoids race conditions
/// in parallel tests that set/unset `CODEFLOW_WORKTREE_MODE`.
fn load_config_inner(
    project_dir: &Path,
    worktree_mode_override: Option<&str>,
) -> Result<ParallelWorkConfig, AutorunError> {
    let config_path = project_dir.join(CONFIG_PATH);

    // Load project config as a JSON Value (or empty object).
    let mut merged = if config_path.exists() {
        let data = std::fs::read_to_string(&config_path).map_err(|e| {
            AutorunError::InvalidBatch(format!("reading {}: {e}", config_path.display()))
        })?;
        serde_json::from_str::<serde_json::Value>(&data).map_err(|e| {
            AutorunError::InvalidBatch(format!("parsing {}: {e}", config_path.display()))
        })?
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    // Deep-merge local config over project config.
    // In worktree mode, the local config file is symlinked into the worktree
    // by setup_shared_file_symlinks, so checking project_dir is sufficient.
    let local_config_path = project_dir.join(LOCAL_CONFIG_PATH);
    if local_config_path.exists() {
        let local_data = std::fs::read_to_string(&local_config_path).map_err(|e| {
            AutorunError::InvalidBatch(format!("reading {}: {e}", local_config_path.display()))
        })?;
        let local_val = serde_json::from_str::<serde_json::Value>(&local_data).map_err(|e| {
            AutorunError::InvalidBatch(format!("parsing {}: {e}", local_config_path.display()))
        })?;
        merge_json_values(&mut merged, &local_val);
    }

    // Apply worktree mode override (from CODEFLOW_WORKTREE_MODE env var).
    if let Some(mode) = worktree_mode_override {
        if let Some(obj) = merged.as_object_mut() {
            let wt = obj
                .entry("worktree")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let Some(wt_obj) = wt.as_object_mut() {
                wt_obj.insert(
                    "mode".to_string(),
                    serde_json::Value::String(mode.to_string()),
                );
            }
        }
    }

    // Deserialize the merged result.
    let config = serde_json::from_value::<ParallelWorkConfig>(merged)
        .map_err(|e| AutorunError::InvalidBatch(format!("deserializing merged config: {e}")))?;

    validate_config(&config)?;
    Ok(config)
}

/// Recursively merge `overlay` JSON values into `base`.
///
/// Objects are merged key-by-key (overlay keys overwrite base keys).
/// Non-object values in overlay replace the base entirely.
fn merge_json_values(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let base_entry = base_map
                    .entry(key.clone())
                    .or_insert(serde_json::Value::Null);
                merge_json_values(base_entry, overlay_val);
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
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
    if config.autorun.worker_timeout_secs < 60 {
        return Err(AutorunError::InvalidBatch(format!(
            "autorun.worker_timeout_secs must be >= 60, got {}",
            config.autorun.worker_timeout_secs
        )));
    }
    let valid_behaviors = ["skip_and_continue", "fail"];
    if !valid_behaviors.contains(&config.autorun.blocked_behavior.as_str()) {
        return Err(AutorunError::InvalidBatch(format!(
            "autorun.blocked_behavior must be one of {:?}, got '{}'",
            valid_behaviors, config.autorun.blocked_behavior
        )));
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
        assert_eq!(cfg.worktree.max_concurrent, 5);
        assert_eq!(cfg.worktree.base_dir, ".git-worktrees");
        assert_eq!(cfg.worktree.shared_files.len(), 2);
        assert!(
            cfg.worktree
                .shared_files
                .contains(&".claude/settings.local.json".to_string())
        );
        assert!(cfg.worktree.shared_files.contains(
            &".codeflow/config/parallel-work/parallel-work-config.local.json".to_string()
        ));
        assert_eq!(
            cfg.sync.interval_secs,
            crate::coordination::sync::DEFAULT_SYNC_INTERVAL_SECS
        );
        assert!(cfg.sync.auto_start);
        assert!(cfg.merge.auto_rebase);
        assert!(cfg.merge.queue_enabled);
        assert_eq!(cfg.merge.max_rebase_attempts, 3);
        assert_eq!(cfg.claims.default_scope_policy, "soft");
        assert_eq!(cfg.claims.ttl_secs, 4200);
        assert!(cfg.claims.capture_events);
        assert_eq!(cfg.autorun.worker_timeout_secs, 3600);
        assert_eq!(cfg.autorun.blocked_behavior, "skip_and_continue");
        assert_eq!(cfg.autorun.report_dir, ".state/autorun/reports");
    }

    // -- M5: Sync interval alignment test --

    #[test]
    fn test_sync_config_default_matches_constant() {
        let cfg = SyncConfig::default();
        assert_eq!(
            cfg.interval_secs,
            crate::coordination::sync::DEFAULT_SYNC_INTERVAL_SECS,
            "SyncConfig default should match DEFAULT_SYNC_INTERVAL_SECS constant"
        );
        assert_eq!(cfg.interval_secs, 5);
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
        assert_eq!(cfg.worktree.max_concurrent, 5);
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
            "worktree": { "mode": "autorun", "max_concurrent": 5, "base_dir": ".git-worktrees", "shared_files": [".claude/settings.local.json", ".codeflow/config/parallel-work/parallel-work-config.local.json"] },
            "sync": { "interval_secs": 5, "auto_start": true },
            "merge": { "auto_rebase": true, "queue_enabled": true, "max_rebase_attempts": 3 },
            "claims": { "default_scope_policy": "soft", "ttl_secs": 4200, "capture_events": true },
            "autorun": { "worker_timeout_secs": 3600, "blocked_behavior": "skip_and_continue", "report_dir": ".state/autorun/reports" }
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

    // -- AutorunConfig tests --

    #[test]
    fn autorun_config_default() {
        let cfg = AutorunConfig::default();
        assert_eq!(cfg.worker_timeout_secs, 3600);
        assert_eq!(cfg.blocked_behavior, "skip_and_continue");
        assert_eq!(cfg.report_dir, ".state/autorun/reports");
    }

    #[test]
    fn autorun_config_custom() {
        let json = r#"{ "autorun": { "worker_timeout_secs": 7200, "blocked_behavior": "fail", "report_dir": "custom/reports" } }"#;
        let cfg: ParallelWorkConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.autorun.worker_timeout_secs, 7200);
        assert_eq!(cfg.autorun.blocked_behavior, "fail");
        assert_eq!(cfg.autorun.report_dir, "custom/reports");
    }

    #[test]
    fn autorun_config_absent_uses_default() {
        let json = r"{}";
        let cfg: ParallelWorkConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.autorun.worker_timeout_secs, 3600);
        assert_eq!(cfg.autorun.blocked_behavior, "skip_and_continue");
        assert_eq!(cfg.autorun.report_dir, ".state/autorun/reports");
    }

    #[test]
    fn full_config_with_autorun() {
        let json = r#"{
            "worktree": { "mode": "autorun", "max_concurrent": 5 },
            "autorun": { "worker_timeout_secs": 1800, "blocked_behavior": "fail", "report_dir": "reports/autorun" }
        }"#;
        let cfg: ParallelWorkConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.autorun.worker_timeout_secs, 1800);
        assert_eq!(cfg.autorun.blocked_behavior, "fail");
        assert_eq!(cfg.autorun.report_dir, "reports/autorun");
        assert_eq!(cfg.worktree.max_concurrent, 5);
    }

    // -- Autorun validation tests --

    #[test]
    fn validate_blocked_behavior_invalid_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.autorun.blocked_behavior = "invalid_value".to_string();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("blocked_behavior"));
    }

    #[test]
    fn validate_blocked_behavior_valid_accepted() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.autorun.blocked_behavior = "skip_and_continue".to_string();
        assert!(validate_config(&cfg).is_ok());
        cfg.autorun.blocked_behavior = "fail".to_string();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_worker_timeout_too_low_rejected() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.autorun.worker_timeout_secs = 59;
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("worker_timeout_secs"));
    }

    #[test]
    fn validate_worker_timeout_boundary_accepted() {
        let mut cfg = ParallelWorkConfig::default();
        cfg.autorun.worker_timeout_secs = 60;
        assert!(validate_config(&cfg).is_ok());
    }

    // -- merge_json_values tests --

    #[test]
    fn merge_json_values_overwrites_scalar() {
        let mut base = serde_json::json!({"a": 1});
        let overlay = serde_json::json!({"a": 2});
        merge_json_values(&mut base, &overlay);
        assert_eq!(base["a"], 2);
    }

    #[test]
    fn merge_json_values_deep_merge_objects() {
        let mut base = serde_json::json!({"worktree": {"mode": "autorun", "max_concurrent": 5}});
        let overlay = serde_json::json!({"worktree": {"max_concurrent": 5}});
        merge_json_values(&mut base, &overlay);
        assert_eq!(base["worktree"]["mode"], "autorun"); // preserved
        assert_eq!(base["worktree"]["max_concurrent"], 5); // overwritten
    }

    #[test]
    fn merge_json_values_adds_new_keys() {
        let mut base = serde_json::json!({"a": 1});
        let overlay = serde_json::json!({"b": 2});
        merge_json_values(&mut base, &overlay);
        assert_eq!(base["a"], 1);
        assert_eq!(base["b"], 2);
    }

    // -- Local config override tests --

    #[test]
    fn load_config_local_overrides_project() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Project config: max_concurrent = 3
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "max_concurrent": 5 }, "claims": { "ttl_secs": 4200 } }"#,
        )
        .unwrap();

        // Local override: max_concurrent = 7
        std::fs::write(
            config_dir.join("parallel-work-config.local.json"),
            r#"{ "worktree": { "max_concurrent": 7 } }"#,
        )
        .unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.worktree.max_concurrent, 7); // local override
        assert_eq!(cfg.claims.ttl_secs, 4200); // project preserved
    }

    #[test]
    fn load_config_local_partial_override() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "mode": "autorun", "max_concurrent": 5 } }"#,
        )
        .unwrap();

        // Local override only changes mode, preserves max_concurrent
        std::fs::write(
            config_dir.join("parallel-work-config.local.json"),
            r#"{ "worktree": { "mode": "always" } }"#,
        )
        .unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.worktree.mode, WorktreeMode::Always); // overridden
        assert_eq!(cfg.worktree.max_concurrent, 5); // preserved from project
    }

    #[test]
    fn load_config_env_var_overrides_mode() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "mode": "autorun", "max_concurrent": 5 } }"#,
        )
        .unwrap();

        // Use load_config_inner to avoid env var race conditions.
        let cfg = load_config_inner(dir.path(), Some("disabled")).unwrap();

        assert_eq!(cfg.worktree.mode, WorktreeMode::Disabled);
        assert_eq!(cfg.worktree.max_concurrent, 5); // unchanged
    }

    #[test]
    fn load_config_env_var_precedence_over_local() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "mode": "autorun" } }"#,
        )
        .unwrap();

        std::fs::write(
            config_dir.join("parallel-work-config.local.json"),
            r#"{ "worktree": { "mode": "always" } }"#,
        )
        .unwrap();

        // Env override should win over local config.
        let cfg = load_config_inner(dir.path(), Some("disabled")).unwrap();

        assert_eq!(cfg.worktree.mode, WorktreeMode::Disabled);
    }

    #[test]
    fn load_config_no_local_file_works() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "max_concurrent": 4 } }"#,
        )
        .unwrap();

        // No local file — should work fine.
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.worktree.max_concurrent, 4);
    }
}
