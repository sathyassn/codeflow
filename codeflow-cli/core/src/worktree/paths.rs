//! Canonical path resolver for worktree-scoped state directories and files.
//!
//! All singleton-scoping tasks (010, 011, 012, 013) MUST use [`WorktreePaths`]
//! to resolve paths within a worktree's `.state/` directory. No ad-hoc path
//! construction is allowed.

use std::path::{Path, PathBuf};

/// Canonical path resolver for worktree-scoped state.
///
/// Holds the worktree root and derives all `.state/` sub-paths from it.
/// Used by SessionStart and all singleton-scoping tasks to ensure consistent
/// path resolution.
#[derive(Debug, Clone)]
pub struct WorktreePaths {
    /// Root directory of the worktree (e.g., `.git-worktrees/worktree-{SID}`).
    root: PathBuf,
}

impl WorktreePaths {
    /// Create a new `WorktreePaths` from a worktree root directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the worktree root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the `.state/` directory within the worktree.
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.root.join(".state")
    }

    /// Return the `.state/runtime/` directory.
    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join(".state").join("runtime")
    }

    /// Path to `codeflow-env.sh` within the worktree.
    #[cfg(test)]
    #[must_use]
    pub fn env_file(&self) -> PathBuf {
        self.runtime_dir().join("codeflow-env.sh")
    }

    /// Path to `active-task.json` within the worktree.
    #[must_use]
    pub fn active_task(&self) -> PathBuf {
        self.runtime_dir().join("active-task.json")
    }

    /// Worktree-scoped temp directory.
    ///
    /// Returns `/tmp/claude/{project_name}/{worktree_name}/managed/` where
    /// `project_name` is derived from the worktree root's grandparent
    /// directory and `worktree_name` from the root's own directory name.
    ///
    /// Each worktree gets its own temp subdirectory so concurrent sessions
    /// never destroy each other's staged protected edits on cleanup.
    #[must_use]
    pub fn temp_dir(&self) -> PathBuf {
        let project_name = self
            .root
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        let wt_name = self
            .root
            .file_name()
            .map_or_else(|| "default".into(), |n| n.to_string_lossy().to_string());
        PathBuf::from("/tmp/claude")
            .join(project_name)
            .join(wt_name)
            .join("managed")
    }

    /// Path to team config directory lookup.
    ///
    /// Returns the worktree's `.state/runtime/` directory where team config
    /// bridge files are written. The actual team config lives in
    /// `~/.claude/teams/{team_name}/`, but this path is used for worktree-local
    /// team state references.
    #[cfg(test)]
    #[must_use]
    pub fn team_config(&self) -> PathBuf {
        self.runtime_dir()
    }

    /// Return the `.state/session/{sid}/` directory.
    #[must_use]
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(".state").join("session").join(session_id)
    }

    /// Return the `.state/session/{sid}/pathflow/` directory.
    #[must_use]
    pub fn pathflow_dir(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("pathflow")
    }

    /// Return the `.state/sentinels/pathflow/{sid}/` directory.
    #[must_use]
    pub fn sentinel_dir(&self, session_id: &str) -> PathBuf {
        self.root
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_paths() -> WorktreePaths {
        WorktreePaths::new("/project/.git-worktrees/worktree-ses-abc123")
    }

    #[test]
    fn test_worktree_paths_root() {
        let wp = make_paths();
        assert_eq!(
            wp.root(),
            Path::new("/project/.git-worktrees/worktree-ses-abc123")
        );
    }

    #[test]
    fn test_worktree_paths_state_dir() {
        let wp = make_paths();
        assert_eq!(
            wp.state_dir(),
            PathBuf::from("/project/.git-worktrees/worktree-ses-abc123/.state")
        );
    }

    #[test]
    fn test_worktree_paths_runtime_dir() {
        let wp = make_paths();
        assert_eq!(
            wp.runtime_dir(),
            PathBuf::from("/project/.git-worktrees/worktree-ses-abc123/.state/runtime")
        );
    }

    #[test]
    fn test_worktree_paths_env_file() {
        let wp = make_paths();
        assert_eq!(
            wp.env_file(),
            PathBuf::from(
                "/project/.git-worktrees/worktree-ses-abc123/.state/runtime/codeflow-env.sh"
            )
        );
    }

    #[test]
    fn test_worktree_paths_active_task() {
        let wp = make_paths();
        assert_eq!(
            wp.active_task(),
            PathBuf::from(
                "/project/.git-worktrees/worktree-ses-abc123/.state/runtime/active-task.json"
            )
        );
    }

    #[test]
    fn test_worktree_paths_temp_dir() {
        let wp = make_paths();
        // project_name = "project", wt_name = "worktree-ses-abc123"
        assert_eq!(
            wp.temp_dir(),
            PathBuf::from("/tmp/claude/project/worktree-ses-abc123/managed")
        );
    }

    #[test]
    fn test_temp_dir_includes_worktree_name() {
        let wp = WorktreePaths::new("/myproject/.git-worktrees/worktree-ses-111");
        assert_eq!(
            wp.temp_dir(),
            PathBuf::from("/tmp/claude/myproject/worktree-ses-111/managed")
        );
    }

    #[test]
    fn test_temp_dir_different_worktrees_different_paths() {
        let wp_a = WorktreePaths::new("/proj/.git-worktrees/worktree-ses-aaa");
        let wp_b = WorktreePaths::new("/proj/.git-worktrees/worktree-ses-bbb");
        assert_ne!(wp_a.temp_dir(), wp_b.temp_dir());
        assert_eq!(
            wp_a.temp_dir(),
            PathBuf::from("/tmp/claude/proj/worktree-ses-aaa/managed")
        );
        assert_eq!(
            wp_b.temp_dir(),
            PathBuf::from("/tmp/claude/proj/worktree-ses-bbb/managed")
        );
    }

    #[test]
    fn test_worktree_paths_team_config() {
        let wp = make_paths();
        assert_eq!(
            wp.team_config(),
            PathBuf::from("/project/.git-worktrees/worktree-ses-abc123/.state/runtime")
        );
    }

    #[test]
    fn test_worktree_paths_session_dir() {
        let wp = make_paths();
        assert_eq!(
            wp.session_dir("ses-xyz789"),
            PathBuf::from("/project/.git-worktrees/worktree-ses-abc123/.state/session/ses-xyz789")
        );
    }

    #[test]
    fn test_worktree_paths_pathflow_dir() {
        let wp = make_paths();
        assert_eq!(
            wp.pathflow_dir("ses-xyz789"),
            PathBuf::from(
                "/project/.git-worktrees/worktree-ses-abc123/.state/session/ses-xyz789/pathflow"
            )
        );
    }

    #[test]
    fn test_worktree_paths_sentinel_dir() {
        let wp = make_paths();
        assert_eq!(
            wp.sentinel_dir("ses-xyz789"),
            PathBuf::from(
                "/project/.git-worktrees/worktree-ses-abc123/.state/sentinels/pathflow/ses-xyz789"
            )
        );
    }

    #[test]
    fn test_worktree_paths_temp_dir_fallback() {
        // If the path doesn't have enough parents, falls back to "codeflow"
        // for project_name, but still uses the root's file_name for wt_name.
        let wp = WorktreePaths::new("/single");
        assert_eq!(
            wp.temp_dir(),
            PathBuf::from("/tmp/claude/codeflow/single/managed")
        );
    }

    #[test]
    fn test_worktree_paths_debug() {
        let wp = make_paths();
        let debug = format!("{wp:?}");
        assert!(debug.contains("WorktreePaths"));
        assert!(debug.contains("worktree-ses-abc123"));
    }

    #[test]
    fn test_worktree_paths_clone() {
        let wp = make_paths();
        let wp2 = wp.clone();
        assert_eq!(wp.root(), wp2.root());
    }
}
