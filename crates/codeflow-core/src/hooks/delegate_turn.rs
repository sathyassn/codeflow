//! Deterministic completion signal for a task-scoped interactive Claude turn.
//!
//! Claude's `Stop` and `StopFailure` hooks call this handler with the hook JSON
//! on stdin. The handler writes one owner-only result and then signals a unique
//! tmux wait channel. It deliberately does not read transcripts or tmux panes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Prefix shared by the producer and the documented `tmux wait-for` consumer.
pub const SIGNAL_PREFIX: &str = "codeflow-delegate-";

/// Input needed to persist and signal one delegate turn.
#[derive(Debug, Clone)]
pub struct TurnConfig {
    /// Caller-generated identifier; also scopes the tmux wait channel.
    pub run_id: String,
    /// Absolute path inside an existing owner-only directory.
    pub result: PathBuf,
}

#[derive(Debug, Deserialize)]
struct HookPayload {
    hook_event_name: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    last_assistant_message: Option<String>,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    error_details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct TurnResult {
    schema_version: u8,
    run_id: String,
    event: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assistant_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_details: Option<serde_json::Value>,
}

/// Persist one terminal hook event and signal its waiter.
///
/// The result is created with `create_new`, so a duplicate or stale hook can
/// never overwrite the first terminal result. An exact retry may re-signal the
/// waiter, which recovers a transient tmux failure without mutating evidence.
/// The waiter is signalled only after the complete JSON document is durable.
///
/// # Errors
///
/// Returns a legible error when the run id, private result path, or hook payload
/// is invalid; when the result cannot be created exactly once; or when tmux
/// cannot signal the scoped waiter.
pub fn record_and_signal(config: &TurnConfig, stdin: &str) -> Result<(), String> {
    record_and_signal_with(config, stdin, signal_tmux)
}

fn record_and_signal_with(
    config: &TurnConfig,
    stdin: &str,
    signal: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    validate_run_id(&config.run_id)?;
    validate_result_path(&config.result)?;

    let payload: HookPayload = serde_json::from_str(stdin)
        .map_err(|error| format!("invalid Claude hook payload: {error}"))?;
    let result = terminal_result(&config.run_id, payload)?;
    write_once(&config.result, &result)?;

    let channel = format!("{SIGNAL_PREFIX}{}", config.run_id);
    signal(&channel)
}

fn signal_tmux(channel: &str) -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["wait-for", "-S", channel])
        .output()
        .map_err(|error| format!("could not run tmux completion signal: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "tmux completion signal failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn terminal_result(run_id: &str, payload: HookPayload) -> Result<TurnResult, String> {
    let (status, last_assistant_message, error, error_details) =
        match payload.hook_event_name.as_str() {
            "Stop" => {
                let message = payload.last_assistant_message.ok_or_else(|| {
                    "Stop payload omitted last_assistant_message; refusing an ambiguous completion"
                        .to_string()
                })?;
                ("completed", Some(message), None, None)
            }
            "StopFailure" => ("failed", None, payload.error, payload.error_details),
            other => {
                return Err(format!(
                    "unsupported hook_event_name {other:?}; expected Stop or StopFailure"
                ));
            }
        };

    Ok(TurnResult {
        schema_version: 1,
        run_id: run_id.to_string(),
        event: payload.hook_event_name,
        status,
        session_id: payload.session_id,
        cwd: payload.cwd,
        last_assistant_message,
        error,
        error_details,
    })
}

fn validate_run_id(run_id: &str) -> Result<(), String> {
    let valid = !run_id.is_empty()
        && run_id.len() <= 64
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err("run id must be 1-64 ASCII letters, digits, '.', '_' or '-'".to_string())
    }
}

fn validate_result_path(result: &Path) -> Result<(), String> {
    if !result.is_absolute() {
        return Err("delegate result path must be absolute".to_string());
    }
    let parent = result
        .parent()
        .ok_or_else(|| "delegate result path has no parent".to_string())?;
    let metadata = std::fs::metadata(parent)
        .map_err(|error| format!("delegate result directory is unavailable: {error}"))?;
    if !metadata.is_dir() {
        return Err("delegate result parent is not a directory".to_string());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "delegate result directory must be owner-only (mode 0700 or stricter)".to_string(),
            );
        }
    }
    Ok(())
}

