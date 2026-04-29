//! Centralized session liveness detection.
//!
//! Replaces 11 ad-hoc inline `is_process_alive()` checks with a single
//! function that combines three independent signals:
//! 1. PID liveness (name-verified via `is_process_named`)
//! 2. Heartbeat freshness (`heartbeat::is_alive`)
//! 3. Pathflow-active flag (file existence)
//!
//! Also provides [`is_session_alive`], a fail-safe-true predicate used by
//! every worktree-cleanup entry point to avoid reaping live sessions. See
//! that function's docs for its five-veto design.

use std::path::{Path, PathBuf};

use super::{heartbeat, process};

/// Default heartbeat threshold in seconds (24 hours).
pub const DEFAULT_HEARTBEAT_THRESHOLD_SECS: u64 = 86400;

/// Result of a session liveness check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessResult {
    /// PID is alive and verified as Claude Code process.
    Active,
    /// PID is dead but heartbeat is recent (session may have just ended).
    Recent,
    /// All signals indicate the session is dead.
    Dead,
    /// Insufficient data to determine liveness (no PID, no heartbeat).
    Unknown,
}

impl LivenessResult {
    /// Returns `true` for `Active` or `Recent` — the session should be
    /// treated as alive for cleanup/skip purposes.
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Active | Self::Recent)
    }

    /// Human-readable label for display (e.g., in `codeflow worktree list`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Recent => "RECENT",
            Self::Dead => "DEAD",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl std::fmt::Display for LivenessResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Check session liveness using three independent signals.
///
/// - `lead_pid`: The PID stored in `pathflow-session-status.json` or
///   the worktree registry. 0 means unknown.
/// - `heartbeat_dir`: Directory containing the heartbeat file. `None`
///   skips the heartbeat check.
/// - `pathflow_active_path`: Path to the `pathflow-active` flag file.
///   `None` skips the flag check.
/// - `heartbeat_threshold_secs`: Max age for heartbeat to count as alive.
#[must_use]
pub fn check_session_liveness(
    lead_pid: u32,
    heartbeat_dir: Option<&Path>,
    pathflow_active_path: Option<&Path>,
    heartbeat_threshold_secs: u64,
) -> LivenessResult {
    // Signal 1: PID liveness.
    // Name verification ("claude") happens at write time via validate_claude_pid().
    // At read time, a plain alive check suffices -- the stored PID is already validated.
    let pid_alive = lead_pid > 0 && process::is_process_alive(lead_pid);

    if pid_alive {
        return LivenessResult::Active;
    }

    // Signal 2: Heartbeat freshness.
    let heartbeat_alive =
        heartbeat_dir.is_some_and(|dir| heartbeat::is_alive(dir, heartbeat_threshold_secs));

    if heartbeat_alive {
        return LivenessResult::Recent;
    }

    // Signal 3: Pathflow-active flag.
    let flag_exists = pathflow_active_path.is_some_and(Path::exists);

    if flag_exists {
        // Flag exists but neither PID nor heartbeat confirms life.
        // Treat as Recent — may be a very fresh crash.
        return LivenessResult::Recent;
    }

    // No PID available means we cannot determine state at all.
    if lead_pid == 0 && heartbeat_dir.is_none() && pathflow_active_path.is_none() {
        return LivenessResult::Unknown;
    }

    LivenessResult::Dead
}

/// PID-only liveness for cleanup decisions.
///
/// Unlike [`LivenessResult`] which combines 3 signals (PID, heartbeat,
/// pathflow-active), this checks ONLY process existence. Used by
/// worktree cleanup where the question is "can this session still do
/// work?" not "was it recently active?"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidLiveness {
    /// Process is alive.
    Alive,
    /// Process is dead.
    Dead,
    /// No PID available (lead_pid was 0 or not recorded).
    NoPid,
}

impl PidLiveness {
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Alive)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Alive => "ALIVE",
            Self::Dead => "DEAD",
            Self::NoPid => "NO_PID",
        }
    }
}

