//! Guards for heavy test work: one heavy gate at a time, and no
//! cargo target directory shared between worktrees.
//!
//! **One gate at a time.** Concurrent full gates on one machine turned green
//! suites into 600 s timeouts. [`acquire_full_gate_lock`] takes an exclusive
//! lock (`flock` on Unix, a byte-range lock on Windows; the OS releases it
//! when the process exits, however it exits) on up to two files:
//!
//! - the machine-wide lock under the `CodeFlow` home (`CODEFLOW_HOME`, else
//!   `~/.codeflow`), which spans every repository on the machine;
//! - the repository lock under the git common directory, which spans every
//!   worktree of the repository.
//!
//! Every configured lock must be opened and acquired before targets start,
//! so essential and full gates, heavy quick targets and reserved commands
//! exclude each other on one machine. An unavailable lock
//! refuses, naming the path and sandbox error. The shipped Claude settings
//! presets allow sandboxed writes to `~/.codeflow/locks` and
//! `~/.codeflow/gate-runs` (TSK-216), the two home directories a full gate
//! writes, and `codeflow doctor --check permissions` names any of them this
//! process cannot write ([`unwritable_gate_dirs`]). A held lock refuses and
//! names the holder recorded beside it. A record that still names a holder
//! while its lock is free was left by a process that died, and is reclaimed.
//!
//! **Lock folders may be writable by a sandboxed seat** (TSK-1003), so a
//! lock file is never written: it is opened read only, without following
//! links, and only locked. The holder record ([`RECORD_FILE`]) is replaced
//! whole by a new file renamed into place, read without following links,
//! and shown only through validated values (numbers, a known mode, and a
//! folder only when its text names one the gate already trusts; a planted
//! folder value is never resolved). A link planted in the folder can be
//! locked or replaced, never written through, and reveals nothing beyond a
//! folder the gate already trusts. A gate that cannot write its record
//! refuses instead of running uncoordinated. The folder
//! stays an advisory coordination boundary: a seat that can write it can
//! block or split coordination between gates. A record a pre-3.2 gate keeps
//! inside the lock file is still read while the record file names no holder.
//!
//! **Target lifetime on Unix.** Heavy quick targets and reserved commands
//! have a watcher that ends their process group when the controller exits.
//! Other targets can outlive a killed gate. While a gate holds the lock, the runner records
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

use crate::bounded_file::FileIdentity;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// File name of the gate lock in each lock directory. The file is only ever
/// opened read only and locked; `CodeFlow` never writes, truncates or deletes
/// it (TSK-1003).
pub const LOCK_FILE: &str = "full-gate.lock";

/// File name of the holder record beside each lock. A record is replaced
/// whole: written to a new file created exclusively under a random name in
/// the same folder, then renamed over this name. An existing record is
/// never opened for writing, so a link planted at this name is replaced,
/// never written through (TSK-1003).
pub const RECORD_FILE: &str = "full-gate.record";

/// A held heavy-work lock. The locks release when this value drops (or when
/// the process exits); a clean drop also clears the holder record.
#[derive(Debug)]
pub struct GateLock {
    id: u64,
    files: Vec<AcquiredFile>,
    /// The lock files held, whose records the drop clears.
    paths: Vec<PathBuf>,
    /// Degradations and reclaimed stale locks, for the caller to print.
    pub notes: Vec<String>,
}

/// A lock held by this process, whose record the runner extends with the
/// process groups of running targets.
struct Held {
    id: u64,
    /// The lock files held, each with its record beside it.
    paths: Vec<PathBuf>,
    record: String,
    groups: BTreeSet<u32>,
    #[cfg(unix)]
    descriptors: Vec<std::os::fd::RawFd>,
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
        for path in &self.paths {
            // Best effort, before the locks release: an uncleared record only
            // reads as a stale lock, which the next gate reclaims. An empty
            // record names no holder, so readers fall back to a legacy record
            // inside the lock file, which a pre-3.2 gate may have left.
            let _ = write_record(path, "");
        }
        // AcquiredFile explicitly unlocks before its descriptor closes.
    }
}

/// Why heavy work may not start.
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
        if self.holder.starts_with("gate lock unavailable:")
            || self.holder.starts_with("cannot read gate lock record:")
            || self.holder.starts_with("cannot reclaim gate lock:")
        {
            return write!(
                f,
                "{} (lock: {})",
                self.holder,
                crate::git::display_path(&self.path)
            );
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
                "another heavy gate's targets are still running on this machine: process \
                 group {} of {holder}, whose gate process has exited. Only one heavy gate \
                 runs at a time; wait for those targets to finish or stop them (lock: {})",
                groups.join(", "),
                crate::git::display_path(&self.path)
            );
        }
        write!(
            f,
            "another heavy gate is running on this machine: {holder}. Only one heavy gate \
             runs at a time; wait for it to finish (lock: {})",
            crate::git::display_path(&self.path)
        )
    }
}

/// The directory under the `CodeFlow` home that holds the machine-wide lock.
pub const HOME_LOCK_DIR: &str = "locks";

/// The directory under the `CodeFlow` home that keeps full-run evidence.
pub const HOME_EVIDENCE_DIR: &str = "gate-runs";

/// Lock directories for a project: the machine-wide one under `home` (when
/// known) and the repository one under the git common directory (when the
/// project is in a git repository).
/// # Errors
/// Refuses repository discovery failures; only `NotFound` means no repository.
pub fn lock_dirs(project_dir: &Path, home: Option<&Path>) -> Result<Vec<PathBuf>, git2::Error> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(HOME_LOCK_DIR));
    }
    match git2::Repository::discover(project_dir) {
        Ok(repo) => dirs.push(repo.commondir().join("codeflow")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(dirs)
}

/// The directories a full gate writes outside the worktree that this
/// process cannot write, each with its error: every lock directory and the
/// evidence directory under `home`. Each lock file is opened as the gate
/// opens it, without taking the lock, so a running gate is not disturbed;
/// the evidence directory is probed with a file that is removed again.
#[must_use]
pub fn unwritable_gate_dirs(
    project_dir: &Path,
    home: Option<&Path>,
) -> Vec<(PathBuf, std::io::Error)> {
    let mut unwritable = Vec::new();
    let dirs = match lock_dirs(project_dir, home) {
        Ok(dirs) => dirs,
        Err(error) => {
            return vec![(
                project_dir.to_path_buf(),
                std::io::Error::other(error.to_string()),
            )]
        }
    };
    for dir in dirs {
        if let Err(error) = probe_lock_dir(&dir) {
            unwritable.push((dir, error));
        }
    }
    if let Some(home) = home {
        let dir = home.join(HOME_EVIDENCE_DIR);
        let probe = dir.join(format!(".write-probe-{}", std::process::id()));
        let result = std::fs::create_dir_all(&dir).and_then(|()| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
                .map(drop)
        });
        match result {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
            }
            Err(error) => unwritable.push((dir, error)),
        }
    }
    unwritable
}

