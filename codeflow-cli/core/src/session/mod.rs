//! Session lifecycle management.
//!
//! Consolidates Go's `session`, `workstate`, and `config` packages into a
//! unified Rust session module. Provides:
//!
//! - Session ID generation (ULID-based)
//! - Session state machine with validated transitions
//! - Builder pattern for session creation
//! - Environment file (`codeflow-env.sh`) management
//! - Active task file management
//! - Current session resolution (worktree-aware, env file only)

pub mod active_task;
pub mod autorun_detect;
pub mod env;
pub mod heartbeat;
pub mod liveness;
pub mod process;
pub mod sentinel;
mod state;

use std::path::Path;

use crate::error::SessionError;
use crate::types::SessionId;
use crate::worktree::WorktreePaths;

pub use active_task::{
    ActiveTask, active_task_path_resolved, clear_active_task, clear_active_task_worktree_aware,
    get_active_task, get_active_task_worktree_aware, set_active_task,
    set_active_task_worktree_aware,
};
pub use autorun_detect::is_autorun_session;
pub use env::{
    EnvFile, SessionPointer, migrate_runtime_layout, read_env_file, read_pid_env_file,
    read_session_pointer, remove_env_file, remove_pid_env_file, remove_session_pointer,
    write_env_file, write_env_file_with_worktree, write_pid_env_file, write_session_pointer,
};

/// Generate a new session ID using ULID format: `ses-{26-char-lowercase-ULID}`.
///
/// The ULID encodes the current timestamp and random component, providing
/// time-sortable, globally unique identifiers.
#[must_use]
pub fn generate_session_id() -> SessionId {
    let ulid = ulid::Ulid::new();
    let id = format!("ses-{}", ulid.to_string().to_lowercase());
    SessionId::new_unchecked(id)
}

/// Generate a session-scoped team name for Claude Agent Teams.
///
/// For planned tasks (with a known task format ID from an epic):
///   `{project}-{task_format_id}-{sid_short}`
///   e.g., `codeflow-inf-tsk-023-008-01kk0t08`
///
/// For adhoc tasks (no task format ID yet):
///   `{project}-{sid_short}`
///   e.g., `codeflow-01kk0t08`
///
/// `sid_short` is the first 8 characters of the ULID portion of the session ID
/// (characters 4..12, skipping the `ses-` prefix).
///
/// All components are lowercased for filesystem compatibility.
#[must_use]
pub fn generate_team_name(project: &str, task_format_id: Option<&str>, session_id: &str) -> String {
    let sid_short = extract_sid_short(session_id);
    match task_format_id {
        Some(fmt_id) => format!(
            "{}-{}-{}",
            project.to_lowercase(),
            fmt_id.to_lowercase(),
            sid_short
        ),
        None => format!("{}-{}", project.to_lowercase(), sid_short),
    }
}

/// Extract the short session ID (first 8 chars of the ULID portion).
///
/// Strips the `ses-` prefix and takes the first 8 characters. If the session
/// ID is non-standard (no `ses-` prefix or too short), falls back to the last
/// 8 characters of the full ID (or the entire ID if shorter than 8).
fn extract_sid_short(session_id: &str) -> String {
    if let Some(ulid) = session_id.strip_prefix("ses-") {
        if ulid.len() >= 8 {
            return ulid[..8].to_lowercase();
        }
    }
    // Fallback: use last 8 chars (or entire string if shorter).
    let start = session_id.len().saturating_sub(8);
    session_id[start..].to_lowercase()
}

/// Validate that a string matches the session ID format.
///
/// Accepts both:
/// - Legacy: `ses-{13-digit-timestamp}{12-hex-chars}`
/// - ULID: `ses-{26-char-lowercase-crockford-base32}`
#[must_use]
pub fn is_valid_session_id(value: &str) -> bool {
    if let Some(suffix) = value.strip_prefix("ses-") {
        if suffix.len() == 25 {
            // Legacy format: 13 digits + 12 hex chars
            let (ts, hex) = suffix.split_at(13);
            ts.chars().all(|c| c.is_ascii_digit())
                && hex
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        } else if suffix.len() == 26 {
            // ULID format: 26 chars of Crockford Base32 (lowercase)
            suffix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && !suffix.contains('i')
                && !suffix.contains('l')
                && !suffix.contains('o')
                && !suffix.contains('u')
        } else {
            false
        }
    } else {
        false
    }
}

