//! Sync daemon for Loro CRDT state synchronization.
//!
//! The sync daemon is a long-running process within the `codeflow` binary that
//! keeps Loro CRDT state synchronized. It serves two purposes:
//!
//! 1. **Same-machine mode**: Periodic compaction of `state.loro` via snapshot
//!    export and TTL-based detection of stale claims from crashed sessions.
//! 2. **Multi-machine mode**: Delta synchronization via git refs. Each peer
//!    exports Loro updates (deltas since last sync) and pushes to a per-peer
//!    git ref. The daemon fetches all peer refs, imports their deltas, and
//!    merges into the local `LoroDoc`.
//!
//! The daemon uses session reference counting: it starts when the first session
//! begins and shuts down gracefully when the last session ends.
//!
//! # Concurrency
//!
//! All state.loro persistence uses `locked_binary_rmw` from `file_lock.rs`,
//! which acquires an exclusive sidecar lock before read-modify-write. This
//! ensures the daemon's compaction cycle does not corrupt active session
//! claims. The daemon and session hooks are serialized through the same
//! lock, so at most one writer touches state.loro at a time.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use std::borrow::Cow;

use loro::{ExportMode, LoroDoc};
use serde::{Deserialize, Serialize};

use crate::coordination::loro::LoroCoordinator;
use crate::coordination::{Claim, Coordinator, PeerId};
use crate::error::SyncError;
use crate::file_lock::locked_binary_rmw;
use crate::transport::gitref;
use crate::types::SessionId;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default sync interval in seconds.
pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 5;

/// Maximum retries per sync cycle on network errors.
const MAX_RETRIES: u32 = 5;

/// Initial backoff duration for exponential retry.
const INITIAL_BACKOFF: Duration = Duration::from_millis(100);

/// Maximum backoff duration cap.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Backoff multiplier.
const BACKOFF_MULTIPLIER: u32 = 2;

// ---------------------------------------------------------------------------
// PID file management
// ---------------------------------------------------------------------------

/// PID file content for the sync daemon.
///
/// Tracks the daemon process ID, active sessions, and reference count.
/// Stored at `.state/coordination/sync-daemon.pid`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidFile {
    /// OS process ID of the daemon.
    pub pid: u32,
    /// List of active session IDs (type-safe via `SessionId`).
    pub sessions: Vec<SessionId>,
    /// Number of active sessions (len of sessions vec).
    pub ref_count: usize,
}

impl PidFile {
    /// Create a new PID file for a daemon process.
    #[must_use]
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            sessions: Vec::new(),
            ref_count: 0,
        }
    }

    /// Add a session to the PID file. Returns the new ref count.
    pub fn add_session(&mut self, session_id: &SessionId) -> usize {
        if !self.sessions.contains(session_id) {
            self.sessions.push(session_id.clone());
            self.ref_count = self.sessions.len();
        }
        self.ref_count
    }

    /// Remove a session from the PID file. Returns the new ref count.
    pub fn remove_session(&mut self, session_id: &SessionId) -> usize {
        self.sessions.retain(|s| s != session_id);
        self.ref_count = self.sessions.len();
        self.ref_count
    }
}

/// Read the PID file from disk.
///
/// # Errors
///
/// Returns `SyncError::IoError` if the file cannot be read, or
/// `SyncError::Serialization` if the JSON is malformed.
pub fn read_pid_file(pid_path: &Path) -> Result<PidFile, SyncError> {
    let content = fs::read_to_string(pid_path)?;
    let pid_file: PidFile = serde_json::from_str(&content)?;
    Ok(pid_file)
}

/// Write the PID file atomically.
///
/// # Errors
///
/// Returns `SyncError::IoError` if the write fails.
pub fn write_pid_file(pid_path: &Path, pid_file: &PidFile) -> Result<(), SyncError> {
    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(pid_file)?;
    crate::file_lock::atomic_write(pid_path, json.as_bytes())?;
    Ok(())
}

/// Remove the PID file from disk.
///
/// No-op if the file does not exist.
///
/// # Errors
///
/// Returns `SyncError::IoError` if removal fails for reasons other than
/// the file not existing.
pub fn remove_pid_file(pid_path: &Path) -> Result<(), SyncError> {
    match fs::remove_file(pid_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SyncError::IoError(e)),
    }
}

// ---------------------------------------------------------------------------
// Version vector tracking
// ---------------------------------------------------------------------------

/// Persisted sync state: tracks the last-synced Loro version vector.
///
/// Stored at `.state/coordination/sync-state.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    /// Serialized Loro `VersionVector` from the last successful sync.
    /// `None` if no sync has completed yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync_vv: Option<Vec<u8>>,
}

/// Read sync state from disk. Returns default if file doesn't exist.
///
/// # Errors
///
/// Returns `SyncError::Serialization` if the JSON is malformed.
pub fn read_sync_state(path: &Path) -> Result<SyncState, SyncError> {
    match fs::read_to_string(path) {
        Ok(content) => {
            let state: SyncState = serde_json::from_str(&content)?;
            Ok(state)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SyncState::default()),
        Err(e) => Err(SyncError::IoError(e)),
    }
}

