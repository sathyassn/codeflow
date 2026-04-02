//! Worktree YAML registry management.
//!
//! Reads and writes the `worktrees.yaml` file that tracks all managed
//! worktrees. Uses `serde_yaml` for structured YAML parsing instead of
//! the Go implementation's line-by-line text manipulation.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diagnostics;
use crate::error::WorktreeError;
use crate::file_lock;

/// Typed worktree lifecycle status.
///
/// Replaces free-form `String` status to enable exhaustive match checking.
/// Serde uses `snake_case` for backwards compatibility with existing YAML files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeStatus {
    Active,
    PendingCleanup,
    Removed,
}

impl std::fmt::Display for WorktreeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::PendingCleanup => f.write_str("pending_cleanup"),
            Self::Removed => f.write_str("removed"),
        }
    }
}

/// A single worktree entry in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// Human-readable worktree name.
    pub name: String,
    /// Absolute filesystem path to the worktree directory.
    pub path: String,
    /// Git branch name associated with this worktree.
    /// `None` for detached worktrees (branch set later at PF3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// ISO 8601 timestamp of when the worktree was created.
    pub created_at: String,
    /// Current lifecycle status.
    pub status: WorktreeStatus,
    /// Session ID that owns this worktree (set by SessionStart integration, task 009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Task ID being worked on in this worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Source that created this worktree: "interactive" or "autorun".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// PID of the Claude Code lead process owning this worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_pid: Option<u32>,
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
#[cfg(test)]
pub fn deregister_worktree(registry_path: &Path, worktree_path: &str) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let mut registry = read_registry(registry_path)?;

    let mut found = false;
    for entry in &mut registry.worktrees {
        if entry.path == worktree_path {
            entry.status = WorktreeStatus::Removed;
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

/// Deregister a worktree by name (defense-in-depth fallback).
///
/// Matches by name instead of path. Matches both `Active` and `PendingCleanup`
/// entries. If the registry doesn't exist or the name is not found, this is a no-op.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn deregister_by_name(registry_path: &Path, name: &str) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let mut registry = read_registry(registry_path)?;

    let mut found = false;
    for entry in &mut registry.worktrees {
        if entry.name == name
            && (entry.status == WorktreeStatus::Active
                || entry.status == WorktreeStatus::PendingCleanup)
        {
            entry.status = WorktreeStatus::Removed;
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

/// Purge old "removed" entries from the registry, keeping at most `max_keep`.
///
/// Removes the oldest removed entries first. Active entries are never touched.
/// If the registry doesn't exist, this is a no-op.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn purge_removed_entries(
    registry_path: &Path,
    max_keep: usize,
) -> Result<usize, WorktreeError> {
    if !registry_path.exists() {
        return Ok(0);
    }

    let mut registry = read_registry(registry_path)?;

    let removed_count = registry
        .worktrees
        .iter()
        .filter(|e| e.status == WorktreeStatus::Removed)
        .count();

    if removed_count <= max_keep {
        return Ok(0);
    }

    let to_remove = removed_count - max_keep;
    let mut removed = 0;
    registry.worktrees.retain(|e| {
        if e.status == WorktreeStatus::Removed && removed < to_remove {
            removed += 1;
            false
        } else {
            true
        }
    });

    if removed > 0 {
        registry.metadata.last_updated = super::now_rfc3339();
        write_registry(registry_path, &registry)?;
    }

    Ok(removed)
}

/// Mark a worktree as "pending_cleanup" by session ID.
///
/// Status lifecycle: `active` -> `pending_cleanup` -> `removed`.
///
/// The `pending_cleanup` status indicates that the PathFlow lifecycle is done
/// (TeamDelete has fired) but the session may still be alive for a short
/// period. Stale sweep logic treats `pending_cleanup` entries with a stale
/// heartbeat as cleanable.
///
/// Matches by `session_id`. If the registry doesn't exist or the session
/// is not found, this is a no-op.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn mark_pending_cleanup(registry_path: &Path, session_id: &str) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let sid = session_id.to_string();
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
            let mut found = false;
            for entry in &mut reg.worktrees {
                if entry.session_id.as_deref() == Some(&sid)
                    && entry.status == WorktreeStatus::Active
                {
                    entry.status = WorktreeStatus::PendingCleanup;
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
    .map_err(|e| WorktreeError::Yaml(format!("locked mark_pending_cleanup: {e}")))
}

/// Update the `source` field on a worktree entry identified by name.
///
/// Uses file-locked read-modify-write to avoid concurrent corruption.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired or the file cannot be written.
pub fn locked_update_source(
    registry_path: &Path,
    worktree_name: &str,
    source: &str,
) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let name = worktree_name.to_string();
    let src = source.to_string();
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
            if let Some(entry) = reg.worktrees.iter_mut().find(|e| e.name == name) {
                entry.source = Some(src.clone());
                reg.metadata.last_updated = super::now_rfc3339();
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked update_source: {e}")))
}

/// Count the number of active worktrees in the registry.
#[must_use]
pub fn count_active(registry: &WorktreeRegistry) -> usize {
    registry
        .worktrees
        .iter()
        .filter(|e| e.status == WorktreeStatus::Active)
        .count()
}

/// List all active worktree entries in the registry.
#[must_use]
pub fn list_active(registry: &WorktreeRegistry) -> Vec<&WorktreeEntry> {
    registry
        .worktrees
        .iter()
        .filter(|e| e.status == WorktreeStatus::Active)
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
            // Check for existing entry with same name (dedup).
            if let Some(existing) = reg
                .worktrees
                .iter_mut()
                .find(|e| e.name == entry_owned.name && e.status == WorktreeStatus::Active)
            {
                diagnostics::warn(
                    "worktree",
                    &format!(
                        "duplicate registration for '{}', updating in place",
                        entry_owned.name
                    ),
                );
                // Only overwrite path/branch if the new value is non-empty.
                // This prevents SessionStart pre-registration (with empty path)
                // from clobbering a valid path set by create_detached_worktree().
                if !entry_owned.path.is_empty() {
                    existing.path.clone_from(&entry_owned.path);
                }
                if entry_owned.branch.is_some() {
                    existing.branch.clone_from(&entry_owned.branch);
                }
                existing.created_at.clone_from(&entry_owned.created_at);
                if entry_owned.session_id.is_some() {
                    existing.session_id.clone_from(&entry_owned.session_id);
                }
                if entry_owned.task_id.is_some() {
                    existing.task_id.clone_from(&entry_owned.task_id);
                }
                reg.metadata.last_updated = super::now_rfc3339();
                return Ok(());
            }

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
                    entry.status = WorktreeStatus::Removed;
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

/// Deregister a worktree by name under an exclusive lock.
///
/// Matches both `Active` and `PendingCleanup` entries.
///
/// # Errors
///
/// - `WorktreeError::Io` on filesystem errors.
/// - `WorktreeError::Yaml` on parse/serialize errors.
pub fn locked_deregister_by_name(registry_path: &Path, name: &str) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let name_owned = name.to_string();
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
                if entry.name == name_owned
                    && (entry.status == WorktreeStatus::Active
                        || entry.status == WorktreeStatus::PendingCleanup)
                {
                    entry.status = WorktreeStatus::Removed;
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
    .map_err(|e| WorktreeError::Yaml(format!("locked deregister_by_name: {e}")))
}

/// Update the `branch` field on a worktree entry identified by name.
///
/// Uses file-locked read-modify-write to avoid concurrent corruption.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired or the file cannot be written.
pub fn locked_update_branch(
    registry_path: &Path,
    worktree_name: &str,
    branch: &str,
) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let name = worktree_name.to_string();
    let br = branch.to_string();
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
            if let Some(entry) = reg.worktrees.iter_mut().find(|e| e.name == name) {
                entry.branch = Some(br.clone());
                reg.metadata.last_updated = super::now_rfc3339();
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked update_branch: {e}")))
}

/// Update the `lead_pid` field on a worktree entry identified by name.
///
/// Uses file-locked read-modify-write to avoid concurrent corruption.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired or the file cannot be written.
pub fn locked_update_lead_pid(
    registry_path: &Path,
    worktree_name: &str,
    pid: u32,
) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let name = worktree_name.to_string();
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
            if let Some(entry) = reg.worktrees.iter_mut().find(|e| e.name == name) {
                entry.lead_pid = Some(pid);
                reg.metadata.last_updated = super::now_rfc3339();
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked update_lead_pid: {e}")))
}

/// Update the `session_id` field on a worktree entry identified by name.
///
/// Uses file-locked read-modify-write to avoid concurrent corruption.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired or the file cannot be written.
pub fn locked_update_session_id(
    registry_path: &Path,
    worktree_name: &str,
    session_id: &str,
) -> Result<(), WorktreeError> {
    if !registry_path.exists() {
        return Ok(());
    }

    let name = worktree_name.to_string();
    let sid = session_id.to_string();
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
            if let Some(entry) = reg.worktrees.iter_mut().find(|e| e.name == name) {
                entry.session_id = Some(sid.clone());
                reg.metadata.last_updated = super::now_rfc3339();
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked update_session_id: {e}")))
}

/// Purge old `Removed` entries from the registry under an exclusive lock.
///
/// Keeps at most `max_keep` removed entries (the most recent ones).
/// Returns the number of entries purged.
///
/// # Errors
///
/// - `WorktreeError::Yaml` on parse/serialize or lock errors.
pub fn locked_purge_removed(registry_path: &Path, max_keep: usize) -> Result<usize, WorktreeError> {
    if !registry_path.exists() {
        return Ok(0);
    }

    let mut purge_count = 0usize;
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
            let removed_count = reg
                .worktrees
                .iter()
                .filter(|e| e.status == WorktreeStatus::Removed)
                .count();

            if removed_count <= max_keep {
                return Ok(());
            }

            let to_remove = removed_count - max_keep;
            let mut removed = 0usize;
            reg.worktrees.retain(|e| {
                if e.status == WorktreeStatus::Removed && removed < to_remove {
                    removed += 1;
                    false
                } else {
                    true
                }
            });

            if removed > 0 {
                reg.metadata.last_updated = super::now_rfc3339();
                purge_count = removed;
            }
            Ok(())
        },
    )
    .map_err(|e| WorktreeError::Yaml(format!("locked purge_removed: {e}")))?;

    Ok(purge_count)
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

/// Convert (year, month, day) to days since Unix epoch.
///
/// Inverse of `days_to_ymd`. Uses Howard Hinnant's `days_from_civil`.
pub(crate) fn ymd_to_days(year: u64, month: u64, day: u64) -> u64 {
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });

        write_registry(&path, &reg).unwrap();
        assert!(path.exists());

        let loaded = read_registry(&path).unwrap();
        assert_eq!(loaded.worktrees.len(), 1);
        assert_eq!(loaded.worktrees[0].name, "my-wt");
        assert_eq!(loaded.worktrees[0].status, WorktreeStatus::Active);
        assert_eq!(loaded.metadata.version, "1.0.0");
    }

    #[test]
    fn test_register_worktree_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "new-wt".to_string(),
            path: "/tmp/wt/new-wt".to_string(),
            branch: Some("feat/new".to_string()),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/one".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        let entry2 = WorktreeEntry {
            name: "wt-2".to_string(),
            path: "/tmp/wt/2".to_string(),
            branch: Some("feat/two".to_string()),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/remove".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        deregister_worktree(&path, "/tmp/wt/to-remove").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Removed);
    }

    #[test]
    fn test_deregister_nonexistent_path_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "wt".to_string(),
            path: "/tmp/wt/exists".to_string(),
            branch: Some("feat/exists".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        // Deregister a path that doesn't match any entry.
        deregister_worktree(&path, "/tmp/wt/does-not-exist").unwrap();

        let reg = read_registry(&path).unwrap();
        // Original entry should be unchanged.
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Active);
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
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: None,
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-abc123".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-abc".to_string()),
            task_id: Some("INF-TSK-023-018".to_string()),
            source: None,
            lead_pid: None,
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
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/a1".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "removed-1".to_string(),
            path: "/tmp/r1".to_string(),
            branch: Some("feat/r1".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Removed,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "active-2".to_string(),
            path: "/tmp/a2".to_string(),
            branch: Some("feat/a2".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/a1".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "removed-1".to_string(),
            path: "/tmp/r1".to_string(),
            branch: Some("feat/r1".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Removed,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/one".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
                branch: Some(format!("feat/{i}")),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: WorktreeStatus::Active,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
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
            branch: Some("feat/only".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
            branch: Some("feat/one".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: Some("TSK-001".to_string()),
            source: None,
            lead_pid: None,
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
                branch: Some(format!("feat/{i}")),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: WorktreeStatus::Active,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            };
            locked_register_with_limit(&path, &entry, 3).unwrap();
        }

        // 4th should be rejected.
        let entry = WorktreeEntry {
            name: "wt-3".to_string(),
            path: "/tmp/wt/3".to_string(),
            branch: Some("feat/3".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
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
                branch: Some(format!("feat/{i}")),
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: WorktreeStatus::Active,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            };
            locked_register_with_limit(&path, &entry, 3).unwrap();
        }

        // Deregister one.
        locked_deregister_worktree(&path, "/tmp/wt/1").unwrap();

        // Should now succeed (2 active, limit 3).
        let entry = WorktreeEntry {
            name: "wt-new".to_string(),
            path: "/tmp/wt/new".to_string(),
            branch: Some("feat/new".to_string()),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry, 3).unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(count_active(&reg), 3);
    }

    #[test]
    fn test_locked_register_dedup_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry1 = WorktreeEntry {
            name: "dedup-wt".to_string(),
            path: "/tmp/wt/dedup-1".to_string(),
            branch: Some("feat/one".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-001".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry1, 3).unwrap();

        // Register again with same name but different path.
        let entry2 = WorktreeEntry {
            name: "dedup-wt".to_string(),
            path: "/tmp/wt/dedup-2".to_string(),
            branch: Some("feat/two".to_string()),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-002".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry2, 3).unwrap();

        // Should have only one entry (dedup'd).
        let reg = read_registry(&path).unwrap();
        let active: Vec<_> = reg
            .worktrees
            .iter()
            .filter(|e| e.status == WorktreeStatus::Active)
            .collect();
        assert_eq!(
            active.len(),
            1,
            "should have exactly one active entry after dedup"
        );
        assert_eq!(
            active[0].path, "/tmp/wt/dedup-2",
            "should have updated path"
        );
        assert_eq!(
            active[0].branch,
            Some("feat/two".to_string()),
            "should have updated branch"
        );
        assert_eq!(active[0].session_id, Some("ses-002".to_string()));
    }

    #[test]
    fn test_locked_register_dedup_preserves_path_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        // First registration with a real path and branch.
        let entry1 = WorktreeEntry {
            name: "preserve-wt".to_string(),
            path: "/tmp/wt/real-path".to_string(),
            branch: Some("feat/real-branch".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-001".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry1, 3).unwrap();

        // Second registration with empty path and branch
        // (simulates SessionStart pre-registration).
        let entry2 = WorktreeEntry {
            name: "preserve-wt".to_string(),
            path: String::new(),
            branch: None,
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-002".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry2, 3).unwrap();

        let reg = read_registry(&path).unwrap();
        let active: Vec<_> = reg
            .worktrees
            .iter()
            .filter(|e| e.name == "preserve-wt" && e.status == WorktreeStatus::Active)
            .collect();
        assert_eq!(active.len(), 1);

        // Path and branch should be preserved (not overwritten with empty).
        assert_eq!(
            active[0].path, "/tmp/wt/real-path",
            "empty path should not overwrite non-empty"
        );
        assert_eq!(
            active[0].branch,
            Some("feat/real-branch".to_string()),
            "empty branch should not overwrite non-empty"
        );
        // session_id should be updated (new value is Some).
        assert_eq!(active[0].session_id, Some("ses-002".to_string()));
    }

    #[test]
    fn test_locked_register_dedup_preserves_session_id_when_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        // First registration with a session_id and task_id.
        let entry1 = WorktreeEntry {
            name: "guard-wt".to_string(),
            path: "/tmp/wt/guard".to_string(),
            branch: Some("feat/guard".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-original".to_string()),
            task_id: Some("task-original".to_string()),
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry1, 3).unwrap();

        // Second registration with None session_id and None task_id
        // (simulates setup_detached re-registration).
        let entry2 = WorktreeEntry {
            name: "guard-wt".to_string(),
            path: "/tmp/wt/guard".to_string(),
            branch: Some("feat/guard".to_string()),
            created_at: "2026-03-07T11:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        locked_register_with_limit(&path, &entry2, 3).unwrap();

        let reg = read_registry(&path).unwrap();
        let active: Vec<_> = reg
            .worktrees
            .iter()
            .filter(|e| e.name == "guard-wt" && e.status == WorktreeStatus::Active)
            .collect();
        assert_eq!(active.len(), 1);

        // session_id and task_id should be PRESERVED (not overwritten with None).
        assert_eq!(
            active[0].session_id,
            Some("ses-original".to_string()),
            "session_id should be preserved when re-registering with None"
        );
        assert_eq!(
            active[0].task_id,
            Some("task-original".to_string()),
            "task_id should be preserved when re-registering with None"
        );
    }

    #[test]
    fn test_deregister_by_name_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "name-dereg".to_string(),
            path: "/tmp/wt/name-dereg".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        // Deregister by name.
        deregister_by_name(&path, "name-dereg").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Removed);
    }

    #[test]
    fn test_deregister_by_name_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "keep-me".to_string(),
            path: "/tmp/wt/keep".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        // Deregister a different name — should be a no-op.
        deregister_by_name(&path, "not-found").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Active);
    }

    #[test]
    fn test_deregister_by_name_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");

        // Should succeed (no-op) when registry doesn't exist.
        deregister_by_name(&path, "any-name").unwrap();
    }

    #[test]
    fn test_purge_removed_entries_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        // Create 8 removed entries and 1 active.
        let mut reg = WorktreeRegistry::new("2026-03-07T10:00:00Z");
        for i in 0..8 {
            reg.worktrees.push(WorktreeEntry {
                name: format!("removed-{i}"),
                path: format!("/tmp/wt/removed-{i}"),
                branch: None,
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: WorktreeStatus::Removed,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            });
        }
        reg.worktrees.push(WorktreeEntry {
            name: "active-wt".to_string(),
            path: "/tmp/wt/active".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&path, &reg).unwrap();

        // Purge keeping max 5.
        let purged = purge_removed_entries(&path, 5).unwrap();
        assert_eq!(purged, 3, "should purge 3 of the 8 removed entries");

        let updated = read_registry(&path).unwrap();
        let removed_count = updated
            .worktrees
            .iter()
            .filter(|e| e.status == WorktreeStatus::Removed)
            .count();
        assert_eq!(
            removed_count, 5,
            "should have exactly 5 removed entries left"
        );

        // Active entry should be untouched.
        let active_count = updated
            .worktrees
            .iter()
            .filter(|e| e.status == WorktreeStatus::Active)
            .count();
        assert_eq!(active_count, 1);
    }

    #[test]
    fn test_purge_removed_entries_nothing_to_purge() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-03-07T10:00:00Z");
        for i in 0..3 {
            reg.worktrees.push(WorktreeEntry {
                name: format!("removed-{i}"),
                path: format!("/tmp/wt/removed-{i}"),
                branch: None,
                created_at: "2026-03-07T10:00:00Z".to_string(),
                status: WorktreeStatus::Removed,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            });
        }
        write_registry(&path, &reg).unwrap();

        // 3 removed, max_keep 5 — nothing to purge.
        let purged = purge_removed_entries(&path, 5).unwrap();
        assert_eq!(purged, 0);
    }

    #[test]
    fn test_purge_removed_entries_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");

        let purged = purge_removed_entries(&path, 5).unwrap();
        assert_eq!(purged, 0);
    }

    // --- mark_pending_cleanup tests ---

    #[test]
    fn test_mark_pending_cleanup_sets_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-03-25T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-ses-test".to_string(),
            path: "/tmp/wt/test".to_string(),
            branch: Some("hotfix/test".to_string()),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-01jqpendcleantest00000".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&path, &reg).unwrap();

        mark_pending_cleanup(&path, "ses-01jqpendcleantest00000").unwrap();

        let loaded = read_registry(&path).unwrap();
        assert_eq!(
            loaded.worktrees[0].status,
            WorktreeStatus::PendingCleanup,
            "should transition from active to pending_cleanup"
        );
    }

    #[test]
    fn test_pending_cleanup_not_counted_as_active() {
        let mut reg = WorktreeRegistry::new("2026-03-25T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-active".to_string(),
            path: "/tmp/wt/active".to_string(),
            branch: Some("feat/active".to_string()),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-01jqactive0000000000000".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-pending".to_string(),
            path: "/tmp/wt/pending".to_string(),
            branch: Some("feat/pending".to_string()),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            status: WorktreeStatus::PendingCleanup,
            session_id: Some("ses-01jqpending000000000000".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });

        assert_eq!(
            count_active(&reg),
            1,
            "pending_cleanup entries should not count as active"
        );
    }

    #[test]
    fn test_mark_pending_cleanup_no_match_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-03-25T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-ses-other".to_string(),
            path: "/tmp/wt/other".to_string(),
            branch: Some("feat/other".to_string()),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some("ses-01jqdifferentsession000".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&path, &reg).unwrap();

        // Mark a session that doesn't exist -- should be no-op.
        mark_pending_cleanup(&path, "ses-01jqnonexistentsession0").unwrap();

        let loaded = read_registry(&path).unwrap();
        assert_eq!(
            loaded.worktrees[0].status,
            WorktreeStatus::Active,
            "should remain active when session_id doesn't match"
        );
    }

    #[test]
    fn test_mark_pending_cleanup_no_registry_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.yaml");

        // Should not error on missing registry.
        mark_pending_cleanup(&path, "ses-01jqmissing0000000000000").unwrap();
    }

    #[test]
    fn test_mark_pending_cleanup_skips_already_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-03-25T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "worktree-ses-removed".to_string(),
            path: "/tmp/wt/removed".to_string(),
            branch: Some("feat/removed".to_string()),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            status: WorktreeStatus::Removed,
            session_id: Some("ses-01jqremovedtest00000000".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&path, &reg).unwrap();

        // Mark pending_cleanup on a "removed" entry should be no-op.
        mark_pending_cleanup(&path, "ses-01jqremovedtest00000000").unwrap();

        let loaded = read_registry(&path).unwrap();
        assert_eq!(
            loaded.worktrees[0].status,
            WorktreeStatus::Removed,
            "should not change status from removed"
        );
    }

    // -- WorktreeStatus serde tests --

    #[test]
    fn test_worktree_status_serde() {
        // Verify snake_case serialization for backwards compat.
        let yaml = serde_yaml::to_string(&WorktreeStatus::Active).unwrap();
        assert!(yaml.trim() == "active");
        let yaml = serde_yaml::to_string(&WorktreeStatus::PendingCleanup).unwrap();
        assert!(yaml.trim() == "pending_cleanup");
        let yaml = serde_yaml::to_string(&WorktreeStatus::Removed).unwrap();
        assert!(yaml.trim() == "removed");

        // Verify deserialization from snake_case strings.
        let s: WorktreeStatus = serde_yaml::from_str("active").unwrap();
        assert_eq!(s, WorktreeStatus::Active);
        let s: WorktreeStatus = serde_yaml::from_str("pending_cleanup").unwrap();
        assert_eq!(s, WorktreeStatus::PendingCleanup);
        let s: WorktreeStatus = serde_yaml::from_str("removed").unwrap();
        assert_eq!(s, WorktreeStatus::Removed);
    }

    #[test]
    fn test_branch_option_serde() {
        // branch: None should NOT appear in YAML.
        let entry = WorktreeEntry {
            name: "opt-test".to_string(),
            path: "/tmp/opt-test".to_string(),
            branch: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(!yaml.contains("branch"), "None branch should be skipped");

        // branch: Some("x") should appear.
        let entry2 = WorktreeEntry {
            branch: Some("feat/test".to_string()),
            ..entry
        };
        let yaml2 = serde_yaml::to_string(&entry2).unwrap();
        assert!(yaml2.contains("branch"), "Some branch should appear");

        // Old YAML without branch field should parse to None.
        let old_yaml = r#"
name: old
path: /tmp/old
created_at: "2026-01-01T00:00:00Z"
status: active
"#;
        let parsed: WorktreeEntry = serde_yaml::from_str(old_yaml).unwrap();
        assert!(parsed.branch.is_none());
        assert!(parsed.lead_pid.is_none());
    }

    // -- locked_update_branch tests --

    #[test]
    fn test_locked_update_branch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "br-test".to_string(),
            path: "/tmp/br-test".to_string(),
            branch: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        locked_update_branch(&path, "br-test", "fix/cleanup").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].branch, Some("fix/cleanup".to_string()));
    }

    // -- locked_update_lead_pid tests --

    #[test]
    fn test_locked_update_lead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "pid-test".to_string(),
            path: "/tmp/pid-test".to_string(),
            branch: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        locked_update_lead_pid(&path, "pid-test", 12345).unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].lead_pid, Some(12345));
    }

    // -- locked_update_session_id tests --

    #[test]
    fn test_locked_update_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "sid-upd".to_string(),
            path: "/tmp/sid-upd".to_string(),
            branch: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        locked_update_session_id(&path, "sid-upd", "ses-new123").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].session_id, Some("ses-new123".to_string()));
    }

    // -- locked_deregister_by_name tests --

    #[test]
    fn test_locked_deregister_by_name_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "dereg-active".to_string(),
            path: "/tmp/dereg-active".to_string(),
            branch: Some("feat/x".to_string()),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        locked_deregister_by_name(&path, "dereg-active").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Removed);
    }

    #[test]
    fn test_locked_deregister_by_name_pending_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let entry = WorktreeEntry {
            name: "dereg-pending".to_string(),
            path: "/tmp/dereg-pending".to_string(),
            branch: Some("feat/p".to_string()),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::PendingCleanup,
            session_id: Some("ses-test".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        register_worktree(&path, entry).unwrap();

        locked_deregister_by_name(&path, "dereg-pending").unwrap();

        let reg = read_registry(&path).unwrap();
        assert_eq!(reg.worktrees[0].status, WorktreeStatus::Removed);
    }

    // -- locked_purge_removed tests --

    #[test]
    fn test_locked_purge_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worktrees.yaml");

        let mut reg = WorktreeRegistry::new("2026-04-01T00:00:00Z");
        for i in 0..5 {
            reg.worktrees.push(WorktreeEntry {
                name: format!("purge-{i}"),
                path: format!("/tmp/purge-{i}"),
                branch: None,
                created_at: "2026-04-01T00:00:00Z".to_string(),
                status: WorktreeStatus::Removed,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            });
        }
        // Add one active entry that should be preserved.
        reg.worktrees.push(WorktreeEntry {
            name: "keep-active".to_string(),
            path: "/tmp/keep-active".to_string(),
            branch: Some("feat/keep".to_string()),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&path, &reg).unwrap();

        // Keep 2, should purge 3.
        let purged = locked_purge_removed(&path, 2).unwrap();
        assert_eq!(purged, 3);

        let after = read_registry(&path).unwrap();
        let removed_count = after
            .worktrees
            .iter()
            .filter(|e| e.status == WorktreeStatus::Removed)
            .count();
        assert_eq!(removed_count, 2);
        // Active entry should still be there.
        assert!(after.worktrees.iter().any(|e| e.name == "keep-active"));
    }

    // -- ymd_to_days roundtrip tests --

    #[test]
    fn test_ymd_to_days_roundtrip() {
        // Unix epoch: 1970-01-01 = day 0.
        assert_eq!(ymd_to_days(1970, 1, 1), 0);
        // Check roundtrip for a known date.
        let days = ymd_to_days(2026, 4, 1);
        let (y, m, d) = days_to_ymd(days);
        assert_eq!((y, m, d), (2026, 4, 1));
    }
}
