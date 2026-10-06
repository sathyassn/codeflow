//! Git operations: merge conflict detection, branch analysis, CI polling,
//! and the one constructor for every `git` process codeflow starts.
//!
//! Uses `git2` for native git operations where it can; CI polling shells to
//! the `gh` CLI.

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod ci;
pub mod conflict;
pub mod name;
pub mod remote_query;
pub(crate) mod stdin;

pub use name::{
    diff_paths, display_key, gitfile_dir, key_is_text, tracking_branch, walk_tree, DisplayName,
    GitName, NotRepresentable, NotUtf8, Walk,
};

/// The variable a codeflow git-hook shim reads to run the codeflow binary
/// whose command started git, instead of the `codeflow` first on PATH
/// (SPC-013 R-85, amended). It is a dispatch hint, not authentication.
pub const HOOK_BINARY_ENV: &str = "CODEFLOW_HOOK_BINARY";

static CALLING_BINARY: OnceLock<PathBuf> = OnceLock::new();

/// Record `binary` as the codeflow binary this process is, so the git
/// processes it starts dispatch their hooks to it. The CLI calls this once
/// at startup with its own runtime path; a library caller that never does
/// keeps the shims' PATH lookup.
pub fn designate_calling_binary(binary: PathBuf) {
    let _ = CALLING_BINARY.set(binary);
}

/// A `git` process, with [`HOOK_BINARY_ENV`] set in its environment only,
/// overwriting any inherited value, when this process designated itself.
/// Every `git` codeflow starts is built here, so a hook git fires during a
/// codeflow command runs that same binary.
#[must_use]
pub fn command() -> std::process::Command {
    process("git")
}

/// A process for `program`. When `program` runs git (its last path part,
/// without an extension, is `git`), it is built as [`command`] builds git,
/// so a caller that holds the program name in a variable still gets the
/// hook dispatch; any other program is a plain `Command`.
#[must_use]
pub fn process(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let program = program.as_ref();
    let mut process = std::process::Command::new(program);
    if runs_git(program) {
        if let Some(binary) = CALLING_BINARY.get() {
            process.env(HOOK_BINARY_ENV, binary);
        }
    }
    process
}

fn runs_git(program: &std::ffi::OsStr) -> bool {
    // Bytes, not text (OS text rule, issue 79): only the ASCII stem is tested.
    let bytes = program.as_encoded_bytes();
    let name = bytes
        .rsplit(|byte| matches!(byte, b'/' | b'\\'))
        .next()
        .unwrap_or_default();
    name.split(|byte| *byte == b'.')
        .next()
        .is_some_and(|stem| stem.eq_ignore_ascii_case(b"git"))
}

/// A linked worktree as git lists it.
#[derive(Debug, Clone)]
pub struct LinkedWorktree {
    /// The administrative folder name, exact.
    pub name: GitName,
    /// The checkout, as git records it in the worktree's `gitdir` file.
    pub path: PathBuf,
    /// The administrative folder, `<common dir>/worktrees/<name>`.
    pub admin: PathBuf,
}

/// Every linked worktree of `repo`, whatever its folder name holds.
///
/// OS text rule (issue 79, `docs/architecture.md`): a worktree whose
/// administrative folder name is not valid UTF-8 cannot be asked for by
/// `find_worktree`, and dropping it from a list would hide a checkout from a
/// guard or from cleanup. Its checkout is read from its `gitdir` file, which
/// is what `find_worktree` reads too. A folder the platform cannot hold as a
/// path refuses the inventory instead of disappearing from protection.
/// # Errors
/// Any registered worktree or its administrative path cannot be read.
pub fn linked_worktrees(repo: &git2::Repository) -> Result<Vec<LinkedWorktree>, String> {
    let names = repo
        .worktrees()
        .map_err(|error| format!("cannot read registered worktrees: {error}"))?;
    let mut out = Vec::new();
    for name in name::names_of(&names) {
        let folder = name.os_path().map_err(|error| error.to_string())?;
        let admin = repo.commondir().join("worktrees").join(folder);
        let path = gitdir_checkout(&admin)?;
        out.push(LinkedWorktree { name, path, admin });
    }
    Ok(out)
}

/// Git writes exactly one LF after the administrative checkout path.
fn gitdir_checkout(admin: &std::path::Path) -> Result<PathBuf, String> {
    let text = std::fs::read(admin.join("gitdir"))
        .map_err(|error| format!("cannot read worktree gitdir {}: {error}", admin.display()))?;
    let bytes = text.strip_suffix(b"\n").unwrap_or(&text);
    let gitdir = GitName::from_bytes(bytes)
        .os_path()
        .map_err(|error| error.to_string())?;
    gitdir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "cannot read worktree gitdir {}: no checkout parent",
                admin.display()
            )
        })
}

