//! Codeflow environment file (`codeflow-env.sh`) read/write operations.
//!
//! The env file exports session environment variables and is sourced by shell
//! hooks. It contains:
//! ```sh
//! export CODEFLOW_SESSION_ID='ses-...'
//! export CF_PROJECT_ROOT='...'
//! ```
//!
//! Write operations use atomic tmp+rename for crash safety.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::SessionError;
use crate::types::SessionId;

/// Default filename for the environment file.
const ENV_FILENAME: &str = "codeflow-env.sh";

/// Parsed contents of a `codeflow-env.sh` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvFile {
    /// The session ID exported as `CODEFLOW_SESSION_ID`.
    pub session_id: SessionId,
    /// The project root exported as `CF_PROJECT_ROOT`.
    pub project_root: String,
    /// Optional worktree path exported as `CODEFLOW_WORKTREE_PATH`.
    /// Present when the session uses a worktree for isolation.
    /// Backward compatible: `None` when parsed from older env files.
    pub worktree_path: Option<String>,
}

/// Read and parse a `codeflow-env.sh` file from the given directory.
///
/// Returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns `SessionError::Io` on read failure, `SessionError::InvalidSessionId`
/// if the file cannot be parsed.
pub fn read_env_file(state_dir: &Path) -> Result<Option<EnvFile>, SessionError> {
    let path = state_dir.join(ENV_FILENAME);
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    parse_env_content(&content).map(Some)
}

/// Write a `codeflow-env.sh` file atomically using tmp+rename.
///
/// # Errors
///
/// Returns `SessionError::Io` on write or rename failure.
pub fn write_env_file(
    state_dir: &Path,
    session_id: &SessionId,
    project_root: &str,
) -> Result<PathBuf, SessionError> {
    write_env_file_with_worktree(state_dir, session_id, project_root, None)
}

/// Write a `codeflow-env.sh` file with an optional worktree path.
///
/// When `worktree_path` is `Some`, adds `CODEFLOW_WORKTREE_PATH` to the env file.
/// This is used during worktree-enabled sessions so that compact/resume can
/// recover the worktree location.
///
/// # Errors
///
/// Returns `SessionError::Io` on write or rename failure.
pub fn write_env_file_with_worktree(
    state_dir: &Path,
    session_id: &SessionId,
    project_root: &str,
    worktree_path: Option<&str>,
) -> Result<PathBuf, SessionError> {
    fs::create_dir_all(state_dir)?;

    let target = state_dir.join(ENV_FILENAME);
    let tmp_path = state_dir.join(format!(".{ENV_FILENAME}.tmp"));

    let mut content = format!(
        "export CODEFLOW_SESSION_ID='{}'\nexport CF_PROJECT_ROOT='{}'\n",
        session_id.as_str(),
        project_root
    );

    if let Some(wt_path) = worktree_path {
        use std::fmt::Write as _;
        let _ = writeln!(content, "export CODEFLOW_WORKTREE_PATH='{wt_path}'");
    }

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, &target)?;

    Ok(target)
}

/// Remove the `codeflow-env.sh` file from the given directory.
///
/// Returns `Ok(())` if the file does not exist (idempotent).
///
/// # Errors
///
/// Returns `SessionError::Io` on removal failure (other than not-found).
pub fn remove_env_file(state_dir: &Path) -> Result<(), SessionError> {
    let path = state_dir.join(ENV_FILENAME);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SessionError::Io(e)),
    }
}

// ---------------------------------------------------------------------------
// Per-PID env file operations
// ---------------------------------------------------------------------------

/// Filename pattern for per-PID env files: `codeflow-env-{pid}.sh`.
const PID_ENV_PREFIX: &str = "codeflow-env-";
const PID_ENV_SUFFIX: &str = ".sh";

/// Write a per-PID env file with the worktree path.
///
/// Creates `{runtime_dir}/codeflow-env-{pid}.sh` containing the
/// `CODEFLOW_WORKTREE_PATH` export. Each Claude Code process gets its own
/// file, avoiding the shared-file overwrite problem with parallel sessions.
///
/// Non-fatal: errors are silently ignored (caller should use `let _ =`).
pub fn write_pid_env_file(runtime_dir: &Path, pid: u32, worktree_path: &str) {
    let shared = runtime_dir.join("shared");
    let _ = fs::create_dir_all(&shared);
    let filename = format!("{PID_ENV_PREFIX}{pid}{PID_ENV_SUFFIX}");
    let path = shared.join(filename);
    let content = format!("export CODEFLOW_WORKTREE_PATH='{worktree_path}'\n");
    let _ = fs::write(&path, content);
}

