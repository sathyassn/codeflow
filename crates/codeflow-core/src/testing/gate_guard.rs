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
//! Every configured lock must be opened and acquired before targets start.
//! An unavailable lock refuses, naming the path and sandbox error. A held lock refuses and
//! names the holder recorded in the file. A file that still names a holder
//! while its lock is free was left by a process that died, and is reclaimed.
//!
//! **Targets outlive a killed gate on Unix.** Each target runs in its own
//! process group, so killing the gate process frees its lock while the
//! target keeps running. While a gate holds the lock, the runner records
//! each running target's process group in the holder record
//! ([`target_group_started`]). A free lock whose record names a process
//! group with a live process is still held: the next gate refuses until
//! that group exits.
//!
//! **On Windows they end with it** (TSK-142). Each target runs in a
//! kill-on-close job object whose only handle the gate process holds, so
//! the OS ends the target's tree when it closes that handle as the gate
//! exits, and the lock never frees while a target of the gate runs. No
//! group is recorded there.
//!
//! **Target directory.** `CARGO_TARGET_DIR` outside the worktree lets builds
//! in parallel worktrees overwrite each other's binaries. A shared directory
//! is also a legitimate way to save disk, and the gate lock already keeps two
//! gates apart, so [`check_cargo_target_dir`] returns a warning, never a
//! refusal.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;

/// File name of the gate lock in each lock directory.
pub const LOCK_FILE: &str = "full-gate.lock";

/// A held full-gate lock. The locks release when this value drops (or when
/// the process exits); a clean drop also clears the holder record.
#[derive(Debug)]
pub struct GateLock {
    id: u64,
    files: Vec<File>,
    /// Degradations and reclaimed stale locks, for the caller to print.
    pub notes: Vec<String>,
}

/// A lock held by this process, whose record the runner extends with the
/// process groups of running targets.
struct Held {
    id: u64,
    paths: Vec<PathBuf>,
    record: String,
    groups: BTreeSet<u32>,
}

static HELD: Mutex<Vec<Held>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn held() -> std::sync::MutexGuard<'static, Vec<Held>> {
    HELD.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Drop for GateLock {
    fn drop(&mut self) {
        // Deregister first, under the registry lock, so no target update can
        // rewrite a record after it is cleared.
        held().retain(|h| h.id != self.id);
        for file in &mut self.files {
            // Best effort: an uncleared record only reads as a stale lock,
            // which the next gate reclaims.
            let _ = file.set_len(0);
            let _ = FileExt::unlock(file);
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
    /// Process groups of targets still running after their gate process
    /// exited; empty when the gate process itself holds the lock.
    pub running_groups: Vec<u32>,
}

impl std::fmt::Display for LockHeld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.holder.starts_with("gate lock unavailable:") {
            return write!(f, "{} (lock: {})", self.holder, self.path.display());
        }
        let holder = if self.holder.trim().is_empty() {
            "a process that has not recorded itself yet".to_string()
        } else {
            describe_holder(&self.holder)
        };
        if !self.running_groups.is_empty() {
            let groups: Vec<String> = self.running_groups.iter().map(u32::to_string).collect();
            return write!(
                f,
                "another full gate's targets are still running on this machine: process \
                 group {} of {holder}, whose gate process has exited. Only one full gate \
                 runs at a time; wait for those targets to finish or stop them (lock: {})",
                groups.join(", "),
                self.path.display()
            );
        }
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
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        files: Vec::new(),
        notes: Vec::new(),
    };
    let record = holder_record(project_dir);
    let mut paths = Vec::new();
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
                return Err(LockHeld {
                    holder: format!("gate lock unavailable: {error}"),
                    path,
                    running_groups: Vec::new(),
                });
            }
        };
        match file.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == fs2::lock_contended_error().kind() => {
                return Err(LockHeld {
                    holder: read_record(&mut file),
                    path,
                    running_groups: Vec::new(),
                });
            }
            Err(error) => {
                return Err(LockHeld {
                    holder: format!("gate lock unavailable: {error}"),
                    path,
                    running_groups: Vec::new(),
                });
            }
        }
        let previous = read_record(&mut file);
        let running_groups = live_groups(&previous);
        if !running_groups.is_empty() {
            // The gate that wrote this record is gone but its targets are
            // not: the lock is still in use. Leave the record for the next
            // check.
            return Err(LockHeld {
                holder: previous,
                path,
                running_groups,
            });
        }
        if !previous.trim().is_empty() {
            lock.notes.push(format!(
                "reclaimed a stale gate lock at {} left by {}",
                path.display(),
                describe_holder(&previous)
            ));
        }
        if let Err(error) = write_record(&mut file, &record) {
            lock.notes.push(format!(
                "gate lock at {} is held but its holder record could not be written: {error}",
                path.display()
            ));
        }
        lock.files.push(file);
        paths.push(path);
    }
    if !paths.is_empty() {
        held().push(Held {
            id: lock.id,
            paths,
            record,
            groups: BTreeSet::new(),
        });
    }
    Ok(lock)
}

