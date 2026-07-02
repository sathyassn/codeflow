//! Infrastructure health checks.
//!
//! The doctor core: a small registry of named checks that run concurrently
//! and report pass/warn/fail with timing. The CLI layer renders results and
//! adds the enforcement-matrix view (charter section 3.1). Checks verify the
//! v2 surface only: the binary's hook subcommands, harness availability,
//! `.codeflow/` config validity and writability, and network reachability.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::DoctorError;

/// Outcome of a health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Fail,
    Warn,
}

/// Result of a single doctor check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: Status,
    pub message: String,
    #[serde(with = "duration_millis")]
    pub duration: Duration,
}

/// Serialization helper for Duration as milliseconds.
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        s.serialize_u64(ms)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Callback to locate an executable by name.
type LookPathFn = fn(&str) -> Result<String, String>;

/// Callback to execute a command with arguments.
type ExecCommandFn = fn(&str, &[&str]) -> Result<String, String>;

/// Configuration for doctor checks.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Project root directory.
    pub project_dir: String,
    /// Locates executables. Returns Ok(path) or Err.
    pub look_path: Option<LookPathFn>,
    /// Runs a command and returns its output.
    pub exec_command: Option<ExecCommandFn>,
}

impl Options {
    fn do_look_path(&self, name: &str) -> Result<String, String> {
        if let Some(f) = self.look_path {
            f(name)
        } else {
            which::which(name)
                .map(|p| p.to_string_lossy().to_string())
                .map_err(|e| e.to_string())
        }
    }

    fn do_exec(&self, cmd: &str, args: &[&str]) -> Result<String, String> {
        if let Some(f) = self.exec_command {
            f(cmd, args)
        } else {
            let output = std::process::Command::new(cmd)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }
    }
}

/// Hook subcommands that must be functional (charter section 3.3): the
/// three Claude-layer hooks plus the three git-hook shims.
const HOOK_SUBCOMMANDS: &[(&str, &str)] = &[
    ("hook", "git-guard"),
    ("hook", "session-orient"),
    ("hook", "session-summary"),
    ("git-hook", "pre-commit"),
    ("git-hook", "commit-msg"),
    ("git-hook", "pre-push"),
];

/// Ordered list of all check names.
const CHECK_NAMES: &[&str] = &[
    "hooks",
    "claude",
    "config",
    "permissions",
    "network",
    "delegates",
];

/// Return the ordered list of all available check names.
#[must_use]
pub fn check_names() -> Vec<&'static str> {
    CHECK_NAMES.to_vec()
}

/// Type alias for individual check functions.
type CheckFn = fn(&Options) -> CheckResult;

/// Build the check registry mapping names to functions.
fn check_registry() -> HashMap<&'static str, CheckFn> {
    let mut m: HashMap<&'static str, CheckFn> = HashMap::new();
    m.insert("hooks", check_hooks);
    m.insert("claude", check_claude);
    m.insert("config", check_config);
    m.insert("permissions", check_permissions);
    m.insert("network", check_network);
    m.insert("delegates", check_delegates);
    m
}

/// Run all checks concurrently and return results in canonical order.
///
/// Uses `tokio::task::spawn_blocking` for concurrent execution. Individual
/// check failures are captured in the returned `CheckResult` items.
pub async fn run_all(opts: &Options) -> Vec<CheckResult> {
    let registry = check_registry();
    let mut handles = Vec::with_capacity(CHECK_NAMES.len());

    for &name in CHECK_NAMES {
        let f = registry[name];
        let opts_clone = opts.clone();
        handles.push(tokio::task::spawn_blocking(move || f(&opts_clone)));
    }

    let mut results = Vec::with_capacity(CHECK_NAMES.len());
    for handle in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(e) => results.push(CheckResult {
                name: "unknown".to_string(),
                status: Status::Fail,
                message: format!("task join error: {e}"),
                duration: Duration::ZERO,
            }),
        }
    }
    results
}

/// Run a single named check.
///
/// # Errors
///
/// Returns `DoctorError::CheckNotFound` if the name is not in the registry.
pub fn run_check(name: &str, opts: &Options) -> Result<CheckResult, DoctorError> {
    let registry = check_registry();
    let f = registry
        .get(name)
        .ok_or_else(|| DoctorError::CheckNotFound(name.to_string()))?;
    Ok(f(opts))
}

// ---------------------------------------------------------------------------
// Individual check functions
// ---------------------------------------------------------------------------

