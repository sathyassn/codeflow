//! Seat capacity: the per-project account profile and the limit readings of
//! each model family (TSK-431, issue #234 items 1 and 2).
//!
//! Neither part reads, copies or stores a token. The profile names folders
//! and spend caps; the limit store keeps percentages and reset times. Neither
//! switches accounts: a launch sets one family's config-home variable to a
//! folder the operator already signed in to.

use std::path::{Path, PathBuf};

pub mod accounts;
pub mod limits;

/// Folder, under the main checkout, for untracked per-machine state.
pub const LOCAL_DIR: &str = ".codeflow/local";

/// A model family a seat can run: the CLI whose config home and limits are
/// kept per project.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Family {
    /// Claude Code.
    Claude,
    /// Codex.
    Codex,
    /// Grok Build.
    Grok,
}

impl Family {
    /// Every family, in display order.
    pub const ALL: [Family; 3] = [Family::Claude, Family::Codex, Family::Grok];

    /// The catalog family id, also the key in the profile and the limit store.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Family::Claude => "claude",
            Family::Codex => "codex",
            Family::Grok => "grok",
        }
    }

    /// The one environment variable that moves this CLI's config home.
    #[must_use]
    pub fn config_home_var(self) -> &'static str {
        match self {
            Family::Claude => "CLAUDE_CONFIG_DIR",
            Family::Codex => "CODEX_HOME",
            Family::Grok => "GROK_HOME",
        }
    }

    /// The family with this id, or `None`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|family| family.id() == id)
    }
}

/// The main checkout's root when `root` is a linked worktree of it, else
/// `root`. The untracked profile and the limit store live in the main
/// checkout, so every worktree of a project reads the same ones. A folder
/// that is not a repository is its own root.
///
/// # Errors
/// The repository exists but cannot be opened, so the shared folder cannot be
/// told; the caller refuses instead of reading a different folder.
pub fn shared_root(root: &Path) -> Result<PathBuf, String> {
    let repo = match git2::Repository::open(root) {
        Ok(repo) => repo,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(root.to_path_buf()),
        Err(error) => {
            return Err(format!(
                "cannot open the repository at {}: {}",
                crate::git::display_path(root),
                error.message()
            ))
        }
    };
    let common = repo.commondir();
    let main = (common.file_name().is_some_and(|name| name == ".git"))
        .then(|| common.parent().map(Path::to_path_buf))
        .flatten();
    Ok(main.unwrap_or_else(|| root.to_path_buf()))
}

/// The untracked local folder of the project that holds `root`.
///
/// # Errors
/// As [`shared_root`].
pub fn local_dir(root: &Path) -> Result<PathBuf, String> {
    Ok(shared_root(root)?.join(LOCAL_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_family_has_its_own_config_home_variable() {
        let vars: Vec<_> = Family::ALL.iter().map(|f| f.config_home_var()).collect();
        assert_eq!(vars, ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "GROK_HOME"]);
        for family in Family::ALL {
            assert_eq!(Family::from_id(family.id()), Some(family));
        }
        assert_eq!(Family::from_id("gemini"), None);
    }

    #[test]
    fn a_linked_worktree_shares_the_main_checkouts_local_folder() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir(&main).unwrap();
        let repo = git2::Repository::init(&main).unwrap();
        let mut index = repo.index().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@example.test").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let linked = dir.path().join("linked");
        repo.worktree("linked", &linked, None).unwrap();
        let main = std::fs::canonicalize(&main).unwrap();
        assert_eq!(
            std::fs::canonicalize(shared_root(&linked).unwrap()).unwrap(),
            main
        );
        assert_eq!(
            local_dir(&linked).unwrap(),
            shared_root(&linked).unwrap().join(LOCAL_DIR)
        );
        assert_eq!(local_dir(&main).unwrap(), main.join(LOCAL_DIR));
        // A folder that is not a repository is its own root.
        let plain = dir.path().join("plain");
        assert_eq!(shared_root(&plain).unwrap(), plain);
    }
}