/// Read `CODEFLOW_WORKTREE_PATH` from a per-PID env file.
///
/// Looks up `{runtime_dir}/codeflow-env-{pid}.sh` and extracts the
/// worktree path. Returns `None` if the file doesn't exist, can't be read,
/// or doesn't contain the variable.
#[must_use]
pub fn read_pid_env_file(runtime_dir: &Path, pid: u32) -> Option<String> {
    let filename = format!("{PID_ENV_PREFIX}{pid}{PID_ENV_SUFFIX}");
    // New layout: check shared/ subdirectory first.
    let shared_path = runtime_dir.join("shared").join(&filename);
    if let Some(value) = extract_worktree_path_from_file(&shared_path) {
        return Some(value);
    }
    // Backward compat: check old flat layout.
    let flat_path = runtime_dir.join(&filename);
    extract_worktree_path_from_file(&flat_path)
}

/// Extract CODEFLOW_WORKTREE_PATH from a file.
fn extract_worktree_path_from_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("export CODEFLOW_WORKTREE_PATH=") {
            let value = rest.trim_matches('\'').trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Remove a per-PID env file. Idempotent (no error if file doesn't exist).
pub fn remove_pid_env_file(runtime_dir: &Path, pid: u32) {
    let filename = format!("{PID_ENV_PREFIX}{pid}{PID_ENV_SUFFIX}");
    let _ = fs::remove_file(runtime_dir.join("shared").join(&filename));
    let _ = fs::remove_file(runtime_dir.join(&filename));
}

// ---------------------------------------------------------------------------
// Runtime layout migration (flat -> shared/local)
// ---------------------------------------------------------------------------

/// Migrate runtime directory from flat layout to shared/local layout.
///
/// Idempotent: creates subdirectories if missing, moves files from old flat
/// layout to the correct subdirectory. Safe to call on every SessionStart.
pub fn migrate_runtime_layout(runtime_dir: &Path) {
    let shared = runtime_dir.join("shared");
    let local = runtime_dir.join("local");
    let _ = fs::create_dir_all(&shared);
    let _ = fs::create_dir_all(&local);

    // Move per-PID files to shared/.
    if let Ok(entries) = fs::read_dir(runtime_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(PID_ENV_PREFIX) && name.ends_with(PID_ENV_SUFFIX) {
                let _ = fs::rename(entry.path(), shared.join(&name));
            }
        }
    }

    // Move session-scoped files to local/.
    for name in [
        "codeflow-env.sh",
        "active-task.json",
        "peer-id",
        "heartbeat",
        "worker-exit-code",
    ] {
        let old = runtime_dir.join(name);
        if old.exists() && !old.is_symlink() {
            let _ = fs::rename(&old, local.join(name));
        }
    }

    // Move cross-session files to shared/.
    {
        let name = "session-worktree-map.json";
        let old = runtime_dir.join(name);
        if old.exists() && !old.is_symlink() {
            let _ = fs::rename(&old, shared.join(name));
        }
    }
}

// ---------------------------------------------------------------------------
// Session pointer operations
// ---------------------------------------------------------------------------

/// Session pointer filename.
const SESSION_POINTER_FILENAME: &str = "session-pointer.json";

/// Session pointer data stored in the main repo for cross-session discovery.
///
/// INF-TSK-024-050 AC #2: the `lead_pid` field was removed. The canonical
/// liveness PID lives in `pathflow-session-status.json::lead_pid`, written
/// via `validate_claude_pid(parent_id())` from session-start hooks. The
/// pointer's role is reduced to recording the worktree path so that
/// compact/resume/teammate detection can find the worktree when env
/// vars are lost.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionPointer {
    pub worktree_path: String,
    pub session_id: String,
    pub created_at: String,
}

