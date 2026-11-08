//! Where `core.hooksPath` points, and the form `CodeFlow` writes (ADR-0099).
//!
//! Git reads a relative `core.hooksPath` against the directory a command
//! runs in, so `CodeFlow` writes one absolute path: the managed hooks folder
//! of the repository's main working tree, which linked worktrees share
//! through the common configuration. When the main working tree holds no
//! managed hooks, or there is none (a bare repository with only linked
//! worktrees), it takes the working tree the writing command ran in. Either
//! way the command's working tree must be one git lists for the repository,
//! or nothing is written. After a write the value git reads is read back,
//! and a value a higher-precedence configuration still supplies is refused.
//! `codeflow update` rewrites the relative form earlier releases wrote, and
//! a path a move or a removed worktree left behind; `codeflow doctor`
//! reports both.

use std::path::Path;

use super::detect::CODEFLOW_HOOKS_PATH;
use super::{gitutil, ScaffoldError, Shown};

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
    // Outside a working tree no working tree can hold the managed hooks.
    if !gitutil::is_repo(root)? {
        return Ok(HooksPathForm::Stale(value.to_string()));
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
/// `root`'s working tree must be one `git worktree list` names for the
/// repository. A directory git treats as the working tree without listing
/// it (a `.git` link that names another repository's git directory, or a
/// work tree set by configuration or the environment) is refused, so the
/// value never names a folder the repository does not own.
///
/// # Errors
/// Git cannot list the working trees or name `root`'s top level, `root`'s
/// working tree is not one git lists, or the working tree's path is not
/// UTF-8 text.
pub fn target(root: &Path) -> Result<String, ScaffoldError> {
    let top = gitutil::top_level(root)?;
    let Some(current) = gitutil::working_trees(root)?
        .into_iter()
        .find(|tree| same_dir(tree, &top))
    else {
        return Err(ScaffoldError::Git(Shown::fixed(
            "core.hooksPath not written: this working tree is not one git lists for the repository (`git worktree list`), so the managed hooks folder cannot be tied to the repository; run the command from a listed working tree",
        )));
    };
    if let Some(main) = gitutil::main_working_tree(root)? {
        if holds_hooks(&main.join(CODEFLOW_HOOKS_PATH)) {
            return spelled(&main);
        }
    }
    spelled(&current)
}

/// Write the managed form for `root` and return it. The value git then
/// reads is checked: a value an included file or a per-worktree
/// configuration supplies outranks the repository's own, and the write is
/// refused as not taking effect.
///
/// # Errors
/// [`target`] refuses, git cannot write or read the configuration, or git
/// still reads another value after the write.
pub fn wire(root: &Path) -> Result<String, ScaffoldError> {
    let value = target(root)?;
    gitutil::config_set(root, "core.hooksPath", &value)?;
    if super::detect::configured_hooks_path(root)?.as_deref() != Some(value.as_str()) {
        return Err(ScaffoldError::Git(Shown::fixed(
            "core.hooksPath was written to the repository configuration, but git still reads another value from a configuration that outranks it (an included file or a per-worktree configuration); `git config --show-origin --get core.hooksPath` names the file that sets it",
        )));
    }
    Ok(value)
}

/// `<top>/.codeflow/git-hooks` with `/` between the parts git gave and the
/// managed folder. A path that is not UTF-8 text is refused, so the value
/// written is never a lossy spelling that names some other folder.
fn spelled(top: &Path) -> Result<String, ScaffoldError> {
    let top = top.to_str().ok_or_else(|| {
        ScaffoldError::Git(Shown::fixed(
            "the working tree path is not UTF-8 text, so core.hooksPath cannot name it",
        ))
    })?;
    let top = top
        .strip_suffix('/')
        .or_else(|| top.strip_suffix('\\'))
        .unwrap_or(top);
    Ok(format!("{top}/{CODEFLOW_HOOKS_PATH}"))
}

/// Write the managed form when `core.hooksPath` holds a `CodeFlow` form that
/// is not it (relative or stale), or is unset and `wire_unset` holds. A
/// foreign path is left as it is. Returns the previous and the new value
/// only when the value git reads changed.
///
/// # Errors
/// [`wire`] refuses, or git cannot read or write the configuration.
pub fn migrate(
    root: &Path,
    wire_unset: bool,
) -> Result<Option<(Option<String>, String)>, ScaffoldError> {
    // Outside a working tree there is no repository configuration to wire.
    if !gitutil::is_repo(root)? {
        return Ok(None);
    }
    let previous = match form(root)? {
        HooksPathForm::Managed | HooksPathForm::Foreign(_) => return Ok(None),
        HooksPathForm::Unset if !wire_unset => return Ok(None),
        HooksPathForm::Unset => None,
        HooksPathForm::Relative => Some(CODEFLOW_HOOKS_PATH.to_string()),
        HooksPathForm::Stale(value) => Some(value),
    };
    if previous.is_some() && previous == Some(target(root)?) {
        return Ok(None);
    }
    let value = wire(root)?;
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
        let expected = spelled(&top(&main)).unwrap();
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
        codeflow_fixture::clone(tmp.path(), &seed, &bare)
            .bare()
            .run();
        let tree = tmp.path().join("tree one");
        git(
            &bare,
            &["worktree", "add", "-q", tree.to_str().unwrap(), "main"],
        );
        assert_eq!(gitutil::main_working_tree(&tree).unwrap(), None);
        let value = target(&tree).unwrap();
        assert_eq!(value, spelled(&top(&tree)).unwrap());
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
        let other = spelled(&top(elsewhere.path())).unwrap();
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

    fn bare_project(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "chore: init"]);
    }

    #[test]
    fn a_working_tree_git_does_not_list_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        bare_project(&repo);
        // A folder whose `.git` link names the repository's git directory.
        let other = tmp.path().join("other");
        let hooks = other.join(CODEFLOW_HOOKS_PATH);
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\n").unwrap();
        let link = format!("gitdir: {}\n", repo.join(".git").display());
        std::fs::write(other.join(".git"), link).unwrap();
        assert!(target(&other).is_err());
        assert!(migrate(&other, true).is_err());
        assert_eq!(
            super::super::detect::configured_hooks_path(&repo).unwrap(),
            None
        );
        // The repository's own working tree still takes its own folder.
        assert_eq!(target(&repo).unwrap(), spelled(&top(&repo)).unwrap());
    }

    #[test]
    fn a_work_tree_set_by_configuration_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        bare_project(&repo);
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        git(&repo, &["config", "core.worktree", other.to_str().unwrap()]);
        // Git now treats `other` as the working tree, which it does not list.
        assert!(target(&repo).is_err());
        assert_eq!(migrate(&repo, true).ok().flatten(), None);
        assert_eq!(
            super::super::detect::configured_hooks_path(&repo.join(".git")).unwrap(),
            None
        );
    }

    #[test]
    fn a_git_directory_kept_apart_from_its_working_tree_is_refused() {
        // Git lists such a repository's git directory as its main working
        // tree, so the working tree cannot be tied to it and nothing is
        // written.
        let tmp = tempfile::tempdir().unwrap();
        let tree = tmp.path().join("tree");
        let store = tmp.path().join("store.git");
        git(
            tmp.path(),
            &[
                "init",
                "-q",
                "-b",
                "main",
                "--separate-git-dir",
                store.to_str().unwrap(),
                tree.to_str().unwrap(),
            ],
        );
        git(
            &tree,
            &["commit", "-q", "--allow-empty", "-m", "chore: init"],
        );
        assert!(target(&tree).is_err());
        assert!(migrate(&tree, true).is_err());
        assert_eq!(
            super::super::detect::configured_hooks_path(&tree).unwrap(),
            None
        );
    }

    #[test]
    fn a_value_an_include_still_supplies_is_refused_not_reported() {
        let tmp = tempfile::tempdir().unwrap();
        project(tmp.path());
        let root = tmp.path();
        gitutil::config_set(root, "core.hooksPath", CODEFLOW_HOOKS_PATH).unwrap();
        let include = tmp.path().join("extra.config");
        std::fs::write(
            &include,
            format!("[core]\n\thooksPath = {CODEFLOW_HOOKS_PATH}\n"),
        )
        .unwrap();
        git(root, &["config", "include.path", include.to_str().unwrap()]);
        assert!(migrate(root, false).is_err());
        assert_eq!(
            super::super::detect::configured_hooks_path(root).unwrap(),
            Some(CODEFLOW_HOOKS_PATH.into())
        );
    }

    #[test]
    fn an_unchanged_value_is_not_reported_as_a_move() {
        let tmp = tempfile::tempdir().unwrap();
        bare_project(tmp.path());
        let root = tmp.path();
        // The folder is missing, so the value reads as stale, and the value
        // `CodeFlow` would write is the same one.
        let value = target(root).unwrap();
        gitutil::config_set(root, "core.hooksPath", &value).unwrap();
        assert_eq!(form(root).unwrap(), HooksPathForm::Stale(value.clone()));
        assert_eq!(migrate(root, false).unwrap(), None);
    }

    #[test]
    fn migrate_outside_a_repository_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(migrate(tmp.path(), true).unwrap(), None);
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
            spelled(Path::new("C:/Users/a b/proj/")).unwrap(),
            "C:/Users/a b/proj/.codeflow/git-hooks"
        );
        assert_eq!(
            spelled(Path::new("/home/a b/proj")).unwrap(),
            "/home/a b/proj/.codeflow/git-hooks"
        );
    }
}
