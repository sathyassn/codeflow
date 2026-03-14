//! Sentinel file CRUD operations.
//!
//! Sentinels are empty marker files stored under
//! `.state/sentinels/pathflow/{session_id}/pathflow-{name}`.
//! They gate `PathFlow` phase and stage progression.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::PathflowError;
use crate::types::Sentinel;

/// The prefix prepended to all sentinel file names.
const SENTINEL_PREFIX: &str = "pathflow-";

/// Resolve the sentinel directory for a session.
///
/// Returns `{base_dir}/.state/sentinels/pathflow/{session_id}/`.
///
/// # Errors
///
/// Returns `PathflowError::Sentinel` if `session_id` is empty.
pub fn resolve_dir(base_dir: &Path, session_id: &str) -> Result<PathBuf, PathflowError> {
    if session_id.is_empty() {
        return Err(PathflowError::Sentinel(
            "session ID required for pathflow scope".to_string(),
        ));
    }
    Ok(base_dir
        .join(".state")
        .join("sentinels")
        .join("pathflow")
        .join(session_id))
}

/// Create a sentinel file using the typed `Sentinel` enum.
///
/// The sentinel is written directly using the enum's `file_name()`
/// (which already includes the `pathflow-` prefix). Idempotent:
/// creating an existing sentinel overwrites it silently.
///
/// # Errors
///
/// Returns `PathflowError::Io` on filesystem errors.
pub fn create_sentinel(sentinel_dir: &Path, sentinel: Sentinel) -> Result<(), PathflowError> {
    fs::create_dir_all(sentinel_dir)?;
    let path = sentinel_dir.join(sentinel.file_name());
    fs::write(&path, b"")?;
    Ok(())
}

/// Create a sentinel file by raw name string.
///
/// Used by the checkpoint system which constructs names like `pf-1` directly.
///
/// # Errors
///
/// Returns `PathflowError::Sentinel` if name is empty.
/// Returns `PathflowError::Io` on filesystem errors.
pub fn create_by_name(sentinel_dir: &Path, name: &str) -> Result<(), PathflowError> {
    if name.is_empty() {
        return Err(PathflowError::Sentinel("empty sentinel name".to_string()));
    }
    fs::create_dir_all(sentinel_dir)?;
    let path = sentinel_dir.join(format!("{SENTINEL_PREFIX}{name}"));
    fs::write(&path, b"")?;
    Ok(())
}

/// Check whether a sentinel exists using the typed `Sentinel` enum.
#[must_use]
pub fn check_sentinel(sentinel_dir: &Path, sentinel: Sentinel) -> bool {
    sentinel_dir.join(sentinel.file_name()).exists()
}

/// Check whether a sentinel exists by raw name string.
#[must_use]
pub fn check_by_name(sentinel_dir: &Path, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    sentinel_dir
        .join(format!("{SENTINEL_PREFIX}{name}"))
        .exists()
}

/// List all sentinel names in the directory, with the `pathflow-` prefix stripped.
///
/// Returns an empty vec if the directory does not exist.
///
/// # Errors
///
/// Returns `PathflowError::Io` on filesystem read errors (other than not-found).
pub fn list_sentinels(sentinel_dir: &Path) -> Result<Vec<String>, PathflowError> {
    if !sentinel_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names: Vec<String> = fs::read_dir(sentinel_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_prefix(SENTINEL_PREFIX).map(ToString::to_string)
        })
        .collect();

    names.sort();
    Ok(names)
}

/// Delete a sentinel file using the typed `Sentinel` enum.
///
/// Idempotent: deleting a non-existent sentinel is not an error.
///
/// # Errors
///
/// Returns `PathflowError::Io` on filesystem errors (other than not-found).
pub fn delete_sentinel(sentinel_dir: &Path, sentinel: Sentinel) -> Result<(), PathflowError> {
    let path = sentinel_dir.join(sentinel.file_name());
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(PathflowError::Io(e)),
    }
}