/// Whether a gate could use the lock folder `dir`: it needs the lock file
/// and a holder record it can create and rename into place. Both are
/// probed, the record with a disposable name through the same anchored
/// write, removed again (TSK-1003). The lock itself is never taken.
fn probe_lock_dir(dir: &Path) -> std::io::Result<()> {
    let lock = dir.join(LOCK_FILE);
    let probe = dir.join(format!(".record-probe-{}", std::process::id()));
    open_lock_file(&lock).map(drop).and_then(|()| {
        let (anchor, relative) = lock_anchor(&probe)?;
        let tree = crate::contained::Tree::open(anchor)?;
        tree.write(&relative, b"")?;
        tree.remove(&relative)
    })
}

/// What a builder seat has to write outside its worktree (TSK-337), each
/// folder with its result: both gate lock folders (`locks` under `home` and
/// `codeflow` under the common git directory), probed as
/// [`unwritable_gate_dirs`] probes them, and the common git directory's
/// `objects` and `refs`, which a commit writes, each probed by creating and
/// removing a file whose dot-led name git never reads as an object or a
/// ref, through an anchored write that follows no link. The gate evidence
/// folder is not probed: builder seats run no full gate.
#[must_use]
pub fn builder_write_probe(
    project_dir: &Path,
    home: Option<&Path>,
) -> Vec<(PathBuf, std::io::Result<()>)> {
    let dirs = match lock_dirs(project_dir, home) {
        Ok(dirs) => dirs,
        Err(error) => {
            return vec![(
                project_dir.to_path_buf(),
                Err(std::io::Error::other(error.to_string())),
            )]
        }
    };
    let mut results: Vec<(PathBuf, std::io::Result<()>)> = dirs
        .iter()
        .map(|dir| (dir.clone(), probe_lock_dir(dir)))
        .collect();
    if home.is_none() {
        results.insert(
            0,
            (
                PathBuf::from(HOME_LOCK_DIR),
                Err(std::io::Error::other(
                    "no CodeFlow home folder, so its lock folder was not probed",
                )),
            ),
        );
    }
    let common = match git2::Repository::discover(project_dir) {
        Ok(repo) => repo.commondir().to_path_buf(),
        Err(error) => {
            results.push((
                project_dir.to_path_buf(),
                Err(std::io::Error::other(format!(
                    "no git repository, so the objects and refs folders were not probed: {error}"
                ))),
            ));
            return results;
        }
    };
    for folder in ["objects", "refs"] {
        let relative = format!("{folder}/.codeflow-write-probe-{}", std::process::id());
        let result = crate::contained::Tree::open(&common).and_then(|tree| {
            tree.create_new(&relative, b"")?;
            tree.remove(&relative)
        });
        results.push((common.join(folder), result));
    }
    results
}

/// Read holder records without taking a lock, changing a record, or creating directories.
/// These are snapshots: admission still checks the OS lock and live target groups.
/// # Errors
/// Refuses discovery failures and unreadable or oversized records.
pub fn gate_lock_status(root: &Path) -> Result<Vec<String>, String> {
    let dirs = lock_dirs(root, crate::registry::codeflow_home().as_deref())
        .map_err(|error| format!("cannot discover gate lock directories: {error}"))?;
    let trusted = trusted_dirs(root);
    dirs.into_iter()
        .map(|dir| holder_status(&dir.join(LOCK_FILE), &trusted))
        .collect()
}

/// One status line for the lock at `path`: its holder's validated values, or
/// that none is recorded.
fn holder_status(path: &Path, trusted: &[String]) -> Result<String, String> {
    let record = read_holder(path, None, trusted).map_err(|error| {
        format!(
            "cannot read gate lock {}: {error}",
            crate::git::display_path(path)
        )
    })?;
    let description = if record.trim().is_empty() {
        "no recorded holder".to_string()
    } else {
        let started = record
            .lines()
            .find_map(|line| line.strip_prefix("started="))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or_else(|| "unknown".to_string(), |value| value.to_string());
        format!(
            "recorded holder: {}; started_unix={started}",
            crate::git::display_line(&describe_holder(&record))
        )
    };
    Ok(format!("{}: {description}", crate::git::display_path(path)))
}

/// Acquire the machine and repository locks, with the operator's repair pointer.
/// # Errors
/// Refuses unavailable directories and held locks before starting heavy work.
pub fn acquire_project_gate_lock(root: &Path, mode: &str) -> Result<GateLock, String> {
    let dirs = lock_dirs(root, crate::registry::codeflow_home().as_deref())
        .map_err(|error| format!("gate lock unavailable: cannot discover repository: {error}; run `codeflow doctor --check permissions`"))?;
    acquire_gate_lock(&dirs, root, mode).map_err(|held| {
        let message = held.to_string();
        if message.contains("gate lock unavailable") {
            format!("{message}; run `codeflow doctor --check permissions` to identify the directories to repair")
        } else { message }
    })
}

/// Test-only interleaving points in lock acquisition, so a test can change
/// the lock folder at the moment a racing seat could (TSK-1003).
#[cfg(test)]
pub(crate) mod interleave {
    use std::cell::RefCell;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Point {
        /// The lock file is open, before any check on it.
        Opened,
        /// The OS lock is taken.
        Locked,
    }

    type Step = Box<dyn FnMut(Point)>;

    thread_local! {
        static STEP: RefCell<Option<Step>> = const { RefCell::new(None) };
    }

    // Only the Unix interleaving tests set a step.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn set(step: impl FnMut(Point) + 'static) {
        STEP.with(|cell| *cell.borrow_mut() = Some(Box::new(step)));
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn clear() {
        STEP.with(|cell| *cell.borrow_mut() = None);
    }

    pub(super) fn at(point: Point) {
        STEP.with(|cell| {
            if let Some(step) = cell.borrow_mut().as_mut() {
                step(point);
            }
        });
    }
}

