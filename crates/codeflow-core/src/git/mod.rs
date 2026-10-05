//! Git operations: merge conflict detection, branch analysis, CI polling,
//! and the one constructor for every `git` process codeflow starts.
//!
//! Uses `git2` for native git operations where it can; CI polling shells to
//! the `gh` CLI.

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod ci;
pub mod conflict;
pub mod remote_query;

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
    let name = program.to_string_lossy();
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    name.split('.')
        .next()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("git"))
}

/// A reference name as text that names it exactly.
///
/// OS text rule (issue 79, `docs/architecture.md`): a branch or ref name is
/// matched against valid-UTF-8 patterns and names (protected globs, task
/// prefixes) and shown, so a name that is not valid UTF-8 must not read as
/// detached or absent. A plain lossy decode would also make two different
/// names equal, such as `caf\xe9` and a real `caf` followed by U+FFFD, and a
/// guard comparing branch names would then take one for the other. Each
/// invalid byte is written as `\xNN` instead. git forbids a backslash in a
/// reference name, so no valid name spells an escape and the text is unique
/// for the bytes.
#[must_use]
pub fn ref_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for byte in chunk.invalid() {
            use std::fmt::Write as _;
            let _ = write!(text, "\\x{byte:02x}");
        }
    }
    text
}

/// The short name of a reference, as [`ref_text`] spells it.
#[must_use]
pub fn reference_shorthand(reference: &git2::Reference<'_>) -> String {
    ref_text(reference.shorthand_bytes())
}

/// A linked worktree as git lists it.
#[derive(Debug, Clone)]
pub struct LinkedWorktree {
    /// The administrative folder name, read lossily; it is shown, never
    /// looked up.
    pub name: String,
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
/// is what `find_worktree` reads too.
#[must_use]
pub fn linked_worktrees(repo: &git2::Repository) -> Vec<LinkedWorktree> {
    let Ok(names) = repo.worktrees() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in names.iter_bytes() {
        let lossy = String::from_utf8_lossy(name).into_owned();
        let path = match std::str::from_utf8(name) {
            Ok(valid) => repo
                .find_worktree(valid)
                .ok()
                .map(|worktree| worktree.path().to_path_buf()),
            Err(_) => gitdir_checkout(repo, name),
        };
        if let Some(path) = path {
            let admin = repo.commondir().join("worktrees").join(os_component(name));
            out.push(LinkedWorktree {
                name: lossy,
                path,
                admin,
            });
        }
    }
    out
}

/// The checkout named by `<common dir>/worktrees/<name>/gitdir`, which holds
/// the path of the checkout's `.git`.
fn gitdir_checkout(repo: &git2::Repository, name: &[u8]) -> Option<PathBuf> {
    let file = repo
        .commondir()
        .join("worktrees")
        .join(os_component(name))
        .join("gitdir");
    let text = std::fs::read(file).ok()?;
    let gitdir = os_path(text.trim_ascii_end());
    gitdir.parent().map(std::path::Path::to_path_buf)
}

/// Every checkout of the repository, the main one and each linked worktree,
/// with the full name of the reference its `HEAD` names (`None` when it is
/// detached or unreadable).
///
/// OS text rule (issue 79): the path is exact bytes where the platform allows
/// it and is read from git's own files, never from a text listing, where a
/// newline in a folder name would end the path early and name another
/// checkout. A caller that acts on a checkout (a `reset --hard`) can trust the
/// path it gets here.
#[must_use]
pub fn checkout_heads(repo: &git2::Repository) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let head_of = |git_dir: &std::path::Path| -> Option<Vec<u8>> {
        let text = std::fs::read(git_dir.join("HEAD")).ok()?;
        text.trim_ascii_end()
            .strip_prefix(b"ref: ")
            .map(<[u8]>::to_vec)
    };
    let mut out = Vec::new();
    if let Ok(main) = git2::Repository::open(repo.commondir()) {
        if let Some(workdir) = main.workdir() {
            out.push((workdir.to_path_buf(), head_of(repo.commondir())));
        }
    }
    for worktree in linked_worktrees(repo) {
        let head = head_of(&worktree.admin);
        out.push((worktree.path, head));
    }
    out
}

/// A path as a key that names its bytes exactly.
///
/// OS text rule (issue 79): a path is bytes. The key is the path itself when
/// it is valid UTF-8. Otherwise it is the lossy text, a NUL and the hex of the
/// bytes: a path never holds a NUL, so no valid path can equal that key, and
/// two different invalid paths differ in their hex. A plain lossy decode would
/// give `caf` plus an invalid byte and `caf` plus a real U+FFFD one key, and a
/// change to one would read as a change to the other.
#[must_use]
pub fn path_key(raw: &[u8]) -> String {
    use std::fmt::Write as _;
    if let Ok(name) = std::str::from_utf8(raw) {
        return name.to_string();
    }
    let mut key = String::from_utf8_lossy(raw).into_owned();
    key.push('\0');
    for byte in raw {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

/// The bytes a [`path_key`] names.
#[must_use]
pub fn path_key_bytes(key: &str) -> Vec<u8> {
    let Some((_, hex)) = key.split_once('\0') else {
        return key.as_bytes().to_vec();
    };
    hex.as_bytes()
        .chunks(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

/// One path component from bytes, exact where the platform allows it.
#[must_use]
pub fn os_component(name: &[u8]) -> PathBuf {
    os_path(name)
}

fn os_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
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

#[cfg(test)]
mod tests {
    use super::{ref_text, runs_git};
    use std::ffi::OsStr;

    #[test]
    fn a_ref_name_keeps_every_invalid_byte_as_its_own_escape() {
        assert_eq!(ref_text(b"release/main"), "release/main");
        assert_eq!(ref_text("caf\u{e9}".as_bytes()), "caf\u{e9}");
        assert_eq!(ref_text(b"caf\xe9"), "caf\\xe9");
        assert_eq!(ref_text(b"a\xff\xfeb"), "a\\xff\\xfeb");
        // The old lossy spelling gave these two one text.
        assert_ne!(ref_text(b"caf\xe9"), ref_text("caf\u{fffd}".as_bytes()));
        assert_ne!(ref_text(b"caf\xe9"), ref_text(b"caf\xff"));
    }

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
        let listed = super::linked_worktrees(&repo);
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].name, "caf\u{fffd}");
        assert_eq!(listed[0].path, checkout);
    }
}

#[cfg(test)]
mod path_key_tests {
    use super::{path_key, path_key_bytes};

    #[test]
    fn a_path_key_names_its_bytes_and_no_other_path() {
        assert_eq!(path_key(b"dir/plain.txt"), "dir/plain.txt");
        assert_eq!(path_key("caf\u{e9}".as_bytes()), "caf\u{e9}");
        let invalid = path_key(b"caf\xe9");
        // A real U+FFFD in a valid path is another path.
        assert_ne!(invalid, path_key("caf\u{fffd}".as_bytes()));
        assert_ne!(invalid, path_key(b"caf\xff"));
        // The key goes back to its bytes, so a path can be rebuilt from it.
        for raw in [
            &b"caf\xe9"[..],
            b"a/b\xff\xfe/c",
            b"plain",
            "caf\u{e9}".as_bytes(),
        ] {
            assert_eq!(path_key_bytes(&path_key(raw)), raw);
        }
    }
}