/// Resolve the worktree path for this process.
///
/// Checks `CODEFLOW_WORKTREE_PATH` env var first, then falls back to the
/// per-PID env file (`codeflow-env-{pid}.sh`). Returns `None` if neither
/// source provides a worktree path.
///
/// The per-PID file is written during worktree setup and is scoped to the
/// process, avoiding the shared-file overwrite race in parallel sessions.
fn resolve_worktree_path(project_dir: &Path) -> Option<String> {
    let env_val = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    resolve_worktree_path_inner(project_dir, env_val.as_deref(), None)
}

/// Inner implementation with explicit env var value and optional PID override
/// for testability (avoids reading real env vars in tests).
fn resolve_worktree_path_inner(
    project_dir: &Path,
    env_var_value: Option<&str>,
    pid_override: Option<u32>,
) -> Option<String> {
    // 1. Env var takes priority (always correct when set by the same process).
    if let Some(wt_path) = env_var_value {
        if !wt_path.is_empty() {
            return Some(wt_path.to_string());
        }
    }
    // 2. Per-PID env file: use Claude Code's PID (grandparent), not our own PID.
    // Hook subprocesses are ephemeral (sh -c -> codeflow), so std::process::id()
    // won't match any per-PID file. Per-PID files are keyed by the Claude Code
    // process PID, which is our grandparent in the hook process tree.
    let pid = pid_override.unwrap_or_else(process::get_claude_code_pid);
    let runtime_dir = project_dir.join(".state").join("runtime");
    if let Some(wt_path) = read_pid_env_file(&runtime_dir, pid) {
        return Some(wt_path);
    }
    // read_pid_env_file now checks shared/ internally (via symlink in worktree).
    None
}

/// Check if the worktree registry has more than one active worktree.
///
/// Used to guard against reading the shared main env file when it could have
/// been overwritten by a parallel session. Returns false if the registry file
/// is missing or unreadable (safe default: allow fallback).
fn has_multiple_active_worktrees(project_dir: &Path) -> bool {
    has_multiple_active_worktrees_at(
        &project_dir
            .join(".state")
            .join("worktrees")
            .join("worktrees.yaml"),
    )
}

/// Inner implementation accepting a direct path for testability.
fn has_multiple_active_worktrees_at(registry_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(registry_path) else {
        return false;
    };
    let active_count = content
        .lines()
        .filter(|line| line.contains("status: active"))
        .count();
    active_count > 1
}

/// Resolve the current session ID from the env file (worktree-aware).
///
/// Resolution order:
/// 1. `CODEFLOW_WORKTREE_PATH` env var -> worktree's env file
/// 2. Per-PID env file (`codeflow-env-{pid}.sh`) -> worktree path -> worktree's env file
/// 3. Main project env file (`{project_dir}/.state/runtime/codeflow-env.sh`)
///
/// The per-PID file bridges a race condition in parallel worktree sessions:
/// both sessions write to the shared main env file, so the second overwrites
/// the first. The per-PID file is scoped to the process and points to the
/// correct worktree.
///
/// # Errors
///
/// Returns `SessionError::NoActiveSession` if no env file exists.
/// Returns `SessionError::Io` or `SessionError::InvalidSessionId` on file read
/// or parse errors.
pub fn current_session_id(project_dir: &Path) -> Result<SessionId, SessionError> {
    let worktree_path = resolve_worktree_path(project_dir);
    current_session_id_inner(project_dir, worktree_path.as_deref())
}

/// Inner implementation of `current_session_id` with explicit worktree path
/// parameter for testability (avoids `std::env::set_var` in tests).
fn current_session_id_inner(
    project_dir: &Path,
    worktree_path: Option<&str>,
) -> Result<SessionId, SessionError> {
    // Check worktree-local env file first (new layout: runtime/local/).
    if let Some(wt_path) = worktree_path {
        let wp = WorktreePaths::new(wt_path);
        if let Ok(Some(env_file)) = read_env_file(&wp.runtime_local_dir()) {
            return Ok(env_file.session_id);
        }
        // Backward compat: check old flat layout (runtime/).
        if let Ok(Some(env_file)) = read_env_file(&wp.runtime_dir()) {
            return Ok(env_file.session_id);
        }
    }
    // Before falling back to main env file, check if multiple worktrees are active.
    // If so, the main file is unreliable (could be clobbered by another session).
    if has_multiple_active_worktrees(project_dir) {
        return Err(SessionError::AmbiguousSession(
            "Multiple active worktrees detected; per-PID env file required but not found".into(),
        ));
    }
    // Safe: 0 or 1 active worktree -- main file is the only writer.
    // Check new layout (runtime/local/) first, then old flat layout.
    let runtime_dir = project_dir.join(".state").join("runtime");
    let local_dir = runtime_dir.join("local");
    if let Ok(Some(env_file)) = read_env_file(&local_dir) {
        return Ok(env_file.session_id);
    }
    match read_env_file(&runtime_dir)? {
        Some(env_file) => Ok(env_file.session_id),
        None => Err(SessionError::NoActiveSession),
    }
}