/// Delete a sentinel file by raw name string.
///
/// # Errors
///
/// Returns `PathflowError::Sentinel` if name is empty.
pub fn delete_by_name(sentinel_dir: &Path, name: &str) -> Result<(), PathflowError> {
    if name.is_empty() {
        return Err(PathflowError::Sentinel("empty sentinel name".to_string()));
    }
    let path = sentinel_dir.join(format!("{SENTINEL_PREFIX}{name}"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(PathflowError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_dir() {
        let base = Path::new("/project");
        let dir = resolve_dir(base, "ses-123").unwrap();
        assert_eq!(
            dir,
            PathBuf::from("/project/.state/sentinels/pathflow/ses-123")
        );
    }

    #[test]
    fn test_resolve_dir_empty_session() {
        let base = Path::new("/project");
        let result = resolve_dir(base, "");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("session ID"));
    }

    #[test]
    fn test_create_and_check_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        create_sentinel(sentinel_dir, Sentinel::PathflowPf1).unwrap();
        assert!(check_sentinel(sentinel_dir, Sentinel::PathflowPf1));
        assert!(!check_sentinel(sentinel_dir, Sentinel::PathflowPf2));
    }

    #[test]
    fn test_create_sentinel_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        create_sentinel(sentinel_dir, Sentinel::PathflowPf3).unwrap();
        create_sentinel(sentinel_dir, Sentinel::PathflowPf3).unwrap();
        assert!(check_sentinel(sentinel_dir, Sentinel::PathflowPf3));
    }

    #[test]
    fn test_list_sentinels_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path().join("nonexistent");
        let names = list_sentinels(&sentinel_dir).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_list_sentinels_with_files() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        create_sentinel(sentinel_dir, Sentinel::PathflowPf1).unwrap();
        create_sentinel(sentinel_dir, Sentinel::PathflowWsDev).unwrap();

        let names = list_sentinels(sentinel_dir).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"pf-1".to_string()));
        assert!(names.contains(&"ws-dev".to_string()));
    }

    #[test]
    fn test_delete_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        create_sentinel(sentinel_dir, Sentinel::PathflowPf1).unwrap();
        assert!(check_sentinel(sentinel_dir, Sentinel::PathflowPf1));

        delete_sentinel(sentinel_dir, Sentinel::PathflowPf1).unwrap();
        assert!(!check_sentinel(sentinel_dir, Sentinel::PathflowPf1));
    }

    #[test]
    fn test_delete_sentinel_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        // Deleting a non-existent sentinel should succeed
        delete_sentinel(sentinel_dir, Sentinel::PathflowPf7).unwrap();
    }

    #[test]
    fn test_create_by_name_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = create_by_name(dir.path(), "");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_by_name_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!check_by_name(dir.path(), ""));
    }

    #[test]
    fn test_list_sentinels_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        create_sentinel(sentinel_dir, Sentinel::PathflowPf3).unwrap();
        create_sentinel(sentinel_dir, Sentinel::PathflowPf1).unwrap();
        create_sentinel(sentinel_dir, Sentinel::PathflowPf2).unwrap();

        let names = list_sentinels(sentinel_dir).unwrap();
        assert_eq!(names, vec!["pf-1", "pf-2", "pf-3"]);
    }

    #[test]
    fn test_sentinel_enum_exhaustive_create() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path();

        // Create all sentinel variants via exhaustive match
        let all_sentinels = [
            Sentinel::PathflowPf1,
            Sentinel::PathflowPf2,
            Sentinel::PathflowPf3,
            Sentinel::PathflowPf4,
            Sentinel::PathflowPf5,
            Sentinel::PathflowPf6,
            Sentinel::PathflowPf7,
            Sentinel::PathflowWsDev,
            Sentinel::PathflowWsRev,
            Sentinel::PathflowWsQa,
            Sentinel::PathflowWsTest,
            Sentinel::PathflowWsPlan,
            Sentinel::PathflowWsDocs,
        ];

        for s in &all_sentinels {
            create_sentinel(sentinel_dir, *s).unwrap();
        }

        let names = list_sentinels(sentinel_dir).unwrap();
        assert_eq!(names.len(), 13);
    }
}