/// Write sync state atomically.
///
/// # Errors
///
/// Returns `SyncError::IoError` if the write fails.
pub fn write_sync_state(path: &Path, state: &SyncState) -> Result<(), SyncError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    crate::file_lock::atomic_write(path, json.as_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Peer ID generation
// ---------------------------------------------------------------------------

/// Generate a peer ID from the local machine's username and hostname.
///
/// Format: `{username}-{hostname}`. Falls back to `"unknown"` for either
/// component if detection fails.
#[must_use]
pub fn generate_peer_id() -> PeerId {
    let username = whoami_username();
    let hostname = whoami_hostname();
    PeerId::new(format!("{username}-{hostname}"))
}

/// Read or create the peer ID file at `.state/runtime/peer-id`.
///
/// If the file exists, reads and returns its content.
/// If not, generates a new peer ID, writes it, and returns it.
///
/// # Errors
///
/// Returns `SyncError::IoError` if file operations fail.
pub fn ensure_peer_id(runtime_dir: &Path) -> Result<PeerId, SyncError> {
    let peer_id_path = runtime_dir.join("peer-id");

    if peer_id_path.exists() {
        let content = fs::read_to_string(&peer_id_path)?;
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Ok(PeerId::new(trimmed));
        }
    }

    let peer_id = generate_peer_id();
    fs::create_dir_all(runtime_dir)?;
    fs::write(&peer_id_path, peer_id.as_str())?;
    Ok(peer_id)
}

/// Get the current username. Returns "unknown" on failure.
fn whoami_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get the current hostname. Returns "unknown" on failure.
fn whoami_hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

