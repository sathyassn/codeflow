//! Operational activity logging for hook events.
//!
//! Provides date-rotated JSONL logging via [`ActivityWriter`], configuration
//! loading from `enforcement-policy.json`, and five hook handlers:
//!
//! - [`ToolUseLogging`] — logs `PostToolUse` events with sensitive redaction
//! - [`PromptLogging`] — logs `UserPromptSubmit` events with intent detection
//! - [`SessionStartLogging`] / [`SessionEndLogging`] — session lifecycle
//! - [`StopLogging`] — agent stop events with task context
//!
//! These are **operational activity logs**, distinct from Tier 0 ledger events.

pub mod prompt;
pub mod session;
pub mod stop;
pub mod tooluse;
mod writer;

pub use prompt::PromptLogging;
pub use session::{SessionEndLogging, SessionStartLogging};
pub use stop::StopLogging;
pub use tooluse::ToolUseLogging;
pub use writer::ActivityWriter;

use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Logging configuration
// ---------------------------------------------------------------------------

/// Top-level logging configuration from `enforcement-policy.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_session_start_config")]
    pub session_start: SessionStartConfig,

    #[serde(default = "default_session_end_config")]
    pub session_end: SessionEndConfig,

    #[serde(default = "default_post_tool_use_config")]
    pub post_tool_use: PostToolUseConfig,

    #[serde(default = "default_stop_config")]
    pub stop: StopConfig,

    #[serde(default = "default_user_prompt_config")]
    pub user_prompt: UserPromptConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionStartConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_log_directory")]
    pub log_directory: String,

    #[serde(default = "default_true")]
    pub capture_metadata: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionEndConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_true")]
    pub generate_summary: bool,

    #[serde(default = "default_true")]
    pub trigger_rotation: bool,

    #[serde(default = "default_log_directory")]
    pub log_directory: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostToolUseConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_log_directory")]
    pub log_directory: String,

    #[serde(default = "default_max_result_size")]
    pub max_result_size: usize,

    #[serde(default = "default_true")]
    pub redact_sensitive: bool,

    #[serde(default)]
    pub tools_to_log: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StopConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_log_directory")]
    pub log_directory: String,

    #[serde(default = "default_true")]
    pub capture_task_context: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Deserialize)]
pub struct UserPromptConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default = "default_log_directory")]
    pub log_directory: String,

    #[serde(default = "default_true")]
    pub capture_full_text: bool,

    #[serde(default = "default_true")]
    pub detect_intent: bool,

    #[serde(default)]
    pub privacy_mode: bool,
}

// Serde default functions
fn default_true() -> bool {
    true
}
fn default_log_directory() -> String {
    ".state/logs/sessions".to_string()
}
fn default_max_result_size() -> usize {
    2000
}

fn default_session_start_config() -> SessionStartConfig {
    SessionStartConfig {
        enabled: true,
        log_directory: default_log_directory(),
        capture_metadata: true,
    }
}
fn default_session_end_config() -> SessionEndConfig {
    SessionEndConfig {
        enabled: true,
        generate_summary: true,
        trigger_rotation: true,
        log_directory: default_log_directory(),
    }
}
fn default_post_tool_use_config() -> PostToolUseConfig {
    PostToolUseConfig {
        enabled: true,
        log_directory: default_log_directory(),
        max_result_size: 2000,
        redact_sensitive: true,
        tools_to_log: Vec::new(),
    }
}
fn default_stop_config() -> StopConfig {
    StopConfig {
        enabled: true,
        log_directory: default_log_directory(),
        capture_task_context: true,
    }
}
fn default_user_prompt_config() -> UserPromptConfig {
    UserPromptConfig {
        enabled: true,
        log_directory: default_log_directory(),
        capture_full_text: true,
        detect_intent: true,
        privacy_mode: false,
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            session_start: default_session_start_config(),
            session_end: default_session_end_config(),
            post_tool_use: default_post_tool_use_config(),
            stop: default_stop_config(),
            user_prompt: default_user_prompt_config(),
        }
    }
}

/// Read logging configuration from `enforcement-policy.json`.
///
/// Returns `Default` on any error (missing file, parse failure, missing key).
#[must_use]
pub fn read_config(project_dir: &Path) -> LoggingConfig {
    let path = project_dir
        .join(".codeflow")
        .join("config")
        .join("enforcement")
        .join("enforcement-policy.json");

    let Ok(data) = std::fs::read_to_string(&path) else {
        return LoggingConfig::default();
    };

    // Parse outer JSON, extract "logging" key.
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(&data) else {
        return LoggingConfig::default();
    };

    let Some(logging_val) = raw.get("logging") else {
        return LoggingConfig::default();
    };

    // Overlay parsed values onto defaults.
    serde_json::from_value(logging_val.clone()).unwrap_or_default()
}