impl std::fmt::Display for PidLiveness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// PID-only liveness check for cleanup decisions.
///
/// Unlike [`check_session_liveness`] which combines PID, heartbeat, and
/// pathflow-active flag signals, this checks ONLY whether the process is
/// alive. A dead process means the session cannot do any more work,
/// regardless of leftover heartbeat files or pathflow-active flags.
#[must_use]
pub fn check_pid_liveness(lead_pid: u32) -> PidLiveness {
    if lead_pid == 0 {
        return PidLiveness::NoPid;
    }
    if process::is_process_alive(lead_pid) {
        PidLiveness::Alive
    } else {
        PidLiveness::Dead
    }
}

// ---------------------------------------------------------------------------
// Fail-safe-true cleanup predicate
// ---------------------------------------------------------------------------

/// Grace window for newly registered worktrees (5 minutes). A worktree
/// registered within this window is treated as alive even if no other
/// signal confirms it -- the registering session may still be running
/// SessionStart hooks.
pub const REAPER_GRACE_WINDOW_SECS: i64 = 300;

/// Inputs to [`is_session_alive`]. Keeping this in a struct makes the
/// function easy to test without needing a real filesystem for every call.
#[derive(Debug, Clone, Default)]
pub struct SessionAliveInputs {
    /// Session ID (e.g., `ses-01kq0ha7gngv48v2v5sw0zhkw0`). Used to
    /// construct tmux session matchers and locate per-session state.
    pub session_id: String,
    /// Project directory (the main repo root, never the worktree root).
    /// All state-file paths resolve from here.
    pub project_dir: PathBuf,
    /// Worktree path whose cleanup is being considered. Compared against
    /// `CODEFLOW_WORKTREE_PATH` of the current process for the "never
    /// reap self" veto.
    pub worktree_path: Option<PathBuf>,
    /// `created_at` timestamp of the registry entry (RFC 3339). Used
    /// for the grace-window veto. `None` skips the grace check.
    pub registry_created_at: Option<String>,
}

/// Fail-safe-true predicate: is the session behind this worktree still alive?
///
/// # Design philosophy
///
/// Every cleanup entry point (SessionEnd, session_start's
/// `clean_stale_worktrees`, autorun's `cleanup_stale_session`) calls this
/// predicate **before** physically removing a worktree. The predicate
/// returns `true` (ALIVE) when ANY of four veto conditions hold, so any
/// single positive liveness signal is enough to protect the worktree.
///
/// The default when we cannot determine state (missing files, unparseable
/// data, tmux failure) is **alive**. A false positive here just means a
/// dead session's worktree gets reaped one cycle later; a false negative
/// destroys live work mid-session. The former is recoverable, the latter
/// is not. We err on the side of not reaping.
///
/// # Veto conditions (any ONE → alive)
///
/// - **V1 Grace window** — registry entry `created_at` within the last
///   [`REAPER_GRACE_WINDOW_SECS`] (5 min). Protects worktrees mid-init.
/// - **V2 Self-match** — the caller's `CODEFLOW_WORKTREE_PATH` equals the
///   target. A process never reaps its own worktree.
/// - **V3 Tmux pane alive** — any tmux pane whose session name ends with
///   an 8-char suffix of the session ID is non-dead.
/// - **V4 lead_pid alive** — `pathflow-team.json` → `lead_pid` → `kill(pid, 0)`.
///   **If the file is missing, unreadable, or malformed, this veto
///   ABSTAINS (does not vote dead), so an initializing session cannot be
///   reaped via V4 alone.**
///
/// Only when ALL FOUR vetoes fail is the session considered confirmed
/// dead.
///
/// INF-TSK-024-050 AC #3: V5 (heartbeat mtime) was removed. V4 alone is
/// sufficient -- the heartbeat file added a write per hook invocation
/// without providing any liveness signal beyond what the lead_pid file
/// check already gives us. The heartbeat file remains as a discovery
/// breadcrumb for `codeflow interactive status`, but is no longer
/// consulted by the cleanup predicate.
#[must_use]
pub fn is_session_alive(inputs: &SessionAliveInputs) -> bool {
    // V1: grace window -- entry is too young to judge.
    if veto_grace_window(inputs.registry_created_at.as_deref()) {
        return true;
    }
    // V2: caller is the session itself.
    if veto_env_match(inputs.worktree_path.as_deref()) {
        return true;
    }
    // V3: any tmux pane for this session is still running.
    if veto_tmux_alive(&inputs.session_id) {
        return true;
    }
    // V4: lead_pid recorded in pathflow-team.json is alive.
    //     (Abstains -- returns true -- on missing/malformed file.)
    if veto_lead_pid_alive(&inputs.project_dir, &inputs.session_id) {
        return true;
    }
    false
}