/// Write a session pointer to the main repo's session directory.
///
/// The pointer lives at `{project_dir}/.state/session/{sid}/session-pointer.json`
/// and records the worktree path so that compact/resume/teammate detection can
/// find the worktree even when env vars are lost.
///
/// Non-fatal: errors are silently ignored.
pub fn write_session_pointer(
    project_dir: &Path,
    session_id: &str,
    worktree_path: &str,
    created_at: &str,
) {
    let pointer_dir = project_dir.join(".state").join("session").join(session_id);
    let _ = fs::create_dir_all(&pointer_dir);
    let pointer = SessionPointer {
        worktree_path: worktree_path.to_string(),
        session_id: session_id.to_string(),
        created_at: created_at.to_string(),
    };
    let _ = fs::write(
        pointer_dir.join(SESSION_POINTER_FILENAME),
        serde_json::to_string_pretty(&pointer).unwrap_or_default(),
    );
}

/// Read a session pointer from the main repo's session directory.
///
/// Returns `None` if the pointer file doesn't exist or can't be parsed.
#[must_use]
pub fn read_session_pointer(project_dir: &Path, session_id: &str) -> Option<SessionPointer> {
    let path = project_dir
        .join(".state")
        .join("session")
        .join(session_id)
        .join(SESSION_POINTER_FILENAME);
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove a session pointer. Idempotent.
pub fn remove_session_pointer(project_dir: &Path, session_id: &str) {
    let path = project_dir
        .join(".state")
        .join("session")
        .join(session_id)
        .join(SESSION_POINTER_FILENAME);
    let _ = fs::remove_file(&path);
}

/// Parse the content of a `codeflow-env.sh` file.
fn parse_env_content(content: &str) -> Result<EnvFile, SessionError> {
    let mut session_id = None;
    let mut project_root = None;
    let mut worktree_path = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(value) = extract_export_value(line, "CODEFLOW_SESSION_ID") {
            session_id = Some(value.to_string());
        } else if let Some(value) = extract_export_value(line, "CF_PROJECT_ROOT") {
            project_root = Some(value.to_string());
        } else if let Some(value) = extract_export_value(line, "CODEFLOW_WORKTREE_PATH") {
            worktree_path = Some(value.to_string());
        }
    }

    let sid_str = session_id.ok_or_else(|| {
        SessionError::InvalidSessionId("CODEFLOW_SESSION_ID not found in env file".into())
    })?;

    let sid =
        SessionId::new(&sid_str).map_err(|e| SessionError::InvalidSessionId(e.to_string()))?;

    let root = project_root.ok_or_else(|| {
        SessionError::InvalidSessionId("CF_PROJECT_ROOT not found in env file".into())
    })?;

    Ok(EnvFile {
        session_id: sid,
        project_root: root,
        worktree_path,
    })
}

