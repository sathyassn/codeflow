//! Files a repository holds that this process is refused when it reads them.
//!
//! A sandbox can refuse a read by file name, and a loose ref is a file named
//! after its branch. Git then treats that ref as broken: `bad ref`, `bad
//! object`, a worktree `HEAD` of zeros and every file staged as new. Outside
//! the sandbox the same repository is healthy, so these reports name the
//! refused read and never read it as damage. Packed refs are not files of
//! their own and are not affected.
//!
//! This asks the file system, never git's output: git's text cannot tell a
//! refused read from a ref that really is broken.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// How many names one report line lists before it counts the rest.
const SHOWN_NAMES: usize = 5;

/// What a repository scan found refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unreadable {
    /// Loose refs, or ref folders, that cannot be read: git reports these as
    /// broken.
    pub refs: Vec<String>,
    /// Reflogs that cannot be read. Only reflog reads are affected.
    pub reflogs: Vec<String>,
    /// Tracked files of this checkout that cannot be read: `git status`
    /// prints a line for each. Their names are legitimate and can stay.
    pub tracked: Vec<String>,
}

impl Unreadable {
    /// Whether git reports something here as broken, so a person has a step
    /// to take. Refused reflogs and tracked files leave git working.
    #[must_use]
    pub fn needs_action(&self) -> bool {
        !self.refs.is_empty()
    }

    /// One line per kind found, in the words `doctor` and `status` share.
    #[must_use]
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.refs.is_empty() {
            lines.push(format!(
                "unreadable ref: {}. The file exists and this process is refused when it reads it. Git reports such a ref as broken (bad ref, bad object, a HEAD of zeros, every file staged as new) and `git fetch` can fail. A sandbox or file permissions refuse the read; this is not evidence of repository damage. Next: outside the sandbox, rename a local branch with `git branch -m <old> <new>`, or run `git pack-refs --all`, which lasts until that ref next moves",
                listed(&self.refs)
            ));
        }
        if !self.reflogs.is_empty() {
            lines.push(format!(
                "unreadable reflog: {}. A sandbox or file permissions refuse the read. Only reflog reads are affected, and this is not evidence of repository damage",
                listed(&self.reflogs)
            ));
        }
        if !self.tracked.is_empty() {
            lines.push(format!(
                "unreadable tracked files: {}. A sandbox or file permissions refuse the read, so `git status` can print \"Operation not permitted\" for them and this checkout's working state can read as unavailable. This is not evidence of repository damage, and the file names can stay",
                listed(&self.tracked)
            ));
        }
        lines
    }
}