fn check_hooks(opts: &Options) -> CheckResult {
    let start = Instant::now();

    let Ok(codeflow_bin) = opts.do_look_path("codeflow") else {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: "codeflow binary not found in PATH".into(),
            duration: start.elapsed(),
        };
    };

    let mut failing = Vec::new();
    for &(group, subcommand) in HOOK_SUBCOMMANDS {
        if opts
            .do_exec(&codeflow_bin, &[group, subcommand, "--help"])
            .is_err()
        {
            failing.push(format!("{group} {subcommand}"));
        }
    }

    if !failing.is_empty() {
        return CheckResult {
            name: "hooks".into(),
            status: Status::Fail,
            message: format!(
                "{} hook subcommand(s) not responding: {}",
                failing.len(),
                failing.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "hooks".into(),
        status: Status::Pass,
        message: format!("all {} hook subcommands functional", HOOK_SUBCOMMANDS.len()),
        duration: start.elapsed(),
    }
}

fn check_claude(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_look_path("claude") {
        Ok(_) => CheckResult {
            name: "claude".into(),
            status: Status::Pass,
            message: "claude CLI found".into(),
            duration: start.elapsed(),
        },
        // Warn, not fail: git hooks + CI are the canonical enforcement
        // plane (charter section 9); codeflow works without a harness.
        Err(_) => CheckResult {
            name: "claude".into(),
            status: Status::Warn,
            message: "claude CLI not found in PATH (Claude-layer hooks inactive)".into(),
            duration: start.elapsed(),
        },
    }
}

fn check_config(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = Path::new(&opts.project_dir).join(".codeflow");

    if !config_dir.is_dir() {
        return CheckResult {
            name: "config".into(),
            status: Status::Warn,
            message: ".codeflow/ directory not found (run codeflow init)".into(),
            duration: start.elapsed(),
        };
    }

    let mut invalid_files = Vec::new();
    if let Err(e) = walk_json_files(&config_dir, &mut invalid_files) {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("error scanning .codeflow/: {e}"),
            duration: start.elapsed(),
        };
    }

    if !invalid_files.is_empty() {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("invalid JSON config files: {}", invalid_files.join(", ")),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "config".into(),
        status: Status::Pass,
        message: "all .codeflow/ JSON files are valid".into(),
        duration: start.elapsed(),
    }
}

/// Walk a directory tree and validate JSON files, recording paths relative
/// to `dir` for any that fail to parse.
fn walk_json_files(dir: &Path, invalid: &mut Vec<String>) -> Result<(), std::io::Error> {
    walk_json_files_inner(dir, dir, invalid)
}

fn walk_json_files_inner(
    root: &Path,
    dir: &Path,
    invalid: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_json_files_inner(root, &path, invalid)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let rel = path.strip_prefix(root).map_or_else(
                |_| path.to_string_lossy().to_string(),
                |p| p.to_string_lossy().to_string(),
            );
            match std::fs::read(&path) {
                Ok(data) => {
                    if serde_json::from_slice::<serde_json::Value>(&data).is_err() {
                        invalid.push(rel);
                    }
                }
                Err(e) => {
                    invalid.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    Ok(())
}

fn check_permissions(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = PathBuf::from(&opts.project_dir).join(".codeflow");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&config_dir) {
            if meta.permissions().mode() & 0o200 == 0 {
                return CheckResult {
                    name: "permissions".into(),
                    status: Status::Fail,
                    message: ".codeflow/ is not writable".into(),
                    duration: start.elapsed(),
                };
            }
        }
    }

    let _ = config_dir; // suppress unused warning on non-unix

    CheckResult {
        name: "permissions".into(),
        status: Status::Pass,
        message: "file permissions correct".into(),
        duration: start.elapsed(),
    }
}

fn check_network(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_exec("host", &["-W", "2", "github.com"]) {
        Ok(_) => CheckResult {
            name: "network".into(),
            status: Status::Pass,
            message: "network connectivity OK".into(),
            duration: start.elapsed(),
        },
        Err(_) => CheckResult {
            name: "network".into(),
            status: Status::Warn,
            message: "network connectivity check failed (offline?)".into(),
            duration: start.elapsed(),
        },
    }
}