/// Extract the value from a line like `export VAR_NAME='value'`.
fn extract_export_value<'a>(line: &'a str, var_name: &str) -> Option<&'a str> {
    let prefix = format!("export {var_name}=");
    let rest = line.strip_prefix(&prefix)?;
    // Strip surrounding quotes (single or double).
    let rest = rest.trim();
    if (rest.starts_with('\'') && rest.ends_with('\''))
        || (rest.starts_with('"') && rest.ends_with('"'))
    {
        Some(&rest[1..rest.len() - 1])
    } else {
        Some(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7abc123xyz456def789");

        write_env_file(dir.path(), &sid, "codeflow").unwrap();

        let env = read_env_file(dir.path()).unwrap().unwrap();
        assert_eq!(env.session_id, sid);
        assert_eq!(env.project_root, "codeflow");
    }

    #[test]
    fn test_read_env_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_env_file(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_write_env_file_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("runtime");
        let sid = SessionId::new_unchecked("ses-test");

        write_env_file(&nested, &sid, "proj").unwrap();

        assert!(nested.join(ENV_FILENAME).exists());
    }

    #[test]
    fn test_remove_env_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-rm");

        write_env_file(dir.path(), &sid, "proj").unwrap();
        assert!(dir.path().join(ENV_FILENAME).exists());

        remove_env_file(dir.path()).unwrap();
        assert!(!dir.path().join(ENV_FILENAME).exists());
    }

    #[test]
    fn test_remove_env_file_not_found_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Should not error when file does not exist.
        remove_env_file(dir.path()).unwrap();
    }

    #[test]
    fn test_parse_env_content_single_quotes() {
        let content = "export CODEFLOW_SESSION_ID='ses-abc123'\nexport CF_PROJECT_ROOT='myproj'\n";
        let env = parse_env_content(content).unwrap();
        assert_eq!(env.session_id.as_str(), "ses-abc123");
        assert_eq!(env.project_root, "myproj");
    }

    #[test]
    fn test_parse_env_content_double_quotes() {
        let content =
            "export CODEFLOW_SESSION_ID=\"ses-abc123\"\nexport CF_PROJECT_ROOT=\"myproj\"\n";
        let env = parse_env_content(content).unwrap();
        assert_eq!(env.session_id.as_str(), "ses-abc123");
        assert_eq!(env.project_root, "myproj");
    }

    #[test]
    fn test_parse_env_content_no_quotes() {
        let content = "export CODEFLOW_SESSION_ID=ses-abc123\nexport CF_PROJECT_ROOT=myproj\n";
        let env = parse_env_content(content).unwrap();
        assert_eq!(env.session_id.as_str(), "ses-abc123");
        assert_eq!(env.project_root, "myproj");
    }

    #[test]
    fn test_parse_env_content_missing_session_id() {
        let content = "export CF_PROJECT_ROOT='myproj'\n";
        let result = parse_env_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_env_content_missing_project_root() {
        let content = "export CODEFLOW_SESSION_ID='ses-abc'\n";
        let result = parse_env_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_env_file_content_format() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-format-check");

        write_env_file(dir.path(), &sid, "codeflow").unwrap();

        let content = fs::read_to_string(dir.path().join(ENV_FILENAME)).unwrap();
        assert!(content.contains("export CODEFLOW_SESSION_ID='ses-format-check'"));
        assert!(content.contains("export CF_PROJECT_ROOT='codeflow'"));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn test_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let sid1 = SessionId::new_unchecked("ses-first");
        let sid2 = SessionId::new_unchecked("ses-second");

        write_env_file(dir.path(), &sid1, "proj").unwrap();
        write_env_file(dir.path(), &sid2, "proj").unwrap();

        let env = read_env_file(dir.path()).unwrap().unwrap();
        assert_eq!(env.session_id, sid2);
    }

    // --- CODEFLOW_WORKTREE_PATH tests ---

    #[test]
    fn test_write_and_read_env_file_with_worktree_path() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7wt123abc456def78901");

        write_env_file_with_worktree(
            dir.path(),
            &sid,
            "codeflow",
            Some("/project/.git-worktrees/worktree-ses-abc"),
        )
        .unwrap();

        let env = read_env_file(dir.path()).unwrap().unwrap();
        assert_eq!(env.session_id, sid);
        assert_eq!(env.project_root, "codeflow");
        assert_eq!(
            env.worktree_path.as_deref(),
            Some("/project/.git-worktrees/worktree-ses-abc")
        );
    }

    #[test]
    fn test_read_env_file_backward_compat_no_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7backcompat123456789");

        // Write WITHOUT worktree path (old format).
        write_env_file(dir.path(), &sid, "codeflow").unwrap();

        let env = read_env_file(dir.path()).unwrap().unwrap();
        assert_eq!(env.session_id, sid);
        assert!(
            env.worktree_path.is_none(),
            "missing CODEFLOW_WORKTREE_PATH should parse as None"
        );
    }

    #[test]
    fn test_write_env_file_with_worktree_path_format() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7format123456789abc");

        write_env_file_with_worktree(
            dir.path(),
            &sid,
            "codeflow",
            Some("/project/.git-worktrees/wt-test"),
        )
        .unwrap();

        let content = fs::read_to_string(dir.path().join(ENV_FILENAME)).unwrap();
        assert!(content.contains("export CODEFLOW_SESSION_ID='ses-01jq7format123456789abc'"));
        assert!(content.contains("export CF_PROJECT_ROOT='codeflow'"));
        assert!(
            content.contains("export CODEFLOW_WORKTREE_PATH='/project/.git-worktrees/wt-test'")
        );
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn test_write_env_file_with_worktree_none_omits_line() {
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7nonewt1234567890abc");

        write_env_file_with_worktree(dir.path(), &sid, "codeflow", None).unwrap();

        let content = fs::read_to_string(dir.path().join(ENV_FILENAME)).unwrap();
        assert!(!content.contains("CODEFLOW_WORKTREE_PATH"));
    }

    #[test]
    fn test_parse_env_content_with_worktree_path() {
        let content = "export CODEFLOW_SESSION_ID='ses-abc123'\n\
                        export CF_PROJECT_ROOT='myproj'\n\
                        export CODEFLOW_WORKTREE_PATH='/project/.git-worktrees/wt-1'\n";
        let env = parse_env_content(content).unwrap();
        assert_eq!(env.session_id.as_str(), "ses-abc123");
        assert_eq!(env.project_root, "myproj");
        assert_eq!(
            env.worktree_path.as_deref(),
            Some("/project/.git-worktrees/wt-1")
        );
    }

    #[test]
    fn test_two_worktrees_independent_env_files() {
        use std::sync::Arc;
        use std::thread;

        let wt1 = tempfile::tempdir().unwrap();
        let wt2 = tempfile::tempdir().unwrap();

        let sid1 = SessionId::new_unchecked("ses-01jq7concwt1test12345678");
        let sid2 = SessionId::new_unchecked("ses-01jq7concwt2test12345678");

        let wt1_runtime = wt1.path().join(".state").join("runtime");
        let wt2_runtime = wt2.path().join(".state").join("runtime");

        // Pre-create directories so both threads can start writing immediately.
        fs::create_dir_all(&wt1_runtime).unwrap();
        fs::create_dir_all(&wt2_runtime).unwrap();

        let wt1_path = Arc::new(wt1_runtime.clone());
        let wt2_path = Arc::new(wt2_runtime.clone());
        let sid1_c = sid1.clone();
        let sid2_c = sid2.clone();
        let wt1_str = wt1.path().to_str().unwrap().to_string();
        let wt2_str = wt2.path().to_str().unwrap().to_string();

        // Write env files from two threads concurrently.
        let h1 = thread::spawn(move || {
            write_env_file_with_worktree(&wt1_path, &sid1_c, "codeflow", Some(&wt1_str)).unwrap();
        });
        let h2 = thread::spawn(move || {
            write_env_file_with_worktree(&wt2_path, &sid2_c, "codeflow", Some(&wt2_str)).unwrap();
        });

        h1.join().expect("thread 1 panicked");
        h2.join().expect("thread 2 panicked");

        // Verify each worktree retained its own session ID.
        let env1 = read_env_file(&wt1_runtime)
            .unwrap()
            .expect("wt1 env missing");
        let env2 = read_env_file(&wt2_runtime)
            .unwrap()
            .expect("wt2 env missing");

        assert_eq!(env1.session_id, sid1, "worktree 1 should have its own SID");
        assert_eq!(env2.session_id, sid2, "worktree 2 should have its own SID");
        assert_ne!(
            env1.session_id, env2.session_id,
            "two worktrees must have different SIDs"
        );

        // Verify worktree_path fields are distinct.
        assert_ne!(
            env1.worktree_path, env2.worktree_path,
            "worktree paths must differ"
        );
    }

    // --- Per-PID env file tests ---

    #[test]
    fn test_write_and_read_pid_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        write_pid_env_file(&runtime_dir, 12345, "/path/to/worktree");
        let result = read_pid_env_file(&runtime_dir, 12345);
        assert_eq!(result, Some("/path/to/worktree".to_string()));
    }

    #[test]
    fn test_read_pid_env_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_pid_env_file(dir.path(), 99999);
        assert!(result.is_none());
    }

    #[test]
    fn test_remove_pid_env_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        write_pid_env_file(dir.path(), 12345, "/path/to/wt");
        assert!(
            dir.path()
                .join("shared")
                .join("codeflow-env-12345.sh")
                .exists()
        );
        remove_pid_env_file(dir.path(), 12345);
        assert!(
            !dir.path()
                .join("shared")
                .join("codeflow-env-12345.sh")
                .exists()
        );
    }

    #[test]
    fn test_remove_pid_env_file_not_found_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic or error.
        remove_pid_env_file(dir.path(), 99999);
    }

    #[test]
    fn test_pid_env_file_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("runtime");
        write_pid_env_file(&nested, 42, "/wt");
        assert!(nested.join("shared").join("codeflow-env-42.sh").exists());
    }

    #[test]
    fn test_parallel_pid_env_files_independent() {
        let dir = tempfile::tempdir().unwrap();
        write_pid_env_file(dir.path(), 100, "/wt-a");
        write_pid_env_file(dir.path(), 200, "/wt-b");

        assert_eq!(
            read_pid_env_file(dir.path(), 100),
            Some("/wt-a".to_string())
        );
        assert_eq!(
            read_pid_env_file(dir.path(), 200),
            Some("/wt-b".to_string())
        );
    }

    #[test]
    fn test_pid_env_file_empty_value_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("codeflow-env-999.sh");
        fs::write(&path, "export CODEFLOW_WORKTREE_PATH=''\n").unwrap();
        assert!(read_pid_env_file(dir.path(), 999).is_none());
    }

    // --- Session pointer tests ---

    #[test]
    fn test_write_and_read_session_pointer() {
        let dir = tempfile::tempdir().unwrap();
        write_session_pointer(
            dir.path(),
            "ses-test123",
            "/path/to/worktree",
            "2026-03-29T00:00:00Z",
        );
        let pointer = read_session_pointer(dir.path(), "ses-test123");
        assert!(pointer.is_some());
        let p = pointer.unwrap();
        assert_eq!(p.worktree_path, "/path/to/worktree");
        assert_eq!(p.session_id, "ses-test123");
        assert_eq!(p.created_at, "2026-03-29T00:00:00Z");
    }

    #[test]
    fn test_read_session_pointer_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_session_pointer(dir.path(), "ses-nonexistent").is_none());
    }

    #[test]
    fn test_remove_session_pointer() {
        let dir = tempfile::tempdir().unwrap();
        write_session_pointer(dir.path(), "ses-rm", "/wt", "now");
        assert!(read_session_pointer(dir.path(), "ses-rm").is_some());
        remove_session_pointer(dir.path(), "ses-rm");
        assert!(read_session_pointer(dir.path(), "ses-rm").is_none());
    }

    #[test]
    fn test_remove_session_pointer_not_found_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic.
        remove_session_pointer(dir.path(), "ses-nonexistent");
    }

    #[test]
    fn test_session_pointer_not_written_for_non_worktree() {
        let dir = tempfile::tempdir().unwrap();
        // Verify: pointer only exists when explicitly written.
        assert!(read_session_pointer(dir.path(), "ses-nowt").is_none());
    }

    // --- Runtime layout migration tests ---

    #[test]
    fn test_migrate_runtime_layout_creates_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        migrate_runtime_layout(&runtime);
        assert!(runtime.join("shared").is_dir());
        assert!(runtime.join("local").is_dir());
    }

    #[test]
    fn test_migrate_runtime_layout_moves_pid_files_to_shared() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("codeflow-env-12345.sh"), "x").unwrap();
        migrate_runtime_layout(&runtime);
        assert!(
            runtime
                .join("shared")
                .join("codeflow-env-12345.sh")
                .exists()
        );
    }

    #[test]
    fn test_migrate_runtime_layout_moves_session_files_to_local() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("active-task.json"), "{}").unwrap();
        migrate_runtime_layout(&runtime);
        assert!(runtime.join("local").join("active-task.json").exists());
    }

    #[test]
    fn test_migrate_runtime_layout_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        migrate_runtime_layout(&runtime);
        migrate_runtime_layout(&runtime);
        assert!(runtime.join("shared").is_dir());
    }

    #[test]
    fn test_migrate_runtime_layout_does_not_move_session_lock() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("session.lock"), "lock").unwrap();
        migrate_runtime_layout(&runtime);
        // session.lock should remain in place (not moved to shared/).
        assert!(
            runtime.join("session.lock").exists(),
            "session.lock should NOT be migrated"
        );
        assert!(
            !runtime.join("shared").join("session.lock").exists(),
            "session.lock should NOT appear in shared/"
        );
    }

    #[test]
    fn test_write_pid_env_file_uses_shared_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        write_pid_env_file(&runtime, 42, "/path/to/wt");
        assert!(runtime.join("shared").join("codeflow-env-42.sh").exists());
    }

    #[test]
    fn test_read_pid_env_file_finds_shared_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        let shared = runtime.join("shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(
            shared.join("codeflow-env-99.sh"),
            "export CODEFLOW_WORKTREE_PATH='/wt'\n",
        )
        .unwrap();
        let result = read_pid_env_file(&runtime, 99);
        assert_eq!(result, Some("/wt".to_string()));
    }

    #[test]
    fn test_read_pid_env_file_backward_compat_flat() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(
            runtime.join("codeflow-env-88.sh"),
            "export CODEFLOW_WORKTREE_PATH='/wt'\n",
        )
        .unwrap();
        let result = read_pid_env_file(&runtime, 88);
        assert_eq!(result, Some("/wt".to_string()));
    }
}
