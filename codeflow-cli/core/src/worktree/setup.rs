//! Worktree creation, symlink setup, and local state directory creation.
//!
//! Uses `git2` for native git worktree operations instead of subprocess shelling.

use std::fs;
use std::path::Path;

use crate::diagnostics;
use crate::error::WorktreeError;
use crate::types::BranchName;

use super::registry::{self, WorktreeEntry};
use super::{LOCAL_STATE_DIRS, SHARED_STATE_DIRS, WorktreeManager};

/// Validate a worktree name.
///
/// Returns `InvalidName` if the name is empty or contains path separators
/// or other disallowed characters.
fn validate_name(name: &str) -> Result<(), WorktreeError> {
    if name.is_empty() {
        return Err(WorktreeError::InvalidName(
            "name cannot be empty".to_string(),
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(WorktreeError::InvalidName(format!(
            "name cannot contain path separators: {name}"
        )));
    }
    if name.starts_with('.') {
        return Err(WorktreeError::InvalidName(format!(
            "name cannot start with a dot: {name}"
        )));
    }
    Ok(())
}

/// Create a new worktree for the given branch.
///
/// This is the internal implementation called by `WorktreeManager::setup`.
pub(crate) fn create_worktree(
    mgr: &WorktreeManager,
    name: &str,
    branch: &BranchName,
) -> Result<WorktreeEntry, WorktreeError> {
    validate_name(name)?;

    let wt_path = mgr.base_dir().join(name);

    // Check if the worktree directory already exists.
    if wt_path.exists() {
        return Err(WorktreeError::AlreadyExists(name.to_string()));
    }

    // Open the main repository.
    let repo = git2::Repository::open(mgr.project_dir())?;

    // Ensure the base directory exists.
    fs::create_dir_all(mgr.base_dir())?;

    // Find the commit to base the worktree on.
    // Use HEAD of the main repo as the starting point.
    let head_ref = repo.head()?;
    let head_commit = head_ref.peel_to_commit()?;

    // Create the branch for the worktree.
    let _new_branch = repo
        .branch(branch.as_str(), &head_commit, false)
        .map_err(|e| {
            if e.code() == git2::ErrorCode::Exists {
                WorktreeError::AlreadyExists(format!("branch '{branch}' already exists"))
            } else {
                WorktreeError::from(e)
            }
        })?;

    // Add the worktree via git2.
    let wt_path_str = wt_path.to_string_lossy();
    repo.worktree(
        name,
        &wt_path,
        Some(
            git2::WorktreeAddOptions::new().reference(Some(
                &repo
                    .find_branch(branch.as_str(), git2::BranchType::Local)?
                    .into_reference(),
            )),
        ),
    )?;

    // Copy .gitignore from the main repo if it exists.
    let gitignore_src = mgr.project_dir().join(".gitignore");
    if gitignore_src.exists() {
        let gitignore_dst = wt_path.join(".gitignore");
        let _ = fs::copy(&gitignore_src, &gitignore_dst);
    }

    // Set up .state directory with selective symlinks.
    let state_dir = wt_path.join(".state");
    fs::create_dir_all(&state_dir)?;

    // Symlink shared state directories from the main repo.
    setup_shared_symlinks(mgr.project_dir(), &state_dir)?;

    // Create per-worktree local directories.
    create_local_dirs(&state_dir)?;

    // Install git hooks in the worktree.
    install_hooks(mgr.project_dir(), &wt_path)?;

    // Symlink shared files (local overrides, etc.).
    setup_shared_file_symlinks(mgr.project_dir(), &wt_path)?;

    // Migrate registry from old location if needed.
    super::migrate_registry_path(mgr.project_dir());

    // Register in the YAML registry.
    let entry = WorktreeEntry {
        name: name.to_string(),
        path: wt_path_str.to_string(),
        branch: branch.to_string(),
        created_at: super::now_rfc3339(),
        status: "active".to_string(),
        session_id: None,
        task_id: None,
        source: None,
    };

    let max_concurrent = crate::autorun::config::load_config(mgr.project_dir())
        .map(|c| c.worktree.max_concurrent)
        .unwrap_or(3);
    registry::locked_register_with_limit(mgr.registry_path(), &entry, max_concurrent)?;

    Ok(entry)
}

/// Create a new worktree on a detached HEAD (no branch).
///
/// Used at SessionStart when the feature branch is not yet known.
/// The branch is set later at PF3-CLASSIFY via `git checkout -b`.
///
/// This is the internal implementation called by `WorktreeManager::setup_detached`.
pub(crate) fn create_detached_worktree(
    mgr: &WorktreeManager,
    name: &str,
) -> Result<WorktreeEntry, WorktreeError> {
    validate_name(name)?;

    let wt_path = mgr.base_dir().join(name);

    // Check if the worktree directory already exists.
    if wt_path.exists() {
        return Err(WorktreeError::AlreadyExists(name.to_string()));
    }

    // Open the main repository.
    let repo = git2::Repository::open(mgr.project_dir())?;

    // Ensure the base directory exists.
    fs::create_dir_all(mgr.base_dir())?;

    // Add the worktree via git2 with no branch reference (detached HEAD).
    // When opts is None, git2/libgit2 creates the worktree on HEAD in detached state.
    // NOTE: libgit2 internally locks the git directory during worktree add,
    // so concurrent git operations on the same repo are serialized.
    repo.worktree(name, &wt_path, None)?;

    // Copy .gitignore from the main repo if it exists.
    let gitignore_src = mgr.project_dir().join(".gitignore");
    if gitignore_src.exists() {
        let gitignore_dst = wt_path.join(".gitignore");
        let _ = fs::copy(&gitignore_src, &gitignore_dst);
    }

    // Set up .state directory with selective symlinks.
    let state_dir = wt_path.join(".state");
    fs::create_dir_all(&state_dir)?;

    // Symlink shared state directories from the main repo.
    setup_shared_symlinks(mgr.project_dir(), &state_dir)?;

    // Create per-worktree local directories.
    create_local_dirs(&state_dir)?;

    // Install git hooks in the worktree.
    install_hooks(mgr.project_dir(), &wt_path)?;

    // Symlink shared files (local overrides, etc.).
    setup_shared_file_symlinks(mgr.project_dir(), &wt_path)?;

    // Migrate registry from old location if needed.
    super::migrate_registry_path(mgr.project_dir());

    // Clean stale session directories from the main repo.
    clean_stale_session_dirs(mgr.project_dir());

    // Register in the YAML registry with empty branch (set later at PF3).
    // NOTE: registry write is not locked; parallel sessions may race.
    // See file_lock.rs for locked_binary_rmw pattern.
    let wt_path_str = wt_path.to_string_lossy();
    let entry = WorktreeEntry {
        name: name.to_string(),
        path: wt_path_str.to_string(),
        branch: String::new(),
        created_at: super::now_rfc3339(),
        status: "active".to_string(),
        session_id: None,
        task_id: None,
        source: None,
    };

    let max_concurrent = crate::autorun::config::load_config(mgr.project_dir())
        .map(|c| c.worktree.max_concurrent)
        .unwrap_or(3);
    registry::locked_register_with_limit(mgr.registry_path(), &entry, max_concurrent)?;

    Ok(entry)
}

/// Clean stale session directories from the main repo's `.state/session/`.
///
/// A session directory is stale if:
/// - It has a `pathflow-session-status.json` with a `lead_pid` and that process is dead
/// - It has no status file at all (orphaned)
///
/// Concurrent sessions (lead_pid alive) are left untouched.
/// Errors are logged but do not fail worktree creation.
fn clean_stale_session_dirs(project_dir: &Path) {
    let session_base = project_dir.join(".state").join("session");
    if !session_base.is_dir() {
        return;
    }

    let entries = match fs::read_dir(&session_base) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Only process session directories (ses-*).
        if !dir_name.starts_with("ses-") {
            continue;
        }

        let status_file = path.join("pathflow").join("pathflow-session-status.json");

        if !status_file.exists() {
            // No status file = orphaned directory. Remove it.
            diagnostics::warn(
                "worktree",
                &format!("removing orphaned session dir: {dir_name}"),
            );
            let _ = fs::remove_dir_all(&path);
            continue;
        }

        // Read the status file and check lead_pid using consolidated process utility.
        if let Ok(content) = fs::read_to_string(&status_file) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(pid) = parsed.get("lead_pid").and_then(serde_json::Value::as_u64) {
                    #[allow(clippy::cast_possible_truncation)]
                    let pid32 = pid as u32;
                    if !crate::session::process::is_process_alive(pid32) {
                        diagnostics::warn(
                            "worktree",
                            &format!("removing stale session dir: {dir_name} (pid {pid} dead)"),
                        );
                        let _ = fs::remove_dir_all(&path);
                    }
                }
            }
        }
    }
}