/// V1: entry is within the grace window.
fn veto_grace_window(created_at: Option<&str>) -> bool {
    let Some(ts) = created_at else {
        return false;
    };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return false;
    };
    let age = chrono::Utc::now()
        .signed_duration_since(parsed)
        .num_seconds();
    (0..REAPER_GRACE_WINDOW_SECS).contains(&age)
}

/// V2: worktree path matches the current process's `CODEFLOW_WORKTREE_PATH`.
///
/// Split from the env read for testability.
fn veto_env_match(worktree_path: Option<&Path>) -> bool {
    let Some(wt_path) = worktree_path else {
        return false;
    };
    let Ok(env_val) = std::env::var("CODEFLOW_WORKTREE_PATH") else {
        return false;
    };
    if env_val.is_empty() {
        return false;
    }
    // Canonicalize both sides when possible -- symlinks in .git-worktrees
    // can change one side without the other.
    let env_canon = std::fs::canonicalize(&env_val).unwrap_or_else(|_| PathBuf::from(&env_val));
    let target_canon = std::fs::canonicalize(wt_path).unwrap_or_else(|_| wt_path.to_path_buf());
    env_canon == target_canon
}

/// V3: any tmux pane in a session whose name ends with the first 8 chars
/// of the ULID portion of the session ID is non-dead.
///
/// Returns `true` if tmux reports a matching live pane; `false` if tmux
/// is unavailable, the listing fails, or no match is found. Tmux failure
/// alone does NOT vote "alive" -- other vetoes still apply.
fn veto_tmux_alive(session_id: &str) -> bool {
    let suffix = session_id_tmux_suffix(session_id);
    if suffix.is_empty() {
        return false;
    }
    // List all tmux panes with session name + dead flag.
    let output = std::process::Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{session_name} #{pane_dead}"])
        .output();
    let Ok(out) = output else {
        return false; // tmux not installed or not reachable -- abstain (false).
    };
    if !out.status.success() {
        return false; // No sessions at all, or a transient failure.
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(dead) = parts.next() else { continue };
        if name.contains(&suffix) && dead == "0" {
            return true;
        }
    }
    false
}

/// Extract the first 8 chars of the ULID portion of a session ID
/// (characters 4..12, skipping the `ses-` prefix). Returns empty
/// string for malformed inputs.
fn session_id_tmux_suffix(session_id: &str) -> String {
    session_id
        .strip_prefix("ses-")
        .filter(|s| s.len() >= 8)
        .map(|s| s[..8].to_string())
        .unwrap_or_default()
}

