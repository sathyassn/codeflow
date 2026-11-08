//! Where `core.hooksPath` points, and the form `CodeFlow` writes (ADR-0099).
//!
//! Git reads a relative `core.hooksPath` against the directory a command
//! runs in, so `CodeFlow` writes one absolute path: the managed hooks folder
//! of the repository's main working tree, which linked worktrees share
//! through the common configuration. A repository with no main working tree
//! (a bare repository with only linked worktrees) takes the working tree
//! the writing command ran in. `codeflow update` rewrites the relative form
//! earlier releases wrote, and a path a move or a removed worktree left
//! behind; `codeflow doctor` reports both.

use std::path::Path;

use super::detect::CODEFLOW_HOOKS_PATH;
use super::{gitutil, ScaffoldError};

/// What `core.hooksPath` holds, as `CodeFlow` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HooksPathForm {
    /// Not set: git uses the common directory's `hooks/`.
    Unset,
    /// The absolute path of a working tree's managed hooks folder that
    /// holds the hooks: the form `CodeFlow` writes.
    Managed,
    /// The project-relative form earlier releases wrote.
    Relative,
    /// An absolute path to a `.codeflow/git-hooks` folder that is not a
    /// current working tree's managed hooks, or that holds no hooks (a moved
    /// or renamed repository, or a removed worktree).
    Stale(String),
    /// A path `CodeFlow` did not write (another hook manager's folder).
    Foreign(String),
}

impl HooksPathForm {
    /// Whether `CodeFlow` wrote this value, in any of its forms.
    #[must_use]
    pub fn is_codeflow(&self) -> bool {
        matches!(self, Self::Managed | Self::Relative | Self::Stale(_))
    }
}

/// Read `core.hooksPath` from `root` and classify it.
///
/// # Errors
/// Git cannot read the configuration or list the working trees.
pub fn form(root: &Path) -> Result<HooksPathForm, ScaffoldError> {
    let Some(value) = super::detect::configured_hooks_path(root)? else {
        return Ok(HooksPathForm::Unset);
    };
    classify(root, &value)
}

/// Classify a configured value read from `root`'s configuration.
///
/// # Errors
/// Git cannot list the working trees.
pub fn classify(root: &Path, value: &str) -> Result<HooksPathForm, ScaffoldError> {
    if value == CODEFLOW_HOOKS_PATH {
        return Ok(HooksPathForm::Relative);
    }
    if !names_managed_folder(value) {
        return Ok(HooksPathForm::Foreign(value.to_string()));
    }
    let configured = Path::new(value);
    for top in gitutil::working_trees(root)? {
        let managed = top.join(CODEFLOW_HOOKS_PATH);
        if same_dir(configured, &managed) && holds_hooks(&managed) {
            return Ok(HooksPathForm::Managed);
        }
    }
    Ok(HooksPathForm::Stale(value.to_string()))
}

/// An absolute path whose last two parts are `.codeflow/git-hooks`.
fn names_managed_folder(value: &str) -> bool {
    let path = Path::new(value);
    if !path.is_absolute() {
        return false;
    }
    let mut parts = path.components().rev().map(std::path::Component::as_os_str);
    parts.next().is_some_and(|last| last == "git-hooks")
        && parts.next().is_some_and(|parent| parent == ".codeflow")
}

/// Both paths name one existing directory.
fn same_dir(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// The folder holds the managed pre-commit hook as a regular file.
fn holds_hooks(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join("pre-commit")).is_ok_and(|meta| meta.is_file())
}

/// The value `CodeFlow` writes for `root`: the main working tree's managed
/// hooks when that tree holds them, else `root`'s own working tree's. The
/// path is spelled as git spells working trees, so a Windows path keeps
/// git's forward slashes.
///
/// # Errors
/// Git cannot list the working trees or name `root`'s top level.
pub fn target(root: &Path) -> Result<String, ScaffoldError> {
    if let Some(main) = gitutil::main_working_tree(root)? {
        let managed = main.join(CODEFLOW_HOOKS_PATH);
        if holds_hooks(&managed) {
            return Ok(spelled(&main));
        }
    }
    let top = gitutil::top_level(root)?;
    Ok(spelled(&top))
}

/// `<top>/.codeflow/git-hooks` with `/` between the parts git gave and the
/// managed folder.
fn spelled(top: &Path) -> String {
    let top = top.to_string_lossy();
    let top = top.trim_end_matches(['/', '\\']);
    format!("{top}/{CODEFLOW_HOOKS_PATH}")
}

