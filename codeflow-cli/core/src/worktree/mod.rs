//! Git worktree lifecycle management.
//!
//! Provides worktree creation, cleanup, state detection, and registry
//! management using the `git2` crate for native git operations instead
//! of subprocess shelling.
//!
//! # Architecture
//!
//! - [`WorktreeManager`]: Main entry point for all worktree operations.
//! - [`WorktreeState`]: Typed enum for worktree health (Active, Stale, Orphaned).
//! - [`CleanupOpts`]: Configuration for cleanup behavior (force, dry-run, prune).
//! - Registry: YAML-based tracking file at `.state/worktrees/worktrees.yaml`.

mod cleanup;
mod paths;
mod registry;
mod setup;

use std::path::{Path, PathBuf};

use crate::error::WorktreeError;
use crate::types::BranchName;

pub use cleanup::{BranchRisk, BranchSafetyResult, CleanupOpts, check_branch_safety};
pub use paths::WorktreePaths;
pub use registry::{
    WorktreeEntry, WorktreeRegistry, WorktreeStatus, count_active, deregister_by_name, list_active,
    locked_deregister_by_name, locked_deregister_worktree, locked_purge_removed,
    locked_read_registry, locked_register_with_limit, locked_update_branch, locked_update_lead_pid,
    locked_update_session_id, locked_update_source, mark_pending_cleanup, maybe_auto_start_daemon,
    maybe_auto_stop_daemon, purge_removed_entries, read_registry, write_registry,
};
pub use setup::repair_symlinks;

// WorktreeHandle is defined in this module (not a sub-module), so no re-export needed.

/// Default base directory for worktrees (relative to project root).
const DEFAULT_BASE_DIR: &str = ".git-worktrees";

/// Default registry file path (relative to project root).
const DEFAULT_REGISTRY_PATH: &str = ".state/worktrees/worktrees.yaml";

/// Shared state directories symlinked from the main repo's `.state/`.
const SHARED_STATE_DIRS: &[&str] = &["db", "backups", "coordination", "logs", "worktrees"];

/// Per-worktree local directories (not symlinked).
///
/// `ledger` is local because each session writes to session-scoped fragment
/// files, and these must appear in the worktree's git index (not the main
/// repo's working tree). Cross-session ledger merging happens at compaction
/// time, not via shared filesystem state.
const LOCAL_STATE_DIRS: &[&str] = &["runtime", "session", "sentinels", "ledger"];

/// Typed worktree health states.
///
/// Uses exhaustive matching: adding a variant forces handling at all call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeState {
    /// Worktree directory exists and is a valid git worktree.
    Active,
    /// Registered in YAML but the directory no longer exists on disk.
    Stale,
    /// Git directory exists but no corresponding working tree is present.
    Orphaned,
}

impl WorktreeState {
    /// Return a human-readable label for this state.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Orphaned => "orphaned",
        }
    }
}

impl std::fmt::Display for WorktreeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Get the current UTC time as an RFC 3339 string.
///
/// Shared by `registry` and `setup` sub-modules.
pub(super) fn now_rfc3339() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = registry::days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Migrate worktrees.yaml from old location to new.
///
/// Called during worktree setup to handle the one-time migration from
/// `.state/worktrees.yaml` to `.state/worktrees/worktrees.yaml`.
pub fn migrate_registry_path(project_dir: &Path) {
    let old_path = project_dir.join(".state/worktrees.yaml");
    let new_dir = project_dir.join(".state/worktrees");
    let new_path = new_dir.join("worktrees.yaml");

    if old_path.exists() && !new_path.exists() {
        let _ = std::fs::create_dir_all(&new_dir);
        let _ = std::fs::rename(&old_path, &new_path);
        // Also migrate lock file if present.
        let old_lock = project_dir.join(".state/worktrees.yaml.lock");
        if old_lock.exists() {
            let _ = std::fs::rename(&old_lock, new_dir.join("worktrees.yaml.lock"));
        }
    }
}

/// Manages worktree operations for a project.
///
/// All git operations use `git2` (libgit2 bindings) instead of subprocess
/// shelling for type safety and reduced overhead.
pub struct WorktreeManager {
    /// Main repository root directory.
    project_dir: PathBuf,
    /// Base directory where worktrees are created.
    base_dir: PathBuf,
    /// Path to the worktrees.yaml registry file.
    registry_path: PathBuf,
    /// Optional callback that returns `true` if a `PathFlow` session is active.
    /// When active, cleanup is blocked unless `force` is set.
    pathflow_guard: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

impl WorktreeManager {
    /// Create a new `WorktreeManager` with default paths.
    ///
    /// - Base directory: `{project_dir}/.git-worktrees/`
    /// - Registry: `{project_dir}/.state/worktrees/worktrees.yaml`
    #[must_use]
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        let project_dir = project_dir.into();
        let base_dir = project_dir.join(DEFAULT_BASE_DIR);
        let registry_path = project_dir.join(DEFAULT_REGISTRY_PATH);
        Self {
            project_dir,
            base_dir,
            registry_path,
            pathflow_guard: None,
        }
    }

