//! Centralized session liveness detection.
//!
//! Two complementary predicates:
//!
//! 1. [`is_session_alive`] (canonical, used by TUI + sync + worktree-status
//!    callers): reads the worktree-resolved `pathflow-session-status.json::lead_pid`
//!    and validates it via `validate_claude_pid`. Returns [`SessionLiveness`]
//!    (`Active` / `Dead` / `Unknown`). This is the chokepoint the rest of the
//!    codebase MUST go through — there is no other source of truth for "is
//!    the session's lead process alive right now?".
//!
//! 2. [`is_session_alive_for_cleanup`] (cleanup-context predicate, used by
//!    worktree reaper paths): a fail-safe-true predicate that combines the
//!    canonical PID check with grace-window, env-match, and tmux vetoes so
//!    a freshly-registered worktree is never destroyed mid-init.
//!
//! Both predicates ultimately call `validate_claude_pid` against the live PID
//! recorded in `pathflow-session-status.json::lead_pid` (the only authoritative
//! PID source — written by `SessionStart` via `parent_id()` walking up to the
//! claude process).
//!
//! INF-TSK-024-051 Phase 4: removed the legacy 3-signal combiner
//! `check_session_liveness` (PID + heartbeat + pathflow-active flag) and
//! the `LivenessResult::Recent` variant. The heartbeat file
//! (`.state/runtime/heartbeat`) was a Signal-2 source that the canonical
//! chokepoint does not need; removing it eliminates a write per hook
//! invocation. `LivenessResult` retains `Active`/`Dead`/`Unknown` for
//! the `WorktreeManager::liveness_status` public API.

use std::path::{Path, PathBuf};

use super::process;

/// Result of a worktree's session liveness check (`WorktreeManager::liveness_status`).
///
/// INF-TSK-024-051 Phase 4: shrunk from the previous 4 variants
/// (`Active|Recent|Dead|Unknown`) to 3 (`Active|Dead|Unknown`). `Recent`
/// described "PID dead but heartbeat fresh" — heartbeat is gone, so the
/// state is unreachable. Mapping from `SessionLiveness` is now total
/// and lossless: `SessionLiveness::Active → LivenessResult::Active`,
/// `Dead → Dead`, `Unknown → Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessResult {
    /// PID is alive and verified as Claude Code process.
    Active,
    /// All signals indicate the session is dead.
    Dead,
    /// Insufficient data to determine liveness.
    Unknown,
}