fn write_once(path: &Path, result: &TurnResult) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(result)
        .map_err(|error| format!("could not encode delegate result: {error}"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return verify_exact_retry(path, result);
        }
        Err(error) => return Err(format!("could not create delegate result: {error}")),
    };
    file.write_all(&json)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not persist delegate result: {error}"))
}

fn verify_exact_retry(path: &Path, result: &TurnResult) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect existing delegate result: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("existing delegate result is not a regular file".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("existing delegate result is not owner-only".to_string());
        }
    }

    let existing = std::fs::read(path)
        .map_err(|error| format!("could not read existing delegate result: {error}"))?;
    let existing: serde_json::Value = serde_json::from_slice(&existing)
        .map_err(|error| format!("existing delegate result is invalid JSON: {error}"))?;
    let expected = serde_json::to_value(result)
        .map_err(|error| format!("could not compare delegate result: {error}"))?;
    if existing == expected {
        Ok(())
    } else {
        Err("delegate result already exists with different terminal evidence".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_run_ids() {
        for run_id in ["a", "run-42", "plan_v2.review"] {
            assert!(validate_run_id(run_id).is_ok(), "{run_id}");
        }
    }

    #[test]
    fn rejects_unsafe_run_ids() {
        for run_id in ["", "has space", "a/b", "$(bad)", &"x".repeat(65)] {
            assert!(validate_run_id(run_id).is_err(), "{run_id}");
        }
    }

    #[test]
    fn stop_requires_and_preserves_last_message() {
        let result = terminal_result(
            "run-1",
            serde_json::from_str(
                r#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"VERDICT: approved"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "completed");
        assert_eq!(value["last_assistant_message"], "VERDICT: approved");
        assert!(value.get("error").is_none());

        let missing: HookPayload = serde_json::from_str(r#"{"hook_event_name":"Stop"}"#).unwrap();
        assert!(terminal_result("run-1", missing).is_err());
    }

    #[test]
    fn stop_failure_preserves_structured_error_only() {
        let result = terminal_result(
            "run-2",
            serde_json::from_str(
                r#"{"hook_event_name":"StopFailure","error":"rate_limit","error_details":{"retryable":true},"last_assistant_message":"partial"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "failed");
        assert_eq!(value["error"], "rate_limit");
        assert!(value.get("last_assistant_message").is_none());
    }

    #[test]
    fn result_path_must_be_absolute_and_private() {
        assert!(validate_result_path(Path::new("relative.json")).is_err());
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(validate_result_path(&dir.path().join("result.json")).is_ok());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(validate_result_path(&dir.path().join("result.json")).is_err());
        }
    }

    #[test]
    fn write_once_allows_exact_retry_but_never_overwrites_terminal_result() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("result.json");
        let payload: HookPayload =
            serde_json::from_str(r#"{"hook_event_name":"Stop","last_assistant_message":"first"}"#)
                .unwrap();
        let result = terminal_result("run-3", payload).unwrap();
        write_once(&path, &result).unwrap();
        write_once(&path, &result).unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("first"));

        let changed = terminal_result(
            "run-3",
            serde_json::from_str(
                r#"{"hook_event_name":"Stop","last_assistant_message":"changed"}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(write_once(&dir.path().join("result.json"), &changed).is_err());
    }

    #[test]
    fn records_before_signalling_the_scoped_channel() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let result = dir.path().join("result.json");
        let config = TurnConfig {
            run_id: "review-42".to_string(),
            result: result.clone(),
        };
        record_and_signal_with(
            &config,
            r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#,
            |channel| {
                assert_eq!(channel, "codeflow-delegate-review-42");
                assert!(result.exists(), "result must exist before signal");
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn exact_retry_recovers_a_transient_signal_failure() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = TurnConfig {
            run_id: "retry-1".to_string(),
            result: dir.path().join("result.json"),
        };
        let payload = r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#;
        assert!(record_and_signal_with(&config, payload, |_| Err("tmux down".into())).is_err());
        assert!(config.result.exists());
        record_and_signal_with(&config, payload, |_| Ok(())).unwrap();
    }
}