/// Symlink shared state directories from the main repo's `.state/`.
fn setup_shared_symlinks(project_dir: &Path, wt_state_dir: &Path) -> Result<(), WorktreeError> {
    let main_state = project_dir.join(".state");

    for dir_name in SHARED_STATE_DIRS {
        let src = main_state.join(dir_name);
        let dst = wt_state_dir.join(dir_name);

        // Skip if source doesn't exist.
        if !src.exists() {
            diagnostics::warn_with_path("worktree", "shared state source missing, skipping", &src);
            continue;
        }

        // If dst is already a correct symlink, skip (idempotent).
        if dst.symlink_metadata().is_ok() {
            let meta = dst.symlink_metadata().unwrap();
            if meta.file_type().is_symlink() {
                continue;
            }
            // dst exists as a real directory (e.g., from git worktree add
            // creating dirs from tracked .gitkeep files). Remove it.
            diagnostics::warn(
                "worktree",
                &format!("replacing real directory with symlink: {}", dst.display()),
            );
            fs::remove_dir_all(&dst)?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&src, &dst)?;
        }
        #[cfg(not(unix))]
        {
            // On non-Unix platforms, fall back to directory junction or copy.
            // For now, just create a directory (worktrees primarily target Unix).
            fs::create_dir_all(&dst)?;
        }
    }

    Ok(())
}

