//! Guards for a full test gate (TSK-134): one full gate at a time, and no
//! cargo target directory shared between worktrees.
//!
//! **One gate at a time.** Concurrent full gates on one machine turned green
//! suites into 600 s timeouts. [`acquire_full_gate_lock`] takes an exclusive
//! advisory lock (`fs2`, released by the OS when the process exits, however
//! it exits) on up to two files:
//!
//! - the machine-wide lock under the `CodeFlow` home (`CODEFLOW_HOME`, else
//!   `~/.codeflow`), which spans every repository on the machine;
//! - the repository lock under the git common directory, which spans every
//!   worktree of the repository and stays writable in a sandbox that can
//!   commit but cannot write the home directory.
//!
//! A lock that cannot be opened is a note, not a refusal: the lock prevents
//! contention, it is not a security boundary. A lock that is held refuses and
//! names the holder recorded in the file. A file that still names a holder
//! while its lock is free was left by a process that died, and is reclaimed.
//!
//! **Target directory.** `CARGO_TARGET_DIR` outside the worktree lets builds
//! in parallel worktrees overwrite each other's binaries. A shared directory
//! is also a legitimate way to save disk, and the gate lock already keeps two
//! gates apart, so [`check_cargo_target_dir`] returns a warning, never a
//! refusal.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;

/// File name of the gate lock in each lock directory.
pub const LOCK_FILE: &str = "full-gate.lock";

/// A held full-gate lock. The locks release when this value drops (or when
/// the process exits); a clean drop also clears the holder record.
#[derive(Debug)]
pub struct GateLock {
    files: Vec<File>,
    /// Degradations and reclaimed stale locks, for the caller to print.
    pub notes: Vec<String>,
}

impl Drop for GateLock {
    fn drop(&mut self) {
        for file in &mut self.files {
            // Best effort: an uncleared record only reads as a stale lock,
            // which the next gate reclaims.
            let _ = file.set_len(0);
        }
    }
}

/// Why a full gate may not start.
#[derive(Debug)]
pub struct LockHeld {
    /// The lock file that is held.
    pub path: PathBuf,
    /// The holder record in that file (empty when the holder has not written
    /// it yet).
    pub holder: String,
}

impl std::fmt::Display for LockHeld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let holder = if self.holder.trim().is_empty() {
            "a process that has not recorded itself yet".to_string()
        } else {
            describe_holder(&self.holder)
        };
        write!(
            f,
            "another full gate is running on this machine: {holder}. Only one full gate \
             runs at a time; wait for it to finish (lock: {})",
            self.path.display()
        )
    }
}

/// Lock directories for a project: the machine-wide one under `home` (when
/// known) and the repository one under the git common directory (when the
/// project is in a git repository).
#[must_use]
pub fn lock_dirs(project_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join("locks"));
    }
    if let Ok(repo) = git2::Repository::discover(project_dir) {
        dirs.push(repo.commondir().join("codeflow"));
    }
    dirs
}

/// Take every full-gate lock in `dirs`, or refuse naming the holder.
///
/// # Errors
///
/// Returns [`LockHeld`] when another process holds any of the locks. Locks
/// already taken in this call are released before returning.
pub fn acquire_full_gate_lock(dirs: &[PathBuf], project_dir: &Path) -> Result<GateLock, LockHeld> {
    let mut lock = GateLock {
        files: Vec::new(),
        notes: Vec::new(),
    };
    if dirs.is_empty() {
        lock.notes.push(
            "no gate lock location (no CodeFlow home and no git repository); \
             this gate does not guard against a concurrent one"
                .to_string(),
        );
    }
    for dir in dirs {
        let path = dir.join(LOCK_FILE);
        let mut file = match open_lock_file(&path) {
            Ok(file) => file,
            Err(error) => {
                lock.notes.push(format!(
                    "gate lock unavailable at {}: {error}; continuing with the other lock",
                    path.display()
                ));
                continue;
            }
        };
        match file.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == fs2::lock_contended_error().kind() => {
                return Err(LockHeld {
                    holder: read_record(&mut file),
                    path,
                });
            }
            Err(error) => {
                lock.notes.push(format!(
                    "gate lock unavailable at {}: {error}; continuing with the other lock",
                    path.display()
                ));
                continue;
            }
        }
        let previous = read_record(&mut file);
        if !previous.trim().is_empty() {
            lock.notes.push(format!(
                "reclaimed a stale gate lock at {} left by {}",
                path.display(),
                describe_holder(&previous)
            ));
        }
        if let Err(error) = write_record(&mut file, project_dir) {
            lock.notes.push(format!(
                "gate lock at {} is held but its holder record could not be written: {error}",
                path.display()
            ));
        }
        lock.files.push(file);
    }
    Ok(lock)
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

fn read_record(file: &mut File) -> String {
    let mut text = String::new();
    if file.seek(SeekFrom::Start(0)).is_ok() {
        let _ = file.take(4096).read_to_string(&mut text);
    }
    text
}

fn write_record(file: &mut File, project_dir: &Path) -> std::io::Result<()> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "started={started}")?;
    writeln!(file, "dir={}", project_dir.display())?;
    file.flush()
}

/// Render a holder record as `pid N in DIR, started S s ago`.
fn describe_holder(record: &str) -> String {
    let field = |key: &str| {
        record
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    let pid = field("pid").unwrap_or("unknown");
    let dir = field("dir").map(|d| format!(" in {d}")).unwrap_or_default();
    let age = field("started")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|started| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            format!(", started {} s ago", now.saturating_sub(started))
        })
        .unwrap_or_default();
    format!("pid {pid}{dir}{age}")
}