/// Open the lock file at `path` and take its OS lock without waiting, or
/// say why not: the holder's record, or why the lock is unavailable.
fn open_and_lock(path: &Path, trusted: &[String]) -> Result<File, LockHeld> {
    let path = path.to_path_buf();
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
    match os_lock::try_lock(&file) {
        Ok(()) => {
            #[cfg(test)]
            interleave::at(interleave::Point::Locked);
            if let Err(error) = same_lock_file(&path, &file) {
                os_lock::unlock(&file);
                return Err(LockHeld {
                    holder: format!("gate lock unavailable: {error}; retry"),
                    path,
                    running_groups: Vec::new(),
                });
            }
        }
        Err(error) if os_lock::is_contended(&error) => {
            return Err(LockHeld {
                holder: read_holder(&path, Some(&mut file), trusted)
                    .unwrap_or_else(|error| format!("cannot read gate lock record: {error}")),
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
    Ok(file)
}

/// Take every full-gate lock in `dirs`, or refuse naming the holder.
///
/// # Errors
///
/// Returns [`LockHeld`] when another process holds any of the locks. Locks
/// already taken in this call are released before returning.
pub fn acquire_full_gate_lock(dirs: &[PathBuf], project_dir: &Path) -> Result<GateLock, LockHeld> {
    acquire_gate_lock(dirs, project_dir, "full")
}

/// Take the shared heavy-work locks and record the requested mode.
/// # Errors
/// Returns the holder or the unavailable lock path without starting work.
pub fn acquire_gate_lock(
    dirs: &[PathBuf],
    project_dir: &Path,
    mode: &str,
) -> Result<GateLock, LockHeld> {
    let mut lock = GateLock {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        files: Vec::new(),
        paths: Vec::new(),
        notes: Vec::new(),
    };
    let record = holder_record(project_dir, mode);
    let trusted = trusted_dirs(project_dir);
    if dirs.is_empty() {
        lock.notes.push(
            "no gate lock location (no CodeFlow home and no git repository); \
             this gate does not guard against a concurrent one"
                .to_string(),
        );
    }
    for dir in dirs {
        let path = dir.join(LOCK_FILE);
        let file = open_and_lock(&path, &trusted)?;
        let mut acquired = AcquiredFile(file);
        let file = &mut acquired.0;
        let previous = read_holder(&path, Some(file), &trusted).map_err(|error| LockHeld {
            holder: format!("cannot read gate lock record: {error}"),
            path: path.clone(),
            running_groups: Vec::new(),
        })?;
        let running_groups = live_groups(&previous).map_err(|error| LockHeld {
            holder: format!("cannot reclaim gate lock: {error}; inspect and repair the lock record after verifying its processes"),
            path: path.clone(),
            running_groups: Vec::new(),
        })?;
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
                crate::git::display_path(&path),
                crate::git::display_line(&describe_holder(&previous))
            ));
        }
        // The record is what lets another gate see this one and its live
        // targets, so a gate that cannot write it does not run uncoordinated
        // (TSK-1003): it refuses, and the lock releases as `acquired` drops.
        if let Err(error) = write_record(&path, &record) {
            return Err(LockHeld {
                holder: format!(
                    "gate lock unavailable: its holder record cannot be written: {error}"
                ),
                path,
                running_groups: Vec::new(),
            });
        }
        lock.files.push(acquired);
        lock.paths.push(path);
    }
    if !lock.files.is_empty() {
        held().push(Held {
            id: lock.id,
            paths: lock.paths.clone(),
            record,
            groups: BTreeSet::new(),
            #[cfg(unix)]
            descriptors: {
                use std::os::fd::AsRawFd;
                lock.files.iter().map(|file| file.0.as_raw_fd()).collect()
            },
        });
    }
    Ok(lock)
}

/// Duplicate held locks for a target's lifetime watcher before spawning it.
/// The registry guard prevents `GateLock::drop` from closing a borrowed descriptor.
#[cfg(unix)]
pub(crate) fn inherited_gate_locks() -> std::io::Result<Vec<File>> {
    use std::os::fd::BorrowedFd;
    let locks = held();
    locks
        .iter()
        .flat_map(|lock| &lock.descriptors)
        .map(|&descriptor| {
            // SAFETY: the registry guard keeps the owning GateLock alive during this clone.
            unsafe { BorrowedFd::borrow_raw(descriptor) }
                .try_clone_to_owned()
                .map(File::from)
        })
        .collect()
}

/// Record that a target started in process group `pgid`, in every full-gate
/// lock this process holds. A no-op when it holds none (light-only quick work, or a
/// runner used outside `codeflow test`).
///
/// # Errors
///
/// When a holder record could not be rewritten; the target then runs
/// without its group recorded, which the caller reports.
pub fn target_group_started(pgid: u32) -> Result<(), String> {
    update_groups(|groups| {
        groups.insert(pgid);
    })
}

/// Record that the target in process group `pgid` finished.
///
/// # Errors
///
/// When a holder record could not be rewritten.
pub fn target_group_finished(pgid: u32) -> Result<(), String> {
    update_groups(|groups| {
        groups.remove(&pgid);
    })
}

fn update_groups(change: impl Fn(&mut BTreeSet<u32>)) -> Result<(), String> {
    let mut failures = Vec::new();
    let mut held = held();
    for lock in held.iter_mut() {
        change(&mut lock.groups);
        let mut text = lock.record.clone();
        for group in &lock.groups {
            // Writing to a String cannot fail.
            let _ = writeln!(text, "group={group}");
        }
        for path in &lock.paths {
            // A new record renamed into place. A missed write weakens the
            // killed-gate check, so it is reported, never ignored (TSK-1003).
            if let Err(error) = write_record(path, &text) {
                failures.push(format!(
                    "the gate lock record beside {} could not be updated with target process groups: {error}",
                    crate::git::display_path(path)
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Process groups named in a holder record that still have a live process.
fn live_groups(record: &str) -> Result<Vec<u32>, String> {
    let mut groups = Vec::new();
    for line in record.split_terminator('\n') {
        if let Some(value) = line.strip_prefix("group=") {
            let pgid = value
                .parse::<u32>()
                .map_err(|_| "a group value is not a target process group".to_string())?;
            if pgid <= 1 {
                return Err("a group value is not a target process group".to_string());
            }
            if group_alive(pgid) {
                groups.push(pgid);
            }
        }
    }
    Ok(groups)
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

#[derive(Debug)]
struct AcquiredFile(File);

impl Drop for AcquiredFile {
    fn drop(&mut self) {
        os_lock::unlock(&self.0);
    }
}

/// The OS lock behind a gate lock. On Unix it is `flock` on the whole file.
/// Windows locks are mandatory, and a locked range cannot be read through
/// another handle, so there the lock covers one byte far past any holder
/// record: every process can still read who holds the gate.
#[cfg(not(windows))]
mod os_lock {
    use fs2::FileExt;
    use std::fs::File;

    pub(super) fn try_lock(file: &File) -> std::io::Result<()> {
        file.try_lock_exclusive()
    }

    pub(super) fn unlock(file: &File) {
        let _ = FileExt::unlock(file);
    }

    pub(super) fn is_contended(error: &std::io::Error) -> bool {
        error.kind() == fs2::lock_contended_error().kind()
    }
}

#[cfg(windows)]
mod os_lock {
    use std::fs::File;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    /// The locked byte: far past any record, which stays a few hundred bytes.
    const LOCK_OFFSET: u64 = 1 << 40;

    fn at_lock_offset() -> OVERLAPPED {
        // SAFETY: OVERLAPPED is plain data; all zeroes is its documented
        // initial state.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.Anonymous.Anonymous.Offset = (LOCK_OFFSET & 0xffff_ffff) as u32;
        overlapped.Anonymous.Anonymous.OffsetHigh = (LOCK_OFFSET >> 32) as u32;
        overlapped
    }

    pub(super) fn try_lock(file: &File) -> std::io::Result<()> {
        let mut overlapped = at_lock_offset();
        // SAFETY: the handle is open for the call, and the OVERLAPPED lives
        // across it; a synchronous handle completes before returning.
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &raw mut overlapped,
            )
        };
        if locked == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn unlock(file: &File) {
        let mut overlapped = at_lock_offset();
        // SAFETY: as in `try_lock`; closing the handle also releases it.
        unsafe { UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &raw mut overlapped) };
    }

    pub(super) fn is_contended(error: &std::io::Error) -> bool {
        error.raw_os_error() == i32::try_from(ERROR_LOCK_VIOLATION).ok()
    }
}

/// The trusted folder a lock file is opened beneath, and the lock's path
/// relative to it: `<anchor>/<lock folder>/full-gate.lock`. The lock folder
/// may be writable by a sandboxed seat (`~/.codeflow/locks` for a Codex
/// builder, the common `.git` for any seat that commits), so it and the lock
/// file are opened without following links (TSK-1003).
fn lock_anchor(path: &Path) -> std::io::Result<(&Path, String)> {
    let unsupported = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "gate lock path has no parent folder or is not valid UTF-8",
        )
    };
    let dir = path.parent().ok_or_else(unsupported)?;
    let anchor = dir.parent().ok_or_else(unsupported)?;
    let folder = dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(unsupported)?;
    let leaf = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(unsupported)?;
    Ok((anchor, format!("{folder}/{leaf}")))
}

/// Open (creating if needed) the lock file at `path`, read only. The lock
/// folder and the file are refused when either is a link, and the file when
/// it is not a regular file, checked on the opened descriptor. Nothing is
/// ever written through this descriptor: it only carries the OS lock, so a
/// lock entry that names an outside file, by a hard link planted before or
/// during the open, lets a seat lock that file but never change it.
fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let (anchor, relative) = lock_anchor(path)?;
    std::fs::create_dir_all(anchor)?;
    let file = crate::contained::Tree::open(anchor)?.create_read_only(&relative)?;
    #[cfg(test)]
    interleave::at(interleave::Point::Opened);
    Ok(file)
}

/// Whether the lock path still names the file `file` holds open. A lock
/// file swapped for another regular file splits coordination: a later gate
/// would lock the new file. The lock folder is an advisory coordination
/// boundary, not a security boundary, so a swap after this check still
/// splits it; the check closes the window while the lock is taken. Both
/// identities are read from handles this module opened, the held one and the
/// anchored no-follow reopen, and an identity that cannot be read refuses.
fn same_lock_file(path: &Path, file: &File) -> std::io::Result<()> {
    let (anchor, relative) = lock_anchor(path)?;
    let current = crate::contained::Tree::open(anchor)?.open_regular(&relative)?;
    if FileIdentity::of(&current)? == FileIdentity::of(file)? {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "the lock path names another file than the one locked",
        ))
    }
}

/// The record path beside the lock file at `lock`.
fn record_path(lock: &Path) -> PathBuf {
    lock.with_file_name(RECORD_FILE)
}

/// The holder record for the lock at `lock`, parsed and rendered again from
/// validated values only ([`parse_record`]), so an outside file reached
/// through a planted link is never printed. The record file is read through an anchored read-only
/// open that refuses links. While it is absent or empty, the legacy record
/// a pre-3.2 gate keeps inside the lock file is read instead, so a gate of
/// an older binary on the same machine is still seen: through `held` when
/// the caller holds the lock file open, else through the same read-only
/// open. Empty when neither names a holder. Nothing is created.
fn read_holder(
    lock: &Path,
    held: Option<&mut File>,
    trusted: &[String],
) -> std::io::Result<String> {
    let unreadable = |error: String| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unreadable gate lock record: {error}"),
        )
    };
    let record = record_path(lock);
    if !crate::absence::proven_absent(&record)? {
        let (anchor, relative) = lock_anchor(&record)?;
        let text =
            read_record(&mut crate::contained::Tree::open(anchor)?.open_regular(&relative)?)?;
        let parsed = parse_record(&text, trusted).map_err(unreadable)?;
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }
    let legacy = match held {
        Some(file) => read_record(file)?,
        None if crate::absence::proven_absent(lock)? => String::new(),
        None => {
            let (anchor, relative) = lock_anchor(lock)?;
            read_record(&mut crate::contained::Tree::open(anchor)?.open_regular(&relative)?)?
        }
    };
    parse_record(&legacy, trusted).map_err(unreadable)
}

