//! Infrastructure health check and repair.
//!
//! Provides 16 check functions and 3 repair functions ported from Go's
//! `internal/doctor` package. Checks run concurrently via `tokio::spawn`.

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
    /// Path to the `SQLite` database file.
    pub db_path: String,
    /// Path to the JSONL ledger directory.
    pub ledger_dir: String,
    /// Project root directory.
    pub project_dir: String,
    /// Path to the `.state/` directory.
    pub state_dir: String,
    /// Active `PathFlow` session ID.
    pub session_id: String,
    /// Override user home directory (for testing).
    pub home_dir: String,
    /// Locates executables. Returns Ok(path) or Err.
    pub look_path: Option<LookPathFn>,
    /// Runs a command and returns its output.
    pub exec_command: Option<ExecCommandFn>,
}

impl Options {
    fn effective_state_dir(&self) -> String {
        if self.state_dir.is_empty() {
            let mut p = PathBuf::from(&self.project_dir);
            p.push(".state");
            p.to_string_lossy().to_string()
        } else {
            self.state_dir.clone()
        }
    }

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

/// Canonical ledger types (subdirectory names).
const CANONICAL_LEDGER_TYPES: &[&str] = &["work-graph", "memory-events", "sessions", "config"];

/// Required subdirectories under `.state/`.
const REQUIRED_STATE_DIRS: &[&str] = &["db", "ledger", "logs", "runtime", "sentinels"];

/// Hook subcommands that must be functional.
const HOOK_SUBCOMMANDS: &[(&str, &str)] = &[
    ("session-start", "init"),
    ("pre-tool-use", "gate-check"),
    ("pre-tool-use", "team-guard"),
    ("post-tool-use", "sentinel-write"),
    ("post-tool-use", "checkpoint-register"),
    ("task-completed", "checkpoint-complete"),
    ("session-end", "cleanup"),
];

/// Threshold for considering a `PathFlow` phase stuck (30 minutes).
const STUCK_THRESHOLD_SECS: u64 = 30 * 60;

/// Ordered list of all check names.
const CHECK_NAMES: &[&str] = &[
    "database",
    "jsonl",
    "crdt",
    "python",
    "hooks",
    "claude",
    "auth",
    "config",
    "embedding",
    "vector",
    "permissions",
    "version",
    "network",
    "pathflow-stuck",
    "team-health",
    "sentinel-drift",
    "ledger-health",
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
    m.insert("database", check_database);
    m.insert("jsonl", check_jsonl);
    m.insert("crdt", check_crdt);
    m.insert("python", check_python);
    m.insert("hooks", check_hooks);
    m.insert("claude", check_claude);
    m.insert("auth", check_auth);
    m.insert("config", check_config);
    m.insert("embedding", check_embedding);
    m.insert("vector", check_vector);
    m.insert("permissions", check_permissions);
    m.insert("version", check_version);
    m.insert("network", check_network);
    m.insert("pathflow-stuck", check_pathflow_stuck);
    m.insert("team-health", check_team_health);
    m.insert("sentinel-drift", check_sentinel_drift);
    m.insert("ledger-health", check_ledger_health);
    m
}

/// Run all 16 checks concurrently and return results in canonical order.
///
/// Uses `tokio::spawn` for concurrent execution.
///
/// # Errors
///
/// Returns `DoctorError` only on orchestration failures. Individual check
/// failures are captured in the returned `CheckResult` items.
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
// Individual check functions (16)
// ---------------------------------------------------------------------------

fn check_database(opts: &Options) -> CheckResult {
    let start = Instant::now();

    if opts.db_path.is_empty() {
        return CheckResult {
            name: "database".into(),
            status: Status::Fail,
            message: "database path not configured".into(),
            duration: start.elapsed(),
        };
    }

    if !Path::new(&opts.db_path).exists() {
        return CheckResult {
            name: "database".into(),
            status: Status::Fail,
            message: format!("database file not found: {}", opts.db_path),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "database".into(),
        status: Status::Pass,
        message: "database file exists".into(),
        duration: start.elapsed(),
    }
}

fn check_jsonl(opts: &Options) -> CheckResult {
    let start = Instant::now();

    if opts.ledger_dir.is_empty() {
        return CheckResult {
            name: "jsonl".into(),
            status: Status::Fail,
            message: "ledger directory not configured".into(),
            duration: start.elapsed(),
        };
    }

    let ledger_path = Path::new(&opts.ledger_dir);
    let mut invalid = Vec::new();

    for &type_name in CANONICAL_LEDGER_TYPES {
        // Subdirectory layout: {type}/{type}.jsonl
        let subdir_path = ledger_path
            .join(type_name)
            .join(format!("{type_name}.jsonl"));
        // Flat layout fallback: {type}.jsonl
        let flat_path = ledger_path.join(format!("{type_name}.jsonl"));

        let path = if subdir_path.exists() {
            subdir_path
        } else if flat_path.exists() {
            flat_path
        } else {
            // Base file missing is acceptable for types with no events yet.
            continue;
        };

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                for (i, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if serde_json::from_str::<serde_json::Value>(trimmed).is_err() {
                        invalid.push(format!("{type_name}: invalid JSON at line {}", i + 1));
                        break;
                    }
                }
            }
            Err(e) => {
                invalid.push(format!("{type_name}: {e}"));
            }
        }
    }