/// Resolve the current env file (worktree-aware).
///
/// Same resolution logic as `current_session_id` but returns the full
/// `EnvFile` struct. Needed by `SessionEnd` cleanup which checks env file
/// ownership before removal.
///
/// # Errors
///
/// Returns `SessionError::Io` or `SessionError::InvalidSessionId` on file
/// read or parse errors.
pub fn current_env_file(project_dir: &Path) -> Result<Option<EnvFile>, SessionError> {
    let worktree_path = resolve_worktree_path(project_dir);
    current_env_file_inner(project_dir, worktree_path.as_deref())
}

/// Inner implementation of `current_env_file` with explicit worktree path
/// parameter for testability.
fn current_env_file_inner(
    project_dir: &Path,
    worktree_path: Option<&str>,
) -> Result<Option<EnvFile>, SessionError> {
    // Check worktree-local env file first (new layout: runtime/local/).
    if let Some(wt_path) = worktree_path {
        let wp = WorktreePaths::new(wt_path);
        if let Ok(Some(env_file)) = read_env_file(&wp.runtime_local_dir()) {
            return Ok(Some(env_file));
        }
        // Backward compat: check old flat layout (runtime/).
        if let Ok(Some(env_file)) = read_env_file(&wp.runtime_dir()) {
            return Ok(Some(env_file));
        }
    }
    // Guard: refuse main file fallback when multiple worktrees are active.
    if has_multiple_active_worktrees(project_dir) {
        return Err(SessionError::AmbiguousSession(
            "Multiple active worktrees detected; per-PID env file required but not found".into(),
        ));
    }
    // Safe: 0 or 1 active worktree -- main file is the only writer.
    let runtime_dir = project_dir.join(".state").join("runtime");
    let local_dir = runtime_dir.join("local");
    if let Ok(Some(env_file)) = read_env_file(&local_dir) {
        return Ok(Some(env_file));
    }
    read_env_file(&runtime_dir)
}