/// At most 4096 bytes of `file`, from its start.
fn read_record(file: &mut File) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(std::io::Error::other("gate lock record exceeds 4096 bytes"));
    }
    String::from_utf8(bytes).map_err(|_| std::io::Error::other("gate lock record is not UTF-8"))
}

/// The folders a holder record's `dir` may name and still be shown: the
/// project folder this gate runs for, its canonical form, and the main
/// checkout and linked worktrees git lists for its repository, as strings
/// with trailing separators removed. Only the project folder, which this
/// process was given, is resolved; the git-listed paths are taken as text
/// and never touched, since a seat that writes `.git` can change them.
fn trusted_dirs(project_dir: &Path) -> Vec<String> {
    let mut dirs = vec![project_dir.to_path_buf()];
    if let Ok(canonical) = std::fs::canonicalize(project_dir) {
        dirs.push(canonical);
    }
    if let Ok(repo) = git2::Repository::discover(project_dir) {
        if let Some(workdir) = repo.workdir() {
            dirs.push(workdir.to_path_buf());
        }
        let common = repo.commondir();
        if common.file_name().is_some_and(|name| name == ".git") {
            if let Some(main) = common.parent() {
                dirs.push(main.to_path_buf());
            }
        }
        if let Ok(names) = repo.worktrees() {
            for name in names.iter().filter_map(|name| name.ok().flatten()) {
                if let Ok(worktree) = repo.find_worktree(name) {
                    dirs.push(worktree.path().to_path_buf());
                }
            }
        }
    }
    let mut out: Vec<String> = dirs
        .iter()
        .map(|dir| trimmed_dir(&dir.display().to_string()).into_owned())
        .filter(|dir| !dir.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `dir` without trailing path separators, compared as text only. On
/// Windows, where git lists worktrees with `/` and paths display with `\\`,
/// both are compared as `/`.
fn trimmed_dir(dir: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    if cfg!(windows) {
        std::borrow::Cow::Owned(trimmed.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(trimmed)
    }
}

/// The modes a holder record names.
const LOCK_MODES: [&str; 4] = ["full", "quick", "essential", "reserve"];

/// Parse a holder record strictly and render it again from validated
/// values only: `pid` as a process number, `started` as a Unix time,
/// `mode` from [`LOCK_MODES`] (at most once each) and `group` lines as
/// process group numbers. `dir` is kept only when its text equals one of
/// `trusted` ([`trusted_dirs`]); otherwise it is left out and shown as an
/// unknown directory. The `dir` value is never resolved, opened or passed
/// to any filesystem call, so a planted value cannot reach a link, a
/// network share or an outside folder (TSK-1003, SEC-4). Any other line
/// refuses the whole record. Errors are fixed texts that never include the
/// record's text, so a file reached through a planted link reveals nothing
/// beyond a folder the gate already trusts.
fn parse_record(text: &str, trusted: &[String]) -> Result<String, String> {
    let mut seen = BTreeSet::new();
    let mut out = String::new();
    for line in text.split_terminator('\n') {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "a line is not key=value".to_string())?;
        let shown = match key {
            "pid" => value
                .parse::<u32>()
                .map(|pid| pid.to_string())
                .map_err(|_| "a pid value is not a process number".to_string())?,
            "started" => value
                .parse::<u64>()
                .map(|started| started.to_string())
                .map_err(|_| "a started value is not a Unix time".to_string())?,
            "mode" => LOCK_MODES
                .iter()
                .find(|mode| **mode == value)
                .map(ToString::to_string)
                .ok_or_else(|| "a mode value is not a known mode".to_string())?,
            "group" => match value.parse::<u32>() {
                Ok(group) if group > 1 => group.to_string(),
                _ => return Err("a group value is not a target process group".to_string()),
            },
            "dir" => {
                if value.is_empty() || value.chars().any(char::is_control) {
                    return Err("a dir value is empty or holds a control character".to_string());
                }
                let value = trimmed_dir(value);
                if let Some(dir) = trusted.iter().find(|dir| dir.as_str() == value.as_ref()) {
                    dir.clone()
                } else {
                    // Not a folder the gate trusts: shown as unknown, never
                    // as the record's text, and never looked up.
                    if !seen.insert("dir") {
                        return Err("a repeated dir".to_string());
                    }
                    continue;
                }
            }
            _ => return Err("an unknown key".to_string()),
        };
        if key != "group" && !seen.insert(key) {
            return Err(format!("a repeated {key}"));
        }
        // Writing to a String cannot fail.
        let _ = writeln!(out, "{key}={shown}");
    }
    Ok(out)
}

fn holder_record(project_dir: &Path, mode: &str) -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "pid={}\nstarted={started}\ndir={}\nmode={mode}\n",
        std::process::id(),
        project_dir.display()
    )
}