/// Every checkout and its exact symbolic HEAD; only a detached HEAD is None.
/// # Errors
/// Checkout registration or HEAD bytes cannot be read.
pub fn checkout_heads(repo: &git2::Repository) -> Result<Vec<(PathBuf, Option<GitName>)>, String> {
    let head_of = |git_dir: &std::path::Path| -> Result<Option<GitName>, String> {
        let text = std::fs::read(git_dir.join("HEAD"))
            .map_err(|error| format!("cannot read HEAD {}: {error}", git_dir.display()))?;
        let bytes = text.strip_suffix(b"\n").unwrap_or(&text);
        if let Some(name) = bytes.strip_prefix(b"ref: ") {
            if name.is_empty() {
                return Err("cannot read HEAD: empty reference".into());
            }
            Ok(Some(GitName::from_bytes(name)))
        } else if matches!(bytes.len(), 40 | 64) && bytes.iter().all(u8::is_ascii_hexdigit) {
            Ok(None)
        } else {
            Err(format!(
                "cannot read HEAD {}: malformed record",
                git_dir.display()
            ))
        }
    };
    let mut out = Vec::new();
    let main = git2::Repository::open(repo.commondir())
        .map_err(|error| format!("cannot read main checkout: {error}"))?;
    if let Some(workdir) = main.workdir() {
        out.push((workdir.to_path_buf(), head_of(repo.commondir())?));
    }
    for worktree in linked_worktrees(repo)? {
        let head = head_of(&worktree.admin)?;
        out.push((worktree.path, head));
    }
    Ok(out)
}

/// Writes `packed-refs` for a test repository with reference names that a
/// file system may refuse as file names, such as names that are not valid
/// UTF-8. Each entry is an object id and the full name as bytes.
#[cfg(test)]
pub(crate) fn write_packed_refs(git_dir: &std::path::Path, refs: &[(String, Vec<u8>)]) {
    let mut packed = b"# pack-refs with: peeled fully-peeled sorted \n".to_vec();
    for (oid, name) in refs {
        packed.extend_from_slice(oid.as_bytes());
        packed.push(b' ');
        packed.extend_from_slice(name);
        packed.push(b'\n');
    }
    std::fs::write(git_dir.join("packed-refs"), packed).unwrap();
}

/// A repository with one empty commit on `HEAD` and the extra references
/// `names` (full reference names as bytes) pointing at it, written to
/// `packed-refs` so a file system that refuses such file names still holds
/// them. Returns the reopened repository.
/// A repository whose one commit holds `files` (a path as exact bytes, its
/// content), written through the index so a path need not be valid UTF-8 and
/// need not exist on this file system. The commit is `HEAD` of branch `main`.
#[cfg(test)]
pub(crate) fn repo_with_tree(
    dir: &std::path::Path,
    files: &[(&[u8], &[u8])],
) -> (git2::Repository, git2::Oid) {
    let repo = git2::Repository::init(dir).unwrap();
    let commit = add_commit(&repo, files);
    (repo, commit)
}

/// A commit on `main` on top of `HEAD` (if any) that adds or changes `files`
/// through the index, as [`repo_with_tree`] does.
#[cfg(test)]
pub(crate) fn add_commit(repo: &git2::Repository, files: &[(&[u8], &[u8])]) -> git2::Oid {
    let modes: Vec<(&[u8], &[u8], u32)> = files
        .iter()
        .map(|(path, content)| (*path, *content, 0o100_644))
        .collect();
    add_commit_modes(repo, &modes)
}

/// [`add_commit`] with each entry's file mode (`0o120_000` is a symlink).
#[cfg(test)]
pub(crate) fn add_commit_modes(
    repo: &git2::Repository,
    files: &[(&[u8], &[u8], u32)],
) -> git2::Oid {
    let mut index = repo.index().unwrap();
    for (path, content, mode) in files {
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: git2::Oid::ZERO_SHA1,
            flags: 0,
            flags_extended: 0,
            path: path.to_vec(),
        };
        index.add_frombuffer(&entry, content).unwrap();
    }
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let who = git2::Signature::now("Test", "test@example.com").unwrap();
    let parent = repo
        .find_reference("refs/heads/main")
        .ok()
        .and_then(|reference| reference.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    let commit = repo
        .commit(
            Some("refs/heads/main"),
            &who,
            &who,
            "files",
            &tree,
            &parents,
        )
        .unwrap();
    drop(tree);
    commit
}

