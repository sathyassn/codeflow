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
    fs::create_dir_all(state_dir)?;

    let target = state_dir.join(ENV_FILENAME);
    let tmp_path = state_dir.join(format!(".{ENV_FILENAME}.tmp"));

    let content = format!(
        "export CODEFLOW_SESSION_ID='{}'\nexport CF_PROJECT_ROOT='{}'\n",
        session_id.as_str(),
        project_root
    );

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

/// Parse the content of a `codeflow-env.sh` file.
fn parse_env_content(content: &str) -> Result<EnvFile, SessionError> {
    let mut session_id = None;
    let mut project_root = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(value) = extract_export_value(line, "CODEFLOW_SESSION_ID") {
            session_id = Some(value.to_string());
        } else if let Some(value) = extract_export_value(line, "CF_PROJECT_ROOT") {
            project_root = Some(value.to_string());
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
}