/// Resolve a log directory path. If relative, joins with `project_dir`.
#[must_use]
pub fn resolve_log_dir(project_dir: &Path, log_dir: &str) -> std::path::PathBuf {
    let p = Path::new(log_dir);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_dir.join(log_dir)
    }
}

/// Resolve session ID for logging purposes.
///
/// Uses the canonical session resolution (env file > env var) with
/// a fallback to `"unknown"` — logging must never fail.
#[must_use]
pub fn resolve_session_id(project_dir: &Path) -> String {
    match crate::session::current_session_id(project_dir) {
        Ok(sid) => sid.into_inner(),
        Err(_) => "unknown".to_string(),
    }
}

/// Get the current git branch name for a project directory.
#[must_use]
pub(super) fn git_branch(project_dir: &Path) -> Option<String> {
    let dir_str = project_dir.display().to_string();
    let output = std::process::Command::new("git")
        .args(["-C", &dir_str, "branch", "--show-current"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = LoggingConfig::default();
        assert!(cfg.session_start.enabled);
        assert!(cfg.session_start.capture_metadata);
        assert_eq!(cfg.session_start.log_directory, ".state/logs/sessions");
        assert!(cfg.post_tool_use.enabled);
        assert_eq!(cfg.post_tool_use.max_result_size, 2000);
        assert!(cfg.post_tool_use.redact_sensitive);
        assert!(cfg.post_tool_use.tools_to_log.is_empty());
        assert!(cfg.user_prompt.enabled);
        assert!(cfg.user_prompt.capture_full_text);
        assert!(cfg.user_prompt.detect_intent);
        assert!(!cfg.user_prompt.privacy_mode);
        assert!(cfg.stop.enabled);
        assert!(cfg.stop.capture_task_context);
    }

    #[test]
    fn test_read_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = read_config(dir.path());
        assert!(cfg.session_start.enabled);
        assert_eq!(cfg.post_tool_use.max_result_size, 2000);
    }

    #[test]
    fn test_read_config_missing_logging_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"other_key": true}"#,
        )
        .unwrap();
        let cfg = read_config(dir.path());
        assert!(cfg.session_start.enabled);
    }

    #[test]
    fn test_read_config_partial_override() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"post_tool_use": {"max_result_size": 500, "enabled": false}}}"#,
        )
        .unwrap();
        let cfg = read_config(dir.path());
        assert_eq!(cfg.post_tool_use.max_result_size, 500);
        assert!(!cfg.post_tool_use.enabled);
        // Other sections should have defaults.
        assert!(cfg.session_start.enabled);
    }

    #[test]
    fn test_resolve_log_dir_relative() {
        let dir = Path::new("/project");
        let result = resolve_log_dir(dir, ".state/logs/sessions");
        assert_eq!(result, Path::new("/project/.state/logs/sessions"));
    }

    #[test]
    fn test_resolve_log_dir_absolute() {
        let dir = Path::new("/project");
        let result = resolve_log_dir(dir, "/var/log/codeflow");
        assert_eq!(result, Path::new("/var/log/codeflow"));
    }

    #[test]
    fn test_resolve_session_id_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_session_id(dir.path());
        assert_eq!(result, "unknown");
    }

    #[test]
    fn test_resolve_session_id_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("codeflow-env.sh"),
            "export CODEFLOW_SESSION_ID=\"ses-test123\"\nexport CF_PROJECT_ROOT=\"test\"\n",
        )
        .unwrap();
        let result = resolve_session_id(dir.path());
        assert_eq!(result, "ses-test123");
    }

    #[test]
    fn test_config_deserialize_full() {
        let json = r#"{
            "session_start": {"enabled": false, "log_directory": "/tmp/logs", "capture_metadata": false},
            "session_end": {"enabled": true},
            "post_tool_use": {"enabled": true, "max_result_size": 100, "tools_to_log": ["Bash", "Edit"]},
            "stop": {"enabled": false},
            "user_prompt": {"enabled": true, "privacy_mode": true}
        }"#;
        let cfg: LoggingConfig = serde_json::from_str(json).unwrap();
        assert!(!cfg.session_start.enabled);
        assert_eq!(cfg.session_start.log_directory, "/tmp/logs");
        assert!(!cfg.session_start.capture_metadata);
        assert_eq!(cfg.post_tool_use.max_result_size, 100);
        assert_eq!(cfg.post_tool_use.tools_to_log, vec!["Bash", "Edit"]);
        assert!(!cfg.stop.enabled);
        assert!(cfg.user_prompt.privacy_mode);
    }
}
