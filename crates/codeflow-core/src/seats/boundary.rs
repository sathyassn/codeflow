//! Where a config home may sit (TSK-431, security review R1).
//!
//! A profile names a folder a CLI will fill with sign-in files, so the folder
//! must not be, or lead into, files a commit can carry. The check works on the
//! resolved location, not the spelling: every existing ancestor is resolved
//! through links first, and a home that reaches the project through a link, or
//! looks like it sits in the project but resolves elsewhere, is refused.
//!
//! Allowed: any folder outside the project, its linked worktrees and the
//! worktrees' own folders; or a folder under the main checkout's untracked
//! `.codeflow/local/`, spelled without links, while nothing under that folder
//! is tracked. The check opens no file inside the home.

use std::path::{Path, PathBuf};

use super::{open_repo, shared_root, LOCAL_DIR};
use crate::absence::proven_absent;

/// `path` with every existing ancestor resolved through links, and the
/// rest (not yet created) appended. The part that exists must be a folder.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = || crate::git::display_path(path);
    let mut existing = path.to_path_buf();
    let mut tail = Vec::new();
    while proven_absent(&existing).map_err(|e| format!("{}: cannot be examined ({e})", shown()))? {
        let name = existing
            .file_name()
            .ok_or_else(|| format!("{}: has no existing folder above it", shown()))?
            .to_os_string();
        tail.push(name);
        existing.pop();
    }
    let canonical = existing.canonicalize().map_err(|e| {
        format!(
            "{}: cannot be resolved ({e}); a dangling link is refused",
            shown()
        )
    })?;
    if crate::absence::is_kind(&canonical, true) != Some(true) {
        return Err(format!(
            "{}: the existing part of the path is not a folder",
            shown()
        ));
    }
    let mut resolved = canonical;
    resolved.extend(tail.into_iter().rev());
    Ok(resolved)
}

/// Every folder whose files a commit of this project can carry: the main
/// checkout, each linked worktree and the checkout the command runs in.
fn project_regions(project_root: &Path, main: &Path) -> Result<Vec<PathBuf>, String> {
    let mut regions = vec![main.to_path_buf()];
    regions
        .push(resolve(project_root).map_err(|reason| {
            format!("cannot place the checkout the command runs in: {reason}")
        })?);
    if let Some(repo) = open_repo(main)? {
        let names = repo
            .worktrees()
            .map_err(|e| format!("cannot list the linked worktrees: {}", e.message()))?;
        for name in crate::git::name::names_of(&names) {
            let text = name
                .rule_text()
                .map_err(|_| "a linked worktree has a name that is not text".to_string())?;
            let worktree = repo
                .find_worktree(text)
                .map_err(|e| format!("cannot read a linked worktree: {}", e.message()))?;
            // A folder that is gone resolves to its place (it holds no files);
            // any other failure to place it stops the check, never skips it.
            regions.push(resolve(worktree.path()).map_err(|reason| {
                format!("cannot place a linked worktree of this repository: {reason}")
            })?);
        }
    }
    Ok(regions)
}

/// Refuse a local folder that holds tracked files: ignoring a folder does not
/// untrack what an index already lists.
fn refuse_tracked_local(main: &Path, project_root: &Path) -> Result<(), String> {
    let prefix = format!("{LOCAL_DIR}/");
    for root in [main, project_root] {
        let Some(repo) = open_repo(root)? else {
            continue;
        };
        // An unreadable index cannot show what is tracked, so the local
        // exception stays closed (fail closed), never skipped.
        let index = repo.index().map_err(|e| {
            format!(
                "cannot read the repository index ({}); a home under {LOCAL_DIR}/ cannot be \
                 shown safe from commits. Repair the index or use a folder outside the project",
                e.message()
            )
        })?;
        if index
            .iter()
            .any(|entry| entry.path.starts_with(prefix.as_bytes()))
        {
            return Err(format!(
                "files under {LOCAL_DIR}/ are tracked by this repository; a home there could be \
                 committed. Untrack them, or use a folder outside the project"
            ));
        }
    }
    Ok(())
}