    /// Set a custom base directory for worktrees.
    #[cfg(test)]
    #[must_use]
    pub fn with_base_dir(mut self, base_dir: impl Into<PathBuf>) -> Self {
        self.base_dir = base_dir.into();
        self
    }

    /// Set a custom registry file path.
    #[must_use]
    pub fn with_registry_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.registry_path = path.into();
        self
    }

    /// Set a `PathFlow` guard callback.
    ///
    /// The callback should return `true` when a `PathFlow` session is active.
    /// During cleanup, if the guard returns `true` and `force` is not set,
    /// cleanup will be blocked with `WorktreeError::PathFlowActive`.
    #[cfg(test)]
    #[must_use]
    pub fn with_pathflow_guard(mut self, guard: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.pathflow_guard = Some(Box::new(guard));
        self
    }

    /// Return the project root directory.
    #[must_use]
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Return the base directory where worktrees are created.
    #[must_use]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Return the path to the registry file.
    #[must_use]
    pub fn registry_path(&self) -> &Path {
        &self.registry_path
    }

    /// Create a new worktree for the given branch.
    ///
    /// This operation:
    /// 1. Validates the worktree name and branch.
    /// 2. Creates a git worktree via `git2`.
    /// 3. Copies `.gitignore` from the main repo.
    /// 4. Symlinks shared state directories from main `.state/`.
    /// 5. Creates per-worktree local directories.
    /// 6. Registers the worktree in the YAML registry.
    ///
    /// # Errors
    ///
    /// - `InvalidName` if the name is empty or contains path separators.
    /// - `AlreadyExists` if the worktree directory already exists.
    /// - `Git` for git2 operation failures.
    /// - `Io` for filesystem errors.
    pub fn setup(&self, name: &str, branch: &BranchName) -> Result<WorktreeEntry, WorktreeError> {
        setup::create_worktree(self, name, branch)
    }

    /// Create a new worktree on a detached HEAD (no branch).
    ///
    /// Used at SessionStart when the feature branch is not yet known.
    /// The branch is set later at PF3-CLASSIFY via `git checkout -b`.
    ///
    /// This operation:
    /// 1. Validates the worktree name.
    /// 2. Creates a git worktree on detached HEAD via `git2`.
    /// 3. Copies `.gitignore` from the main repo.
    /// 4. Symlinks shared state directories from main `.state/`.
    /// 5. Creates per-worktree local directories.
    /// 6. Registers the worktree in the YAML registry with an empty branch.
    ///
    /// # Errors
    ///
    /// - `InvalidName` if the name is empty or contains path separators.
    /// - `AlreadyExists` if the worktree directory already exists.
    /// - `Git` for git2 operation failures.
    /// - `Io` for filesystem errors.
    pub fn setup_detached(&self, name: &str) -> Result<WorktreeEntry, WorktreeError> {
        setup::create_detached_worktree(self, name)
    }

    /// Remove a worktree and deregister it.
    ///
    /// # Errors
    ///
    /// - `PathFlowActive` if guard is set, returns true, and `force` is false.
    /// - `InvalidName` if the name is empty.
    /// - `NotFound` if the worktree doesn't exist (non-force mode).
    /// - `Git` for git2 operation failures.
    pub fn cleanup(&self, name: &str, opts: &CleanupOpts) -> Result<(), WorktreeError> {
        cleanup::cleanup_worktree(self, name, opts)
    }

    /// Determine the state of a worktree.
    ///
    /// Checks the filesystem and git state to classify the worktree as
    /// Active, Stale, or Orphaned.
    #[must_use]
    pub fn detect_state(&self, entry: &WorktreeEntry) -> WorktreeState {
        let path = Path::new(&entry.path);

        // If the directory doesn't exist, it's stale.
        if !path.exists() {
            return WorktreeState::Stale;
        }

        // If the directory exists, check if it's a valid git worktree.
        let git_dir = path.join(".git");
        if git_dir.exists() {
            WorktreeState::Active
        } else {
            // Directory exists but not a valid git worktree.
            WorktreeState::Orphaned
        }
    }

    /// Check if a worktree's owning session is alive (public API for CLI).
    ///
    /// Returns `"ACTIVE"` if the session PID is alive, `"DEAD"` if the PID
    /// is confirmed dead, or `"UNKNOWN"` if no session ID is available.
    #[must_use]
    pub fn liveness_status(&self, entry: &WorktreeEntry) -> &'static str {
        match &entry.session_id {
            Some(sid) if !sid.is_empty() => {
                if let Some(pid) = self.read_lead_pid(entry) {
                    if crate::session::process::is_process_alive(pid) {
                        return "ACTIVE";
                    }
                    return "DEAD";
                }
                // No PID file — check pathflow-active as fallback.
                let _ = sid; // suppress unused warning
                if self.has_pathflow_active(entry) {
                    "ACTIVE"
                } else {
                    "UNKNOWN"
                }
            }
            _ => "UNKNOWN",
        }
    }

    /// List all worktrees from the registry, optionally filtered by status.
    ///
    /// # Errors
    ///
    /// Returns `WorktreeError::Yaml` or `WorktreeError::Io` on registry read failure.
    pub fn list(&self, status_filter: Option<&str>) -> Result<Vec<WorktreeEntry>, WorktreeError> {
        if !self.registry_path.exists() {
            return Ok(Vec::new());
        }
        let reg = registry::read_registry(&self.registry_path)?;
        let entries = reg.worktrees;
        if let Some(filter) = status_filter {
            Ok(entries
                .into_iter()
                .filter(|e| e.status.to_string() == filter)
                .collect())
        } else {
            Ok(entries)
        }
    }

    /// Check if the `PathFlow` guard is active.
    pub(crate) fn is_pathflow_active(&self) -> bool {
        self.pathflow_guard.as_ref().is_some_and(|guard| guard())
    }

    /// Remove all stale (and optionally orphaned) worktrees from the registry.
    ///
    /// Iterates over registry entries, detects their state, checks session
    /// liveness (PID check + pathflow-active flag), and verifies branch safety
    /// before removal. Active sessions are skipped. Unpushed branches are
    /// blocked unless `force` is set.
    ///
    /// Also purges old "removed" entries beyond the `keep` threshold (default 5).
    ///
    /// Returns the names of worktrees that were removed (or would be removed
    /// in dry-run mode).
    ///
    /// # Errors
    ///
    /// Returns `WorktreeError::Io` or `WorktreeError::Yaml` on registry access failure.
    pub fn cleanup_stale(&self, opts: &CleanupOpts) -> Result<Vec<String>, WorktreeError> {
        let reg = registry::locked_read_registry(&self.registry_path)?;
        let mut removed = Vec::new();

        for entry in &reg.worktrees {
            if entry.status == registry::WorktreeStatus::Removed {
                continue;
            }

            let state = self.detect_state(entry);
            let liveness = self.liveness_status(entry);

            // Determine if this entry is a cleanup candidate.
            let is_candidate = match entry.status {
                // PendingCleanup entries are always cleanable (no --force needed).
                registry::WorktreeStatus::PendingCleanup => true,
                registry::WorktreeStatus::Active => match state {
                    // Directory missing — always cleanable.
                    WorktreeState::Stale => true,
                    // Dead session — cleanable without --force.
                    WorktreeState::Active if liveness == "DEAD" => true,
                    // No session_id and entry is old enough — cleanable.
                    WorktreeState::Active
                        if liveness == "UNKNOWN" && Self::is_older_than(entry, 3600) =>
                    {
                        true
                    }
                    // Live session — skip.
                    WorktreeState::Active => false,
                    // Orphaned worktree — only with force.
                    WorktreeState::Orphaned => opts.force,
                },
                registry::WorktreeStatus::Removed => false,
            };

            if !is_candidate {
                continue;
            }

            // Branch safety check for entries with existing directories.
            let wt_path = Path::new(&entry.path);
            if wt_path.exists() {
                let safety = cleanup::check_branch_safety(wt_path);
                if safety.risk >= BranchRisk::High && !opts.force {
                    crate::diagnostics::warn(
                        "worktree",
                        &format!(
                            "skipping '{}': {} (use --force to override)",
                            entry.name, safety.message
                        ),
                    );
                    continue;
                }
                if safety.risk == BranchRisk::Medium {
                    crate::diagnostics::warn(
                        "worktree",
                        &format!("{}: {}", entry.name, safety.message),
                    );
                }
            }

            removed.push(entry.name.clone());
            if !opts.dry_run {
                self.cleanup(&entry.name, opts)?;
            }
        }

        // Purge old "removed" entries from registry (locked).
        let max_keep = opts.keep.unwrap_or(5);
        if !opts.dry_run {
            let _ = registry::locked_purge_removed(&self.registry_path, max_keep);
        }

        Ok(removed)
    }

    /// Read the `lead_pid` for a worktree entry.
    ///
    /// Checks three sources in order:
    /// 1. The `lead_pid` field on the registry entry (fast path).
    /// 2. The session status file in the worktree path.
    /// 3. The session status file in the main repo.
    fn read_lead_pid(&self, entry: &WorktreeEntry) -> Option<u32> {
        // 1. Fast path: check the entry itself.
        if let Some(pid) = entry.lead_pid {
            return Some(pid);
        }

        let session_id = entry.session_id.as_deref().filter(|s| !s.is_empty())?;

        // 2. Check the worktree's session status file.
        let wt_path = Path::new(&entry.path);
        if wt_path.exists() {
            let wt_status = wt_path
                .join(".state/session")
                .join(session_id)
                .join("pathflow/pathflow-session-status.json");
            if let Some(pid) = Self::read_pid_from_status_file(&wt_status) {
                return Some(pid);
            }
        }

        // 3. Fallback: check the main repo's session directory.
        let main_status = self
            .project_dir
            .join(".state/session")
            .join(session_id)
            .join("pathflow/pathflow-session-status.json");
        Self::read_pid_from_status_file(&main_status)
    }

    /// Read `lead_pid` from a `pathflow-session-status.json` file.
    fn read_pid_from_status_file(path: &Path) -> Option<u32> {
        let content = std::fs::read_to_string(path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let pid = parsed.get("lead_pid")?.as_u64()?;
        #[allow(clippy::cast_possible_truncation)]
        Some(pid as u32)
    }

    /// Check if a worktree entry is older than `secs` seconds.
    ///
    /// Uses `created_at` field (ISO 8601). Returns `true` if parsing fails
    /// (conservative: treat unparseable entries as old).
    fn is_older_than(entry: &WorktreeEntry, secs: u64) -> bool {
        // Simple ISO 8601 parsing: extract year-month-day hour:min:sec.
        let ts = &entry.created_at;
        if ts.len() < 19 {
            return true; // Unparseable — treat as old.
        }

        // Use the same epoch-based approach as now_rfc3339().
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Parse created_at naively (YYYY-MM-DDTHH:MM:SSZ).
        let parts: Vec<&str> = ts.split(|c: char| !c.is_ascii_digit()).collect();
        if parts.len() < 6 {
            return true;
        }
        let Ok(year) = parts[0].parse::<u64>() else {
            return true;
        };
        let Ok(month) = parts[1].parse::<u64>() else {
            return true;
        };
        let Ok(day) = parts[2].parse::<u64>() else {
            return true;
        };
        let Ok(hour) = parts[3].parse::<u64>() else {
            return true;
        };
        let Ok(minute) = parts[4].parse::<u64>() else {
            return true;
        };
        let Ok(second) = parts[5].parse::<u64>() else {
            return true;
        };

        // Rough epoch calculation (not leap-second accurate, but good enough).
        let days = registry::ymd_to_days(year, month, day);
        let created_epoch = days * 86400 + hour * 3600 + minute * 60 + second;

        now.saturating_sub(created_epoch) > secs
    }

    /// Check if a worktree has a `pathflow-active` flag in its state directory.
    #[allow(clippy::unused_self)]
    fn has_pathflow_active(&self, entry: &WorktreeEntry) -> bool {
        let wt_path = Path::new(&entry.path);
        let active_flag = wt_path.join(".state/session/pathflow-active");
        active_flag.exists()
    }

    /// Reconcile the YAML registry against the filesystem.
    ///
    /// Detects two kinds of inconsistencies:
    /// - **Stale entries**: in registry but directory missing on disk.
    /// - **Orphaned directories**: on disk in `.git-worktrees/` but not in registry.
    ///
    /// Returns `(stale_entries, orphaned_dirs)` — names/paths of each category.
    /// In non-dry-run mode, stale entries are deregistered from the registry.
    /// Orphaned directories are reported but NOT auto-registered.
    ///
    /// # Errors
    ///
    /// Returns `WorktreeError::Io` or `WorktreeError::Yaml` on registry/filesystem access failure.
    pub fn reconcile_registry(
        &self,
        dry_run: bool,
    ) -> Result<(Vec<String>, Vec<String>), WorktreeError> {
        let reg = registry::locked_read_registry(&self.registry_path)?;

        // Find stale entries: in registry (active) but directory missing.
        let mut stale_entries = Vec::new();
        for entry in &reg.worktrees {
            if entry.status == registry::WorktreeStatus::Removed {
                continue;
            }
            let state = self.detect_state(entry);
            if state == WorktreeState::Stale {
                stale_entries.push(entry.name.clone());
                if !dry_run {
                    let wt_path_str = entry.path.clone();
                    registry::locked_deregister_worktree(&self.registry_path, &wt_path_str)?;
                }
            }
        }

        // Find orphaned directories: on disk but not in registry.
        let mut orphaned_dirs = Vec::new();
        if self.base_dir.exists() {
            let registry_paths: std::collections::HashSet<String> = reg
                .worktrees
                .iter()
                .filter(|e| e.status != registry::WorktreeStatus::Removed)
                .map(|e| e.path.clone())
                .collect();

            if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
                for dir_entry in entries.flatten() {
                    let path = dir_entry.path();
                    if path.is_dir() {
                        let path_str = path.to_string_lossy().to_string();
                        if !registry_paths.contains(&path_str) {
                            orphaned_dirs.push(path_str);
                        }
                    }
                }
            }
        }

        Ok((stale_entries, orphaned_dirs))
    }
}