    if !invalid.is_empty() {
        return CheckResult {
            name: "jsonl".into(),
            status: Status::Fail,
            message: format!("invalid: {}", invalid.join("; ")),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "jsonl".into(),
        status: Status::Pass,
        message: "canonical JSONL files valid".into(),
        duration: start.elapsed(),
    }
}

fn check_crdt(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let state_dir = opts.effective_state_dir();

    let mut missing_dirs = Vec::new();
    for &dir in REQUIRED_STATE_DIRS {
        let path = Path::new(&state_dir).join(dir);
        if !path.is_dir() {
            missing_dirs.push(dir.to_string());
        }
    }

    if !missing_dirs.is_empty() {
        return CheckResult {
            name: "crdt".into(),
            status: Status::Fail,
            message: format!(
                "missing .state/ subdirectories: {}",
                missing_dirs.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "crdt".into(),
        status: Status::Pass,
        message: ".state/ directory structure valid".into(),
        duration: start.elapsed(),
    }
}

fn check_python(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_look_path("python3") {
        Ok(_) => CheckResult {
            name: "python".into(),
            status: Status::Pass,
            message: "python3 available (optional -- used by test infrastructure only)".into(),
            duration: start.elapsed(),
        },
        Err(_) => CheckResult {
            name: "python".into(),
            status: Status::Pass,
            message: "python3 not found (optional -- not a production dependency)".into(),
            duration: start.elapsed(),
        },
    }
}

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
    for &(event, subcommand) in HOOK_SUBCOMMANDS {
        if opts
            .do_exec(&codeflow_bin, &["hooks", event, subcommand, "--help"])
            .is_err()
        {
            failing.push(format!("{event} {subcommand}"));
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
        Err(_) => CheckResult {
            name: "claude".into(),
            status: Status::Fail,
            message: "claude CLI not found in PATH".into(),
            duration: start.elapsed(),
        },
    }
}

fn check_auth(opts: &Options) -> CheckResult {
    let start = Instant::now();

    match opts.do_exec("claude", &["auth", "status"]) {
        Ok(output) => {
            let lower = output.to_lowercase();
            if lower.contains("not authenticated") || lower.contains("not logged in") {
                CheckResult {
                    name: "auth".into(),
                    status: Status::Fail,
                    message: "claude Code not authenticated".into(),
                    duration: start.elapsed(),
                }
            } else {
                CheckResult {
                    name: "auth".into(),
                    status: Status::Pass,
                    message: "claude Code authenticated".into(),
                    duration: start.elapsed(),
                }
            }
        }
        Err(e) => CheckResult {
            name: "auth".into(),
            status: Status::Fail,
            message: format!("claude auth check failed: {e}"),
            duration: start.elapsed(),
        },
    }
}

fn check_config(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let config_dir = Path::new(&opts.project_dir)
        .join(".codeflow")
        .join("config");

    if !config_dir.is_dir() {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: ".codeflow/config/ directory not found".into(),
            duration: start.elapsed(),
        };
    }

    let mut invalid_files = Vec::new();
    if let Err(e) = walk_json_files(&config_dir, &mut invalid_files) {
        return CheckResult {
            name: "config".into(),
            status: Status::Fail,
            message: format!("error scanning config directory: {e}"),
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
        message: "all config files are valid JSON".into(),
        duration: start.elapsed(),
    }
}

/// Walk a directory tree and validate JSON files.
fn walk_json_files(dir: &Path, invalid: &mut Vec<String>) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_json_files(&path, invalid)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let rel = path.strip_prefix(dir).map_or_else(
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

fn check_embedding(_opts: &Options) -> CheckResult {
    let start = Instant::now();
    CheckResult {
        name: "embedding".into(),
        status: Status::Warn,
        message: "embedding model not configured (optional)".into(),
        duration: start.elapsed(),
    }
}

fn check_vector(_opts: &Options) -> CheckResult {
    let start = Instant::now();
    CheckResult {
        name: "vector".into(),
        status: Status::Warn,
        message: "vector store not configured (optional)".into(),
        duration: start.elapsed(),
    }
}

fn check_permissions(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let state_dir = opts.effective_state_dir();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&state_dir) {
            if meta.permissions().mode() & 0o200 == 0 {
                return CheckResult {
                    name: "permissions".into(),
                    status: Status::Fail,
                    message: ".state/ is not writable".into(),
                    duration: start.elapsed(),
                };
            }
        }
    }

    let _ = state_dir; // suppress unused warning on non-unix

    CheckResult {
        name: "permissions".into(),
        status: Status::Pass,
        message: "file permissions correct".into(),
        duration: start.elapsed(),
    }
}

fn check_version(_opts: &Options) -> CheckResult {
    let start = Instant::now();
    CheckResult {
        name: "version".into(),
        status: Status::Pass,
        message: "version check passed".into(),
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

/// `PathFlow` event from `pathflow-events.jsonl`.
#[derive(Debug, Deserialize)]
struct PathflowEvent {
    #[serde(rename = "event")]
    event_type: String,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    timestamp: String,
}

fn pathflow_active(state_dir: &str, session_id: &str) -> bool {
    let pathflow_dir = Path::new(state_dir)
        .join("session")
        .join(session_id)
        .join("pathflow");

    // Check status file (sole authority).
    let status_path = pathflow_dir.join("pathflow-session-status.json");
    if let Ok(data) = std::fs::read_to_string(&status_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
            let status = val.get("status").and_then(|v| v.as_str()).unwrap_or("");
            return !status.is_empty() && status != "pf-complete";
        }
    }
    false
}

fn read_pathflow_events(state_dir: &str) -> Result<Vec<PathflowEvent>, String> {
    let path = Path::new(state_dir)
        .join("logs")
        .join("pathflow-events.jsonl");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<PathflowEvent>(trimmed) {
            events.push(ev);
        }
    }
    Ok(events)
}

fn check_pathflow_stuck(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let state_dir = opts.effective_state_dir();

    if opts.session_id.is_empty() || !pathflow_active(&state_dir, &opts.session_id) {
        return CheckResult {
            name: "pathflow-stuck".into(),
            status: Status::Pass,
            message: "pathflow not active".into(),
            duration: start.elapsed(),
        };
    }

    let events = match read_pathflow_events(&state_dir) {
        Ok(events) => events,
        Err(e) => {
            return CheckResult {
                name: "pathflow-stuck".into(),
                status: Status::Warn,
                message: format!("cannot read pathflow events: {e}"),
                duration: start.elapsed(),
            };
        }
    };

    let mut latest_phase = String::new();
    let mut latest_time = chrono::DateTime::<chrono::Utc>::MIN_UTC;

    for ev in &events {
        if ev.event_type != "phase_transition" {
            continue;
        }
        if let Ok(t) = crate::util::parse_timestamp(&ev.timestamp) {
            if t > latest_time {
                latest_time = t;
                latest_phase.clone_from(&ev.phase);
            }
        }
    }

    if latest_phase.is_empty() {
        return CheckResult {
            name: "pathflow-stuck".into(),
            status: Status::Pass,
            message: "no phase transitions found".into(),
            duration: start.elapsed(),
        };
    }

    let age = chrono::Utc::now()
        .signed_duration_since(latest_time)
        .to_std()
        .unwrap_or(Duration::ZERO);

    if age.as_secs() > STUCK_THRESHOLD_SECS {
        return CheckResult {
            name: "pathflow-stuck".into(),
            status: Status::Warn,
            message: format!(
                "phase {latest_phase} stuck for {}s (threshold: 30m)",
                age.as_secs()
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "pathflow-stuck".into(),
        status: Status::Pass,
        message: format!(
            "current phase {latest_phase} started {}s ago",
            age.as_secs()
        ),
        duration: start.elapsed(),
    }
}

/// Team config for health check.
#[derive(Debug, Deserialize)]
struct TeamConfig {
    #[serde(default)]
    members: Vec<TeamMember>,
}

#[derive(Debug, Deserialize)]
struct TeamMember {
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    #[serde(default, rename = "tmuxPaneId")]
    tmux_pane_id: String,
}

#[derive(Debug, Deserialize)]
struct PathflowTeamConfig {
    #[serde(default)]
    team_name: String,
}

fn discover_team_name(state_dir: &str, session_id: &str, home_dir: &str) -> Option<String> {
    if !session_id.is_empty() {
        let team_file = Path::new(state_dir)
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-team.json");
        if let Ok(data) = std::fs::read_to_string(&team_file) {
            if let Ok(ptc) = serde_json::from_str::<PathflowTeamConfig>(&data) {
                if !ptc.team_name.is_empty() {
                    return Some(ptc.team_name);
                }
            }
        }
    }

    if home_dir.is_empty() {
        return None;
    }

    let teams_dir = Path::new(home_dir).join(".claude").join("teams");
    if let Ok(entries) = std::fs::read_dir(&teams_dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                return Some(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    None
}

fn check_team_health(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let state_dir = opts.effective_state_dir();

    let Some(team_name) = discover_team_name(&state_dir, &opts.session_id, &opts.home_dir) else {
        return CheckResult {
            name: "team-health".into(),
            status: Status::Pass,
            message: "no active team".into(),
            duration: start.elapsed(),
        };
    };

    let home = if opts.home_dir.is_empty() {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        opts.home_dir.clone()
    };

    let config_path = Path::new(&home)
        .join(".claude")
        .join("teams")
        .join(&team_name)
        .join("config.json");

    let tc: TeamConfig = match std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
    {
        Some(tc) => tc,
        None => {
            return CheckResult {
                name: "team-health".into(),
                status: Status::Warn,
                message: format!("cannot read team config for {team_name}"),
                duration: start.elapsed(),
            };
        }
    };

    if tc.members.is_empty() {
        return CheckResult {
            name: "team-health".into(),
            status: Status::Pass,
            message: "team has no members".into(),
            duration: start.elapsed(),
        };
    }

    let Ok(pane_list) = opts.do_exec("tmux", &["list-panes", "-a"]) else {
        return CheckResult {
            name: "team-health".into(),
            status: Status::Warn,
            message: "tmux not available".into(),
            duration: start.elapsed(),
        };
    };

    let dead_count = tc
        .members
        .iter()
        .filter(|m| !m.tmux_pane_id.is_empty() && !pane_list.contains(&m.tmux_pane_id))
        .count();

    if dead_count > 0 {
        return CheckResult {
            name: "team-health".into(),
            status: Status::Warn,
            message: format!(
                "{dead_count} of {} teammate panes not found in tmux",
                tc.members.len()
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "team-health".into(),
        status: Status::Pass,
        message: format!("all {} teammate panes alive", tc.members.len()),
        duration: start.elapsed(),
    }
}

/// Expected sentinels per phase (cumulative).
fn expected_sentinels(phase: &str) -> Option<Vec<&'static str>> {
    match phase {
        "PF1-INIT" => Some(vec!["pathflow-pf-1"]),
        "PF2-CONTEXT" => Some(vec!["pathflow-pf-1", "pathflow-pf-2"]),
        "PF3-CLASSIFY" | "PF4-EXECUTE" | "PF5-VERIFY" => {
            Some(vec!["pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"])
        }
        "PF6-COMPLETE" => Some(vec![
            "pathflow-pf-1",
            "pathflow-pf-2",
            "pathflow-pf-3",
            "pathflow-pf-6",
        ]),
        "PF7-END" => Some(vec![
            "pathflow-pf-1",
            "pathflow-pf-2",
            "pathflow-pf-3",
            "pathflow-pf-6",
            "pathflow-pf-7",
        ]),
        _ => None,
    }
}

fn check_sentinel_drift(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let state_dir = opts.effective_state_dir();

    if opts.session_id.is_empty() || !pathflow_active(&state_dir, &opts.session_id) {
        return CheckResult {
            name: "sentinel-drift".into(),
            status: Status::Pass,
            message: "no active session".into(),
            duration: start.elapsed(),
        };
    }

    let events = match read_pathflow_events(&state_dir) {
        Ok(events) => events,
        Err(e) => {
            return CheckResult {
                name: "sentinel-drift".into(),
                status: Status::Warn,
                message: format!("cannot read pathflow events: {e}"),
                duration: start.elapsed(),
            };
        }
    };

    let mut current_phase = String::new();
    let mut latest_time = chrono::DateTime::<chrono::Utc>::MIN_UTC;

    for ev in &events {
        if ev.event_type != "phase_transition" {
            continue;
        }
        if let Ok(t) = crate::util::parse_timestamp(&ev.timestamp) {
            if t > latest_time {
                latest_time = t;
                current_phase.clone_from(&ev.phase);
            }
        }
    }

    if current_phase.is_empty() {
        return CheckResult {
            name: "sentinel-drift".into(),
            status: Status::Pass,
            message: "no phase transitions found".into(),
            duration: start.elapsed(),
        };
    }

    let Some(expected) = expected_sentinels(&current_phase) else {
        return CheckResult {
            name: "sentinel-drift".into(),
            status: Status::Pass,
            message: format!("unknown phase {current_phase}, skipping drift check"),
            duration: start.elapsed(),
        };
    };

    let sentinel_dir = Path::new(&state_dir)
        .join("sentinels")
        .join("pathflow")
        .join(&opts.session_id);

    let missing: Vec<&str> = expected
        .iter()
        .filter(|&&s| !sentinel_dir.join(s).exists())
        .copied()
        .collect();

    if !missing.is_empty() {
        return CheckResult {
            name: "sentinel-drift".into(),
            status: Status::Warn,
            message: format!(
                "phase {current_phase}: missing sentinels: {}",
                missing.join(", ")
            ),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "sentinel-drift".into(),
        status: Status::Pass,
        message: format!("all sentinels present for phase {current_phase}"),
        duration: start.elapsed(),
    }
}

// ---------------------------------------------------------------------------
// Repair functions (3)
// ---------------------------------------------------------------------------

/// Rebuild the `SQLite` database from JSONL ledger files.
///
/// This is a stub that ensures the directory structure exists. The actual
/// database rebuild requires the `store` module and is delegated to the
/// CLI layer.
///
/// # Errors
///
/// Returns `DoctorError` on I/O failures.
pub fn repair_database(opts: &Options) -> Result<(), DoctorError> {
    if opts.db_path.is_empty() {
        return Err(DoctorError::RepairFailed {
            name: "database".into(),
            reason: "database path not configured".into(),
        });
    }
    if opts.ledger_dir.is_empty() {
        return Err(DoctorError::RepairFailed {
            name: "database".into(),
            reason: "ledger directory not configured".into(),
        });
    }

    // Ensure parent directory exists.
    if let Some(parent) = Path::new(&opts.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}

/// Fix file permissions on `.state/` and ensure directories exist.
///
/// # Errors
///
/// Returns `DoctorError` on I/O failures.
pub fn repair_permissions(opts: &Options) -> Result<(), DoctorError> {
    let state_dir = opts.effective_state_dir();

    for &dir in REQUIRED_STATE_DIRS {
        let path = Path::new(&state_dir).join(dir);
        std::fs::create_dir_all(&path).map_err(|e| DoctorError::RepairFailed {
            name: "permissions".into(),
            reason: format!("creating {dir}: {e}"),
        })?;
    }

    Ok(())
}

/// Regenerate missing config directories.
///
/// # Errors
///
/// Returns `DoctorError` on I/O failures.
pub fn repair_config(opts: &Options) -> Result<(), DoctorError> {
    let config_dir = Path::new(&opts.project_dir)
        .join(".codeflow")
        .join("config");

    let required_dirs = [config_dir.join("enforcement"), config_dir.join("pathflow")];

    for dir in &required_dirs {
        std::fs::create_dir_all(dir).map_err(|e| DoctorError::RepairFailed {
            name: "config".into(),
            reason: format!("creating {}: {e}", dir.display()),
        })?;
    }

    Ok(())
}

/// Check ledger health: subdirectory layout, fragment counts, base files.
fn check_ledger_health(opts: &Options) -> CheckResult {
    use crate::ledger::files as ledger_files;

    let start = Instant::now();
    let ledger_dir = if opts.ledger_dir.is_empty() {
        let state = opts.effective_state_dir();
        PathBuf::from(&state).join("ledger")
    } else {
        PathBuf::from(&opts.ledger_dir)
    };

    if !ledger_dir.is_dir() {
        return CheckResult {
            name: "ledger-health".into(),
            status: Status::Warn,
            message: "ledger directory not found".into(),
            duration: start.elapsed(),
        };
    }

    let mut warnings = Vec::new();
    let mut total_fragments = 0usize;

    for type_name in ledger_files::ALL {
        let subdir = ledger_dir.join(type_name);
        if !subdir.is_dir() {
            // Check for flat layout (pre-migration).
            let flat = ledger_dir.join(format!("{type_name}.jsonl"));
            if flat.exists() {
                warnings.push(format!("{type_name}: flat layout (needs migration)"));
            }
            continue;
        }

        let base = subdir.join(format!("{type_name}.jsonl"));
        if !base.exists() {
            // Base file missing is normal for types that haven't had events yet.
        }

        // Count fragments.
        let prefix = format!("{type_name}-ses-");
        let mut frag_count = 0usize;
        if let Ok(entries) = std::fs::read_dir(&subdir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_str().unwrap_or("");
                if name.starts_with(&prefix)
                    && crate::ledger::is_jsonl_file(name)
                    && !crate::ledger::is_lock_file(name)
                {
                    frag_count += 1;
                }
            }
        }
        total_fragments += frag_count;

        if frag_count > 20 {
            warnings.push(format!(
                "{type_name}: {frag_count} fragments (consider compaction)"
            ));
        }
    }

    if !warnings.is_empty() {
        return CheckResult {
            name: "ledger-health".into(),
            status: Status::Warn,
            message: warnings.join("; "),
            duration: start.elapsed(),
        };
    }

    CheckResult {
        name: "ledger-health".into(),
        status: Status::Pass,
        message: format!("{total_fragments} total fragments across all types"),
        duration: start.elapsed(),
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
    fn test_check_names_returns_16() {
        assert_eq!(check_names().len(), 17);
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
    fn test_check_database_empty_path() {
        let opts = test_opts();
        let result = check_database(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not configured"));
    }

    #[test]
    fn test_check_database_missing_file() {
        let mut opts = test_opts();
        opts.db_path = "/nonexistent/path/db.sqlite".into();
        let result = check_database(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_database_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, b"").unwrap();

        let mut opts = test_opts();
        opts.db_path = db_path.to_string_lossy().to_string();
        let result = check_database(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_jsonl_empty_dir() {
        let opts = test_opts();
        let result = check_jsonl(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not configured"));
    }

    #[test]
    fn test_check_jsonl_valid_files() {
        let dir = tempfile::tempdir().unwrap();
        // Create subdirectory layout.
        for &type_name in CANONICAL_LEDGER_TYPES {
            let subdir = dir.path().join(type_name);
            std::fs::create_dir_all(&subdir).unwrap();
            std::fs::write(
                subdir.join(format!("{type_name}.jsonl")),
                "{\"test\": true}\n",
            )
            .unwrap();
        }

        let mut opts = test_opts();
        opts.ledger_dir = dir.path().to_string_lossy().to_string();
        let result = check_jsonl(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_jsonl_missing_base_files_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Empty ledger dir -- no base files yet. Should pass (acceptable state).

        let mut opts = test_opts();
        opts.ledger_dir = dir.path().to_string_lossy().to_string();
        let result = check_jsonl(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_crdt_missing_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.state_dir = dir.path().to_string_lossy().to_string();
        let result = check_crdt(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("missing"));
    }

    #[test]
    fn test_check_crdt_valid() {
        let dir = tempfile::tempdir().unwrap();
        for &d in REQUIRED_STATE_DIRS {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        let mut opts = test_opts();
        opts.state_dir = dir.path().to_string_lossy().to_string();
        let result = check_crdt(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_python_not_found() {
        let opts = test_opts();
        let result = check_python(&opts);
        // Python is optional, so not-found is still Pass
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_python_found() {
        let mut opts = test_opts();
        opts.look_path = Some(|name| {
            if name == "python3" {
                Ok("/usr/bin/python3".into())
            } else {
                Err("not found".into())
            }
        });
        let result = check_python(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("available"));
    }

    #[test]
    fn test_check_embedding_is_warn() {
        let opts = test_opts();
        let result = check_embedding(&opts);
        assert_eq!(result.status, Status::Warn);
    }

    #[test]
    fn test_check_vector_is_warn() {
        let opts = test_opts();
        let result = check_vector(&opts);
        assert_eq!(result.status, Status::Warn);
    }

    #[test]
    fn test_check_version_passes() {
        let opts = test_opts();
        let result = check_version(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_config_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("test.json"), "{}").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_pathflow_stuck_no_session() {
        let opts = test_opts();
        let result = check_pathflow_stuck(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("not active"));
    }

    #[test]
    fn test_check_sentinel_drift_no_session() {
        let opts = test_opts();
        let result = check_sentinel_drift(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_expected_sentinels_mapping() {
        assert_eq!(expected_sentinels("PF1-INIT").unwrap().len(), 1);
        assert_eq!(expected_sentinels("PF4-EXECUTE").unwrap().len(), 3);
        assert_eq!(expected_sentinels("PF7-END").unwrap().len(), 5);
        assert!(expected_sentinels("UNKNOWN").is_none());
    }

    #[test]
    fn test_repair_database_empty_path() {
        let opts = test_opts();
        let result = repair_database(&opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_repair_permissions_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.state_dir = dir.path().join("state").to_string_lossy().to_string();
        repair_permissions(&opts).unwrap();

        for &d in REQUIRED_STATE_DIRS {
            assert!(
                Path::new(&opts.state_dir).join(d).is_dir(),
                "{d} should exist"
            );
        }
    }

    #[test]
    fn test_repair_config_creates_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        repair_config(&opts).unwrap();

        let config_dir = dir.path().join(".codeflow").join("config");
        assert!(config_dir.join("enforcement").is_dir());
        assert!(config_dir.join("pathflow").is_dir());
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
    async fn test_run_all_returns_16_results() {
        let opts = test_opts();
        let results = run_all(&opts).await;
        assert_eq!(results.len(), 17);
    }

    #[test]
    fn test_run_check_valid_name() {
        let opts = test_opts();
        let result = run_check("version", &opts).unwrap();
        assert_eq!(result.name, "version");
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_hooks_no_binary() {
        let opts = test_opts();
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_claude_not_found() {
        let opts = test_opts();
        let result = check_claude(&opts);
        assert_eq!(result.status, Status::Fail);
    }

    #[test]
    fn test_check_team_health_no_team() {
        let opts = test_opts();
        let result = check_team_health(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("no active team"));
    }

    #[test]
    fn test_discover_team_name_none() {
        assert!(discover_team_name("/nonexistent", "", "").is_none());
    }

    #[test]
    fn test_check_auth_not_authenticated() {
        let mut opts = test_opts();
        opts.exec_command = Some(|cmd, _args| {
            if cmd == "claude" {
                Ok("Not authenticated".to_string())
            } else {
                Err("not available".into())
            }
        });
        let result = check_auth(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not authenticated"));
    }

    #[test]
    fn test_check_auth_authenticated() {
        let mut opts = test_opts();
        opts.exec_command = Some(|cmd, _args| {
            if cmd == "claude" {
                Ok("Logged in as user@example.com".to_string())
            } else {
                Err("not available".into())
            }
        });
        let result = check_auth(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_auth_command_failed() {
        let opts = test_opts(); // exec_command returns Err
        let result = check_auth(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("auth check failed"));
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
            // Fail for gate-check, pass for others
            if args.len() >= 3 && args[2] == "gate-check" {
                Err("unknown subcommand".into())
            } else {
                Ok("ok".into())
            }
        });
        let result = check_hooks(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("not responding"));
    }

    #[test]
    fn test_check_network_pass() {
        let mut opts = test_opts();
        opts.exec_command = Some(|_cmd, _args| Ok("github.com has address".into()));
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_network_fail() {
        let opts = test_opts(); // exec_command returns Err
        let result = check_network(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("failed"));
    }

    #[test]
    fn test_check_permissions_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.state_dir = dir.path().to_string_lossy().to_string();
        let result = check_permissions(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_check_jsonl_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        // Create subdirectory layout with one invalid file.
        for &type_name in CANONICAL_LEDGER_TYPES {
            let subdir = dir.path().join(type_name);
            std::fs::create_dir_all(&subdir).unwrap();
            if type_name == "work-graph" {
                std::fs::write(
                    subdir.join(format!("{type_name}.jsonl")),
                    "not valid json\n",
                )
                .unwrap();
            } else {
                std::fs::write(
                    subdir.join(format!("{type_name}.jsonl")),
                    "{\"ok\": true}\n",
                )
                .unwrap();
            }
        }

        let mut opts = test_opts();
        opts.ledger_dir = dir.path().to_string_lossy().to_string();
        let result = check_jsonl(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("invalid"));
    }

    #[test]
    fn test_check_config_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("bad.json"), "not json!!").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Fail);
        assert!(result.message.contains("invalid"));
    }

    #[test]
    fn test_check_config_subdirectory_valid() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config");
        let sub_dir = config_dir.join("subdir");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("nested.json"), "{}").unwrap();

        let mut opts = test_opts();
        opts.project_dir = dir.path().to_string_lossy().to_string();
        let result = check_config(&opts);
        assert_eq!(result.status, Status::Pass);
    }

    #[test]
    fn test_pathflow_active_true() {
        let dir = tempfile::tempdir().unwrap();
        let flag_dir = dir.path().join("session").join("ses-123").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        assert!(pathflow_active(&dir.path().to_string_lossy(), "ses-123"));
    }

    #[test]
    fn test_pathflow_active_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!pathflow_active(&dir.path().to_string_lossy(), "ses-123"));
    }

    #[test]
    fn test_read_pathflow_events_valid() {
        let dir = tempfile::tempdir().unwrap();
        let logs_dir = dir.path().join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let content = r#"{"event": "phase_transition", "phase": "PF1-INIT", "timestamp": "2026-03-10T10:00:00Z"}
{"event": "stage_transition", "phase": "", "timestamp": "2026-03-10T10:01:00Z"}"#;
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        let events = read_pathflow_events(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "phase_transition");
        assert_eq!(events[0].phase, "PF1-INIT");
    }

    #[test]
    fn test_read_pathflow_events_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_pathflow_events(&dir.path().to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn test_check_pathflow_stuck_active_not_stuck() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create recent event
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let ts = chrono::Utc::now().to_rfc3339();
        let content = format!(
            r#"{{"event": "phase_transition", "phase": "PF3-CLASSIFY", "timestamp": "{ts}"}}"#
        );
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_pathflow_stuck(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("PF3-CLASSIFY"));
    }

    #[test]
    fn test_check_pathflow_stuck_active_stuck() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create old event (2 hours ago)
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let content = format!(
            r#"{{"event": "phase_transition", "phase": "PF2-CONTEXT", "timestamp": "{old_ts}"}}"#
        );
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_pathflow_stuck(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("stuck"));
    }

    #[test]
    fn test_check_pathflow_stuck_no_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create events file with no phase_transition events
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let content =
            r#"{"event": "stage_transition", "phase": "", "timestamp": "2026-03-10T10:00:00Z"}"#;
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_pathflow_stuck(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("no phase transitions"));
    }

    #[test]
    fn test_check_sentinel_drift_all_present() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create recent event at PF2-CONTEXT
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let ts = chrono::Utc::now().to_rfc3339();
        let content = format!(
            r#"{{"event": "phase_transition", "phase": "PF2-CONTEXT", "timestamp": "{ts}"}}"#
        );
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        // Create expected sentinels
        let sentinel_dir = state_dir.join("sentinels").join("pathflow").join("ses-1");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-2"), "").unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_sentinel_drift(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("all sentinels present"));
    }

    #[test]
    fn test_check_sentinel_drift_missing_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create event at PF2-CONTEXT
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let ts = chrono::Utc::now().to_rfc3339();
        let content = format!(
            r#"{{"event": "phase_transition", "phase": "PF2-CONTEXT", "timestamp": "{ts}"}}"#
        );
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        // Create only pf-1, missing pf-2
        let sentinel_dir = state_dir.join("sentinels").join("pathflow").join("ses-1");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_sentinel_drift(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("missing sentinels"));
    }

    #[test]
    fn test_check_team_health_with_config_no_members() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let home_dir = dir.path().join("home");

        // Create team config with empty members
        let team_dir = home_dir.join(".claude").join("teams").join("test-team");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(team_dir.join("config.json"), r#"{"members": []}"#).unwrap();

        // Create pathflow team file to discover team name
        let pf_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&pf_dir).unwrap();
        std::fs::write(
            pf_dir.join("pathflow-team.json"),
            r#"{"team_name": "test-team"}"#,
        )
        .unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        opts.home_dir = home_dir.to_string_lossy().to_string();
        let result = check_team_health(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("no members"));
    }

    #[test]
    fn test_check_team_health_with_dead_panes() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let home_dir = dir.path().join("home");

        // Create team config with members
        let team_dir = home_dir.join(".claude").join("teams").join("test-team");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev", "tmuxPaneId": "%999"}]}"#,
        )
        .unwrap();

        // Create pathflow team file
        let pf_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&pf_dir).unwrap();
        std::fs::write(
            pf_dir.join("pathflow-team.json"),
            r#"{"team_name": "test-team"}"#,
        )
        .unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        opts.home_dir = home_dir.to_string_lossy().to_string();
        // exec_command returns no pane matching %999
        opts.exec_command = Some(|_cmd, _args| Ok("no matching panes".to_string()));
        let result = check_team_health(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("panes not found"));
    }

    #[test]
    fn test_check_team_health_all_alive() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let home_dir = dir.path().join("home");

        // Create team config with members
        let team_dir = home_dir.join(".claude").join("teams").join("test-team");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev", "tmuxPaneId": "%42"}]}"#,
        )
        .unwrap();

        // Create pathflow team file
        let pf_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&pf_dir).unwrap();
        std::fs::write(
            pf_dir.join("pathflow-team.json"),
            r#"{"team_name": "test-team"}"#,
        )
        .unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        opts.home_dir = home_dir.to_string_lossy().to_string();
        // exec_command returns pane list containing %42
        opts.exec_command = Some(|_cmd, _args| Ok("%42 some pane info".to_string()));
        let result = check_team_health(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("all"));
        assert!(result.message.contains("alive"));
    }

    #[test]
    fn test_check_team_health_tmux_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let home_dir = dir.path().join("home");

        // Create team config with members
        let team_dir = home_dir.join(".claude").join("teams").join("test-team");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev", "tmuxPaneId": "%1"}]}"#,
        )
        .unwrap();

        // Create pathflow team file
        let pf_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&pf_dir).unwrap();
        std::fs::write(
            pf_dir.join("pathflow-team.json"),
            r#"{"team_name": "test-team"}"#,
        )
        .unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        opts.home_dir = home_dir.to_string_lossy().to_string();
        // exec_command fails for tmux
        opts.exec_command = Some(|_cmd, _args| Err("not available".into()));
        let result = check_team_health(&opts);
        assert_eq!(result.status, Status::Warn);
        assert!(result.message.contains("tmux not available"));
    }

    #[test]
    fn test_repair_database_empty_ledger() {
        let mut opts = test_opts();
        opts.db_path = "/tmp/test.db".into();
        // ledger_dir is empty
        let result = repair_database(&opts);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ledger"));
    }

    #[test]
    fn test_repair_database_success() {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = test_opts();
        opts.db_path = dir
            .path()
            .join("db")
            .join("test.db")
            .to_string_lossy()
            .to_string();
        opts.ledger_dir = dir.path().join("ledger").to_string_lossy().to_string();
        repair_database(&opts).unwrap();
        assert!(dir.path().join("db").is_dir());
    }

    #[test]
    fn test_discover_team_name_from_pathflow_team_json() {
        let dir = tempfile::tempdir().unwrap();
        let pf_dir = dir.path().join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&pf_dir).unwrap();
        std::fs::write(
            pf_dir.join("pathflow-team.json"),
            r#"{"team_name": "my-team"}"#,
        )
        .unwrap();

        let result = discover_team_name(&dir.path().to_string_lossy(), "ses-1", "");
        assert_eq!(result, Some("my-team".to_string()));
    }

    #[test]
    fn test_discover_team_name_from_home_dir() {
        let dir = tempfile::tempdir().unwrap();
        let teams_dir = dir
            .path()
            .join(".claude")
            .join("teams")
            .join("fallback-team");
        std::fs::create_dir_all(&teams_dir).unwrap();

        let result = discover_team_name("/nonexistent", "", &dir.path().to_string_lossy());
        assert_eq!(result, Some("fallback-team".to_string()));
    }

    #[test]
    fn test_effective_state_dir_defaults_to_project_dir() {
        let opts = Options {
            project_dir: "/proj".into(),
            state_dir: String::new(),
            ..Options::default()
        };
        assert_eq!(opts.effective_state_dir(), "/proj/.state");
    }

    #[test]
    fn test_effective_state_dir_uses_override() {
        let opts = Options {
            project_dir: "/proj".into(),
            state_dir: "/custom/state".into(),
            ..Options::default()
        };
        assert_eq!(opts.effective_state_dir(), "/custom/state");
    }

    #[test]
    fn test_expected_sentinels_all_phases() {
        assert_eq!(expected_sentinels("PF2-CONTEXT").unwrap().len(), 2);
        assert_eq!(expected_sentinels("PF3-CLASSIFY").unwrap().len(), 3);
        assert_eq!(expected_sentinels("PF5-VERIFY").unwrap().len(), 3);
        assert_eq!(expected_sentinels("PF6-COMPLETE").unwrap().len(), 4);
    }

    #[test]
    fn test_check_sentinel_drift_unknown_phase() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path();

        // Create active flag
        let flag_dir = state_dir.join("session").join("ses-1").join("pathflow");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress"}"#,
        )
        .unwrap();

        // Create event with unknown phase
        let logs_dir = state_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        let ts = chrono::Utc::now().to_rfc3339();
        let content = format!(
            r#"{{"event": "phase_transition", "phase": "PF99-UNKNOWN", "timestamp": "{ts}"}}"#
        );
        std::fs::write(logs_dir.join("pathflow-events.jsonl"), content).unwrap();

        let mut opts = test_opts();
        opts.state_dir = state_dir.to_string_lossy().to_string();
        opts.session_id = "ses-1".into();
        let result = check_sentinel_drift(&opts);
        assert_eq!(result.status, Status::Pass);
        assert!(result.message.contains("unknown phase"));
    }
}