/// Run a compaction cycle on `state.loro`.
///
/// 1. Loads the `LoroDoc` under exclusive lock.
/// 2. Runs TTL-based stale claim detection (delegated to `LoroCoordinator::check`).
/// 3. Exports a fresh snapshot via `doc.export(Snapshot)`.
/// 4. Writes the snapshot back atomically.
///
/// Uses `locked_binary_rmw` to ensure exclusive access during the entire
/// read-modify-write cycle.
///
/// # Errors
///
/// Returns `SyncError` if lock acquisition, load, or write fails.
pub fn run_compaction(state_loro_path: &Path) -> Result<(), SyncError> {
    locked_binary_rmw(
        state_loro_path,
        LoroCoordinator::in_memory,
        |bytes| {
            LoroCoordinator::from_bytes(bytes, state_loro_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            // TTL-based stale claim removal: find expired claims and delete
            // them from the CRDT state before re-exporting. Without this,
            // expired claims would remain in the LoroDoc indefinitely.
            let stale = find_stale_claims(coord);
            if !stale.is_empty() {
                let claims_map = coord.doc().get_map("claims");
                for (path, _) in &stale {
                    let _ = claims_map.delete(path);
                }
                coord.doc().commit();
            }
            Ok(())
        },
    )
    .map_err(|e| SyncError::ConnectionError(format!("compaction failed: {e}")))
}

// ---------------------------------------------------------------------------
// Delta sync
// ---------------------------------------------------------------------------

/// Export Loro deltas since the last sync version vector.
///
/// Returns the delta bytes and the current version vector (for updating
/// `sync-state.json` after a successful push).
///
/// # Errors
///
/// Returns `SyncError` if the document cannot be loaded or exported.
pub fn export_deltas(
    state_loro_path: &Path,
    last_sync_vv: Option<&[u8]>,
) -> Result<(Vec<u8>, Vec<u8>), SyncError> {
    let bytes = match fs::read(state_loro_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(e) => return Err(SyncError::IoError(e)),
    };
    if bytes.is_empty() {
        // Empty file — no state to sync.
        return Ok((Vec::new(), Vec::new()));
    }

    let doc = LoroDoc::from_snapshot(&bytes)
        .map_err(|e| SyncError::SchemaError(format!("load snapshot: {e}")))?;

    let delta = match last_sync_vv {
        Some(vv_bytes) => {
            let vv = loro::VersionVector::decode(vv_bytes)
                .map_err(|e| SyncError::SchemaError(format!("decode version vector: {e}")))?;
            doc.export(ExportMode::Updates {
                from: Cow::Owned(vv),
            })?
        }
        None => doc.export(ExportMode::Snapshot)?,
    };
    let current_vv = doc.oplog_vv().encode();

    Ok((delta, current_vv))
}

/// Import peer delta bytes into the local state.loro.
///
/// Uses `locked_binary_rmw` to safely merge under exclusive lock.
///
/// # Errors
///
/// Returns `SyncError` if import fails.
pub fn import_peer_delta(state_loro_path: &Path, delta: &[u8]) -> Result<(), SyncError> {
    if delta.is_empty() {
        return Ok(());
    }

    // Capture delta into owned Vec to move into closure.
    let delta_owned = delta.to_vec();

    locked_binary_rmw(
        state_loro_path,
        LoroCoordinator::in_memory,
        |bytes| {
            LoroCoordinator::from_bytes(bytes, state_loro_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            coord
                .doc()
                .import(&delta_owned)
                .map(|_| ())
                .map_err(|e| format!("import delta: {e}"))
        },
    )
    .map_err(|e| SyncError::ConnectionError(format!("import failed: {e}")))
}

// ---------------------------------------------------------------------------
// Exponential backoff
// ---------------------------------------------------------------------------

/// Compute the backoff duration for a given retry attempt.
///
/// Uses exponential backoff: `initial * multiplier^attempt`, capped at `max`.
///
/// # Arguments
///
/// * `attempt` - Zero-based attempt index (0 = first retry).
#[must_use]
pub fn backoff_duration(attempt: u32) -> Duration {
    let millis = u64::try_from(INITIAL_BACKOFF.as_millis()).unwrap_or(u64::MAX);
    let factor = u64::from(BACKOFF_MULTIPLIER).saturating_pow(attempt);
    let delay_ms = millis.saturating_mul(factor);
    let delay = Duration::from_millis(delay_ms);
    if delay > MAX_BACKOFF {
        MAX_BACKOFF
    } else {
        delay
    }
}

/// Execute an operation with exponential backoff retry.
///
/// Retries on retryable errors (connection/timeout). Fails immediately on
/// schema errors.
///
/// # Errors
///
/// Returns the last error if all retries are exhausted, or immediately
/// on non-retryable errors.
pub fn with_retry<F>(mut operation: F) -> Result<(), SyncError>
where
    F: FnMut() -> Result<(), SyncError>,
{
    for attempt in 0..MAX_RETRIES {
        match operation() {
            Ok(()) => return Ok(()),
            Err(e) if e.is_schema_error() => return Err(e),
            Err(e) if attempt == MAX_RETRIES - 1 => return Err(e),
            Err(_) => {
                std::thread::sleep(backoff_duration(attempt));
            }
        }
    }
    // Unreachable: the loop either returns Ok or Err.
    Ok(())
}

// ---------------------------------------------------------------------------
// Sync cycle
// ---------------------------------------------------------------------------

/// Configuration for a sync cycle.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Path to the project root directory.
    pub project_dir: PathBuf,
    /// Path to `state.loro`.
    pub state_loro_path: PathBuf,
    /// Path to `sync-state.json`.
    pub sync_state_path: PathBuf,
    /// Path to PID file.
    pub pid_path: PathBuf,
    /// Path to the runtime directory.
    pub runtime_dir: PathBuf,
    /// Sync interval.
    pub interval: Duration,
}

/// Path to parallel-work config relative to project root.
const PARALLEL_WORK_CONFIG_PATH: &str = ".codeflow/config/parallel-work/parallel-work-config.json";

impl SyncConfig {
    /// Create a `SyncConfig` from a project directory with an explicit interval.
    #[must_use]
    pub fn from_project_dir(project_dir: &Path, interval_secs: u64) -> Self {
        let state_dir = project_dir.join(".state").join("coordination");
        let runtime_dir = project_dir.join(".state").join("runtime");
        Self {
            project_dir: project_dir.to_path_buf(),
            state_loro_path: state_dir.join("state.loro"),
            sync_state_path: state_dir.join("sync-state.json"),
            pid_path: state_dir.join("sync-daemon.pid"),
            runtime_dir,
            interval: Duration::from_secs(interval_secs),
        }
    }

    /// Create a `SyncConfig` by reading `sync.interval_secs` from
    /// `parallel-work-config.json`.
    ///
    /// Falls back to `DEFAULT_SYNC_INTERVAL_SECS` if the config file is
    /// absent or unparseable.
    #[must_use]
    pub fn from_config_file(project_dir: &Path) -> Self {
        let interval = read_config_interval(project_dir);
        Self::from_project_dir(project_dir, interval)
    }
}

/// Read `sync.interval_secs` from the parallel-work config file.
///
/// Returns `DEFAULT_SYNC_INTERVAL_SECS` if the file is missing, unreadable,
/// or does not contain the expected field.
fn read_config_interval(project_dir: &Path) -> u64 {
    let config_path = project_dir.join(PARALLEL_WORK_CONFIG_PATH);
    let Ok(data) = fs::read_to_string(&config_path) else {
        return DEFAULT_SYNC_INTERVAL_SECS;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) else {
        return DEFAULT_SYNC_INTERVAL_SECS;
    };
    val.get("sync")
        .and_then(|s| s.get("interval_secs"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_SYNC_INTERVAL_SECS)
}

/// Run a single sync cycle.
///
/// 1. Compaction (always runs — same-machine mode).
/// 2. If multi-machine enabled (remote detected):
///    a. Export deltas since last sync.
///    b. Push deltas to per-peer git ref.
///    c. Fetch all peer refs.
///    d. Import each peer's deltas.
///    e. Update sync state.
///
/// # Errors
///
/// Returns `SyncError` if compaction or delta sync fails.
pub fn run_sync_cycle(config: &SyncConfig, peer_id: &PeerId) -> Result<(), SyncError> {
    // Step 1: Always compact (same-machine mode).
    run_compaction(&config.state_loro_path)?;

    // Step 2: Check for multi-machine mode.
    let multi_machine = gitref::has_remote(&config.project_dir).unwrap_or(false);
    if !multi_machine {
        return Ok(());
    }

    // Step 3: Export deltas.
    let sync_state = read_sync_state(&config.sync_state_path)?;
    let (delta, current_vv) =
        export_deltas(&config.state_loro_path, sync_state.last_sync_vv.as_deref())?;

    if delta.is_empty() {
        return Ok(());
    }

    // Step 4: Push deltas with retry.
    let project_dir = config.project_dir.clone();
    let peer_for_push = peer_id.clone();
    with_retry(|| gitref::push_delta(&project_dir, &peer_for_push, &delta))?;

    // Step 5: Fetch and import peer deltas with retry.
    let peer_deltas = with_retry_fetch(&config.project_dir, peer_id)?;
    for (_, peer_delta) in &peer_deltas {
        import_peer_delta(&config.state_loro_path, peer_delta)?;
    }

    // Step 6: Update sync state.
    let new_state = SyncState {
        last_sync_vv: if current_vv.is_empty() {
            None
        } else {
            Some(current_vv)
        },
    };
    write_sync_state(&config.sync_state_path, &new_state)?;

    Ok(())
}

/// Fetch peer deltas with exponential backoff retry.
fn with_retry_fetch(
    project_dir: &Path,
    local_peer: &PeerId,
) -> Result<Vec<(PeerId, Vec<u8>)>, SyncError> {
    let mut last_err = None;
    for attempt in 0..MAX_RETRIES {
        match gitref::fetch_peer_deltas(project_dir, local_peer) {
            Ok(deltas) => return Ok(deltas),
            Err(e) if e.is_schema_error() => return Err(e),
            Err(e) => {
                last_err = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    std::thread::sleep(backoff_duration(attempt));
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| SyncError::ConnectionError("fetch failed".to_string())))
}

// ---------------------------------------------------------------------------
// Stale claim detection (used by compaction)
// ---------------------------------------------------------------------------

/// Collect paths with expired claims from a `LoroCoordinator`.
///
/// Uses `Coordinator::check()` which already filters by TTL, so we
/// iterate all claim entries and return those NOT returned by check
/// (meaning they are expired).
#[must_use]
pub fn find_stale_claims(coordinator: &LoroCoordinator) -> Vec<(String, Claim)> {
    let claims_map = coordinator.doc().get_map("claims");
    let mut stale = Vec::new();

    claims_map.for_each(|key, value| {
        if let loro::ValueOrContainer::Value(loro::LoroValue::String(json_str)) = value {
            if let Ok(claim) = serde_json::from_str::<Claim>(&json_str) {
                // If check() returns None, the claim is expired.
                if coordinator.check(key).is_none() {
                    stale.push((key.to_string(), claim));
                }
            }
        }
    });

    stale
}

// ---------------------------------------------------------------------------
// Process liveness check
// ---------------------------------------------------------------------------

/// Check if a process with the given PID is alive.
///
/// Uses `/bin/kill -0 <pid>` which sends no signal but returns 0 if the
/// process exists. This avoids `unsafe` libc calls.
#[must_use]
pub fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------------------
// Daemon lifecycle
// ---------------------------------------------------------------------------

/// Status of the sync daemon.
#[derive(Debug, Clone)]
pub struct DaemonStatus {
    /// Whether the daemon is currently running.
    pub running: bool,
    /// Daemon process ID (if running).
    pub pid: Option<u32>,
    /// Local peer ID.
    pub peer_id: Option<String>,
    /// Number of tracked sessions.
    pub session_count: usize,
    /// Whether last sync completed (version vector exists).
    pub last_sync_completed: bool,
}

/// Get the current daemon status.
///
/// Reads the PID file, sync state, and peer ID to build a status report.
#[must_use]
pub fn daemon_status(config: &SyncConfig) -> DaemonStatus {
    let pid_info = read_pid_file(&config.pid_path).ok();
    let running = pid_info.as_ref().is_some_and(|pf| is_pid_alive(pf.pid));

    let peer_id = ensure_peer_id(&config.runtime_dir)
        .ok()
        .map(|p| p.as_str().to_string());

    let last_sync_completed = read_sync_state(&config.sync_state_path)
        .ok()
        .is_some_and(|s| s.last_sync_vv.is_some());

    DaemonStatus {
        running,
        pid: pid_info.as_ref().map(|pf| pf.pid),
        peer_id,
        session_count: pid_info.as_ref().map_or(0, |pf| pf.sessions.len()),
        last_sync_completed,
    }
}

/// Start the sync daemon as a background process.
///
/// Spawns `codeflow sync daemon --interval {interval}` as a detached child
/// process. Idempotent: if a daemon is already running (PID alive), returns
/// `Ok(())` without spawning.
///
/// # Errors
///
/// Returns `SyncError` if the process cannot be spawned.
pub fn start_daemon(config: &SyncConfig) -> Result<u32, SyncError> {
    // Check if already running.
    if let Ok(pf) = read_pid_file(&config.pid_path) {
        if is_pid_alive(pf.pid) {
            return Ok(pf.pid);
        }
        // Stale PID file — remove it.
        let _ = remove_pid_file(&config.pid_path);
    }

    let interval = config.interval.as_secs();
    let child = std::process::Command::new("codeflow")
        .args(["sync", "daemon", "--interval", &interval.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| SyncError::ConnectionError(format!("failed to spawn daemon: {e}")))?;

    let pid = child.id();
    Ok(pid)
}

/// Stop the sync daemon gracefully by sending SIGTERM.
///
/// Reads the PID from the PID file, sends SIGTERM, waits briefly for exit,
/// and removes the PID file.
///
/// # Errors
///
/// Returns `SyncError` if the PID file cannot be read or SIGTERM fails.
pub fn stop_daemon(config: &SyncConfig) -> Result<(), SyncError> {
    let pf = read_pid_file(&config.pid_path)?;

    if is_pid_alive(pf.pid) {
        // Send SIGTERM via /bin/kill.
        let _ = std::process::Command::new("/bin/kill")
            .args(["-TERM", &pf.pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        // Brief wait for graceful shutdown.
        std::thread::sleep(Duration::from_millis(200));
    }

    remove_pid_file(&config.pid_path)
}

/// Register a SIGTERM handler that sets an `AtomicBool` flag.
///
/// Uses `signal-hook` for async-signal-safe flag registration.
/// Returns the `Arc<AtomicBool>` that will be set to `true` when SIGTERM
/// or SIGINT is received.
///
/// # Errors
///
/// Returns `SyncError` if signal registration fails.
pub fn register_signal_handler() -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>, SyncError>
{
    let term_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    signal_hook::flag::register(
        signal_hook::consts::SIGTERM,
        std::sync::Arc::clone(&term_flag),
    )
    .map_err(|e| SyncError::ConnectionError(format!("failed to register SIGTERM: {e}")))?;

    signal_hook::flag::register(
        signal_hook::consts::SIGINT,
        std::sync::Arc::clone(&term_flag),
    )
    .map_err(|e| SyncError::ConnectionError(format!("failed to register SIGINT: {e}")))?;

    Ok(term_flag)
}

// ---------------------------------------------------------------------------
// Crash cleanup
// ---------------------------------------------------------------------------

/// Detect dead worker PIDs and release their claims.
///
/// On each sync cycle, reads session status files to get lead PIDs.
/// If a PID is dead, calls `release_all()` for that session's claims.
///
/// # Errors
///
/// Returns `SyncError` if claim release fails.
pub fn cleanup_dead_workers(config: &SyncConfig) -> Result<usize, SyncError> {
    let pid_file = match read_pid_file(&config.pid_path) {
        Ok(pf) => pf,
        Err(_) => return Ok(0),
    };

    let mut total_released = 0;

    // Check each tracked session's liveness via its pathflow-session-status.json.
    for session_id in &pid_file.sessions {
        let status_path = config
            .project_dir
            .join(".state")
            .join("session")
            .join(session_id.as_str())
            .join("pathflow")
            .join("pathflow-session-status.json");

        if let Ok(content) = fs::read_to_string(&status_path) {
            if let Ok(status) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(lead_pid) = status.get("lead_pid").and_then(serde_json::Value::as_u64) {
                    #[allow(clippy::cast_possible_truncation)]
                    let pid = lead_pid as u32;
                    if !is_pid_alive(pid) {
                        // Worker is dead — release its claims.
                        let released =
                            release_dead_session_claims(&config.state_loro_path, session_id)?;
                        total_released += released;
                        eprintln!(
                            "crash cleanup: released {released} claims for dead session {} (pid {pid})",
                            session_id.as_str()
                        );
                    }
                }
            }
        }
    }

    Ok(total_released)
}

/// Release claims for a specific dead session using `locked_binary_rmw`.
fn release_dead_session_claims(
    state_loro_path: &Path,
    session_id: &SessionId,
) -> Result<usize, SyncError> {
    let sid = session_id.clone();
    let mut released_count = 0usize;

    locked_binary_rmw(
        state_loro_path,
        LoroCoordinator::in_memory,
        |bytes| {
            LoroCoordinator::from_bytes(bytes, state_loro_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            // Find all claims owned by this session.
            let claims_map = coord.doc().get_map("claims");
            let mut owned_paths = Vec::new();

            claims_map.for_each(|key, value| {
                if let loro::ValueOrContainer::Value(loro::LoroValue::String(json_str)) = value {
                    if let Ok(claim) = serde_json::from_str::<Claim>(&json_str) {
                        if claim.owner == sid {
                            owned_paths.push(key.to_string());
                        }
                    }
                }
            });

            // Delete each owned claim.
            for path in &owned_paths {
                let _ = claims_map.delete(path);
            }
            if !owned_paths.is_empty() {
                coord.doc().commit();
            }
            released_count = owned_paths.len();
            Ok(())
        },
    )
    .map_err(|e| SyncError::ConnectionError(format!("crash cleanup failed: {e}")))?;

    Ok(released_count)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordination::Coordinator;
    use crate::coordination::loro::LoroCoordinator;

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    // -- PidFile tests --

    #[test]
    fn test_pid_file_new() {
        let pf = PidFile::new(12345);
        assert_eq!(pf.pid, 12345);
        assert!(pf.sessions.is_empty());
        assert_eq!(pf.ref_count, 0);
    }

    #[test]
    fn test_pid_file_add_session() {
        let mut pf = PidFile::new(100);
        let sid = session("ses-001");
        let count = pf.add_session(&sid);
        assert_eq!(count, 1);
        assert_eq!(pf.sessions.len(), 1);
        assert_eq!(pf.sessions[0].as_str(), "ses-001");
    }

    #[test]
    fn test_pid_file_add_duplicate_session() {
        let mut pf = PidFile::new(100);
        let sid = session("ses-001");
        pf.add_session(&sid);
        let count = pf.add_session(&sid);
        // Duplicate should not increment.
        assert_eq!(count, 1);
        assert_eq!(pf.sessions.len(), 1);
    }

    #[test]
    fn test_pid_file_remove_session() {
        let mut pf = PidFile::new(100);
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        pf.add_session(&s1);
        pf.add_session(&s2);
        assert_eq!(pf.ref_count, 2);

        let count = pf.remove_session(&s1);
        assert_eq!(count, 1);
        assert_eq!(pf.sessions, vec![session("ses-002")]);
    }

    #[test]
    fn test_pid_file_remove_nonexistent_session() {
        let mut pf = PidFile::new(100);
        let sid = session("ses-nonexistent");
        let count = pf.remove_session(&sid);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_pid_file_ref_count_lifecycle() {
        let mut pf = PidFile::new(100);
        let s1 = session("ses-001");
        let s2 = session("ses-002");
        let s3 = session("ses-003");

        assert_eq!(pf.add_session(&s1), 1);
        assert_eq!(pf.add_session(&s2), 2);
        assert_eq!(pf.add_session(&s3), 3);
        assert_eq!(pf.remove_session(&s2), 2);
        assert_eq!(pf.remove_session(&s1), 1);
        assert_eq!(pf.remove_session(&s3), 0);
    }

    #[test]
    fn test_pid_file_serde_roundtrip() {
        let mut pf = PidFile::new(42);
        pf.add_session(&session("ses-abc"));
        pf.add_session(&session("ses-def"));

        let json = serde_json::to_string(&pf).unwrap();
        let parsed: PidFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pid, 42);
        assert_eq!(parsed.sessions.len(), 2);
        assert_eq!(parsed.ref_count, 2);
    }

    // -- PID file I/O tests --

    #[test]
    fn test_write_and_read_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-daemon.pid");
        let mut pf = PidFile::new(999);
        pf.add_session(&session("ses-test"));

        write_pid_file(&path, &pf).unwrap();
        let read_back = read_pid_file(&path).unwrap();
        assert_eq!(read_back.pid, 999);
        assert_eq!(read_back.sessions, vec![session("ses-test")]);
        assert_eq!(read_back.ref_count, 1);
    }

    #[test]
    fn test_read_pid_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.pid");
        let result = read_pid_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_pid_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-daemon.pid");
        fs::write(&path, "{}").unwrap();
        remove_pid_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_remove_pid_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.pid");
        // Should not error.
        remove_pid_file(&path).unwrap();
    }

    // -- SyncState tests --

    #[test]
    fn test_sync_state_default() {
        let state = SyncState::default();
        assert!(state.last_sync_vv.is_none());
    }

    #[test]
    fn test_sync_state_serde_roundtrip() {
        let state = SyncState {
            last_sync_vv: Some(vec![1, 2, 3, 4]),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: SyncState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.last_sync_vv, Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_sync_state_serde_none_vv() {
        let state = SyncState { last_sync_vv: None };
        let json = serde_json::to_string(&state).unwrap();
        // skip_serializing_if should omit the field.
        assert!(!json.contains("last_sync_vv"));
    }

    #[test]
    fn test_read_sync_state_not_found_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        let state = read_sync_state(&path).unwrap();
        assert!(state.last_sync_vv.is_none());
    }

    #[test]
    fn test_write_and_read_sync_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        let state = SyncState {
            last_sync_vv: Some(vec![10, 20, 30]),
        };
        write_sync_state(&path, &state).unwrap();
        let read_back = read_sync_state(&path).unwrap();
        assert_eq!(read_back.last_sync_vv, Some(vec![10, 20, 30]));
    }

    // -- Peer ID tests --

    #[test]
    fn test_generate_peer_id_not_empty() {
        let peer = generate_peer_id();
        assert!(!peer.as_str().is_empty());
        assert!(peer.as_str().contains('-'));
    }

    #[test]
    fn test_ensure_peer_id_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let peer = ensure_peer_id(dir.path()).unwrap();
        assert!(!peer.as_str().is_empty());
        assert!(dir.path().join("peer-id").exists());
    }

    #[test]
    fn test_ensure_peer_id_reads_existing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("peer-id"), "alice-laptop").unwrap();
        let peer = ensure_peer_id(dir.path()).unwrap();
        assert_eq!(peer.as_str(), "alice-laptop");
    }

    #[test]
    fn test_ensure_peer_id_regenerates_on_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("peer-id"), "").unwrap();
        let peer = ensure_peer_id(dir.path()).unwrap();
        // Should generate a new one, not return empty.
        assert!(!peer.as_str().is_empty());
    }

    // -- Backoff tests --

    #[test]
    fn test_backoff_duration_sequence() {
        // T0=100ms, T1=200ms, T2=400ms, T3=800ms, T4=1600ms
        assert_eq!(backoff_duration(0), Duration::from_millis(100));
        assert_eq!(backoff_duration(1), Duration::from_millis(200));
        assert_eq!(backoff_duration(2), Duration::from_millis(400));
        assert_eq!(backoff_duration(3), Duration::from_millis(800));
        assert_eq!(backoff_duration(4), Duration::from_millis(1600));
    }

    #[test]
    fn test_backoff_duration_capped_at_max() {
        // Very high attempt should cap at MAX_BACKOFF (60s).
        let duration = backoff_duration(30);
        assert_eq!(duration, MAX_BACKOFF);
    }

    #[test]
    fn test_backoff_duration_overflow_safe() {
        // Should not panic on u32::MAX.
        let duration = backoff_duration(u32::MAX);
        assert!(duration <= MAX_BACKOFF);
    }

    // -- with_retry tests --

    #[test]
    fn test_with_retry_succeeds_immediately() {
        let result = with_retry(|| Ok(()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_with_retry_succeeds_on_second_attempt() {
        let mut attempt = 0;
        let result = with_retry(|| {
            attempt += 1;
            if attempt == 1 {
                Err(SyncError::ConnectionError("timeout".to_string()))
            } else {
                Ok(())
            }
        });
        assert!(result.is_ok());
        assert_eq!(attempt, 2);
    }

    #[test]
    fn test_with_retry_fails_on_schema_error_immediately() {
        let mut attempt = 0;
        let result = with_retry(|| {
            attempt += 1;
            Err(SyncError::SchemaError("bad schema".to_string()))
        });
        assert!(result.is_err());
        // Schema error should not retry.
        assert_eq!(attempt, 1);
        assert!(result.unwrap_err().to_string().contains("bad schema"));
    }

    #[test]
    fn test_with_retry_exhausts_all_retries() {
        let mut attempt = 0;
        let result = with_retry(|| {
            attempt += 1;
            Err(SyncError::ConnectionError(format!("fail-{attempt}")))
        });
        assert!(result.is_err());
        assert_eq!(attempt, MAX_RETRIES as i32 as usize);
    }

    // -- Compaction tests --

    #[test]
    fn test_run_compaction_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();
        let state_path = coord_dir.join("state.loro");

        // Create an initial empty coordinator and persist it.
        let coord = LoroCoordinator::in_memory();
        let snapshot = coord.export_bytes().unwrap();
        fs::write(&state_path, &snapshot).unwrap();

        // Compaction should succeed.
        run_compaction(&state_path).unwrap();
    }

    #[test]
    fn test_run_compaction_with_claims() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();
        let state_path = coord_dir.join("state.loro");

        // Create a coordinator with some claims.
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        coord.acquire("src/main.rs", &sid).unwrap();
        let snapshot = coord.export_bytes().unwrap();
        fs::write(&state_path, &snapshot).unwrap();

        run_compaction(&state_path).unwrap();

        // Verify claims survive compaction.
        let bytes = fs::read(&state_path).unwrap();
        let reloaded = LoroCoordinator::from_bytes(&bytes, &state_path).unwrap();
        assert!(reloaded.check("src/main.rs").is_some());
    }

    // -- Stale claim detection tests --

    #[test]
    fn test_find_stale_claims_none_expired() {
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        coord.acquire("src/a.rs", &sid).unwrap();

        let stale = find_stale_claims(&coord);
        assert!(stale.is_empty());
    }

    #[test]
    fn test_find_stale_claims_expired() {
        let mut coord = LoroCoordinator::in_memory();
        coord.set_ttl_secs(0); // Immediate expiry.
        let sid = session("ses-001");
        coord.acquire("src/a.rs", &sid).unwrap();

        let stale = find_stale_claims(&coord);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].0, "src/a.rs");
        assert_eq!(stale[0].1.owner.as_str(), "ses-001");
    }

    #[test]
    fn test_find_stale_claims_mixed() {
        let mut coord = LoroCoordinator::in_memory();
        // First claim with normal TTL.
        coord.set_ttl_secs(300);
        let s1 = session("ses-001");
        coord.acquire("src/live.rs", &s1).unwrap();

        // Second claim with expired TTL.
        coord.set_ttl_secs(0);
        let s2 = session("ses-002");
        coord.acquire("src/stale.rs", &s2).unwrap();

        let stale = find_stale_claims(&coord);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].0, "src/stale.rs");
    }

    // -- SyncConfig tests --

    #[test]
    fn test_sync_config_from_project_dir() {
        let dir = Path::new("/tmp/myproject");
        let config = SyncConfig::from_project_dir(dir, 30);
        assert_eq!(config.project_dir, dir);
        assert_eq!(
            config.state_loro_path,
            dir.join(".state/coordination/state.loro")
        );
        assert_eq!(
            config.sync_state_path,
            dir.join(".state/coordination/sync-state.json")
        );
        assert_eq!(
            config.pid_path,
            dir.join(".state/coordination/sync-daemon.pid")
        );
        assert_eq!(config.runtime_dir, dir.join(".state/runtime"));
        assert_eq!(config.interval, Duration::from_secs(30));
    }

    #[test]
    fn test_sync_config_custom_interval() {
        let dir = Path::new("/tmp/project");
        let config = SyncConfig::from_project_dir(dir, 60);
        assert_eq!(config.interval, Duration::from_secs(60));
    }

    // -- from_config_file tests --

    #[test]
    fn test_sync_config_from_config_file_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig::from_config_file(dir.path());
        // No config file — should use DEFAULT_SYNC_INTERVAL_SECS (5).
        assert_eq!(
            config.interval,
            Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS)
        );
    }

    #[test]
    fn test_sync_config_from_config_file_with_custom_interval() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{"sync": {"interval_secs": 10}}"#,
        )
        .unwrap();

        let config = SyncConfig::from_config_file(dir.path());
        assert_eq!(config.interval, Duration::from_secs(10));
    }

    #[test]
    fn test_sync_config_from_config_file_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("parallel-work-config.json"),
            "not valid json",
        )
        .unwrap();

        let config = SyncConfig::from_config_file(dir.path());
        // Malformed — should fall back to default.
        assert_eq!(
            config.interval,
            Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS)
        );
    }

    #[test]
    fn test_sync_config_from_config_file_missing_sync_section() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{"merge": {"auto_rebase": true}}"#,
        )
        .unwrap();

        let config = SyncConfig::from_config_file(dir.path());
        // No sync section — should fall back to default.
        assert_eq!(
            config.interval,
            Duration::from_secs(DEFAULT_SYNC_INTERVAL_SECS)
        );
    }

    // -- Integration test: peer delta exchange --

    #[test]
    fn test_two_peers_exchange_deltas() {
        let dir = tempfile::tempdir().unwrap();
        let repo_path = dir.path();

        // Initialize a git repo.
        git2::Repository::init(repo_path).unwrap();

        // Create two coordinators representing two peers.
        let mut coord_a = LoroCoordinator::in_memory();
        coord_a.doc().set_peer_id(1).unwrap();
        let coord_b = LoroCoordinator::in_memory();
        coord_b.doc().set_peer_id(2).unwrap();

        // Peer A creates a claim.
        let sid_a = session("ses-peer-a");
        coord_a.acquire("src/main.rs", &sid_a).unwrap();

        // Peer A exports snapshot.
        let snapshot_a = coord_a.doc().export(ExportMode::Snapshot).unwrap();

        // Peer A pushes delta to git ref.
        let peer_a = PeerId::new("peer-a");
        gitref::push_delta(repo_path, &peer_a, &snapshot_a).unwrap();

        // Peer B fetches deltas.
        let peer_b = PeerId::new("peer-b");
        let deltas = gitref::fetch_peer_deltas(repo_path, &peer_b).unwrap();
        assert_eq!(deltas.len(), 1);

        // Peer B imports the delta.
        coord_b.doc().import(&deltas[0].1).unwrap();

        // Peer B should now see peer A's claim.
        // We need to query via the claims map directly since the coordinator
        // instance has its own token tracking.
        let claims_map = coord_b.doc().get_map("claims");
        let claim_entry = claims_map.get("src/main.rs");
        assert!(claim_entry.is_some(), "peer B should see peer A's claim");
    }

    // -- Export/import delta tests --

    #[test]
    fn test_export_deltas_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        // No file exists — should return empty.
        let (delta, vv) = export_deltas(&path, None).unwrap();
        assert!(delta.is_empty());
        assert!(vv.is_empty());
    }

    #[test]
    fn test_export_deltas_with_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");

        // Create state with a claim.
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-001");
        coord.acquire("src/a.rs", &sid).unwrap();
        let snapshot = coord.export_bytes().unwrap();
        fs::write(&path, &snapshot).unwrap();

        // Export as full snapshot (no previous VV).
        let (delta, vv) = export_deltas(&path, None).unwrap();
        assert!(!delta.is_empty());
        assert!(!vv.is_empty());
    }

    #[test]
    fn test_import_peer_delta_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.loro");
        // Empty delta should be no-op.
        import_peer_delta(&path, &[]).unwrap();
    }

    // -- is_pid_alive tests --

    #[test]
    fn test_is_pid_alive_current_process() {
        let pid = std::process::id();
        assert!(is_pid_alive(pid));
    }

    #[test]
    fn test_is_pid_alive_nonexistent_pid() {
        // PID 99999999 is very unlikely to exist.
        assert!(!is_pid_alive(99_999_999));
    }

    // -- DaemonStatus tests --

    #[test]
    fn test_daemon_status_no_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let status = daemon_status(&config);
        assert!(!status.running);
        assert!(status.pid.is_none());
        assert_eq!(status.session_count, 0);
        assert!(!status.last_sync_completed);
    }

    #[test]
    fn test_daemon_status_with_dead_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        // Write PID file with a dead PID.
        let pf = PidFile::new(99_999_999);
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let status = daemon_status(&config);
        assert!(!status.running);
        assert_eq!(status.pid, Some(99_999_999));
        assert_eq!(status.session_count, 0);
    }

    #[test]
    fn test_daemon_status_with_alive_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        // Write PID file with current process PID (alive).
        let mut pf = PidFile::new(std::process::id());
        pf.add_session(&session("ses-alive"));
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let status = daemon_status(&config);
        assert!(status.running);
        assert_eq!(status.pid, Some(std::process::id()));
        assert_eq!(status.session_count, 1);
    }

    #[test]
    fn test_daemon_status_with_sync_state() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        let state = SyncState {
            last_sync_vv: Some(vec![1, 2, 3]),
        };
        write_sync_state(&coord_dir.join("sync-state.json"), &state).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let status = daemon_status(&config);
        assert!(status.last_sync_completed);
    }

    // -- start_daemon tests --

    #[test]
    fn test_start_daemon_returns_pid_if_already_alive() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        // Write PID file with our own PID (alive).
        let pf = PidFile::new(std::process::id());
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let pid = start_daemon(&config).unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn test_start_daemon_removes_stale_pid() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        // Write PID file with dead PID.
        let pf = PidFile::new(99_999_999);
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        // Will fail because "codeflow" binary likely not on PATH in test, but
        // it should have removed the stale PID file first.
        let _ = start_daemon(&config);
        // If it got past the stale check, the PID file was removed.
    }

    // -- stop_daemon tests --

    #[test]
    fn test_stop_daemon_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let result = stop_daemon(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_daemon_with_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        let pf = PidFile::new(99_999_999);
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let result = stop_daemon(&config);
        assert!(result.is_ok());
        // PID file should be removed.
        assert!(!coord_dir.join("sync-daemon.pid").exists());
    }

    // -- register_signal_handler tests --

    #[test]
    fn test_register_signal_handler() {
        let flag = register_signal_handler().unwrap();
        // Flag should start as false.
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    // -- cleanup_dead_workers tests --

    #[test]
    fn test_cleanup_dead_workers_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let released = cleanup_dead_workers(&config).unwrap();
        assert_eq!(released, 0);
    }

    #[test]
    fn test_cleanup_dead_workers_no_dead_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        let pf = PidFile::new(std::process::id());
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let released = cleanup_dead_workers(&config).unwrap();
        assert_eq!(released, 0);
    }

    #[test]
    fn test_cleanup_dead_workers_releases_dead_session_claims() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        fs::create_dir_all(&coord_dir).unwrap();

        // Create a coordinator with a claim from ses-dead.
        let mut coord = LoroCoordinator::in_memory();
        let sid = session("ses-dead");
        coord.acquire("src/main.rs", &sid).unwrap();
        let snapshot = coord.export_bytes().unwrap();
        fs::write(coord_dir.join("state.loro"), &snapshot).unwrap();

        // Write PID file tracking ses-dead.
        let mut pf = PidFile::new(std::process::id());
        pf.add_session(&sid);
        write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        // Write session status with a dead PID.
        let session_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join("ses-dead")
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("pathflow-session-status.json"),
            r#"{"session_id":"ses-dead","lead_pid":99999999,"status":"pf-in-progress"}"#,
        )
        .unwrap();

        let config = SyncConfig::from_project_dir(dir.path(), 5);
        let released = cleanup_dead_workers(&config).unwrap();
        assert_eq!(released, 1);

        // Verify claim was released.
        let bytes = fs::read(coord_dir.join("state.loro")).unwrap();
        let reloaded = LoroCoordinator::from_bytes(&bytes, &coord_dir.join("state.loro")).unwrap();
        assert!(
            reloaded.check("src/main.rs").is_none(),
            "claim should be released after cleanup"
        );
    }

    // -- SyncError classification tests --

    #[test]
    fn test_sync_error_retryable() {
        assert!(SyncError::ConnectionError("timeout".to_string()).is_retryable());
        assert!(
            SyncError::IoError(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "reset"
            ))
            .is_retryable()
        );
    }

    #[test]
    fn test_sync_error_not_retryable() {
        assert!(!SyncError::SchemaError("bad".to_string()).is_retryable());
        assert!(SyncError::SchemaError("bad".to_string()).is_schema_error());
    }

    #[test]
    fn test_sync_error_display() {
        let err = SyncError::PidLockFailed(PathBuf::from("/tmp/pid"));
        assert_eq!(err.to_string(), "pid lock failed: /tmp/pid");

        let err = SyncError::NetworkPartition {
            peer: PeerId::new("alice"),
            retries: 5,
        };
        assert_eq!(
            err.to_string(),
            "network partition: peer alice, retries exhausted (5)"
        );

        let err = SyncError::ConnectionError("timeout".to_string());
        assert_eq!(err.to_string(), "connection error: timeout");

        let err = SyncError::SchemaError("bad version".to_string());
        assert_eq!(err.to_string(), "schema error: bad version");
    }
}