/// RAII handle for a worktree that ensures cleanup on scope exit.
///
/// When dropped, the handle removes the worktree directory and deregisters
/// it from the YAML registry. This is the safety net for cleanup — the
/// happy-path cleanup is performed by the SessionEnd hook (task 014).
///
/// Drop failures are logged to stderr (Drop cannot return errors or panic).
pub struct WorktreeHandle {
    /// Worktree name (used for git worktree removal).
    name: String,
    /// Absolute path to the worktree directory.
    path: PathBuf,
    /// Path to the YAML registry file.
    registry_path: PathBuf,
    /// Path to the project root (for git operations).
    project_dir: PathBuf,
    /// Whether cleanup should be skipped (set when ownership is transferred).
    defused: bool,
}

impl WorktreeHandle {
    /// Create a new handle from a worktree entry and manager.
    #[must_use]
    pub fn new(entry: &WorktreeEntry, mgr: &WorktreeManager) -> Self {
        Self {
            name: entry.name.clone(),
            path: PathBuf::from(&entry.path),
            registry_path: mgr.registry_path().to_path_buf(),
            project_dir: mgr.project_dir().to_path_buf(),
            defused: false,
        }
    }

    /// Return the worktree name.
    #[cfg(test)]
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the worktree path.
    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Defuse the handle so Drop does NOT perform cleanup.
    ///
    /// Call this when the worktree ownership is transferred to another
    /// cleanup mechanism (e.g., the SessionEnd hook for happy-path cleanup).
    pub fn defuse(&mut self) {
        self.defused = true;
    }