/// Write the managed form when `core.hooksPath` holds a `CodeFlow` form that
/// is not it (relative or stale), or is unset and `wire_unset` holds. A
/// foreign path is left as it is. Returns the previous and the new value
/// when it wrote.
///
/// # Errors
/// Git cannot read or write the configuration.
pub fn migrate(
    root: &Path,
    wire_unset: bool,
) -> Result<Option<(Option<String>, String)>, ScaffoldError> {
    let previous = match form(root)? {
        HooksPathForm::Managed | HooksPathForm::Foreign(_) => return Ok(None),
        HooksPathForm::Unset if !wire_unset => return Ok(None),
        HooksPathForm::Unset => None,
        HooksPathForm::Relative => Some(CODEFLOW_HOOKS_PATH.to_string()),
        HooksPathForm::Stale(value) => Some(value),
    };
    let value = target(root)?;
    gitutil::config_set(root, "core.hooksPath", &value)?;
    Ok(Some((previous, value)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn project(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
        let hooks = dir.join(CODEFLOW_HOOKS_PATH);
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\n").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "chore: init"]);
    }

    fn top(dir: &Path) -> PathBuf {
        gitutil::top_level(dir).unwrap()
    }

    #[test]
    fn the_target_is_the_main_working_trees_hooks_from_every_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main project");
        std::fs::create_dir_all(&main).unwrap();
        project(&main);
        let linked = tmp.path().join("linked tree");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat/x",
                linked.to_str().unwrap(),
            ],
        );
        let expected = spelled(&top(&main));
        assert!(expected.contains("main project"), "{expected}");
        assert_eq!(target(&main).unwrap(), expected);
        assert_eq!(target(&linked).unwrap(), expected);
        gitutil::config_set(&main, "core.hooksPath", &expected).unwrap();
        assert_eq!(form(&main).unwrap(), HooksPathForm::Managed);
        assert_eq!(form(&linked).unwrap(), HooksPathForm::Managed);
    }

    #[test]
    fn a_bare_repository_takes_the_working_tree_the_command_ran_in() {
        let tmp = tempfile::tempdir().unwrap();
        let seed = tmp.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        project(&seed);
        let bare = tmp.path().join("repo.git");
        git(
            tmp.path(),
            &[
                "clone",
                "-q",
                "--bare",
                seed.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let tree = tmp.path().join("tree one");
        git(
            &bare,
            &["worktree", "add", "-q", tree.to_str().unwrap(), "main"],
        );
        assert_eq!(gitutil::main_working_tree(&tree).unwrap(), None);
        let value = target(&tree).unwrap();
        assert_eq!(value, spelled(&top(&tree)));
        gitutil::config_set(&tree, "core.hooksPath", &value).unwrap();
        assert_eq!(form(&tree).unwrap(), HooksPathForm::Managed);
    }

    #[test]
    fn forms_are_classified() {
        let tmp = tempfile::tempdir().unwrap();
        project(tmp.path());
        let root = tmp.path();
        assert_eq!(form(root).unwrap(), HooksPathForm::Unset);
        gitutil::config_set(root, "core.hooksPath", CODEFLOW_HOOKS_PATH).unwrap();
        assert_eq!(form(root).unwrap(), HooksPathForm::Relative);
        gitutil::config_set(root, "core.hooksPath", ".husky").unwrap();
        assert_eq!(form(root).unwrap(), HooksPathForm::Foreign(".husky".into()));
        let gone = tmp.path().join("moved-away").join(CODEFLOW_HOOKS_PATH);
        let gone = gone.to_string_lossy().into_owned();
        gitutil::config_set(root, "core.hooksPath", &gone).unwrap();
        assert_eq!(form(root).unwrap(), HooksPathForm::Stale(gone));
        let elsewhere = tempfile::tempdir().unwrap();
        project(elsewhere.path());
        let other = spelled(&top(elsewhere.path()));
        gitutil::config_set(root, "core.hooksPath", &other).unwrap();
        assert_eq!(form(root).unwrap(), HooksPathForm::Stale(other));
    }

    #[test]
    fn migrate_rewrites_codeflow_forms_and_leaves_a_foreign_path() {
        let tmp = tempfile::tempdir().unwrap();
        project(tmp.path());
        let root = tmp.path();
        assert_eq!(migrate(root, false).unwrap(), None);
        let expected = target(root).unwrap();
        assert_eq!(migrate(root, true).unwrap(), Some((None, expected.clone())));
        assert_eq!(migrate(root, true).unwrap(), None);
        gitutil::config_set(root, "core.hooksPath", CODEFLOW_HOOKS_PATH).unwrap();
        assert_eq!(
            migrate(root, false).unwrap(),
            Some((Some(CODEFLOW_HOOKS_PATH.into()), expected.clone()))
        );
        gitutil::config_set(root, "core.hooksPath", ".husky").unwrap();
        assert_eq!(migrate(root, true).unwrap(), None);
        assert_eq!(
            super::super::detect::configured_hooks_path(root).unwrap(),
            Some(".husky".into())
        );
    }

    #[test]
    fn only_an_absolute_managed_folder_name_is_codeflows() {
        assert!(!names_managed_folder(CODEFLOW_HOOKS_PATH));
        assert!(!names_managed_folder("/x/.husky"));
        assert!(!names_managed_folder("/x/git-hooks"));
        #[cfg(unix)]
        assert!(names_managed_folder("/x y/.codeflow/git-hooks"));
        #[cfg(windows)]
        {
            assert!(names_managed_folder("C:/x y/.codeflow/git-hooks"));
            assert!(names_managed_folder("C:\\x y\\.codeflow\\git-hooks"));
        }
    }

    #[test]
    fn the_spelling_keeps_gits_separators() {
        assert_eq!(
            spelled(Path::new("C:/Users/a b/proj/")),
            "C:/Users/a b/proj/.codeflow/git-hooks"
        );
        assert_eq!(
            spelled(Path::new("/home/a b/proj")),
            "/home/a b/proj/.codeflow/git-hooks"
        );
    }
}