/// Warn about a `CARGO_TARGET_DIR` outside the worktree when the gate runs
/// cargo.
///
/// `value` is the variable as set (relative paths resolve against `cwd`, as
/// cargo resolves them). `None` or an empty value passes.
///
/// Returns the warning naming the shared directory, or `None`.
#[must_use]
pub fn check_cargo_target_dir(
    worktree: &Path,
    cwd: &Path,
    value: Option<&std::ffi::OsStr>,
) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;
    let raw = Path::new(value);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };
    let target = resolve(&joined);
    let root = resolve(worktree);
    if target.starts_with(&root) {
        return None;
    }
    Some(format!(
        "CARGO_TARGET_DIR={} is shared outside this worktree ({}). When worktrees build \
         in parallel, a shared target directory lets one build overwrite another's \
         binaries mid-run; for parallel work, point it inside each worktree (for \
         example {}).",
        target.display(),
        root.display(),
        root.join("target").display()
    ))
}

/// Canonicalize the longest existing ancestor and append the rest lexically,
/// so a target directory that does not exist yet still resolves symlinks in
/// its existing part (`/tmp` versus `/private/tmp` on macOS).
fn resolve(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    let mut existing = normal.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical) = existing.canonicalize() {
            let mut out = canonical;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(tmp: &Path) -> Vec<PathBuf> {
        vec![tmp.join("home-locks"), tmp.join("repo-locks")]
    }

    #[test]
    fn a_second_gate_is_refused_naming_the_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let first = acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/work/a")).unwrap();
        assert!(first.notes.is_empty(), "{:?}", first.notes);

        let held = acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/work/b")).unwrap_err();
        let message = held.to_string();
        assert!(
            message.contains(&format!("pid {}", std::process::id())),
            "{message}"
        );
        assert!(message.contains("in /work/a"), "{message}");
        assert!(message.contains("one full gate"), "{message}");

        drop(first);
        // A sibling test forking a child can hold a duplicate of the lock
        // descriptor until that child's exec closes it; allow that window.
        let again = (0..40)
            .find_map(|_| {
                acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/work/b"))
                    .map_err(|_| std::thread::sleep(std::time::Duration::from_millis(50)))
                    .ok()
            })
            .expect("the lock is free once its holder drops it");
        assert!(
            again.notes.is_empty(),
            "a clean release leaves no stale record: {:?}",
            again.notes
        );
    }

    #[test]
    fn the_repository_lock_alone_still_refuses() {
        // A sandboxed gate may reach only the repository lock; it must still
        // see a gate that holds both.
        let tmp = tempfile::tempdir().unwrap();
        let _both = acquire_full_gate_lock(&dirs(tmp.path()), Path::new("/w")).unwrap();
        let repo_only = vec![tmp.path().join("repo-locks")];
        assert!(acquire_full_gate_lock(&repo_only, Path::new("/w")).is_err());
    }

    #[test]
    fn a_stale_lock_from_a_dead_process_is_reclaimed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        // A record with no flock behind it: its process died without cleanup.
        std::fs::write(
            dir.join(LOCK_FILE),
            "pid=999999\nstarted=1\ndir=/gone/worktree\n",
        )
        .unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert!(
            lock.notes
                .iter()
                .any(|n| n.contains("reclaimed a stale gate lock")
                    && n.contains("pid 999999 in /gone/worktree")),
            "{:?}",
            lock.notes
        );
        let record = std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
        assert!(record.contains(&format!("pid={}", std::process::id())));
    }

    #[test]
    fn an_unopenable_lock_is_a_note_not_a_refusal() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        let unusable = vec![blocker.join("locks"), tmp.path().join("repo-locks")];
        let lock = acquire_full_gate_lock(&unusable, Path::new("/w")).unwrap();
        assert!(
            lock.notes
                .iter()
                .any(|n| n.contains("gate lock unavailable")),
            "{:?}",
            lock.notes
        );
        assert_eq!(lock.files.len(), 1);
    }

    #[test]
    fn lock_dirs_cover_home_and_the_git_common_dir() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let found = lock_dirs(tmp.path(), Some(&home));
        assert_eq!(found[0], home.join("locks"));
        assert!(found[1].ends_with(".git/codeflow"), "{found:?}");
    }

    #[test]
    fn target_dir_inside_the_worktree_gives_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        for value in ["target", "./build/cargo", ""] {
            assert!(
                check_cargo_target_dir(&root, &root, Some(std::ffi::OsStr::new(value))).is_none(),
                "{value}"
            );
        }
        let abs = root.join("target");
        assert!(check_cargo_target_dir(&root, &root, Some(abs.as_os_str())).is_none());
        assert!(check_cargo_target_dir(&root, &root, None).is_none());
    }

    #[test]
    fn target_dir_outside_the_worktree_warns_naming_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("wt");
        std::fs::create_dir_all(&root).unwrap();
        let shared = tmp.path().join("shared-target");
        let warning = check_cargo_target_dir(&root, &root, Some(shared.as_os_str())).unwrap();
        assert!(warning.contains("shared-target"), "{warning}");
        assert!(warning.contains("in parallel"), "{warning}");
        // A relative escape resolves against the working directory.
        let escape = check_cargo_target_dir(&root, &root, Some(std::ffi::OsStr::new("../x")));
        assert!(escape.is_some());
    }
}