/// Replace the record beside the lock at `lock` with `text`: a new file
/// created exclusively under a random name in the lock folder, written
/// through its own descriptor, then renamed over the record name. An
/// existing record is never opened for writing; a link at the record name
/// is refused, never followed, and a hard link there is replaced by the
/// rename.
fn write_record(lock: &Path, text: &str) -> std::io::Result<()> {
    let record = record_path(lock);
    let (anchor, relative) = lock_anchor(&record)?;
    let tree = crate::contained::Tree::open(anchor)?;
    // On Windows a rename over a record another process is reading fails
    // with a sharing violation until that brief read ends; retry a few
    // times before reporting it.
    let mut attempts = 0;
    loop {
        match tree.write(&relative, text.as_bytes()) {
            Err(error) if cfg!(windows) && attempts < 20 && is_sharing_violation(&error) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

/// Windows `ERROR_SHARING_VIOLATION` (32) or `ERROR_ACCESS_DENIED` (5),
/// which a rename over an open file reports.
fn is_sharing_violation(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32))
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
    let dir = field("dir").map_or_else(
        || " in an unknown directory".to_string(),
        |d| format!(" in {}", crate::git::display_line(d)),
    );
    let age = field("started")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|started| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            format!(", started {} s ago", now.saturating_sub(started))
        })
        .unwrap_or_default();
    format!(
        "pid {pid}{dir}{age}, {} mode",
        field("mode").unwrap_or("full")
    )
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
        crate::git::display_path(&target),
        crate::git::display_path(&root),
        crate::git::display_path(&root.join("target"))
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
        // The holder's folder is shown only when it exists.
        let work = std::fs::canonicalize(tmp.path()).unwrap().join("work-a");
        std::fs::create_dir(&work).unwrap();
        let first = acquire_full_gate_lock(&dirs(tmp.path()), &work).unwrap();
        assert!(first.notes.is_empty(), "{:?}", first.notes);

        // A second gate for the same project sees the holder's folder, which
        // it trusts; any other folder shows as unknown.
        let held = acquire_full_gate_lock(&dirs(tmp.path()), &work).unwrap_err();
        let message = held.to_string();
        assert!(
            message.contains(&format!("pid {}", std::process::id())),
            "{message}"
        );
        assert!(
            message.contains(&format!("in {}", trimmed_dir(&work.display().to_string()))),
            "{message}"
        );
        assert!(message.contains("one heavy gate"), "{message}");

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
                    && n.contains("pid 999999 in an unknown directory")),
            "{:?}",
            lock.notes
        );
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
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
        assert!(
            message.contains("pid 999999 in an unknown directory"),
            "{message}"
        );
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
        target_group_started(4_000_001).unwrap();
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
        assert!(record.contains("group=4000001"), "{record}");
        assert!(record.contains(&format!("pid={}", std::process::id())));
        target_group_finished(4_000_001).unwrap();
        let record = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap();
        assert!(!record.contains("group=4000001"), "{record}");
        drop(lock);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap(), "");
        // The lock file itself is never written (TSK-1003).
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
        os_lock::try_lock(&file).expect("partial acquisition released its OS lock");
    }

    #[test]
    fn the_builder_write_probe_covers_locks_objects_and_refs() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let found = builder_write_probe(tmp.path(), Some(&home));
        let dirs: Vec<&PathBuf> = found.iter().map(|(dir, _)| dir).collect();
        assert_eq!(dirs.len(), 4, "{dirs:?}");
        assert_eq!(dirs[0], &home.join("locks"));
        assert!(dirs[1].ends_with(".git/codeflow"), "{dirs:?}");
        assert!(dirs[2].ends_with(".git/objects"), "{dirs:?}");
        assert!(dirs[3].ends_with(".git/refs"), "{dirs:?}");
        assert!(found.iter().all(|(_, result)| result.is_ok()), "{found:?}");
        for dir in &dirs[2..] {
            let left: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("write-probe"))
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }
        let without_home = builder_write_probe(tmp.path(), None);
        assert_eq!(without_home.len(), 4);
        assert!(without_home[0].1.is_err());
        let plain = tempfile::tempdir().unwrap();
        let outside = builder_write_probe(plain.path(), Some(&home));
        assert!(
            outside.iter().any(|(_, result)| result.is_err()),
            "{outside:?}"
        );
    }

    #[test]
    fn lock_dirs_cover_home_and_the_git_common_dir() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let found = lock_dirs(tmp.path(), Some(&home)).unwrap();
        assert_eq!(found[0], home.join("locks"));
        assert!(found[1].ends_with(".git/codeflow"), "{found:?}");
    }

    /// TSK-216 AC-3: doctor's probe names each gate directory this process
    /// cannot write. Where it can write, it creates the directories and the
    /// empty lock files a gate would, and keeps them: a lock file is shared
    /// with any running gate and is never unlinked. Only its evidence probe
    /// file is removed.
    #[test]
    fn unwritable_gate_dirs_names_each_directory_the_gate_cannot_write() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        assert!(unwritable_gate_dirs(tmp.path(), Some(&home)).is_empty());
        let evidence: Vec<_> = std::fs::read_dir(home.join(HOME_EVIDENCE_DIR))
            .unwrap()
            .collect();
        assert!(evidence.is_empty(), "the probe file is removed");
        let lock = home.join(HOME_LOCK_DIR).join(LOCK_FILE);
        assert_eq!(
            std::fs::metadata(&lock).map(|m| m.len()).ok(),
            Some(0),
            "{lock:?}"
        );

        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        let found: Vec<PathBuf> = unwritable_gate_dirs(tmp.path(), Some(&blocker))
            .into_iter()
            .map(|(dir, _)| dir)
            .collect();
        assert_eq!(
            found,
            [blocker.join(HOME_LOCK_DIR), blocker.join(HOME_EVIDENCE_DIR)]
        );
    }

    /// TSK-1003: a sandboxed seat that may write a lock folder must not be
    /// able to redirect a later, less restricted gate's writes. A lock file
    /// planted as a link is refused and its target is left unchanged.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_planted_as_a_link_is_refused_and_its_target_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let target = tmp.path().join("sentinel");
        std::fs::write(&target, "keep me\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(LOCK_FILE)).unwrap();
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a linked lock file is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
        assert!(read_holder(&dir.join(LOCK_FILE), None, &[]).is_err());
    }

    /// TSK-1003: a lock folder replaced by a link is refused as well.
    #[cfg(unix)]
    #[test]
    fn a_lock_folder_planted_as_a_link_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = elsewhere.join(LOCK_FILE);
        std::fs::write(&target, "keep me\n").unwrap();
        let dir = tmp.path().join("locks");
        std::os::unix::fs::symlink(&elsewhere, &dir).unwrap();
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a linked lock folder is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    }

    /// TSK-1003: a special file in place of the lock is refused, without
    /// blocking on it.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_that_is_a_fifo_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo =
            std::ffi::CString::new(dir.join(LOCK_FILE).as_os_str().as_encoded_bytes().to_vec())
                .unwrap();
        // SAFETY: a NUL-terminated path and a plain mode.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("a FIFO lock is refused");
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
    }

    /// TSK-1003: once the lock is held, its record is updated through the
    /// acquired descriptor. A path swapped for a link afterwards is never
    /// written through.
    #[cfg(unix)]
    #[test]
    fn a_lock_path_swapped_for_a_link_after_acquisition_is_not_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
        let target = tmp.path().join("sentinel");
        std::fs::write(&target, "keep me\n").unwrap();
        std::fs::remove_file(dir.join(LOCK_FILE)).unwrap();
        std::os::unix::fs::symlink(&target, dir.join(LOCK_FILE)).unwrap();
        target_group_started(4_000_003).unwrap();
        target_group_finished(4_000_003).unwrap();
        drop(lock);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me\n");
    }

    /// TSK-1003 round three: a lock file planted as a hard link to an outside
    /// file is refused at both lock locations, by the gate and by the status
    /// read, and the outside file is neither read nor changed.
    #[cfg(unix)]
    #[test]
    fn a_lock_file_planted_as_a_hard_link_is_never_written_or_printed() {
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let target = tmp.path().join(format!("sentinel-{index}"));
            std::fs::write(&target, "pid=1\nSECRET keep me\n").unwrap();
            std::fs::hard_link(&target, dir.join(LOCK_FILE)).unwrap();
            // Its text is no holder record, so the gate refuses; it says so
            // without printing the text.
            let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                .expect_err("an unreadable legacy record is refused");
            assert!(
                error.to_string().contains("unreadable gate lock record")
                    && !error.to_string().contains("SECRET"),
                "{error}"
            );
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(status.is_err(), "{status:?}");
            assert!(!format!("{status:?}").contains("SECRET"), "{status:?}");
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "pid=1\nSECRET keep me\n"
            );
        }
    }

    /// TSK-1003 round three: a lock file swapped for another regular file
    /// while it is being taken is not trusted: the path and the locked
    /// descriptor must name the same file.
    #[cfg(unix)]
    #[test]
    fn a_lock_whose_path_names_another_file_after_locking_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locks");
        let path = dir.join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "").unwrap();
        assert!(same_lock_file(&path, &file).is_err());
        let fresh = open_lock_file(&path).unwrap();
        assert!(same_lock_file(&path, &fresh).is_ok());
    }

    /// TSK-1021 AC-1: the check compares the identity of the held handle
    /// with the anchored reopen on every platform: the lock path's own file
    /// passes, a handle on another file is refused.
    #[test]
    fn a_lock_check_compares_file_identities() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("locks").join(LOCK_FILE);
        let other = tmp.path().join("other").join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        let elsewhere = open_lock_file(&other).unwrap();
        assert!(same_lock_file(&path, &file).is_ok());
        let error = same_lock_file(&path, &elsewhere).expect_err("another file");
        assert!(error.to_string().contains("another file"), "{error}");
    }

    /// TSK-1003 round four, the review's interleaving: the lock entry is a
    /// hard link to an outside file when it is opened, the entry is removed
    /// before any check (the outside file now has one name), and the entry
    /// is linked to the outside file again once the lock is taken. Nothing
    /// in the outside file may change, at either lock location.
    #[cfg(unix)]
    #[test]
    fn unlink_and_relink_around_the_checks_never_writes_the_outside_file() {
        use std::rc::Rc;
        let tmp = tempfile::tempdir().unwrap();
        // The first text is no record, so acquisition refuses; the second
        // parses as a stale record, so acquisition succeeds and the group
        // update and the drop run under the same interleaving.
        let cases = [
            ("SECRET-OUTSIDE\n", false),
            ("pid=999999\nstarted=1\n", true),
        ];
        let locations = ["home/locks", "repo/.git/codeflow"];
        for (index, (location, (text, acquires))) in locations
            .iter()
            .flat_map(|location| cases.iter().map(move |case| (*location, *case)))
            .enumerate()
        {
            let dir = tmp.path().join(format!("case-{index}")).join(location);
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = Rc::new(tmp.path().join(format!("outside-{index}")));
            std::fs::write(&*sentinel, text).unwrap();
            let entry = Rc::new(dir.join(LOCK_FILE));
            std::fs::hard_link(&*sentinel, &*entry).unwrap();
            let (s, e) = (Rc::clone(&sentinel), Rc::clone(&entry));
            interleave::set(move |point| match point {
                interleave::Point::Opened => std::fs::remove_file(&*e).unwrap(),
                interleave::Point::Locked => std::fs::hard_link(&*s, &*e).unwrap(),
            });
            let result = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"));
            interleave::clear();
            assert_eq!(result.is_ok(), acquires, "{dir:?}: {result:?}");
            if let Ok(lock) = result {
                target_group_started(4_000_005).unwrap();
                target_group_finished(4_000_005).unwrap();
                drop(lock);
            }
            assert_eq!(
                std::fs::read_to_string(&*sentinel).unwrap(),
                text,
                "outside file changed at {dir:?}"
            );
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(
                !status.as_deref().unwrap_or_default().contains("SECRET"),
                "{status:?}"
            );
        }
    }

    /// TSK-1003 round four: a holder record planted as a hard link to an
    /// outside file is replaced by rename, never written through, and the
    /// status read never prints the outside file's text.
    #[cfg(unix)]
    #[test]
    fn a_planted_record_link_is_replaced_never_written_or_printed() {
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = tmp.path().join(format!("record-outside-{index}"));
            std::fs::write(&sentinel, "SECRET-OUTSIDE\n").unwrap();
            std::fs::hard_link(&sentinel, dir.join(RECORD_FILE)).unwrap();
            let status = holder_status(&dir.join(LOCK_FILE), &[]);
            assert!(
                !status.as_deref().unwrap_or_default().contains("SECRET"),
                "{status:?}"
            );
            let error = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                .expect_err("an unreadable record is refused");
            assert!(!error.to_string().contains("SECRET"), "{error}");
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n"
            );
            // A planted record that parses is reclaimed, and the new record
            // replaces the planted name by rename: the outside file keeps
            // its text.
            let stale = tmp.path().join(format!("stale-outside-{index}"));
            std::fs::write(&stale, "pid=999999\n").unwrap();
            std::fs::remove_file(dir.join(RECORD_FILE)).unwrap();
            std::fs::hard_link(&stale, dir.join(RECORD_FILE)).unwrap();
            let lock = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")).unwrap();
            target_group_started(4_000_007).unwrap();
            target_group_finished(4_000_007).unwrap();
            drop(lock);
            assert_eq!(std::fs::read_to_string(&stale).unwrap(), "pid=999999\n");
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n"
            );
        }
    }

    /// TSK-1003 round four (SEC-1): an attacker thread that keeps removing
    /// the lock entry and linking it to an outside file again, while gates
    /// take and release the lock and record groups, never changes the
    /// outside file, at either lock location.
    #[cfg(unix)]
    #[test]
    fn a_racing_relinker_never_changes_the_outside_file() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let tmp = tempfile::tempdir().unwrap();
        for (index, dir) in [
            tmp.path().join("home").join(HOME_LOCK_DIR),
            tmp.path().join("repo/.git/codeflow"),
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::create_dir_all(&dir).unwrap();
            let sentinel = tmp.path().join(format!("raced-{index}"));
            std::fs::write(&sentinel, "SECRET-OUTSIDE\n").unwrap();
            let entry = dir.join(LOCK_FILE);
            std::fs::hard_link(&sentinel, &entry).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let attacker = {
                let (stop, sentinel, entry) = (Arc::clone(&stop), sentinel.clone(), entry.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = std::fs::remove_file(&entry);
                        let _ = std::fs::hard_link(&sentinel, &entry);
                    }
                })
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(lock) =
                    acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
                {
                    target_group_started(4_000_009).unwrap();
                    target_group_finished(4_000_009).unwrap();
                    drop(lock);
                }
                if std::fs::read_to_string(&sentinel).unwrap() != "SECRET-OUTSIDE\n" {
                    break;
                }
            }
            stop.store(true, Ordering::Relaxed);
            attacker.join().unwrap();
            assert_eq!(
                std::fs::read_to_string(&sentinel).unwrap(),
                "SECRET-OUTSIDE\n",
                "outside file changed at {dir:?}"
            );
        }
    }

    /// TSK-1003: a gate of a pre-3.2 binary keeps its record inside the lock
    /// file. While the record file names no holder, that legacy record is
    /// still read, so its killed gate's live targets keep the lock held.
    #[cfg(unix)]
    #[test]
    fn a_legacy_record_is_read_while_the_record_file_is_empty() {
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
        std::fs::write(dir.join(RECORD_FILE), "").unwrap();
        std::fs::write(
            dir.join(LOCK_FILE),
            format!("pid=999999\nstarted=1\ndir=/old\ngroup={group}\n"),
        )
        .unwrap();
        let held = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"))
            .expect_err("the legacy record's live group keeps the lock");
        assert_eq!(held.running_groups, vec![group]);
        target.kill().unwrap();
        target.wait().unwrap();
    }

    /// TSK-1003 round five (SEC-3): text planted in either lock file name,
    /// at either lock location, never reaches any output: not a refusal,
    /// not a note and not the status line. A `group` value that is not a
    /// process group refuses with a fixed message; a `dir` that is not an
    /// existing directory is shown as unknown.
    #[test]
    fn planted_record_text_never_reaches_any_output() {
        let tmp = tempfile::tempdir().unwrap();
        // The review's case: a link to a folder whose name holds record
        // lines, which a resolved `dir` would have injected (SEC-4).
        #[cfg(unix)]
        let injected = {
            let base = std::fs::canonicalize(tmp.path()).unwrap();
            let target = base.join("x\npid=SECRET_PID\nmode=SECRET_MODE");
            std::fs::create_dir(&target).unwrap();
            let link = base.join("injecting-link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            format!("pid=999999\ndir={}\n", link.display())
        };
        #[cfg(not(unix))]
        let injected = "pid=999999\ndir=/SECRET_PID\n".to_string();
        let mut case = 0;
        for location in ["home/locks", "repo/.git/codeflow"] {
            for name in [LOCK_FILE, RECORD_FILE] {
                for planted in [
                    "pid=999999\ngroup=SECRET\n",
                    "pid=999999\ndir=/SECRET-dir\n",
                    "pid=999999\nmode=SECRET\n",
                    injected.as_str(),
                ] {
                    case += 1;
                    let dir = tmp.path().join(format!("case-{case}")).join(location);
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(name), planted).unwrap();
                    let mut output = String::new();
                    let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                    match acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")) {
                        Ok(lock) => {
                            output.push_str(&lock.notes.join("\n"));
                            drop(lock);
                        }
                        Err(held) => {
                            let _ = write!(output, "{held}\n{held:?}");
                        }
                    }
                    let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                    assert!(
                        !output.contains("SECRET"),
                        "{location}/{name} {planted:?}:\n{output}"
                    );
                }
            }
        }
    }

    /// TSK-1003 round six (SEC-4, SEC-5): a planted `dir` value is never
    /// resolved. Only a value equal to a folder the gate already trusts is
    /// shown; a link to a folder whose name holds a newline cannot inject
    /// record lines, and an outside or relative existing folder, or a UNC
    /// path, shows as an unknown directory.
    #[cfg(unix)]
    #[test]
    fn a_planted_dir_value_is_never_resolved_or_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let injected = base.join("x\nmode=PLANTED");
        std::fs::create_dir(&injected).unwrap();
        let link = base.join("link");
        std::os::unix::fs::symlink(&injected, &link).unwrap();
        let outside = base.join("outside-existing");
        std::fs::create_dir(&outside).unwrap();
        let mut case = 0;
        for (value, secret) in [
            (link.display().to_string(), "PLANTED"),
            (outside.display().to_string(), "outside-existing"),
            ("src".to_string(), "codeflow-core"),
            ("\\\\host\\share".to_string(), "host"),
        ] {
            for name in [LOCK_FILE, RECORD_FILE] {
                case += 1;
                let dir = base.join(format!("case-{case}")).join("locks");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(name), format!("pid=999999\ndir={value}\n")).unwrap();
                let mut output = String::new();
                let _ = writeln!(output, "{:?}", holder_status(&dir.join(LOCK_FILE), &[]));
                match acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w")) {
                    Ok(lock) => {
                        output.push_str(&lock.notes.join("\n"));
                        drop(lock);
                    }
                    Err(held) => {
                        let _ = write!(output, "{held}\n{held:?}");
                    }
                }
                assert!(
                    !output.contains(secret) && output.contains("unknown directory"),
                    "{name} dir={value:?}:\n{output}"
                );
            }
        }
    }

    /// TSK-1003: with an existing lock file in a folder the process cannot
    /// write, the lock opens read only but the holder record cannot be
    /// written, so the gate refuses instead of running uncoordinated, and
    /// the permissions probe names the folder.
    #[cfg(unix)]
    #[test]
    fn a_read_only_lock_folder_refuses_and_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let home = tmp.path().join("home");
        let dir = home.join(HOME_LOCK_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOCK_FILE), "").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let refused = acquire_full_gate_lock(std::slice::from_ref(&dir), Path::new("/w"));
        let reported = unwritable_gate_dirs(tmp.path(), Some(&home));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = refused.expect_err("a gate that cannot write its record refuses");
        assert!(
            error
                .to_string()
                .contains("holder record cannot be written"),
            "{error}"
        );
        assert!(
            reported.iter().any(|(found, _)| *found == dir),
            "{reported:?}"
        );
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