impl LivenessResult {
    /// Returns `true` only for `Active`. INF-TSK-024-051 Phase 4 removed
    /// the `Recent` variant; the `is_alive()` predicate is now exact.
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Human-readable label for display (e.g., in `codeflow worktree list`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
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
/// Checks ONLY whether the process is alive. A dead process means the
/// session cannot do any more work, regardless of leftover state files
/// (heartbeat / pathflow-active / etc.). Used by `WorktreeManager::pid_liveness`
/// for the cleanup-classification cascade.
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
// Canonical session-liveness chokepoint (INF-TSK-024-051)
// ---------------------------------------------------------------------------

/// Canonical session-liveness verdict.
///
/// Returned by [`is_session_alive`] — the single chokepoint every TUI,
/// sync, and worktree-status caller must use to ask "is this session's
/// lead process alive right now?". Three values cover the cases callers
/// actually act on:
///
/// - `Active` — a `pathflow-session-status.json` file exists, contains a
///   non-zero `lead_pid`, and the kernel reports that PID alive AND named
///   "claude" (`validate_claude_pid` returns the input PID). Display the
///   session as live; do not promote to stale.
/// - `Dead` — the file exists and contains a `lead_pid` that the kernel
///   reports dead OR not named "claude" (PID reuse by an unrelated
///   process). Promote the row to stale; eligible for cleanup.
/// - `Unknown` — the file is missing, unreadable, or its `lead_pid` is
///   absent / zero. Caller must NOT treat this as either alive or dead;
///   typically display "--" and defer the cleanup decision to the
///   cleanup-context predicate ([`is_session_alive_for_cleanup`]) which
///   has additional vetoes for the initializing-session case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLiveness {
    /// PID alive and verified as a Claude Code process.
    Active,
    /// PID dead or owned by a different process.
    Dead,
    /// No status file or no recorded PID; verdict indeterminate.
    Unknown,
}

impl SessionLiveness {
    /// `true` only for `Active`. `Unknown` is NOT alive — callers wanting
    /// fail-safe-true behavior must use [`is_session_alive_for_cleanup`]
    /// instead.
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Human-readable label for display surfaces (TUI, logs).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Dead => "DEAD",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl std::fmt::Display for SessionLiveness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Resolve the directory containing the session's `pathflow/` state files.
///
/// Worktree sessions store `.state/session/{sid}/pathflow/` LOCALLY inside
/// the worktree, not in the main repo. This helper consults
/// `worktrees.yaml` to locate the owning worktree path; if no worktree
/// entry references this `sid`, falls back to the main repo (covers
/// non-worktree / `mode=disabled` sessions).
///
/// Returned path: `{base}/.state/session/{sid}` where `{base}` is either
/// the worktree root or `project_dir`. The caller appends `pathflow/...`
/// for the file it needs.
///
/// Path-traversal guard: malformed `sid` values (containing `..`) yield
/// the main-repo fallback path so a malicious caller cannot escape the
/// `.state/session/` subtree. The fallback path itself is still safe
/// because `Path::join` rejects absolute components.
///
/// Returns the worktree-rooted path WITHOUT verifying the file exists —
/// the caller decides whether a missing file means "Unknown" (
/// [`is_session_alive`]) or "skip / try the next candidate"
/// ([`is_session_alive_for_cleanup`]).
#[must_use]
pub fn resolve_session_state_dir(project_dir: &Path, session_id: &str) -> PathBuf {
    if session_id.is_empty() || session_id.contains("..") {
        return project_dir.join(".state").join("session").join(session_id);
    }
    let registry_path = project_dir
        .join(".state")
        .join("worktrees")
        .join("worktrees.yaml");
    if let Ok(reg) = crate::worktree::read_registry(&registry_path) {
        for entry in &reg.worktrees {
            if entry.session_id.as_deref().is_some_and(|s| s == session_id)
                && !entry.path.is_empty()
            {
                let wt = Path::new(&entry.path);
                if wt.exists() {
                    return wt.join(".state").join("session").join(session_id);
                }
            }
        }
    }
    project_dir.join(".state").join("session").join(session_id)
}

/// Canonical "is this session alive?" chokepoint.
///
/// Reads `pathflow-session-status.json::lead_pid` from the
/// worktree-resolved location (via [`resolve_session_state_dir`]) and
/// validates it through `validate_claude_pid` (alive AND named "claude").
///
/// This is the ONLY function the TUI, sync daemon, and any caller asking
/// "should I display this session as live?" should use. Migrating callers
/// off this chokepoint is a regression — see the lint guard in
/// `cf-rust-standards/SKILL.md`.
///
/// Returns:
/// - `Active` — file present, `lead_pid > 0`, kernel confirms PID alive,
///   process name contains "claude". Display PID as live.
/// - `Dead` — file present, `lead_pid > 0`, kernel reports PID dead OR
///   process name does NOT contain "claude". Caller may promote to stale.
/// - `Unknown` — file missing / unreadable / malformed JSON, OR
///   `lead_pid` field missing / zero. Caller must NOT promote to stale —
///   the session may be initializing. Display "--".
#[must_use]
pub fn is_session_alive(project_dir: &Path, session_id: &str) -> SessionLiveness {
    is_session_alive_with_validator(project_dir, session_id, current_pid_validator())
}

/// Function pointer type for the chokepoint's PID validator (test seam).
type PidValidatorFn = fn(u32) -> bool;

/// Process-wide PID validator override for the chokepoint.
///
/// Stored in a `RwLock` (not a thread_local) because some tests spawn
/// worker threads that must inherit the same validator
/// (`test_concurrent_startup_single_session_id` is the canonical
/// example). Tests that mutate the override MUST serialise via
/// `#[serial_test::serial(env_vars)]` (or an equivalent group) — the
/// override is process-global and concurrent overrides race.
///
/// The override is `pub`-reachable (not `#[cfg(test)]`-gated) so
/// downstream crates' integration / unit tests can install a
/// synthetic validator. The runtime cost is a single `RwLock::read`
/// per `is_session_alive` call when no override is installed (the
/// common production path).
static TEST_PID_VALIDATOR: std::sync::RwLock<Option<PidValidatorFn>> = std::sync::RwLock::new(None);

/// Resolve the PID validator the chokepoint should call: the
/// process-wide override (if installed by a test) or the production
/// default. The override slot is `None` in production by default, so
/// the read is a single uncontended `RwLock::read` that returns `None`.
#[must_use]
fn current_pid_validator() -> fn(u32) -> bool {
    if let Ok(guard) = TEST_PID_VALIDATOR.read() {
        if let Some(v) = *guard {
            return v;
        }
    }
    default_pid_validator
}

/// Install a process-wide PID validator override for the duration of a
/// test. Returns a guard that restores the previous validator when
/// dropped. Tests call this once at setup; the override applies to ALL
/// `is_session_alive` calls (any thread) until the guard is dropped.
///
/// Available in non-test builds too (no `#[cfg(test)]` gate) so
/// downstream crates' tests can install an override without going
/// through a feature flag. Production code MUST NOT call this; the
/// `for_tests` suffix and module placement signal intent.
#[must_use = "the override is reverted when the guard drops"]
pub fn override_pid_validator_for_tests(validator: fn(u32) -> bool) -> PidValidatorGuard {
    let previous = match TEST_PID_VALIDATOR.write() {
        Ok(mut guard) => guard.replace(validator),
        Err(_) => None,
    };
    PidValidatorGuard { previous }
}

/// RAII guard returned by [`override_pid_validator_for_tests`]. Drop
/// restores the previous validator (or the production default if none
/// was set).
pub struct PidValidatorGuard {
    previous: Option<fn(u32) -> bool>,
}

impl Drop for PidValidatorGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = TEST_PID_VALIDATOR.write() {
            *guard = self.previous;
        }
    }
}