/// Symlink individual shared files from the main repo into the worktree.
///
/// Unlike shared directories (which are entire `.state/` subdirs), shared files
/// are individual non-git-tracked files that need to be visible in worktrees.
/// Examples: `.claude/settings.local.json`, config local overrides.
///
/// Skips files that don't exist in the main repo (the user may not have
/// created a local override yet).
fn setup_shared_file_symlinks(project_dir: &Path, wt_path: &Path) -> Result<(), WorktreeError> {
    let config = crate::autorun::config::load_config(project_dir).unwrap_or_default();
    for rel_path in &config.worktree.shared_files {
        let src = project_dir.join(rel_path);
        let dst = wt_path.join(rel_path);

        // Skip if source doesn't exist (user hasn't created the local override).
        if !src.exists() {
            continue;
        }

        // Ensure parent directory exists in the worktree.
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // If dst already exists as a correct symlink, skip.
        if let Ok(meta) = dst.symlink_metadata() {
            if meta.file_type().is_symlink() {
                continue;
            }
            // Exists as a real file — remove it to replace with symlink.
            let _ = fs::remove_file(&dst);
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&src, &dst)?;
        }
    }
    Ok(())
}

/// Repair shared state symlinks in an existing worktree.
///
/// For each `SHARED_STATE_DIRS` entry, checks if the worktree has a real directory
/// instead of a symlink. If so, removes the real directory and creates the symlink.
/// This fixes worktrees created by older binaries that didn't set up symlinks.
///
/// Called from `SessionStart` on resume/compact into an existing worktree.
#[allow(clippy::missing_errors_doc)]
pub fn repair_symlinks(project_dir: &Path, worktree_path: &Path) -> Result<(), WorktreeError> {
    let wt_state_dir = worktree_path.join(".state");
    let main_state = project_dir.join(".state");

    // Ensure .state dir exists in the worktree.
    if !wt_state_dir.exists() {
        fs::create_dir_all(&wt_state_dir)?;
    }

    for dir_name in SHARED_STATE_DIRS {
        let src = main_state.join(dir_name);
        let dst = wt_state_dir.join(dir_name);

        // Skip if source doesn't exist in the main repo.
        if !src.exists() {
            continue;
        }

        // If dst doesn't exist at all, create symlink.
        let meta = match dst.symlink_metadata() {
            Ok(m) => m,
            Err(_) => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&src, &dst)?;
                }
                diagnostics::warn(
                    "worktree",
                    &format!("created missing symlink: {}", dst.display()),
                );
                continue;
            }
        };

        // Already a symlink — nothing to repair.
        if meta.file_type().is_symlink() {
            continue;
        }

        // Real directory — replace with symlink.
        diagnostics::warn(
            "worktree",
            &format!(
                "repairing: replacing real dir with symlink: {}",
                dst.display()
            ),
        );
        fs::remove_dir_all(&dst)?;

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&src, &dst)?;
        }
    }

    // Also ensure local dirs exist.
    create_local_dirs(&wt_state_dir)?;

    // Repair shared file symlinks.
    let config = crate::autorun::config::load_config(project_dir).unwrap_or_default();
    for rel_path in &config.worktree.shared_files {
        let src = project_dir.join(rel_path);
        let dst = worktree_path.join(rel_path);
        if !src.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let needs_symlink = match dst.symlink_metadata() {
            Ok(meta) => !meta.file_type().is_symlink(),
            Err(_) => true,
        };
        if needs_symlink {
            let _ = fs::remove_file(&dst);
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&src, &dst);
            }
        }
    }

    Ok(())
}