/// Refuse a config home that is, leads into or could be committed with the
/// project. `spelled` is the absolute path from the profile.
///
/// # Errors
/// The home resolves inside the project or one of its linked worktrees
/// outside `.codeflow/local/`, reaches the project through a link, is spelled
/// inside the project but resolves elsewhere, is not a folder, or sits under a
/// local folder that holds tracked files.
pub fn check_config_home(spelled: &Path, project_root: &Path) -> Result<(), String> {
    let shown = crate::git::display_path(spelled);
    let resolved = resolve(spelled).map_err(|reason| format!("config_home {reason}"))?;
    let main_spelled = shared_root(project_root)?;
    let main = resolve(&main_spelled)?;
    // The part of the spelling below the deepest ancestor that is the main
    // checkout, whatever way that ancestor is written (a symlinked temp or
    // volume folder included).
    let relative = spelled.ancestors().skip(1).find_map(|ancestor| {
        let is_main = resolve(ancestor).is_ok_and(|resolved| resolved == main);
        is_main
            .then(|| spelled.strip_prefix(ancestor).ok())
            .flatten()
    });
    if let Some(relative) = relative {
        if resolved != main.join(relative) {
            return Err(format!(
                "config_home `{shown}` is spelled inside the project but resolves elsewhere \
                 through a link; name the real folder"
            ));
        }
    }
    let local = main.join(LOCAL_DIR);
    let regions = project_regions(project_root, &main)?;
    if regions.iter().any(|region| resolved.starts_with(region)) {
        let allowed = relative.is_some() && resolved.starts_with(&local);
        if !allowed {
            return Err(format!(
                "config_home `{shown}` is inside the project or one of its worktrees (or \
                 reaches it through a link); a sign-in stored there could be committed. Use a \
                 folder outside the project, or one under {LOCAL_DIR}/"
            ));
        }
    }
    if resolved.starts_with(&local) {
        refuse_tracked_local(&main, project_root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Only the symlink tests are Unix-only; creating a link needs a privilege
    // on Windows. Everything else here runs on every platform.
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    struct Fixture {
        _dir: tempfile::TempDir,
        main: PathBuf,
        outside: PathBuf,
        repo: git2::Repository,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let main = base.join("project");
        let outside = base.join("outside");
        std::fs::create_dir_all(main.join("tracked")).unwrap();
        std::fs::create_dir_all(main.join(LOCAL_DIR)).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let repo = git2::Repository::init(&main).unwrap();
        std::fs::write(main.join("tracked/file"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked/file")).unwrap();
        {
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = git2::Signature::now("t", "t@example.test").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        Fixture {
            _dir: dir,
            main,
            outside,
            repo,
        }
    }

    fn refused(f: &Fixture, path: &Path) -> String {
        check_config_home(path, &f.main).unwrap_err()
    }

    #[test]
    fn a_folder_outside_the_project_and_one_under_local_are_allowed() {
        let f = fixture();
        assert!(check_config_home(&f.outside.join("codex"), &f.main).is_ok());
        assert!(check_config_home(&f.main.join(LOCAL_DIR).join("codex"), &f.main).is_ok());
        std::fs::create_dir(f.outside.join("exists")).unwrap();
        assert!(check_config_home(&f.outside.join("exists"), &f.main).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_from_outside_into_the_project_is_refused() {
        let f = fixture();
        symlink(f.main.join("tracked"), f.outside.join("link")).unwrap();
        assert!(refused(&f, &f.outside.join("link")).contains("inside the project"));
        // A not-yet-created home below the link resolves into the project too.
        assert!(refused(&f, &f.outside.join("link/new")).contains("inside the project"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_local_is_refused_in_both_directions() {
        let f = fixture();
        let local = f.main.join(LOCAL_DIR);
        symlink(f.main.join("tracked"), local.join("into")).unwrap();
        assert!(refused(&f, &local.join("into")).contains("project"));
        symlink(&f.outside, local.join("away")).unwrap();
        assert!(refused(&f, &local.join("away")).contains("resolves elsewhere"));
        assert!(refused(&f, &local.join("away/deeper")).contains("resolves elsewhere"));
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_refused() {
        let f = fixture();
        symlink(f.outside.join("missing"), f.outside.join("dangling")).unwrap();
        assert!(refused(&f, &f.outside.join("dangling")).contains("dangling"));
    }

    #[test]
    fn a_regular_file_is_refused_as_not_a_folder() {
        let f = fixture();
        std::fs::write(f.outside.join("file"), "x").unwrap();
        assert!(refused(&f, &f.outside.join("file")).contains("not a folder"));
        // Below a regular file nothing can be a folder, however it is reported.
        refused(&f, &f.outside.join("file/below"));
    }

    #[test]
    fn a_linked_worktree_outside_the_main_checkout_is_protected() {
        let f = fixture();
        let linked = f.outside.join("linked");
        f.repo.worktree("linked", &linked, None).unwrap();
        assert!(refused(&f, &linked.join("home")).contains("inside the project"));
        // The same holds when the command runs from the linked worktree.
        assert!(check_config_home(&linked.join("home"), &linked).is_err());
        assert!(check_config_home(&f.outside.join("codex"), &linked).is_ok());
    }

    #[test]
    fn tracked_files_under_local_close_the_local_exception() {
        let f = fixture();
        let local = f.main.join(LOCAL_DIR);
        assert!(check_config_home(&local.join("codex"), &f.main).is_ok());
        std::fs::write(local.join("kept"), "x").unwrap();
        let mut index = f.repo.index().unwrap();
        index
            .add_path(Path::new(&format!("{LOCAL_DIR}/kept")))
            .unwrap();
        index.write().unwrap();
        assert!(refused(&f, &local.join("codex")).contains("tracked"));
        // A home outside the project is unaffected.
        assert!(check_config_home(&f.outside.join("codex"), &f.main).is_ok());
    }

    #[test]
    fn an_unreadable_index_closes_the_local_exception() {
        let f = fixture();
        let local = f.main.join(LOCAL_DIR);
        std::fs::write(local.join("kept"), "x").unwrap();
        let mut index = f.repo.index().unwrap();
        index
            .add_path(Path::new(&format!("{LOCAL_DIR}/kept")))
            .unwrap();
        index.write().unwrap();
        assert!(refused(&f, &local.join("codex")).contains("tracked"));
        // The same index, damaged: the tracked file can no longer be seen, and
        // the home under the local folder must not be allowed because of it.
        std::fs::write(f.repo.path().join("index"), b"not an index").unwrap();
        let reason = refused(&f, &local.join("codex"));
        assert!(
            reason.contains("cannot read the repository index"),
            "{reason}"
        );
        // A home outside the project never needed the index.
        assert!(check_config_home(&f.outside.join("codex"), &f.main).is_ok());
    }

    #[test]
    fn a_worktree_that_cannot_be_placed_stops_the_check() {
        let f = fixture();
        let linked = f.outside.join("linked");
        f.repo.worktree("linked", &linked, None).unwrap();
        // Replace the worktree folder by a regular file: it can neither be
        // read as a folder nor be shown absent, so the check refuses outright
        // rather than skipping it.
        std::fs::remove_dir_all(&linked).unwrap();
        std::fs::write(&linked, "x").unwrap();
        let reason = refused(&f, &f.outside.join("codex"));
        assert!(
            reason.contains("cannot place a linked worktree"),
            "{reason}"
        );
    }
}