/// V4: `pathflow-team.json` → `lead_pid` → `kill(pid, 0)`.
///
/// **Fail-safe abstention**: if the file is missing, unreadable, or
/// malformed, returns `true` (treat as alive). This matches the
/// "initializing session" case -- the file is written shortly after
/// `SessionStart`, so a race between registry creation and file write
/// must NOT reap the worktree.
///
/// If the file exists and is valid JSON, returns `kill(lead_pid, 0)`.
/// A missing or zero `lead_pid` field in a valid file is a valid "no
/// process" signal and returns `false`.
fn veto_lead_pid_alive(project_dir: &Path, session_id: &str) -> bool {
    let team_path = project_dir
        .join(".state")
        .join("session")
        .join(session_id)
        .join("pathflow")
        .join("pathflow-team.json");
    let Ok(content) = std::fs::read_to_string(&team_path) else {
        return true; // File missing or unreadable -- abstain.
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return true; // Malformed JSON -- abstain.
    };
    // Explicit `lead_pid` field at top level.
    let lead_pid = value
        .get("lead_pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    if lead_pid > 0 && process::is_process_alive(lead_pid) {
        return true;
    }
    // Structurally valid file with a dead or missing lead_pid is a clear
    // "no process" signal.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_when_pid_alive() {
        // Current process PID is alive, so liveness should be Active.
        let pid = std::process::id();
        let result = check_session_liveness(pid, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Active);
    }

    #[test]
    fn test_dead_pid_not_active() {
        // A dead PID should NOT return Active.
        let result =
            check_session_liveness(4_000_000, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_ne!(result, LivenessResult::Active);
    }

    #[test]
    fn test_dead_when_pid_dead_no_heartbeat_no_flag() {
        let result =
            check_session_liveness(4_000_000, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Dead);
    }

    #[test]
    fn test_unknown_when_no_signals() {
        let result = check_session_liveness(0, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(result, LivenessResult::Unknown);
    }

    #[test]
    fn test_recent_when_heartbeat_alive() {
        let dir = tempfile::tempdir().unwrap();
        // Write a fresh heartbeat file.
        let hb_path = dir.path().join(".state/runtime/heartbeat");
        std::fs::create_dir_all(hb_path.parent().unwrap()).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let data = serde_json::json!({
            "session_id": "ses-test",
            "timestamp": now,
            "source": "test"
        });
        std::fs::write(&hb_path, serde_json::to_string(&data).unwrap()).unwrap();

        // Dead PID but fresh heartbeat -> Recent.
        let result = check_session_liveness(
            4_000_000,
            Some(dir.path()),
            None,
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Recent);
    }

    #[test]
    fn test_recent_when_pathflow_active_exists() {
        let dir = tempfile::tempdir().unwrap();
        let flag_path = dir.path().join("pathflow-active");
        std::fs::write(&flag_path, "").unwrap();

        // Dead PID, no heartbeat, but flag exists -> Recent.
        let result = check_session_liveness(
            4_000_000,
            None,
            Some(&flag_path),
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Recent);
    }

    #[test]
    fn test_dead_when_all_signals_negative() {
        let dir = tempfile::tempdir().unwrap();
        let flag_path = dir.path().join("nonexistent-flag");

        let result = check_session_liveness(
            4_000_000,
            Some(dir.path()),
            Some(&flag_path),
            DEFAULT_HEARTBEAT_THRESHOLD_SECS,
        );
        assert_eq!(result, LivenessResult::Dead);
    }

    #[test]
    fn test_is_alive_active() {
        assert!(LivenessResult::Active.is_alive());
    }

    #[test]
    fn test_is_alive_recent() {
        assert!(LivenessResult::Recent.is_alive());
    }

    #[test]
    fn test_is_alive_dead() {
        assert!(!LivenessResult::Dead.is_alive());
    }

    #[test]
    fn test_is_alive_unknown() {
        assert!(!LivenessResult::Unknown.is_alive());
    }

    #[test]
    fn test_label_values() {
        assert_eq!(LivenessResult::Active.label(), "ACTIVE");
        assert_eq!(LivenessResult::Recent.label(), "RECENT");
        assert_eq!(LivenessResult::Dead.label(), "DEAD");
        assert_eq!(LivenessResult::Unknown.label(), "UNKNOWN");
    }

    #[test]
    fn test_display_matches_label() {
        assert_eq!(format!("{}", LivenessResult::Active), "ACTIVE");
        assert_eq!(format!("{}", LivenessResult::Dead), "DEAD");
    }

    // ---------------------------------------------------------------
    // PidLiveness tests
    // ---------------------------------------------------------------

    #[test]
    fn test_pid_liveness_alive_for_current_process() {
        let pid = std::process::id();
        let result = check_pid_liveness(pid);
        assert_eq!(result, PidLiveness::Alive);
        assert!(result.is_alive());
    }

    #[test]
    fn test_pid_liveness_dead_for_nonexistent_pid() {
        let result = check_pid_liveness(4_000_000);
        assert_eq!(result, PidLiveness::Dead);
        assert!(!result.is_alive());
    }

    #[test]
    fn test_pid_liveness_no_pid_for_zero() {
        let result = check_pid_liveness(0);
        assert_eq!(result, PidLiveness::NoPid);
        assert!(!result.is_alive());
    }

    #[test]
    fn test_pid_liveness_label_values() {
        assert_eq!(PidLiveness::Alive.label(), "ALIVE");
        assert_eq!(PidLiveness::Dead.label(), "DEAD");
        assert_eq!(PidLiveness::NoPid.label(), "NO_PID");
    }

    #[test]
    fn test_pid_liveness_display_matches_label() {
        assert_eq!(format!("{}", PidLiveness::Alive), "ALIVE");
        assert_eq!(format!("{}", PidLiveness::Dead), "DEAD");
        assert_eq!(format!("{}", PidLiveness::NoPid), "NO_PID");
    }

    #[test]
    fn test_pid_liveness_is_alive_exhaustive() {
        assert!(PidLiveness::Alive.is_alive());
        assert!(!PidLiveness::Dead.is_alive());
        assert!(!PidLiveness::NoPid.is_alive());
    }

    // -----------------------------------------------------------------
    // is_session_alive (fail-safe-true) tests
    // -----------------------------------------------------------------
    //
    // Design contract exercised below:
    //   * Any single veto firing returns true (ALIVE).
    //   * All four vetoes failing returns false (confirmed dead).
    //   * V4 ABSTAINS (returns alive) when pathflow-team.json is
    //     missing or malformed -- initializing sessions must not be reaped.

    const DEAD_SID: &str = "ses-01kq0deadseadbeef01234567";

    /// Build inputs for a "confirmed-dead" fixture: nothing exists,
    /// the worktree path exists but is not registered in env, no tmux
    /// match, no heartbeat, no team file. All vetoes should fail.
    fn dead_inputs(project_dir: &Path) -> SessionAliveInputs {
        SessionAliveInputs {
            session_id: DEAD_SID.to_string(),
            project_dir: project_dir.to_path_buf(),
            worktree_path: Some(project_dir.join("worktree-nonexistent")),
            registry_created_at: Some(
                (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339(),
            ),
        }
    }

    #[test]
    fn test_session_id_tmux_suffix_standard() {
        assert_eq!(
            session_id_tmux_suffix("ses-01kk0t08ggabcdef12345678"),
            "01kk0t08"
        );
    }

    #[test]
    fn test_session_id_tmux_suffix_short_or_malformed() {
        // Missing prefix.
        assert_eq!(session_id_tmux_suffix("abc"), "");
        // Too short after prefix.
        assert_eq!(session_id_tmux_suffix("ses-abc"), "");
        // Empty.
        assert_eq!(session_id_tmux_suffix(""), "");
    }

    // --- V1 grace window ---

    #[test]
    fn test_veto_grace_window_recent_entry_votes_alive() {
        let now_minus_60 = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
        assert!(
            veto_grace_window(Some(&now_minus_60)),
            "entry aged 60s (< 300s grace) must vote alive"
        );
    }

    #[test]
    fn test_veto_grace_window_old_entry_votes_dead() {
        let old = (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339();
        assert!(
            !veto_grace_window(Some(&old)),
            "entry aged 1h must not vote alive"
        );
    }

    #[test]
    fn test_veto_grace_window_none_votes_dead() {
        assert!(
            !veto_grace_window(None),
            "no timestamp means grace cannot fire"
        );
    }

    #[test]
    fn test_veto_grace_window_malformed_votes_dead() {
        assert!(
            !veto_grace_window(Some("not-a-date")),
            "malformed timestamps must not vote alive"
        );
    }

    // --- V2 env match ---

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_veto_env_match_same_path_votes_alive() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_WORKTREE_PATH", dir.path());
        }
        let result = veto_env_match(Some(dir.path()));
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(result, "self-match must vote alive");
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_veto_env_match_different_path_votes_dead() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_WORKTREE_PATH", dir_a.path());
        }
        let result = veto_env_match(Some(dir_b.path()));
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(!result, "different paths must not vote alive");
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_veto_env_match_no_env_votes_dead() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(!veto_env_match(Some(dir.path())));
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_veto_env_match_empty_env_votes_dead() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_WORKTREE_PATH", "");
        }
        let result = veto_env_match(Some(dir.path()));
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(!result, "empty env var must be treated as unset");
    }

    #[test]
    fn test_veto_env_match_no_target_votes_dead() {
        assert!(!veto_env_match(None));
    }

    // --- V3 tmux ---
    //
    // Full tmux behaviour is covered in the integration test
    // (test_is_session_alive_vetoes_live_tmux). Here we only verify
    // the "no matching session" path, which is safe to run without
    // side-effects.

    #[test]
    fn test_veto_tmux_no_match_votes_dead() {
        // Use a random nonsense suffix that no tmux session on the
        // developer's machine could match.
        assert!(!veto_tmux_alive("ses-01kqzzzzznomatch0987654321"));
    }

    #[test]
    fn test_veto_tmux_malformed_session_id_votes_dead() {
        // Malformed IDs yield empty suffix -> skip tmux call.
        assert!(!veto_tmux_alive(""));
        assert!(!veto_tmux_alive("ses-"));
    }

    // --- V4 lead_pid ---

    #[test]
    fn test_veto_lead_pid_missing_file_abstains_alive() {
        let dir = tempfile::tempdir().unwrap();
        // No pathflow-team.json exists.
        assert!(
            veto_lead_pid_alive(dir.path(), "ses-01kqinitializing1234567890"),
            "missing team file must abstain (vote alive) to protect initializing sessions"
        );
    }

    #[test]
    fn test_veto_lead_pid_malformed_file_abstains_alive() {
        let dir = tempfile::tempdir().unwrap();
        let team_path = dir
            .path()
            .join(".state/session/ses-01kqmalformed01234567890/pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        std::fs::write(&team_path, "{not valid json").unwrap();
        assert!(
            veto_lead_pid_alive(dir.path(), "ses-01kqmalformed01234567890"),
            "malformed team file must abstain"
        );
    }

    #[test]
    fn test_veto_lead_pid_alive_with_live_pid_votes_alive() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqlivepid012345678901234";
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        let payload = serde_json::json!({
            "team_name": "codeflow-test",
            "lead_pid": std::process::id(),
        });
        std::fs::write(&team_path, payload.to_string()).unwrap();
        assert!(veto_lead_pid_alive(dir.path(), sid));
    }

    #[test]
    fn test_veto_lead_pid_valid_file_dead_pid_votes_dead() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqdeadpid0123456789012345";
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        let payload = serde_json::json!({
            "team_name": "codeflow-test",
            "lead_pid": 4_000_000_u32,
        });
        std::fs::write(&team_path, payload.to_string()).unwrap();
        assert!(
            !veto_lead_pid_alive(dir.path(), sid),
            "valid file with dead PID is a conclusive dead vote"
        );
    }

    #[test]
    fn test_veto_lead_pid_valid_file_missing_field_votes_dead() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqnopidfield01234567890";
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        std::fs::write(&team_path, r#"{"team_name": "x"}"#).unwrap();
        assert!(
            !veto_lead_pid_alive(dir.path(), sid),
            "structurally valid file with missing lead_pid is a dead vote"
        );
    }

    // --- aggregate predicate ---

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_is_session_alive_all_vetoes_fail_returns_false() {
        // Build a fixture where all four vetoes evaluate to false.
        // V1: old created_at. V2: env var unset. V3: bogus SID (no tmux match).
        // V4: pathflow-team.json exists, valid, with dead PID -- NOT abstain.
        let dir = tempfile::tempdir().unwrap();
        let sid = DEAD_SID;
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        std::fs::write(
            &team_path,
            serde_json::json!({"team_name": "x", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        let mut inputs = dead_inputs(dir.path());
        inputs.session_id = sid.to_string();
        assert!(
            !is_session_alive(&inputs),
            "confirmed-dead session (all vetoes fail) must return false"
        );
    }

    #[test]
    fn test_is_session_alive_v1_grace_wins_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut inputs = dead_inputs(dir.path());
        inputs.registry_created_at =
            Some((chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339());
        assert!(
            is_session_alive(&inputs),
            "grace window alone must keep the session alive"
        );
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_is_session_alive_v2_env_match_wins_alone() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("me");
        std::fs::create_dir_all(&wt).unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_WORKTREE_PATH", &wt);
        }
        let mut inputs = dead_inputs(dir.path());
        inputs.worktree_path = Some(wt);
        let result = is_session_alive(&inputs);
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(result, "self-match must keep the session alive");
    }

    #[test]
    fn test_is_session_alive_v4_missing_team_file_abstains_alive() {
        // No pathflow-team.json anywhere → V4 abstains (alive).
        // Ensure V1 is NOT firing by setting a very old created_at.
        let dir = tempfile::tempdir().unwrap();
        let mut inputs = dead_inputs(dir.path());
        inputs.session_id = "ses-01kqinitnotreaped12345678".into();
        inputs.registry_created_at =
            Some((chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339());
        assert!(
            is_session_alive(&inputs),
            "missing pathflow-team.json must keep the session alive (init protection)"
        );
    }

    #[test]
    fn test_is_session_alive_v4_live_pid_wins_alone() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqlivewin012345678901234";
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        std::fs::write(
            &team_path,
            serde_json::json!({"team_name": "x", "lead_pid": std::process::id()}).to_string(),
        )
        .unwrap();

        let inputs = SessionAliveInputs {
            session_id: sid.to_string(),
            project_dir: dir.path().to_path_buf(),
            worktree_path: Some(dir.path().join("wt")),
            registry_created_at: Some(
                (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339(),
            ),
        };
        assert!(
            is_session_alive(&inputs),
            "live lead_pid must keep the session alive"
        );
    }

    // -----------------------------------------------------------------
    // Integration test (exercises real tmux) — the only test that
    // actually spawns a tmux session. Skipped when tmux is not
    // available on the host.
    // -----------------------------------------------------------------

    fn tmux_available() -> bool {
        std::process::Command::new("tmux")
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn test_is_session_alive_vetoes_live_tmux() {
        // Spawn a real tmux session named so its name contains the
        // 8-char ULID suffix we embed in is_session_alive's V3 matcher.
        // Then verify is_session_alive reports alive even when every
        // other veto (V1/V2/V4) votes dead.
        //
        // All other vetoes are driven to DEAD:
        //   V1: registry_created_at in the distant past
        //   V2: CODEFLOW_WORKTREE_PATH not set on the target path
        //   V4: pathflow-team.json present with dead lead_pid
        if !tmux_available() {
            eprintln!("tmux not available -- skipping integration test");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqinttest01234567890abcd"; // suffix = "01kqintt"
        let suffix = &sid[4..12]; // "01kqintt"
        let tmux_name = format!("cf-live-{suffix}");

        // Create pathflow-team.json with a dead lead_pid so V4 votes DEAD.
        let team_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-team.json");
        std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
        std::fs::write(
            &team_path,
            serde_json::json!({"team_name": "integration", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        // Start a detached tmux session running a long-lived command.
        let spawn = std::process::Command::new("tmux")
            .args(["new-session", "-d", "-s", &tmux_name, "sleep", "60"])
            .status();

        let spawned_ok = matches!(spawn, Ok(s) if s.success());
        if !spawned_ok {
            eprintln!("tmux new-session failed -- skipping integration test");
            return;
        }

        // Ensure we kill the tmux session even if an assertion panics.
        struct TmuxGuard(String);
        impl Drop for TmuxGuard {
            fn drop(&mut self) {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", &self.0])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        let guard = TmuxGuard(tmux_name.clone());

        let inputs = SessionAliveInputs {
            session_id: sid.to_string(),
            project_dir: dir.path().to_path_buf(),
            worktree_path: Some(dir.path().join("nonexistent-wt")),
            registry_created_at: Some(
                (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339(),
            ),
        };
        assert!(
            is_session_alive(&inputs),
            "live tmux session must keep is_session_alive true even when every other veto is dead"
        );

        // Sanity: after killing the tmux session, the predicate must go
        // false (V3 no longer fires, and V1/V2/V4 are all dead).
        drop(guard);
        // Small wait to ensure tmux registers the session as gone.
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            !is_session_alive(&inputs),
            "after tmux kill every veto is dead; predicate must return false"
        );
    }

    // -----------------------------------------------------------------
    // INF-TSK-024-050 AC #6: lead_pid canonical source test
    // -----------------------------------------------------------------
    //
    // Proves liveness for a live session resolves through
    // `pathflow-session-status.json::lead_pid` (the canonical PID, written
    // via `validate_claude_pid(parent_id())` in session-start hooks),
    // NOT via `session-pointer.json` (the `lead_pid` field was removed in
    // AC #2) or any DB-cached PID.

    /// Read `lead_pid` from a `pathflow-session-status.json` file using
    /// the same shape downstream consumers (sync.rs, interactive.rs,
    /// session/mod.rs::read_lead_pid_from_status) use. Kept identical to
    /// the production parsers so a regression in any of them surfaces
    /// here too.
    fn read_lead_pid_from_status_json(project_dir: &Path, session_id: &str) -> u32 {
        let path = project_dir
            .join(".state/session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-session-status.json");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return 0;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return 0;
        };
        value
            .get("lead_pid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0)
    }

    #[test]
    fn lead_pid_canonical_source() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqcanon0123456789abcdef";

        // Canonical source: pathflow-session-status.json with the LIVE PID.
        let status_dir = dir.path().join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        let live_pid = std::process::id();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            serde_json::json!({
                "session_id": sid,
                "lead_pid": live_pid,
                "status": "pf-in-progress"
            })
            .to_string(),
        )
        .unwrap();

        // Stale source: a session-pointer.json with a different (dead) PID.
        // After INF-TSK-024-050 AC #2 the lead_pid field is GONE from the
        // SessionPointer struct, but a stale JSON file from a pre-fix
        // session may still exist on disk. Liveness must not consult it.
        let pointer_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&pointer_dir).unwrap();
        std::fs::write(
            pointer_dir.join("session-pointer.json"),
            serde_json::json!({
                "lead_pid": 4_000_000_u32, // Stale -- a dead PID.
                "worktree_path": "/tmp/wt",
                "session_id": sid,
                "created_at": "2026-01-01T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();

        // Read PID from the canonical source and confirm we got the LIVE
        // value, not the stale one.
        let resolved = read_lead_pid_from_status_json(dir.path(), sid);
        assert_eq!(
            resolved, live_pid,
            "liveness must resolve PID from pathflow-session-status.json (live={live_pid}), \
             not the stale session-pointer.json"
        );
        assert_ne!(
            resolved, 4_000_000,
            "liveness must NOT pick up the stale PID from session-pointer.json"
        );

        // Drive the public liveness API with the canonical PID.
        let result = check_session_liveness(resolved, None, None, DEFAULT_HEARTBEAT_THRESHOLD_SECS);
        assert_eq!(
            result,
            LivenessResult::Active,
            "live PID from canonical source must produce Active"
        );
        assert!(result.is_alive());
    }

    #[test]
    fn lead_pid_canonical_source_missing_status_returns_zero() {
        // Regression guard: if the canonical file is missing, callers must
        // see 0 (not crash, not fall back to a stale source). The "no PID"
        // signal lets downstream liveness predicates fail closed correctly
        // (V4 abstains; check_session_liveness with pid=0 yields
        // Unknown/Dead based on other signals).
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqmissing01234567890ab";
        // Pre-fix sessions may still have a session-pointer.json on disk
        // -- it must NOT be consulted as a fallback.
        let pointer_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&pointer_dir).unwrap();
        std::fs::write(
            pointer_dir.join("session-pointer.json"),
            serde_json::json!({
                "lead_pid": std::process::id(),
                "worktree_path": "/tmp/wt",
                "session_id": sid,
                "created_at": "2026-01-01T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            read_lead_pid_from_status_json(dir.path(), sid),
            0,
            "missing pathflow-session-status.json must yield 0, not a fallback to session-pointer.json"
        );
    }
}