/// Cross-vendor delegation readiness (ADR-0005). Optional by design, so a
/// missing or unauthenticated delegate warns — never fails. `codex` is the
/// primary tier: authenticated under the user's own subscription. `agy`
/// (Antigravity) is a degraded, opt-in consult tier reported informationally.
fn check_delegates(opts: &Options) -> CheckResult {
    let start = Instant::now();

    // `agy` presence is informational; codex authentication decides pass/warn.
    let agy_note = if opts.do_look_path("agy").is_ok() {
        "; agy present (degraded read-only consult tier, opt-in)"
    } else {
        ""
    };

    let Ok(codex_bin) = opts.do_look_path("codex") else {
        return CheckResult {
            name: "delegates".into(),
            status: Status::Warn,
            message: format!(
                "codex not found in PATH — cross-vendor delegation unavailable (optional){agy_note}"
            ),
            duration: start.elapsed(),
        };
    };

    // `codex login status` exits 0 only when subscription-authenticated.
    match opts.do_exec(&codex_bin, &["login", "status"]) {
        Ok(_) => CheckResult {
            name: "delegates".into(),
            status: Status::Pass,
            message: format!("codex: subscription-authenticated{agy_note}"),
            duration: start.elapsed(),
        },
        Err(_) => CheckResult {
            name: "delegates".into(),
            status: Status::Warn,
            message: format!(
                "codex present but not authenticated — run `codex login` to enable delegation{agy_note}"
            ),
            duration: start.elapsed(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_opts() -> Options {
        Options {
            look_path: Some(|_| Err("not found".into())),
            exec_command: Some(|_, _| Err("not available".into())),
            ..Options::default()
        }
    }

    #[test]
    fn test_check_names_count() {
        assert_eq!(check_names().len(), 6);
    }

    #[test]
    fn test_check_registry_has_all_entries() {
        let registry = check_registry();
        for &name in CHECK_NAMES {
            assert!(registry.contains_key(name), "missing check: {name}");
        }
    }

    #[test]
    fn test_run_check_unknown() {
        let opts = test_opts();
        let result = run_check("nonexistent", &opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_check_claude_not_found_warns() {
        let opts = test_opts();
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Warn);
    }

    #[test]
    fn test_check_claude_found() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "claude" {
                Ok("/usr/local/bin/claude".into())
            } else {
                Err("not found".into())
            }
        });
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_missing_dir_warns() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("policy.json"), "{}").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("bad.json"), "not json!!").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("invalid"));
        assert!(result.message.contains("bad.json"));
    }

    #[test]
    fn test_check_config_nested_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join(".codeflow").join(".baseline");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("settings.json"), "{ nope").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
    }

    #[test]
    fn test_check_hooks_no_binary() {
        let opts = test_opts();
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_hooks_all_pass() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, _args| Ok("help output".to_string()));
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("functional"));
        assert!(result.message.contains('6'));
    }

    #[test]
    fn test_check_hooks_some_failing() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codeflow" {
                Ok("/usr/local/bin/codeflow".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_cmd, args| {
            // Fail for git-guard, pass for others.
            if args.len() >= 2 && args[1] == "git-guard" {
                Err("unknown subcommand".into())
            } else {
                Ok("ok".into())
            }
        });
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not responding"));
        assert!(result.message.contains("hook git-guard"));
    }

    #[test]
    fn test_check_network_pass() {
        let mut opts = test_opts();
        opts.exec_command = Some(|_cmd, _args| Ok("github.com has address".into()));
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_network_fail_warns() {
        let opts = test_opts(); // exec_command returns Err
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("failed"));
    }

    #[test]
    fn test_check_permissions_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[cfg(unix)]
    #[test]
    fn test_check_permissions_readonly_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);

        // Restore so tempdir cleanup succeeds.
        std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not writable"));
    }

    #[test]
    fn test_check_delegates_codex_authenticated_passes() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codex" {
                Ok("/usr/local/bin/codex".into())
            } else {
                Err("not found".into())
            }
        });
        opts.exec_command = Some(|_, _| Ok("Logged in using ChatGPT".into()));
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("subscription-authenticated"), "got: {}", result.message);
    }

    #[test]
    fn test_check_delegates_codex_missing_warns_not_fails() {
        // test_opts look_path always errs → codex absent. Optional, so warn.
        let opts = test_opts();
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("optional"), "got: {}", result.message);
    }

    #[test]
    fn test_check_delegates_codex_present_unauthenticated_warns_with_remedy() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "codex" {
                Ok("/usr/local/bin/codex".into())
            } else {
                Err("not found".into())
            }
        });
        // exec_command errs → `codex login status` nonzero → unauthenticated.
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("codex login"), "remedy line: {}", result.message);
    }

    #[test]
    fn test_check_delegates_agy_presence_is_informational() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| match name {
            "codex" => Ok("/usr/local/bin/codex".into()),
            "agy" => Ok("/usr/local/bin/agy".into()),
            _ => Err("not found".into()),
        });
        opts.exec_command = Some(|_, _| Ok("Logged in".into()));
        let result = check_delegates(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("agy present"), "got: {}", result.message);
    }

    #[test]
    fn test_status_serde() {
        let json = serde_json::to_string(&Status::Pass).unwrap();
        assert_eq!(json, "\"pass\"");
        let parsed: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Status::Pass);
    }

    #[test]
    fn test_check_result_serialization() {
        let result = CheckResult {
            name: "test".into(),
            status: Status::Pass,
            message: "ok".into(),
            duration: Duration::from_millis(42),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"duration\":42"));
    }

    #[tokio::test]
    async fn test_run_all_returns_all_results() {
        let opts = test_opts();
        let results = run_all(&opts).await;
        assert_eq!(results.len(), CHECK_NAMES.len());
    }

    #[test]
    fn test_run_check_valid_name() {
        let opts = test_opts();
        let result = run_check("claude", &opts).unwrap();
        assert_eq!(result.name, "claude");
    }
}