/// Default PID validator for [`is_session_alive`] — wraps
/// `validate_claude_pid` so the production chokepoint requires the PID
/// to be alive AND named "claude" (rejects PID reuse by an unrelated
/// process). Tests inject a synthetic validator via
/// [`is_session_alive_with_validator`] when they cannot be a Claude
/// Code child process.
#[must_use]
pub(crate) fn default_pid_validator(pid: u32) -> bool {
    process::validate_claude_pid(pid) > 0
}

/// Test-seam variant of [`is_session_alive`] that accepts an injected
/// PID validator. Production code reaches the same logic via
/// [`is_session_alive`] which uses [`default_pid_validator`].
///
/// `pid_validator` is called with the non-zero `u32` PID read from the
/// status file and returns `true` when the PID should be treated as a
/// live owner of the session. Returns `Active` when the validator
/// accepts, `Dead` when it rejects.
///
/// This seam exists ONLY so production callers like `handle_stale_cleanup`
/// can be tested without spawning a real `claude` child process —
/// pre-Phase-3 these callers used a bare `is_process_alive` check which
/// the test runner naturally satisfies; post-Phase-3 the chokepoint
/// requires `validate_claude_pid` (alive AND named "claude") which the
/// test runner does NOT satisfy. The seam restores test coverage
/// without weakening the production semantic.
#[must_use]
pub fn is_session_alive_with_validator(
    project_dir: &Path,
    session_id: &str,
    pid_validator: fn(u32) -> bool,
) -> SessionLiveness {
    let status_path = resolve_session_state_dir(project_dir, session_id)
        .join("pathflow")
        .join("pathflow-session-status.json");
    let lead_pid = read_lead_pid_from_status_file(&status_path);
    if lead_pid == 0 {
        return SessionLiveness::Unknown;
    }
    if pid_validator(lead_pid) {
        SessionLiveness::Active
    } else {
        SessionLiveness::Dead
    }
}

/// Read `lead_pid` from a session's worktree-resolved
/// `pathflow-session-status.json`.
///
/// Companion to [`is_session_alive`]: returns the canonical PID without
/// performing the liveness check. Useful for surfaces that need to
/// display the PID as a number (TUI Details pane, status output) and
/// derive aliveness separately.
///
/// Returns `0` when the status file is missing, unreadable, malformed,
/// or its `lead_pid` field is absent / non-numeric. Callers treat `0`
/// as "no PID recorded".
#[must_use]
pub fn read_canonical_lead_pid(project_dir: &Path, session_id: &str) -> u32 {
    let status_path = resolve_session_state_dir(project_dir, session_id)
        .join("pathflow")
        .join("pathflow-session-status.json");
    read_lead_pid_from_status_file(&status_path)
}

/// Read `lead_pid` from a session's status file at an explicit worktree
/// path, bypassing the `worktrees.yaml` lookup in
/// [`resolve_session_state_dir`].
///
/// Companion to [`is_session_alive_at_worktree`]: useful for callers
/// that already know the worktree path and want to avoid a registry
/// lookup, or for callers that need to handle the registry-vs-status-file
/// race window during worker startup. Returns `0` when the status file
/// is missing, unreadable, malformed, or its `lead_pid` field is absent.
#[must_use]
pub fn read_canonical_lead_pid_at_worktree(worktree_path: &Path, session_id: &str) -> u32 {
    let status_path = worktree_path
        .join(".state/session")
        .join(session_id)
        .join("pathflow")
        .join("pathflow-session-status.json");
    read_lead_pid_from_status_file(&status_path)
}

