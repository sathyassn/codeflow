//! Worktree YAML registry management.
//!
//! Reads and writes the `worktrees.yaml` file that tracks all managed
//! worktrees. Uses `serde_yaml` for structured YAML parsing instead of
//! the Go implementation's line-by-line text manipulation.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::WorktreeError;
use crate::file_lock;

/// A single worktree entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Human-readable worktree name.
    pub name: String,
    /// Absolute filesystem path to the worktree directory.
    pub path: String,
    /// Git branch name associated with this worktree.
    /// Empty string for detached worktrees (branch set later at PF3).
    pub branch: String,
    /// ISO 8601 timestamp of when the worktree was created.
    pub created_at: String,
    /// Current status: "active" or "removed".
    pub status: String,
    /// Session ID that owns this worktree (set by SessionStart integration, task 009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Task ID being worked on in this worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
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
#[cfg(test)]
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

/// Count the number of active worktrees in the registry.
#[must_use]
pub fn count_active(registry: &WorktreeRegistry) -> usize {
    registry
        .worktrees
        .iter()
        .filter(|e| e.status == "active")
        .count()
}

/// List all active worktree entries in the registry.
#[must_use]
pub fn list_active(registry: &WorktreeRegistry) -> Vec<&WorktreeEntry> {
    registry
        .worktrees
        .iter()
        .filter(|e| e.status == "active")
        .collect()
}

/// Perform a locked read of the registry file.
///
/// Uses a sidecar `.lock` file for shared access. Returns the parsed registry
/// or a default empty registry if the file does not exist.
///
/// # Errors
///
/// - `WorktreeError::Yaml` if YAML parsing fails.
/// - `WorktreeError::Io` on filesystem errors.
pub fn locked_read_registry(path: &Path) -> Result<WorktreeRegistry, WorktreeError> {
    if !path.exists() {
        return Ok(WorktreeRegistry::new(""));
    }
    // Use locked_binary_rmw in read-only mode (no modifications).
    let mut result_registry = None;
    file_lock::locked_binary_rmw(
        path,
        || WorktreeRegistry::new(""),
        |bytes| {
            let content = std::str::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))?;
            serde_yaml::from_str(content).map_err(|e| format!("yaml: {e}"))
        },
        |reg| {
            serde_yaml::to_string(reg)
                .map(String::into_bytes)
                .map_err(|e| format!("yaml: {e}"))
        },
        |reg| {
            result_registry = Some(reg.clone());
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked read: {e}")))?;
    Ok(result_registry.unwrap_or_else(|| WorktreeRegistry::new("")))
}

/// Register a new worktree with an atomic count-check + register under a single lock.
///
/// Prevents TOCTOU races where two sessions both see count < max and both register.
///
/// # Errors
///
/// - `WorktreeError::Creation` if the number of active worktrees is already at the limit.
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn locked_register_with_limit(
    registry_path: &Path,
    entry: &WorktreeEntry,
    max_concurrent: usize,
) -> Result<(), WorktreeError> {
    let entry_owned = entry.clone();
    file_lock::locked_binary_rmw(
        registry_path,
        || WorktreeRegistry::new(""),
        |bytes| {
            if bytes.is_empty() {
                return Ok(WorktreeRegistry::new(""));
            }
            let content = std::str::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))?;
            serde_yaml::from_str(content).map_err(|e| format!("yaml: {e}"))
        },
        |reg| {
            let content = serde_yaml::to_string(reg).map_err(|e| format!("yaml: {e}"))?;
            let output =
                format!("# Worktree Tracking\n# Managed by: codeflow worktree\n\n{content}");
            Ok(output.into_bytes())
        },
        |reg| {
            let active = count_active(reg);
            if active >= max_concurrent {
                return Err(format!(
                    "max concurrent worktrees reached: {active}/{max_concurrent}"
                ));
            }
            reg.worktrees.push(entry_owned.clone());
            reg.metadata.last_updated = super::now_rfc3339();
            Ok(())
        },
    )
    .map_err(|e| {
        if e.contains("max concurrent worktrees reached") {
            WorktreeError::Creation(e)
        } else {
            WorktreeError::Yaml(format!("locked register: {e}"))
        }
    })
}

