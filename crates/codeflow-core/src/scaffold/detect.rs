//! Environment detection: stack, existing git-hook managers, empty dirs.

use std::path::{Path, PathBuf};

use super::{gitutil, ScaffoldError};

/// Detected project stack, used for `{{STACK}}` and recorded in project.toml.
///
/// # Errors
///
/// Returns an error when a marker file's presence cannot be read, a
/// dangling link included: a marker that cannot be read never lets a later
/// marker choose the stack.
pub fn detect_stack(root: &Path) -> Result<&'static str, ScaffoldError> {
    for (marker, stack) in [
        ("Cargo.toml", "rust"),
        ("package.json", "node"),
        ("pyproject.toml", "python"),
    ] {
        if super::path_exists(&root.join(marker))? {
            return Ok(stack);
        }
    }
    Ok("unset")
}

/// A pre-existing git-hook manager codeflow must not clobber (charter AC #2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookManager {
    Husky,
    Lefthook,
    /// `core.hooksPath` already points somewhere that is not ours (a storage
    /// key; show it through `Display`).
    HooksPath(String),
}

impl std::fmt::Display for HookManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Husky => f.write_str("husky"),
            Self::Lefthook => f.write_str("lefthook"),
            Self::HooksPath(path) => {
                write!(f, "core.hooksPath={}", crate::git::display_key(path))
            }
        }
    }
}

/// Path codeflow wires hooks into via `core.hooksPath`.
pub const CODEFLOW_HOOKS_PATH: &str = ".codeflow/git-hooks";

/// Raw `core.hooksPath` as git stores it (relative or absolute) as a storage
/// key: a value that is not valid UTF-8 is kept, as a key that never equals
/// [`CODEFLOW_HOOKS_PATH`] (OS text rule, issue 79). `None` if unset.
/// # Errors
/// Git cannot read the effective configuration.
pub fn configured_hooks_path(root: &Path) -> Result<Option<String>, ScaffoldError> {
    gitutil::config_get_key(root, "core.hooksPath")
}

/// Detects an existing hook manager that owns this repo's hooks.
/// # Errors
/// Git cannot read the effective hook configuration.
pub fn detect_hook_manager(root: &Path) -> Result<Option<HookManager>, ScaffoldError> {
    let configured = configured_hooks_path(root)?;
    if super::path_exists(&root.join(".husky"))?
        && std::fs::metadata(root.join(".husky"))
            .map_err(|error| ScaffoldError::io(root.join(".husky"), error))?
            .is_dir()
    {
        return Ok(Some(HookManager::Husky));
    }
    for f in [
        "lefthook.yml",
        ".lefthook.yml",
        "lefthook.toml",
        ".lefthook.toml",
    ] {
        if super::path_exists(&root.join(f))? {
            return Ok(Some(HookManager::Lefthook));
        }
    }
    if let Some(path) = configured {
        if path != CODEFLOW_HOOKS_PATH {
            return Ok(Some(HookManager::HooksPath(path)));
        }
    }
    Ok(None)
}

/// Hook scripts in the repository's own hooks folder (`hooks/` in the
/// common git dir, which linked worktrees share), such as those `pre-commit
/// install` writes or a person wrote by hand. Git runs them only while
/// `core.hooksPath` is unset, so wiring codeflow's shims stops them
/// silently; init, update and doctor name them as a brownfield choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDirHooks {
    /// The hooks folder, as a path from `root` when it lies under it.
    pub dir: String,
    /// The hook names, sorted.
    pub names: Vec<String>,
}

impl GitDirHooks {
    /// The setup report line: what stops running, and the adopter's
    /// options. Nothing is moved or deleted.
    #[must_use]
    pub fn report_note(&self) -> String {
        format!(
            "existing git hooks in {dir}: {names}. git no longer runs them once CodeFlow's hooks are wired (core.hooksPath = {CODEFLOW_HOOKS_PATH}); nothing was moved or deleted. For each, choose one: (1) move its check into the project's CI or a supported hook manager (husky, lefthook), then delete the file; (2) keep it: set core.hooksPath to a project-relative hooks folder the project owns, whose hooks call both the check and the CodeFlow shim. `codeflow doctor` warns while they stay in {dir}",
            dir = self.dir,
            names = self.names.join(", "),
        )
    }
}

/// The hook events git runs from its hooks folder (githooks(5)). A file under
/// any other name, such as git's `*.sample` files or a helper script, is
/// never run by git as a hook.
const GIT_HOOK_NAMES: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// The executable hook files in the common git dir's `hooks/`, skipping
/// files whose name is not a git hook event (git's `*.sample` files, helper
/// scripts) and anything git would not run. `None` when there
/// are none, outside a repository, or when `core.hooksPath` already points
/// at another manager's folder (git was not running these files anyway, and
/// that manager is reported instead).
#[must_use]
pub fn git_dir_hooks(root: &Path) -> Option<GitDirHooks> {
    // This function only supplies an advisory note; it never selects wiring.
    if configured_hooks_path(root)
        .ok()?
        .is_some_and(|path| path != CODEFLOW_HOOKS_PATH)
    {
        return None;
    }
    let dir = gitutil::common_dir(root).ok()??.join("hooks");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_file() && is_executable(&entry.path()))
        // Hook names are ASCII; a name that is not valid UTF-8 is none of them.
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| GIT_HOOK_NAMES.contains(&name.as_str()))
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();
    Some(GitDirHooks {
        dir: display_from(root, &dir),
        names,
    })
}

