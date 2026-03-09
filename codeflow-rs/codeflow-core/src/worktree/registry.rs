//! Worktree YAML registry management.
//!
//! Reads and writes the `worktrees.yaml` file that tracks all managed
//! worktrees. Uses `serde_yaml` for structured YAML parsing instead of
//! the Go implementation's line-by-line text manipulation.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::WorktreeError;

/// A single worktree entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Human-readable worktree name.
    pub name: String,
    /// Absolute filesystem path to the worktree directory.
    pub path: String,
    /// Git branch name associated with this worktree.
    pub branch: String,
    /// ISO 8601 timestamp of when the worktree was created.
    pub created_at: String,
    /// Current status: "active" or "removed".
    pub status: String,
}

/// Registry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMetadata {
    /// Schema version of the registry file.
    pub version: String,
    /// ISO 8601 timestamp of the last registry update.
    pub last_updated: String,
}

/// Top-level registry file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRegistry {
    /// List of tracked worktrees.
    pub worktrees: Vec<WorktreeEntry>,
    /// Registry metadata.
    pub metadata: RegistryMetadata,
}

impl WorktreeRegistry {
    /// Create a new empty registry with default metadata.
    #[must_use]
    pub fn new(timestamp: &str) -> Self {
        Self {
            worktrees: Vec::new(),
            metadata: RegistryMetadata {
                version: "1.0.0".to_string(),
                last_updated: timestamp.to_string(),
            },
        }
    }
}

/// Read the registry from a YAML file.
///
/// # Errors
///
/// - `WorktreeError::Io` if the file cannot be read.
/// - `WorktreeError::Yaml` if the YAML is malformed.
pub fn read_registry(path: &Path) -> Result<WorktreeRegistry, WorktreeError> {
    let content = fs::read_to_string(path)?;
    serde_yaml::from_str(&content).map_err(|e| WorktreeError::Yaml(e.to_string()))
}

/// Write the registry to a YAML file.
///
/// Creates parent directories if they don't exist.
///
/// # Errors
///
/// - `WorktreeError::Io` if the file or directories cannot be written.
/// - `WorktreeError::Yaml` if serialization fails.
pub fn write_registry(path: &Path, registry: &WorktreeRegistry) -> Result<(), WorktreeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content =
        serde_yaml::to_string(registry).map_err(|e| WorktreeError::Yaml(e.to_string()))?;

    // Prepend the standard header comment.
    let output = format!("# Worktree Tracking\n# Managed by: codeflow worktree\n\n{content}");
    fs::write(path, output)?;
    Ok(())
}

/// Register a new worktree entry in the registry file.
///
/// If the registry file doesn't exist, creates it with default metadata.
/// Updates `last_updated` in metadata.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn register_worktree(registry_path: &Path, entry: WorktreeEntry) -> Result<(), WorktreeError> {
    let mut registry = if registry_path.exists() {
        read_registry(registry_path)?
    } else {
        WorktreeRegistry::new(&entry.created_at)
    };

    registry.worktrees.push(entry);
    registry.metadata.last_updated = super::now_rfc3339();
    write_registry(registry_path, &registry)
}

/// Deregister a worktree by setting its status to "removed".
///
/// Matches by path. If the registry doesn't exist or the path is not found,
/// this is a no-op (returns `Ok(())`).
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn deregister_worktree(registry_path: &Path, worktree_path: &str) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let mut registry = read_registry(registry_path)?;

    let mut found = false;
    for entry in &mut registry.worktrees {
        if entry.path == worktree_path {
            entry.status = "removed".to_string();
            found = true;
            break;
        }
    }

    if found {
        registry.metadata.last_updated = super::now_rfc3339();
        write_registry(registry_path, &registry)?;
    }

    Ok(())
}

/// Convert days since Unix epoch to (year, month, day).
pub(crate) fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let reg = WorktreeRegistry::new("2026-03-07T10:30:00Z");
        assert!(reg.worktrees.is_empty());
        assert_eq!(reg.metadata.version, "1.0.0");
        assert_eq!(reg.metadata.last_updated, "2026-03-07T10:30:00Z");
    }

    #[test]
    fn test_write_and_read_registry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-03-07T10:30:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "my-wt".to_string(),
            path: "/tmp/wt/my-wt".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
        });

        write_registry(&path, &reg).unwrap();
        assert!(path.exists());

        let loaded = read_registry(&path).unwrap();
        assert_eq!(loaded.worktrees.len(), 1);
        assert_eq!(loaded.worktrees[0].name, "my-wt");
        assert_eq!(loaded.worktrees[0].status, "active");
        assert_eq!(loaded.metadata.version, "1.0.0");
    }

    #[test]
    fn test_register_worktree_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "new-wt".to_string(),
            path: "/tmp/wt/new-wt".to_string(),
            branch: "feat/new".to_string(),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: "active".to_string(),
        };

        register_worktree(&path, entry).unwrap();
        assert!(path.exists());

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees.len(), 1);
        assert_eq!(reg.worktrees[0].name, "new-wt");
    }

    #[test]
    fn test_register_worktree_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry1 = WorktreeEntry {
            name: "wt-1".to_string(),
            path: "/tmp/wt/1".to_string(),
            branch: "feat/one".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
        };
        let entry2 = WorktreeEntry {
            name: "wt-2".to_string(),
            path: "/tmp/wt/2".to_string(),
            branch: "feat/two".to_string(),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: "active".to_string(),
        };

        register_worktree(&path, entry1).unwrap();
        register_worktree(&path, entry2).unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees.len(), 2);
        assert_eq!(reg.worktrees[0].name, "wt-1");
        assert_eq!(reg.worktrees[1].name, "wt-2");
    }

    #[test]
    fn test_deregister_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "to-remove".to_string(),
            path: "/tmp/wt/to-remove".to_string(),
            branch: "feat/remove".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
        };
        register_worktree(&path, entry).unwrap();

        deregister_worktree(&path, "/tmp/wt/to-remove").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, "removed");
    }

    #[test]
    fn test_deregister_nonexistent_path_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "wt".to_string(),
            path: "/tmp/wt/exists".to_string(),
            branch: "feat/exists".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
        };
        register_worktree(&path, entry).unwrap();

        // Deregister a path that doesn't match any entry.
        deregister_worktree(&path, "/tmp/wt/does-not-exist").unwrap();

        let reg = read_registry(&path).unwrap();
        // Original entry should be unchanged.
        assert_eq!(reg.worktrees[0].status, "active");
    }

    #[test]
    fn test_deregister_no_registry_file_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-such-file.yaml");
        // Should not error when file doesn't exist.
        deregister_worktree(&path, "/tmp/wt/anything").unwrap();
    }

    #[test]
    fn test_read_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.yaml");
        fs::write(&path, "this is not valid yaml: [[[").unwrap();

        let result = read_registry(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, WorktreeError::Yaml(_)),
            "expected Yaml error, got: {err}"
        );
    }

    #[test]
    fn test_registry_header_comment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let reg = WorktreeRegistry::new("2026-03-07T10:30:00Z");
        write_registry(&path, &reg).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("# Worktree Tracking"));
        assert!(content.contains("# Managed by: codeflow worktree"));
    }

    #[test]
    fn test_worktree_entry_serde_roundtrip() {
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/tmp/test-wt".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        let parsed: WorktreeEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(entry, parsed);
    }
}