/// Check if a session is a live worktree session by reading its session pointer.
///
/// Returns `true` if a session pointer exists, the worktree directory is present,
/// AND centralized liveness indicates the session is active or recent.
/// Returns `false` if no pointer, pointer unreadable, worktree gone, or session dead.
pub(crate) fn is_live_worktree_session(project_dir: &Path, sid: &str) -> bool {
    let Some(pointer) = read_session_pointer(project_dir, sid) else {
        return false;
    };
    let wt_path = std::path::Path::new(&pointer.worktree_path);
    if !wt_path.exists() {
        return false;
    }
    liveness::check_session_liveness(
        pointer.lead_pid,
        Some(wt_path),
        None,
        liveness::DEFAULT_HEARTBEAT_THRESHOLD_SECS,
    )
    .is_alive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_id_format() {
        let id = generate_session_id();
        let s = id.as_str();
        assert!(s.starts_with("ses-"), "should start with ses-: {s}");
        // ses- prefix + 26 char ULID = 30 chars total
        assert_eq!(s.len(), 30, "should be 30 chars: {s}");
    }

    #[test]
    fn test_generate_session_id_uniqueness() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2, "two generated IDs should differ");
    }

    #[test]
    fn test_generate_session_id_is_lowercase() {
        let id = generate_session_id();
        let suffix = &id.as_str()[4..]; // skip "ses-"
        assert_eq!(
            suffix,
            suffix.to_lowercase(),
            "ULID suffix should be lowercase: {suffix}"
        );
    }

    #[test]
    fn test_is_valid_session_id_ulid() {
        let id = generate_session_id();
        assert!(
            is_valid_session_id(id.as_str()),
            "generated ULID ID should be valid: {}",
            id.as_str()
        );
    }

    #[test]
    fn test_is_valid_session_id_legacy() {
        // 13 digits + 12 hex chars
        assert!(is_valid_session_id("ses-1771372021317a9e89b2d5688"));
    }

    #[test]
    fn test_is_valid_session_id_invalid_prefix() {
        assert!(!is_valid_session_id("session-abc123"));
    }

    #[test]
    fn test_is_valid_session_id_empty() {
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn test_is_valid_session_id_too_short() {
        assert!(!is_valid_session_id("ses-abc"));
    }

    #[test]
    fn test_is_valid_session_id_uppercase_rejected() {
        assert!(!is_valid_session_id("ses-01JQABCDEF0123456789ABCDEF"));
    }

    #[test]
    fn test_current_session_id_reads_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let sid = SessionId::new_unchecked("ses-01jq7envfiletest12345678");
        write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // No worktree -- reads from project dir.
        let result = current_session_id_inner(dir.path(), None).unwrap();
        assert_eq!(result, sid);
    }

    #[test]
    fn test_current_session_id_no_env_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let result = current_session_id_inner(dir.path(), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no active session")
        );
    }

    #[test]
    fn test_current_session_id_ignores_env_var() {
        // Even if CODEFLOW_SESSION_ID env var is set, current_session_id
        // should only read from the env file. We verify by having no env
        // file -- it should return NoActiveSession regardless of env var.
        let dir = tempfile::tempdir().unwrap();

        let result = current_session_id_inner(dir.path(), None);
        assert!(result.is_err(), "should not read env var, only env file");
    }

    /// INF-TSK-024-028 invariant: the env file is the SINGLE source of truth.
    /// The production resolver does NOT consult `CODEFLOW_SESSION_ID` at all
    /// when an env file exists — the env var is not a fallback, not a tiebreaker,
    /// not consulted, period. Even if the env var is set to a different value,
    /// the env file's SID is returned. This nails down the "no two session IDs"
    /// mandate from the user directive.
    #[test]
    #[serial_test::serial(env_vars)]
    fn test_current_session_id_ignores_env_var_when_file_present() {
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::set_var("CODEFLOW_SESSION_ID", "ses-01jq7envvarwouldbewrong0");
        }

        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let file_sid = SessionId::new_unchecked("ses-01jq7envfilewinscorrectk");
        write_env_file(&runtime_dir, &file_sid, "codeflow").unwrap();

        // Capture the result, then clean up env state BEFORE asserting so a
        // panic does not leak the env var to subsequent serialized tests.
        // (Project convention: see testing/runner/mod.rs:641.)
        let result = current_session_id_inner(dir.path(), None);
        // SAFETY: serialized via #[serial(env_vars)].
        unsafe {
            std::env::remove_var("CODEFLOW_SESSION_ID");
        }

        let sid = result.expect("current_session_id_inner must return Ok");
        assert_eq!(
            sid, file_sid,
            "env file is the single source; CODEFLOW_SESSION_ID env var must be ignored when the env file exists"
        );
    }

    #[test]
    fn test_is_valid_session_id_with_crockford_excluded_chars() {
        // Crockford Base32 excludes i, l, o, u — these should fail validation
        // Construct a 26-char string with an excluded char
        assert!(!is_valid_session_id("ses-01jq7abcdef0123456789il0a"));
    }

    // --- Worktree-aware current_session_id tests ---

    #[test]
    fn test_current_session_id_prefers_worktree_env() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let project_sid = SessionId::new_unchecked("ses-01jq7projectsid123456789");
        let worktree_sid = SessionId::new_unchecked("ses-01jq7worktreesid12345678");

        // Write env file to both locations.
        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        let wt_paths = WorktreePaths::new(worktree_dir.path());
        write_env_file_with_worktree(
            &wt_paths.runtime_dir(),
            &worktree_sid,
            "codeflow",
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();

        // With worktree path set, should prefer worktree env file.
        let result = current_session_id_inner(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();
        assert_eq!(
            result, worktree_sid,
            "should read session ID from worktree env file"
        );
    }

    #[test]
    fn test_current_session_id_falls_back_without_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let project_sid = SessionId::new_unchecked("ses-01jq7fallbacksid12345678");

        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // No worktree path -- falls back to project dir.
        let result = current_session_id_inner(project_dir.path(), None).unwrap();
        assert_eq!(
            result, project_sid,
            "should fall back to project env file when no worktree"
        );
    }

    #[test]
    fn test_current_session_id_worktree_missing_file_falls_back() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let project_sid = SessionId::new_unchecked("ses-01jq7missingwt1234567890");

        // Write env file only to project dir (NOT worktree).
        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // Worktree dir exists but has no env file -- should fall back.
        let result = current_session_id_inner(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();
        assert_eq!(
            result, project_sid,
            "should fall back to project env file when worktree env missing"
        );
    }

    // --- current_env_file tests ---

    #[test]
    fn test_current_env_file_returns_full_struct() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let wt_sid = SessionId::new_unchecked("ses-01jq7envfilefull12345678");
        let wt_path_str = worktree_dir.path().to_str().unwrap();

        let wt_paths = WorktreePaths::new(worktree_dir.path());
        write_env_file_with_worktree(
            &wt_paths.runtime_dir(),
            &wt_sid,
            "codeflow",
            Some(wt_path_str),
        )
        .unwrap();

        let env = current_env_file_inner(project_dir.path(), Some(wt_path_str))
            .unwrap()
            .expect("should return Some(EnvFile)");

        assert_eq!(env.session_id, wt_sid);
        assert_eq!(env.project_root, "codeflow");
        assert_eq!(
            env.worktree_path.as_deref(),
            Some(wt_path_str),
            "worktree_path field should be populated"
        );
    }

    #[test]
    fn test_current_env_file_falls_back_to_project() {
        let project_dir = tempfile::tempdir().unwrap();
        let project_sid = SessionId::new_unchecked("ses-01jq7envfallback12345678");

        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        let env = current_env_file_inner(project_dir.path(), None)
            .unwrap()
            .expect("should return Some(EnvFile)");

        assert_eq!(env.session_id, project_sid);
        assert!(
            env.worktree_path.is_none(),
            "project env file should not have worktree_path"
        );
    }

    #[test]
    fn test_current_env_file_none_when_no_env_file() {
        let project_dir = tempfile::tempdir().unwrap();

        let result = current_env_file_inner(project_dir.path(), None).unwrap();
        assert!(
            result.is_none(),
            "should return None when no env file exists"
        );
    }

    // --- generate_team_name tests ---

    #[test]
    fn test_generate_team_name_planned() {
        let name = generate_team_name(
            "codeflow",
            Some("INF-TSK-023-008"),
            "ses-01kk0t08ggabcdef12345678",
        );
        assert_eq!(name, "codeflow-inf-tsk-023-008-01kk0t08");
    }

    #[test]
    fn test_generate_team_name_adhoc() {
        let name = generate_team_name("codeflow", None, "ses-01kk0t08ggabcdef12345678");
        assert_eq!(name, "codeflow-01kk0t08");
    }

    #[test]
    fn test_generate_team_name_lowercase() {
        let name = generate_team_name(
            "CodeFlow",
            Some("INF-TSK-023-013"),
            "ses-01KK0T08GGABCDEF12345678",
        );
        // All components must be lowercased.
        assert_eq!(name, name.to_lowercase(), "team name must be all lowercase");
        assert!(
            name.starts_with("codeflow-"),
            "project should be lowercased: {name}"
        );
        assert!(
            name.contains("inf-tsk-023-013"),
            "format ID should be lowercased: {name}"
        );
    }

    #[test]
    fn test_generate_team_name_uniqueness() {
        let name1 = generate_team_name("codeflow", None, "ses-01kk0t08ggabcdef12345678");
        let name2 = generate_team_name("codeflow", None, "ses-01kk0t09ggabcdef12345678");
        assert_ne!(
            name1, name2,
            "different session IDs must produce different team names"
        );
    }

    #[test]
    fn test_generate_team_name_empty_project() {
        let name = generate_team_name("", Some("TSK-001"), "ses-01kk0t08ggabcdef12345678");
        assert_eq!(name, "-tsk-001-01kk0t08");
    }

    #[test]
    fn test_extract_sid_short_standard() {
        let short = extract_sid_short("ses-01kk0t08ggabcdef12345678");
        assert_eq!(short, "01kk0t08");
    }

    #[test]
    fn test_extract_sid_short_no_prefix() {
        // Non-standard ID without ses- prefix: fallback to last 8 chars.
        let short = extract_sid_short("abcdef1234567890");
        assert_eq!(short, "34567890");
    }

    #[test]
    fn test_extract_sid_short_short_id() {
        // Very short ID: return entire string.
        let short = extract_sid_short("abc");
        assert_eq!(short, "abc");
    }

    #[test]
    fn test_extract_sid_short_ses_prefix_short_ulid() {
        // ses- prefix but ULID portion is less than 8 chars: fallback.
        let short = extract_sid_short("ses-abc");
        assert_eq!(short, "ses-abc");
    }

    #[test]
    fn test_generate_team_name_with_real_session_id() {
        // Use a real generated session ID to verify integration.
        let sid = generate_session_id();
        let name = generate_team_name("myproject", Some("FIX-001"), sid.as_str());
        assert!(
            name.starts_with("myproject-fix-001-"),
            "should start with project-format_id-: {name}"
        );
        // sid_short should be 8 chars after the last hyphen.
        let parts: Vec<&str> = name.rsplitn(2, '-').collect();
        assert_eq!(
            parts[0].len(),
            8,
            "sid_short should be 8 chars: {}",
            parts[0]
        );
    }

    #[test]
    fn test_generate_team_name_filesystem_compatible() {
        let name = generate_team_name(
            "codeflow",
            Some("INF-TSK-023-013"),
            "ses-01kk0t08ggabcdef12345678",
        );
        // Team name should only contain lowercase alphanumerics and hyphens.
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "team name must be filesystem-compatible (lowercase alphanum + hyphens): {name}"
        );
    }

    // --- Additional coverage tests for is_valid_session_id ---

    #[test]
    fn test_is_valid_session_id_legacy_invalid_timestamp() {
        // Legacy format with non-digit in timestamp portion (first 13 chars).
        // 13 chars timestamp + 12 hex = 25 suffix chars.
        assert!(!is_valid_session_id("ses-177137202131xa9e89b2d5688"));
    }

    #[test]
    fn test_is_valid_session_id_legacy_uppercase_hex() {
        // Legacy format with uppercase hex chars (should fail).
        assert!(!is_valid_session_id("ses-1771372021317A9E89B2D5688"));
    }

    #[test]
    fn test_is_valid_session_id_wrong_length_suffix() {
        // Suffix is neither 25 (legacy) nor 26 (ULID) -- should fail.
        // 24-char suffix:
        assert!(!is_valid_session_id("ses-abcdefghijklmnopqrstuvwx"));
        // 27-char suffix:
        assert!(!is_valid_session_id("ses-abcdefghijklmnopqrstuvwxyz0"));
    }

    #[test]
    fn test_is_valid_session_id_ulid_with_excluded_i() {
        // Crockford Base32 excludes 'i' — must be exactly 26-char suffix.
        //                    1234567890123456789012345 6
        assert!(!is_valid_session_id("ses-01jq7abcdef012345678901iab"));
    }

    #[test]
    fn test_is_valid_session_id_ulid_with_excluded_l() {
        assert!(!is_valid_session_id("ses-01jq7abcdef012345678901lab"));
    }

    #[test]
    fn test_is_valid_session_id_ulid_with_excluded_o() {
        assert!(!is_valid_session_id("ses-01jq7abcdef012345678901oab"));
    }

    #[test]
    fn test_is_valid_session_id_ulid_with_excluded_u() {
        assert!(!is_valid_session_id("ses-01jq7abcdef012345678901uab"));
    }

    #[test]
    fn test_is_valid_session_id_valid_ulid_all_digits() {
        // 26-char all-digit suffix is valid Crockford Base32.
        assert!(is_valid_session_id("ses-01234567890123456789012345"));
    }

    #[test]
    fn test_is_valid_session_id_valid_ulid_mixed() {
        // Valid 26-char lowercase Crockford (no i/l/o/u).
        assert!(is_valid_session_id("ses-01jq7abcdef0123456789abcde"));
    }

    #[test]
    fn test_is_valid_session_id_prefix_only() {
        // Just "ses-" with empty suffix.
        assert!(!is_valid_session_id("ses-"));
    }

    #[test]
    fn test_is_valid_session_id_legacy_valid_boundary() {
        // Exactly 25-char suffix: 13 digits + 12 lowercase hex.
        assert!(is_valid_session_id("ses-1234567890123abcdef012345"));
    }

    // --- resolve_worktree_path tests ---

    #[test]
    fn test_resolve_worktree_path_from_pid_file() {
        let project_dir = tempfile::tempdir().unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        let fake_pid = 99990;

        // Write a per-PID env file pointing to a worktree.
        write_pid_env_file(&runtime_dir, fake_pid, "/path/to/worktree-a");

        // No env var, should read from PID file.
        let result = resolve_worktree_path_inner(project_dir.path(), None, Some(fake_pid));
        assert_eq!(result, Some("/path/to/worktree-a".to_string()));
    }

    #[test]
    fn test_resolve_worktree_path_no_pid_file_returns_none() {
        let project_dir = tempfile::tempdir().unwrap();
        let fake_pid = 99991;

        // No PID file exists and no env var.
        let result = resolve_worktree_path_inner(project_dir.path(), None, Some(fake_pid));
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_worktree_path_env_var_takes_priority() {
        let project_dir = tempfile::tempdir().unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        let fake_pid = 99997;

        // Write a per-PID file pointing to worktree-b.
        write_pid_env_file(&runtime_dir, fake_pid, "/path/to/worktree-b");

        // Env var points to worktree-a -- should take priority.
        let result = resolve_worktree_path_inner(
            project_dir.path(),
            Some("/path/to/worktree-a"),
            Some(fake_pid),
        );
        assert_eq!(result, Some("/path/to/worktree-a".to_string()));
    }

    #[test]
    fn test_resolve_worktree_path_empty_env_var_uses_pid_file() {
        let project_dir = tempfile::tempdir().unwrap();
        let runtime_dir = project_dir.path().join(".state").join("runtime");
        let fake_pid = 99998;

        write_pid_env_file(&runtime_dir, fake_pid, "/path/to/worktree-c");

        // Empty env var should be treated as unset, falling through to PID file.
        let result = resolve_worktree_path_inner(project_dir.path(), Some(""), Some(fake_pid));
        assert_eq!(result, Some("/path/to/worktree-c".to_string()));
    }

    #[test]
    fn test_current_session_id_via_pid_file_resolution() {
        // Simulate: env var not set, but per-PID file points to worktree
        // that has a valid codeflow-env.sh with session ID.
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();
        let fake_pid = 99992;

        let worktree_sid = SessionId::new_unchecked("ses-01jq7pidrestest123456789");
        let project_sid = SessionId::new_unchecked("ses-01jq7projectoverwritten9");

        // Write env file in main project (would be overwritten by session B).
        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // Write env file in worktree (correct for session A).
        let wt_paths = WorktreePaths::new(worktree_dir.path());
        write_env_file_with_worktree(
            &wt_paths.runtime_dir(),
            &worktree_sid,
            "codeflow",
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();

        // Write per-PID file pointing to the worktree.
        write_pid_env_file(
            &project_runtime,
            fake_pid,
            worktree_dir.path().to_str().unwrap(),
        );

        // Resolve using per-PID file (no env var).
        let wt_path = resolve_worktree_path_inner(project_dir.path(), None, Some(fake_pid));
        let result = current_session_id_inner(project_dir.path(), wt_path.as_deref()).unwrap();

        assert_eq!(
            result, worktree_sid,
            "should resolve session ID via per-PID file -> worktree env file"
        );
    }

    #[test]
    fn test_parallel_sessions_different_pid_files() {
        // Two parallel sessions with different PIDs should resolve to
        // their own worktrees via per-PID files.
        let project_dir = tempfile::tempdir().unwrap();
        let wt_a = tempfile::tempdir().unwrap();
        let wt_b = tempfile::tempdir().unwrap();

        let sid_a = SessionId::new_unchecked("ses-01jq7parallela1234567890");
        let sid_b = SessionId::new_unchecked("ses-01jq7parallelb1234567890");
        let pid_a: u32 = 99993;
        let pid_b: u32 = 99994;

        let project_runtime = project_dir.path().join(".state").join("runtime");

        // Write per-PID files.
        write_pid_env_file(&project_runtime, pid_a, wt_a.path().to_str().unwrap());
        write_pid_env_file(&project_runtime, pid_b, wt_b.path().to_str().unwrap());

        // Write worktree env files.
        let wt_a_paths = WorktreePaths::new(wt_a.path());
        write_env_file_with_worktree(
            &wt_a_paths.runtime_dir(),
            &sid_a,
            "codeflow",
            Some(wt_a.path().to_str().unwrap()),
        )
        .unwrap();

        let wt_b_paths = WorktreePaths::new(wt_b.path());
        write_env_file_with_worktree(
            &wt_b_paths.runtime_dir(),
            &sid_b,
            "codeflow",
            Some(wt_b.path().to_str().unwrap()),
        )
        .unwrap();

        // Each PID resolves to its own worktree (no env var).
        let wt_path_a = resolve_worktree_path_inner(project_dir.path(), None, Some(pid_a));
        let result_a = current_session_id_inner(project_dir.path(), wt_path_a.as_deref()).unwrap();

        let wt_path_b = resolve_worktree_path_inner(project_dir.path(), None, Some(pid_b));
        let result_b = current_session_id_inner(project_dir.path(), wt_path_b.as_deref()).unwrap();

        assert_eq!(result_a, sid_a, "PID A should resolve to session A");
        assert_eq!(result_b, sid_b, "PID B should resolve to session B");
        assert_ne!(result_a, result_b, "parallel sessions should differ");
    }

    #[test]
    fn test_resolve_worktree_path_pid_file_fallback_to_main() {
        // When no PID file and no env var, current_session_id should
        // fall back to the main project env file.
        let project_dir = tempfile::tempdir().unwrap();
        let project_sid = SessionId::new_unchecked("ses-01jq7nopidmain1234567890");
        let fake_pid = 99995;

        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // No PID file exists.
        let wt_path = resolve_worktree_path_inner(project_dir.path(), None, Some(fake_pid));
        assert!(wt_path.is_none(), "no PID file should yield None");

        let result = current_session_id_inner(project_dir.path(), wt_path.as_deref()).unwrap();
        assert_eq!(
            result, project_sid,
            "should fall back to main env file when no PID file"
        );
    }

    #[test]
    fn test_current_env_file_via_pid_file_resolution() {
        // current_env_file should also benefit from per-PID resolution.
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();
        let fake_pid = 99996;

        let worktree_sid = SessionId::new_unchecked("ses-01jq7envpidtest123456789");
        let wt_path_str = worktree_dir.path().to_str().unwrap();

        let wt_paths = WorktreePaths::new(worktree_dir.path());
        write_env_file_with_worktree(
            &wt_paths.runtime_dir(),
            &worktree_sid,
            "codeflow",
            Some(wt_path_str),
        )
        .unwrap();

        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_pid_env_file(&project_runtime, fake_pid, wt_path_str);

        // Resolve via PID file (no env var).
        let wt_path = resolve_worktree_path_inner(project_dir.path(), None, Some(fake_pid));
        let env = current_env_file_inner(project_dir.path(), wt_path.as_deref())
            .unwrap()
            .expect("should return Some(EnvFile)");

        assert_eq!(env.session_id, worktree_sid);
        assert_eq!(
            env.worktree_path.as_deref(),
            Some(wt_path_str),
            "worktree_path should be populated via PID file resolution"
        );
    }

    // --- has_multiple_active_worktrees tests ---

    #[test]
    fn test_has_multiple_active_worktrees_blocks_when_two_active() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("worktrees.yaml");
        std::fs::write(
            &registry_path,
            "worktrees:
             - name: wt-a
  status: active
             - name: wt-b
  status: active
",
        )
        .unwrap();
        assert!(has_multiple_active_worktrees_at(&registry_path));
    }

    #[test]
    fn test_has_multiple_active_worktrees_allows_single() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("worktrees.yaml");
        std::fs::write(
            &registry_path,
            "worktrees:
             - name: wt-a
  status: active
             - name: wt-b
  status: pending_cleanup
",
        )
        .unwrap();
        assert!(!has_multiple_active_worktrees_at(&registry_path));
    }

    #[test]
    fn test_has_multiple_active_worktrees_allows_when_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join("nonexistent.yaml");
        assert!(!has_multiple_active_worktrees_at(&registry_path));
    }

    #[test]
    fn test_current_session_id_refuses_main_when_multi_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let project_sid = SessionId::new_unchecked("ses-01jq7multiwtblock12345678");

        // Write env file to main project dir.
        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // Write registry with 2 active worktrees.
        let registry_dir = project_dir.path().join(".state").join("worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("worktrees.yaml"),
            "worktrees:
             - name: wt-a
  status: active
             - name: wt-b
  status: active
",
        )
        .unwrap();

        // No worktree path => tries main file fallback => blocked by registry guard.
        let result = current_session_id_inner(project_dir.path(), None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("ambiguous session"),
            "should return AmbiguousSession, got: {err_msg}"
        );
    }

    #[test]
    fn test_current_session_id_allows_main_when_single_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let project_sid = SessionId::new_unchecked("ses-01jq7singlewtallow12345");

        let project_runtime = project_dir.path().join(".state").join("runtime");
        write_env_file(&project_runtime, &project_sid, "codeflow").unwrap();

        // Write registry with 1 active worktree.
        let registry_dir = project_dir.path().join(".state").join("worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("worktrees.yaml"),
            "worktrees:
             - name: wt-a
  status: active
             - name: wt-b
  status: pending_cleanup
",
        )
        .unwrap();

        // Single active worktree => main file fallback is safe.
        let result = current_session_id_inner(project_dir.path(), None).unwrap();
        assert_eq!(result, project_sid);
    }
}