/// `path` as a path from `root` with `/` separators when it lies under it,
/// else as given.
fn display_from(root: &Path, path: &Path) -> String {
    let canonical =
        |p: &Path| crate::portable_path::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (root, path): (PathBuf, PathBuf) = (canonical(root), canonical(path));
    match path.strip_prefix(&root) {
        // A report line: the key of a name that is not text is shown as escapes.
        Ok(relative) => crate::git::display_key(&crate::portable_path::slashed(relative)),
        Err(_) => path.display().to_string(),
    }
}

/// Whether git would run `path` as a hook.
#[cfg(unix)]
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

/// Git for Windows runs a hook without an executable bit.
#[cfg(not(unix))]
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// True when the directory contains nothing (ignoring nothing — a truly
/// empty dir, the canonical bootstrap-grace case).
#[must_use]
pub fn is_empty_dir(root: &Path) -> bool {
    std::fs::read_dir(root).map_or(true, |mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn r22_hook_manager_refuses_unreadable_candidates() {
        for name in [
            ".husky",
            "lefthook.yml",
            ".lefthook.yml",
            "lefthook.toml",
            ".lefthook.toml",
        ] {
            let dir = tempfile::tempdir().unwrap();
            assert_eq!(detect_hook_manager(dir.path()).unwrap(), None);
            std::os::unix::fs::symlink("missing", dir.path().join(name)).unwrap();
            assert!(detect_hook_manager(dir.path()).is_err(), "{name}");
        }
    }

    /// A marker that cannot be read refuses detection; a later marker never
    /// chooses the stack in its place.
    #[cfg(unix)]
    #[test]
    fn r23_a_dangling_stack_marker_refuses_detection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "node");
        std::os::unix::fs::symlink("gone", dir.path().join("Cargo.toml")).unwrap();
        assert!(detect_stack(dir.path()).is_err());
        std::fs::write(dir.path().join("real.toml"), "").unwrap();
        std::fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
        std::os::unix::fs::symlink("real.toml", dir.path().join("Cargo.toml")).unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "rust");
    }

    #[test]
    fn stack_detection() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "unset");
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "python");
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "node");
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_stack(dir.path()).unwrap(), "rust");
    }

    #[test]
    fn husky_detected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".husky")).unwrap();
        assert_eq!(
            detect_hook_manager(dir.path()).unwrap(),
            Some(HookManager::Husky)
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn hook(dir: &Path, name: &str, executable: bool) {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\ntrue\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = executable;
    }

    #[test]
    fn git_dir_hooks_name_executable_hooks_and_resolve_the_common_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let hooks = main.join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for entry in std::fs::read_dir(&hooks).unwrap().flatten() {
            std::fs::remove_file(entry.path()).unwrap();
        }
        assert_eq!(git_dir_hooks(&main), None, "an empty hooks folder");

        hook(&hooks, "pre-push.sample", true);
        hook(&hooks, "pre-commit", true);
        hook(&hooks, "commit-msg", true);
        #[cfg(unix)]
        hook(&hooks, "post-merge", false);
        let found = git_dir_hooks(&main).unwrap();
        assert_eq!(found.names, ["commit-msg", "pre-commit"]);
        assert_eq!(found.dir, ".git/hooks");
        let note = found.report_note();
        assert!(note.contains("commit-msg, pre-commit") && note.contains("git no longer runs"));

        // A linked worktree resolves the main checkout's hooks folder.
        git(&main, &["worktree", "add", "-q", "wt", "-b", "chore/wt"]);
        let linked = git_dir_hooks(&main.join("wt")).unwrap();
        assert_eq!(linked.names, ["commit-msg", "pre-commit"]);
        assert!(
            Path::new(&linked.dir).ends_with("main/.git/hooks"),
            "{}",
            linked.dir
        );

        // codeflow's own wiring keeps reporting them; another manager's
        // folder means git was not running them anyway.
        git(&main, &["config", "core.hooksPath", CODEFLOW_HOOKS_PATH]);
        assert!(git_dir_hooks(&main).is_some());
        git(&main, &["config", "core.hooksPath", ".husky"]);
        assert_eq!(git_dir_hooks(&main), None);
    }

    /// Round seven on issue 79: a quoted hooks path that ends in a carriage
    /// return is not the shipped hooks path; it is another owner's path.
    #[test]
    fn a_hooks_path_with_a_trailing_carriage_return_is_not_ours() {
        use std::io::Write as _;
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(tmp.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(format!("[core]\n\thooksPath = \"{CODEFLOW_HOOKS_PATH}\r\"\n").as_bytes())
            .unwrap();
        assert!(matches!(
            detect_hook_manager(tmp.path()).unwrap(),
            Some(HookManager::HooksPath(_))
        ));
    }

    /// Issue 79: a `core.hooksPath` that is not valid UTF-8 is an existing
    /// hook setup, so detection reports it and init does not overwrite it.
    #[test]
    fn a_hooks_path_that_is_not_utf8_is_an_existing_manager() {
        use std::io::Write as _;
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(tmp.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[core]\n\thooksPath = hooks-\xff\n")
            .unwrap();
        let found = detect_hook_manager(tmp.path()).unwrap().expect("a manager");
        assert!(matches!(found, HookManager::HooksPath(_)), "{found:?}");
        assert_eq!(found.to_string(), "core.hooksPath=hooks-\\xff");
        assert_eq!(git_dir_hooks(tmp.path()), None);
    }

    #[test]
    fn git_dir_hooks_outside_a_repository_are_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(git_dir_hooks(dir.path()), None);
    }

    #[test]
    fn empty_dir_detection() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_empty_dir(dir.path()));
        std::fs::write(dir.path().join("x"), "").unwrap();
        assert!(!is_empty_dir(dir.path()));
    }
}