#[cfg(test)]
pub(crate) fn repo_with_refs(dir: &std::path::Path, names: &[&[u8]]) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let tree = repo
        .find_tree(repo.index().unwrap().write_tree().unwrap())
        .unwrap();
    let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
    let commit = repo
        .commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
        .unwrap();
    drop(tree);
    let refs: Vec<(String, Vec<u8>)> = names
        .iter()
        .map(|name| (commit.to_string(), name.to_vec()))
        .collect();
    write_packed_refs(&dir.join(".git"), &refs);
    drop(repo);
    git2::Repository::open(dir).unwrap()
}

pub use ci::{wait_for_ci_green, CiOutcome, CiWaitConfig, CiWaitError};
pub use conflict::{attempt_rebase, check_merge_conflicts, ConflictResult, RebaseResult};
pub use stdin::{output_with_input, spawn_with_input, spawn_with_input_stopping, InputWriter};

#[cfg(test)]
mod tests {
    use super::{output_with_input, runs_git};
    use std::ffi::OsStr;

    #[test]
    fn a_program_runs_git_by_its_last_path_part() {
        for program in [
            "git",
            "GIT",
            "git.exe",
            "/usr/bin/git",
            r"C:\Git\cmd\git.exe",
        ] {
            assert!(runs_git(OsStr::new(program)), "{program}");
        }
        for program in ["gh", "gitleaks", "git-lfs", "/opt/git/bin/sh", "digit"] {
            assert!(!runs_git(OsStr::new(program)), "{program}");
        }
    }

    /// Review finding on issue 79: a linked worktree whose folder name is not
    /// valid UTF-8 stays in the list, with its checkout, instead of vanishing
    /// from guards and cleanup. Runs where the file system accepts the name,
    /// as on Linux.
    #[cfg(unix)]
    #[test]
    fn a_worktree_named_in_latin1_is_listed_with_its_checkout() {
        use std::os::unix::ffi::OsStrExt as _;
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let admin = dir
            .path()
            .join(".git")
            .join("worktrees")
            .join(OsStr::from_bytes(b"caf\xe9"));
        if std::fs::create_dir_all(&admin).is_err() {
            return;
        }
        let checkout = dir.path().join("linked");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", checkout.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let listed = super::linked_worktrees(&repo).unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].name, super::GitName::from_bytes(b"caf\xe9"));
        assert_eq!(listed[0].path, checkout);
    }

    /// Run `work` on its own thread and fail, instead of hanging, when it
    /// does not finish within a minute.
    #[cfg(unix)]
    fn bounded<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("the child exchange finished instead of deadlocking on a pipe")
    }

    #[cfg(unix)]
    #[test]
    fn output_larger_than_a_pipe_does_not_block_the_input() {
        // `cat` answers as it reads, so a writer that sends all 4 MiB before
        // reading blocks once the output pipe fills.
        let input: Vec<u8> = (b'a'..=b'w').cycle().take(4 * 1024 * 1024).collect();
        let expected = input.clone();
        let out = bounded(move || {
            output_with_input(&mut std::process::Command::new("cat"), &input).unwrap()
        });
        assert!(out.status.success());
        assert_eq!(out.stdout, expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_stops_reading_is_judged_by_its_exit_status() {
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = bounded(move || {
            output_with_input(&mut std::process::Command::new("true"), &input).unwrap()
        });
        assert!(out.status.success());
        let input = vec![b'x'; 4 * 1024 * 1024];
        let out = bounded(move || {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "exit 3"]);
            output_with_input(&mut command, &input).unwrap()
        });
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn a_program_that_cannot_start_is_an_error() {
        let mut command = std::process::Command::new("/nonexistent/codeflow-test-program");
        assert!(output_with_input(&mut command, b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn output_on_both_pipes_does_not_block_the_input() {
        // The child fills stderr with 2 MiB before it reads any stdin, then
        // echoes stdin: a caller that reads stdout alone, or writes first,
        // blocks with a pipe full.
        let input = vec![b'y'; 4 * 1024 * 1024];
        let out = bounded(move || {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "head -c 2097152 /dev/zero >&2 && cat"]);
            output_with_input(&mut command, &input).unwrap()
        });
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 4 * 1024 * 1024);
        assert_eq!(out.stderr.len(), 2 * 1024 * 1024);
    }
}