    /// Return whether this handle has been defused.
    #[cfg(test)]
    #[must_use]
    pub fn is_defused(&self) -> bool {
        self.defused
    }
}

impl Drop for WorktreeHandle {
    fn drop(&mut self) {
        if self.defused {
            return;
        }

        // Attempt to remove the worktree via git2.
        if let Ok(repo) = git2::Repository::open(&self.project_dir) {
            if let Ok(wt) = repo.find_worktree(&self.name) {
                let _ = wt.prune(Some(
                    git2::WorktreePruneOptions::new()
                        .valid(true)
                        .working_tree(true),
                ));
            }
        }

        // Remove the directory if it still exists.
        if self.path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&self.path) {
                eprintln!(
                    "WorktreeHandle: failed to remove worktree directory {}: {e}",
                    self.path.display()
                );
            }
        }

        // Deregister from the YAML registry (locked to avoid TOCTOU).
        let path_str = self.path.to_string_lossy();
        if let Err(e) = registry::locked_deregister_worktree(&self.registry_path, &path_str) {
            eprintln!(
                "WorktreeHandle: failed to deregister worktree {}: {e}",
                self.name
            );
        }
    }
}

impl std::fmt::Debug for WorktreeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeHandle")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("registry_path", &self.registry_path)
            .field("project_dir", &self.project_dir)
            .field("defused", &self.defused)
            .finish()
    }
}