/// Deregister a worktree under an exclusive lock.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn locked_deregister_worktree(
    registry_path: &Path,
    worktree_path: &str,
) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let wt_path = worktree_path.to_string();
    file_lock::locked_binary_rmw(
        registry_path,
        || WorktreeRegistry::new(""),
        |bytes| {
            let content = std::str::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))?;
            serde_yaml::from_str(content).map_err(|e| format!("yaml: {e}"))
        },
        |reg| {
            let content = serde_yaml::to_string(reg).map_err(|e| format!("yaml: {e}"))?;
            let output =
                format!("# Worktree Tracking\n# Managed by: codeflow worktree\n\n{content}");
            Ok(output.into_bytes())
        },
        |reg| {
            let mut found = false;
            for entry in &mut reg.worktrees {
                if entry.path == wt_path {
                    entry.status = "removed".to_string();
                    found = true;
                    break;
                }
            }
            if found {
                reg.metadata.last_updated = super::now_rfc3339();
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked deregister: {e}")))
}

/// Trigger daemon auto-start if active worktree count transitions above 1.
///
/// Should be called AFTER a successful worktree registration (outside the
/// registry lock). Reads the registry count, and if count > 1, starts the
/// sync daemon. The interval is read from `parallel-work-config.json`.
///
/// # Errors
///
/// Returns `WorktreeError` if the registry cannot be read.
pub fn maybe_auto_start_daemon(
    registry_path: &Path,
    project_dir: &std::path::Path,
) -> Result<(), WorktreeError> {
    let reg = locked_read_registry(registry_path)?;
    let active = count_active(&reg);

    if active > 1 {
        let config = crate::coordination::sync::SyncConfig::from_config_file(project_dir);
        match crate::coordination::sync::start_daemon(&config) {
            Ok(pid) => {
                eprintln!("sync daemon auto-started (pid={pid}, active_worktrees={active})");
            }
            Err(e) => {
                eprintln!("sync daemon auto-start failed: {e}");
            }
        }
    }

    Ok(())
}

/// Trigger daemon auto-stop if active worktree count drops to 1 or below.
///
/// Should be called AFTER a successful worktree deregistration (outside the
/// registry lock). Reads the registry count, and if count <= 1, stops the
/// sync daemon. The config is read from `parallel-work-config.json`.
///
/// # Errors
///
/// Returns `WorktreeError` if the registry cannot be read.
pub fn maybe_auto_stop_daemon(
    registry_path: &Path,
    project_dir: &std::path::Path,
) -> Result<(), WorktreeError> {
    let reg = locked_read_registry(registry_path)?;
    let active = count_active(&reg);

    if active <= 1 {
        let config = crate::coordination::sync::SyncConfig::from_config_file(project_dir);
        match crate::coordination::sync::stop_daemon(&config) {
            Ok(()) => {
                eprintln!("sync daemon auto-stopped (active_worktrees={active})");
            }
            Err(e) => {
                // Not an error if daemon wasn't running.
                eprintln!("sync daemon auto-stop: {e}");
            }
        }
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
            session_id: None,
            task_id: None,
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
            session_id: None,
            task_id: None,
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
            session_id: None,
            task_id: None,
        };
        let entry2 = WorktreeEntry {
            name: "wt-2".to_string(),
            path: "/tmp/wt/2".to_string(),
            branch: "feat/two".to_string(),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
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
            session_id: None,
            task_id: None,
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
            session_id: None,
            task_id: None,
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
            session_id: None,
            task_id: None,
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        let parsed: WorktreeEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_worktree_entry_backward_compat_no_session_id() {
        // YAML files created before session_id was added should still parse.
        let yaml = r#"
name: old-wt
path: /tmp/old-wt
branch: feat/old
created_at: "2026-03-07T10:30:00Z"
status: active
"#;
        let parsed: WorktreeEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.name, "old-wt");
        assert!(
            parsed.session_id.is_none(),
            "session_id should default to None"
        );
    }

    #[test]
    fn test_worktree_entry_session_id_present() {
        let entry = WorktreeEntry {
            name: "sid-wt".to_string(),
            path: "/tmp/sid-wt".to_string(),
            branch: String::new(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some("ses-abc123".to_string()),
            task_id: None,
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(
            yaml.contains("session_id"),
            "session_id should be serialized when Some"
        );

        let parsed: WorktreeEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.session_id, Some("ses-abc123".to_string()));
    }

    #[test]
    fn test_worktree_entry_session_id_none_not_serialized() {
        let entry = WorktreeEntry {
            name: "nosid-wt".to_string(),
            path: "/tmp/nosid-wt".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(
            !yaml.contains("session_id"),
            "session_id: None should be skipped in serialization"
        );
    }

    // -- task_id field tests --

    #[test]
    fn test_worktree_entry_task_id_serde_roundtrip() {
        let entry = WorktreeEntry {
            name: "tid-wt".to_string(),
            path: "/tmp/tid-wt".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some("ses-abc".to_string()),
            task_id: Some("INF-TSK-023-018".to_string()),
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(
            yaml.contains("task_id"),
            "task_id should be serialized when Some"
        );
        let parsed: WorktreeEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.task_id, Some("INF-TSK-023-018".to_string()));
    }

    #[test]
    fn test_worktree_entry_backward_compat_no_task_id() {
        // YAML files created before task_id was added should still parse.
        let yaml = r#"
name: old-wt
path: /tmp/old-wt
branch: feat/old
created_at: "2026-03-07T10:30:00Z"
status: active
session_id: ses-123
"#;
        let parsed: WorktreeEntry = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(parsed.name, "old-wt");
        assert!(parsed.task_id.is_none(), "task_id should default to None");
        assert_eq!(parsed.session_id, Some("ses-123".to_string()));
    }

    #[test]
    fn test_worktree_entry_task_id_none_not_serialized() {
        let entry = WorktreeEntry {
            name: "notid-wt".to_string(),
            path: "/tmp/notid-wt".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(
            !yaml.contains("task_id"),
            "task_id: None should be skipped in serialization"
        );
    }

    // -- count_active / list_active tests --

    #[test]
    fn test_count_active_mixed_statuses() {
        let mut reg = WorktreeRegistry::new("2026-03-07T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "active-1".to_string(),
            path: "/tmp/a1".to_string(),
            branch: "feat/a1".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "removed-1".to_string(),
            path: "/tmp/r1".to_string(),
            branch: "feat/r1".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "removed".to_string(),
            session_id: None,
            task_id: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "active-2".to_string(),
            path: "/tmp/a2".to_string(),
            branch: "feat/a2".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });

        assert_eq!(count_active(&reg), 2);
    }

    #[test]
    fn test_count_active_empty_registry() {
        let reg = WorktreeRegistry::new("2026-03-07T10:00:00Z");
        assert_eq!(count_active(&reg), 0);
    }

    #[test]
    fn test_list_active_filters_removed() {
        let mut reg = WorktreeRegistry::new("2026-03-07T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "active-1".to_string(),
            path: "/tmp/a1".to_string(),
            branch: "feat/a1".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "removed-1".to_string(),
            path: "/tmp/r1".to_string(),
            branch: "feat/r1".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "removed".to_string(),
            session_id: None,
            task_id: None,
        });

        let active = list_active(&reg);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "active-1");
    }

    // -- auto-start/stop daemon tests --

    #[test]
    fn test_maybe_auto_start_daemon_below_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join("worktrees.yaml");

        // Register 1 active worktree — should NOT trigger daemon start.
        let entry = WorktreeEntry {
            name: "wt-1".to_string(),
            path: "/tmp/wt/1".to_string(),
            branch: "feat/one".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };
        locked_register_with_limit(&reg_path, &entry, 3).unwrap();

        // Should succeed without starting daemon (count == 1, threshold > 1).
        let result = maybe_auto_start_daemon(&reg_path, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_maybe_auto_stop_daemon_above_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join("worktrees.yaml");

        // Register 2 active worktrees — should NOT trigger daemon stop.
        for i in 0..2 {
            let entry = WorktreeEntry {
                name: format!("wt-{i}"),
                path: format!("/tmp/wt/{i}"),
                branch: format!("feat/{i}"),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: "active".to_string(),
                session_id: None,
                task_id: None,
            };
            locked_register_with_limit(&reg_path, &entry, 3).unwrap();
        }

        // Should succeed without stopping daemon (count == 2, threshold <= 1).
        let result = maybe_auto_stop_daemon(&reg_path, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_maybe_auto_stop_daemon_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join("worktrees.yaml");

        // Register 1 active worktree — should trigger daemon stop attempt.
        let entry = WorktreeEntry {
            name: "wt-only".to_string(),
            path: "/tmp/wt/only".to_string(),
            branch: "feat/only".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };
        locked_register_with_limit(&reg_path, &entry, 3).unwrap();

        // Should succeed — stop attempt is a no-op when no daemon is running.
        let result = maybe_auto_stop_daemon(&reg_path, dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_maybe_auto_start_daemon_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join("nonexistent.yaml");

        // With no registry, count is 0 — should not start daemon.
        let result = maybe_auto_start_daemon(&reg_path, dir.path());
        assert!(result.is_ok());
    }

    // -- locked_register_with_limit tests --

    #[test]
    fn test_locked_register_with_limit_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "wt-1".to_string(),
            path: "/tmp/wt/1".to_string(),
            branch: "feat/one".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: Some("TSK-001".to_string()),
        };

        locked_register_with_limit(&path, &entry, 3).unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees.len(), 1);
        assert_eq!(reg.worktrees[0].task_id, Some("TSK-001".to_string()));
    }

    #[test]
    fn test_locked_register_with_limit_at_limit_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        // Register 3 active worktrees (the limit).
        for i in 0..3 {
            let entry = WorktreeEntry {
                name: format!("wt-{i}"),
                path: format!("/tmp/wt/{i}"),
                branch: format!("feat/{i}"),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: "active".to_string(),
                session_id: None,
                task_id: None,
            };
            locked_register_with_limit(&path, &entry, 3).unwrap();
        }

        // 4th should be rejected.
        let entry = WorktreeEntry {
            name: "wt-3".to_string(),
            path: "/tmp/wt/3".to_string(),
            branch: "feat/3".to_string(),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };

        let result = locked_register_with_limit(&path, &entry, 3);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, WorktreeError::Creation(_)),
            "expected Creation error, got: {err}"
        );
        assert!(err.to_string().contains("max concurrent"));
    }

    #[test]
    fn test_locked_register_with_limit_removed_not_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        // Register 3, then remove 1 — should allow a new one.
        for i in 0..3 {
            let entry = WorktreeEntry {
                name: format!("wt-{i}"),
                path: format!("/tmp/wt/{i}"),
                branch: format!("feat/{i}"),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: "active".to_string(),
                session_id: None,
                task_id: None,
            };
            locked_register_with_limit(&path, &entry, 3).unwrap();
        }

        // Deregister one.
        locked_deregister_worktree(&path, "/tmp/wt/1").unwrap();

        // Should now succeed (2 active, limit 3).
        let entry = WorktreeEntry {
            name: "wt-new".to_string(),
            path: "/tmp/wt/new".to_string(),
            branch: "feat/new".to_string(),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };
        locked_register_with_limit(&path, &entry, 3).unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(count_active(&reg), 3);
    }
}