/// Create per-worktree local directories that are NOT symlinked.
fn create_local_dirs(wt_state_dir: &Path) -> Result<(), WorktreeError> {
    for dir_name in LOCAL_STATE_DIRS {
        let dst = wt_state_dir.join(dir_name);
        fs::create_dir_all(&dst)?;
    }
    Ok(())
}

/// Install git hooks by symlinking from the project's hook scripts.
///
/// Reads the worktree's `.git` file to find the actual git directory,
/// then symlinks each hook from `.codeflow/scripts/git-hooks/` into
/// the worktree's hooks directory.
fn install_hooks(project_dir: &Path, wt_path: &Path) -> Result<(), WorktreeError> {
    let hooks_src = project_dir.join(".codeflow/scripts/git-hooks");
    if !hooks_src.is_dir() {
        return Ok(());
    }

    // Read the .git file to find the actual git dir.
    // Format: "gitdir: /path/to/main/.git/worktrees/{name}"
    let git_file = wt_path.join(".git");
    let git_content = match fs::read_to_string(&git_file) {
        Ok(content) => content,
        Err(e) => {
            diagnostics::warn(
                "worktree",
                &format!("cannot read .git file for hooks installation: {e}"),
            );
            return Ok(());
        }
    };

    let git_dir = git_content.trim().strip_prefix("gitdir: ").map(Path::new);

    let git_dir = match git_dir {
        Some(dir) => dir,
        None => {
            diagnostics::warn(
                "worktree",
                &format!("unexpected .git file format: {}", git_content.trim()),
            );
            return Ok(());
        }
    };

    let hooks_dst = git_dir.join("hooks");
    fs::create_dir_all(&hooks_dst)?;

    // List all hook files in the source directory.
    let entries = match fs::read_dir(&hooks_src) {
        Ok(entries) => entries,
        Err(e) => {
            diagnostics::warn(
                "worktree",
                &format!("cannot read hooks source directory: {e}"),
            );
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let src_path = entry.path();
        if !src_path.is_file() {
            continue;
        }
        let file_name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };

        let dst_path = hooks_dst.join(&file_name);

        // Skip if already a symlink.
        if dst_path.symlink_metadata().is_ok()
            && dst_path
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        {
            continue;
        }

        // Remove existing non-symlink file if present.
        if dst_path.exists() {
            let _ = fs::remove_file(&dst_path);
        }

        #[cfg(unix)]
        {
            if let Err(e) = std::os::unix::fs::symlink(&src_path, &dst_path) {
                diagnostics::warn(
                    "worktree",
                    &format!("failed to symlink hook {file_name}: {e}"),
                );
            }
        }
        #[cfg(not(unix))]
        {
            // On non-Unix, copy the hook file instead.
            if let Err(e) = fs::copy(&src_path, &dst_path) {
                diagnostics::warn("worktree", &format!("failed to copy hook {file_name}: {e}"));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_empty() {
        let result = validate_name("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_validate_name_with_slash() {
        let result = validate_name("feat/test");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_validate_name_with_dot_prefix() {
        let result = validate_name(".hidden");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("my-worktree").is_ok());
        assert!(validate_name("feature-123").is_ok());
        assert!(validate_name("test_wt").is_ok());
    }

    #[test]
    fn test_setup_shared_symlinks() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_state = tempfile::tempdir().unwrap();

        // Create some source directories.
        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();
        fs::create_dir_all(main_dir.path().join(".state/logs")).unwrap();

        setup_shared_symlinks(main_dir.path(), wt_state.path()).unwrap();

        // Verify symlinks were created for existing source dirs.
        let db_link = wt_state.path().join("db");
        assert!(db_link.exists(), "db symlink should exist");
        assert!(
            db_link.symlink_metadata().unwrap().file_type().is_symlink(),
            "db should be a symlink"
        );

        // ledger is now LOCAL — should NOT be symlinked.
        let ledger_link = wt_state.path().join("ledger");
        assert!(
            !ledger_link.exists(),
            "ledger should not be symlinked (it is now local)"
        );

        // Dirs not present in source should not be created.
        let registry_link = wt_state.path().join("registry");
        assert!(
            !registry_link.exists(),
            "registry should not exist when source doesn't"
        );
    }

    #[test]
    fn test_setup_shared_symlinks_replaces_real_dir() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_state = tempfile::tempdir().unwrap();

        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();
        // Pre-create destination as a real directory (simulates git worktree add).
        fs::create_dir_all(wt_state.path().join("db")).unwrap();

        setup_shared_symlinks(main_dir.path(), wt_state.path()).unwrap();

        // Should now be a symlink, not a real directory.
        let db_link = wt_state.path().join("db");
        assert!(db_link.exists(), "db should exist after replacement");
        assert!(
            db_link.symlink_metadata().unwrap().file_type().is_symlink(),
            "db should be a symlink after replacing real dir"
        );
    }

    #[test]
    fn test_setup_shared_symlinks_idempotent() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_state = tempfile::tempdir().unwrap();

        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();

        // Run twice — second run should be a no-op.
        setup_shared_symlinks(main_dir.path(), wt_state.path()).unwrap();
        setup_shared_symlinks(main_dir.path(), wt_state.path()).unwrap();

        let db_link = wt_state.path().join("db");
        assert!(
            db_link.symlink_metadata().unwrap().file_type().is_symlink(),
            "db should still be a symlink after second call"
        );
    }

    #[test]
    fn test_create_local_dirs() {
        let state_dir = tempfile::tempdir().unwrap();
        create_local_dirs(state_dir.path()).unwrap();

        assert!(state_dir.path().join("runtime").is_dir());
        assert!(state_dir.path().join("session").is_dir());
        assert!(state_dir.path().join("sentinels").is_dir());
        assert!(state_dir.path().join("ledger").is_dir());
    }

    #[test]
    fn test_install_hooks_no_source_dir() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();
        // No .codeflow/scripts/git-hooks/ — should be a no-op.
        let result = install_hooks(main_dir.path(), wt_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_install_hooks_with_hooks() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        // Create hook source directory with a test hook.
        let hooks_src = main_dir.path().join(".codeflow/scripts/git-hooks");
        fs::create_dir_all(&hooks_src).unwrap();
        fs::write(hooks_src.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::write(hooks_src.join("commit-msg"), "#!/bin/sh\nexit 0\n").unwrap();

        // Create a .git file pointing to a git dir.
        let git_dir = wt_dir.path().join(".actual-git-dir");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(
            wt_dir.path().join(".git"),
            format!("gitdir: {}", git_dir.display()),
        )
        .unwrap();

        let result = install_hooks(main_dir.path(), wt_dir.path());
        assert!(result.is_ok());

        // Verify hooks were installed.
        let hooks_dst = git_dir.join("hooks");
        let pre_commit = hooks_dst.join("pre-commit");
        assert!(pre_commit.exists(), "pre-commit hook should exist");
        assert!(
            pre_commit
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "pre-commit should be a symlink"
        );

        let commit_msg = hooks_dst.join("commit-msg");
        assert!(commit_msg.exists(), "commit-msg hook should exist");
    }

    #[test]
    fn test_install_hooks_idempotent() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        let hooks_src = main_dir.path().join(".codeflow/scripts/git-hooks");
        fs::create_dir_all(&hooks_src).unwrap();
        fs::write(hooks_src.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();

        let git_dir = wt_dir.path().join(".actual-git-dir");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(
            wt_dir.path().join(".git"),
            format!("gitdir: {}", git_dir.display()),
        )
        .unwrap();

        // Run twice — should not error.
        install_hooks(main_dir.path(), wt_dir.path()).unwrap();
        install_hooks(main_dir.path(), wt_dir.path()).unwrap();

        let pre_commit = git_dir.join("hooks/pre-commit");
        assert!(
            pre_commit
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "should still be a symlink after second install"
        );
    }

    #[test]
    fn test_create_worktree_invalid_name() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let branch = BranchName::new_unchecked("feat/test");

        let result = create_worktree(&mgr, "", &branch);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_create_worktree_already_exists_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let branch = BranchName::new_unchecked("feat/test");

        // Pre-create the worktree directory.
        fs::create_dir_all(mgr.base_dir().join("test-wt")).unwrap();

        let result = create_worktree(&mgr, "test-wt", &branch);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorktreeError::AlreadyExists(_)
        ));
    }

    #[test]
    fn test_create_worktree_full_lifecycle() {
        let dir = tempfile::tempdir().unwrap();

        // Initialize a bare repo so we have a proper git repo.
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create an initial commit so HEAD exists.
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));
        let branch = BranchName::new_unchecked("feat/test-wt");

        let entry = mgr.setup("test-wt", &branch).unwrap();

        assert_eq!(entry.name, "test-wt");
        assert_eq!(entry.branch, "feat/test-wt");
        assert_eq!(entry.status, "active");
        assert!(mgr.base_dir().join("test-wt").exists());

        // Verify local state dirs were created.
        let wt_state = mgr.base_dir().join("test-wt/.state");
        assert!(wt_state.join("runtime").is_dir());
        assert!(wt_state.join("session").is_dir());
        assert!(wt_state.join("sentinels").is_dir());

        // Verify registry was written.
        let entries = mgr.list(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test-wt");
    }

    // ---------------------------------------------------------------
    // create_detached_worktree tests
    // ---------------------------------------------------------------

    /// Helper: initialize a git repo with an initial commit.
    fn init_repo_with_commit(dir: &Path) -> git2::Repository {
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
    fn test_create_detached_worktree_full_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        // Create .state dirs that should be symlinked.
        fs::create_dir_all(dir.path().join(".state/db")).unwrap();
        fs::create_dir_all(dir.path().join(".state/ledger")).unwrap();
        fs::create_dir_all(dir.path().join(".state/logs")).unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = create_detached_worktree(&mgr, "detached-wt").unwrap();

        // Verify entry fields.
        assert_eq!(entry.name, "detached-wt");
        assert_eq!(entry.branch, "", "branch should be empty for detached");
        assert_eq!(entry.status, "active");
        assert!(entry.session_id.is_none());
        assert!(!entry.created_at.is_empty());

        // Verify worktree directory exists.
        let wt_path = mgr.base_dir().join("detached-wt");
        assert!(wt_path.exists(), "worktree directory should exist");

        // Verify .git file exists (marks it as a git worktree).
        assert!(
            wt_path.join(".git").exists(),
            "worktree should have .git file"
        );
    }

    #[test]
    fn test_create_detached_worktree_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        // Create source shared state dirs.
        for shared in super::SHARED_STATE_DIRS {
            fs::create_dir_all(dir.path().join(".state").join(shared)).unwrap();
        }

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = create_detached_worktree(&mgr, "sym-wt").unwrap();
        let wt_state = Path::new(&entry.path).join(".state");

        // Verify shared dirs are symlinked.
        for shared in super::SHARED_STATE_DIRS {
            let link = wt_state.join(shared);
            assert!(link.exists(), "{shared} symlink should exist");
            assert!(
                link.symlink_metadata().unwrap().file_type().is_symlink(),
                "{shared} should be a symlink"
            );
        }

        // Verify local dirs are created fresh (not symlinks).
        for local in super::LOCAL_STATE_DIRS {
            let dir = wt_state.join(local);
            assert!(dir.is_dir(), "{local} should be a directory");
            assert!(
                !dir.symlink_metadata().unwrap().file_type().is_symlink(),
                "{local} should NOT be a symlink"
            );
        }
    }

    #[test]
    fn test_create_detached_worktree_registry_entry() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        create_detached_worktree(&mgr, "reg-wt").unwrap();

        // Read the registry and verify the entry.
        let entries = mgr.list(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "reg-wt");
        assert_eq!(entries[0].branch, "", "branch should be empty");
        assert_eq!(entries[0].status, "active");
    }

    #[test]
    fn test_create_detached_worktree_invalid_name() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());
        let mgr = WorktreeManager::new(dir.path());

        let result = create_detached_worktree(&mgr, "");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_create_detached_worktree_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());
        let mgr = WorktreeManager::new(dir.path());

        // Pre-create the worktree directory.
        fs::create_dir_all(mgr.base_dir().join("exists-wt")).unwrap();

        let result = create_detached_worktree(&mgr, "exists-wt");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorktreeError::AlreadyExists(_)
        ));
    }

    #[test]
    fn test_create_detached_worktree_base_dir_is_git_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = create_detached_worktree(&mgr, "basedir-wt").unwrap();

        // Verify the path is under .git-worktrees/.
        assert!(
            entry.path.contains(".git-worktrees"),
            "path should be under .git-worktrees/: {}",
            entry.path
        );
    }

    #[test]
    fn test_create_detached_worktree_copies_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        // Create a .gitignore in the main repo.
        fs::write(dir.path().join(".gitignore"), "*.tmp\n").unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = create_detached_worktree(&mgr, "gi-wt").unwrap();
        let wt_gitignore = Path::new(&entry.path).join(".gitignore");

        assert!(wt_gitignore.exists(), ".gitignore should be copied");
        let content = fs::read_to_string(wt_gitignore).unwrap();
        assert_eq!(content, "*.tmp\n");
    }

    #[test]
    fn test_create_detached_worktree_installs_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_commit(dir.path());

        // Create hook source directory with test hooks.
        let hooks_src = dir.path().join(".codeflow/scripts/git-hooks");
        fs::create_dir_all(&hooks_src).unwrap();
        fs::write(hooks_src.join("pre-commit"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::write(hooks_src.join("commit-msg"), "#!/bin/sh\nexit 0\n").unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let entry = create_detached_worktree(&mgr, "hooks-wt").unwrap();

        // Read the .git file to find the git dir.
        let wt_path = Path::new(&entry.path);
        let git_content = fs::read_to_string(wt_path.join(".git")).unwrap();
        let git_dir_str = git_content.trim().strip_prefix("gitdir: ").unwrap();
        let hooks_dir = Path::new(git_dir_str).join("hooks");

        // Verify hooks were installed.
        assert!(
            hooks_dir.join("pre-commit").exists(),
            "pre-commit hook should be installed"
        );
        assert!(
            hooks_dir.join("commit-msg").exists(),
            "commit-msg hook should be installed"
        );
    }

    // -- repair_symlinks tests --

    #[test]
    fn test_repair_symlinks_converts_real_dirs() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        // Create main repo .state with shared dirs.
        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();
        fs::create_dir_all(main_dir.path().join(".state/logs")).unwrap();

        // Create worktree .state with real dirs (not symlinks).
        let wt_state = wt_dir.path().join(".state");
        fs::create_dir_all(wt_state.join("db")).unwrap();
        fs::create_dir_all(wt_state.join("logs")).unwrap();

        // Verify they are real dirs.
        assert!(
            !wt_state
                .join("db")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );

        repair_symlinks(main_dir.path(), wt_dir.path()).unwrap();

        // After repair, they should be symlinks.
        assert!(
            wt_state
                .join("db")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "db should be a symlink after repair"
        );
        assert!(
            wt_state
                .join("logs")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "logs should be a symlink after repair"
        );
    }

    #[test]
    fn test_repair_symlinks_creates_missing() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        // Create main repo .state with shared dirs.
        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();

        // No .state dir in worktree at all.
        repair_symlinks(main_dir.path(), wt_dir.path()).unwrap();

        let wt_state = wt_dir.path().join(".state");
        assert!(
            wt_state
                .join("db")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "db should be created as symlink"
        );
    }

    #[test]
    fn test_repair_symlinks_preserves_existing_symlinks() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        fs::create_dir_all(main_dir.path().join(".state/db")).unwrap();
        let wt_state = wt_dir.path().join(".state");
        fs::create_dir_all(&wt_state).unwrap();

        // Create a proper symlink first.
        #[cfg(unix)]
        std::os::unix::fs::symlink(main_dir.path().join(".state/db"), wt_state.join("db")).unwrap();

        // Repair should be a no-op for existing symlinks.
        repair_symlinks(main_dir.path(), wt_dir.path()).unwrap();

        assert!(
            wt_state
                .join("db")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "db should still be a symlink"
        );
    }

    #[test]
    fn test_repair_symlinks_creates_local_dirs() {
        let main_dir = tempfile::tempdir().unwrap();
        let wt_dir = tempfile::tempdir().unwrap();

        fs::create_dir_all(main_dir.path().join(".state")).unwrap();

        repair_symlinks(main_dir.path(), wt_dir.path()).unwrap();

        let wt_state = wt_dir.path().join(".state");
        assert!(
            wt_state.join("runtime").is_dir(),
            "runtime should be created"
        );
        assert!(
            wt_state.join("session").is_dir(),
            "session should be created"
        );
        assert!(
            wt_state.join("sentinels").is_dir(),
            "sentinels should be created"
        );
    }
}