fn listed(names: &[String]) -> String {
    let shown = names
        .iter()
        .take(SHOWN_NAMES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(SHOWN_NAMES) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

/// Scan `repo`'s loose refs, its reflogs and this checkout's tracked files.
#[must_use]
pub fn scan(repo: &git2::Repository) -> Unreadable {
    scan_with(repo, &read_regular_file)
}

/// Stat `path`, then open it when it is a regular file. A sandbox refuses the
/// stat as well as the open; a mode of 000 refuses only the open. Anything
/// but a regular file is never opened, so a stray FIFO cannot block the scan.
fn read_regular_file(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_file() {
        fs::File::open(path)?;
    }
    Ok(())
}

fn scan_with(repo: &git2::Repository, read: &dyn Fn(&Path) -> io::Result<()>) -> Unreadable {
    let common = repo.commondir();
    let mut refs = Vec::new();
    walk(&common.join("refs"), "refs", read, &mut refs);
    let mut reflogs = Vec::new();
    walk(&common.join("logs/refs"), "refs", read, &mut reflogs);
    refs.sort();
    // A ref already named carries its reflog with it in the same report.
    reflogs.retain(|name| !refs.contains(name));
    reflogs.sort();
    let mut tracked = tracked_refused(repo, read);
    tracked.sort();
    Unreadable {
        refs,
        reflogs,
        tracked,
    }
}

/// Add every refused file or folder under `dir` to `out`, named from `shown`.
/// Only a refusal counts: a file that vanished mid-scan is not a finding.
fn walk(dir: &Path, shown: &str, read: &dyn Fn(&Path) -> io::Result<()>, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            if refused(&error) {
                out.push(shown.to_string());
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = format!(
            "{shown}/{}",
            crate::git::display_path(Path::new(&entry.file_name()))
        );
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            walk(&path, &name, read, out);
        } else if read(&path).as_ref().is_err_and(refused) {
            out.push(name);
        }
    }
}

fn tracked_refused(repo: &git2::Repository, read: &dyn Fn(&Path) -> io::Result<()>) -> Vec<String> {
    const FILE_KIND_MASK: u32 = 0o170_000;
    const REGULAR_FILE: u32 = 0o100_000;
    let (Some(workdir), Ok(index)) = (repo.workdir(), repo.index()) else {
        return Vec::new();
    };
    index
        .iter()
        .filter(|entry| entry.mode & FILE_KIND_MASK == REGULAR_FILE)
        .filter_map(|entry| {
            let relative = path_of(&entry.path);
            read(&workdir.join(&relative))
                .as_ref()
                .is_err_and(refused)
                .then(|| crate::git::display_path(&relative))
        })
        .collect()
}

#[cfg(unix)]
fn path_of(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// A sandbox gives EPERM and file permissions give EACCES; both are this kind.
fn refused(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository with one commit on `main`, a second branch and one
    /// tracked file, its refs and reflogs loose.
    fn repository(dir: &Path, branch: &str) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join("plain.txt"), "x\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("plain.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("t", "t@example.test").unwrap();
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "init", &tree, &[])
            .unwrap();
        repo.reference(
            &format!("refs/heads/{branch}"),
            commit,
            false,
            "branch: created",
        )
        .unwrap();
        drop(tree);
        repo
    }

    #[cfg(unix)]
    fn file_name_holds(path: &Path, word: &str) -> bool {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().contains(word))
    }

    #[test]
    fn a_healthy_repository_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-plain");
        let found = scan(&repo);
        assert_eq!(found, Unreadable::default());
        assert!(!found.needs_action());
        assert!(found.report_lines().is_empty());
    }

    /// The sandbox refuses by name with EPERM, which a test cannot ask the
    /// operating system for, so the refusal is injected here. The file
    /// permission test below uses a real one.
    #[cfg(unix)]
    #[test]
    fn a_refusal_by_name_is_named_and_is_never_called_damage() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-a-denied-store");
        let found = scan_with(&repo, &|path| {
            if file_name_holds(path, "denied") {
                Err(io::Error::from_raw_os_error(1))
            } else {
                read_regular_file(path)
            }
        });
        assert_eq!(found.refs, ["refs/heads/task/TSK-1-a-denied-store"]);
        // Its reflog is refused too, and the ref's line already covers it.
        assert!(found.reflogs.is_empty(), "{found:?}");
        assert!(found.needs_action());
        let line = found.report_lines().join("\n");
        assert!(
            line.contains("refs/heads/task/TSK-1-a-denied-store"),
            "{line}"
        );
        assert!(line.contains("not evidence of repository damage"), "{line}");
        assert!(line.contains("git pack-refs --all"), "{line}");
    }

    /// A tracked file with a denied name stays legitimate: it is named,
    /// without a step to take.
    #[cfg(unix)]
    #[test]
    fn a_refused_tracked_file_is_named_without_a_step_to_take() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-plain");
        let found = scan_with(&repo, &|path| {
            if file_name_holds(path, "plain.txt") {
                Err(io::Error::from_raw_os_error(1))
            } else {
                read_regular_file(path)
            }
        });
        assert_eq!(found.tracked, ["plain.txt"]);
        assert!(found.refs.is_empty(), "{found:?}");
        assert!(!found.needs_action());
        let line = found.report_lines().join("\n");
        assert!(line.contains("Operation not permitted"), "{line}");
        assert!(line.contains("not evidence of repository damage"), "{line}");
        assert!(line.contains("names can stay"), "{line}");
    }

    /// A reflog alone leaves git working, so it is a note and asks nothing.
    #[cfg(unix)]
    #[test]
    fn a_refused_reflog_alone_asks_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-plain");
        let logs = repo.commondir().join("logs");
        let found = scan_with(&repo, &|path| {
            if path.starts_with(&logs) && file_name_holds(path, "TSK-1-plain") {
                Err(io::Error::from_raw_os_error(1))
            } else {
                read_regular_file(path)
            }
        });
        assert_eq!(found.reflogs, ["refs/heads/task/TSK-1-plain"]);
        assert!(found.refs.is_empty(), "{found:?}");
        assert!(!found.needs_action());
    }

    /// Only a refusal is this finding. Another failure stays unreported here,
    /// as git's own error is the accurate one.
    #[cfg(unix)]
    #[test]
    fn other_read_failures_are_not_reported_as_refusals() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-plain");
        let eio = 5;
        let found = scan_with(&repo, &|_| Err(io::Error::from_raw_os_error(eio)));
        assert_eq!(found, Unreadable::default());
    }

    /// A real refusal from the operating system, with the folder refused too.
    #[cfg(unix)]
    #[test]
    fn a_real_mode_000_ref_and_ref_folder_are_named() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let repo = repository(dir.path(), "task/TSK-1-plain");
        let common = repo.commondir().to_path_buf();
        let loose = common.join("refs/heads/task/TSK-1-plain");
        let set = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        set(&loose, 0o000);
        if std::fs::File::open(&loose).is_ok() {
            // Root reads through a mode of 000; there is no refusal to observe.
            set(&loose, 0o644);
            eprintln!("skipped: this user reads through mode 000");
            return;
        }
        let found = scan(&repo);
        set(&loose, 0o644);
        assert_eq!(found.refs, ["refs/heads/task/TSK-1-plain"]);

        let folder = common.join("refs/heads/task");
        set(&folder, 0o000);
        let found = scan(&repo);
        set(&folder, 0o755);
        assert_eq!(found.refs, ["refs/heads/task"]);
    }

    #[test]
    fn a_long_list_is_counted_after_the_first_names() {
        let names: Vec<String> = (1..=7).map(|n| format!("f{n}")).collect();
        assert_eq!(listed(&names), "f1, f2, f3, f4, f5 and 2 more");
        assert_eq!(listed(&names[..2]), "f1, f2");
    }
}