#[cfg(test)]
mod r22_regressions {
    use super::*;

    #[test]
    fn r22_lock_dirs_refuse_corrupt_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(lock_dirs(dir.path(), None).unwrap().is_empty());
        std::fs::write(dir.path().join(".git"), "broken gitdir").unwrap();
        assert!(lock_dirs(dir.path(), None).is_err());
    }
    #[test]
    fn r22_invalid_group_record_cannot_be_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        for group in ["oops", "4294967296", "0", "1"] {
            let path = dir.path().join(LOCK_FILE);
            std::fs::write(&path, format!("pid=999999\ngroup={group}\n")).unwrap();
            let result = acquire_full_gate_lock(&[dir.path().to_path_buf()], dir.path());
            assert!(result.is_err(), "{group}");
            assert!(std::fs::read_to_string(path).unwrap().contains(group));
        }
        std::fs::write(dir.path().join(LOCK_FILE), "pid=999999\n").unwrap();
        assert!(acquire_full_gate_lock(&[dir.path().to_path_buf()], dir.path()).is_ok());
    }
}

#[cfg(test)]
mod heavy_admission_tests {
    use super::*;

    #[test]
    fn essential_record_adds_only_mode_and_legacy_reader_keeps_holder_fields() {
        let record = holder_record(Path::new("/work/essential"), "essential");
        let keys: BTreeSet<_> = record
            .lines()
            .map(|line| line.split_once('=').unwrap().0)
            .collect();
        assert_eq!(keys, BTreeSet::from(["pid", "started", "dir", "mode"]));
        // 3.1 looks up known key prefixes and ignores additional keys.
        let old_field = |key: &str| {
            record
                .lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .map(str::trim)
        };
        assert_eq!(
            old_field("pid"),
            Some(std::process::id().to_string().as_str())
        );
        assert_eq!(old_field("dir"), Some("/work/essential"));
        assert!(old_field("started").unwrap().parse::<u64>().is_ok());
        assert!(describe_holder(&record).contains("essential mode"));
        assert!(describe_holder("pid=9\ndir=/old\n").contains("full mode"));
    }

    #[cfg(unix)]
    #[test]
    fn refusal_unlocks_even_when_a_fork_duplicate_keeps_the_descriptor_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        let file = open_lock_file(&path).unwrap();
        os_lock::try_lock(&file).unwrap();
        // dup and fork share the open file description behind flock.
        let inherited = file.try_clone().unwrap();
        let refused = AcquiredFile(file);
        drop(refused);
        let next = open_lock_file(&path).unwrap();
        os_lock::try_lock(&next).expect("explicit unlock also releases inherited descriptors");
        os_lock::unlock(&next);
        drop(inherited);
    }
}