/// Record that a target started in process group `pgid`, in every full-gate
/// lock this process holds. A no-op when it holds none (quick mode, or a
/// runner used outside `codeflow test`).
pub fn target_group_started(pgid: u32) {
    update_groups(|groups| {
        groups.insert(pgid);
    });
}

/// Record that the target in process group `pgid` finished.
pub fn target_group_finished(pgid: u32) {
    update_groups(|groups| {
        groups.remove(&pgid);
    });
}

fn update_groups(change: impl Fn(&mut BTreeSet<u32>)) {
    let mut held = held();
    for lock in held.iter_mut() {
        change(&mut lock.groups);
        let mut text = lock.record.clone();
        for group in &lock.groups {
            // Writing to a String cannot fail.
            let _ = writeln!(text, "group={group}");
        }
        for path in &lock.paths {
            // Best effort, through a second descriptor: `flock` locks belong
            // to the open file description, so this neither takes nor drops
            // the lock. A missed write only weakens the killed-gate check.
            let _ = std::fs::write(path, &text);
        }
    }
}

/// Process groups named in a holder record that still have a live process.
fn live_groups(record: &str) -> Vec<u32> {
    record
        .lines()
        .filter_map(|line| line.strip_prefix("group=")?.trim().parse::<u32>().ok())
        .filter(|&pgid| group_alive(pgid))
        .collect()
}

#[cfg(unix)]
fn group_alive(pgid: u32) -> bool {
    // 0 and 1 are not target groups, and as negative pids they address this
    // process's own group or every process.
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return false;
    };
    if pgid <= 1 {
        return false;
    }
    // SAFETY: signal 0 only checks that the group exists and is signalable;
    // nothing is delivered.
    if unsafe { libc::kill(-pgid, 0) } == 0 {
        return true;
    }
    // EPERM: the group exists but belongs to another user.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn group_alive(_pgid: u32) -> bool {
    // No group is recorded here: on Windows each target's tree ends with
    // its gate through the target's job object (TSK-142), so nothing can
    // outlive the gate.
    false
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

fn holder_record(project_dir: &Path) -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "pid={}\nstarted={started}\ndir={}\n",
        std::process::id(),
        project_dir.display()
    )
}

fn write_record(file: &mut File, record: &str) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(record.as_bytes())?;
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

    #[cfg(unix)]
    #[test]
    fn a_free_lock_is_held_while_its_target_group_runs() {
        use std::os::unix::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let mut target = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = target.id();
        // The record a killed gate leaves: its pid is gone, its target is not.
        std::fs::write(
            dir.join(LOCK_FILE),
            format!("pid=999999\nstarted=1\ndir=/gone\ngroup={group}\n"),
        )
        .unwrap();
        let held = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a live target group keeps the lock");
        assert_eq!(held.running_groups, vec![group]);
        let message = held.to_string();
        assert!(
            message.contains(&format!("process group {group}")),
            "{message}"
        );
        assert!(message.contains("pid 999999 in /gone"), "{message}");
        assert!(
            std::fs::read_to_string(dir.join(LOCK_FILE))
                .unwrap()
                .contains(&format!("group={group}")),
            "the refusal leaves the record for the next check"
        );

        target.kill().unwrap();
        target.wait().unwrap();
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        assert!(
            lock.notes
                .iter()
                .any(|n| n.contains("reclaimed a stale gate lock")),
            "{:?}",
            lock.notes
        );
    }

    #[test]
    fn a_held_lock_records_running_target_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        // A group number no other test uses: records are shared by every
        // lock this process holds.
        target_group_started(4_000_001);
        let record = std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
        assert!(record.contains("group=4000001"), "{record}");
        assert!(record.contains(&format!("pid={}", std::process::id())));
        target_group_finished(4_000_001);
        let record = std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap();
        assert!(!record.contains("group=4000001"), "{record}");
        drop(lock);
        assert_eq!(std::fs::read_to_string(dir.join(LOCK_FILE)).unwrap(), "");
    }

    #[test]
    fn an_unopenable_lock_refuses_and_releases_other_locks() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        let usable = tmp.path().join("repo-locks");
        let unusable = vec![usable.clone(), blocker.join("locks")];
        let error = acquire_full_gate_lock(&unusable, Path::new("/w")).unwrap_err();
        assert!(error.to_string().contains("gate lock unavailable"));
        let file = open_lock_file(&usable.join(LOCK_FILE)).unwrap();
        file.try_lock_exclusive()
            .expect("partial acquisition released its OS lock");
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