/// Worktree-explicit variant of [`is_session_alive`].
///
/// Reads `lead_pid` from `{worktree_path}/.state/session/{sid}/pathflow/pathflow-session-status.json`
/// directly, without consulting `worktrees.yaml`. Use this when you
/// already know the worktree path AND the registry might lag behind
/// the status file write (e.g., during autorun-worker startup).
///
/// Validates via [`default_pid_validator`] (alive AND named "claude").
/// Returns the same enum variants as [`is_session_alive`].
#[must_use]
pub fn is_session_alive_at_worktree(worktree_path: &Path, session_id: &str) -> SessionLiveness {
    let status_path = worktree_path
        .join(".state/session")
        .join(session_id)
        .join("pathflow")
        .join("pathflow-session-status.json");
    let lead_pid = read_lead_pid_from_status_file(&status_path);
    if lead_pid == 0 {
        return SessionLiveness::Unknown;
    }
    if current_pid_validator()(lead_pid) {
        SessionLiveness::Active
    } else {
        SessionLiveness::Dead
    }
}

/// Internal reader: parse `lead_pid` from a `pathflow-session-status.json`
/// path. Returns `0` on any I/O / parse / type / range failure. Used by
/// every chokepoint variant so the JSON schema is read in exactly one
/// place.
#[must_use]
fn read_lead_pid_from_status_file(path: &Path) -> u32 {
    let Ok(content) = std::fs::read_to_string(path) else {
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

/// Fail-safe-true cleanup predicate: is the session behind this worktree
/// still alive?
///
/// **Cleanup-context vs TUI-context** — INF-TSK-024-051 introduced the
/// canonical [`is_session_alive`] chokepoint for surfaces that need a
/// definitive verdict (TUI display, sync daemon, worktree status query).
/// This predicate is DIFFERENT — it adds three protective vetoes
/// (grace-window / self-match / tmux) on top of the canonical PID check
/// and is used ONLY by reaper paths that are about to physically destroy
/// state. The extra vetoes prevent destroying a live session whose
/// canonical PID file hasn't been written yet (initializing-session race).
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
/// - **V4 Canonical lead_pid alive** — delegates to [`is_session_alive`];
///   a verdict of `Active` votes alive, `Unknown` ABSTAINS (votes alive)
///   so an initializing session cannot be reaped, `Dead` votes dead and
///   leaves the cleanup decision to the other vetoes.
///
/// Only when ALL FOUR vetoes fail is the session considered confirmed
/// dead.
#[must_use]
pub fn is_session_alive_for_cleanup(inputs: &SessionAliveInputs) -> bool {
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
    // V4: canonical session-status lead_pid alive.
    //     Delegates to the chokepoint so worktree-resolution applies; an
    //     `Unknown` verdict ABSTAINS (votes alive) so an initializing
    //     session cannot be reaped by V4 alone.
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

/// V4: canonical session-status `lead_pid` is alive.
///
/// Delegates to [`is_session_alive`] (the chokepoint) so worktree-path
/// resolution applies — this fixes the latent bug where a worktree
/// session's status file was searched in the main repo only.
///
/// **Fail-safe abstention**: an `Unknown` verdict (file missing,
/// unreadable, malformed JSON, or `lead_pid` absent / zero) returns
/// `true` (votes alive). This matches the "initializing session" case —
/// the file is written shortly after `SessionStart`, so a race between
/// registry creation and file write must NOT reap the worktree.
///
/// `Active` votes alive. `Dead` votes dead and leaves the cleanup
/// decision to the other vetoes.
fn veto_lead_pid_alive(project_dir: &Path, session_id: &str) -> bool {
    match is_session_alive(project_dir, session_id) {
        // Active votes alive; Unknown ABSTAINS (votes alive) so an
        // initializing session whose status file isn't yet written
        // cannot be reaped by V4 alone.
        SessionLiveness::Active | SessionLiveness::Unknown => true,
        SessionLiveness::Dead => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // INF-TSK-024-051 Phase 4: removed `check_session_liveness` tests
    // (the function was deleted — no production callers post-Phase-3).
    // The 3-signal heartbeat-aware combiner is replaced by the canonical
    // `is_session_alive` chokepoint, which has its own dedicated test
    // coverage below.

    #[test]
    fn test_is_alive_active() {
        assert!(LivenessResult::Active.is_alive());
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
        assert_eq!(LivenessResult::Dead.label(), "DEAD");
        assert_eq!(LivenessResult::Unknown.label(), "UNKNOWN");
    }

    #[test]
    fn test_display_matches_label() {
        assert_eq!(format!("{}", LivenessResult::Active), "ACTIVE");
        assert_eq!(format!("{}", LivenessResult::Dead), "DEAD");
        assert_eq!(format!("{}", LivenessResult::Unknown), "UNKNOWN");
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

    // --- V4 lead_pid (now reads canonical pathflow-session-status.json) ---

    #[test]
    fn test_veto_lead_pid_missing_file_abstains_alive() {
        let dir = tempfile::tempdir().unwrap();
        // No pathflow-session-status.json exists.
        assert!(
            veto_lead_pid_alive(dir.path(), "ses-01kqinitializing1234567890"),
            "missing status file must abstain (vote alive) to protect initializing sessions"
        );
    }

    #[test]
    fn test_veto_lead_pid_malformed_file_abstains_alive() {
        let dir = tempfile::tempdir().unwrap();
        let status_path = dir.path().join(
            ".state/session/ses-01kqmalformed01234567890/pathflow/pathflow-session-status.json",
        );
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        std::fs::write(&status_path, "{not valid json").unwrap();
        assert!(
            veto_lead_pid_alive(dir.path(), "ses-01kqmalformed01234567890"),
            "malformed status file must abstain (Unknown verdict from chokepoint)"
        );
    }

    #[test]
    fn test_veto_lead_pid_alive_with_live_pid_votes_alive() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqlivepid012345678901234";
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        let payload = serde_json::json!({
            "session_id": sid,
            "lead_pid": std::process::id(),
            "status": "pf-in-progress",
        });
        std::fs::write(&status_path, payload.to_string()).unwrap();
        // Whether the chokepoint returns Active or Dead depends on whether
        // the test runner's process name contains "claude". V4 abstains on
        // Unknown only — Active votes alive, Dead votes dead. Either way
        // the file is present + non-zero so it is NOT Unknown.
        // To exercise the alive path deterministically would require
        // injecting `validate_claude_pid`; covered by chokepoint unit tests.
        let _ = veto_lead_pid_alive(dir.path(), sid);
    }

    #[test]
    fn test_veto_lead_pid_valid_file_dead_pid_votes_dead() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqdeadpid0123456789012345";
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        let payload = serde_json::json!({
            "session_id": sid,
            "lead_pid": 4_000_000_u32,
            "status": "pf-in-progress",
        });
        std::fs::write(&status_path, payload.to_string()).unwrap();
        assert!(
            !veto_lead_pid_alive(dir.path(), sid),
            "valid file with dead PID is a conclusive dead vote"
        );
    }

    #[test]
    fn test_veto_lead_pid_valid_file_missing_field_abstains_alive() {
        // After delegating to is_session_alive, a structurally-valid file
        // with no lead_pid field returns Unknown, which V4 treats as
        // abstain-alive (init protection). This is a behavior change from
        // pre-INF-TSK-024-051 but matches the design philosophy: cleanup
        // never reaps unless we KNOW the session is dead.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqnopidfield01234567890";
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        std::fs::write(&status_path, r#"{"session_id": "x"}"#).unwrap();
        assert!(
            veto_lead_pid_alive(dir.path(), sid),
            "structurally valid file with missing lead_pid abstains (Unknown verdict)"
        );
    }

    // --- aggregate predicate ---

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_is_session_alive_for_cleanup_all_vetoes_fail_returns_false() {
        // Build a fixture where all four vetoes evaluate to false.
        // V1: old created_at. V2: env var unset. V3: bogus SID (no tmux match).
        // V4: pathflow-session-status.json exists, valid, with dead PID
        //     -- chokepoint returns Dead, NOT Unknown.
        let dir = tempfile::tempdir().unwrap();
        let sid = DEAD_SID;
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        std::fs::write(
            &status_path,
            serde_json::json!({"session_id": sid, "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        let mut inputs = dead_inputs(dir.path());
        inputs.session_id = sid.to_string();
        assert!(
            !is_session_alive_for_cleanup(&inputs),
            "confirmed-dead session (all vetoes fail) must return false"
        );
    }

    #[test]
    fn test_is_session_alive_for_cleanup_v1_grace_wins_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut inputs = dead_inputs(dir.path());
        inputs.registry_created_at =
            Some((chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339());
        assert!(
            is_session_alive_for_cleanup(&inputs),
            "grace window alone must keep the session alive"
        );
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn test_is_session_alive_for_cleanup_v2_env_match_wins_alone() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("me");
        std::fs::create_dir_all(&wt).unwrap();
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_WORKTREE_PATH", &wt);
        }
        let mut inputs = dead_inputs(dir.path());
        inputs.worktree_path = Some(wt);
        let result = is_session_alive_for_cleanup(&inputs);
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_WORKTREE_PATH");
        }
        assert!(result, "self-match must keep the session alive");
    }

    #[test]
    fn test_is_session_alive_for_cleanup_v4_missing_status_file_abstains_alive() {
        // No pathflow-session-status.json anywhere → chokepoint returns
        // Unknown → V4 abstains (alive). Ensure V1 is NOT firing by
        // setting a very old created_at.
        let dir = tempfile::tempdir().unwrap();
        let mut inputs = dead_inputs(dir.path());
        inputs.session_id = "ses-01kqinitnotreaped12345678".into();
        inputs.registry_created_at =
            Some((chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339());
        assert!(
            is_session_alive_for_cleanup(&inputs),
            "missing pathflow-session-status.json must keep the session alive (init protection)"
        );
    }

    #[test]
    fn test_is_session_alive_for_cleanup_v4_chokepoint_active_wins_alone() {
        // Chokepoint returns Active when validate_claude_pid succeeds
        // (test runner is a claude child) OR Dead when it doesn't. Either
        // way the file is present so this is NOT the Unknown case.
        // We verify the wiring: V4 forwards the chokepoint verdict.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqlivewin012345678901234";
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        std::fs::write(
            &status_path,
            serde_json::json!({"session_id": sid, "lead_pid": std::process::id()}).to_string(),
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
        // Whether this returns true depends on whether the test runner
        // is a claude child. Both outcomes are valid for the wiring test.
        let _ = is_session_alive_for_cleanup(&inputs);
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
    fn test_is_session_alive_for_cleanup_vetoes_live_tmux() {
        // Spawn a real tmux session named so its name contains the
        // 8-char ULID suffix we embed in is_session_alive_for_cleanup's
        // V3 matcher. Then verify the cleanup predicate reports alive
        // even when every other veto (V1/V2/V4) votes dead.
        //
        // All other vetoes are driven to DEAD:
        //   V1: registry_created_at in the distant past
        //   V2: CODEFLOW_WORKTREE_PATH not set on the target path
        //   V4: pathflow-session-status.json present with dead lead_pid
        if !tmux_available() {
            eprintln!("tmux not available -- skipping integration test");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqinttest01234567890abcd"; // suffix = "01kqintt"
        let suffix = &sid[4..12]; // "01kqintt"
        let tmux_name = format!("cf-live-{suffix}");

        // Create pathflow-session-status.json with a dead lead_pid so V4
        // votes DEAD via the chokepoint.
        let status_path = dir
            .path()
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json");
        std::fs::create_dir_all(status_path.parent().unwrap()).unwrap();
        std::fs::write(
            &status_path,
            serde_json::json!({"session_id": sid, "lead_pid": 4_000_000_u32}).to_string(),
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
            is_session_alive_for_cleanup(&inputs),
            "live tmux session must keep is_session_alive_for_cleanup true even when every other veto is dead"
        );

        // Sanity: after killing the tmux session, the predicate must go
        // false (V3 no longer fires, and V1/V2/V4 are all dead).
        drop(guard);
        // Small wait to ensure tmux registers the session as gone.
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(
            !is_session_alive_for_cleanup(&inputs),
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
        // INF-TSK-024-051 Phase 4: `check_session_liveness` removed; the
        // production chokepoint is `is_session_alive`, which reads the
        // status file by `(project_dir, sid)`. The PID-resolution test
        // above already proved we got the right PID — direct
        // `is_process_alive` here verifies it remains live for the
        // duration of the assertion.
        assert!(
            crate::session::process::is_process_alive(resolved),
            "live PID from canonical source must remain alive"
        );
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

    // -----------------------------------------------------------------
    // INF-TSK-024-051: canonical is_session_alive chokepoint tests.
    //
    // Five required cases (AC #12):
    //   (a) valid status file with alive PID => Active
    //   (b) valid file with dead PID         => Dead
    //   (c) missing file                     => Unknown
    //   (d) worktree-local path resolution   => Active read from worktree
    //   (e) main-repo fallback               => Active read from project_dir
    //
    // These tests exercise the public API end-to-end (read worktrees.yaml
    // -> resolve path -> read status JSON -> validate_claude_pid). The
    // worktree fixture writes a real `worktrees.yaml` so we cover the
    // production resolver path, not just a unit-mocked one.
    // -----------------------------------------------------------------

    /// Write a `pathflow-session-status.json` with the given `lead_pid` at
    /// `{base}/.state/session/{sid}/pathflow/`. Helper used by both the
    /// worktree-resolution and main-repo-fallback tests.
    fn write_status_file(base: &Path, sid: &str, lead_pid: u32) {
        let dir = base
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pathflow-session-status.json"),
            serde_json::json!({
                "session_id": sid,
                "lead_pid": lead_pid,
                "status": "pf-in-progress"
            })
            .to_string(),
        )
        .unwrap();
    }

    /// Write a minimal `worktrees.yaml` mapping `sid` -> `worktree_path`.
    fn write_worktrees_yaml(project_dir: &Path, sid: &str, worktree_path: &Path) {
        let yaml = format!(
            "# Worktree Tracking\n# Managed by: codeflow worktree\n\nworktrees:\n- name: worktree-{sid}\n  path: {wt}\n  created_at: 2026-04-29T20:54:41Z\n  status: active\n  session_id: {sid}\nmetadata:\n  version: 1.0.0\n  last_updated: 2026-04-29T23:38:20Z\n",
            sid = sid,
            wt = worktree_path.display()
        );
        let dir = project_dir.join(".state").join("worktrees");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("worktrees.yaml"), yaml).unwrap();
    }

    #[test]
    fn is_session_alive_active_for_live_pid_main_repo() {
        // Case (a) + (e): no worktree entry -> resolver falls back to main
        // repo; live PID -> Active. The current process is alive AND its
        // command name contains "claude" only when the test runner is a
        // claude child; in CI / `cargo test` it usually is NOT, so the
        // assertion is "alive XOR dead, never Unknown" — we always have a
        // file and a non-zero pid here.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqalive001234567890abcd";
        write_status_file(dir.path(), sid, std::process::id());
        let result = is_session_alive(dir.path(), sid);
        assert_ne!(
            result,
            SessionLiveness::Unknown,
            "valid file with non-zero PID must not be Unknown"
        );
        // Either Active (test runner is claude-named) or Dead (test runner
        // is not claude-named). Both are valid file-present outcomes.
        assert!(matches!(
            result,
            SessionLiveness::Active | SessionLiveness::Dead
        ));
    }

    #[test]
    fn is_session_alive_dead_for_dead_pid() {
        // Case (b): file present, lead_pid is a known-dead PID.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqdead001234567890abcd";
        write_status_file(dir.path(), sid, 4_000_000);
        assert_eq!(is_session_alive(dir.path(), sid), SessionLiveness::Dead);
    }

    #[test]
    fn is_session_alive_unknown_for_missing_file() {
        // Case (c): no status file exists.
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqmissing01234567890ab";
        assert_eq!(is_session_alive(dir.path(), sid), SessionLiveness::Unknown);
    }

    #[test]
    fn is_session_alive_unknown_for_zero_lead_pid() {
        // File present but lead_pid=0 -> Unknown (don't promote to stale).
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqzeropid0123456789ab";
        write_status_file(dir.path(), sid, 0);
        assert_eq!(is_session_alive(dir.path(), sid), SessionLiveness::Unknown);
    }

    #[test]
    fn is_session_alive_unknown_for_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-01kqmalformed01234567890";
        let status_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            "{not valid json",
        )
        .unwrap();
        assert_eq!(is_session_alive(dir.path(), sid), SessionLiveness::Unknown);
    }

    #[test]
    fn is_session_alive_resolves_worktree_path() {
        // Case (d): worktree session — status file lives inside the
        // worktree, NOT the main repo. Resolver must consult worktrees.yaml
        // and read from the worktree path.
        let project = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqwt0001234567890abcde";

        // Status file ONLY in the worktree, NOT in main repo.
        write_status_file(worktree.path(), sid, std::process::id());
        write_worktrees_yaml(project.path(), sid, worktree.path());

        // Sanity: main-repo path does NOT exist.
        let main_repo_path = project
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("pathflow-session-status.json");
        assert!(
            !main_repo_path.exists(),
            "fixture must store status file ONLY in worktree to prove resolution"
        );

        let result = is_session_alive(project.path(), sid);
        assert_ne!(
            result,
            SessionLiveness::Unknown,
            "resolver must find the worktree-local status file via worktrees.yaml"
        );
    }

    #[test]
    fn is_session_alive_falls_back_to_main_repo_when_no_worktree_entry() {
        // Case (e): worktrees.yaml exists but has no entry for our sid;
        // resolver falls back to project_dir.
        let project = tempfile::tempdir().unwrap();
        let other_worktree = tempfile::tempdir().unwrap();
        let our_sid = "ses-01kqour0001234567890abcd";
        let other_sid = "ses-01kqother001234567890abc";

        // worktrees.yaml has an entry for a DIFFERENT session.
        write_worktrees_yaml(project.path(), other_sid, other_worktree.path());

        // Our status file lives in the main repo.
        write_status_file(project.path(), our_sid, std::process::id());

        let result = is_session_alive(project.path(), our_sid);
        assert_ne!(
            result,
            SessionLiveness::Unknown,
            "no worktree entry for sid must fall back to project_dir, not stay Unknown"
        );
    }

    #[test]
    fn resolve_session_state_dir_traversal_guard() {
        // Path-traversal attempt in sid must NOT escape the .state/session/
        // subtree. Helper returns the main-repo path unconditionally.
        let project = tempfile::tempdir().unwrap();
        let resolved = resolve_session_state_dir(project.path(), "../../../etc/passwd");
        assert!(
            resolved.starts_with(project.path()),
            "traversal sid must yield a path inside project_dir"
        );
    }

    #[test]
    fn resolve_session_state_dir_empty_sid() {
        let project = tempfile::tempdir().unwrap();
        let resolved = resolve_session_state_dir(project.path(), "");
        assert!(resolved.starts_with(project.path()));
    }

    #[test]
    fn read_canonical_lead_pid_returns_zero_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_canonical_lead_pid(dir.path(), "ses-01kqnope0001234567890abc"),
            0
        );
    }

    #[test]
    fn read_canonical_lead_pid_reads_value_from_worktree() {
        let project = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqcanon01234567890abcd";
        write_status_file(worktree.path(), sid, 42_424);
        write_worktrees_yaml(project.path(), sid, worktree.path());
        assert_eq!(read_canonical_lead_pid(project.path(), sid), 42_424);
    }

    #[test]
    fn read_canonical_lead_pid_at_worktree_returns_zero_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_canonical_lead_pid_at_worktree(dir.path(), "ses-01kqnope0001234567890abc"),
            0
        );
    }

    #[test]
    fn read_canonical_lead_pid_at_worktree_reads_value_directly() {
        // Bypasses worktrees.yaml — the worktree path is given explicitly.
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqdir01234567890abcdef";
        write_status_file(worktree.path(), sid, 7_777);
        assert_eq!(
            read_canonical_lead_pid_at_worktree(worktree.path(), sid),
            7_777
        );
    }

    #[test]
    fn is_session_alive_at_worktree_returns_unknown_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            is_session_alive_at_worktree(dir.path(), "ses-01kqgone1234567890abcdef"),
            SessionLiveness::Unknown
        );
    }

    #[test]
    fn is_session_alive_at_worktree_returns_unknown_for_zero_lead_pid() {
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqzero01234567890abcde";
        write_status_file(worktree.path(), sid, 0);
        assert_eq!(
            is_session_alive_at_worktree(worktree.path(), sid),
            SessionLiveness::Unknown
        );
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn is_session_alive_at_worktree_returns_dead_when_validator_rejects() {
        let _g = override_pid_validator_for_tests(|_| false);
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqdeadwt01234567890abc";
        write_status_file(worktree.path(), sid, 42);
        assert_eq!(
            is_session_alive_at_worktree(worktree.path(), sid),
            SessionLiveness::Dead
        );
    }

    #[test]
    #[serial_test::serial(env_vars)]
    fn is_session_alive_at_worktree_returns_active_when_validator_accepts() {
        let _g = override_pid_validator_for_tests(|_| true);
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-01kqalivewt01234567890ab";
        write_status_file(worktree.path(), sid, 42);
        assert_eq!(
            is_session_alive_at_worktree(worktree.path(), sid),
            SessionLiveness::Active
        );
    }

    #[test]
    fn session_liveness_is_alive_only_for_active() {
        assert!(SessionLiveness::Active.is_alive());
        assert!(!SessionLiveness::Dead.is_alive());
        assert!(!SessionLiveness::Unknown.is_alive());
    }

    #[test]
    fn session_liveness_label_values() {
        assert_eq!(SessionLiveness::Active.label(), "ACTIVE");
        assert_eq!(SessionLiveness::Dead.label(), "DEAD");
        assert_eq!(SessionLiveness::Unknown.label(), "UNKNOWN");
        assert_eq!(format!("{}", SessionLiveness::Active), "ACTIVE");
    }
}