impl std::fmt::Debug for WorktreeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorktreeManager")
            .field("project_dir", &self.project_dir)
            .field("base_dir", &self.base_dir)
            .field("registry_path", &self.registry_path)
            .field("pathflow_guard", &self.pathflow_guard.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worktree_state_as_str() {
        assert_eq!(WorktreeState::Active.as_str(), "active");
        assert_eq!(WorktreeState::Stale.as_str(), "stale");
        assert_eq!(WorktreeState::Orphaned.as_str(), "orphaned");
    }

    #[test]
    fn test_worktree_state_display() {
        assert_eq!(WorktreeState::Active.to_string(), "active");
        assert_eq!(WorktreeState::Stale.to_string(), "stale");
        assert_eq!(WorktreeState::Orphaned.to_string(), "orphaned");
    }

    #[test]
    fn test_worktree_state_exhaustive_match() {
        // Exhaustive match: if a new variant is added, this will fail to compile.
        let states = [
            WorktreeState::Active,
            WorktreeState::Stale,
            WorktreeState::Orphaned,
        ];
        for state in states {
            let label = match state {
                WorktreeState::Active => "active",
                WorktreeState::Stale => "stale",
                WorktreeState::Orphaned => "orphaned",
            };
            assert_eq!(state.as_str(), label);
        }
    }

    #[test]
    fn test_manager_default_paths() {
        let mgr = WorktreeManager::new("/tmp/project");
        assert_eq!(mgr.project_dir(), Path::new("/tmp/project"));
        assert_eq!(mgr.base_dir(), Path::new("/tmp/project/.git-worktrees"));
        assert_eq!(
            mgr.registry_path(),
            Path::new("/tmp/project/.state/worktrees/worktrees.yaml")
        );
    }

    #[test]
    fn test_manager_custom_paths() {
        let mgr = WorktreeManager::new("/tmp/project")
            .with_base_dir("/custom/worktrees")
            .with_registry_path("/custom/registry.yaml");
        assert_eq!(mgr.base_dir(), Path::new("/custom/worktrees"));
        assert_eq!(mgr.registry_path(), Path::new("/custom/registry.yaml"));
    }

    #[test]
    fn test_manager_pathflow_guard_none() {
        let mgr = WorktreeManager::new("/tmp/project");
        assert!(!mgr.is_pathflow_active());
    }

    #[test]
    fn test_manager_pathflow_guard_active() {
        let mgr = WorktreeManager::new("/tmp/project").with_pathflow_guard(|| true);
        assert!(mgr.is_pathflow_active());
    }

    #[test]
    fn test_manager_pathflow_guard_inactive() {
        let mgr = WorktreeManager::new("/tmp/project").with_pathflow_guard(|| false);
        assert!(!mgr.is_pathflow_active());
    }

    #[test]
    fn test_manager_debug() {
        let mgr = WorktreeManager::new("/tmp/project");
        let debug = format!("{mgr:?}");
        assert!(debug.contains("WorktreeManager"));
        assert!(debug.contains("/tmp/project"));
    }

    #[test]
    fn test_detect_state_stale() {
        let mgr = WorktreeManager::new("/tmp/nonexistent-project");
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/tmp/nonexistent-worktree-path".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.detect_state(&entry), WorktreeState::Stale);
    }

    #[test]
    fn test_detect_state_orphaned() {
        // A directory that exists but has no .git file/dir is Orphaned.
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.detect_state(&entry), WorktreeState::Orphaned);
    }

    #[test]
    fn test_detect_state_active() {
        // A directory with a .git file/dir is Active.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".git"), "gitdir: /somewhere").unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-07T10:30:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.detect_state(&entry), WorktreeState::Active);
    }

    #[test]
    fn test_list_empty_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entries = mgr.list(None).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_shared_state_dirs_constant() {
        assert_eq!(SHARED_STATE_DIRS.len(), 5);
        assert!(SHARED_STATE_DIRS.contains(&"db"));
        assert!(
            !SHARED_STATE_DIRS.contains(&"ledger"),
            "ledger is now LOCAL"
        );
        assert!(
            !SHARED_STATE_DIRS.contains(&"registry"),
            "registry removed from shared dirs"
        );
        assert!(SHARED_STATE_DIRS.contains(&"logs"));
        assert!(SHARED_STATE_DIRS.contains(&"worktrees"));
    }

    #[test]
    fn test_migrate_registry_path_moves_file() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("worktrees.yaml"), "test content").unwrap();
        std::fs::write(state_dir.join("worktrees.yaml.lock"), "lock").unwrap();

        migrate_registry_path(dir.path());

        assert!(!state_dir.join("worktrees.yaml").exists());
        assert!(!state_dir.join("worktrees.yaml.lock").exists());
        assert!(state_dir.join("worktrees/worktrees.yaml").exists());
        assert!(state_dir.join("worktrees/worktrees.yaml.lock").exists());
        let content = std::fs::read_to_string(state_dir.join("worktrees/worktrees.yaml")).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_migrate_registry_path_noop_when_new_exists() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(state_dir.join("worktrees")).unwrap();
        std::fs::write(state_dir.join("worktrees.yaml"), "old").unwrap();
        std::fs::write(state_dir.join("worktrees/worktrees.yaml"), "new").unwrap();

        migrate_registry_path(dir.path());

        // Old file should still exist (not moved).
        assert!(state_dir.join("worktrees.yaml").exists());
        // New file should be unchanged.
        let content = std::fs::read_to_string(state_dir.join("worktrees/worktrees.yaml")).unwrap();
        assert_eq!(content, "new");
    }

    #[test]
    fn test_migrate_registry_path_noop_when_old_missing() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();

        // Should be a no-op with no errors.
        migrate_registry_path(dir.path());
        assert!(!state_dir.join("worktrees/worktrees.yaml").exists());
    }

    #[test]
    fn test_local_state_dirs_constant() {
        assert_eq!(LOCAL_STATE_DIRS.len(), 4);
        assert!(LOCAL_STATE_DIRS.contains(&"runtime"));
        assert!(LOCAL_STATE_DIRS.contains(&"session"));
        assert!(LOCAL_STATE_DIRS.contains(&"sentinels"));
        assert!(LOCAL_STATE_DIRS.contains(&"ledger"));
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = now_rfc3339();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "should end with Z: {ts}");
        assert_eq!(ts.len(), 20, "should be 20 chars: {ts}");
        assert_eq!(&ts[4..5], "-", "should have dash at pos 4: {ts}");
        assert_eq!(&ts[10..11], "T", "should have T at pos 10: {ts}");
    }

    // ---------------------------------------------------------------
    // setup_detached tests (via WorktreeManager)
    // ---------------------------------------------------------------

    /// Helper: initialize a git repo with an initial commit for tests.
    fn init_test_repo(dir: &std::path::Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn test_setup_detached_creates_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = mgr.setup_detached("ses-test").unwrap();

        assert_eq!(entry.name, "ses-test");
        assert!(entry.branch.is_none(), "branch should be None for detached");
        assert_eq!(entry.status, registry::WorktreeStatus::Active);
        assert!(entry.session_id.is_none());

        // Verify directory exists with .git file (valid worktree).
        let wt_dir = mgr.base_dir().join("ses-test");
        assert!(wt_dir.exists());
        assert!(wt_dir.join(".git").exists());
    }

    #[test]
    fn test_setup_detached_uses_git_worktrees_base() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = mgr.setup_detached("base-test").unwrap();

        assert!(
            entry.path.contains(".git-worktrees"),
            "path should use .git-worktrees/ base: {}",
            entry.path
        );
        assert_eq!(
            mgr.base_dir(),
            dir.path().join(".git-worktrees"),
            "base_dir should be .git-worktrees/"
        );
    }

    #[test]
    fn test_setup_detached_invalid_name_empty() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let mgr = WorktreeManager::new(dir.path());

        let result = mgr.setup_detached("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_setup_detached_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());
        let mgr = WorktreeManager::new(dir.path());

        // Pre-create the directory.
        std::fs::create_dir_all(mgr.base_dir().join("dup-wt")).unwrap();

        let result = mgr.setup_detached("dup-wt");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorktreeError::AlreadyExists(_)
        ));
    }

    // ---------------------------------------------------------------
    // WorktreeHandle tests
    // ---------------------------------------------------------------

    #[test]
    fn test_worktree_handle_new() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "handle-test".to_string(),
            path: "/tmp/handle-test".to_string(),
            branch: None,
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let handle = WorktreeHandle::new(&entry, &mgr);
        assert_eq!(handle.name(), "handle-test");
        assert_eq!(handle.path(), std::path::Path::new("/tmp/handle-test"));
        assert!(!handle.is_defused());
    }

    #[test]
    fn test_worktree_handle_defuse() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "defuse-test".to_string(),
            path: "/tmp/defuse-test".to_string(),
            branch: None,
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let mut handle = WorktreeHandle::new(&entry, &mgr);
        assert!(!handle.is_defused());
        handle.defuse();
        assert!(handle.is_defused());
    }

    #[test]
    fn test_worktree_handle_debug() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "debug-test".to_string(),
            path: "/tmp/debug-test".to_string(),
            branch: None,
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let handle = WorktreeHandle::new(&entry, &mgr);
        let debug = format!("{handle:?}");
        assert!(debug.contains("WorktreeHandle"));
        assert!(debug.contains("debug-test"));
        assert!(debug.contains("defused: false"));
    }

    #[test]
    fn test_worktree_handle_drop_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        // Create a real detached worktree.
        let entry = mgr.setup_detached("drop-test").unwrap();
        let wt_path = PathBuf::from(&entry.path);
        assert!(wt_path.exists(), "worktree should exist before drop");

        // Create handle and drop it.
        {
            let _handle = WorktreeHandle::new(&entry, &mgr);
            // Handle drops here.
        }

        // Verify cleanup happened.
        assert!(
            !wt_path.exists(),
            "worktree dir should be removed after drop"
        );

        // Verify registry was updated.
        let entries = mgr.list(Some("removed")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "drop-test");
    }

    #[test]
    fn test_worktree_handle_defused_does_not_clean_up() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_test_repo(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = mgr.setup_detached("defused-drop").unwrap();
        let wt_path = PathBuf::from(&entry.path);

        // Create handle, defuse it, then drop.
        {
            let mut handle = WorktreeHandle::new(&entry, &mgr);
            handle.defuse();
            // Handle drops here but should NOT clean up.
        }

        // Verify worktree still exists (defused = no cleanup).
        assert!(
            wt_path.exists(),
            "worktree should still exist after defused drop"
        );

        // Verify registry still shows active.
        let entries = mgr.list(Some("active")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "defused-drop");
    }

    #[test]
    fn test_worktree_handle_drop_on_nonexistent() {
        // Handle for an already-removed worktree should not panic on drop.
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "ghost-wt".to_string(),
            path: dir
                .path()
                .join("nonexistent-wt")
                .to_string_lossy()
                .to_string(),
            branch: None,
            created_at: "2026-03-07T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        // This should not panic.
        {
            let _handle = WorktreeHandle::new(&entry, &mgr);
        }
    }

    // ---------------------------------------------------------------
    // Liveness check tests
    // ---------------------------------------------------------------

    #[test]
    fn test_liveness_status_no_session_id() {
        let mgr = WorktreeManager::new("/tmp/project");
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-31T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.liveness_status(&entry), "UNKNOWN");
    }

    #[test]
    fn test_liveness_status_empty_session_id() {
        let mgr = WorktreeManager::new("/tmp/project");
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-31T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: Some(String::new()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.liveness_status(&entry), "UNKNOWN");
    }

    #[test]
    fn test_liveness_status_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let sid = "ses-test-dead";

        // Create a session status file with a definitely-dead PID.
        let status_dir = dir.path().join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            r#"{"lead_pid": 999999999}"#,
        )
        .unwrap();

        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-31T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: Some(sid.to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.liveness_status(&entry), "DEAD");
    }

    #[test]
    fn test_read_lead_pid_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "test".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: Some("ses-nonexistent".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert!(mgr.read_lead_pid(&entry).is_none());
    }

    #[test]
    fn test_read_lead_pid_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let sid = "ses-test-pid";

        let status_dir = dir.path().join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            r#"{"lead_pid": 12345}"#,
        )
        .unwrap();

        let entry = WorktreeEntry {
            name: "test".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: Some(sid.to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert_eq!(mgr.read_lead_pid(&entry), Some(12345));
    }

    #[test]
    fn test_read_lead_pid_from_entry_field() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "test".to_string(),
            path: "/nonexistent".to_string(),
            branch: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: Some("ses-test".to_string()),
            task_id: None,
            source: None,
            lead_pid: Some(99999),
        };
        // Should return the entry's lead_pid directly (fast path).
        assert_eq!(mgr.read_lead_pid(&entry), Some(99999));
    }

    #[test]
    fn test_has_pathflow_active_false() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-31T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert!(!mgr.has_pathflow_active(&entry));
    }

    #[test]
    fn test_has_pathflow_active_true() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        // Create pathflow-active flag.
        let flag_dir = dir.path().join(".state/session");
        std::fs::create_dir_all(&flag_dir).unwrap();
        std::fs::write(flag_dir.join("pathflow-active"), "").unwrap();

        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: dir.path().to_string_lossy().to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-31T10:00:00Z".to_string(),
            status: registry::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };
        assert!(mgr.has_pathflow_active(&entry));
    }

    #[test]
    fn test_cleanup_stale_purges_old_removed() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = registry::WorktreeRegistry::new("2026-03-31T10:00:00Z");

        // Add 8 removed entries.
        for i in 0..8 {
            reg.worktrees.push(WorktreeEntry {
                name: format!("removed-{i}"),
                path: format!("/gone/{i}"),
                branch: Some(format!("feat/r{i}")),
                created_at: "2026-03-01T10:00:00Z".to_string(),
                status: registry::WorktreeStatus::Removed,
                session_id: None,
                task_id: None,
                source: None,
                lead_pid: None,
            });
        }
        registry::write_registry(&reg_path, &reg).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts {
            keep: Some(3),
            ..Default::default()
        };
        let removed = mgr.cleanup_stale(&opts).unwrap();
        assert!(removed.is_empty()); // No stale entries to remove

        // But old removed entries should be purged.
        let reg_after = registry::read_registry(&reg_path).unwrap();
        let removed_count = reg_after
            .worktrees
            .iter()
            .filter(|e| e.status == registry::WorktreeStatus::Removed)
            .count();
        assert_eq!(removed_count, 3, "should keep only 3 removed entries");
    }
}
